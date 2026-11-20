use std::collections::HashSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct RepositoryInfo {
    pub name: String,
    #[serde(default, rename = "description")]
    pub _description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageIndex {
    pub schema: u32,
    pub repository: RepositoryInfo,
    pub packages: Vec<PackageManifest>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageManifest {
    #[serde(default = "format_v1")]
    pub format: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub portability: String,
    pub source: PackageSource,
    pub install: PackageInstall,
    #[serde(default)]
    pub execution: Option<PackageExecution>,
}

fn format_v1() -> u32 { 1 }

/// Versioned execution contract common to CLI, runtimes, in-process modules
/// and managed environments. Category is only for catalog presentation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageExecution {
    pub kind: String,
    #[serde(default)]
    pub entry: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub required_files: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub abi: Option<String>,
    #[serde(default)]
    pub startup: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub distribution: Option<String>,
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
pub(super) struct RepoConfig {
    pub repositories: Vec<RepoSource>,
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
    /// Execution adapter contract recorded at installation; older installs
    /// have no adapter and must not claim an embedded engine is functional.
    #[serde(default)]
    pub execution: Option<PackageExecution>,
    /// Explicit authorization to retain this package between functional SST releases.
    #[serde(default)]
    pub persistent: bool,
    /// Functional generation when it was installed or migrated into the catalog.
    #[serde(default)]
    pub functional_generation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledCommand {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct InstalledState {
    pub packages: Vec<InstalledPackage>,
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub package: InstalledPackage,
    pub missing_exposed_commands: Vec<String>,
    pub source_url: String,
}

pub(super) fn validate_index(index: &PackageIndex) -> Result<()> {
    if index.schema != 2 {
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
        validate_portability(&package.portability)?;
        validate_install_kind(&package.install.kind)?;
        if package.format != 1 {
            anyhow::bail!("instp: formato SST-PACKAGE desconocido: {}", package.format);
        }
        let execution = package.execution.as_ref().ok_or_else(|| {
            anyhow::anyhow!("instp: {} no declara adaptador execution", package.id)
        })?;
        // All catalog entries must be genuinely installable SST packages or
        // explicitly authorized system environments. No vendor EXE metadata.
        validate_execution(package, execution)?;

        if let Some(name) = package.install.rename.as_deref() {
            validate_relative_payload_path(name)?;
        }
        for exposed in &package.install.expose {
            validate_relative_payload_path(exposed)?;
        }
    }
    Ok(())
}

fn validate_execution(package: &PackageManifest, execution: &PackageExecution) -> Result<()> {
    match execution.kind.as_str() {
        "embedded" => {
            if package.category != "module" || package.install.kind != "pkgx"
                || execution.abi.as_deref() != Some("sst-hostfxr/1")
                || execution.startup.as_deref() != Some("load")
            {
                anyhow::bail!("instp: módulo {} tiene ABI, categoría o arranque inválido", package.id);
            }
            validate_relative_payload_path(&execution.entry)?;
            if execution.extensions.is_empty() && execution.commands.is_empty() {
                anyhow::bail!("instp: módulo {} no declara comandos ni extensiones", package.id);
            }
        }
        "runtime" | "process" => {
            anyhow::bail!(
                "instp: {} requiere un adaptador interno de ejecución; no se admiten runtimes ni CLI externos como .exe",
                package.id
            );
        }
        "environment" => {
            if package.install.kind == "pkgx" {
                if execution.target.as_deref() != Some("unix") {
                    anyhow::bail!("instp: {} debe declarar plataforma Unix", package.id);
                }
            } else if package.category != "environment" || package.portability != "system"
                || package.source.kind != "system" || package.install.kind != "system"
                || execution.provider.as_deref() != Some("wsl")
                || execution.distribution.as_deref() != Some("archlinux")
                || execution.entry != "wsl.exe"
            {
                anyhow::bail!("instp: entorno {} sin proveedor WSL reconocido", package.id);
            }
        }
        "data" => {
            if package.install.kind != "pkgx" {
                anyhow::bail!("instp: los datos de {} requieren un .pkgx", package.id);
            }
        }
        "system" => {
            if package.install.kind != "system" {
                anyhow::bail!("instp: {} no es un paquete de sistema", package.id);
            }
        }
        other => anyhow::bail!("instp: {} usa adaptador desconocido: {other}", package.id),
    }
    if package.install.kind == "pkgx" && execution.target.is_none() {
        anyhow::bail!("instp: {} no declara su plataforma win/unix", package.id);
    }
    if let Some(target) = execution.target.as_deref() {
        if !matches!(target, "win" | "unix") {
            anyhow::bail!("instp: {} tiene plataforma no soportada: {target}", package.id);
        }
    }
    for file in &execution.required_files {
        validate_relative_payload_path(file)?;
        if package.install.kind == "pkgx" && forbidden_executable(file) {
            anyhow::bail!("instp: {} no puede instalar ejecutables externos", package.id);
        }
    }
    if package.install.kind == "pkgx" && forbidden_executable(&execution.entry) {
        anyhow::bail!("instp: {} no puede incluir un .exe como entrada", package.id);
    }
    for command in &execution.commands {
        validate_identifier(command)?;
    }
    for ext in &execution.extensions {
        if !ext.starts_with('.') || ext.len() < 2 || ext.len() > 12
            || !ext[1..].bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            anyhow::bail!("instp: {} declara extensión insegura: {ext}", package.id);
        }
    }
    Ok(())
}

/// Manifest file paths must stay inside the isolated package staging tree.
/// Reject Windows drive prefixes, UNC paths, empty segments and traversal
/// even when indices are parsed on a non-Windows build host.
pub(super) fn validate_relative_payload_path(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || (value.starts_with('/') || value.starts_with('\\'))
        || value.contains(':')
        || value.chars().any(|ch| ch == '\0')
        || value.split(|ch| ch == '/' || ch == '\\').any(|part| part == "." || part == ".." || part.is_empty())
    {
        anyhow::bail!("instp: ruta de paquete insegura: {value}");
    }
    Ok(())
}

pub(super) fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty() || matches!(value, "." | "..")
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        anyhow::bail!("instp: identificador inválido: {value}");
    }
    Ok(())
}

pub(super) fn validate_https(url: &str) -> Result<()> {
    if !url.to_ascii_lowercase().starts_with("https://") {
        anyhow::bail!("instp: solo se permiten fuentes HTTPS en repositorios remotos");
    }
    Ok(())
}

fn forbidden_executable(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".exe", ".com", ".bat", ".cmd"].iter().any(|ext| lower.ends_with(ext))
}

fn validate_portability(value: &str) -> Result<()> {
    if !matches!(value, "pure" | "isolated" | "system" | "driver") {
        anyhow::bail!("instp: clase de portabilidad desconocida: {value}");
    }
    Ok(())
}

fn validate_install_kind(value: &str) -> Result<()> {
    if !matches!(value, "pkgx" | "system") {
        anyhow::bail!("instp: tipo de instalación desconocido: {value}");
    }
    Ok(())
}

pub(super) fn sanitize_file_name(value: &str) -> String {
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

#[cfg(test)]
mod manifest_security_tests {
    use super::*;

    #[test]
    fn payload_paths_stay_beneath_package_root() {
        for valid in ["pwsh.exe", "bin/tool.exe", r"bin\tool.exe"] {
            assert!(validate_relative_payload_path(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "", "../escape.exe", r"..\escape.exe", "bin/../../escape.exe",
            "/root.exe", r"\absolute.exe", "C:/Windows/system32/cmd.exe",
            r"\\server\share\tool.exe", "bin//tool.exe", "./tool.exe",
        ] {
            assert!(validate_relative_payload_path(invalid).is_err(), "{invalid}");
        }
    }
}

#[cfg(test)]
mod standard_manifest_tests {
    use super::*;

    #[test]
    fn all_bundled_packages_follow_sst_package_1() {
        let index: PackageIndex = serde_json::from_str(include_str!(
            "../../../packages/index.json"
        )).unwrap();
        validate_index(&index).unwrap();
        assert_eq!(index.schema, 2);
        assert!(index.packages.iter().all(|package| package.format == 1
            && package.execution.is_some()));
        assert_eq!(index.packages.len(), 2);
        assert!(index.packages.iter().all(|package| {
            package.install.kind == "pkgx"
                || (package.id == "arch"
                    && package.install.kind == "system"
                    && package.execution.as_ref().is_some_and(|e| e.kind == "environment"))
        }));
    }

    #[test]
    fn rejects_external_executable_catalog_entries() {
        let json = include_str!("../../../packages/index.json");
        let mut index: PackageIndex = serde_json::from_str(json).unwrap();
        let mut rogue = index.packages.iter().find(|p| p.id == "powershell").unwrap().clone();
        rogue.id = "foreign-exe".to_owned();
        rogue.install.kind = "zip".to_owned();
        rogue.execution.as_mut().unwrap().kind = "process".to_owned();
        index.packages.push(rogue);
        assert!(validate_index(&index).is_err());
    }

    #[test]
    fn arch_wsl2_is_an_environment_not_a_windows_executable() {
        let index: PackageIndex = serde_json::from_str(include_str!(
            "../../../packages/index.json"
        )).unwrap();
        let arch = index.packages.iter().find(|package| package.id == "arch").unwrap();
        let exec = arch.execution.as_ref().unwrap();
        assert_eq!(exec.kind, "environment");
        assert_eq!(exec.provider.as_deref(), Some("wsl"));
        assert_eq!(exec.distribution.as_deref(), Some("archlinux"));
        assert!(arch.install.expose.is_empty());
    }

    #[test]
    fn powershell_requires_real_embedded_engine_and_script_routing() {
        let index: PackageIndex = serde_json::from_str(include_str!(
            "../../../packages/index.json"
        )).unwrap();
        let ps = index.packages.iter().find(|package| package.id == "powershell").unwrap();
        let exec = ps.execution.as_ref().unwrap();
        assert_eq!(exec.kind, "embedded");
        assert_eq!(exec.abi.as_deref(), Some("sst-hostfxr/1"));
        assert!(exec.extensions.iter().any(|ext| ext == ".ps1"));
        assert_eq!(exec.entry, "Sst.PowerShell.Bridge.dll");
    }
}
