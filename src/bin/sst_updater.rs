#![cfg_attr(windows, windows_subsystem = "windows")]
// Independent SST update worker. Its executable is NOT sst.exe, so it can
// replace SST without copying/running SST in a private "helper" mode.

use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

const PARENT: &str = "SST_UPDATER_PARENT_PID";
const TARGET: &str = "SST_UPDATER_TARGET";
const READY: &str = "SST_UPDATER_READY";
const BACKUP: &str = "SST_UPDATER_BACKUP";
const PENDING: &str = "SST_UPDATER_PENDING";
const APPLIED_PENDING: &str = "SST_UPDATER_APPLIED_PENDING";
const APPLIED: &str = "SST_UPDATER_APPLIED";
const STAGED: &str = "SST_UPDATER_STAGED";
const SHA256: &str = "SST_UPDATER_SHA256";

fn main() {
    if let Err(error) = run() {
        // A background worker has no console or message box. Keep diagnostics
        // under data/update so the next SST invocation can report them.
        let message = format!("{error:#}\n");
        if let Some(dir) = env::var_os(PENDING)
            .and_then(|path| PathBuf::from(path).parent().map(Path::to_path_buf))
        {
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(dir.join("updater-error.log"), &message);
        }
        eprintln!("SST updater: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match env::args().nth(1).as_deref() {
        Some("--version") => {
            println!("SST updater {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--apply") if env::args().len() == 2 => {}
        _ => bail!("uso: sst-updater.exe --apply"),
    }

    let parent: u32 = env::var(PARENT)
        .context("falta PID de SST")?
        .parse().context("PID de SST inválido")?;
    if parent == 0 || parent == std::process::id() {
        bail!("PID inválido para el reemplazo");
    }

    let target = required_path(TARGET)?;
    let ready = required_path(READY)?;
    let backup = required_path(BACKUP)?;
    let pending = required_path(PENDING)?;
    let applied_pending = required_path(APPLIED_PENDING)?;
    let applied = required_path(APPLIED)?;
    let staged = required_path(STAGED)?;
    let expected_sha = env::var(SHA256).context("falta SHA-256 de actualización")?;
    if expected_sha.len() != 64 || !expected_sha.bytes().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("SHA-256 de actualización inválido");
    }

    validate_paths(&target, &ready, &backup)?;
    verify_sha256(&ready, &expected_sha)?;

    #[cfg(windows)]
    {
        wait_for_parent(parent)?;
        let mut committed = false;
        let mut last_error = None;
        for _ in 0..40 {
            match replace_file(&target, &ready, Some(&backup)) {
                Ok(()) => {
                    committed = true;
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(250));
                }
            }
        }
        if !committed {
            return Err(last_error.context("no se pudo reemplazar SST")?);
        }
    }
    #[cfg(not(windows))]
    bail!("el actualizador independiente necesita Windows");

    if let Err(error) = verify_sha256(&target, &expected_sha) {
        #[cfg(windows)]
        if backup.is_file() {
            let _ = replace_file(&target, &backup, None);
        }
        return Err(error).context("el ejecutable actualizado no pasó el control de integridad");
    }

    // Never leave a successfully updated SST unlaunchable because metadata or
    // telemetry could not be written. Retain the .old backup for recovery.
    if applied_pending.is_file() {
        let _ = fs::remove_file(&applied);
        if let Err(error) = fs::rename(&applied_pending, &applied) {
            eprintln!("SST updater: recibo no guardado: {error}");
        }
    }
    let _ = fs::remove_file(&pending);
    if staged != target && staged != ready {
        let _ = fs::remove_file(&staged);
    }

    let mut launch = Command::new(&target);
    for key in [
        PARENT, TARGET, READY, BACKUP, PENDING, APPLIED_PENDING, APPLIED,
        STAGED, SHA256, "SST_INTERNAL_APPLY_UPDATE", "SST_UPDATE_PARENT_PID",
        "SST_UPDATE_TARGET", "SST_UPDATE_READY", "SST_UPDATE_BACKUP",
        "SST_UPDATE_PENDING", "SST_UPDATE_APPLIED_PENDING",
        "SST_UPDATE_APPLIED", "SST_UPDATE_STAGED", "SST_UPDATE_SHA256",
        "SST_UPDATE_HELPER_CLEANUP",
    ] {
        launch.env_remove(key);
    }
    launch.spawn().context("SST se actualizó pero no pudo relanzarse")?;
    Ok(())
}

fn required_path(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("falta variable de actualización: {name}"))
}

fn validate_paths(target: &Path, ready: &Path, backup: &Path) -> Result<()> {
    let installed = env::current_exe().context("no se pudo localizar sst-updater.exe")?;
    let root = installed.parent().context("el actualizador no tiene carpeta")?;
    let canonical_root = fs::canonicalize(root)?;
    let canonical_target_dir = fs::canonicalize(
        target.parent().context("sst.exe no tiene carpeta")?
    )?;
    if canonical_root != canonical_target_dir {
        bail!("el actualizador sólo puede sustituir SST en su propia carpeta");
    }
    let target_name = target.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let ready_name = ready.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let backup_name = backup.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if !target_name.eq_ignore_ascii_case("sst.exe")
        || !ready_name.eq_ignore_ascii_case("sst.exe.new")
        || !backup_name.eq_ignore_ascii_case("sst.exe.old")
        || ready.parent() != target.parent()
        || backup.parent() != target.parent()
    {
        bail!("rutas de reemplazo SST inconsistentes");
    }
    if !target.is_file() || !ready.is_file() {
        bail!("no existe el ejecutable instalado o la actualización preparada");
    }
    Ok(())
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        digest.update(&buffer[..count]);
    }
    if !format!("{:x}", digest.finalize()).eq_ignore_ascii_case(expected.trim()) {
        bail!("el SHA-256 no coincide para {}", path.display());
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_parent(pid: u32) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, WaitForSingleObject},
    };
    const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
    const WAIT_OBJECT_0_VALUE: u32 = 0;
    const WAIT_TIMEOUT_VALUE: u32 = 0x102;
    const WAIT_FAILED_VALUE: u32 = 0xffff_ffff;
    let handle = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, pid) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        // Win32 ERROR_INVALID_PARAMETER: process has already terminated.
        if error.raw_os_error() == Some(87) { return Ok(()); }
        return Err(error).context("no se pudo observar el cierre de SST");
    }
    let outcome = unsafe { WaitForSingleObject(handle, 60_000) };
    unsafe { let _ = CloseHandle(handle); }
    match outcome {
        WAIT_OBJECT_0_VALUE => Ok(()),
        WAIT_TIMEOUT_VALUE => bail!("SST no terminó dentro del plazo de actualización"),
        WAIT_FAILED_VALUE => Err(std::io::Error::last_os_error())
            .context("error esperando el cierre de SST"),
        other => bail!("WaitForSingleObject devolvió 0x{other:08x}"),
    }
}

#[cfg(windows)]
fn replace_file(target: &Path, ready: &Path, backup: Option<&Path>) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    let target_w = wide(target);
    let ready_w = wide(ready);
    let backup_w = backup.map(wide);
    let backup_ptr = backup_w
        .as_ref()
        .map_or(std::ptr::null(), |w| w.as_ptr());
    let result = unsafe {
        ReplaceFileW(
            target_w.as_ptr(), ready_w.as_ptr(), backup_ptr, 0,
            std::ptr::null_mut(), std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("no se pudo reemplazar {}", target.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_invalid_hex_hash() {
        assert_ne!("abc".len(), 64);
        let dir = std::env::temp_dir().join(format!("sst-updater-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("sample");
        fs::write(&file, b"abc").unwrap();
        assert!(verify_sha256(&file, &"0".repeat(64)).is_err());
        let expected = format!("{:x}", Sha256::digest(b"abc"));
        assert!(verify_sha256(&file, &expected).is_ok());
        let _ = fs::remove_file(file);
    }
}
