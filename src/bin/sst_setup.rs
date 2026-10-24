#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    fs::{self, File},
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use zip::ZipArchive;

const FOOTER_MAGIC: &[u8; 8] = b"SSTSFX01";
const FOOTER_LEN: u64 = 16;
const INSTALL_DIR: &str = "SST";

fn main() {
    if let Err(error) = run() {
        show_error(&format!("No se pudo extraer Shell Shock Tool:\n\n{error:#}"));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let self_path = std::env::current_exe().context("no se pudo resolver el autoextraible")?;
    let parent = self_path
        .parent()
        .context("el autoextraible no tiene una carpeta contenedora")?;
    let destination = parent.join(INSTALL_DIR);

    let payload = read_appended_payload(&self_path)?;
    extract_payload(payload, &destination)?;

    let sst = destination.join("sst.exe");
    if !sst.is_file() {
        bail!("el payload no contiene sst.exe");
    }

    Command::new(&sst)
        .current_dir(&destination)
        .spawn()
        .with_context(|| format!("no se pudo iniciar {}", sst.display()))?;
    Ok(())
}

fn read_appended_payload(path: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(path)
        .with_context(|| format!("no se pudo abrir {}", path.display()))?;
    let total = file.metadata()?.len();
    if total < FOOTER_LEN {
        bail!("autoextraible incompleto: falta footer SST");
    }

    file.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut magic = [0u8; 8];
    let mut size = [0u8; 8];
    file.read_exact(&mut magic)?;
    file.read_exact(&mut size)?;
    if &magic != FOOTER_MAGIC {
        bail!("autoextraible invalido: firma de payload SST ausente");
    }

    let payload_len = u64::from_le_bytes(size);
    if payload_len == 0 || payload_len > total.saturating_sub(FOOTER_LEN) {
        bail!("autoextraible invalido: longitud de payload incorrecta");
    }
    let payload_start = total - FOOTER_LEN - payload_len;
    file.seek(SeekFrom::Start(payload_start))?;
    let mut payload = vec![0u8; payload_len as usize];
    file.read_exact(&mut payload)?;
    Ok(payload)
}

fn extract_payload(payload: Vec<u8>, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("no se pudo crear {}", destination.display()))?;

    let mut archive = ZipArchive::new(Cursor::new(payload)).context("payload ZIP invalido")?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative: PathBuf = entry
            .enclosed_name()
            .map(Path::to_path_buf)
            .context("el payload contiene una ruta insegura")?;
        let output = destination.join(relative);

        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = File::create(&output)
            .with_context(|| format!("no se pudo escribir {}", output.display()))?;
        std::io::copy(&mut entry, &mut target)?;
    }
    Ok(())
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
