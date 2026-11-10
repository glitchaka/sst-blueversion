//! Native Windows Defender Firewall profile query (COM INetFwPolicy2).
//! Read-only. Does not launch netsh.exe, CMD, PowerShell or assume a local
//! policy is necessarily the effective Group Policy result.
use crate::core::CommandOutput;
use anyhow::Result;

#[derive(Clone, Copy)]
enum Target { All, Current, Domain, Private, Public }

pub fn execute(args: &[String]) -> Result<CommandOutput> {
    let words: Vec<_> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["help"] | ["?"] | ["--help"] => Ok(CommandOutput::ok(
            "notsh advfirewall — perfiles nativos de Windows Defender Firewall\n\
             notsh advfirewall show allprofiles\n\
             notsh advfirewall show currentprofile\n\
             notsh advfirewall show domainprofile|privateprofile|publicprofile\n\
             Muestra configuración local de perfiles. Reglas y modificaciones pendientes.\n"
        )),
        ["show", "allprofiles"] => native::show(Target::All),
        ["show", "currentprofile"] => native::show(Target::Current),
        ["show", "domainprofile"] => native::show(Target::Domain),
        ["show", "privateprofile"] => native::show(Target::Private),
        ["show", "publicprofile"] => native::show(Target::Public),
        _ => Ok(CommandOutput::error(
            "notsh advfirewall: opción no implementada; usa 'notsh advfirewall help'.", 2
        )),
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub fn show(_: Target) -> Result<CommandOutput> {
        Ok(CommandOutput::error("notsh advfirewall: requiere Windows", 2))
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, mem, ptr};
    use windows_sys::core::GUID;
    use windows_sys::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize,
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };

    const CLSID_POLICY: GUID = GUID {
        data1: 0xE2B3C97F, data2: 0x6AE1, data3: 0x41AC,
        data4: [0x81, 0x7A, 0xF6, 0xF9, 0x21, 0x66, 0xD7, 0xDD],
    };
    const IID_POLICY: GUID = GUID {
        data1: 0x98325047, data2: 0xC671, data3: 0x4174,
        data4: [0x8D, 0x81, 0xDE, 0xFC, 0xD3, 0xF0, 0x31, 0x86],
    };

    // COM's IDispatch base occupies slots 0..=6.
    // INetFwPolicy2: CurrentProfileTypes=7, FirewallEnabled=8,
    // BlockAllInboundTraffic=12. No write method is invoked.
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    type CurrentProfiles = unsafe extern "system" fn(*mut c_void, *mut i32) -> i32;
    type BoolProperty = unsafe extern "system" fn(*mut c_void, i32, *mut i16) -> i32;

    unsafe fn slot<T>(object: *mut c_void, index: usize) -> T {
        let vtable = unsafe { *(object as *mut *const *const c_void) };
        let function = unsafe { *vtable.add(index) };
        unsafe { mem::transmute_copy(&function) }
    }

    struct Apartment { uninitialize: bool }
    impl Apartment {
        fn start() -> std::result::Result<Self, CommandOutput> {
            let status = unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED as u32) };
            // RPC_E_CHANGED_MODE means COM was already initialized by the host
            // using another threading apartment; do not call CoUninitialize then.
            if status < 0 && status != 0x80010106u32 as i32 {
                return Err(CommandOutput::error(format!(
                    "notsh advfirewall: CoInitializeEx falló (HRESULT {status:#x})"
                ), 1));
            }
            Ok(Self { uninitialize: status >= 0 })
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.uninitialize { unsafe { CoUninitialize(); } }
        }
    }

    struct Policy {
        object: *mut c_void,
        _apartment: Apartment,
    }
    impl Policy {
        fn open() -> std::result::Result<Self, CommandOutput> {
            let apartment = Apartment::start()?;
            let mut object: *mut c_void = ptr::null_mut();
            let status = unsafe {
                CoCreateInstance(
                    &CLSID_POLICY, ptr::null_mut(), CLSCTX_INPROC_SERVER,
                    &IID_POLICY, &mut object
                )
            };
            if status < 0 || object.is_null() {
                return Err(CommandOutput::error(format!(
                    "notsh advfirewall: INetFwPolicy2 no está disponible (HRESULT {status:#x}); \
                     comprueba el servicio de Windows Defender Firewall."
                ), 1));
            }
            Ok(Self { object, _apartment: apartment })
        }
        fn active(&self) -> std::result::Result<i32, i32> {
            let call: CurrentProfiles = unsafe { slot(self.object, 7) };
            let mut mask = 0;
            let hr = unsafe { call(self.object, &mut mask) };
            if hr < 0 { Err(hr) } else { Ok(mask) }
        }
        fn bool_property(&self, slot_number: usize, profile: i32)
            -> std::result::Result<bool, i32>
        {
            let call: BoolProperty = unsafe { slot(self.object, slot_number) };
            let mut value = 0i16;
            let hr = unsafe { call(self.object, profile, &mut value) };
            if hr < 0 { Err(hr) } else { Ok(value != 0) }
        }
    }
    impl Drop for Policy {
        fn drop(&mut self) {
            let release: Release = unsafe { slot(self.object, 2) };
            unsafe { release(self.object); }
        }
    }
    pub fn show(target: Target) -> Result<CommandOutput> {
        let policy = match Policy::open() {
            Ok(value) => value, Err(error) => return Ok(error)
        };
        let active = match policy.active() {
            Ok(value) => value, Err(hr) => return Ok(CommandOutput::error(
                format!("notsh advfirewall: perfiles no disponibles (HRESULT {hr:#x})"), 1
            ))
        };
        let mut output = String::from(
            "WINDOWS DEFENDER FIREWALL — perfiles nativos\n\
             Fuente: INetFwPolicy2 (configuración local; GPO puede alterar la política efectiva)\n\n"
        );
        let mut selected = 0usize;
        let mut failed = 0usize;
        for (name, flag, show) in [
            ("DOMINIO", 1, matches!(target, Target::All | Target::Current | Target::Domain)),
            ("PRIVADA", 2, matches!(target, Target::All | Target::Current | Target::Private)),
            ("PÚBLICA", 4, matches!(target, Target::All | Target::Current | Target::Public)),
        ] {
            if !show || (matches!(target, Target::Current) && active & flag == 0) {
                continue;
            }
            selected += 1;
            output.push_str(&format!("[{name}] {}\n", if active & flag != 0 {
                "ACTIVO EN ESTE EQUIPO"
            } else {
                "NO ACTIVO EN ESTE EQUIPO"
            }));
            match policy.bool_property(8, flag) {
                Ok(enabled) => output.push_str(&format!(
                    "  Firewall local: {}\n", if enabled { "HABILITADO" } else { "DESHABILITADO" }
                )),
                Err(hr) => {
                    failed += 1;
                    output.push_str(&format!("  Firewall local: consulta fallida (HRESULT {hr:#x})\n"));
                }
            }
            match policy.bool_property(12, flag) {
                Ok(blocked) => output.push_str(&format!(
                    "  Bloqueo de todo tráfico entrante: {}\n",
                    if blocked { "SÍ" } else { "NO" }
                )),
                Err(hr) => {
                    failed += 1;
                    output.push_str(&format!("  Tráfico entrante: consulta fallida (HRESULT {hr:#x})\n"));
                }
            }
            output.push('\n');
        }
        if selected == 0 {
            output.push_str("Windows no informó perfiles activos.\n");
        }
        output.push_str(
            "No se enumeran reglas ni se modifican perfiles desde esta consulta.\n"
        );
        Ok(CommandOutput {
            stdout: output, stderr: String::new(),
            status: if selected > 0 && failed == selected * 2 { 1 } else { 0 }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutating_firewall_commands_are_not_implemented() {
        assert!(execute(&[]).unwrap().stdout.contains("show allprofiles"));
        assert_eq!(execute(&["set".into(), "allprofiles".into()]).unwrap().status, 2);
    }
}
