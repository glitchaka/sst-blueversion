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
        validate_portability(&package.portability)?;
        validate_install_kind(&package.install.kind)?;
    }
    Ok(())
}

pub(super) fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty()
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

fn validate_portability(value: &str) -> Result<()> {
    if !matches!(value, "pure" | "isolated" | "system" | "driver") {
        anyhow::bail!("instp: clase de portabilidad desconocida: {value}");
    }
    Ok(())
}

fn validate_install_kind(value: &str) -> Result<()> {
    if !matches!(value, "single" | "zip" | "archive" | "system") {
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
