//! Native, read-only Event Tracing for Windows (ETW) session inventory.
//! Does not invoke netsh.exe, logman.exe, CMD, PowerShell or create a trace.
//! Returned sessions are limited by the caller's Windows permissions.
use crate::core::CommandOutput;
use anyhow::Result;

pub fn execute(args: &[String]) -> Result<CommandOutput> {
    let words = args.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice() {
        [] | ["help"] | ["?"] | ["--help"] => Ok(CommandOutput::ok(
            "NOTSH TRACE — sesiones ETW consultadas por API de Windows\n\
             notsh trace show sessions    sesiones ETW visibles al usuario\n\
             Esto no inicia ni modifica trazas. 'notsh trace HOST' sigue siendo traceroute.\n"
        )),
        ["show", "sessions"] => native::sessions(),
        _ => Ok(CommandOutput::error(
            "notsh trace: función de control ETW no implementada de forma nativa. \
             Usa 'notsh trace show sessions' para consultar sesiones existentes.", 2
        )),
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub fn sessions() -> Result<CommandOutput> {
        Ok(CommandOutput::error("NOTSH ETW requiere Windows.", 2))
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::mem::size_of;

    const ERROR_MORE_DATA: u32 = 234;
    const MAX_SESSIONS: usize = 256;
    const SESSION_CHARS: usize = 1024;
    const PATH_CHARS: usize = 1024;

    // The ABI below matches WNODE_HEADER and EVENT_TRACE_PROPERTIES from
    // evntrace.h on x64 Windows, the architecture targeted by SST.
    #[repr(C)]
    struct WnodeHeader {
        buffer_size: u32,
        provider_id: u32,
        historical_context: u64,
        kernel_handle: usize,
        guid: [u8; 16],
        client_context: u32,
        flags: u32,
    }

    #[repr(C)]
    struct TraceProperties {
        wnode: WnodeHeader,
        buffer_size: u32,
        minimum_buffers: u32,
        maximum_buffers: u32,
        maximum_file_size: u32,
        log_file_mode: u32,
        flush_timer: u32,
        enable_flags: u32,
        age_limit: i32,
        number_of_buffers: u32,
        free_buffers: u32,
        events_lost: u32,
        buffers_written: u32,
        log_buffers_lost: u32,
        realtime_buffers_lost: u32,
        logger_thread_id: usize,
        logfile_name_offset: u32,
        logger_name_offset: u32,
    }

    // Sechost.lib is not distributed in every Windows build environment.
    // Resolve the documented export from trusted System32 at runtime instead.
    type QueryAllTraces = unsafe extern "system" fn(
        *mut *mut TraceProperties, u32, *mut u32
    ) -> u32;

    struct EtwLibrary {
        module: *mut std::ffi::c_void,
        query: QueryAllTraces,
    }
    impl EtwLibrary {
        fn open() -> std::result::Result<Self, CommandOutput> {
            use windows_sys::Win32::Foundation::FreeLibrary;
            use windows_sys::Win32::System::LibraryLoader::{
                GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32
            };
            for name in ["sechost.dll", "advapi32.dll"] {
                let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                let module = unsafe {
                    LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
                };
                if module.is_null() { continue; }
                let proc = unsafe { GetProcAddress(module, b"QueryAllTracesW\0".as_ptr()) };
                if let Some(pointer) = proc {
                    // Both are pointer-sized; the ABI is documented as WMIAPI/stdcall.
                    let query: QueryAllTraces = unsafe { std::mem::transmute_copy(&pointer) };
                    return Ok(Self { module, query });
                }
                unsafe { FreeLibrary(module); }
            }
            Err(CommandOutput::error(
                "NOTSH ETW: Windows no expone QueryAllTracesW en las DLL oficiales de System32.",
                1
            ))
        }
    }
    impl Drop for EtwLibrary {
        fn drop(&mut self) {
            unsafe { windows_sys::Win32::Foundation::FreeLibrary(self.module); }
        }
    }

    fn wide_at(storage: &[u64], offset: u32) -> Option<String> {
        let byte_offset = offset as usize;
        let bytes = storage.len() * size_of::<u64>();
        if byte_offset == 0 || byte_offset % 2 != 0 || byte_offset >= bytes {
            return None;
        }
        let len = ((bytes - byte_offset) / 2).min(SESSION_CHARS);
        let start = unsafe {
            storage.as_ptr().cast::<u8>().add(byte_offset).cast::<u16>()
        };
        let chars = unsafe { std::slice::from_raw_parts(start, len) };
        let end = chars.iter().position(|&ch| ch == 0)?;
        Some(String::from_utf16_lossy(&chars[..end]))
    }

    pub fn sessions() -> Result<CommandOutput> {
        if size_of::<WnodeHeader>() != 48 || size_of::<TraceProperties>() != 120 {
            return Ok(CommandOutput::error(
                "NOTSH ETW: el diseño ABI no coincide con EVENT_TRACE_PROPERTIES de x64.", 1
            ));
        }
        let etw = match EtwLibrary::open() { Ok(api) => api, Err(err) => return Ok(err) };
        let mut capacity = 64usize;
        loop {
            let bytes_per_session = size_of::<TraceProperties>()
                + (SESSION_CHARS + PATH_CHARS) * size_of::<u16>();
            let mut buffers = Vec::with_capacity(capacity);
            let mut addresses: Vec<*mut TraceProperties> = Vec::with_capacity(capacity);
            for _ in 0..capacity {
                let mut block = vec![0u64; bytes_per_session.div_ceil(size_of::<u64>())];
                let p = block.as_mut_ptr().cast::<TraceProperties>();
                unsafe {
                    (*p).wnode.buffer_size = (block.len() * size_of::<u64>()) as u32;
                    (*p).logger_name_offset = size_of::<TraceProperties>() as u32;
                    (*p).logfile_name_offset =
                        (size_of::<TraceProperties>() + SESSION_CHARS * size_of::<u16>()) as u32;
                }
                addresses.push(p);
                buffers.push(block);
            }

            let mut count = 0u32;
            let result = unsafe {
                (etw.query)(addresses.as_mut_ptr(), capacity as u32, &mut count)
            };
            if result == ERROR_MORE_DATA && count as usize > capacity && count as usize <= MAX_SESSIONS {
                capacity = count as usize;
                continue;
            }
            if result != 0 {
                return Ok(CommandOutput::error(format!(
                    "NOTSH ETW: QueryAllTracesW falló (Win32 {result}). \
                     Windows puede limitar las sesiones visibles según los permisos.", 
                ), 1));
            }
            if count as usize > capacity {
                return Ok(CommandOutput::error(
                    "NOTSH ETW: la API informó más sesiones de las solicitadas.", 1
                ));
            }
            let mut out = format!(
                "ETW — {} SESIONES DE TRAZADO VISIBLES\n\
                 Fuente: QueryAllTracesW. No incluye sesiones privadas ni sesiones sin permisos.\n\n",
                count
            );
            if count == 0 {
                out.push_str("Windows no expuso sesiones de trazado para este usuario.\n");
            }
            for index in 0..count as usize {
                let p = unsafe { &*addresses[index] };
                let session_name = wide_at(&buffers[index], p.logger_name_offset)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "<sin nombre público>".to_owned());
                out.push_str(&format!("[{}] {}\n", index + 1, session_name));
                out.push_str(&format!(
                    "  Búferes escritos: {}\n  Eventos perdidos: {}\n  Búferes de archivo perdidos: {}\n  Búferes en tiempo real perdidos: {}\n",
                    p.buffers_written,
                    p.events_lost,
                    p.log_buffers_lost,
                    p.realtime_buffers_lost
                ));
                if let Some(path) = wide_at(&buffers[index], p.logfile_name_offset) {
                    if !path.is_empty() {
                        out.push_str(&format!("  Archivo ETL: {}\n", path));
                    }
                }
                out.push('\n');
            }
            out.push_str(
                "Esta consulta enumera sesiones ETW del sistema; no representa el estado de una traza creada por SST.\n"
            );
            return Ok(CommandOutput::ok(out));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn abi_sizes_match_windows_x64() {
            assert_eq!(size_of::<WnodeHeader>(), 48);
            assert_eq!(size_of::<TraceProperties>(), 120);
        }
        #[test]
        fn returned_strings_are_bounded_to_the_query_buffer() {
            let block = vec![0u64; 16];
            assert_eq!(wide_at(&block, 0), None);
            assert_eq!(wide_at(&block, 129), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_read_only_sessions_command_is_accepted() {
        let help = execute(&[]).unwrap().stdout;
        assert!(help.contains("notsh trace show sessions"));
        assert_eq!(execute(&["start".into()]).unwrap().status, 2);
    }
}
