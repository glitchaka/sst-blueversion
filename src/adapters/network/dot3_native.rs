//! NOTSH wired 802.1X configuration using Windows dot3api.dll, without netsh.exe.
//!
//! The Dot3 API is loaded from the trusted Windows System32 directory. Dot3
//! reports profiles and interfaces, but neither a configured profile nor an IP
//! address proves successful RADIUS/802.1X authorization.
use crate::core::CommandOutput;
use anyhow::Result;

pub fn interfaces() -> Result<CommandOutput> {
    platform::interfaces()
}

pub fn profiles() -> Result<CommandOutput> {
    platform::profiles()
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub fn interfaces() -> Result<CommandOutput> {
        Ok(CommandOutput::error("NOTSH Dot3 requiere Windows", 2))
    }
    pub fn profiles() -> Result<CommandOutput> {
        Ok(CommandOutput::error("NOTSH Dot3 requiere Windows", 2))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{ffi::c_void, mem, ptr};
    use windows_sys::core::GUID;
    use windows_sys::Win32::{
        Foundation::{HANDLE, FreeLibrary},
        System::LibraryLoader::{
            GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32
        },
    };

    type Open = unsafe extern "system" fn(u32, *const c_void, *mut u32, *mut HANDLE) -> u32;
    type Close = unsafe extern "system" fn(HANDLE, *const c_void) -> u32;
    type Enumerate = unsafe extern "system" fn(HANDLE, *const c_void, *mut *mut Dot3List) -> u32;
    type GetProfile = unsafe extern "system" fn(HANDLE, *const GUID, *const c_void, *mut *mut u16) -> u32;
    type Free = unsafe extern "system" fn(*mut c_void);

    #[repr(C)]
    struct Dot3Info {
        guid: GUID,
        description: [u16; 256],
        state: u32,
    }

    #[repr(C)]
    struct Dot3List {
        count: u32,
        index: u32,
        infos: [Dot3Info; 1],
    }

    struct Dot3Library {
        module: *mut c_void,
        open: Open,
        close: Close,
        enumerate: Enumerate,
        profile: GetProfile,
        free: Free,
    }
    impl Drop for Dot3Library {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.module); }
        }
    }

    impl Dot3Library {
        fn load() -> std::result::Result<Self, CommandOutput> {
            let wide: Vec<u16> = "dot3api.dll".encode_utf16().chain(Some(0)).collect();
            let module = unsafe {
                LoadLibraryExW(wide.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
            };
            if module.is_null() {
                return Err(CommandOutput::error(
                    "NOTSH Dot3: Windows no expone dot3api.dll mediante System32.", 1
                ));
            }
            let exports: [&[u8]; 5] = [
                b"Dot3OpenHandle\0",
                b"Dot3CloseHandle\0",
                b"Dot3EnumInterfaces\0",
                b"Dot3GetProfile\0",
                b"Dot3FreeMemory\0",
            ];
            let mut pointers = Vec::new();
            for name in exports {
                let Some(address) = (unsafe { GetProcAddress(module, name.as_ptr()) }) else {
                    unsafe { FreeLibrary(module); }
                    return Err(CommandOutput::error(
                        format!("NOTSH Dot3: la biblioteca de Windows no implementa {}.",
                            String::from_utf8_lossy(&name[..name.len() - 1])),
                        1
                    ));
                };
                pointers.push(address);
            }
            // Win32 FARPROC and each of these function pointer types are pointer-sized.
            // The export names and extern "system" ABIs above match dot3api.dll.
            Ok(unsafe {
                Self {
                    module,
                    open: mem::transmute_copy(&pointers[0]),
                    close: mem::transmute_copy(&pointers[1]),
                    enumerate: mem::transmute_copy(&pointers[2]),
                    profile: mem::transmute_copy(&pointers[3]),
                    free: mem::transmute_copy(&pointers[4]),
                }
            })
        }
    }

    struct Dot3Client {
        lib: Dot3Library,
        handle: HANDLE,
    }

    struct Adapter {
        guid: GUID,
        description: String,
        state: u32,
    }

    impl Dot3Client {
        fn new() -> std::result::Result<Self, CommandOutput> {
            let lib = Dot3Library::load()?;
            let mut handle: HANDLE = ptr::null_mut();
            let mut negotiated = 0u32;
            let status = unsafe { (lib.open)(1, ptr::null(), &mut negotiated, &mut handle) };
            if status != 0 || handle.is_null() {
                return Err(CommandOutput::error(format!(
                    "NOTSH Dot3: no se puede abrir el servicio de autenticación cableada \
                     (Win32 {status}). Revisa dot3svc y los permisos de Windows."
                ), 1));
            }
            Ok(Self { lib, handle })
        }
        fn enumerate(&self) -> std::result::Result<Vec<Adapter>, CommandOutput> {
            let mut ptr_list: *mut Dot3List = ptr::null_mut();
            let status = unsafe {
                (self.lib.enumerate)(self.handle, ptr::null(), &mut ptr_list)
            };
            if status != 0 {
                return Err(CommandOutput::error(format!(
                    "NOTSH Dot3: Dot3EnumInterfaces falló (Win32 {status})."
                ), 1));
            }
            if ptr_list.is_null() { return Ok(Vec::new()); }
            let mut output = Vec::new();
            unsafe {
                let list = &*ptr_list;
                let count = (list.count as usize).min(256);
                let info_ptr = list.infos.as_ptr();
                for i in 0..count {
                    let info = &*info_ptr.add(i);
                    output.push(Adapter {
                        guid: ptr::read(&info.guid),
                        description: wide(&info.description),
                        state: info.state,
                    });
                }
                (self.lib.free)(ptr_list.cast());
            }
            Ok(output)
        }
        fn profile_xml(&self, guid: &GUID) -> std::result::Result<String, u32> {
            let mut profile: *mut u16 = ptr::null_mut();
            let status = unsafe {
                (self.lib.profile)(self.handle, guid, ptr::null(), &mut profile)
            };
            if status != 0 { return Err(status); }
            if profile.is_null() { return Err(1168); }
            let mut size = 0usize;
            // Bound the UTF-16 walk even if Windows unexpectedly returns malformed data.
            while size < 131_072 && unsafe { *profile.add(size) } != 0 { size += 1; }
            let xml = if size < 131_072 {
                unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(profile, size)) }
            } else {
                String::new()
            };
            unsafe { (self.lib.free)(profile.cast()); }
            if xml.is_empty() { Err(13) } else { Ok(xml) }
        }
    }

    impl Drop for Dot3Client {
        fn drop(&mut self) {
            unsafe { (self.lib.close)(self.handle, ptr::null()); }
        }
    }

    fn wide(s: &[u16]) -> String {
        let len = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        String::from_utf16_lossy(&s[..len])
    }

    fn xml_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let (_, tail) = xml.split_once(&open)?;
        let (value, _) = tail.split_once(&close)?;
        Some(value.trim())
    }

    fn profile_summary(xml: &str) -> String {
        let enabled = match xml_text(xml, "OneXEnabled")
            .or_else(|| xml_text(xml, "enable8021X")) {
            Some(s) if s.eq_ignore_ascii_case("true") => "SÍ",
            Some(s) if s.eq_ignore_ascii_case("false") => "NO",
            _ => "NO ESPECIFICADO",
        };
        let method = xml_text(xml, "authMode").unwrap_or("No especificado");
        format!("802.1X configurado: {enabled}\n  Modo de autenticación: {method}")
    }

    pub fn interfaces() -> Result<CommandOutput> {
        let client = match Dot3Client::new() { Ok(client) => client, Err(err) => return Ok(err) };
        let adapters = match client.enumerate() { Ok(list) => list, Err(err) => return Ok(err) };
        let mut out = format!(
            "DOT3 — {} interfaces administradas por Wired AutoConfig\n\
             Estado de autenticación: la presencia de perfil NO demuestra autorización NAC.\n\n",
            adapters.len()
        );
        if adapters.is_empty() {
            out.push_str("Windows Dot3 no tiene interfaces cableadas administradas.\n");
        }
        for (index, adapter) in adapters.iter().enumerate() {
            out.push_str(&format!("[{}] {}\n", index + 1, adapter.description));
            out.push_str(&format!(
                "  Estado Dot3 (código nativo): {}\n", adapter.state
            ));
            match client.profile_xml(&adapter.guid) {
                Ok(profile) => out.push_str(&format!("  Perfil: configurado\n  {}\n",
                    profile_summary(&profile))),
                Err(1168) => out.push_str("  Perfil: sin perfil asociado\n"),
                Err(code) => out.push_str(&format!(
                    "  Perfil: consulta no disponible (Win32 {code})\n"
                )),
            }
            out.push_str("  Autorización 802.1X/NAC efectiva: NO VERIFICADA\n\n");
        }
        Ok(CommandOutput::ok(out))
    }

    pub fn profiles() -> Result<CommandOutput> {
        let client = match Dot3Client::new() { Ok(client) => client, Err(err) => return Ok(err) };
        let adapters = match client.enumerate() { Ok(list) => list, Err(err) => return Ok(err) };
        let mut out = String::from(
            "PERFILES ETHERNET — Windows Dot3 API (sin XML sensible ni credenciales)\n"
        );
        if adapters.is_empty() { out.push_str("No hay interfaces Dot3 con perfiles.\n"); }
        for adapter in &adapters {
            out.push_str(&format!("\n[{}]\n", adapter.description));
            match client.profile_xml(&adapter.guid) {
                Ok(xml) => out.push_str(&format!("  Perfil: configurado\n  {}\n",
                    profile_summary(&xml))),
                Err(1168) => out.push_str("  Perfil: no existe\n"),
                Err(rc) => out.push_str(&format!(
                    "  Error consultando perfil: Win32 {rc}\n"
                )),
            }
        }
        Ok(CommandOutput::ok(out))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn xml_summary_does_not_leak_profile_xml() {
            let xml = "<LANProfile><OneXEnabled>true</OneXEnabled><authMode>machineOrUser</authMode><secret>not-for-console</secret></LANProfile>";
            let text = profile_summary(xml);
            assert!(text.contains("802.1X configurado: SÍ"));
            assert!(text.contains("machineOrUser"));
            assert!(!text.contains("not-for-console"));
        }
    }
}
