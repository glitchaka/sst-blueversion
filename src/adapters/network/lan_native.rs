//! Read-only Wired AutoConfig service health through Windows Service Control Manager.
//! Does not infer 802.1X authentication or NAC authorization from the service state.
#[cfg(windows)]
pub fn autoconfig_state() -> String {
    use std::{ptr, mem};
    use windows_sys::Win32::System::Services::*;
    unsafe {
        let manager = OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() { return "Wired AutoConfig (dot3svc): consulta no autorizada".to_owned(); }
        let name: Vec<u16> = "dot3svc".encode_utf16().chain(std::iter::once(0)).collect();
        let service = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS);
        if service.is_null() {
            CloseServiceHandle(manager);
            return "Wired AutoConfig (dot3svc): servicio no disponible o sin permisos".to_owned();
        }
        let mut status: SERVICE_STATUS_PROCESS = mem::zeroed();
        let mut needed = 0u32;
        let ok = QueryServiceStatusEx(
            service, SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed
        );
        CloseServiceHandle(service);
        CloseServiceHandle(manager);
        if ok == 0 {
            return "Wired AutoConfig (dot3svc): no se pudo consultar".to_owned();
        }
        let state = match status.dwCurrentState {
            SERVICE_RUNNING => "EJECUTÁNDOSE",
            SERVICE_STOPPED => "DETENIDO",
            SERVICE_START_PENDING => "INICIANDO",
            SERVICE_STOP_PENDING => "DETENIENDO",
            _ => "ESTADO NO DEFINIDO",
        };
        format!("Wired AutoConfig (dot3svc): {state} (esto NO indica autorización NAC)")
    }
}
#[cfg(not(windows))]
pub fn autoconfig_state() -> String {
    "Wired AutoConfig: requiere Windows".to_owned()
}
