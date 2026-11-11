#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    env,
    ffi::OsString,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};

#[path = "../support/sfx_repair.rs"]
mod repair;

const PACKAGE_MAGIC: &[u8; 8] = b"SSTPKG01";
const SFX_MAGIC: &[u8; 8] = b"SSTSFX03";
const PACKAGE_VERSION: u32 = 1;
const PACKAGE_ID_LEN: usize = 16;
const FOOTER_LEN: u64 = 32;
const INSTALL_DIR: &str = "SST";
const MAX_FILES: u32 = 100_000;
const MAX_PATH_BYTES: u32 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct SfxMetadata {
    package_offset: u64,
    package_id: [u8; PACKAGE_ID_LEN],
}

enum Claim {
    Acquired(ConsumptionGuard),
    AlreadyUsed,
}

struct ConsumptionGuard {
    pending: PathBuf,
    used: PathBuf,
    committed: bool,
}

impl ConsumptionGuard {
    fn acquire(package_id: &[u8; PACKAGE_ID_LEN]) -> Result<Claim> {
        let root = consumption_root()?;
        let id = package_id_hex(package_id);
        let used = root.join(format!("{id}.used"));
        if used.is_file() {
            return Ok(Claim::AlreadyUsed);
        }

        let pending = root.join(format!("{id}.pending"));
        match OpenOptions::new().write(true).create_new(true).open(&pending) {
            Ok(mut file) => {
                if used.is_file() {
                    drop(file);
                    let _ = fs::remove_file(&pending);
                    return Ok(Claim::AlreadyUsed);
                }
                if let Err(error) = writeln!(file, "pid={}", std::process::id()) {
                    drop(file);
                    let _ = fs::remove_file(&pending);
                    return Err(error).context("no se pudo registrar la reserva del paquete");
                }
                if let Err(error) = file.flush() {
                    drop(file);
                    let _ = fs::remove_file(&pending);
                    return Err(error).context("no se pudo confirmar la reserva del paquete");
                }
                Ok(Claim::Acquired(Self {
                    pending,
                    used,
                    committed: false,
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!("este paquete ya tiene una extracción en curso")
            }
            Err(error) => Err(error).context("no se pudo reservar el uso único del paquete"),
        }
    }

    fn commit(mut self) -> Result<()> {
        fs::rename(&self.pending, &self.used).with_context(|| {
            format!(
                "no se pudo marcar el paquete como consumido: {}",
                self.used.display()
            )
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for ConsumptionGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.pending);
        }
    }
}

fn main() {
    let verify_mode = env::args_os().skip(1).any(|arg| arg == "--extract-only");
    if let Err(error) = run() {
        if !verify_mode {
            show_error(&format!("No se pudo completar SST Setup:\n\n{error:#}"));
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let self_path = env::current_exe().context("no se pudo resolver el autoextraible")?;
    let parent = self_path
        .parent()
        .context("el autoextraible no tiene una carpeta contenedora")?;
    let metadata = read_sfx_metadata(&self_path)?;

    let claim = ConsumptionGuard::acquire(&metadata.package_id)?;
    let guard = match claim {
        Claim::Acquired(guard) => guard,
        Claim::AlreadyUsed => {
            let _ = schedule_self_delete(&self_path, &metadata.package_id);
            bail!("este autoextraible ya fue utilizado y no admite una segunda extracción");
        }
    };

    let (destination, launch, repair_mode) = resolve_destination(parent)?;
    if repair_mode {
        // Extract to an isolated directory on the same volume. The installed
        // configuration, user data and background image are never overwritten.
        let staged = destination.join(format!(
            ".sst-repair-stage-{}", std::process::id()
        ));
        if staged.exists() {
            bail!("existe una reparación anterior pendiente: {}", staged.display());
        }
        let operation = (|| -> Result<()> {
            extract_bundle(&self_path, &staged, metadata.package_offset)?;
            repair::repair(&staged, &destination, launch)
        })();
        let _ = fs::remove_dir_all(&staged);
        operation?;
    } else {
        extract_bundle(&self_path, &destination, metadata.package_offset)?;
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
    }

    // A failed launch does NOT consume the installer. A successful repair
    // needs no functioning SST console at any point.
    guard.commit()?;
    schedule_self_delete(&self_path, &metadata.package_id)
        .context("la extracción terminó, pero no se pudo programar la autodestrucción del paquete")?;

    Ok(())
}

fn resolve_destination(parent: &Path) -> Result<(PathBuf, bool, bool)> {
    let mut args = env::args_os().skip(1);
    match args.next() {
        None if parent.join("sst.exe").is_file() => {
            // Double-clicking the official installer from the BROKEN SST folder
            // automatically performs an in-place repair. No terminal needed.
            Ok((parent.to_path_buf(), true, true))
        }
        None => Ok((parent.join(INSTALL_DIR), true, false)),
        Some(flag) if flag == OsString::from("--extract-only") => {
            let destination = args
                .next()
                .map(PathBuf::from)
                .context("--extract-only requiere una carpeta de destino")?;
            if args.next().is_some() {
                bail!("--extract-only acepta una sola carpeta de destino");
            }
            Ok((destination, false, false))
        }
        Some(flag) if flag == OsString::from("--repair") => {
            let destination = args.next().map(PathBuf::from)
                .unwrap_or_else(|| parent.to_path_buf());
            if args.next().is_some() {
                bail!("--repair acepta solamente una carpeta de SST");
            }
            if !destination.join("sst.exe").is_file() {
                bail!("--repair requiere una instalación SST existente en {}", destination.display());
            }
            Ok((destination, true, true))
        }
        Some(other) => bail!(
            "argumento no reconocido para SST Setup: {}",
            other.to_string_lossy()
        ),
    }
}

fn read_sfx_metadata(executable: &Path) -> Result<SfxMetadata> {
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
        bail!("este archivo no contiene un paquete SST de un solo uso válido");
    }

    let package_offset = read_u64(&mut input)?;
    if package_offset >= total.saturating_sub(FOOTER_LEN) {
        bail!("autoextraible inválido: offset de bundle fuera de rango");
    }

    let mut package_id = [0u8; PACKAGE_ID_LEN];
    input.read_exact(&mut package_id)?;
    if package_id.iter().all(|byte| *byte == 0) {
        bail!("autoextraible inválido: ID de paquete ausente");
    }

    Ok(SfxMetadata {
        package_offset,
        package_id,
    })
}

fn extract_bundle(executable: &Path, destination: &Path, package_offset: u64) -> Result<()> {
    if destination.as_os_str().is_empty()
        || destination == destination.ancestors().last().unwrap_or(destination)
    {
        bail!("ruta de destino insegura");
    }

    let mut input = File::open(executable)
        .with_context(|| format!("no se pudo abrir {}", executable.display()))?;
    let total = input.metadata()?.len();
    let payload_end = total.saturating_sub(FOOTER_LEN);

    input.seek(SeekFrom::Start(package_offset))?;
    let mut package_magic = [0u8; 8];
    input.read_exact(&mut package_magic)?;
    if &package_magic != PACKAGE_MAGIC {
        bail!("autoextraible inválido: cabecera de bundle SST ausente");
    }

    let version = read_u32(&mut input)?;
    if version != PACKAGE_VERSION {
        bail!("versión de bundle SST no soportada: {version}");
    }
    let count = read_u32(&mut input)?;
    if count == 0 || count > MAX_FILES {
        bail!("cantidad de archivos inválida en bundle: {count}");
    }

    fs::create_dir_all(destination)
        .with_context(|| format!("no se pudo crear {}", destination.display()))?;

    let mut buffer = vec![0u8; 1024 * 1024];
    for _ in 0..count {
        let path_len = read_u32(&mut input)?;
        let size = read_u64(&mut input)?;
        if path_len == 0 || path_len > MAX_PATH_BYTES {
            bail!("ruta inválida dentro del bundle");
        }

        let mut path_bytes = vec![0u8; path_len as usize];
        input.read_exact(&mut path_bytes)?;
        let path_text = std::str::from_utf8(&path_bytes)
            .context("ruta UTF-8 inválida dentro del bundle")?;
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
        target.flush()?;
    }

    Ok(())
}

fn consumption_root() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("PROGRAMDATA") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(env::temp_dir());

    let mut last_error = None;
    for base in candidates {
        let root = base.join("Shell Shock Tool").join("sfx-consumed");
        match fs::create_dir_all(&root) {
            Ok(()) => return Ok(root),
            Err(error) => last_error = Some((root, error)),
        }
    }

    if let Some((root, error)) = last_error {
        return Err(error).with_context(|| {
            format!("no se pudo preparar el registro de paquetes consumidos en {}", root.display())
        });
    }
    bail!("no existe una ubicación válida para registrar el consumo del paquete")
}

fn package_id_hex(id: &[u8; PACKAGE_ID_LEN]) -> String {
    let mut out = String::with_capacity(PACKAGE_ID_LEN * 2);
    for byte in id {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(windows)]
fn schedule_self_delete(executable: &Path, package_id: &[u8; PACKAGE_ID_LEN]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = env::temp_dir().join(format!(
        "sst-sfx-delete-{}-{}.cmd",
        package_id_hex(package_id),
        std::process::id()
    ));
    // Do not truncate the setup while waiting for Windows to release its image
    // mapping. A failed deletion must leave the original package intact.
    let body = concat!(
        "@echo off\r\n",
        "setlocal\r\n",
        "set \"target=%~1\"\r\n",
        "for /L %%I in (1,1,40) do (\r\n",
        "  del /f /q \"%target%\" >nul 2>&1\r\n",
        "  if not exist \"%target%\" goto :done\r\n",
        "  ping 127.0.0.1 -n 2 >nul\r\n",
        ")\r\n",
        ":done\r\n",
        "del /f /q \"%~f0\" >nul 2>&1\r\n"
    );
    fs::write(&script, body)
        .with_context(|| format!("no se pudo crear {}", script.display()))?;

    let system_root = env::var_os("SystemRoot")
        .or_else(|| env::var_os("WINDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let cmd = system_root.join("System32").join("cmd.exe");

    // Pass the batch path and target as independent arguments. Building one
    // quoted `call "..." "..."` string is fragile under cmd.exe /C quoting and
    // can leave the deletion helper running without ever reaching the script.
    let mut command = Command::new(&cmd);
    command.args(["/D", "/Q", "/C", "call"]);
    command.arg(&script);
    command.arg(executable);
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .spawn()
        .with_context(|| format!("no se pudo iniciar {}", cmd.display()))?;
    Ok(())
}

#[cfg(not(windows))]
fn schedule_self_delete(executable: &Path, _package_id: &[u8; PACKAGE_ID_LEN]) -> Result<()> {
    fs::remove_file(executable)
        .with_context(|| format!("no se pudo eliminar {}", executable.display()))
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
