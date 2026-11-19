//! SST-PACKAGE/1: native .sstpkg distribution container (gzip-compressed TAR).
//! Manifest digests are verified before a staged payload is activated by instp.
//! Payload subtrees are platform-aware: payload/win/... and payload/unix/....
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tar::Archive;

use super::model::{PackageManifest, validate_identifier};

const MAX_MANIFEST: u64 = 1024 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 10000;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: u32,
    id: String,
    version: String,
    kind: String,
    files: Vec<PayloadFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadFile {
    path: String,
    sha256: String,
}

pub(super) fn kind_folder(kind: &str) -> Result<&'static str> {
    match kind {
        "embedded" => Ok("modules"),
        "runtime" => Ok("runtimes"),
        "environment" => Ok("environments"),
        "data" => Ok("data"),
        _ => bail!("sstpkg: tipo de ejecución no admisible en un plugin: {kind}"),
    }
}

/// Reject traversal, Windows-specific metacharacters and executable programs.
/// Package modules may contain DLLs/shared libraries but never standalone EXEs.
fn valid_payload_path(path: &str) -> Result<()> {
    ensure!(!path.starts_with('/') && !path.contains('\\') && !path.contains(':')
        && !path.chars().any(|ch| ch.is_control()), "sstpkg: ruta insegura: {path}");
    let mut segments = path.split('/');
    ensure!(segments.next() == Some("payload"), "sstpkg: ruta fuera del payload");
    let target = segments.next();
    ensure!(matches!(target, Some("win" | "unix")), "sstpkg: plataforma inválida");
    let group = segments.next();
    ensure!(matches!(group, Some("modules" | "runtimes" | "environments" | "data")),
        "sstpkg: directorio de ejecución inválido");
    let id = segments.next().unwrap_or_default();
    validate_identifier(id)?;
    let remainder: Vec<_> = segments.collect();
    ensure!(!remainder.is_empty(), "sstpkg: falta contenido en {path}");
    for part in std::iter::once(id).chain(remainder.iter().copied()) {
        ensure!(!part.is_empty() && part != "." && part != ".."
            && !part.ends_with('.') && !part.ends_with(' ')
            && !part.contains(|ch: char| matches!(ch, '<' | '>' | '"' | '|' | '?' | '*')),
            "sstpkg: componente de ruta inseguro");
        let stem = part.split('.').next().unwrap_or_default().to_ascii_uppercase();
        ensure!(!matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL"
            | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9"
            | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"),
            "sstpkg: nombre reservado en Windows");
    }
    ensure!(!path.to_ascii_lowercase().ends_with(".exe")
        && !path.to_ascii_lowercase().ends_with(".com")
        && !path.to_ascii_lowercase().ends_with(".bat")
        && !path.to_ascii_lowercase().ends_with(".cmd"),
        "sstpkg: programa ejecutable externo prohibido: {path}");
    Ok(())
}

fn sha256_reader(reader: &mut impl Read, mut output: Option<&mut File>) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let len = reader.read(&mut buffer)?;
        if len == 0 { break; }
        total += len as u64;
        ensure!(total <= MAX_FILE, "sstpkg: archivo supera el tamaño permitido");
        digest.update(&buffer[..len]);
        if let Some(file) = output.as_mut() { file.write_all(&buffer[..len])?; }
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Extract selected platform subtree into an isolated staging directory.
/// Both platform trees may coexist in the archive; only the requested target
/// is deployed. All listed files, including inactive-platform files, are hashed.
pub(super) fn install_sstpkg(
    package: &PackageManifest, source: &Path, staging: &Path,
) -> Result<()> {
    let exec = package.execution.as_ref()
        .context("sstpkg: falta contrato de ejecución del paquete")?;
    let target = exec.target.as_deref().unwrap_or("win");
    ensure!(matches!(target, "win" | "unix"), "sstpkg: plataforma no reconocida");
    let folder = kind_folder(&exec.kind)?;
    let prefix = format!("payload/{target}/{folder}/{}/", package.id);
    let archive = File::open(source)?;
    let mut tar = Archive::new(GzDecoder::new(archive));
    let mut entries = tar.entries()?;
    let Some(first) = entries.next() else { bail!("sstpkg: contenedor vacío"); };
    let mut manifest_entry = first?;
    ensure!(manifest_entry.path_bytes().as_ref() == b"manifest.json",
        "sstpkg: manifest.json debe ser el primer elemento");
    ensure!(manifest_entry.header().entry_type().is_file()
        && manifest_entry.header().size()? <= MAX_MANIFEST, "sstpkg: manifiesto inválido");
    let mut bytes = Vec::new();
    manifest_entry.read_to_end(&mut bytes)?;
    let manifest: Envelope = serde_json::from_slice(&bytes)
        .context("sstpkg: no se pudo leer manifest.json")?;
    drop(manifest_entry);
    ensure!(manifest.format == 1 && manifest.id == package.id,
        "sstpkg: identidad/formato no coincide con el catálogo");
    ensure!(manifest.kind == exec.kind, "sstpkg: adaptador no coincide");
    ensure!(!manifest.version.trim().is_empty(), "sstpkg: versión vacía");
    ensure!(manifest.files.len() <= MAX_FILES && !manifest.files.is_empty(),
        "sstpkg: cantidad de archivos inválida");
    let mut expected = HashMap::new();
    let mut casefold = HashSet::new();
    for item in manifest.files {
        valid_payload_path(&item.path)?;
        ensure!(item.sha256.len() == 64 && item.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
            "sstpkg: digest SHA-256 inválido");
        ensure!(casefold.insert(item.path.to_ascii_lowercase()),
            "sstpkg: nombres duplicados (Windows ignora mayúsculas)");
        let parts = item.path.split('/').collect::<Vec<_>>();
        ensure!(parts[3] == package.id && parts[2] == folder,
            "sstpkg: archivos de otro paquete o adaptador");
        expected.insert(item.path, item.sha256.to_ascii_lowercase());
    }
    ensure!(expected.keys().any(|file| file.starts_with(&prefix)),
        "sstpkg: falta el árbol requerido {prefix}");
    let mut seen = HashSet::new();
    let mut total_bytes = 0u64;
    let mut total_entries = 0usize;
    for item in entries {
        total_entries += 1;
        ensure!(total_entries <= MAX_FILES * 2, "sstpkg: exceso de entradas");
        let mut entry = item?;
        let name = String::from_utf8(entry.path_bytes().into_owned())
            .context("sstpkg: ruta no UTF-8")?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            // Directories are structural; no links or device nodes accepted.
            let normalized = name.trim_end_matches('/');
            if normalized != "payload" && normalized != "payload/win"
                && normalized != "payload/unix" && !normalized.starts_with("payload/win/")
                && !normalized.starts_with("payload/unix/") {
                bail!("sstpkg: directorio no permitido");
            }
            continue;
        }
        ensure!(kind.is_file(), "sstpkg: enlaces o nodos especiales prohibidos");
        valid_payload_path(&name)?;
        let checksum = expected.get(&name)
            .with_context(|| format!("sstpkg: archivo no declarado en manifest.json: {name}"))?;
        ensure!(seen.insert(name.clone()), "sstpkg: archivo duplicado");
        let size = entry.header().size()?;
        ensure!(size <= MAX_FILE, "sstpkg: archivo demasiado grande");
        total_bytes = total_bytes.checked_add(size)
            .context("sstpkg: tamaño total inválido")?;
        ensure!(total_bytes <= MAX_TOTAL, "sstpkg: carga útil demasiado grande");
        let mut destination = if let Some(relative) = name.strip_prefix(&prefix) {
            let path = staging.join(relative);
            if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
            Some(File::create(path)?)
        } else {
            None
        };
        let actual = sha256_reader(&mut entry, destination.as_mut())?;
        ensure!(actual == *checksum, "sstpkg: SHA-256 no coincide para {name}");
    }
    ensure!(seen.len() == expected.len(), "sstpkg: faltan archivos declarados");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{install_sstpkg, valid_payload_path};
    use std::{fs::{self, File}, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};
    use flate2::{write::GzEncoder, Compression};
    use sha2::{Digest, Sha256};

    fn fixture(bad_digest: bool) -> (PathBuf, PathBuf, super::PackageManifest) {
        let tick = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("sstpkg-{}-{tick}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("demo.sstpkg");
        let files: [(&str, &[u8]); 2] = [
            ("payload/win/modules/demo/sub/bridge.dll", b"win bridge"),
            ("payload/unix/modules/demo/sub/bridge.so", b"unix bridge"),
        ];
        let manifest = serde_json::json!({
            "format": 1, "id": "demo", "version": "1.0", "kind": "embedded",
            "files": files.iter().map(|(path, bytes)| serde_json::json!({
                "path": path,
                "sha256": if bad_digest { "0".repeat(64) } else { format!("{:x}", Sha256::digest(bytes)) },
            })).collect::<Vec<_>>(),
        });
        let compressed = GzEncoder::new(File::create(&archive_path).unwrap(), Compression::default());
        let mut tar = tar::Builder::new(compressed);
        let mut add = |name: &str, payload: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, payload).unwrap();
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        add("manifest.json", &manifest_bytes);
        for (name, bytes) in files { add(name, bytes); }
        drop(add);
        tar.into_inner().unwrap().finish().unwrap();
        let package: super::PackageManifest = serde_json::from_value(serde_json::json!({
            "format": 1, "id": "demo", "name": "Demo", "version": "1.0",
            "description": "Prueba", "category": "module", "portability": "isolated",
            "source": {"type": "url", "url": "https://example.org/demo.sstpkg"},
            "install": {"type": "sstpkg", "expose": []},
            "execution": {"kind": "embedded", "target": "win", "entry": "sub/bridge.dll",
                "commands": ["demo"], "extensions": [".demo"],
                "abi": "sst-hostfxr/1", "startup": "load"}
        })).unwrap();
        (root, archive_path, package)
    }

    #[test]
    fn extracts_only_requested_platform_preserving_subfolders() {
        let (root, archive, mut package) = fixture(false);
        let windows = root.join("win");
        fs::create_dir_all(&windows).unwrap();
        install_sstpkg(&package, &archive, &windows).unwrap();
        assert_eq!(fs::read(windows.join("sub/bridge.dll")).unwrap(), b"win bridge");
        assert!(!windows.join("sub/bridge.so").exists());

        package.execution.as_mut().unwrap().target = Some("unix".to_owned());
        let unix = root.join("unix");
        fs::create_dir_all(&unix).unwrap();
        install_sstpkg(&package, &archive, &unix).unwrap();
        assert_eq!(fs::read(unix.join("sub/bridge.so")).unwrap(), b"unix bridge");
        assert!(!unix.join("sub/bridge.dll").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_payload_with_wrong_sha256() {
        let (root, archive, package) = fixture(true);
        let staging = root.join("staging");
        fs::create_dir_all(&staging).unwrap();
        assert!(install_sstpkg(&package, &archive, &staging).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_programs_and_path_traversal() {
        assert!(valid_payload_path("payload/win/modules/powershell/bridge.dll").is_ok());
        assert!(valid_payload_path("payload/unix/environments/arch/etc/pacman.conf").is_ok());
        for name in [
            "payload/win/modules/ps/../escape", "payload/win/tools/x/other.exe",
            "payload/win/modules/ps/CON", "payload/unix/modules/ps\\bad.so",
            "payload/win/modules/ps/a:stream", "/payload/win/modules/ps/a",
        ] { assert!(valid_payload_path(name).is_err(), "{name}"); }
    }
}
