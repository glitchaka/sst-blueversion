#[cfg(windows)]
use std::{ffi::OsStr, mem::zeroed, os::windows::ffi::OsStrExt, ptr::null_mut};

#[cfg(windows)]
use anyhow::Result;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, SetLastError},
    Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES,
        SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

#[cfg(windows)]
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

/// Tries to enable a privilege already present in the current process token.
/// Returns false when the token does not contain that privilege.
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
