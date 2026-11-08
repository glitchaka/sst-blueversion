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


const UPDATE_DIR: &str = "update";
const PENDING_FILE: &str = "pending.json";
const APPLIED_STAGED_FILE: &str = "applied.pending.json";
const APPLIED_FILE: &str = "applied.json";

const HELPER_MODE_ENV: &str = "SST_INTERNAL_APPLY_UPDATE";
const HELPER_PARENT_PID_ENV: &str = "SST_UPDATE_PARENT_PID";
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
    updater_sha256: Option<String>,
    #[serde(default)]
    changes: Vec<String>,
}

pub fn run() -> Result<()> {
    if env::var_os(HELPER_MODE_ENV).is_some() {
        // An older updater launches its newly installed SST while retaining
        // its own helper environment. That new SST must boot normally, not
        // try to apply sst.exe.new a second time after it was consumed.
        if inherited_update_helper_mode() {
            clear_inherited_update_helper_mode();
        } else {
            return run_update_helper();
        }
    }

    schedule_helper_cleanup();

    let paths = AppPaths::detect();
    paths.ensure_layout()?;

    // Opening SST must be deterministic and offline-safe: never fetch, stage,
    // apply, or schedule an update before the terminal is shown.
    // A previous pending.json is inert unless the operator explicitly invokes
    // "update install" in THIS GUI session.
    let result = super::gui_slint_base::run();

    if result.is_ok() && update::take_explicit_install_authorization() {
        schedule_pending_update(&paths)?;
    }
    result
}

// The helper executable runs from update/tmp, while a successful update
// re-launches the installed executable at SST_UPDATE_TARGET. Check the
// identity of the running executable before trusting the inherited mode flag.
fn inherited_update_helper_mode() -> bool {
    let Some(target) = env::var_os(HELPER_TARGET_ENV) else {
        return false;
    };
    let Ok(actual) = env::current_exe() else {
        return false;
    };
    same_installed_executable(&actual, Path::new(&target))
}

fn same_installed_executable(actual: &Path, target: &Path) -> bool {
    match (fs::canonicalize(actual), fs::canonicalize(target)) {
        (Ok(actual), Ok(target)) => {
            #[cfg(windows)]
            {
                actual.to_string_lossy().eq_ignore_ascii_case(&target.to_string_lossy())
            }
            #[cfg(not(windows))]
            {
                actual == target
            }
        }
        _ => false,
    }
}

// Must execute before creating GUI threads. Preserve the one-shot cleanup
// instruction but remove the helper's arguments and mode from the main SST.
fn clear_inherited_update_helper_mode() {
    // SAFETY: This function runs at GUI startup before launching threads.
    // Mutating the process environment in this single-threaded startup phase
    // prevents stale updater variables leaking into future subprocesses.
    unsafe {
        for variable in [
            HELPER_MODE_ENV,
            HELPER_PARENT_PID_ENV,
            HELPER_TARGET_ENV,
            HELPER_READY_ENV,
            HELPER_BACKUP_ENV,
            HELPER_PENDING_ENV,
            HELPER_APPLIED_PENDING_ENV,
            HELPER_APPLIED_ENV,
            HELPER_STAGED_ENV,
            HELPER_HASH_ENV,
        ] {
            env::remove_var(variable);
        }
    }
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
    // A previous update may have consumed the staged executable before
    // recording completion. Broken staging must not prevent an explicitly
    // requested update after the operator closes this session.
    if !staged.is_file() {
        let _ = fs::remove_file(&pending_path);
        return Ok(false);
    }
    verify_staged(&staged, &pending.sha256)?;

    // The separate updater is mandatory for new handoffs. Legacy pending
    // data has no updater SHA-256; don't apply it using a copy of SST.
    let Some(updater_sha256) = pending.updater_sha256.as_deref() else {
        return Ok(false);
    };
    let updater = match update::verified_independent_updater(paths, &pending.version, updater_sha256) {
        Ok(worker) => worker,
        Err(error) => {
            let _ = fs::write(update_dir.join("updater-error.log"), format!("{error:#}\n"));
            return Ok(false);
        }
    };

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

    // Keep the current and previous rollback copies until the independent
    // updater has atomically committed the replacement.

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

        Command::new(&updater)
            .arg("--apply")
            .env("SST_UPDATER_PARENT_PID", std::process::id().to_string())
            .env("SST_UPDATER_TARGET", &target)
            .env("SST_UPDATER_READY", &ready)
            .env("SST_UPDATER_BACKUP", &backup)
            .env("SST_UPDATER_PENDING", &pending_path)
            .env("SST_UPDATER_APPLIED_PENDING", &applied_staged)
            .env("SST_UPDATER_APPLIED", &applied_file)
            .env("SST_UPDATER_STAGED", &staged)
            .env("SST_UPDATER_SHA256", pending.sha256.trim())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("no se pudo iniciar sst-updater.exe")?;
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
    let parent_pid = env::var(HELPER_PARENT_PID_ENV)
        .context("falta PID de la instancia SST que entregó la actualización")?
        .parse::<u32>()
        .context("PID interno de actualización inválido")?;
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
        // File availability is not a reliable proxy for process lifetime on
        // Windows. Wait for the exact SST instance that handed off the update
        // to terminate before touching the installed executable.
        wait_for_process_exit(parent_pid)?;

        // ReplaceFileW is the commit point. TARGET remains runnable until the
        // replacement succeeds; if Windows rejects the operation, nothing is
        // removed or renamed away first.
        let mut installed = false;
        let mut last_error = None;
        for _ in 0..40 {
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
            bail!("sst.exe no pudo reemplazarse después de cerrar la instancia anterior{detail}");
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (&parent_pid, &target, &ready, &backup);
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
    updated_sst_command(&target, &helper)
        .spawn()
        .context("la actualización se aplicó, pero no se pudo relanzar SST")?;

    Ok(())
}

#[cfg(windows)]
fn wait_for_process_exit(pid: u32) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, WaitForSingleObject},
    };

    const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
    const WAIT_OBJECT_0_VALUE: u32 = 0x0000_0000;
    const WAIT_TIMEOUT_VALUE: u32 = 0x0000_0102;
    const WAIT_FAILED_VALUE: u32 = 0xffff_ffff;
    const PARENT_EXIT_TIMEOUT_MS: u32 = 60_000;

    let handle = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, pid) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        // ERROR_INVALID_PARAMETER means the process already ceased to exist
        // before the helper could open a synchronization handle.
        if error.raw_os_error() == Some(87) {
            return Ok(());
        }
        return Err(error).with_context(|| format!("no se pudo observar el proceso SST padre {pid}"));
    }

    let wait = unsafe { WaitForSingleObject(handle, PARENT_EXIT_TIMEOUT_MS) };
    unsafe {
        let _ = CloseHandle(handle);
    }

    match wait {
        WAIT_OBJECT_0_VALUE => Ok(()),
        WAIT_TIMEOUT_VALUE => bail!("la instancia SST anterior no terminó dentro de 60 segundos"),
        WAIT_FAILED_VALUE => Err(std::io::Error::last_os_error())
            .context("falló la espera del cierre de la instancia SST anterior"),
        other => bail!("WaitForSingleObject devolvió un estado inesperado: 0x{other:08x}"),
    }
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

// An update helper inherits SST_INTERNAL_APPLY_UPDATE=1 to apply the
// replacement. NEVER pass that mode or its one-shot arguments to the
// relaunched application: sst.exe.new is consumed by ReplaceFileW.
fn updated_sst_command(target: &Path, helper: &Path) -> Command {
    let mut command = Command::new(target);
    for variable in [
        HELPER_MODE_ENV,
        HELPER_PARENT_PID_ENV,
        HELPER_TARGET_ENV,
        HELPER_READY_ENV,
        HELPER_BACKUP_ENV,
        HELPER_PENDING_ENV,
        HELPER_APPLIED_PENDING_ENV,
        HELPER_APPLIED_ENV,
        HELPER_STAGED_ENV,
        HELPER_HASH_ENV,
        HELPER_CLEANUP_ENV,
    ] {
        command.env_remove(variable);
    }
    command.env(HELPER_CLEANUP_ENV, helper);
    command
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

#[cfg(test)]
mod updater_relaunch_tests {
    use super::*;

    #[test]
    fn identifies_the_installed_binary_in_an_old_helper_relaunch() {
        let current = std::env::current_exe().expect("current exe");
        assert!(same_installed_executable(&current, &current));
        assert!(!same_installed_executable(
            &current,
            &current.with_file_name("missing-sst-update-helper.exe")
        ));
    }

    #[test]
    fn relaunched_app_does_not_inherit_update_helper_mode() {
        let target = Path::new("sst.exe");
        let helper = Path::new("sst-update-helper.exe");
        let cmd = updated_sst_command(target, helper);
        let variables: std::collections::HashMap<_, _> = cmd.get_envs().collect();

        for variable in [
            HELPER_MODE_ENV,
            HELPER_PARENT_PID_ENV,
            HELPER_TARGET_ENV,
            HELPER_READY_ENV,
            HELPER_BACKUP_ENV,
            HELPER_PENDING_ENV,
            HELPER_APPLIED_PENDING_ENV,
            HELPER_APPLIED_ENV,
            HELPER_STAGED_ENV,
            HELPER_HASH_ENV,
        ] {
            assert_eq!(variables.get(std::ffi::OsStr::new(variable)), Some(&None));
        }
        assert_eq!(
            variables.get(std::ffi::OsStr::new(HELPER_CLEANUP_ENV)),
            Some(&Some(helper.as_os_str()))
        );
    }
}
