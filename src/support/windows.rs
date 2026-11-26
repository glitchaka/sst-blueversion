#[cfg(windows)]
use std::{
    ffi::OsStr,
    mem::zeroed,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::null_mut,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(windows)]
use anyhow::Result;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, SetLastError},
    Security::{
        AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES,
        SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
        TokenPrivileges,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

#[cfg(windows)]
static SHELL_HANDOFF_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
pub fn request_shell_handoff() {
    SHELL_HANDOFF_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(windows)]
pub fn take_shell_handoff_request() -> bool {
    SHELL_HANDOFF_REQUESTED.swap(false, Ordering::SeqCst)
}

#[cfg(not(windows))]
pub fn request_shell_handoff() {}

#[cfg(not(windows))]
pub fn take_shell_handoff_request() -> bool { false }

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

/// Tries to enable a privilege already present in the current process token.
/// Returns false when the token does not contain that privilege.
#[cfg(windows)]
pub fn system_executable(relative: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            !matches!(component, std::path::Component::Normal(_))
        })
    {
        anyhow::bail!("ruta de ejecutable del sistema inválida: {relative}");
    }

    let root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let path = root.join("System32").join(candidate);
    if !path.is_file() {
        anyhow::bail!("ejecutable del sistema no encontrado: {}", path.display());
    }
    Ok(path)
}

#[cfg(not(windows))]
pub fn system_executable(relative: &str) -> anyhow::Result<std::path::PathBuf> {
    anyhow::bail!("{relative}: sólo disponible en Windows")
}

pub fn system32_executable(name: &str) -> anyhow::Result<std::path::PathBuf> {
    system_executable(name)
}

#[cfg(windows)]
pub fn enable_privilege(name: &str) -> Result<bool> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
            &mut token,
        ) == 0
        {
            anyhow::bail!(
                "OpenProcessToken falló con error Win32 {}",
                GetLastError()
            );
        }

        let result = (|| -> Result<bool> {
            let mut luid = zeroed();
            let name_w = wide(name);
            if LookupPrivilegeValueW(std::ptr::null(), name_w.as_ptr(), &mut luid) == 0 {
                anyhow::bail!(
                    "LookupPrivilegeValueW({name}) falló con error Win32 {}",
                    GetLastError()
                );
            }

            let mut privileges: TOKEN_PRIVILEGES = zeroed();
            privileges.PrivilegeCount = 1;
            privileges.Privileges[0] = LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            };

            SetLastError(0);
            if AdjustTokenPrivileges(
                token,
                0,
                &privileges,
                0,
                null_mut(),
                null_mut(),
            ) == 0
            {
                anyhow::bail!(
                    "AdjustTokenPrivileges({name}) falló con error Win32 {}",
                    GetLastError()
                );
            }

            // ERROR_NOT_ALL_ASSIGNED
            Ok(GetLastError() != 1300)
        })();

        CloseHandle(token);
        result
    }
}

#[cfg(not(windows))]
pub fn enable_privilege(_name: &str) -> anyhow::Result<bool> {
    Ok(false)
}

#[cfg(windows)]
pub fn enable_all_token_privileges() -> Result<usize> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
            &mut token,
        ) == 0
        {
            anyhow::bail!(
                "OpenProcessToken falló con error Win32 {}",
                GetLastError()
            );
        }

        let result = (|| -> Result<usize> {
            let mut size = 0u32;
            GetTokenInformation(token, TokenPrivileges, null_mut(), 0, &mut size);
            if size == 0 {
                return Ok(0);
            }

            let mut buffer = vec![0u8; size as usize];
            if GetTokenInformation(
                token,
                TokenPrivileges,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            ) == 0
            {
                anyhow::bail!(
                    "GetTokenInformation(TokenPrivileges) falló con error Win32 {}",
                    GetLastError()
                );
            }

            let privileges = buffer.as_mut_ptr().cast::<TOKEN_PRIVILEGES>();
            let count = (*privileges).PrivilegeCount as usize;
            let first = (*privileges).Privileges.as_mut_ptr();
            let entries = std::slice::from_raw_parts_mut(first, count);

            for entry in entries.iter_mut() {
                entry.Attributes |= SE_PRIVILEGE_ENABLED;
            }

            SetLastError(0);
            if AdjustTokenPrivileges(
                token,
                0,
                privileges,
                0,
                null_mut(),
                null_mut(),
            ) == 0
            {
                anyhow::bail!(
                    "AdjustTokenPrivileges(all) falló con error Win32 {}",
                    GetLastError()
                );
            }

            let error = GetLastError();
            if error != 0 && error != 1300 {
                anyhow::bail!(
                    "AdjustTokenPrivileges(all) devolvió error Win32 {error}"
                );
            }

            // Re-read the token and count privileges that are actually enabled.
            let mut verify_size = 0u32;
            GetTokenInformation(token, TokenPrivileges, null_mut(), 0, &mut verify_size);
            if verify_size == 0 {
                return Ok(0);
            }
            let mut verify = vec![0u8; verify_size as usize];
            if GetTokenInformation(
                token,
                TokenPrivileges,
                verify.as_mut_ptr().cast(),
                verify_size,
                &mut verify_size,
            ) == 0
            {
                anyhow::bail!(
                    "GetTokenInformation(TokenPrivileges) de verificación falló con error Win32 {}",
                    GetLastError()
                );
            }

            let verified = verify.as_ptr().cast::<TOKEN_PRIVILEGES>();
            let verified_count = (*verified).PrivilegeCount as usize;
            let verified_entries =
                std::slice::from_raw_parts((*verified).Privileges.as_ptr(), verified_count);
            Ok(verified_entries
                .iter()
                .filter(|entry| entry.Attributes & SE_PRIVILEGE_ENABLED != 0)
                .count())
        })();

        CloseHandle(token);
        result
    }
}

#[cfg(not(windows))]
pub fn enable_all_token_privileges() -> anyhow::Result<usize> {
    Ok(0)
}


#[cfg(all(test, windows))]
mod tests {
    use super::system_executable;

    #[test]
    fn system_executable_rejects_path_traversal() {
        assert!(system_executable(r"..\curl.exe").is_err());
        assert!(system_executable(r"subdir\..\curl.exe").is_err());
    }

    #[test]
    fn system_executable_rejects_absolute_paths() {
        assert!(system_executable(r"C:\Windows\System32\curl.exe").is_err());
        assert!(system_executable(r"\\server\share\tool.exe").is_err());
    }
}


/// Decode captured Windows console output without discarding localized text.
/// Unicode BOMs and valid UTF-8 take precedence; older system tools commonly
/// write the system OEM code page to redirected handles.
pub fn decode_console_output(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let wide: Vec<u16> = rest.chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        return String::from_utf16_lossy(&wide);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::MultiByteToWideChar;
        if let Ok(len) = i32::try_from(bytes.len()) {
            // CP_OEMCP = 1, used by traditional Windows console utilities.
            let count = unsafe { MultiByteToWideChar(1, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0) };
            if count > 0 {
                let mut wide = vec![0u16; count as usize];
                let written = unsafe { MultiByteToWideChar(1, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), count) };
                if written > 0 {
                    return String::from_utf16_lossy(&wide[..written as usize]);
                }
            }
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod console_decode_tests {
    use super::decode_console_output;
    #[test]
    fn preserves_unicode_text_and_boms() {
        assert_eq!(decode_console_output("Configuración".as_bytes()), "Configuración");
        assert_eq!(decode_console_output(&[0xFF,0xFE,0xE1,0]), "á");
        assert_eq!(decode_console_output(&[0xEF,0xBB,0xBF,b'A']), "A");
    }
}
