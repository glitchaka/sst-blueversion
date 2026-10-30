use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{adapters::persistence::AppPaths, application::update};

use super::beacon_overlay::BeaconOverlayGuard;

const UPDATE_DIR: &str = "update";
const UPDATE_TMP_DIR: &str = "tmp";
const PENDING_FILE: &str = "pending.json";
const APPLIED_STAGED_FILE: &str = "applied.pending.json";
const APPLIED_FILE: &str = "applied.json";

const HELPER_MODE_ENV: &str = "SST_INTERNAL_APPLY_UPDATE";
const HELPER_TARGET_ENV: &str = "SST_UPDATE_TARGET";
const HELPER_READY_ENV: &str = "SST_UPDATE_READY";
const HELPER_BACKUP_ENV: &str = "SST_UPDATE_BACKUP";
const HELPER_PENDING_ENV: &str = "SST_UPDATE_PENDING";
const HELPER_APPLIED_PENDING_ENV: &str = "SST_UPDATE_APPLIED_PENDING";
const HELPER_APPLIED_ENV: &str = "SST_UPDATE_APPLIED";
const HELPER_STAGED_ENV: &str = "SST_UPDATE_STAGED";
const HELPER_HASH_ENV: &str = "SST_UPDATE_SHA256";
const HELPER_CLEANUP_ENV: &str = "SST_UPDATE_HELPER_CLEANUP";

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
    if env::var_os(HELPER_MODE_ENV).is_some() {
        return run_update_helper();
    }

    schedule_helper_cleanup();

    let paths = AppPaths::detect();
    paths.ensure_layout()?;

    if schedule_pending_update(&paths)? {
        return Ok(());
    }

    if update::configured_manifest_url(&paths).is_some()
        && update::startup_update(&paths, |_| {}).is_ok()
        && schedule_pending_update(&paths)?
    {
        return Ok(());
    }

    let _beacon_overlay = BeaconOverlayGuard::start();
    let result = super::gui_slint_base::run();

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
    let target = env::current_exe().context("no se pudo resolver sst.exe")?;
    let ready = target.with_extension("exe.new");
    let backup = target.with_extension("exe.old");
    let applied_staged = update_dir.join(APPLIED_STAGED_FILE);
    let applied_file = update_dir.join(APPLIED_FILE);

    if ready != staged {
        let _ = fs::remove_file(&ready);
        fs::copy(&staged, &ready).with_context(|| {
            format!(
                "no se pudo preparar {} desde {}",
                ready.display(),
                staged.display()
            )
        })?;
    }
    verify_staged(&ready, &pending.sha256)?;

    // ReplaceFileW creates BACKUP atomically from TARGET on success. Do not
    // create or rename TARGET away beforehand: TARGET must always remain a
    // runnable SST until Windows commits the replacement.
    let _ = fs::remove_file(&backup);

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

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let helper_dir = update_dir.join(UPDATE_TMP_DIR);
        fs::create_dir_all(&helper_dir)?;
        let helper = helper_dir.join(format!(
            "sst-update-helper-{}-{}.exe",
            std::process::id(),
            Utc::now().timestamp_millis()
        ));
        let _ = fs::remove_file(&helper);
        fs::copy(&target, &helper).with_context(|| {
            format!("no se pudo crear el helper temporal {}", helper.display())
        })?;

        Command::new(&helper)
            .arg("--gui")
            .env(HELPER_MODE_ENV, "1")
            .env(HELPER_TARGET_ENV, &target)
            .env(HELPER_READY_ENV, &ready)
            .env(HELPER_BACKUP_ENV, &backup)
            .env(HELPER_PENDING_ENV, &pending_path)
            .env(HELPER_APPLIED_PENDING_ENV, &applied_staged)
            .env(HELPER_APPLIED_ENV, &applied_file)
            .env(HELPER_STAGED_ENV, &staged)
            .env(HELPER_HASH_ENV, pending.sha256.trim())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("no se pudo iniciar el helper temporal de actualización")?;
    }

    #[cfg(not(windows))]
    {
        let _ = (
            &target,
            &ready,
            &backup,
            &pending_path,
            &applied_staged,
            &applied_file,
            &staged,
        );
        bail!("la aplicación automática de updates sólo está disponible en Windows");
    }

    Ok(true)
}

fn run_update_helper() -> Result<()> {
    let target = required_env_path(HELPER_TARGET_ENV)?;
    let ready = required_env_path(HELPER_READY_ENV)?;
    let backup = required_env_path(HELPER_BACKUP_ENV)?;
    let pending = required_env_path(HELPER_PENDING_ENV)?;
    let applied_pending = required_env_path(HELPER_APPLIED_PENDING_ENV)?;
    let applied = required_env_path(HELPER_APPLIED_ENV)?;
    let staged = required_env_path(HELPER_STAGED_ENV)?;
    let expected_hash = env::var(HELPER_HASH_ENV)
        .context("falta SHA-256 para el helper de actualización")?;

    verify_staged(&ready, &expected_hash)?;

    #[cfg(windows)]
    {
        // ReplaceFileW is the commit point. While TARGET is still locked by the
        // parent process it simply fails and TARGET remains untouched. There is
        // no intermediate state where sst.exe has been deleted or renamed away.
        let mut installed = false;
        let mut last_error = None;
        for _ in 0..240 {
            match replace_file_atomic(&target, &ready, Some(&backup)) {
                Ok(()) => {
                    installed = true;
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(250));
                }
            }
        }
        if !installed {
            let detail = last_error
                .map(|error| format!(": {error:#}"))
                .unwrap_or_default();
            bail!("sst.exe no pudo reemplazarse dentro del plazo de actualización{detail}");
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (&target, &ready, &backup);
        bail!("la aplicación automática de updates sólo está disponible en Windows");
    }

    if let Err(error) = verify_staged(&target, &expected_hash) {
        #[cfg(windows)]
        {
            if backup.is_file() {
                let _ = replace_file_atomic(&target, &backup, None);
            }
        }
        return Err(error).context("el sst.exe instalado no pasó la verificación final");
    }

    let _ = fs::remove_file(&applied);
    fs::rename(&applied_pending, &applied)
        .context("no se pudo registrar la actualización aplicada")?;
    let _ = fs::remove_file(&pending);
    if staged != target && staged != ready {
        let _ = fs::remove_file(&staged);
    }

    let helper = env::current_exe().context("no se pudo resolver el helper temporal")?;
    Command::new(&target)
        .env(HELPER_CLEANUP_ENV, &helper)
        .spawn()
        .context("la actualización se aplicó, pero no se pudo relanzar SST")?;

    Ok(())
}

#[cfg(windows)]
fn replace_file_atomic(target: &Path, replacement: &Path, backup: Option<&Path>) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let target_w = wide(target);
    let replacement_w = wide(replacement);
    let backup_w = backup.map(wide);
    let backup_ptr = backup_w
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());

    let result = unsafe {
        ReplaceFileW(
            target_w.as_ptr(),
            replacement_w.as_ptr(),
            backup_ptr,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!(
                "ReplaceFileW falló al sustituir {} por {}",
                target.display(),
                replacement.display()
            )
        });
    }
    Ok(())
}

fn schedule_helper_cleanup() {
    let Some(path) = env::var_os(HELPER_CLEANUP_ENV) else {
        return;
    };
    let path = PathBuf::from(path);
    thread::spawn(move || {
        for _ in 0..80 {
            match fs::remove_file(&path) {
                Ok(()) => {
                    if let Some(parent) = path.parent() {
                        let _ = fs::remove_dir(parent);
                    }
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if let Some(parent) = path.parent() {
                        let _ = fs::remove_dir(parent);
                    }
                    return;
                }
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    });
}

fn required_env_path(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("falta variable interna de actualización {name}"))
}

fn verify_staged(path: &Path, expected_hash: &str) -> Result<()> {
    let bytes = fs::read(path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected_hash.trim()) {
        bail!("SHA-256 de actualización no coincide");
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
