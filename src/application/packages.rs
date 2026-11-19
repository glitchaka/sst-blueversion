mod installer;
mod sstpkg;
mod execution;
mod lifecycle;
mod model;
mod store;
mod transport;

use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::Read,
    path::{Component, PathBuf},
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::adapters::persistence::AppPaths;

pub use model::{InstallReport, InstalledPackage, PackageManifest, RepoSource};
pub use execution::ScriptRunner;

use installer::{command_name_from_exposed, find_file, install_payload};
use lifecycle::{functional_generation, expires_on_generation};
use model::{
    InstalledCommand, PackageIndex, validate_identifier, validate_index, validate_https,
};
use store::PackageStore;
use transport::{ResolvedSource, download, get, resolve_source};

#[cfg(windows)]
fn decode_wsl_output(bytes: &[u8]) -> String {
    // WSL on Windows can return UTF-16LE even to a redirected pipe.
    let utf16 = bytes.len() % 2 == 0
        && bytes.chunks_exact(2).take(32).any(|pair| pair[1] == 0);
    if utf16 {
        let words = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>();
        String::from_utf16_lossy(&words).trim_start_matches('\u{feff}').to_owned()
    } else {
        String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_owned()
    }
}

const BUILTIN_INDEX: &str = include_str!("../../packages/index.json");

#[derive(Clone)]
pub struct PackageService {
    store: PackageStore,
}

impl PackageService {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            store: PackageStore::new(paths),
        }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        self.store.ensure_layout()
    }

    pub fn activate_process_environment(&self) -> Result<()> {
        self.reconcile_for_functional_release()?;
        let mut entries = Vec::new();
        // Do not expose installed third-party executables in SST's PATH.
        // Embedded modules are dispatched by their registered ABI instead.

        let inherited = env::var_os("PATH").unwrap_or_default();
        entries.extend(env::split_paths(&inherited));
        dedup_paths(&mut entries);
        let joined = env::join_paths(entries).context("no se pudo construir PATH portable de instp")?;

        // SAFETY: build_engine invokes this before NativeShellEngine creates the
        // Bash environment and before the interactive UI starts worker threads.
        // Only SST's process environment changes; Windows' user/system PATH is
        // never modified.
        unsafe {
            env::set_var("PATH", joined);
            env::set_var("SST_PACKAGES", self.store.packages_dir());

        }
        Ok(())
    }

    /// Before building the shell's PATH, remove all non-persistent packages
    /// belonging to an earlier functional generation. This removes their
    /// extracted payloads AND their downloaded package archives.
    pub fn reconcile_for_functional_release(&self) -> Result<Vec<String>> {
        self.ensure_layout()?;
        let mut state = self.store.load_installed_state()?;
        let mut migrated = false;
        // Older installed.json files do not contain lifetime metadata.
        // Migrate them to this generation rather than destructively deleting
        // everything on the first startup with the new manager.
        for package in &mut state.packages {
            if package.functional_generation.is_empty() {
                package.functional_generation = functional_generation().to_owned();
                migrated = true;
            }
            // Preserve previously installed PowerShell modules when migrating
            // from the flat legacy packages/modules directory to win/modules.
            if package.id.eq_ignore_ascii_case("powershell")
                && package.execution.as_ref().is_some_and(|e| e.kind == "embedded")
            {
                let previous = self.store.resolve_root_relative(&package.install_dir);
                let legacy = self.store.packages_dir().join("modules").join("powershell");
                let destination = self.store.modules_dir().join("powershell");
                if previous == legacy && previous.is_dir() && !destination.exists() {
                    // The .NET apphost is not used: SST loads Bridge.dll in-process.
                    let apphost = previous.join("Sst.PowerShell.Bridge.exe");
                    if apphost.is_file() { fs::remove_file(apphost)?; }
                    fs::rename(&previous, &destination)
                        .context("instp: no se pudo migrar el módulo PowerShell")?;
                    package.install_dir = self.store.root_relative(&destination);
                    migrated = true;
                }
            }
        }
        if migrated {
            // Old versions left archives in the cache indefinitely.
            // They are never necessary for packages that are already installed.
            self.store.clean_cache()?;
        }
        let expired = state.packages.iter()
            .filter(|package| expires_on_generation(package, functional_generation()))
            .cloned()
            .collect::<Vec<_>>();
        for package in &expired {
            let target = self.store.resolve_root_relative(&package.install_dir);
            anyhow::ensure!(
                !target.components().any(|component| matches!(component, Component::ParentDir))
                    && (target.starts_with(self.store.tools_dir())
                    || target.starts_with(self.store.runtimes_dir())
                    || target.starts_with(self.store.modules_dir())
                    || target.starts_with(self.store.environments_dir())
                    || target.starts_with(self.store.packages_dir().join("tools"))
                    || target.starts_with(self.store.packages_dir().join("runtimes"))
                    || target.starts_with(self.store.packages_dir().join("modules"))
                    || target.starts_with(self.store.packages_dir().join("environments"))),
                "instp: ruta de paquete fuera de su área aislada: {}",
                target.display()
            );
            if target.exists() {
                fs::remove_dir_all(&target)
                    .with_context(|| format!("no se pudo limpiar {}", package.id))?;
            }
            self.store.clean_package_cache(&package.id)?;
        }
        if migrated || !expired.is_empty() {
            state.packages.retain(|package| {
                !expires_on_generation(package, functional_generation())
            });
            self.store.save_installed_state(&state)?;
        }
        Ok(expired.into_iter().map(|package| package.id).collect())
    }

    pub fn search(&self, term: Option<&str>) -> Result<Vec<PackageManifest>> {
        let term = term.unwrap_or_default().trim().to_ascii_lowercase();
        let mut packages = self.catalog()?;
        // Metasploit permanece definido en el repositorio, pero se excluye
        // temporalmente del listado y de las búsquedas visibles de instp.
        packages.retain(|package| !package.id.eq_ignore_ascii_case("metasploit"));
        if !term.is_empty() {
            packages.retain(|package| {
                package.id.to_ascii_lowercase().contains(&term)
                    || package.name.to_ascii_lowercase().contains(&term)
                    || package.description.to_ascii_lowercase().contains(&term)
                    || package.category.to_ascii_lowercase().contains(&term)
            });
        }
        packages.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(packages)
    }

    pub fn info(&self, id: &str) -> Result<Option<PackageManifest>> {
        Ok(self
            .catalog()?
            .into_iter()
            .find(|package| package.id.eq_ignore_ascii_case(id)))
    }

    pub fn installed(&self) -> Result<Vec<InstalledPackage>> {
        self.ensure_layout()?;
        let mut packages = self.store.load_installed_state()?.packages;
        packages.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(packages)
    }
    /// Resolve a genuinely installed embeddable module, never an empty legacy
    /// "builtin" marker. The manifest is consulted only at install time;
    /// runtime activation follows the validated installed-state journal.
    pub fn module_directory(&self, id: &str) -> Result<Option<PathBuf>> {
        let Some(package) = self.installed()?.into_iter()
            .find(|item| item.id.eq_ignore_ascii_case(id)) else {
            return Ok(None);
        };
        let Some(execution) = package.execution else { return Ok(None); };
        if execution.kind != "embedded" || execution.abi.as_deref() != Some("sst-hostfxr/1") {
            return Ok(None);
        }
        let root = self.store.modules_dir().canonicalize()?;
        let path = self.store.resolve_root_relative(&package.install_dir).canonicalize()?;
        anyhow::ensure!(path.starts_with(&root), "instp: módulo fuera de su directorio autorizado");
        anyhow::ensure!(path.join(&execution.entry).is_file(),
            "instp: módulo {} incompleto, reinstálalo", package.id);
        for required in &execution.required_files {
            anyhow::ensure!(path.join(required).is_file(),
                "instp: módulo {} incompleto: falta {}", package.id, required);
        }
        Ok(Some(path))
    }


    pub fn install(&self, id: &str) -> Result<InstallReport> {
        self.install_with_persistence(id, false)
    }

    pub fn install_with_persistence(&self, id: &str, persistent: bool) -> Result<InstallReport> {
        self.reconcile_for_functional_release()?;
        validate_identifier(id)?;
        let package = self
            .info(id)?
            .ok_or_else(|| anyhow::anyhow!("instp: paquete no encontrado: {id}"))?;

        if package.install.kind != "sstpkg"
            && !package.execution.as_ref().is_some_and(|e| e.kind == "environment")
        {
            anyhow::bail!(
                "instp: {} se distribuye como ejecutable externo o archivo de proveedor; sólo se admiten módulos .sstpkg sin nuevos .exe",
                id
            );
        }
        if package.execution.as_ref().is_some_and(|e| e.kind == "environment") {
            return self.install_wsl_environment(&package, persistent);
        }

        if matches!(package.portability.as_str(), "system" | "driver")
            || package.install.kind == "system"
        {
            anyhow::bail!(
                "instp: {} está marcado como {} y modifica Windows; el instalador portable no lo ejecuta automáticamente",
                package.id,
                package.portability.to_ascii_uppercase()
            );
        }

        let resolved = resolve_source(&package)?;
        let archive = self.obtain_source(&package, &resolved)?;
        let execution = package.execution.as_ref()
            .context("instp: falta adaptador de ejecución")?;
        let target = execution.target.as_deref().unwrap_or("win");
        anyhow::ensure!(matches!(target, "win" | "unix"),
            "instp: plataforma de paquete desconocida");
        let folder = sstpkg::kind_folder(&execution.kind)?;
        let install_dir = self.store.packages_dir()
            .join(target).join(folder).join(&package.id);

        let staging = install_dir.with_extension("instp-new");
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        fs::create_dir_all(&staging)?;
        if let Err(error) = install_payload(&package, &archive, &staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }

        // Check the staged payload before replacing a working installation.
        // A manifest promising commands that are absent must fail atomically.
        let mut staged_missing = Vec::new();
        for exposed in &package.install.expose {
            if find_file(&staging, exposed)?.is_none() {
                staged_missing.push(exposed.clone());
            }
        }
        if !staged_missing.is_empty() {
            let _ = fs::remove_dir_all(&staging);
            anyhow::bail!(
                "instp: {} no contiene los ejecutables declarados: {}. Se conserva la instalación anterior.",
                package.id,
                staged_missing.join(", ")
            );
        }
        if let Some(execution) = &package.execution {
            if matches!(execution.kind.as_str(), "embedded" | "runtime" | "process")
                && find_file(&staging, &execution.entry)?.is_none()
            {
                let _ = fs::remove_dir_all(&staging);
                anyhow::bail!("instp: {} no contiene su entrada {}: se conserva versión anterior",
                    package.id, execution.entry);
            }
            for required in &execution.required_files {
                if find_file(&staging, required)?.is_none() {
                    let _ = fs::remove_dir_all(&staging);
                    anyhow::bail!("instp: {} no contiene el archivo obligatorio {}", package.id, required);
                }
            }
        }

        let backup = install_dir.with_extension("instp-old");
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        if install_dir.exists() {
            fs::rename(&install_dir, &backup)
                .with_context(|| format!("no se pudo preparar actualización de {}", package.id))?;
        }
        if let Err(error) = fs::rename(&staging, &install_dir) {
            if backup.exists() {
                let _ = fs::rename(&backup, &install_dir);
            }
            return Err(error).with_context(|| format!("no se pudo activar {}", package.id));
        }
        // Keep the previous installation until the installed-state journal
        // has been durably updated. A failed journal write must roll back.

        let mut commands = Vec::new();
        let mut path_entries = Vec::new();
        let mut missing = Vec::new();
        for exposed in &package.install.expose {
            match find_file(&install_dir, exposed)? {
                Some(target) => {
                    commands.push(InstalledCommand {
                        name: command_name_from_exposed(exposed),
                        target: self.store.root_relative(&target),
                    });
                    if let Some(parent) = target.parent() {
                        path_entries.push(self.store.root_relative(parent));
                    }
                }
                None => missing.push(exposed.clone()),
            }
        }
        // SST plugins are loaded internally, never exported to the OS PATH.
        // Runtime and CLI programs from upstream are not installed as EXEs.
        path_entries.sort();
        path_entries.dedup();

        let mut state = self.store.load_installed_state()?;
        // Reinstalling/upgrading without -p must never silently remove an
        // earlier explicit choice to make the package persistent.
        let effective_persistent = persistent || state.packages.iter().any(|existing| {
            existing.id.eq_ignore_ascii_case(&package.id) && existing.persistent
        });
        let installed = InstalledPackage {
            id: package.id.clone(),
            name: package.name.clone(),
            version: resolved.version.clone(),
            category: package.category.clone(),
            portability: package.portability.clone(),
            install_dir: self.store.root_relative(&install_dir),
            commands,
            path_entries,
            execution: package.execution.clone(),
            persistent: effective_persistent,
            functional_generation: functional_generation().to_owned(),
        };

        state
            .packages
            .retain(|existing| !existing.id.eq_ignore_ascii_case(&installed.id));
        state.packages.push(installed.clone());
        if let Err(error) = self.store.save_installed_state(&state) {
            // Do not claim a successful upgrade when the journal failed.
            // Restore the old payload if it existed.
            let _ = fs::remove_dir_all(&install_dir);
            if backup.exists() {
                let _ = fs::rename(&backup, &install_dir);
            }
            return Err(error).context("instp: error al registrar paquete; restauración solicitada");
        }
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
        }
        // Installed payload is stored in its own directory. Never leave
        // additional downloaded archives behind after a successful install.
        self.store.clean_package_cache(&package.id)?;

        Ok(InstallReport {
            package: installed,
            missing_exposed_commands: missing,
            source_url: resolved.url,
        })
    }

    /// Arch is a Windows-managed WSL distribution, not a zip archive or a
    /// Windows executable. Only an explicit instp install arch authorizes
    /// calling the official WSL installer; startup never installs WSL.
    fn install_wsl_environment(
        &self, package: &PackageManifest, persistent: bool,
    ) -> Result<InstallReport> {
        anyhow::ensure!(package.id == "arch"
            && package.execution.as_ref().is_some_and(|e|
                e.kind == "environment" && e.provider.as_deref() == Some("wsl")
                    && e.distribution.as_deref() == Some("archlinux")),
            "instp: entorno WSL desconocido");
        #[cfg(windows)]
        {
            use std::process::Command;
            let wsl = crate::support::windows::system32_executable("wsl.exe")?;
            let check = Command::new(&wsl).args(["--list", "--quiet"]).output()
                .context("instp: no se pudo consultar WSL")?;
            if !check.status.success() {
                anyhow::bail!("instp arch: WSL2 no está disponible; habilítalo y reinicia Windows antes de instalar Arch");
            }
            let list = decode_wsl_output(&check.stdout).to_ascii_lowercase();
            if !list.lines().any(|line| line.trim() == "archlinux") {
                let outcome = Command::new(&wsl).args(["--install", "archlinux"]).status()
                    .context("instp arch: falló la ejecución del instalador oficial WSL")?;
                if !outcome.success() {
                    anyhow::bail!("instp arch: WSL no completó instalación (código {:?}); comprueba permisos y reinicia si Windows lo solicita", outcome.code());
                }
                let verify = Command::new(&wsl).args(["--list", "--quiet"]).output()?;
                if !verify.status.success() || !decode_wsl_output(&verify.stdout)
                    .lines().any(|line| line.trim().eq_ignore_ascii_case("archlinux"))
                {
                    anyhow::bail!("instp arch: Windows todavía no registró Arch; termina la instalación de WSL y repite instp install arch");
                }
            }
            let directory = self.store.environments_dir().join(&package.id);
            fs::create_dir_all(&directory)?;
            let mut state = self.store.load_installed_state()?;
            // A WSL distribution is external and survives SST updates.
            let _ = persistent;
            let is_persistent = true;
            let installed = InstalledPackage {
                id: package.id.clone(),
                name: package.name.clone(),
                version: package.version.clone(),
                category: package.category.clone(),
                portability: package.portability.clone(),
                install_dir: self.store.root_relative(&directory),
                commands: Vec::new(),
                path_entries: Vec::new(),
                execution: package.execution.clone(),
                persistent: is_persistent,
                functional_generation: functional_generation().to_owned(),
            };
            state.packages.retain(|p| !p.id.eq_ignore_ascii_case("arch"));
            state.packages.push(installed.clone());
            self.store.save_installed_state(&state)?;
            return Ok(InstallReport {
                package: installed,
                missing_exposed_commands: Vec::new(),
                source_url: "WSL2: distribución oficial archlinux".to_owned(),
            });
        }
        #[cfg(not(windows))]
        {
            let _ = persistent;
            anyhow::bail!("instp arch: la integración WSL2 requiere Windows")
        }
    }

    /// Upgrade installed packages against the currently published upstream
    /// releases. Retains their existing temporary/persistent lifetime.
    /// No package is installed implicitly when only consulting its status.
    pub fn upgrade(&self, selected: Option<&str>) -> Result<Vec<String>> {
        self.reconcile_for_functional_release()?;
        let installed = self.installed()?;
        let targets = if let Some(id) = selected {
            let found = installed.into_iter()
                .find(|pkg| pkg.id.eq_ignore_ascii_case(id))
                .ok_or_else(|| anyhow::anyhow!("instp upgrade: {id} no está instalado"))?;
            vec![found]
        } else {
            installed
        };
        let mut output = Vec::new();
        for installed in targets {
            let Some(manifest) = self.info(&installed.id)? else {
                output.push(format!("{}: no figura en el catálogo actual", installed.id));
                continue;
            };
            if manifest.install.kind == "builtin" {
                if manifest.version == installed.version {
                    output.push(format!("{}: ya tiene la última versión ({})",
                        installed.id, installed.version));
                } else {
                    let report = self.install_with_persistence(&installed.id, installed.persistent)?;
                    output.push(format!("{}: {} -> {} (módulo nativo)",
                        installed.id, installed.version, report.package.version));
                }
                continue;
            }
            if manifest.execution.as_ref().is_some_and(|e| e.kind == "environment") {
                output.push(format!("{}: entorno gestionado por WSL2 (usa instp install arch para verificarlo)", installed.id));
                continue;
            }
            let latest = resolve_source(&manifest)?;
            // For direct URLs with "latest", there is no published release
            // identifier to compare; always refresh the actual payload.
            let refresh = latest.version != installed.version
                || (manifest.source.kind == "url" && manifest.version == "latest");
            if !refresh {
                output.push(format!("{}: ya tiene la última versión ({})",
                    installed.id, installed.version));
                continue;
            }
            let previous = installed.version.clone();
            let report = self.install_with_persistence(&installed.id, installed.persistent)?;
            output.push(format!("{}: {} -> {} ({})",
                installed.id, previous, report.package.version,
                if report.package.persistent { "persistente" } else { "temporal" }));
        }
        Ok(output)
    }

    pub fn remove(&self, id: &str) -> Result<bool> {
        self.ensure_layout()?;
        let mut state = self.store.load_installed_state()?;
        let Some(index) = state
            .packages
            .iter()
            .position(|package| package.id.eq_ignore_ascii_case(id))
        else {
            return Ok(false);
        };

        let package = state.packages.remove(index);
        let install_dir = self.store.resolve_root_relative(&package.install_dir);
        anyhow::ensure!(
            !install_dir.components().any(|part| matches!(part, Component::ParentDir))
                && (install_dir.starts_with(self.store.tools_dir())
                    || install_dir.starts_with(self.store.runtimes_dir())
                    || install_dir.starts_with(self.store.modules_dir())
                    || install_dir.starts_with(self.store.environments_dir())
                    || install_dir.starts_with(self.store.packages_dir().join("tools"))
                    || install_dir.starts_with(self.store.packages_dir().join("runtimes"))
                    || install_dir.starts_with(self.store.packages_dir().join("modules"))
                    || install_dir.starts_with(self.store.packages_dir().join("environments"))),
            "instp: instalación fuera de su área aislada: {}",
            install_dir.display()
        );
        let quarantine = install_dir.with_extension("instp-removed");
        anyhow::ensure!(
            !quarantine.exists(),
            "instp: hay una eliminación anterior pendiente: {}",
            quarantine.display()
        );
        let moved = install_dir.exists();
        if moved {
            fs::rename(&install_dir, &quarantine)
                .with_context(|| format!("no se pudo preparar eliminación de {}", package.id))?;
        }
        if let Err(error) = self.store.save_installed_state(&state) {
            if moved {
                fs::rename(&quarantine, &install_dir).context(
                    "instp: no se pudo restaurar el paquete después de fallar el registro"
                )?;
            }
            return Err(error).context("instp: no se pudo registrar la eliminación");
        }
        if moved {
            fs::remove_dir_all(&quarantine).with_context(|| format!(
                "instp: paquete desregistrado, pero falta limpiar {}", quarantine.display()
            ))?;
        }
        self.store.clean_package_cache(id)?;
        Ok(true)
    }

    pub fn which(&self, command: &str) -> Result<Option<PathBuf>> {
        for package in self.installed()? {
            if let Some(found) = package
                .commands
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(command))
            {
                return Ok(Some(self.store.resolve_root_relative(&found.target)));
            }
        }
        Ok(None)
    }

    pub fn repositories(&self) -> Result<Vec<RepoSource>> {
        self.ensure_layout()?;
        let mut rows = self.store.load_repo_config()?.repositories;
        rows.sort_by(|left, right| right.priority.cmp(&left.priority));
        Ok(rows)
    }

    pub fn add_repository(&self, name: &str, url: &str, priority: i32) -> Result<()> {
        self.ensure_layout()?;
        validate_identifier(name)?;
        validate_https(url)?;
        let mut config = self.store.load_repo_config()?;
        if config
            .repositories
            .iter()
            .any(|repo| repo.name.eq_ignore_ascii_case(name))
        {
            anyhow::bail!("instp repo add: ya existe el repositorio {name}");
        }
        config.repositories.push(RepoSource {
            name: name.to_owned(),
            url: url.to_owned(),
            priority,
            enabled: true,
        });
        self.store.save_repo_config(&config)
    }

    pub fn remove_repository(&self, name: &str) -> Result<bool> {
        self.ensure_layout()?;
        let mut config = self.store.load_repo_config()?;
        let previous = config.repositories.len();
        config
            .repositories
            .retain(|repo| !repo.name.eq_ignore_ascii_case(name));
        let removed = config.repositories.len() != previous;
        if removed {
            self.store.save_repo_config(&config)?;
            let cached = self.store.repository_cache_file(name);
            if cached.exists() {
                let _ = fs::remove_file(cached);
            }
        }
        Ok(removed)
    }

    pub fn update_repositories(&self) -> Result<Vec<String>> {
        self.ensure_layout()?;
        let repos = self.repositories()?;
        let mut updated = Vec::new();
        for repo in repos.into_iter().filter(|repo| repo.enabled) {
            validate_https(&repo.url)?;
            let bytes = get(&repo.url)
                .with_context(|| format!("instp update: no se pudo descargar {}", repo.name))?;
            let index: PackageIndex = serde_json::from_slice(&bytes)
                .with_context(|| format!("instp update: índice inválido en {}", repo.name))?;
            validate_index(&index)?;
            fs::write(self.store.repository_cache_file(&repo.name), &bytes)?;
            updated.push(format!("{}: {} paquetes", repo.name, index.packages.len()));
        }
        Ok(updated)
    }

    pub fn clean_cache(&self) -> Result<usize> {
        self.store.clean_cache()
    }

    pub fn doctor(&self) -> Result<Vec<String>> {
        self.ensure_layout()?;
        Ok(vec![
            format!("root: {}", self.store.root_dir().display()),
            format!("packages: {}", self.store.packages_dir().display()),
            format!("cache: {}", self.store.cache_dir().display()),
            format!("repos: {}", self.repositories()?.len()),
            format!("installed: {}", self.installed()?.len()),
            format!("curl: {}", transport::curl_program()?.display()),
            format!("tar: {}", installer::tar_program()?.display()),
        ])
    }

    fn catalog(&self) -> Result<Vec<PackageManifest>> {
        self.ensure_layout()?;
        let builtin: PackageIndex = serde_json::from_str(BUILTIN_INDEX)
            .context("índice builtin de instp inválido")?;
        validate_index(&builtin)?;

        let mut repos = self.repositories()?;
        repos.sort_by(|left, right| right.priority.cmp(&left.priority));
        let mut seen = HashSet::new();
        let mut packages = Vec::new();

        for repo in repos {
            if !repo.enabled {
                continue;
            }
            let cache = self.store.repository_cache_file(&repo.name);
            if !cache.is_file() {
                continue;
            }
            let bytes = fs::read(&cache)?;
            let index: PackageIndex = serde_json::from_slice(&bytes)
                .with_context(|| format!("índice cacheado inválido: {}", repo.name))?;
            validate_index(&index)?;
            for package in index.packages {
                if seen.insert(package.id.to_ascii_lowercase()) {
                    packages.push(package);
                }
            }
        }

        for package in builtin.packages {
            if seen.insert(package.id.to_ascii_lowercase()) {
                packages.push(package);
            }
        }
        Ok(packages)
    }

    fn obtain_source(
        &self,
        package: &PackageManifest,
        source: &ResolvedSource,
    ) -> Result<PathBuf> {
        let safe_name = format!(
            "{}__{}__{}",
            package.id,
            model::sanitize_file_name(&source.version),
            model::sanitize_file_name(&source.file_name)
        );
        let destination = self.store.cache_dir().join(safe_name);
        if destination.is_file() {
            // Without a trustworthy digest the cached archive may be an
            // obsolete package with the same asset filename: fetch afresh.
            let valid = match source.sha256.as_deref() {
                Some(expected) => sha256_matches(&destination, expected)?,
                None => false,
            };
            if valid {
                return Ok(destination);
            }
            fs::remove_file(&destination)?;
        }

        download(&source.url, &destination, source.size)?;
        if let Some(expected) = source.sha256.as_deref() {
            if !sha256_matches(&destination, expected)? {
                let _ = fs::remove_file(&destination);
                anyhow::bail!("{}: SHA-256 no coincide; descarga rechazada", package.id);
            }
        }
        Ok(destination)
    }
}

fn sha256_matches(path: &std::path::Path, expected: &str) -> Result<bool> {
    let expected = expected.trim().trim_start_matches("sha256:");
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected))
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.to_string_lossy().to_ascii_lowercase()));
}
