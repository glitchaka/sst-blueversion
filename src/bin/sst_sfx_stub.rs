#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    ffi::OsString,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};

const PACKAGE_MAGIC: &[u8; 8] = b"SSTPKG01";
const SFX_MAGIC: &[u8; 8] = b"SSTSFX02";
const PACKAGE_VERSION: u32 = 1;
const FOOTER_LEN: u64 = 16;
const INSTALL_DIR: &str = "SST";
const MAX_FILES: u32 = 100_000;
const MAX_PATH_BYTES: u32 = 1024 * 1024;

fn main() {
    let verify_mode = std::env::args_os().skip(1).any(|arg| arg == "--extract-only");
    if let Err(error) = run() {
        if !verify_mode {
            show_error(&format!("No se pudo extraer Shell Shock Tool:\n\n{error:#}"));
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let self_path = std::env::current_exe().context("no se pudo resolver el autoextraible")?;
    let parent = self_path
        .parent()
        .context("el autoextraible no tiene una carpeta contenedora")?;

    let (destination, launch) = resolve_destination(parent)?;
    extract_bundle(&self_path, &destination)?;

    let sst = destination.join("sst.exe");
    if !sst.is_file() {
        bail!("el bundle no contiene sst.exe");
    }

    if launch {
        Command::new(&sst)
            .current_dir(&destination)
            .spawn()
            .with_context(|| format!("no se pudo iniciar {}", sst.display()))?;
    }

    Ok(())
}

fn resolve_destination(parent: &Path) -> Result<(PathBuf, bool)> {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        None => Ok((parent.join(INSTALL_DIR), true)),
        Some(flag) if flag == OsString::from("--extract-only") => {
            let destination = args
                .next()
                .map(PathBuf::from)
                .context("--extract-only requiere una carpeta de destino")?;
            if args.next().is_some() {
                bail!("--extract-only acepta una sola carpeta de destino");
            }
            Ok((destination, false))
        }
        Some(other) => bail!(
            "argumento no reconocido para SST Setup: {}",
            other.to_string_lossy()
        ),
    }
}

fn extract_bundle(executable: &Path, destination: &Path) -> Result<()> {
    if destination.as_os_str().is_empty() || destination == destination.ancestors().last().unwrap_or(destination) {
        bail!("ruta de destino insegura");
    }

    let mut input = File::open(executable)
        .with_context(|| format!("no se pudo abrir {}", executable.display()))?;
    let total = input.metadata()?.len();
    if total < FOOTER_LEN {
        bail!("autoextraible incompleto: falta footer SST");
    }

    input.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut footer_magic = [0u8; 8];
    input.read_exact(&mut footer_magic)?;
    if &footer_magic != SFX_MAGIC {
        bail!("este archivo es el stub interno de SST y no contiene el bundle distribuible");
    }
    let package_offset = read_u64(&mut input)?;
    if package_offset >= total.saturating_sub(FOOTER_LEN) {
        bail!("autoextraible invalido: offset de bundle fuera de rango");
    }

    input.seek(SeekFrom::Start(package_offset))?;
    let mut package_magic = [0u8; 8];
    input.read_exact(&mut package_magic)?;
    if &package_magic != PACKAGE_MAGIC {
        bail!("autoextraible invalido: cabecera de bundle SST ausente");
    }

    let version = read_u32(&mut input)?;
    if version != PACKAGE_VERSION {
        bail!("version de bundle SST no soportada: {version}");
    }
    let count = read_u32(&mut input)?;
    if count == 0 || count > MAX_FILES {
        bail!("cantidad de archivos invalida en bundle: {count}");
    }

    fs::create_dir_all(destination)
        .with_context(|| format!("no se pudo crear {}", destination.display()))?;

    let payload_end = total - FOOTER_LEN;
    let mut buffer = vec![0u8; 1024 * 1024];
    for _ in 0..count {
        let path_len = read_u32(&mut input)?;
        let size = read_u64(&mut input)?;
        if path_len == 0 || path_len > MAX_PATH_BYTES {
            bail!("ruta invalida dentro del bundle");
        }

        let mut path_bytes = vec![0u8; path_len as usize];
        input.read_exact(&mut path_bytes)?;
        let path_text = std::str::from_utf8(&path_bytes)
            .context("ruta UTF-8 invalida dentro del bundle")?;
        let relative = PathBuf::from(path_text);
        validate_relative_path(&relative)?;

        let current = input.stream_position()?;
        if size > payload_end.saturating_sub(current) {
            bail!("bundle truncado al extraer {}", relative.display());
        }

        let output = destination.join(&relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = File::create(&output)
            .with_context(|| format!("no se pudo escribir {}", output.display()))?;

        let mut remaining = size;
        while remaining > 0 {
            let want = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
            input.read_exact(&mut buffer[..want])?;
            target.write_all(&buffer[..want])?;
            remaining -= want as u64;
        }
    }

    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("ruta insegura en bundle: {}", path.display());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("ruta insegura en bundle: {}", path.display()),
        }
    }
    Ok(())
}

fn read_u32(input: &mut impl Read) -> Result<u32> {
    let mut bytes = [0u8; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(input: &mut impl Read) -> Result<u64> {
    let mut bytes = [0u8; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(windows)]
fn show_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "SST Setup".encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_error(message: &str) {
    eprintln!("{message}");
}
