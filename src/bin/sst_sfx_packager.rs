use std::{
    env,
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use rand_core::{OsRng, RngCore};

const PACKAGE_MAGIC: &[u8; 8] = b"SSTPKG01";
const SFX_MAGIC: &[u8; 8] = b"SSTSFX03";
const PACKAGE_VERSION: u32 = 1;
const PACKAGE_ID_LEN: usize = 16;

#[derive(Debug)]
struct SourceFile {
    absolute: PathBuf,
    relative: String,
    size: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("sst_sfx_packager: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = env::args_os().collect::<Vec<_>>();
    if args.len() != 4 {
        bail!("uso: sst_sfx_packager <payload-dir> <sst_sfx_stub.exe> <salida.exe>");
    }

    let payload_root = PathBuf::from(&args[1]);
    let stub = PathBuf::from(&args[2]);
    let output = PathBuf::from(&args[3]);

    if !payload_root.is_dir() {
        bail!("payload inexistente: {}", payload_root.display());
    }
    if !stub.is_file() {
        bail!("stub inexistente: {}", stub.display());
    }

    let files = scan_files(&payload_root)?;
    if files.is_empty() {
        bail!("el payload no contiene archivos");
    }

    let package_id = generate_package_id();
    build_self_extractor(&stub, &output, &files, &package_id)?;
    println!("Autoextraible creado: {}", output.display());
    println!("ID de paquete: {}", package_id_hex(&package_id));
    println!("Archivos empaquetados: {}", files.len());
    Ok(())
}

fn generate_package_id() -> [u8; PACKAGE_ID_LEN] {
    let mut id = [0u8; PACKAGE_ID_LEN];
    let mut rng = OsRng;
    rng.fill_bytes(&mut id);
    id
}

fn package_id_hex(id: &[u8; PACKAGE_ID_LEN]) -> String {
    let mut out = String::with_capacity(PACKAGE_ID_LEN * 2);
    for byte in id {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn scan_files(root: &Path) -> Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    scan_dir(root, root, &mut files)?;
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

fn scan_dir(root: &Path, current: &Path, out: &mut Vec<SourceFile>) -> Result<()> {
    for entry in fs::read_dir(current)
        .with_context(|| format!("no se pudo leer {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            scan_dir(root, &path, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(root)
            .with_context(|| format!("{} no pertenece al payload", path.display()))?;
        validate_relative_path(relative_path)?;
        let relative = relative_path
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let size = fs::metadata(&path)?.len();
        out.push(SourceFile {
            absolute: path,
            relative,
            size,
        });
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("ruta insegura en payload: {}", path.display());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("ruta insegura en payload: {}", path.display()),
        }
    }
    Ok(())
}

fn build_self_extractor(
    stub: &Path,
    output: &Path,
    files: &[SourceFile],
    package_id: &[u8; PACKAGE_ID_LEN],
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut stub_in = File::open(stub)
        .with_context(|| format!("no se pudo abrir {}", stub.display()))?;
    let mut out = File::create(output)
        .with_context(|| format!("no se pudo crear {}", output.display()))?;

    io::copy(&mut stub_in, &mut out)?;
    let package_offset = out.stream_position()?;

    out.write_all(PACKAGE_MAGIC)?;
    write_u32(&mut out, PACKAGE_VERSION)?;
    write_u32(
        &mut out,
        u32::try_from(files.len()).context("demasiados archivos en payload")?,
    )?;

    let mut buffer = vec![0u8; 1024 * 1024];
    for (index, file) in files.iter().enumerate() {
        let path_bytes = file.relative.as_bytes();
        write_u32(
            &mut out,
            u32::try_from(path_bytes.len()).context("ruta demasiado larga")?,
        )?;
        write_u64(&mut out, file.size)?;
        out.write_all(path_bytes)?;

        let mut input = File::open(&file.absolute)
            .with_context(|| format!("no se pudo abrir {}", file.absolute.display()))?;
        let mut remaining = file.size;
        while remaining > 0 {
            let want = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
            let got = input.read(&mut buffer[..want])?;
            if got == 0 {
                bail!("payload truncado al leer {}", file.absolute.display());
            }
            out.write_all(&buffer[..got])?;
            remaining -= got as u64;
        }

        println!("[{}/{}] {}", index + 1, files.len(), file.relative);
    }

    out.write_all(SFX_MAGIC)?;
    write_u64(&mut out, package_offset)?;
    out.write_all(package_id)?;
    out.flush()?;
    Ok(())
}

fn write_u32(out: &mut impl Write, value: u32) -> Result<()> {
    out.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_u64(out: &mut impl Write, value: u64) -> Result<()> {
    out.write_all(&value.to_le_bytes())?;
    Ok(())
}

trait StreamPosition {
    fn stream_position(&mut self) -> io::Result<u64>;
}

impl StreamPosition for File {
    fn stream_position(&mut self) -> io::Result<u64> {
        use std::io::Seek;
        self.seek(io::SeekFrom::Current(0))
    }
}
