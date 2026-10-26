use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Threading::{GetExitCodeProcess, WaitForSingleObject},
    UI::{
        Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};
use zip::ZipArchive;

use crate::{
    core::{CommandContext, CommandOutput},
    support::windows::system32_executable,
};

use super::BuiltinCommand;

const GET_HELP_DOWNLOAD: &str = "https://aka.ms/SaRA_EnterpriseVersionFiles";
const GET_HELP_ZIP: &str = "GetHelpCmd.zip";

pub struct ToolsBuiltin;

impl BuiltinCommand for ToolsBuiltin {
    fn name(&self) -> &'static str {
        "tools"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["tool", "utils"]
    }

    fn help(&self) -> &'static str {
        "tools — suite de utilidades de mantenimiento y reparación"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
            return Ok(help_output());
        }

        match args[0].to_ascii_lowercase().as_str() {
            "list" => Ok(help_output()),
            "office" => office_tools(&args[1..]),
            other => Ok(CommandOutput::error(
                format!("tools: utilidad desconocida: {other}\nuse: tools list"),
                2,
            )),
        }
    }
}

fn help_output() -> CommandOutput {
    CommandOutput::ok(
        "SST Tools\n\n\
         Office\n\
           tools office scrub [all|m365|2024|2021|2019|2016|2013|2010|2007]\n\
           tools office uninstall [VERSION]\n\n\
         office scrub usa la herramienta oficial GetHelpCmd de Microsoft y ejecuta\n\
         OfficeScrubScenario. Sin VERSION elimina todas las versiones de Office.\n",
    )
}

fn office_tools(args: &[String]) -> Result<CommandOutput> {
    let Some(action) = args.first() else {
        return Ok(CommandOutput::error(
            "tools office: falta acción\nuse: tools office scrub [VERSION]",
            2,
        ));
    };

    match action.to_ascii_lowercase().as_str() {
        "scrub" | "uninstall" | "remove" => {
            let version = args.get(1).map(String::as_str).unwrap_or("all");
            if args.len() > 2 {
                return Ok(CommandOutput::error(
                    "tools office scrub: demasiados argumentos",
                    2,
                ));
            }
            let version = normalize_office_version(version)?;
            run_office_scrub(version)
        }
        _ => Ok(CommandOutput::error(
            format!("tools office: acción desconocida: {action}"),
            2,
        )),
    }
}

fn normalize_office_version(value: &str) -> Result<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "all" => Ok("All"),
        "m365" | "365" | "microsoft365" => Ok("M365"),
        "2024" => Ok("2024"),
        "2021" => Ok("2021"),
        "2019" => Ok("2019"),
        "2016" => Ok("2016"),
        "2013" => Ok("2013"),
        "2010" => Ok("2010"),
        "2007" => Ok("2007"),
        other => bail!(
            "versión de Office no válida: {other}; use all, m365, 2024, 2021, 2019, 2016, 2013, 2010 o 2007"
        ),
    }
}

fn run_office_scrub(version: &str) -> Result<CommandOutput> {
    let root = env::temp_dir().join("sst").join("tools").join("office-scrub");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)
        .with_context(|| format!("no se pudo crear {}", root.display()))?;

    let zip_path = root.join(GET_HELP_ZIP);
    download_get_help(&zip_path)?;
    extract_zip(&zip_path, &root)?;
    let get_help = find_get_help_cmd(&root)
        .context("el paquete oficial de Microsoft no contiene GetHelpCmd.exe")?;

    let exit_code = run_elevated_get_help(&get_help, version)?;
    let _ = fs::remove_dir_all(&root);
    Ok(office_result(exit_code, version))
}

fn download_get_help(destination: &Path) -> Result<()> {
    let curl = system32_executable("curl.exe")?;
    let status = Command::new(curl)
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            GET_HELP_DOWNLOAD,
            "-o",
        ])
        .arg(destination)
        .status()
        .context("no se pudo ejecutar curl para descargar GetHelpCmd")?;

    if !status.success() {
        bail!("Microsoft GetHelpCmd no pudo descargarse desde el enlace oficial");
    }
    Ok(())
}

fn extract_zip(zip_path: &Path, destination: &Path) -> Result<()> {
    let file = File::open(zip_path)
        .with_context(|| format!("no se pudo abrir {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).context("el paquete GetHelpCmd descargado no es ZIP válido")?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
            bail!("el paquete GetHelpCmd contiene una ruta insegura");
        };
        let output = destination.join(relative);

        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = File::create(&output)?;
        io::copy(&mut entry, &mut target)?;
    }
    Ok(())
}

fn find_get_help_cmd(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_get_help_cmd(&path) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("GetHelpCmd.exe"))
        {
            return Some(path);
        }
    }
    None
}

fn run_elevated_get_help(exe: &Path, version: &str) -> Result<u32> {
    let verb = wide("runas");
    let exe_w = wide(exe.as_os_str());
    let params = format!(
        "-S OfficeScrubScenario -AcceptEula -OfficeVersion {version}"
    );
    let params_w = wide(params);
    let dir_w = wide(exe.parent().unwrap_or_else(|| Path::new(".")).as_os_str());

    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.lpDirectory = dir_w.as_ptr();
    info.nShow = SW_SHOWNORMAL;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        bail!("no se pudo elevar la herramienta de desinstalación de Office mediante UAC");
    }
    if info.hProcess.is_null() {
        bail!("Microsoft GetHelpCmd se inició sin un handle de proceso válido");
    }

    unsafe {
        WaitForSingleObject(info.hProcess, u32::MAX);
        let mut exit_code = 1u32;
        if GetExitCodeProcess(info.hProcess, &mut exit_code) == 0 {
            CloseHandle(info.hProcess);
            bail!("no se pudo obtener el resultado de GetHelpCmd");
        }
        CloseHandle(info.hProcess);
        Ok(exit_code)
    }
}

fn office_result(code: u32, version: &str) -> CommandOutput {
    match code {
        0 => CommandOutput::ok(format!(
            "OfficeScrubScenario completado para {version}. Microsoft recomienda reiniciar el equipo para finalizar la limpieza.\n"
        )),
        6 => CommandOutput::error(
            "OfficeScrubScenario: hay aplicaciones de Office/Teams abiertas. Ciérrelas y vuelva a ejecutar la limpieza.",
            6,
        ),
        8 => CommandOutput::error(
            "OfficeScrubScenario: se detectaron varias versiones de Office y Microsoft no pudo resolver la selección.",
            8,
        ),
        9 => CommandOutput::error("OfficeScrubScenario: Microsoft no pudo quitar Office.", 9),
        10 => CommandOutput::error("OfficeScrubScenario: la herramienta no quedó elevada.", 10),
        66 => CommandOutput::error("OfficeScrubScenario: la versión solicitada no coincide con la instalada.", 66),
        67 => CommandOutput::error("OfficeScrubScenario: Microsoft rechazó la versión indicada.", 67),
        68 => CommandOutput::error("OfficeScrubScenario: no se encontró Office instalado.", 68),
        other => CommandOutput::error(
            format!("OfficeScrubScenario terminó con código Microsoft {other}."),
            other.min(i32::MAX as u32) as i32,
        ),
    }
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
