use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    thread,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{adapters::persistence::AppPaths, support::windows::system32_executable};

const UPDATE_DIR: &str = "update";
const CHANNEL_FILE: &str = "update-channel.txt";
const REMOTE_MANIFEST_FILE: &str = "remote-manifest.json";
const PENDING_FILE: &str = "pending.json";
const STAGED_EXE: &str = "sst.exe.new";
const APPLY_SCRIPT: &str = "apply-update.cmd";
const CURL_USER_AGENT: &str = "Shell-Shock-Tool-updater";

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateManifest {
    pub schema: u32,
    pub version: String,
    #[serde(default)]
    pub channel: String,
    pub asset_url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingUpdate {
    version: String,
    sha256: String,
    staged_path: String,
    staged_at: String,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn spawn_background_check(paths: AppPaths) {
    if configured_manifest_url(&paths).is_none() {
        return;
    }
    thread::spawn(move || {
        let _ = check_and_stage(&paths);
    });
}

pub fn status(paths: &AppPaths) -> Result<String> {
    let pending = load_pending(paths)?;
    let channel = configured_manifest_url(paths);
    let mut output = format!("SST {}\n", current_version());
    match channel {
        Some(url) => output.push_str(&format!("Canal de actualización: {url}\n")),
        None => output.push_str("Canal de actualización: no configurado\n"),
    }
    if let Some(pending) = pending {
        if is_newer(&pending.version, current_version())? {
            output.push_str(&format!(
                "Actualización preparada: {} (se aplicará al cerrar SST)\n",
                pending.version
            ));
        } else {
            output.push_str("Actualización preparada: ninguna\n");
        }
    } else {
        output.push_str("Actualización preparada: ninguna\n");
    }
    Ok(output)
}

pub fn check_now(paths: &AppPaths, stage: bool) -> Result<String> {
    let manifest = fetch_manifest(paths)?;
    validate_manifest(&manifest)?;
    if !is_newer(&manifest.version, current_version())? {
        return Ok(format!(
            "SST {} ya está actualizado. Última versión publicada: {}\n",
            current_version(), manifest.version
        ));
    }

    if !stage {
        return Ok(format!(
            "Actualización disponible: {} -> {}\n",
            current_version(), manifest.version
        ));
    }

    stage_manifest(paths, &manifest)?;
    Ok(format!(
        "Actualización {} descargada y verificada. Se aplicará al cerrar SST.\n",
        manifest.version
    ))
}

pub fn check_and_stage(paths: &AppPaths) -> Result<bool> {
    let manifest = fetch_manifest(paths)?;
    validate_manifest(&manifest)?;
    if !is_newer(&manifest.version, current_version())? {
        cleanup_obsolete_pending(paths)?;
        return Ok(false);
    }
    stage_manifest(paths, &manifest)?;
    Ok(true)
}

/// Starts a hidden one-shot Windows batch helper that waits until this SST
/// process releases sst.exe, replaces it with the already verified staged
/// binary and relaunches SST. No permanent updater executable is installed.
pub fn schedule_pending_update(paths: &AppPaths) -> Result<bool> {
    let Some(pending) = load_pending(paths)? else {
        return Ok(false);
    };
    if !is_newer(&pending.version, current_version())? {
        cleanup_obsolete_pending(paths)?;
        return Ok(false);
    }

    let staged = PathBuf::from(&pending.staged_path);
    verify_file(&staged, &pending.sha256, None)?;
    let target = std::env::current_exe().context("no se pudo resolver sst.exe")?;
    let update_dir = update_dir(paths);
    fs::create_dir_all(&update_dir)?;
    let script = update_dir.join(APPLY_SCRIPT);
    let backup = target.with_extension("exe.old");
    let pending_file = pending_path(paths);

    let body = format!(
        "@echo off\r\n\
         setlocal DisableDelayedExpansion\r\n\
         set \"TARGET={}\"\r\n\
         set \"NEW={}\"\r\n\
         set \"BACKUP={}\"\r\n\
         set \"PENDING={}\"\r\n\
         for /L %%I in (1,1,120) do (\r\n\
           move /Y \"%TARGET%\" \"%BACKUP%\" >nul 2>&1 && goto replace\r\n\
           timeout /t 1 /nobreak >nul\r\n\
         )\r\n\
         exit /b 10\r\n\
         :replace\r\n\
         move /Y \"%NEW%\" \"%TARGET%\" >nul 2>&1\r\n\
         if errorlevel 1 (\r\n\
           move /Y \"%BACKUP%\" \"%TARGET%\" >nul 2>&1\r\n\
           exit /b 11\r\n\
         )\r\n\
         del /Q \"%BACKUP%\" >nul 2>&1\r\n\
         del /Q \"%PENDING%\" >nul 2>&1\r\n\
         start \"\" \"%TARGET%\"\r\n\
         del /Q \"%~f0\" >nul 2>&1\r\n",
        batch_value(&target),
        batch_value(&staged),
        batch_value(&backup),
        batch_value(&pending_file),
    );
    fs::write(&script, body)?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let cmd = system32_executable("cmd.exe")?;
        Command::new(cmd)
            .args(["/D", "/S", "/C"])
            .arg(&script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("no se pudo iniciar el aplicador de actualización")?;
    }

    #[cfg(not(windows))]
    {
        let _ = script;
        bail!("la aplicación automática de updates sólo está disponible en Windows");
    }

    Ok(true)
}

pub fn configured_manifest_url(paths: &AppPaths) -> Option<String> {
    std::env::var("SST_UPDATE_MANIFEST_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_owned())
        .or_else(|| {
            fs::read_to_string(paths.data_dir().join(CHANNEL_FILE))
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            option_env!("SST_UPDATE_MANIFEST_URL")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

fn fetch_manifest(paths: &AppPaths) -> Result<UpdateManifest> {
    let url = configured_manifest_url(paths).context(
        "canal de actualización no configurado; falta SST_UPDATE_MANIFEST_URL o data/update-channel.txt",
    )?;
    ensure_https(&url)?;
    let dir = update_dir(paths);
    fs::create_dir_all(&dir)?;
    let destination = dir.join(REMOTE_MANIFEST_FILE);
    download(&url, &destination)?;
    let bytes = fs::read(&destination)?;
    let mut manifest: UpdateManifest = serde_json::from_slice(&bytes)
        .context("sst-update.json no es un manifiesto válido")?;
    manifest.asset_url = resolve_asset_url(&url, &manifest.asset_url)?;
    Ok(manifest)
}

fn stage_manifest(paths: &AppPaths, manifest: &UpdateManifest) -> Result<()> {
    let dir = update_dir(paths);
    fs::create_dir_all(&dir)?;
    let partial = dir.join("sst.exe.part");
    let staged = dir.join(STAGED_EXE);
    let _ = fs::remove_file(&partial);
    download(&manifest.asset_url, &partial)?;
    verify_file(&partial, &manifest.sha256, manifest.size)?;
    let _ = fs::remove_file(&staged);
    fs::rename(&partial, &staged)?;

    let pending = PendingUpdate {
        version: manifest.version.clone(),
        sha256: manifest.sha256.to_ascii_lowercase(),
        staged_path: staged.to_string_lossy().into_owned(),
        staged_at: Utc::now().to_rfc3339(),
    };
    fs::write(pending_path(paths), serde_json::to_vec_pretty(&pending)?)?;
    Ok(())
}

fn validate_manifest(manifest: &UpdateManifest) -> Result<()> {
    if manifest.schema != 1 {
        bail!("schema de actualización no soportado: {}", manifest.schema);
    }
    parse_version(&manifest.version)
        .with_context(|| format!("versión remota inválida: {}", manifest.version))?;
    ensure_https(&manifest.asset_url)?;
    let hash = manifest.sha256.trim();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("SHA-256 inválido en el manifiesto");
    }
    Ok(())
}

fn download(url: &str, destination: &Path) -> Result<()> {
    ensure_https(url)?;
    #[cfg(windows)]
    let program = system32_executable("curl.exe")?;
    #[cfg(not(windows))]
    let program = PathBuf::from("curl");

    let mut command = Command::new(program);
    command.args([
        "-L",
        "--fail",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "-H",
        &format!("User-Agent: {CURL_USER_AGENT}"),
    ]);
    command.arg(url).arg("-o").arg(destination);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let status = command.status().context("no se pudo ejecutar curl")?;
    if !status.success() {
        bail!("curl no pudo descargar {url}");
    }
    Ok(())
}

fn verify_file(path: &Path, expected_hash: &str, expected_size: Option<u64>) -> Result<()> {
    let bytes = fs::read(path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    if let Some(size) = expected_size {
        if bytes.len() as u64 != size {
            bail!(
                "tamaño de actualización inválido: esperado {size}, recibido {}",
                bytes.len()
            );
        }
    }
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected_hash.trim()) {
        bail!("SHA-256 de actualización no coincide");
    }
    Ok(())
}

fn resolve_asset_url(manifest_url: &str, asset: &str) -> Result<String> {
    if asset.starts_with("https://") {
        return Ok(asset.to_owned());
    }
    if asset.contains("://") {
        bail!("el asset de actualización debe usar HTTPS");
    }
    let path = Path::new(asset);
    if path.is_absolute()
        || path.components().any(|component| !matches!(component, Component::Normal(_)))
        || asset.contains('/')
        || asset.contains('\\')
    {
        bail!("nombre de asset de actualización inválido: {asset}");
    }
    let (base, _) = manifest_url
        .rsplit_once('/')
        .context("URL de manifiesto inválida")?;
    Ok(format!("{base}/{asset}"))
}

fn ensure_https(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        bail!("SST sólo permite actualizaciones mediante HTTPS");
    }
    Ok(())
}

fn parse_version(value: &str) -> Option<(u64, u64, u64, u64)> {
    let value = value.trim().trim_start_matches('v');
    let (core, suffix) = value.split_once('-').unwrap_or((value, ""));
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let build = if suffix.is_empty() {
        u64::MAX
    } else {
        suffix.strip_prefix("blue.")?.parse().ok()?
    };
    Some((major, minor, patch, build))
}

fn is_newer(candidate: &str, current: &str) -> Result<bool> {
    let candidate = parse_version(candidate)
        .with_context(|| format!("versión candidata inválida: {candidate}"))?;
    let current = parse_version(current)
        .with_context(|| format!("versión local inválida: {current}"))?;
    Ok(candidate > current)
}

fn update_dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join(UPDATE_DIR)
}

fn pending_path(paths: &AppPaths) -> PathBuf {
    update_dir(paths).join(PENDING_FILE)
}

fn load_pending(paths: &AppPaths) -> Result<Option<PendingUpdate>> {
    let path = pending_path(paths);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path)?;
    Ok(Some(
        serde_json::from_slice(&bytes).context("pending.json de update está dañado")?,
    ))
}

fn cleanup_obsolete_pending(paths: &AppPaths) -> Result<()> {
    let Some(pending) = load_pending(paths)? else {
        return Ok(());
    };
    if is_newer(&pending.version, current_version())? {
        return Ok(());
    }
    let _ = fs::remove_file(&pending.staged_path);
    let _ = fs::remove_file(pending_path(paths));
    Ok(())
}

fn batch_value(path: &Path) -> String {
    path.to_string_lossy().replace('%', "%%").replace('"', "\"\"")
}
