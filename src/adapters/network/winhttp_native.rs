//! Native WinHTTP proxy report, independent of netsh.exe.
use crate::core::CommandOutput;
use anyhow::Result;
pub fn execute(args: &[String]) -> Result<CommandOutput> {
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] | ["help"] | ["?"] | ["--help"] => Ok(CommandOutput::ok(
            "notsh winhttp — API WinHTTP\n\
             notsh winhttp show proxy    proxy predeterminado del equipo\n\
             Los cambios de proxy están pendientes de control administrativo.\n"
        )),
        ["show", "proxy"] => native::show_proxy(),
        _ => Ok(CommandOutput::error(
            "notsh winhttp: subcomando nativo no implementado. Consulta 'notsh winhttp help'.", 2
        )),
    }
}
#[cfg(not(windows))]
mod native {
    use super::*;
    pub fn show_proxy() -> Result<CommandOutput> {
        Ok(CommandOutput::error("notsh winhttp requiere Windows", 2))
    }
}
#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct ProxyInfo { access: u32, servers: *mut u16, bypass: *mut u16 }
    #[link(name = "winhttp")]
    unsafe extern "system" {
        fn WinHttpGetDefaultProxyConfiguration(info: *mut ProxyInfo) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalFree(memory: *mut c_void) -> *mut c_void;
    }
    fn wide(pointer: *const u16) -> String {
        if pointer.is_null() { return String::new(); }
        let mut len = 0usize;
        while len < 32767 && unsafe { *pointer.add(len) } != 0 { len += 1; }
        unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(pointer, len)) }
    }
    pub fn show_proxy() -> Result<CommandOutput> {
        let mut info = ProxyInfo {
            access: 0, servers: ptr::null_mut(), bypass: ptr::null_mut()
        };
        if unsafe { WinHttpGetDefaultProxyConfiguration(&mut info) } == 0 {
            return Ok(CommandOutput::error(format!(
                "notsh winhttp: error consultando WinHTTP ({})",
                std::io::Error::last_os_error()
            ), 1));
        }
        let servers = wide(info.servers);
        let bypass = wide(info.bypass);
        unsafe {
            if !info.servers.is_null() { GlobalFree(info.servers.cast()); }
            if !info.bypass.is_null() { GlobalFree(info.bypass.cast()); }
        }
        let mode = match info.access {
            1 => "DIRECTO",
            3 => "PROXY DEFINIDO",
            _ => "OTRO",
        };
        let mut out = format!("WINHTTP — CONFIGURACIÓN PREDETERMINADA\n  Modo: {mode}\n");
        if info.access == 3 {
            out.push_str(&format!(
                "  Servidores: {}\n  Exclusiones: {}\n",
                if servers.is_empty() { "-" } else { &servers },
                if bypass.is_empty() { "-" } else { &bypass }
            ));
        }
        out.push_str("  Fuente: Windows WinHTTP API; no equivale a la configuración de cada navegador.\n");
        Ok(CommandOutput::ok(out))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_invented_mutations() {
        assert!(execute(&[]).unwrap().stdout.contains("show proxy"));
        assert_eq!(execute(&["reset".into(), "proxy".into()]).unwrap().status, 2);
    }
}
