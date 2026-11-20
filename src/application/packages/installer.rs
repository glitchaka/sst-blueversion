use std::{fs, path::{Path, PathBuf}};

use anyhow::Result;

use super::model::PackageManifest;

pub(super) fn install_payload(
    package: &PackageManifest,
    source: &Path,
    destination: &Path,
) -> Result<()> {
    match package.install.kind.as_str() {
        "single" => anyhow::bail!("{}: no se instalan ejecutables independientes", package.id),
        "pkgx" => super::pkgx::install_pkgx(package, source, destination),
        "zip" => anyhow::bail!("{}: ZIP de proveedor no es un paquete SST", package.id),
        "archive" => anyhow::bail!("{}: archivo externo no es un paquete SST", package.id),
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
