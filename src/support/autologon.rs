#[cfg(windows)]
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

#[cfg(windows)]
use anyhow::{Context, Result, bail};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError},
    System::Threading::{GetExitCodeProcess, WaitForSingleObject},
    UI::{
        Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};
#[cfg(windows)]
use zip::ZipArchive;

#[cfg(windows)]
use crate::support::windows::system32_executable;

#[cfg(windows)]
const AUTOLOGON_DOWNLOAD: &str = "https://download.sysinternals.com/files/AutoLogon.zip";
#[cfg(windows)]
const WAIT_INFINITE: u32 = 0xffff_ffff;

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn quote_arg(value: &str) -> String {
    if !value.is_empty()
        && !value.chars().any(|c| c.is_whitespace() || c == '"')
    {
        return value.to_owned();
    }

    let mut out = String::from("\"");
    let mut slashes = 0usize;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        if ch == '"' {
            out.push_str(&"\\".repeat(slashes * 2 + 1));
            out.push('"');
            slashes = 0;
            continue;
        }
        out.push_str(&"\\".repeat(slashes));
        slashes = 0;
        out.push(ch);
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

#[cfg(windows)]
fn download(destination: &Path) -> Result<()> {
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
            AUTOLOGON_DOWNLOAD,
            "-o",
        ])
        .arg(destination)
        .status()
        .context("no se pudo ejecutar curl para descargar Sysinternals Autologon")?;

    if !status.success() {
        bail!("no se pudo descargar Sysinternals Autologon desde Microsoft");
    }
    Ok(())
}

#[cfg(windows)]
fn extract_zip(zip_path: &Path, destination: &Path) -> Result<()> {
    let file = File::open(zip_path)
        .with_context(|| format!("no se pudo abrir {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).context("AutoLogon.zip no es un ZIP válido")?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("AutoLogon.zip contiene una ruta insegura");
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

#[cfg(windows)]
fn expected_executable() -> &'static str {
    match env::consts::ARCH {
        "x86_64" => "Autologon64.exe",
        "aarch64" => "Autologon64a.exe",
        _ => "Autologon.exe",
    }
}

#[cfg(windows)]
fn find_executable(root: &Path, expected: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_executable(&path, expected) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
        {
            return Some(path);
        }
    }
    None
}

#[cfg(windows)]
fn verify_microsoft_signature(exe: &Path) -> Result<()> {
    let path = exe.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$s=Get-AuthenticodeSignature -LiteralPath '{path}'; \
         if($s.Status -ne 'Valid' -or $null -eq $s.SignerCertificate -or \
         $s.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation'){{ \
         [Console]::Error.WriteLine(('Firma no válida: ' + $s.Status + ' / ' + $s.SignerCertificate.Subject)); exit 1 }}"
    );
    let powershell = system32_executable(r"WindowsPowerShell\v1.0\powershell.exe")?;
    let output = Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
        ])
        .arg(script)
        .output()
        .context("no se pudo validar la firma de Sysinternals Autologon")?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        bail!(
            "SST rechazó Sysinternals Autologon porque la firma Microsoft no pudo validarse{}",
            if detail.is_empty() { String::new() } else { format!(": {detail}") }
        );
    }
    Ok(())
}

#[cfg(windows)]
fn run_elevated_cli(exe: &Path, user: &str, domain: &str, password: &str) -> Result<u32> {
    let verb = wide("runas");
    let exe_w = wide(exe.as_os_str());
    let params = ["/accepteula", user, domain, password]
        .into_iter()
        .map(quote_arg)
        .collect::<Vec<_>>()
        .join(" ");
    let params_w = wide(params);
    let dir_w = wide(exe.parent().unwrap_or_else(|| Path::new(".")).as_os_str());

    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.lpDirectory = dir_w.as_ptr();
    info.nShow = SW_HIDE;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if error == 1223 {
            bail!("elevación UAC cancelada por el usuario");
        }
        bail!("no se pudo elevar Sysinternals Autologon (Win32 {error})");
    }
    if info.hProcess.is_null() {
        bail!("Sysinternals Autologon se inició sin un proceso válido");
    }

    unsafe {
        WaitForSingleObject(info.hProcess, WAIT_INFINITE);
        let mut exit_code = 1u32;
        if GetExitCodeProcess(info.hProcess, &mut exit_code) == 0 {
            let error = GetLastError();
            CloseHandle(info.hProcess);
            bail!("no se pudo obtener el resultado de Autologon (Win32 {error})");
        }
        CloseHandle(info.hProcess);
        Ok(exit_code)
    }
}

#[cfg(windows)]
pub fn configure(user: &str, domain: &str, password: &str) -> Result<()> {
    if user.trim().is_empty() || domain.trim().is_empty() || password.is_empty() {
        bail!("usuario, dominio y contraseña son obligatorios");
    }

    let root = env::temp_dir().join("sst").join("tools").join("sysinternals-autologon");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)
        .with_context(|| format!("no se pudo crear {}", root.display()))?;

    let zip_path = root.join("AutoLogon.zip");
    let result = (|| {
        download(&zip_path)?;
        extract_zip(&zip_path, &root)?;
        let expected = expected_executable();
        let exe = find_executable(&root, expected)
            .with_context(|| format!("el paquete de Microsoft no contiene {expected}"))?;
        verify_microsoft_signature(&exe)?;
        let code = run_elevated_cli(&exe, user.trim(), domain.trim(), password)?;
        if code != 0 {
            bail!("Sysinternals Autologon terminó con código {code}");
        }
        Ok(())
    })();

    let _ = fs::remove_dir_all(&root);
    result
}

#[cfg(not(windows))]
pub fn configure(_user: &str, _domain: &str, _password: &str) -> anyhow::Result<()> {
    anyhow::bail!("Sysinternals Autologon sólo está disponible en Windows")
}
