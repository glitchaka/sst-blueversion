//! Repair an SST installation from a verified single-use SFX payload.
//!
//! This code runs in the standalone Windows installer. It never asks SST to
//! start in order to repair SST, never launches PowerShell/CMD, and never
//! changes Defender, WDAC, ACLs, or Mark-of-the-Web security policy.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

fn unique_backup(destination: &Path, file_name: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    destination.join(format!(
        "{file_name}.repair.{}.{}.bak", timestamp, std::process::id()
    ))
}

fn move_with_rollback(from: &Path, to: &Path, old: &Path) -> Result<()> {
    if old.exists() {
        bail!("la copia de seguridad de reparación ya existe: {}", old.display());
    }
    fs::rename(to, old).with_context(|| format!(
        "no se pudo conservar el archivo original {}", to.display()
    ))?;
    if let Err(error) = fs::rename(from, to) {
        fs::rename(old, to).with_context(|| format!(
            "no se pudo restaurar {} tras un error de instalación: {error}", to.display()
        ))?;
        return Err(error).with_context(|| format!(
            "no se pudo activar {}; se restauró el archivo anterior", to.display()
        ));
    }
    Ok(())
}

fn restore_file(current: &Path, backup: &Path) -> Result<()> {
    if !backup.is_file() {
        bail!("no existe la copia anterior: {}", backup.display());
    }
    let rejected = unique_backup(
        current.parent().context("ruta del ejecutable inválida")?,
        "sst.exe.rejected",
    );
    fs::rename(current, &rejected).with_context(|| format!(
        "no se pudo preservar el ejecutable rechazado {}", current.display()
    ))?;
    if let Err(error) = fs::rename(backup, current) {
        let _ = fs::rename(&rejected, current);
        return Err(error).with_context(|| format!(
            "no se pudo restaurar el ejecutable original {}", current.display()
        ));
    }
    Ok(())
}

/// Repairs the given existing installation without touching data/, packages/,
/// user settings, backgrounds, logos, or the user's working files.
/// Both binaries are installed from the same extracted single-use SFX bundle.
pub fn repair(extracted: &Path, destination: &Path, launch: bool) -> Result<()> {
    let destination = fs::canonicalize(destination)
        .with_context(|| format!("no se pudo localizar la instalación {}", destination.display()))?;
    let installed = destination.join("sst.exe");
    let replacement = extracted.join("sst.exe");
    if !installed.is_file() {
        bail!("no existe sst.exe en {}; no se puede reparar esta instalación", destination.display());
    }
    if !replacement.is_file() || replacement.metadata()?.len() < 1024 {
        bail!("el autoextraíble no contiene un sst.exe utilizable");
    }

    let old_sst = unique_backup(&destination, "sst.exe");
    move_with_rollback(&replacement, &installed, &old_sst)?;

    let installed_updater = destination.join("sst-updater.exe");
    let replacement_updater = extracted.join("sst-updater.exe");
    let mut old_updater: Option<PathBuf> = None;
    let mut introduced_updater = false;

    if replacement_updater.is_file() {
        if installed_updater.is_file() {
            let preserved = unique_backup(&destination, "sst-updater.exe");
            if let Err(error) = move_with_rollback(&replacement_updater, &installed_updater, &preserved) {
                restore_file(&installed, &old_sst).context(
                    "se rechazó el actualizador; falló la restauración del SST anterior"
                )?;
                return Err(error).context("se restauró SST tras el fallo del actualizador");
            }
            old_updater = Some(preserved);
        } else {
            if let Err(error) = fs::rename(&replacement_updater, &installed_updater) {
                restore_file(&installed, &old_sst).context(
                    "no se pudo restaurar SST tras fallar la instalación del actualizador"
                )?;
                return Err(error).context("no se pudo instalar el actualizador independiente");
            }
            introduced_updater = true;
        }
    }

    // A successful extraction is not a successful repair: ask Windows to
    // actually CreateProcess for the new SST before finalizing.
    if launch {
        let launch_result = Command::new(&installed)
            .current_dir(&destination)
            .spawn();
        if let Err(error) = launch_result {
            let sst_rollback = restore_file(&installed, &old_sst);
            if let Some(ref old) = old_updater {
                let _ = restore_file(&installed_updater, old);
            } else if introduced_updater {
                let _ = fs::remove_file(&installed_updater);
            }
            sst_rollback.context("Windows rechazó el nuevo SST y la restauración falló")?;
            return Err(error).context(
                "Windows bloqueó el arranque del nuevo SST; se restauró automáticamente la versión anterior"
            );
        }
    }
    // Keep the old executable backup in the install directory so a future
    // repair can always revert without depending on a running SST console.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_names_do_not_overwrite_existing_legacy_rollbacks() {
        let name = unique_backup(Path::new(r"C:\SST"), "sst.exe");
        assert!(name.file_name().unwrap().to_string_lossy().starts_with("sst.exe.repair."));
        assert!(name.file_name().unwrap().to_string_lossy().ends_with(".bak"));
    }

    #[cfg(windows)]
    #[test]
    fn launch_rejection_restores_executable_without_touching_user_data() {
        let root = std::env::temp_dir().join(format!(
            "sst-repair-denied-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        let source = root.join("payload");
        let target = root.join("install");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(target.join("data")).unwrap();
        fs::write(target.join("sst.exe"), b"prior known-good bytes").unwrap();
        fs::write(target.join("data").join("config.json"), b"untouched settings").unwrap();
        fs::write(source.join("sst.exe"), vec![0u8; 4096]).unwrap();
        let outcome = repair(&source, &target, true);
        assert!(outcome.is_err(), "Windows must reject an invalid PE image");
        assert_eq!(fs::read(target.join("sst.exe")).unwrap(), b"prior known-good bytes");
        assert_eq!(fs::read(target.join("data").join("config.json")).unwrap(), b"untouched settings");
        assert!(fs::read_dir(&target).unwrap().filter_map(Result::ok).any(|f|
            f.file_name().to_string_lossy().contains("sst.exe.rejected")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn successful_repair_preserves_existing_user_settings_and_backup() {
        let root = std::env::temp_dir().join(format!(
            "sst-repair-success-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        let source = root.join("payload");
        let target = root.join("install");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(target.join("data")).unwrap();
        let windir = std::env::var_os("SystemRoot")
            .map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let where_exe = windir.join("System32").join("where.exe");
        if !where_exe.is_file() { return; }
        fs::write(target.join("sst.exe"), b"old executable").unwrap();
        fs::write(target.join("data").join("user-settings.txt"), b"keep this").unwrap();
        fs::copy(&where_exe, source.join("sst.exe")).unwrap();
        repair(&source, &target, true).unwrap();
        assert_eq!(fs::read(target.join("data").join("user-settings.txt")).unwrap(), b"keep this");
        assert_eq!(fs::metadata(target.join("sst.exe")).unwrap().len(), fs::metadata(&where_exe).unwrap().len());
        assert!(fs::read_dir(&target).unwrap().filter_map(Result::ok).any(|f|
            f.file_name().to_string_lossy().starts_with("sst.exe.repair.")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_repair_when_no_existing_sst_is_present() {
        let root = std::env::temp_dir().join(format!("sst-repair-missing-{}", std::process::id()));
        let _ = fs::create_dir_all(&root);
        let result = repair(&root, &root, false);
        assert!(result.is_err());
        let _ = fs::remove_dir_all(root);
    }
}
