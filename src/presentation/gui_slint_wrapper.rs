use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    adapters::persistence::AppPaths,
    application::update,
    support::windows::system32_executable,
};

use super::beacon_overlay::BeaconOverlayGuard;

const UPDATE_DIR: &str = "update";
const PENDING_FILE: &str = "pending.json";
const APPLIED_STAGED_FILE: &str = "applied.pending.json";
const APPLIED_FILE: &str = "applied.json";
const APPLY_SCRIPT: &str = "apply-update.cmd";
const SAFE_APPLY_SCRIPT: &str = "apply-update-safe.cmd";

#[derive(Debug, Deserialize)]
struct PendingUpdate {
    #[serde(default)]
    from_version: String,
    version: String,
    sha256: String,
    staged_path: String,
    #[serde(default)]
    changes: Vec<String>,
}

pub fn run() -> Result<()> {
    let paths = AppPaths::detect();
    update::spawn_background_check(paths.clone());

    let _beacon_overlay = BeaconOverlayGuard::start();
    let result = super::gui_slint_base::run();

    // Keep the legacy handoff referenced while the GUI uses the hardened
    // console-independent path below. This can be removed when update.rs is
    // folded onto the same helper implementation.
    let _legacy_handoff: fn(&AppPaths) -> Result<bool> = update::schedule_pending_update;

    if result.is_ok() {
        schedule_pending_update(&paths)?;
    }
    result
}

fn schedule_pending_update(paths: &AppPaths) -> Result<bool> {
    let update_dir = paths.data_dir().join(UPDATE_DIR);
    let pending_path = update_dir.join(PENDING_FILE);
    if !pending_path.is_file() {
        return Ok(false);
    }

    let pending_bytes = fs::read(&pending_path)
        .with_context(|| format!("no se pudo leer {}", pending_path.display()))?;
    let pending: PendingUpdate = serde_json::from_slice(&pending_bytes)
        .context("pending.json de update está dañado")?;

    if !is_newer(&pending.version, update::current_version())? {
        return Ok(false);
    }

    let staged = PathBuf::from(&pending.staged_path);
    verify_staged(&staged, &pending.sha256)?;

    fs::create_dir_all(&update_dir)?;
    let target = std::env::current_exe().context("no se pudo resolver sst.exe")?;
    let backup = target.with_extension("exe.old");
    let applied_staged = update_dir.join(APPLIED_STAGED_FILE);
    let applied_file = update_dir.join(APPLIED_FILE);
    let legacy_script = update_dir.join(APPLY_SCRIPT);
    let safe_script = update_dir.join(SAFE_APPLY_SCRIPT);
    let ping = system32_executable("ping.exe")?;

    let from_version = if pending.from_version.trim().is_empty() {
        update::current_version().to_owned()
    } else {
        pending.from_version.clone()
    };
    let receipt = serde_json::json!({
        "from_version": from_version,
        "version": pending.version,
        "applied_at": Utc::now().to_rfc3339(),
        "changes": pending.changes,
    });
    fs::write(&applied_staged, serde_json::to_vec_pretty(&receipt)?)?;

    // Do not use timeout.exe here. SST is a Windows GUI subsystem process and
    // the helper is deliberately spawned with CREATE_NO_WINDOW; timeout.exe can
    // fail immediately without a usable console/input handle. ping.exe provides
    // a console-independent one-second delay while we wait for Windows to release
    // the running image before replacing it.
    let body = format!(
        "@echo off\r\n\
         setlocal DisableDelayedExpansion\r\n\
         set \"TARGET={}\"\r\n\
         set \"NEW={}\"\r\n\
         set \"BACKUP={}\"\r\n\
         set \"PENDING={}\"\r\n\
         set \"APPLIED_PENDING={}\"\r\n\
         set \"APPLIED={}\"\r\n\
         set \"LEGACY_SCRIPT={}\"\r\n\
         set \"PING={}\"\r\n\
         del /Q \"%BACKUP%\" >nul 2>&1\r\n\
         for /L %%I in (1,1,120) do (\r\n\
           move /Y \"%TARGET%\" \"%BACKUP%\" >nul 2>&1 && goto replace\r\n\
           \"%PING%\" -n 2 -w 1000 127.0.0.1 >nul 2>&1\r\n\
         )\r\n\
         exit /b 10\r\n\
         :replace\r\n\
         move /Y \"%NEW%\" \"%TARGET%\" >nul 2>&1\r\n\
         if errorlevel 1 (\r\n\
           move /Y \"%BACKUP%\" \"%TARGET%\" >nul 2>&1\r\n\
           exit /b 11\r\n\
         )\r\n\
         del /Q \"%BACKUP%\" >nul 2>&1\r\n\
         move /Y \"%APPLIED_PENDING%\" \"%APPLIED%\" >nul 2>&1\r\n\
         del /Q \"%PENDING%\" >nul 2>&1\r\n\
         del /Q \"%LEGACY_SCRIPT%\" >nul 2>&1\r\n\
         start \"\" \"%TARGET%\"\r\n\
         del /Q \"%~f0\" >nul 2>&1\r\n",
        batch_value(&target),
        batch_value(&staged),
        batch_value(&backup),
        batch_value(&pending_path),
        batch_value(&applied_staged),
        batch_value(&applied_file),
        batch_value(&legacy_script),
        batch_value(&ping),
    );
    fs::write(&safe_script, body)?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let cmd = system32_executable("cmd.exe")?;
        Command::new(cmd)
            .args(["/D", "/Q", "/C", "call"])
            .arg(&safe_script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("no se pudo iniciar el aplicador seguro de actualización")?;
    }

    Ok(true)
}

fn verify_staged(path: &Path, expected_hash: &str) -> Result<()> {
    let bytes = fs::read(path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected_hash.trim()) {
        anyhow::bail!("SHA-256 de actualización no coincide");
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

fn batch_value(path: &Path) -> String {
    path.to_string_lossy().replace('%', "%%").replace('"', "\"\"")
}
