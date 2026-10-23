use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::adapters::persistence::AppPaths;

const BUILTIN_INDEX: &str = include_str!("../../packages/index.json");
const DEFAULT_REPO_URL: &str =
    "https://raw.githubusercontent.com/glitchaka/sst-blueversion/main/packages/index.json";

#[derive(Debug, Clone, Deserialize)]
pub struct RepositoryInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageIndex {
    pub schema: u32,
    pub repository: RepositoryInfo,
    pub packages: Vec<PackageManifest>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub portability: String,
    pub source: PackageSource,
    pub install: PackageInstall,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageSource {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub asset_contains: Vec<String>,
    #[serde(default)]
    pub asset_excludes: Vec<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageInstall {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub rename: Option<String>,
    #[serde(default)]
    pub expose: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSource {
    pub name: String,
    pub url: String,
    pub priority: i32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RepoConfig {
    repositories: Vec<RepoSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub portability: String,
    pub install_dir: String,
    pub commands: Vec<InstalledCommand>,
    pub path_entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledCommand {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct InstalledState {
    packages: Vec<InstalledPackage>,
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub package: InstalledPackage,
    pub missing_exposed_commands: Vec<String>,
    pub source_url: String,
}

#[derive(Debug, Clone)]
struct ResolvedSource {
    url: String,
    file_name: String,
    version: String,
    sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Clone)]
pub struct PackageService {
    paths: AppPaths,
}

impl PackageService {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for path in [
            self.packages_dir(),
            self.cache_dir(),
            self.tools_dir(),
            self.runtimes_dir(),
            self.repositories_dir(),
            self.ecosystems_dir(),
            self.bin_dir(),
        ] {
            fs::create_dir_all(path)?;
        }

        if !self.repos_file().exists() {
            self.save_repo_config(&RepoConfig {
                repositories: vec![RepoSource {
                    name: "sst-main".to_owned(),
                    url: DEFAULT_REPO_URL.to_owned(),
                    priority: 100,
                    enabled: true,
                }],
            })?;
        }
        if !self.installed_file().exists() {
            self.save_installed_state(&InstalledState::default())?;
        }
        Ok(())
    }

    pub fn activate_process_environment(&self) -> Result<()> {
        self.ensure_layout()?;
        let state = self.load_installed_state()?;
        let mut entries = vec![self.bin_dir()];
        for package in &state.packages {
            for entry in &package.path_entries {
                entries.push(self.resolve_root_relative(entry));
            }
        }

        let ruby_home = self.ecosystems_dir().join("ruby");
        if state.packages.iter().any(|package| package.id == "ruby") {
            fs::create_dir_all(ruby_home.join("bin"))?;
            entries.push(ruby_home.join("bin"));
        }

        let inherited = env::var_os("PATH").unwrap_or_default();
        entries.extend(env::split_paths(&inherited));
        dedup_paths(&mut entries);
        let joined = env::join_paths(entries).context("no se pudo construir PATH portable de instp")?;

        // SAFETY: build_engine invokes this before NativeShellEngine creates the
        // Bash environment and before the interactive UI starts worker threads.
        // This changes only SST's process environment; it does not modify the
        // user or system PATH in Windows.
        unsafe {
            env::set_var("PATH", joined);
            env::set_var("SST_PACKAGES", self.packages_dir());
            if state.packages.iter().any(|package| package.id == "ruby") {
                env::set_var("GEM_HOME", &ruby_home);
                env::set_var("GEM_PATH", &ruby_home);
            }
        }
        Ok(())
    }

    pub fn search(&self, term: Option<&str>) -> Result<Vec<PackageManifest>> {
        let term = term.unwrap_or_default().trim().to_ascii_lowercase();
        let mut packages = self.catalog()?;
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
        let mut packages = self.load_installed_state()?.packages;
        packages.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(packages)
    }

    pub fn install(&self, id: &str) -> Result<InstallReport> {
        self.ensure_layout()?;
        validate_identifier(id)?;
        let package = self
            .info(id)?
            .ok_or_else(|| anyhow::anyhow!("instp: paquete no encontrado: {id}"))?;

        if package.install.kind == "system" || package.portability == "system" {
            anyhow::bail!(
                "instp: {} está marcado como SYSTEM y modifica Windows; esta primera implementación solo instala paquetes PURE/ISOLATED",
                package.id
            );
        }

        let resolved = self.resolve_source(&package)?;
        let archive = self.obtain_source(&package, &resolved)?;
        let install_dir = if package.category == "runtime" {
            self.runtimes_dir().join(&package.id)
        } else {
            self.tools_dir().join(&package.id)
        };

        if install_dir.exists() {
            fs::remove_dir_all(&install_dir)
                .with_context(|| format!("no se pudo reemplazar {}", install_dir.display()))?;
        }
        fs::create_dir_all(&install_dir)?;
        self.install_payload(&package, &archive, &install_dir)?;

        let mut commands = Vec::new();
        let mut path_entries = Vec::new();
        let mut missing = Vec::new();
        for exposed in &package.install.expose {
            match find_file(&install_dir, exposed)? {
                Some(target) => {
                    let command_name = command_name_from_exposed(exposed);
                    let relative_target = self.root_relative(&target);
                    commands.push(InstalledCommand {
                        name: command_name,
                        target: relative_target,
                    });
                    if let Some(parent) = target.parent() {
                        path_entries.push(self.root_relative(parent));
                    }
                }
                None => missing.push(exposed.clone()),
            }
        }
        if path_entries.is_empty() {
            path_entries.push(self.root_relative(&install_dir));
        }
        path_entries.sort();
        path_entries.dedup();

        let installed = InstalledPackage {
            id: package.id.clone(),
            name: package.name.clone(),
            version: resolved.version.clone(),
            category: package.category.clone(),
            portability: package.portability.clone(),
            install_dir: self.root_relative(&install_dir),
            commands,
            path_entries,
        };

        let mut state = self.load_installed_state()?;
        state
            .packages
            .retain(|existing| !existing.id.eq_ignore_ascii_case(&installed.id));
        state.packages.push(installed.clone());
        self.save_installed_state(&state)?;

        Ok(InstallReport {
            package: installed,
            missing_exposed_commands: missing,
            source_url: resolved.url,
        })
    }

    pub fn remove(&self, id: &str) -> Result<bool> {
        self.ensure_layout()?;
        let mut state = self.load_installed_state()?;
        let Some(index) = state
            .packages
            .iter()
            .position(|package| package.id.eq_ignore_ascii_case(id))
        else {
            return Ok(false);
        };
        let package = state.packages.remove(index);
        let install_dir = self.resolve_root_relative(&package.install_dir);
        if install_dir.exists() {
            fs::remove_dir_all(&install_dir)
                .with_context(|| format!("no se pudo eliminar {}", install_dir.display()))?;
        }
        self.save_installed_state(&state)?;
        Ok(true)
    }

    pub fn which(&self, command: &str) -> Result<Option<PathBuf>> {
        for package in self.installed()? {
            if let Some(found) = package
                .commands
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(command))
            {
                return Ok(Some(self.resolve_root_relative(&found.target)));
            }
        }
        Ok(None)
    }

    pub fn repositories(&self) -> Result<Vec<RepoSource>> {
        self.ensure_layout()?;
        let mut rows = self.load_repo_config()?.repositories;
        rows.sort_by(|left, right| right.priority.cmp(&left.priority));
        Ok(rows)
    }

    pub fn add_repository(&self, name: &str, url: &str, priority: i32) -> Result<()> {
        self.ensure_layout()?;
        validate_identifier(name)?;
        validate_https(url)?;
        let mut config = self.load_repo_config()?;
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
        self.save_repo_config(&config)
    }

    pub fn remove_repository(&self, name: &str) -> Result<bool> {
        self.ensure_layout()?;
        let mut config = self.load_repo_config()?;
        let previous = config.repositories.len();
        config
            .repositories
            .retain(|repo| !repo.name.eq_ignore_ascii_case(name));
        let removed = config.repositories.len() != previous;
        if removed {
            self.save_repo_config(&config)?;
            let cached = self.repository_cache_file(name);
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
            let bytes = curl_get(&repo.url)
                .with_context(|| format!("instp update: no se pudo descargar {}", repo.name))?;
            let index: PackageIndex = serde_json::from_slice(&bytes)
                .with_context(|| format!("instp update: índice inválido en {}", repo.name))?;
            validate_index(&index)?;
            fs::write(self.repository_cache_file(&repo.name), &bytes)?;
            updated.push(format!("{}: {} paquetes", repo.name, index.packages.len()));
        }
        Ok(updated)
    }

    pub fn clean_cache(&self) -> Result<usize> {
        self.ensure_layout()?;
        let mut removed = 0usize;
        for entry in fs::read_dir(self.cache_dir())? {
            let entry = entry?;
            if entry.path().is_file() {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn doctor(&self) -> Result<Vec<String>> {
        self.ensure_layout()?;
        let mut rows = vec![
            format!("root: {}", self.paths.root_dir().display()),
            format!("packages: {}", self.packages_dir().display()),
            format!("cache: {}", self.cache_dir().display()),
            format!("repos: {}", self.repositories()?.len()),
            format!("installed: {}", self.installed()?.len()),
        ];
        rows.push(format!("curl: {}", curl_program()?.display()));
        rows.push(format!("tar: {}", tar_program()?.display()));
        Ok(rows)
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
            let cache = self.repository_cache_file(&repo.name);
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

    fn resolve_source(&self, package: &PackageManifest) -> Result<ResolvedSource> {
        match package.source.kind.as_str() {
            "url" => {
                let url = package
                    .source
                    .url
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("{}: source.url ausente", package.id))?;
                validate_https(url)?;
                let file_name = url
                    .split('?')
                    .next()
                    .and_then(|value| value.rsplit('/').next())
                    .filter(|value| !value.is_empty())
                    .unwrap_or(&package.id)
                    .to_owned();
                Ok(ResolvedSource {
                    url: url.to_owned(),
                    file_name,
                    version: package.version.clone(),
                    sha256: package.source.sha256.clone(),
                })
            }
            "github_release" => self.resolve_github_release(package),
            other => anyhow::bail!("{}: source.type no soportado: {other}", package.id),
        }
    }

    fn resolve_github_release(&self, package: &PackageManifest) -> Result<ResolvedSource> {
        let repository = package
            .source
            .repository
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("{}: source.repository ausente", package.id))?;
        let url = format!("https://api.github.com/repos/{repository}/releases/latest");
        let bytes = curl_get(&url)?;
        let release: GitHubRelease = serde_json::from_slice(&bytes)
            .with_context(|| format!("{}: respuesta de GitHub Releases inválida", package.id))?;

        let asset = release
            .assets
            .iter()
            .find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                package
                    .source
                    .asset_contains
                    .iter()
                    .all(|needle| name.contains(&needle.to_ascii_lowercase()))
                    && package
                        .source
                        .asset_excludes
                        .iter()
                        .all(|needle| !name.contains(&needle.to_ascii_lowercase()))
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{}: GitHub Release {} no contiene un asset compatible",
                    package.id,
                    release.tag_name
                )
            })?;

        let digest = asset
            .digest
            .as_deref()
            .and_then(|value| value.strip_prefix("sha256:"))
            .map(str::to_owned)
            .or_else(|| package.source.sha256.clone());

        Ok(ResolvedSource {
            url: asset.browser_download_url.clone(),
            file_name: asset.name.clone(),
            version: release.tag_name,
            sha256: digest,
        })
    }

    fn obtain_source(&self, package: &PackageManifest, source: &ResolvedSource) -> Result<PathBuf> {
        let safe_name = format!("{}-{}", package.id, sanitize_file_name(&source.file_name));
        let destination = self.cache_dir().join(safe_name);
        if destination.is_file() {
            if source
                .sha256
                .as_deref()
                .is_none_or(|expected| sha256_matches(&destination, expected).unwrap_or(false))
            {
                return Ok(destination);
            }
            fs::remove_file(&destination)?;
        }

        curl_download(&source.url, &destination)?;
        if let Some(expected) = source.sha256.as_deref() {
            if !sha256_matches(&destination, expected)? {
                let _ = fs::remove_file(&destination);
                anyhow::bail!("{}: SHA-256 no coincide; descarga rechazada", package.id);
            }
        }
        Ok(destination)
    }

    fn install_payload(
        &self,
        package: &PackageManifest,
        source: &Path,
        destination: &Path,
    ) -> Result<()> {
        match package.install.kind.as_str() {
            "single" => {
                let name = package
                    .install
                    .rename
                    .as_deref()
                    .or_else(|| source.file_name().and_then(|value| value.to_str()))
                    .unwrap_or(&package.id);
                fs::copy(source, destination.join(name))?;
                Ok(())
            }
            "zip" => extract_zip(source, destination),
            "archive" => extract_archive(source, destination),
            "system" => anyhow::bail!("{} requiere instalación de sistema", package.id),
            other => anyhow::bail!("{}: install.type no soportado: {other}", package.id),
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.paths.root_dir().join("packages")
    }

    fn cache_dir(&self) -> PathBuf {
        self.packages_dir().join("cache")
    }

    fn tools_dir(&self) -> PathBuf {
        self.packages_dir().join("tools")
    }

    fn runtimes_dir(&self) -> PathBuf {
        self.packages_dir().join("runtimes")
    }

    fn repositories_dir(&self) -> PathBuf {
        self.packages_dir().join("repositories")
    }

    fn ecosystems_dir(&self) -> PathBuf {
        self.packages_dir().join("ecosystems")
    }

    fn bin_dir(&self) -> PathBuf {
        self.packages_dir().join("bin")
    }

    fn installed_file(&self) -> PathBuf {
        self.packages_dir().join("installed.json")
    }

    fn repos_file(&self) -> PathBuf {
        self.packages_dir().join("repos.json")
    }

    fn repository_cache_file(&self, name: &str) -> PathBuf {
        self.repositories_dir()
            .join(format!("{}.json", sanitize_file_name(name)))
    }

    fn load_repo_config(&self) -> Result<RepoConfig> {
        let bytes = fs::read(self.repos_file())?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn save_repo_config(&self, config: &RepoConfig) -> Result<()> {
        fs::write(self.repos_file(), serde_json::to_vec_pretty(config)?)?;
        Ok(())
    }

    fn load_installed_state(&self) -> Result<InstalledState> {
        let bytes = fs::read(self.installed_file())?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn save_installed_state(&self, state: &InstalledState) -> Result<()> {
        fs::write(self.installed_file(), serde_json::to_vec_pretty(state)?)?;
        Ok(())
    }

    fn root_relative(&self, path: &Path) -> String {
        path.strip_prefix(self.paths.root_dir())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    fn resolve_root_relative(&self, value: &str) -> PathBuf {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            self.paths.root_dir().join(path)
        }
    }
}

fn validate_index(index: &PackageIndex) -> Result<()> {
    if index.schema != 1 {
        anyhow::bail!("instp: schema de repositorio no soportado: {}", index.schema);
    }
    if index.repository.name.trim().is_empty() {
        anyhow::bail!("instp: repositorio sin nombre");
    }
    let mut ids = HashSet::new();
    for package in &index.packages {
        validate_identifier(&package.id)?;
        if !ids.insert(package.id.to_ascii_lowercase()) {
            anyhow::bail!("instp: paquete duplicado en índice: {}", package.id);
        }
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        anyhow::bail!("instp: identificador inválido: {value}");
    }
    Ok(())
}

fn validate_https(url: &str) -> Result<()> {
    if !url.to_ascii_lowercase().starts_with("https://") {
        anyhow::bail!("instp: solo se permiten fuentes HTTPS en repositorios remotos");
    }
    Ok(())
}

fn curl_program() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        return crate::support::windows::system32_executable("curl.exe");
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from("curl"))
    }
}

fn tar_program() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        return crate::support::windows::system32_executable("tar.exe");
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from("tar"))
    }
}

fn configure_hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

fn curl_get(url: &str) -> Result<Vec<u8>> {
    validate_https(url)?;
    let mut command = Command::new(curl_program()?);
    command.args([
        "-L",
        "--fail",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "-H",
        "User-Agent: Shell-Shock-Tool-instp",
        url,
    ]);
    configure_hidden(&mut command);
    let output = command.output().context("instp: no se pudo ejecutar curl")?;
    if !output.status.success() {
        anyhow::bail!(
            "instp: curl falló: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn curl_download(url: &str, destination: &Path) -> Result<()> {
    validate_https(url)?;
    let mut command = Command::new(curl_program()?);
    command.args([
        "-L",
        "--fail",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "-H",
        "User-Agent: Shell-Shock-Tool-instp",
        "-o",
    ]);
    command.arg(destination).arg(url);
    configure_hidden(&mut command);
    let output = command.output().context("instp: no se pudo ejecutar curl")?;
    if !output.status.success() {
        let _ = fs::remove_file(destination);
        anyhow::bail!(
            "instp: descarga falló: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn extract_zip(source: &Path, destination: &Path) -> Result<()> {
    let file = File::open(source)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
            continue;
        };
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&output)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }
    Ok(())
}

fn extract_archive(source: &Path, destination: &Path) -> Result<()> {
    let mut command = Command::new(tar_program()?);
    command.arg("-xf").arg(source).arg("-C").arg(destination);
    configure_hidden(&mut command);
    let output = command.output().context("instp: no se pudo ejecutar tar")?;
    if !output.status.success() {
        anyhow::bail!(
            "instp: no se pudo extraer {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn sha256_matches(path: &Path, expected: &str) -> Result<bool> {
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

fn find_file(root: &Path, requested: &str) -> Result<Option<PathBuf>> {
    let direct = root.join(requested);
    if direct.is_file() {
        return Ok(Some(direct));
    }
    let requested_name = Path::new(requested)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(requested);
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(requested_name))
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

fn command_name_from_exposed(exposed: &str) -> String {
    Path::new(exposed)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(exposed)
        .to_owned()
}

fn sanitize_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.to_string_lossy().to_ascii_lowercase()));
}
