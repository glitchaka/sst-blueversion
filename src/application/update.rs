use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
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
const APPLIED_FILE: &str = "applied.json";
const STAGED_EXE: &str = "sst.exe.new";
const CURL_USER_AGENT: &str = "Shell-Shock-Tool-updater";
const MAX_CHANGES: usize = 32;
const MAX_CHANGE_LEN: usize = 240;

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
    #[serde(default)]
    pub updater_asset_url: Option<String>,
    #[serde(default)]
    pub updater_sha256: Option<String>,
    #[serde(default)]
    pub updater_size: Option<u64>,
    #[serde(default)]
    pub previous_version: Option<String>,
    #[serde(default)]
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingUpdate {
    #[serde(default)]
    from_version: String,
    version: String,
    sha256: String,
    staged_path: String,
    staged_at: String,
    #[serde(default)]
    updater_sha256: Option<String>,
    #[serde(default)]
    changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppliedUpdate {
    from_version: String,
    version: String,
    applied_at: String,
    #[serde(default)]
    changes: Vec<String>,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// Authorization is deliberately process-local. A pending.json left behind
// from another run must never trigger an automatic update on the next launch.
// Only an explicit "instp upgrade sst" in this GUI session authorizes handoff
// when the terminal is closed.
static EXPLICIT_INSTALL_AUTHORIZATION: AtomicBool = AtomicBool::new(false);

pub fn take_explicit_install_authorization() -> bool {
    EXPLICIT_INSTALL_AUTHORIZATION.swap(false, Ordering::SeqCst)
}

pub fn status(paths: &AppPaths) -> Result<String> {
    let pending = load_pending(paths)?;
    let channel = configured_manifest_url(paths);
    let mut output = format!("SST {}\n", current_version());
    if let Some(applied) = take_applied_receipt(paths)? {
        output.push_str(&format!(
            "Última actualización aplicada: {} -> {}\n",
            applied.from_version, applied.version
        ));
        append_plain_changes(&mut output, &applied.changes);
    }
    match channel {
        Some(url) => output.push_str(&format!("Canal de actualización: {url}\n")),
        None => output.push_str("Canal de actualización: no configurado\n"),
    }
    if let Some(pending) = pending {
        if is_newer(&pending.version, current_version())? {
            output.push_str(&format!(
                "Actualización preparada: {} (requiere 'instp upgrade sst' para autorizar su aplicación)\n",
                pending.version
            ));
            append_plain_changes(&mut output, &pending.changes);
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
        if is_newer(current_version(), &manifest.version)? {
            return Ok(format!(
                "Canal de actualización desactualizado: publica {}, SST instalado {}.\n",
                manifest.version, current_version()
            ));
        }
        return Ok(format!(
            "SST {} ya está actualizado. Última versión publicada: {}\n",
            current_version(), manifest.version
        ));
    }

    let mut output = format!(
        "Actualización disponible: {} -> {}\n",
        current_version(), manifest.version
    );
    append_plain_changes(&mut output, &manifest.changes);

    if !stage {
        return Ok(output);
    }

    stage_manifest(paths, &manifest)?;
    // Grant handoff only AFTER the asset and updater are downloaded and
    // verified. Browsing the channel or opening the application does not
    // grant this authorization.
    EXPLICIT_INSTALL_AUTHORIZATION.store(true, Ordering::SeqCst);
    output.push_str(&format!(
        "Actualización {} descargada y verificada. Se aplicará al cerrar esta sesión de SST.\n",
        manifest.version
    ));
    Ok(output)
}

/// Read-only update discovery for the terminal's non-blocking startup notice.
/// It checks the channel manifest, but never downloads an executable,
/// grants install authorization, or schedules a replacement.
pub fn available_update_notice(paths: &AppPaths) -> Result<Option<String>> {
    if configured_manifest_url(paths).is_none() {
        return Ok(None);
    }
    let manifest = fetch_manifest(paths)?;
    validate_manifest(&manifest)?;
    if !is_newer(&manifest.version, current_version())? {
        return Ok(None);
    }
    Ok(Some(format!(
        "Nueva versión de SST disponible: {} -> {}.\nPara actualizar cuando lo decidas: instp upgrade sst",
        current_version(),
        manifest.version
    )))
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
    if let Some(updater_url) = manifest.updater_asset_url.as_deref() {
        manifest.updater_asset_url = Some(resolve_asset_url(&url, updater_url)?);
    }
    Ok(manifest)
}

fn stage_manifest(paths: &AppPaths, manifest: &UpdateManifest) -> Result<()> {
    stage_manifest_with_progress(paths, manifest, &mut |_| {})
}

fn stage_manifest_with_progress<F>(
    paths: &AppPaths,
    manifest: &UpdateManifest,
    notify: &mut F,
) -> Result<()>
where
    F: FnMut(&str),
{
    let dir = update_dir(paths);
    fs::create_dir_all(&dir)?;
    let partial = dir.join("sst.exe.part");
    let staged = dir.join(STAGED_EXE);

    // Download and authenticate the independent updater BEFORE marking the
    // SST binary as pending. An existing SST installation might not have an
    // updater yet if it was installed through the old single-exe channel.
    if let (Some(url), Some(hash), Some(size)) = (
        manifest.updater_asset_url.as_deref(),
        manifest.updater_sha256.as_deref(),
        manifest.updater_size,
    ) {
        let updater_path = independent_updater_path(paths, &manifest.version);
        let updater_partial = updater_path.with_extension("exe.part");
        let _ = fs::remove_file(&updater_partial);
        download_tracked(url, &updater_partial, Some(size), "SST updater")?;
        verify_file(&updater_partial, hash, Some(size))?;
        if updater_path.is_file() {
            let _ = fs::remove_file(&updater_path);
        }
        fs::rename(&updater_partial, &updater_path)
            .context("no se pudo activar el actualizador independiente verificado")?;
    } else {
        bail!("el canal de actualización no incluye un actualizador independiente verificable");
    }
    let _ = fs::remove_file(&partial);
    download_tracked(&manifest.asset_url, &partial, manifest.size, "SST")?;
    notify("\x1b[38;5;250mActualizaciones: verificando tamaño y SHA-256...\x1b[0m\r\n");
    verify_file(&partial, &manifest.sha256, manifest.size)?;
    let _ = fs::remove_file(&staged);
    fs::rename(&partial, &staged)?;

    let pending = PendingUpdate {
        from_version: current_version().to_owned(),
        version: manifest.version.clone(),
        sha256: manifest.sha256.to_ascii_lowercase(),
        staged_path: staged.to_string_lossy().into_owned(),
        staged_at: Utc::now().to_rfc3339(),
        updater_sha256: manifest.updater_sha256.clone(),
        changes: manifest.changes.clone(),
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
    if let Some(previous) = manifest.previous_version.as_deref()
        && !previous.trim().is_empty()
    {
        parse_version(previous)
            .with_context(|| format!("versión anterior inválida: {previous}"))?;
    }
    if manifest.channel.chars().any(char::is_control) {
        bail!("canal de actualización inválido en el manifiesto");
    }
    ensure_https(&manifest.asset_url)?;
    let updater = (
        manifest.updater_asset_url.as_ref(),
        manifest.updater_sha256.as_ref(),
        manifest.updater_size,
    );
    match updater {
        (Some(url), Some(hash), Some(size)) if size > 0 => {
            ensure_https(url)?;
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                bail!("SHA-256 del actualizador independiente inválido");
            }
        }
        _ => bail!("el manifiesto debe publicar URL, SHA-256 y tamaño del actualizador independiente"),
    }
    let hash = manifest.sha256.trim();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("SHA-256 inválido en el manifiesto");
    }
    if manifest.changes.len() > MAX_CHANGES {
        bail!("el manifiesto contiene demasiadas entradas de cambios");
    }
    for change in &manifest.changes {
        let text = change.trim();
        if text.is_empty() || text.chars().count() > MAX_CHANGE_LEN || text.chars().any(char::is_control) {
            bail!("entrada de cambios inválida en el manifiesto");
        }
    }
    Ok(())
}

fn download(url: &str, destination: &Path) -> Result<()> {
    download_impl(url, destination, None)
}

fn download_tracked(
    url: &str,
    destination: &Path,
    expected_size: Option<u64>,
    title: &str,
) -> Result<()> {
    download_impl(url, destination, Some((expected_size, title)))
}

fn download_impl(
    url: &str,
    destination: &Path,
    progress: Option<(Option<u64>, &str)>,
) -> Result<()> {
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
        "--connect-timeout",
        "5",
        "--max-time",
        "120",
        "--retry",
        "1",
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

    if let Some((total, label)) = progress {
        let output = crate::support::transfer::run(&mut command, destination, total, label)?;
        if !output.status.success() {
            bail!("curl no pudo descargar: {}", String::from_utf8_lossy(&output.stderr).trim());
        }
    } else {
        let status = command.status().context("no se pudo ejecutar curl")?;
        if !status.success() {
            bail!("curl no pudo descargar {url}");
        }
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

pub(crate) fn independent_updater_path(paths: &AppPaths, version: &str) -> PathBuf {
    // The version is validated with parse_version before it reaches staging.
    // Keep one worker per update to avoid replacing a still-running worker.
    update_dir(paths).join(format!("sst-updater-{version}.exe"))
}

pub(crate) fn verified_independent_updater(
    paths: &AppPaths,
    version: &str,
    expected_hash: &str,
) -> Result<PathBuf> {
    let worker = independent_updater_path(paths, version);
    verify_file(&worker, expected_hash, None)
        .context("actualizador independiente ausente o modificado")?;
    Ok(worker)
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

fn take_applied_receipt(paths: &AppPaths) -> Result<Option<AppliedUpdate>> {
    let path = update_dir(paths).join(APPLIED_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path)?;
    let receipt: AppliedUpdate = serde_json::from_slice(&bytes)
        .context("applied.json de update está dañado")?;
    let _ = fs::remove_file(path);
    Ok(Some(receipt))
}

fn append_plain_changes(output: &mut String, changes: &[String]) {
    if changes.is_empty() {
        return;
    }
    output.push_str("Cambios:\n");
    for change in changes {
        output.push_str("  + ");
        output.push_str(change.trim());
        output.push('\n');
    }
}


#[cfg(test)]
mod startup_policy_tests {
    #[test]
    fn embedded_terminal_does_not_launch_background_updates() {
        let worker = include_str!("../adapters/terminal/embedded.rs");
        let supervisor = include_str!("../adapters/terminal/embedded_supervised.rs");
        for source in [worker, supervisor] {
            assert!(!source.contains("application::update::startup_update("));
            assert!(!source.contains("application::update::schedule_pending_update("));
            assert!(!source.contains("update::startup_update("));
            assert!(!source.contains("update::schedule_pending_update("));
        }
    }

    #[test]
    fn native_window_only_applies_updates_explicitly_authorized_in_session() {
        let gui = include_str!("../presentation/gui_slint_wrapper.rs");
        let entry = gui.split("pub fn run() -> Result<()> {").nth(1)
            .expect("GUI startup entrypoint is present");
        let entry = entry.split("// The helper executable runs").next()
            .expect("startup function boundary is present");

        assert!(!entry.contains("startup_update("));
        assert!(!entry.contains("configured_manifest_url("));
        assert!(entry.contains("take_explicit_install_authorization()"));
        assert!(entry.contains("if result.is_ok() && update::take_explicit_install_authorization()"));
    }
}
