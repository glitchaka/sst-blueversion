#[cfg(windows)]
use std::{
    ffi::{c_void, OsStr},
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
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
const NERR_SUCCESS: u32 = 0;
#[cfg(windows)]
const ERROR_ACCESS_DENIED: u32 = 5;
#[cfg(windows)]
const NETSETUP_ACCT_CREATE: u32 = 0x0000_0002;
#[cfg(windows)]
const NETSETUP_DOMAIN_NAME: i32 = 3;
#[cfg(windows)]
const COMPUTER_NAME_PHYSICAL_DNS_HOSTNAME: i32 = 5;
#[cfg(windows)]
const WAIT_INFINITE: u32 = 0xffff_ffff;

#[cfg(windows)]
#[link(name = "Netapi32")]
unsafe extern "system" {
    fn NetGetJoinInformation(
        lpServer: *const u16,
        lpNameBuffer: *mut *mut u16,
        BufferType: *mut i32,
    ) -> u32;
    fn NetApiBufferFree(Buffer: *mut c_void) -> u32;
    fn NetRenameMachineInDomain(
        lpServer: *const u16,
        lpNewMachineName: *const u16,
        lpAccount: *const u16,
        lpPassword: *const u16,
        fRenameOptions: u32,
    ) -> u32;
}

#[cfg(windows)]
#[link(name = "Kernel32")]
unsafe extern "system" {
    fn SetComputerNameExW(NameType: i32, lpBuffer: *const u16) -> i32;
}

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn validate_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("el nombre del equipo no puede estar vacío");
    }
    if name.len() > 63 {
        bail!("el nombre del equipo no puede superar 63 caracteres");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("el nombre del equipo no puede comenzar ni terminar con '-'");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        bail!("el nombre del equipo sólo puede contener letras ASCII, números y '-'");
    }
    if name.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("el nombre del equipo no puede estar formado sólo por números");
    }
    Ok(())
}

#[cfg(windows)]
fn domain_joined() -> std::result::Result<bool, u32> {
    unsafe {
        let mut buffer: *mut u16 = null_mut();
        let mut status = 0i32;
        let code = NetGetJoinInformation(null(), &mut buffer, &mut status);
        if !buffer.is_null() {
            let _ = NetApiBufferFree(buffer.cast());
        }
        if code != NERR_SUCCESS {
            return Err(code);
        }
        Ok(status == NETSETUP_DOMAIN_NAME)
    }
}

/// Performs the rename using the current token only. This function never prompts for UAC.
/// It is also used by the elevated SST command launched by `rename_computer`.
#[cfg(windows)]
pub fn rename_computer_current_token(name: &str) -> std::result::Result<(), u32> {
    if validate_name(name).is_err() {
        return Err(87); // ERROR_INVALID_PARAMETER
    }

    let name_w = wide(name.trim());
    match domain_joined() {
        Ok(true) => {
            let code = unsafe {
                NetRenameMachineInDomain(
                    null(),
                    name_w.as_ptr(),
                    null(),
                    null(),
                    NETSETUP_ACCT_CREATE,
                )
            };
            if code == NERR_SUCCESS { Ok(()) } else { Err(code) }
        }
        Ok(false) => {
            if unsafe { SetComputerNameExW(COMPUTER_NAME_PHYSICAL_DNS_HOSTNAME, name_w.as_ptr()) } != 0 {
                Ok(())
            } else {
                Err(unsafe { GetLastError() })
            }
        }
        Err(code) => Err(code),
    }
}

#[cfg(windows)]
fn run_elevated_helper(name: &str) -> Result<u32> {
    let executable = std::env::current_exe().context("no se pudo resolver sst.exe")?;
    let verb = wide("runas");
    let executable_w = wide(executable.as_os_str());
    let params = format!(
        "-c \"hostname {} --elevated-internal\"",
        name.trim()
    );
    let params_w = wide(params);

    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.nShow = SW_HIDE;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if error == 1223 {
            bail!("elevación UAC cancelada por el usuario");
        }
        bail!("no se pudo elevar SST para renombrar el equipo (Win32 {error})");
    }
    if info.hProcess.is_null() {
        bail!("el helper elevado de SST no devolvió un proceso válido");
    }

    unsafe {
        WaitForSingleObject(info.hProcess, WAIT_INFINITE);
        let mut exit_code = 1u32;
        if GetExitCodeProcess(info.hProcess, &mut exit_code) == 0 {
            let error = GetLastError();
            CloseHandle(info.hProcess);
            bail!("no se pudo obtener el resultado del renombrado (Win32 {error})");
        }
        CloseHandle(info.hProcess);
        Ok(exit_code)
    }
}

/// Renames the local Windows computer. If the current token is not elevated, SST
/// re-runs only this operation through the native UAC prompt. Domain-joined machines
/// use NetRenameMachineInDomain so the computer account in AD is renamed as well.
#[cfg(windows)]
pub fn rename_computer(name: &str) -> Result<()> {
    validate_name(name)?;

    match rename_computer_current_token(name.trim()) {
        Ok(()) => Ok(()),
        Err(ERROR_ACCESS_DENIED) => {
            let code = run_elevated_helper(name)?;
            if code == NERR_SUCCESS {
                Ok(())
            } else if code == ERROR_ACCESS_DENIED {
                bail!("Windows/Active Directory rechazó el cambio de nombre por permisos insuficientes");
            } else {
                bail!("Windows no pudo renombrar el equipo (NetAPI/Win32 {code})");
            }
        }
        Err(code) => bail!("Windows no pudo renombrar el equipo (NetAPI/Win32 {code})"),
    }
}

#[cfg(not(windows))]
pub fn rename_computer(_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("cambiar el nombre del equipo sólo está disponible en Windows")
}

#[cfg(not(windows))]
pub fn rename_computer_current_token(_name: &str) -> std::result::Result<(), u32> {
    Err(50)
}
