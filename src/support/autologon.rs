//! Native Windows Autologon with protected credential capture.
//! No Sysinternals executable, PowerShell, registry plaintext password,
//! command-line secret, environment variable or temporary secret file.
//! The Windows automatic-logon feature inherently weakens physical security.

#[cfg(windows)]
use std::{
    ffi::{c_void, OsStr},
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};

#[cfg(windows)]
use anyhow::{bail, Context, Result};
#[cfg(windows)]
use base64::{engine::general_purpose::STANDARD, Engine as _};
#[cfg(windows)]
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError},
    Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
    System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
    },
    UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
};

#[cfg(windows)]
const ERROR_ACCESS_DENIED: u32 = 5;
#[cfg(windows)]
const ERROR_CANCELLED: u32 = 1223;
#[cfg(windows)]
const ERROR_INVALID_PARAMETER: u32 = 87;
#[cfg(windows)]
const WAIT_INFINITE: u32 = 0xffff_ffff;
#[cfg(windows)]
const POLICY_CREATE_SECRET: u32 = 0x0000_0020;
#[cfg(windows)]
const KEY_SET_VALUE: u32 = 0x0002;
#[cfg(windows)]
const KEY_WOW64_64KEY: u32 = 0x0100;
#[cfg(windows)]
const REG_SZ: u32 = 1;

#[cfg(windows)]
#[repr(C)]
struct LsaUnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[cfg(windows)]
#[repr(C)]
struct LsaObjectAttributes {
    length: u32,
    root_directory: *mut c_void,
    object_name: *mut LsaUnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}

#[cfg(windows)]
#[link(name = "Advapi32")]
unsafe extern "system" {
    fn LsaOpenPolicy(
        system_name: *const LsaUnicodeString,
        object_attributes: *const LsaObjectAttributes,
        desired_access: u32,
        policy_handle: *mut *mut c_void,
    ) -> i32;
    fn LsaStorePrivateData(
        policy_handle: *mut c_void,
        key_name: *const LsaUnicodeString,
        private_data: *const LsaUnicodeString,
    ) -> i32;
    fn LsaClose(handle: *mut c_void) -> i32;
    fn LsaNtStatusToWinError(status: i32) -> u32;
    fn RegOpenKeyExW(
        hkey: *mut c_void,
        subkey: *const u16,
        options: u32,
        desired_access: u32,
        result: *mut *mut c_void,
    ) -> i32;
    fn RegSetValueExW(
        hkey: *mut c_void,
        value_name: *const u16,
        reserved: u32,
        data_type: u32,
        data: *const u8,
        data_size: u32,
    ) -> i32;
    fn RegDeleteValueW(hkey: *mut c_void, value_name: *const u16) -> i32;
    fn RegCloseKey(hkey: *mut c_void) -> i32;
}

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn elevated_token() -> Result<bool> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            bail!("autologon: no se pudo consultar el token (Win32 {})", GetLastError());
        }
        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let error = if ok == 0 { GetLastError() } else { 0 };
        CloseHandle(token);
        if ok == 0 { bail!("autologon: consulta de token falló (Win32 {error})"); }
        Ok(elevation.TokenIsElevated != 0)
    }
}

#[cfg(windows)]
struct RegistryKey(*mut c_void);
#[cfg(windows)]
impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0); }
    }
}

#[cfg(windows)]
impl RegistryKey {
    fn winlogon() -> std::result::Result<Self, u32> {
        let path = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon");
        let mut key = null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                (-2147483646isize) as *mut c_void, // HKEY_LOCAL_MACHINE
                path.as_ptr(), 0, KEY_SET_VALUE | KEY_WOW64_64KEY, &mut key,
            )
        };
        if status != 0 { Err(status as u32) } else { Ok(Self(key)) }
    }

    fn set_string(&self, name: &str, value: &str) -> std::result::Result<(), u32> {
        let key = wide(name);
        let data = wide(value);
        let status = unsafe {
            RegSetValueExW(
                self.0, key.as_ptr(), 0, REG_SZ, data.as_ptr().cast(),
                (data.len() * size_of::<u16>()) as u32,
            )
        };
        if status == 0 { Ok(()) } else { Err(status as u32) }
    }

    fn delete(&self, name: &str) -> std::result::Result<(), u32> {
        let key = wide(name);
        let status = unsafe { RegDeleteValueW(self.0, key.as_ptr()) };
        if status == 0 || status == 2 { Ok(()) } else { Err(status as u32) }
    }
}

// Microsoft documents "DefaultPassword" as the LSA private-data key
// consumed by Windows automatic logon. Data is stored encrypted by LSA.
#[cfg(windows)]
fn store_lsa_password(secret: Option<&[u16]>) -> std::result::Result<(), u32> {
    let name = wide("DefaultPassword");
    let name_len = ((name.len() - 1) * 2) as u16;
    let key_name = LsaUnicodeString {
        length: name_len,
        maximum_length: (name.len() * 2) as u16,
        buffer: name.as_ptr() as *mut u16,
    };
    let attributes = LsaObjectAttributes {
        length: size_of::<LsaObjectAttributes>() as u32,
        root_directory: null_mut(),
        object_name: null_mut(),
        attributes: 0,
        security_descriptor: null_mut(),
        security_quality_of_service: null_mut(),
    };
    let mut policy = null_mut();
    let opened = unsafe {
        LsaOpenPolicy(null(), &attributes, POLICY_CREATE_SECRET, &mut policy)
    };
    if opened != 0 {
        return Err(unsafe { LsaNtStatusToWinError(opened) });
    }
    let status = if let Some(secret) = secret {
        if secret.is_empty() || secret.len() >= 32767 {
            unsafe { LsaClose(policy); }
            return Err(ERROR_INVALID_PARAMETER);
        }
        let data = LsaUnicodeString {
            length: (secret.len() * 2) as u16,
            maximum_length: (secret.len() * 2) as u16,
            buffer: secret.as_ptr() as *mut u16,
        };
        unsafe { LsaStorePrivateData(policy, &key_name, &data) }
    } else {
        unsafe { LsaStorePrivateData(policy, &key_name, null()) }
    };
    unsafe { LsaClose(policy); }
    if status == 0 { Ok(()) } else {
        Err(unsafe { LsaNtStatusToWinError(status) })
    }
}

#[cfg(windows)]
fn configure_current_token(user: &str, domain: &str) -> std::result::Result<(), u32> {
    if user.trim().is_empty() || domain.trim().is_empty() {
        return Err(ERROR_INVALID_PARAMETER);
    }
    // Don't change any system state if the user cancels the credential prompt.
    let secret = crate::support::domain::request_password(
        &format!("{domain}\\{user}"), "configurar el inicio automático"
    ).map_err(|_| ERROR_CANCELLED)?;

    let key = RegistryKey::winlogon()?;
    // Fail closed if a later registry/LSA operation fails. Never leave
    // AutoAdminLogon enabled against incomplete state.
    key.set_string("AutoAdminLogon", "0")?;
    // Explicitly remove any old cleartext password in the Winlogon key.
    key.delete("DefaultPassword")?;
    store_lsa_password(Some(secret.units()))?;
    key.set_string("DefaultUserName", user)?;
    key.set_string("DefaultDomainName", domain)?;
    key.set_string("AutoAdminLogon", "1")?;
    Ok(())
}

#[cfg(windows)]
fn disable_current_token() -> std::result::Result<(), u32> {
    let key = RegistryKey::winlogon()?;
    key.set_string("AutoAdminLogon", "0")?;
    key.delete("DefaultPassword")?;
    store_lsa_password(None)
}

// Only non-secret metadata may cross the UAC process boundary.
#[cfg(windows)]
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ElevatedRequest {
    action: String,
    user: Option<String>,
    domain: Option<String>,
}

#[cfg(windows)]
fn launch_elevated(request: &ElevatedRequest) -> Result<u32> {
    let exe = std::env::current_exe().context("autologon: no se pudo localizar SST")?;
    let verb = wide("runas");
    let exe_wide = wide(exe.as_os_str());
    let encoded = STANDARD.encode(serde_json::to_vec(request)?);
    let params = wide(format!("-c \"sys autologon __elevated {encoded}\""));
    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe_wide.as_ptr();
    info.lpParameters = params.as_ptr();
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_CANCELLED { bail!("autologon: elevación UAC cancelada"); }
        bail!("autologon: no se pudo elevar SST (Win32 {error})");
    }
    if info.hProcess.is_null() {
        bail!("autologon: elevación no devolvió un proceso válido");
    }
    unsafe {
        WaitForSingleObject(info.hProcess, WAIT_INFINITE);
        let mut code = 1u32;
        if GetExitCodeProcess(info.hProcess, &mut code) == 0 {
            let error = GetLastError();
            CloseHandle(info.hProcess);
            bail!("autologon: no se pudo obtener resultado elevado (Win32 {error})");
        }
        CloseHandle(info.hProcess);
        Ok(code)
    }
}

#[cfg(windows)]
fn validate_metadata(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && !value.chars().any(|ch| ch.is_control())
}

#[cfg(windows)]
pub fn configure(user: &str, domain: &str) -> Result<()> {
    if !validate_metadata(user) || !validate_metadata(domain) {
        bail!("autologon: usuario y dominio deben ser válidos");
    }
    let code = if elevated_token()? {
        configure_current_token(user, domain).err().unwrap_or(0)
    } else {
        launch_elevated(&ElevatedRequest {
            action: "enable".into(),
            user: Some(user.to_owned()),
            domain: Some(domain.to_owned()),
        })?
    };
    if code != 0 { bail!("autologon: Windows rechazó la configuración (Win32 {code})"); }
    Ok(())
}

#[cfg(windows)]
pub fn disable() -> Result<()> {
    let code = if elevated_token()? {
        disable_current_token().err().unwrap_or(0)
    } else {
        launch_elevated(&ElevatedRequest {
            action: "disable".into(), user: None, domain: None,
        })?
    };
    if code != 0 { bail!("autologon: Windows no pudo deshabilitar inicio automático (Win32 {code})"); }
    Ok(())
}

#[cfg(windows)]
pub fn run_elevated_payload(encoded: &str) -> std::result::Result<(), u32> {
    if !elevated_token().map_err(|_| ERROR_ACCESS_DENIED)? {
        return Err(ERROR_ACCESS_DENIED);
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| ERROR_INVALID_PARAMETER)?;
    let request: ElevatedRequest = serde_json::from_slice(&bytes)
        .map_err(|_| ERROR_INVALID_PARAMETER)?;
    match request.action.as_str() {
        "enable" => {
            let user = request.user.as_deref().ok_or(ERROR_INVALID_PARAMETER)?;
            let domain = request.domain.as_deref().ok_or(ERROR_INVALID_PARAMETER)?;
            if !validate_metadata(user) || !validate_metadata(domain) {
                return Err(ERROR_INVALID_PARAMETER);
            }
            configure_current_token(user, domain)
        }
        "disable" if request.user.is_none() && request.domain.is_none() => disable_current_token(),
        _ => Err(ERROR_INVALID_PARAMETER),
    }
}

#[cfg(not(windows))]
pub fn configure(_user: &str, _domain: &str) -> anyhow::Result<()> {
    anyhow::bail!("autologon sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn disable() -> anyhow::Result<()> {
    anyhow::bail!("autologon sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn run_elevated_payload(_encoded: &str) -> std::result::Result<(), u32> {
    Err(50)
}
