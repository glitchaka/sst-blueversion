//! Administración de dominio mediante NetAPI y credenciales interactivas.
//! Nunca se aceptan contraseñas como argumentos, variables de entorno, ficheros
//! temporales o mensajes de UAC. CredUI recoge el secreto en el proceso con
//! el token que ejecutará NetJoinDomain/NetUnjoinDomain.


/// Recognize credential-bearing SST commands, including execution through
/// sudo and both forms of the Autologon entry point.
fn credential_command(words: &[&str]) -> bool {
    words.windows(3).any(|part| {
        part[0] == "sys" && part[1] == "domain"
            && matches!(part[2], "join" | "leave")
    }) || words.windows(2).any(|part| part[0] == "sys" && part[1] == "autologon")
        || words.iter().any(|word| *word == "autologon")
}

/// Requests for a protected credential prompt are never recorded in history.
/// This includes previous releases that allowed the password as a value.
pub fn has_sensitive_password_marker(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    credential_command(&words) && words.iter().any(|word|
        matches!(*word, "-p" | "--password")
        || word.starts_with("--password=")
        || (word.starts_with("-p") && word.len() > 2)
    )
}

/// Block inline secrets before verbose output, history expansion and xtrace.
pub fn contains_unsafe_inline_password(line: &str) -> bool {
    line.split(['\n', ';']).any(|command| {
        let lower = command.to_ascii_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        credential_command(&words) && words.iter().enumerate().any(|(index, word)| {
            word.starts_with("--password=")
                || (word.starts_with("-p") && word.len() > 2)
                || (matches!(*word, "-p" | "--password") && index + 1 < words.len())
        })
    })
}

/// Once -p appears anywhere in a protected operation, block further
/// inserted text in the interactive editor, regardless of cursor position.
pub fn awaiting_protected_password_dialog(line: &str) -> bool {
    has_sensitive_password_marker(line)
}

/// Purge legacy domain and Autologon command lines containing passwords.
/// Do not create a backup containing the exposed credentials.
pub fn scrub_sensitive_history(path: &std::path::Path) -> std::io::Result<usize> {
    let original = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut retained = Vec::new();
    let mut count = 0usize;
    for line in original.lines() {
        if has_sensitive_password_marker(line) {
            count += 1;
            if retained.last().is_some_and(|last: &&str| {
                last.strip_prefix('#').is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|ch| ch.is_ascii_digit())
                })
            }) {
                retained.pop();
            }
        } else {
            retained.push(line);
        }
    }
    if count > 0 {
        use std::io::Write;
        let mut output = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
        for line in retained {
            output.write_all(line.as_bytes())?;
            output.write_all(b"\n")?;
        }
        output.sync_all()?;
    }
    Ok(count)
}

#[cfg(test)]
mod password_guard_tests {
    use super::{has_sensitive_password_marker, awaiting_protected_password_dialog, contains_unsafe_inline_password};
    #[test]
    fn domain_password_requests_are_not_history_safe() {
        for line in [
            "sys autologon -u user -d empresa.cl -p",
            "autologon enable -u user -d empresa.cl --password insecure",
            "sys domain join -d domain.cl -u admin -p",
            "sudo sys domain leave -u admin --password old-secret",
            "sys domain join -u admin --password=legacy",
            "sys domain join -u admin -pSecret",
        ] {
            assert!(has_sensitive_password_marker(line), "{line}");
        }
        assert!(!has_sensitive_password_marker("sys domain status"));
        assert!(!has_sensitive_password_marker("sys processes --top 10"));
    }
    #[test]
    fn unsafe_inline_values_are_blocked_before_trace_or_expansion() {
        assert!(contains_unsafe_inline_password(
            "sys domain join -d example.cl -u admin -p never-share-this"
        ));
        assert!(contains_unsafe_inline_password(
            "sys autologon enable -u user -d empresa.cl -p legacy-secret"
        ));
        assert!(contains_unsafe_inline_password(
            "autologon -u user -d empresa.cl --password=legacy-secret"
        ));

        assert!(contains_unsafe_inline_password(
            "sys domain leave -u admin --password=never-share-this"
        ));
        assert!(!contains_unsafe_inline_password(
            "sys domain join -d example.cl -u admin -p"
        ));
        assert!(!contains_unsafe_inline_password(
            "sys domain join -d example.cl -u admin -p\necho after"
        ));
    }

    #[test]
    fn previous_history_passwords_are_removed_without_a_secret_backup() {
        use super::scrub_sensitive_history;
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("sst-domain-history-{}-{id}", std::process::id()));
        std::fs::write(&path,
            "#100\nsys domain join -d internal.invalid -u admin -p old-secret\n#101\nsys info\n#102\nsys autologon enable -u operator -d internal.invalid -p legacy-secret\n"
        ).unwrap();
        assert_eq!(scrub_sensitive_history(&path).unwrap(), 2);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, "#101\nsys info\n");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn password_flag_must_finish_the_editable_line() {
        assert!(awaiting_protected_password_dialog("sys domain join -d empresa.cl -u admin -p"));
        assert!(awaiting_protected_password_dialog("sys autologon enable -u user -d empresa.cl -p"));
        assert!(awaiting_protected_password_dialog("sys domain leave -u admin --password "));
        assert!(!awaiting_protected_password_dialog("sys domain join -d empresa.cl -u admin"));
    }
}

#[cfg(windows)]
use std::{
    ffi::{c_void, OsStr},
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
    sync::atomic::{compiler_fence, Ordering},
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
const ERROR_CANCELLED: u32 = 1223;
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
#[repr(C)]
struct CredUiInfo {
    size: u32,
    parent: *mut c_void,
    message: *const u16,
    caption: *const u16,
    banner: *mut c_void,
}

#[cfg(windows)]
#[link(name = "Credui")]
unsafe extern "system" {
    fn CredUIPromptForCredentialsW(
        info: *const CredUiInfo,
        target: *const u16,
        reserved: *mut c_void,
        authentication_error: u32,
        username: *mut u16,
        username_capacity: u32,
        password: *mut u16,
        password_capacity: u32,
        save: *mut i32,
        flags: u32,
    ) -> u32;
}

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn read_wide(ptr: *const u16) -> String {
    if ptr.is_null() { return String::new(); }
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 { len += 1; }
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
pub(crate) struct SecretPassword(Vec<u16>);

#[cfg(windows)]
impl SecretPassword {
    fn as_ptr(&self) -> *const u16 { self.0.as_ptr() }
    pub(crate) fn units(&self) -> &[u16] {
        let len = self.0.iter().position(|unit| *unit == 0).unwrap_or(self.0.len());
        &self.0[..len]
    }
}

#[cfg(windows)]
impl Drop for SecretPassword {
    fn drop(&mut self) {
        // An observable volatile overwrite prevents the compiler from removing
        // the wipe after the final Win32 call. Do not copy into a Rust String.
        for unit in &mut self.0 {
            unsafe { std::ptr::write_volatile(unit, 0); }
        }
        compiler_fence(Ordering::SeqCst);
    }
}

#[cfg(windows)]
pub(crate) fn request_password(account: &str, action: &str) -> Result<SecretPassword> {
    const DO_NOT_PERSIST: u32 = 0x0000_0002;
    const ALWAYS_SHOW_UI: u32 = 0x0000_0080;
    const GENERIC_CREDENTIALS: u32 = 0x0004_0000;
    const KEEP_USERNAME: u32 = 0x0010_0000;

    let caption = wide("Shell Shock Tool — Credenciales de dominio");
    let message = wide(format!(
        "SST necesita la contraseña de {account} para {action}.\n\
         No se mostrará en SST, no se guardará y no pasará como argumento a otro proceso."
    ));
    let target = wide("SST/domain-operation");
    let mut username = vec![0u16; 514];
    let account_wide = wide(account);
    if account_wide.len() > username.len() {
        bail!("domain: el nombre de la cuenta es demasiado largo");
    }
    username[..account_wide.len()].copy_from_slice(&account_wide);
    let mut password = SecretPassword(vec![0u16; 257]);
    let info = CredUiInfo {
        size: size_of::<CredUiInfo>() as u32,
        parent: null_mut(),
        message: message.as_ptr(),
        caption: caption.as_ptr(),
        banner: null_mut(),
    };
    let mut save = 0i32;
    let result = unsafe {
        CredUIPromptForCredentialsW(
            &info,
            target.as_ptr(),
            null_mut(),
            0,
            username.as_mut_ptr(),
            username.len() as u32,
            password.0.as_mut_ptr(),
            password.0.len() as u32,
            &mut save,
            DO_NOT_PERSIST | ALWAYS_SHOW_UI | GENERIC_CREDENTIALS | KEEP_USERNAME,
        )
    };
    if result == ERROR_CANCELLED {
        bail!("domain: entrada de credenciales cancelada");
    }
    if result != NERR_SUCCESS {
        bail!("domain: Windows no pudo solicitar credenciales (CredUI {result})");
    }
    if password.0[0] == 0 {
        bail!("domain: se necesita una contraseña no vacía");
    }
    Ok(password)
}

#[cfg(windows)]
fn elevated_token() -> Result<bool> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            bail!("domain: no se pudo consultar el token actual (Win32 {})", GetLastError());
        }
        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut returned = 0u32;
        let success = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let error = if success == 0 { GetLastError() } else { 0 };
        CloseHandle(token);
        if success == 0 {
            bail!("domain: consulta de elevación falló (Win32 {error})");
        }
        Ok(elevation.TokenIsElevated != 0)
    }
}

#[cfg(windows)]
fn join_current_token(domain: &str, account: &str, ou: Option<&str>) -> std::result::Result<(), u32> {
    let secret = request_password(account, "unir el equipo al dominio")
        .map_err(|_| ERROR_CANCELLED)?;
    let domain_w = wide(domain);
    let account_w = wide(account);
    let ou_w = ou.map(wide);
    let code = unsafe {
        NetJoinDomain(
            null(),
            domain_w.as_ptr(),
            ou_w.as_ref().map_or(null(), |ou| ou.as_ptr()),
            account_w.as_ptr(),
            secret.as_ptr(),
            NETSETUP_JOIN_DOMAIN | NETSETUP_ACCT_CREATE,
        )
    };
    if code == NERR_SUCCESS { Ok(()) } else { Err(code) }
}

#[cfg(windows)]
fn leave_current_token(account: &str) -> std::result::Result<(), u32> {
    let secret = request_password(account, "sacar el equipo del dominio")
        .map_err(|_| ERROR_CANCELLED)?;
    let account_w = wide(account);
    let code = unsafe {
        NetUnjoinDomain(null(), account_w.as_ptr(), secret.as_ptr(), 0)
    };
    if code == NERR_SUCCESS { Ok(()) } else { Err(code) }
}

// Only public metadata crosses the UAC process boundary. Never add a password
// field to this structure: command lines can be read by other processes.
#[cfg(windows)]
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ElevatedPayload {
    action: String,
    domain: Option<String>,
    ou: Option<String>,
    account: String,
}

#[cfg(windows)]
fn run_elevated(payload: &ElevatedPayload) -> Result<u32> {
    let executable = std::env::current_exe().context("no se pudo resolver sst.exe")?;
    let verb = wide("runas");
    let executable_w = wide(executable.as_os_str());
    let encoded = STANDARD.encode(serde_json::to_vec(payload)?);
    let params = wide(format!("-c \"sys domain __elevated {encoded}\""));

    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable_w.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = SW_HIDE;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_CANCELLED {
            bail!("domain: elevación UAC cancelada");
        }
        bail!("domain: no se pudo elevar SST (Win32 {error})");
    }
    if info.hProcess.is_null() {
        bail!("domain: la elevación no devolvió un proceso válido");
    }

    unsafe {
        WaitForSingleObject(info.hProcess, WAIT_INFINITE);
        let mut exit_code = 1u32;
        if GetExitCodeProcess(info.hProcess, &mut exit_code) == 0 {
            let error = GetLastError();
            CloseHandle(info.hProcess);
            bail!("domain: resultado del proceso elevado no disponible (Win32 {error})");
        }
        CloseHandle(info.hProcess);
        Ok(exit_code)
    }
}

#[cfg(windows)]
pub fn join(domain: &str, account: &str, ou: Option<&str>) -> Result<()> {
    if domain.trim().is_empty() || account.trim().is_empty() {
        bail!("domain: dominio y cuenta son obligatorios");
    }
    let code = if elevated_token()? {
        join_current_token(domain.trim(), account.trim(), ou)
            .err().unwrap_or(NERR_SUCCESS)
    } else {
        run_elevated(&ElevatedPayload {
            action: "join".into(),
            domain: Some(domain.trim().to_owned()),
            ou: ou.map(str::to_owned),
            account: account.trim().to_owned(),
        })?
    };
    if code == ERROR_CANCELLED { bail!("domain: entrada de credenciales cancelada"); }
    if code != NERR_SUCCESS {
        bail!("Windows no pudo unir el equipo al dominio (NetAPI {code})");
    }
    Ok(())
}

#[cfg(windows)]
pub fn leave(account: &str) -> Result<()> {
    if account.trim().is_empty() {
        bail!("domain: la cuenta es obligatoria");
    }
    let code = if elevated_token()? {
        leave_current_token(account.trim()).err().unwrap_or(NERR_SUCCESS)
    } else {
        run_elevated(&ElevatedPayload {
            action: "leave".into(),
            domain: None,
            ou: None,
            account: account.trim().to_owned(),
        })?
    };
    if code == ERROR_CANCELLED { bail!("domain: entrada de credenciales cancelada"); }
    if code != NERR_SUCCESS {
        bail!("Windows no pudo sacar el equipo del dominio (NetAPI {code})");
    }
    Ok(())
}

#[cfg(windows)]
pub fn run_elevated_payload(encoded: &str) -> std::result::Result<(), u32> {
    if !elevated_token().map_err(|_| ERROR_ACCESS_DENIED)? {
        return Err(ERROR_ACCESS_DENIED);
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| ERROR_INVALID_PARAMETER)?;
    let payload: ElevatedPayload = serde_json::from_slice(&bytes)
        .map_err(|_| ERROR_INVALID_PARAMETER)?;
    if payload.account.trim().is_empty() { return Err(ERROR_INVALID_PARAMETER); }
    match payload.action.as_str() {
        "join" => {
            let domain = payload.domain.as_deref().ok_or(ERROR_INVALID_PARAMETER)?;
            if domain.trim().is_empty() { return Err(ERROR_INVALID_PARAMETER); }
            join_current_token(domain, &payload.account, payload.ou.as_deref())
        }
        "leave" if payload.domain.is_none() && payload.ou.is_none() =>
            leave_current_token(&payload.account),
        _ => Err(ERROR_INVALID_PARAMETER),
    }
}

#[cfg(not(windows))]
pub fn status() -> anyhow::Result<String> {
    anyhow::bail!("la administración de dominio sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn join(_domain: &str, _account: &str, _ou: Option<&str>) -> anyhow::Result<()> {
    anyhow::bail!("la administración de dominio sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn leave(_account: &str) -> anyhow::Result<()> {
    anyhow::bail!("la administración de dominio sólo está disponible en Windows")
}
#[cfg(not(windows))]
pub fn run_elevated_payload(_encoded: &str) -> std::result::Result<(), u32> {
    Err(50)
}
