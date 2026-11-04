use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

use super::{model::PackageManifest, transport::configure_hidden};

pub(super) fn install_payload(
    package: &PackageManifest,
    source: &Path,
    destination: &Path,
) -> Result<()> {
    match package.install.kind.as_str() {
        "single" => {
            let name = package
                .install
                .rename
                .as_deref()
                .or_else(|| source.file_name().and_then(|value| value.to_str()))
                .unwrap_or(&package.id);
            fs::copy(source, destination.join(name))?;
            Ok(())
        }
        "zip" => extract_zip(source, destination),
        "archive" => extract_archive(source, destination),
        "system" => anyhow::bail!("{} requiere instalación de sistema", package.id),
        other => anyhow::bail!("{}: install.type no soportado: {other}", package.id),
    }
}

pub(super) fn find_file(root: &Path, requested: &str) -> Result<Option<PathBuf>> {
    let direct = root.join(requested);
    if direct.is_file() {
        return Ok(Some(direct));
    }

    let requested_name = Path::new(requested)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(requested);
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(requested_name))
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

pub(super) fn command_name_from_exposed(exposed: &str) -> String {
    Path::new(exposed)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(exposed)
        .to_owned()
}

fn extract_zip(source: &Path, destination: &Path) -> Result<()> {
    let file = File::open(source)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&output)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }
    Ok(())
}

fn extract_archive(source: &Path, destination: &Path) -> Result<()> {
    let mut command = Command::new(tar_program()?);
    command.arg("-xf").arg(source).arg("-C").arg(destination);
    configure_hidden(&mut command);
    let output = command.output().context("instp: no se pudo ejecutar tar")?;
    if !output.status.success() {
        anyhow::bail!(
            "instp: no se pudo extraer {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub(super) fn tar_program() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        crate::support::windows::system32_executable("tar.exe")
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from("tar"))
    }
}
