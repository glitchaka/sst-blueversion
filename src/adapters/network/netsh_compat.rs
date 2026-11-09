//! Windows Netsh compatibility adapter for commands requiring Wired AutoConfig
//! or Native Wi-Fi authentication/profile status. Unlike the IP Helper adapter,
//! this preserves the authoritative 802.1X and WLAN semantics of Windows.
//! It invokes only the official System32 netsh.exe, never CMD or PowerShell.

use std::path::Path;
use anyhow::Result;
use crate::core::CommandOutput;

fn arguments(namespace: &str, args: &[String]) -> Vec<String> {
    let mut actual = vec![namespace.to_owned()];
    actual.extend(args.iter().cloned());
    if args.is_empty() || (namespace == "interface" && args.len() == 1
        && matches!(args[0].as_str(), "ipv4" | "ipv6")) {
        actual.push("help".to_owned());
    }
    actual
}

#[cfg(windows)]
fn decode(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::MultiByteToWideChar;
    if bytes.starts_with(&[0xff, 0xfe]) {
        let words: Vec<u16> = bytes[2..].chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        return String::from_utf16_lossy(&words);
    }
    if let Ok(utf8) = std::str::from_utf8(bytes) { return utf8.to_owned(); }
    if bytes.is_empty() { return String::new(); }
    let Ok(length) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    // CP_OEMCP=1. Netsh often writes the console OEM code page to pipes.
    let length16 = unsafe {
        MultiByteToWideChar(1, 0, bytes.as_ptr(), length, std::ptr::null_mut(), 0)
    };
    if length16 == 0 { return String::from_utf8_lossy(bytes).into_owned(); }
    let mut wide = vec![0u16; length16 as usize];
    let written = unsafe {
        MultiByteToWideChar(1, 0, bytes.as_ptr(), length, wide.as_mut_ptr(), length16)
    };
    if written == 0 { return String::from_utf8_lossy(bytes).into_owned(); }
    String::from_utf16_lossy(&wide[..written as usize])
}

#[cfg(windows)]
pub fn execute(namespace: &str, args: &[String], cwd: &Path) -> Result<CommandOutput> {
    use anyhow::Context;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let executable = crate::support::windows::system32_executable("netsh.exe")
        .context("notsh: no se encuentra el componente de red oficial de Windows")?;
    let output = Command::new(executable)
        .args(arguments(namespace, args))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .with_context(|| format!("notsh {namespace}: el contexto de Windows no pudo ejecutarse"))?;
    Ok(CommandOutput {
        stdout: decode(&output.stdout),
        stderr: decode(&output.stderr),
        status: output.status.code().unwrap_or(1),
    })
}

#[cfg(not(windows))]
pub fn execute(namespace: &str, args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let _ = (args, cwd);
    Ok(CommandOutput::error(
        format!("notsh {namespace}: requiere Windows"), 2
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_full_windows_context_not_generic_interface_list() {
        assert_eq!(
            arguments("lan", &["show".into(), "interfaces".into()]),
            ["lan", "show", "interfaces"]
        );
        assert_eq!(
            arguments("lan", &["show".into(), "profiles".into()]),
            ["lan", "show", "profiles"]
        );
        assert_eq!(
            arguments("wlan", &["show".into(), "interfaces".into()]),
            ["wlan", "show", "interfaces"]
        );
        assert_eq!(
            arguments("wlan", &["show".into(), "networks".into()]),
            ["wlan", "show", "networks"]
        );
        assert_eq!(
            arguments("interface", &["ipv4".into(), "show".into(), "interfaces".into()]),
            ["interface", "ipv4", "show", "interfaces"]
        );
    }

    #[test]
    fn namespace_alone_does_not_launch_interactive_netsh() {
        assert_eq!(arguments("lan", &[]), ["lan", "help"]);
        assert_eq!(arguments("wlan", &[]), ["wlan", "help"]);
        assert_eq!(arguments("interface", &["ipv4".into()]), ["interface", "ipv4", "help"]);
    }

    #[cfg(windows)]
    #[test]
    fn real_windows_backend_returns_lan_status_or_dot3svc_diagnostic() {
        // dot3svc may not be running on CI; both a status and a real Windows
        // diagnostic are valid. A generic IP Helper dump is not.
        let result = execute("lan", &["show".into(), "interfaces".into()],
            &std::env::temp_dir()).unwrap();
        assert!(!result.stdout.is_empty() || !result.stderr.is_empty());
        assert!(!result.stdout.starts_with("LAN CABLEADA —"));
    }
}
