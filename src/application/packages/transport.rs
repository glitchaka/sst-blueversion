use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::model::{PackageManifest, validate_https};

#[derive(Debug, Clone)]
pub(super) struct ResolvedSource {
    pub url: String,
    pub file_name: String,
    pub version: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
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
    #[serde(default)]
    size: Option<u64>,
}

pub(super) fn resolve_source(package: &PackageManifest) -> Result<ResolvedSource> {
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
                size: None,
            })
        }
        "github_release" => resolve_github_release(package),
        other => anyhow::bail!("{}: source.type no soportado: {other}", package.id),
    }
}

fn resolve_github_release(package: &PackageManifest) -> Result<ResolvedSource> {
    let repository = package
        .source
        .repository
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("{}: source.repository ausente", package.id))?;
    let url = format!("https://api.github.com/repos/{repository}/releases/latest");
    let bytes = get(&url)?;
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

    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256:"))
        .map(str::to_owned)
        .or_else(|| package.source.sha256.clone());

    Ok(ResolvedSource {
        url: asset.browser_download_url.clone(),
        file_name: asset.name.clone(),
        version: release.tag_name,
        sha256,
        size: asset.size,
    })
}

pub(super) fn get(url: &str) -> Result<Vec<u8>> {
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

pub(super) fn download(url: &str, destination: &Path, size: Option<u64>) -> Result<()> {
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
    let output = crate::support::transfer::run(&mut command, destination, size, "INSTP")?;
    if !output.status.success() {
        let _ = std::fs::remove_file(destination);
        anyhow::bail!(
            "instp: descarga falló: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub(super) fn curl_program() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        crate::support::windows::system32_executable("curl.exe")
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from("curl"))
    }
}

pub(super) fn configure_hidden(command: &mut Command) {
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
