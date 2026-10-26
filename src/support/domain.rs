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
use base64::{engine::general_purpose::STANDARD, Engine as _};
#[cfg(windows)]
use serde::{Deserialize, Serialize};
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
const ERROR_INVALID_PARAMETER: u32 = 87;
#[cfg(windows)]
const NETSETUP_JOIN_DOMAIN: u32 = 0x0000_0001;
#[cfg(windows)]
const NETSETUP_ACCT_CREATE: u32 = 0x0000_0002;
#[cfg(windows)]
const NETSETUP_DOMAIN_NAME: i32 = 3;
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
    fn NetJoinDomain(
        lpServer: *const u16,
        lpDomain: *const u16,
        lpMachineAccountOU: *const u16,
        lpAccount: *const u16,
        lpPassword: *const u16,
        fJoinOptions: u32,
    ) -> u32;
    fn NetUnjoinDomain(
        lpServer: *const u16,
        lpAccount: *const u16,
        lpPassword: *const u16,
        fUnjoinOptions: u32,
    ) -> u32;
}

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn read_wide(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

#[cfg(windows)]
pub fn status() -> Result<String> {
    unsafe {
        let mut buffer: *mut u16 = null_mut();
        let mut join_status = 0i32;
        let code = NetGetJoinInformation(null(), &mut buffer, &mut join_status);
        if code != NERR_SUCCESS {
            bail!("NetGetJoinInformation falló (NetAPI {code})");
        }
        let name = read_wide(buffer);
        if !buffer.is_null() {
            let _ = NetApiBufferFree(buffer.cast());
        }
        if join_status == NETSETUP_DOMAIN_NAME {
            Ok(format!("DOMAIN / {name}"))
        } else if name.is_empty() {
            Ok("WORKGROUP / UNKNOWN".to_owned())
        } else {
            Ok(format!("WORKGROUP / {name}"))
        }
    }
}

#[cfg(windows)]
fn join_current_token(
    domain: &str,
    account: &str,
    password: &str,
    ou: Option<&str>,
) -> std::result::Result<(), u32> {
    let domain_w = wide(domain);
    let account_w = wide(account);
    let mut password_w = wide(password);
    let ou_w = ou.map(wide);
    let ou_ptr = ou_w.as_ref().map_or(null(), |v| v.as_ptr());
    let code = unsafe {
        NetJoinDomain(
            null(),
            domain_w.as_ptr(),
            ou_ptr,
            account_w.as_ptr(),
            password_w.as_ptr(),
            NETSETUP_JOIN_DOMAIN | NETSETUP_ACCT_CREATE,
        )
    };
    password_w.fill(0);
    if code == NERR_SUCCESS { Ok(()) } else { Err(code) }
}

#[cfg(windows)]
fn leave_current_token(account: &str, password: &str) -> std::result::Result<(), u32> {
    let account_w = wide(account);
    let mut password_w = wide(password);
    let code = unsafe {
        NetUnjoinDomain(
            null(),
            account_w.as_ptr(),
            password_w.as_ptr(),
            0,
        )
    };
    password_w.fill(0);
    if code == NERR_SUCCESS { Ok(()) } else { Err(code) }
}

#[cfg(windows)]
#[derive(Serialize, Deserialize)]
struct ElevatedPayload {
    action: String,
    domain: Option<String>,
    ou: Option<String>,
    account: String,
    password: String,
}

#[cfg(windows)]
fn run_elevated(payload: &ElevatedPayload) -> Result<u32> {
    let executable = std::env::current_exe().context("no se pudo resolver sst.exe")?;
    let verb = wide("runas");
    let executable_w = wide(executable.as_os_str());
    let encoded = STANDARD.encode(serde_json::to_vec(payload)?);
    let params = format!("-c \"sys domain __elevated {encoded}\"");
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
        bail!("no se pudo elevar SST para administrar el dominio (Win32 {error})");
    }
    if info.hProcess.is_null() {
        bail!("el helper elevado de dominio no devolvió un proceso válido");
    }

    unsafe {
        WaitForSingleObject(info.hProcess, WAIT_INFINITE);
        let mut exit_code = 1u32;
        if GetExitCodeProcess(info.hProcess, &mut exit_code) == 0 {
            let error = GetLastError();
            CloseHandle(info.hProcess);
            bail!("no se pudo obtener el resultado de la operación de dominio (Win32 {error})");
        }
        CloseHandle(info.hProcess);
        Ok(exit_code)
    }
}

#[cfg(windows)]
pub fn join(domain: &str, account: &str, password: &str, ou: Option<&str>) -> Result<()> {
    if domain.trim().is_empty() || account.trim().is_empty() || password.is_empty() {
        bail!("dominio, cuenta y contraseña son obligatorios");
    }
    match join_current_token(domain.trim(), account.trim(), password, ou) {
        Ok(()) => Ok(()),
        Err(ERROR_ACCESS_DENIED) => {
            let payload = ElevatedPayload {
                action: "join".to_owned(),
                domain: Some(domain.trim().to_owned()),
                ou: ou.map(str::to_owned),
                account: account.trim().to_owned(),
                password: password.to_owned(),
            };
            let code = run_elevated(&payload)?;
            if code == NERR_SUCCESS { Ok(()) } else { bail!("Windows no pudo unir el equipo al dominio (NetAPI {code})") }
        }
        Err(code) => bail!("Windows no pudo unir el equipo al dominio (NetAPI {code})"),
    }
}

#[cfg(windows)]
pub fn leave(account: &str, password: &str) -> Result<()> {
    if account.trim().is_empty() || password.is_empty() {
        bail!("cuenta y contraseña son obligatorios");
    }
    match leave_current_token(account.trim(), password) {
        Ok(()) => Ok(()),
        Err(ERROR_ACCESS_DENIED) => {
            let payload = ElevatedPayload {
                action: "leave".to_owned(),
                domain: None,
                ou: None,
                account: account.trim().to_owned(),
                password: password.to_owned(),
            };
            let code = run_elevated(&payload)?;
            if code == NERR_SUCCESS { Ok(()) } else { bail!("Windows no pudo sacar el equipo del dominio (NetAPI {code})") }
        }
        Err(code) => bail!("Windows no pudo sacar el equipo del dominio (NetAPI {code})"),
    }
}

#[cfg(windows)]
pub fn run_elevated_payload(encoded: &str) -> std::result::Result<(), u32> {
    let bytes = STANDARD.decode(encoded).map_err(|_| ERROR_INVALID_PARAMETER)?;
    let payload: ElevatedPayload = serde_json::from_slice(&bytes).map_err(|_| ERROR_INVALID_PARAMETER)?;
    match payload.action.as_str() {
        "join" => join_current_token(
            payload.domain.as_deref().ok_or(ERROR_INVALID_PARAMETER)?,
            &payload.account,
            &payload.password,
            payload.ou.as_deref(),
        ),
        "leave" => leave_current_token(&payload.account, &payload.password),
        _ => Err(ERROR_INVALID_PARAMETER),
    }
}

#[cfg(not(windows))]
pub fn status() -> anyhow::Result<String> {
    anyhow::bail!("la administración de dominio sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn join(_domain: &str, _account: &str, _password: &str, _ou: Option<&str>) -> anyhow::Result<()> {
    anyhow::bail!("la administración de dominio sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn leave(_account: &str, _password: &str) -> anyhow::Result<()> {
    anyhow::bail!("la administración de dominio sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn run_elevated_payload(_encoded: &str) -> std::result::Result<(), u32> {
    Err(50)
}
