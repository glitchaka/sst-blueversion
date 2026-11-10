//! Read-only HTTP.sys service configuration through the Windows HTTP Server API.
//! No netsh.exe, PowerShell, CMD, subprocess, registry scraping or synthetic data.
//! The IP-listen list restricts addresses, not sockets/ports currently listening.
use anyhow::Result;
use crate::core::CommandOutput;

pub fn execute(args: &[String]) -> Result<CommandOutput> {
    let words = args.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice() {
        [] | ["help"] | ["?"] | ["--help"] => Ok(CommandOutput::ok(
            "NOTSH HTTP — consultas nativas de HTTP.sys\n\
             notsh http show iplisten    direcciones permitidas por HTTP.sys\n\
             notsh http show urlacl      reservas de prefijos URL (sin publicar SDDL)\n\
             No modifica certificados, reglas, URLACL ni puertos.\n"
        )),
        ["show", "iplisten"] => native::ip_listen(),
        ["show", "urlacl"] => native::url_acl(),
        _ => Ok(CommandOutput::error(
            "notsh http: operación no implementada. Usa 'notsh http help'.", 2
        )),
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub fn ip_listen() -> Result<CommandOutput> {
        Ok(CommandOutput::error("NOTSH HTTP necesita Windows.", 2))
    }
    pub fn url_acl() -> Result<CommandOutput> {
        Ok(CommandOutput::error("NOTSH HTTP necesita Windows.", 2))
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, net::{Ipv4Addr, Ipv6Addr}, ptr, mem::size_of};

    const HTTP_CONFIG: u32 = 2;
    const IP_LISTEN_ID: i32 = 0;
    const URL_ACL_ID: i32 = 2;
    const QUERY_NEXT: u32 = 1;
    const INSUFFICIENT: u32 = 122;
    const NO_MORE_ITEMS: u32 = 259;
    const MAX_QUERY_BYTES: usize = 1024 * 1024;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct HttpVersion {
        major: u16,
        minor: u16,
    }

    #[repr(C)]
    struct UrlAclQuery {
        kind: u32,
        key: *const u16,
        token: u32,
    }

    #[repr(C)]
    struct UrlAclRecord {
        prefix: *const u16,
        _sddl: *const u16,
    }

    #[link(name = "httpapi")]
    unsafe extern "system" {
        fn HttpInitialize(version: HttpVersion, flags: u32, reserved: *mut c_void) -> u32;
        fn HttpTerminate(flags: u32, reserved: *mut c_void) -> u32;
        fn HttpQueryServiceConfiguration(
            handle: *mut c_void,
            config_id: i32,
            input: *const c_void,
            input_length: u32,
            output: *mut c_void,
            output_length: u32,
            required_length: *mut u32,
            overlapped: *mut c_void,
        ) -> u32;
    }

    struct Session;
    impl Session {
        fn open() -> std::result::Result<Self, CommandOutput> {
            let result = unsafe {
                HttpInitialize(HttpVersion { major: 1, minor: 0 },
                    HTTP_CONFIG, ptr::null_mut())
            };
            if result != 0 {
                Err(CommandOutput::error(format!(
                    "NOTSH HTTP: HttpInitialize rechazado por Windows (Win32 {result})."
                ), 1))
            } else {
                Ok(Self)
            }
        }
    }
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe { HttpTerminate(HTTP_CONFIG, ptr::null_mut()); }
        }
    }

    /// The HTTP Server API requires an aligned buffer whose size comes from
    /// ERROR_INSUFFICIENT_BUFFER. Store it as u64 words to keep pointer fields aligned.
    fn query(config_id: i32, input: *const c_void, input_length: u32)
        -> std::result::Result<Option<(Vec<u64>, usize)>, u32>
    {
        let mut required = 0u32;
        let first = unsafe {
            HttpQueryServiceConfiguration(ptr::null_mut(), config_id, input,
                input_length, ptr::null_mut(), 0, &mut required, ptr::null_mut())
        };
        if config_id == URL_ACL_ID && first == NO_MORE_ITEMS {
            return Ok(None);
        }
        if first != INSUFFICIENT && first != 0 {
            return Err(first);
        }
        if required == 0 {
            return Ok(None);
        }
        let size = required as usize;
        if size > MAX_QUERY_BYTES {
            return Err(8); // ERROR_NOT_ENOUGH_MEMORY: do not allocate arbitrary sizes.
        }
        let mut storage = vec![0u64; size.div_ceil(size_of::<u64>())];
        let mut written = 0u32;
        let result = unsafe {
            HttpQueryServiceConfiguration(ptr::null_mut(), config_id, input,
                input_length, storage.as_mut_ptr().cast(),
                (storage.len() * size_of::<u64>()) as u32,
                &mut written, ptr::null_mut())
        };
        if result != 0 {
            return Err(result);
        }
        let bytes = if written == 0 { size } else { written as usize };
        if bytes > storage.len() * size_of::<u64>() {
            return Err(13); // Invalid output size from the API.
        }
        Ok(Some((storage, bytes)))
    }

    fn parse_socket_address(raw: &[u8]) -> Option<String> {
        if raw.len() < 128 { return None; }
        let family = u16::from_ne_bytes([raw[0], raw[1]]);
        match family {
            2 => Some(Ipv4Addr::new(raw[4], raw[5], raw[6], raw[7]).to_string()),
            23 => {
                let bytes: [u8; 16] = raw[8..24].try_into().ok()?;
                let scope = u32::from_ne_bytes(raw[24..28].try_into().ok()?);
                let address = Ipv6Addr::from(bytes);
                if scope != 0 {
                    Some(format!("{address}%{scope}"))
                } else {
                    Some(address.to_string())
                }
            }
            _ => Some(format!("<familia de dirección {family}>")),
        }
    }

    pub fn ip_listen() -> Result<CommandOutput> {
        let _session = match Session::open() {
            Ok(s) => s, Err(e) => return Ok(e)
        };
        let (buffer, valid) = match query(IP_LISTEN_ID, ptr::null(), 0) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(CommandOutput::ok(
                "HTTP.sys — no se informó una lista IPListen específica.\n"
            )),
            Err(code) => return Ok(CommandOutput::error(format!(
                "NOTSH HTTP: consulta IPListen no disponible (Win32 {code})."
            ), 1)),
        };
        let data = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), valid)
        };
        if data.len() < 8 {
            return Ok(CommandOutput::error("HTTP.sys devolvió una lista IPListen incompleta.", 1));
        }
        let count = u32::from_ne_bytes(data[..4].try_into().expect("4 bytes")) as usize;
        if count > 4096 || count.checked_mul(128).and_then(|v| v.checked_add(8))
            .is_none_or(|required| required > data.len())
        {
            return Ok(CommandOutput::error(
                "HTTP.sys devolvió una cantidad de IP incompatible con su buffer.", 1
            ));
        }
        let mut out = format!("HTTP.sys — LISTA IPListen ({} direcciones)\n", count);
        if count == 0 {
            out.push_str("  Sin restricciones específicas. No representa puertos abiertos.\n");
        }
        for index in 0..count {
            let start = 8 + index * 128;
            let address = parse_socket_address(&data[start..start + 128])
                .unwrap_or_else(|| "<dirección no válida>".to_string());
            out.push_str(&format!("  {}. {}\n", index + 1, address));
        }
        out.push_str("  Nota: una dirección IPListen no demuestra que exista un servidor HTTP activo.\n");
        Ok(CommandOutput::ok(out))
    }

    fn prefixed_utf16(buffer: &[u64], len: usize, pointer: *const u16) -> Option<String> {
        if pointer.is_null() { return None; }
        let base = buffer.as_ptr() as usize;
        let addr = pointer as usize;
        let end = base.checked_add(len)?;
        if addr < base || addr >= end || (addr - base) % 2 != 0 {
            return None;
        }
        let max_chars = ((end - addr) / 2).min(2048);
        if max_chars == 0 { return None; }
        let words = unsafe { std::slice::from_raw_parts(pointer, max_chars) };
        let n = words.iter().position(|&ch| ch == 0)?;
        Some(String::from_utf16_lossy(&words[..n]))
    }

    pub fn url_acl() -> Result<CommandOutput> {
        let _session = match Session::open() {
            Ok(s) => s, Err(e) => return Ok(e)
        };
        let mut out = String::from(
            "HTTP.sys — RESERVAS URL (lectura nativa, sin descriptores SDDL)\n"
        );
        let mut entries = 0usize;
        for token in 0..2048u32 {
            let request = UrlAclQuery {
                kind: QUERY_NEXT, key: ptr::null(), token
            };
            let result = query(
                URL_ACL_ID, (&request as *const UrlAclQuery).cast(),
                size_of::<UrlAclQuery>() as u32
            );
            let (storage, valid) = match result {
                Ok(Some(data)) => data,
                Ok(None) => break,
                Err(code) => return Ok(CommandOutput::error(format!(
                    "NOTSH HTTP: consulta URLACL falló en índice {token} (Win32 {code})."
                ), 1)),
            };
            if valid < size_of::<UrlAclRecord>() {
                return Ok(CommandOutput::error(
                    "HTTP.sys devolvió un registro URLACL incompleto.", 1
                ));
            }
            let record = unsafe {
                (storage.as_ptr() as *const UrlAclRecord).read()
            };
            let Some(prefix) = prefixed_utf16(&storage, valid, record.prefix) else {
                return Ok(CommandOutput::error(
                    "HTTP.sys devolvió un prefijo URLACL fuera de su buffer.", 1
                ));
            };
            entries += 1;
            out.push_str(&format!("  {}. {}\n", entries, prefix));
        }
        if entries == 0 { out.push_str("  Sin reservas URL registradas.\n"); }
        if entries == 2048 { out.push_str("  Se truncó el listado al límite de 2048 registros.\n"); }
        out.push_str("  Nota: una reserva URL no significa que el servicio esté escuchando.\n");
        Ok(CommandOutput::ok(out))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn sockaddr_ipv4_and_ipv6_are_parsed_from_native_buffers() {
            let mut bytes = [0u8; 128];
            bytes[0..2].copy_from_slice(&2u16.to_ne_bytes());
            bytes[4..8].copy_from_slice(&[192, 168, 1, 70]);
            assert_eq!(parse_socket_address(&bytes).as_deref(), Some("192.168.1.70"));
            bytes = [0; 128];
            bytes[0..2].copy_from_slice(&23u16.to_ne_bytes());
            bytes[23] = 1;
            assert_eq!(parse_socket_address(&bytes).as_deref(), Some("::1"));
        }

        #[test]
        fn url_acl_buffer_never_reads_outside_the_api_response() {
            let buffer = vec![0u64; 8];
            assert_eq!(prefixed_utf16(&buffer, 64, ptr::null()), None);
            assert_eq!(prefixed_utf16(&buffer, 16,
                unsafe { (buffer.as_ptr() as *const u16).add(16) }), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_documented_native_read_queries_are_supported() {
        let help = execute(&[]).unwrap();
        assert!(help.stdout.contains("notsh http show iplisten"));
        assert!(help.stdout.contains("notsh http show urlacl"));
        assert_eq!(execute(&["delete".into(), "urlacl".into()]).unwrap().status, 2);
    }
}
