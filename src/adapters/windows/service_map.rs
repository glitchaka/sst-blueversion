//! Resolve Windows service names to PIDs via the Service Control Manager.
//! This is an ownership hint. Shared host processes do not provide per-service
//! byte attribution, and must never be reported as such.
use std::{collections::HashMap, ffi::c_void, ptr};
use anyhow::{bail, Result};

type ServiceHandle = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct ServiceStatusProcess {
    service_type: u32,
    current_state: u32,
    controls_accepted: u32,
    win32_exit_code: u32,
    service_specific_exit_code: u32,
    checkpoint: u32,
    wait_hint: u32,
    process_id: u32,
    service_flags: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct EnumServiceStatusProcess {
    service_name: *const u16,
    display_name: *const u16,
    state: ServiceStatusProcess,
}
#[link(name="advapi32")]
unsafe extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> ServiceHandle;
    fn EnumServicesStatusExW(manager: ServiceHandle, level: u32, kind: u32,
        state: u32, buffer: *mut u8, buffer_size: u32, needed: *mut u32,
        count: *mut u32, resume: *mut u32, group: *const u16) -> i32;
    fn CloseServiceHandle(manager: ServiceHandle) -> i32;
}
struct Manager(ServiceHandle);
impl Drop for Manager {
    fn drop(&mut self) { unsafe { CloseServiceHandle(self.0); } }
}
fn wide_in_buffer(base: *const u8, length: usize, start: *const u16) -> Option<String> {
    if start.is_null() { return None; }
    let begin = start as usize;
    let low = base as usize;
    let high = low.checked_add(length)?;
    if begin < low || begin >= high || begin % 2 != 0 { return None; }
    let available = (high - begin) / 2;
    let max_chars = available.min(256);
    let slice = unsafe { std::slice::from_raw_parts(start, max_chars) };
    let end = slice.iter().position(|ch| *ch == 0)?;
    Some(String::from_utf16_lossy(&slice[..end]))
}
pub fn services_by_pid() -> Result<HashMap<u32, Vec<String>>> {
    const SC_MANAGER_ENUMERATE_SERVICE: u32 = 0x0004;
    const SERVICE_WIN32: u32 = 0x0030;
    const SERVICE_STATE_ALL: u32 = 0x0003;
    let handle = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_ENUMERATE_SERVICE) };
    if handle.is_null() { bail!("SCM: {}", std::io::Error::last_os_error()); }
    let manager = Manager(handle);
    let mut capacity = 512 * 1024usize;
    for _ in 0..3 {
        let mut buffer = vec![0u64; capacity.div_ceil(8)];
        let ptr = buffer.as_mut_ptr().cast::<u8>();
        let mut needed = 0u32;
        let mut count = 0u32;
        let mut resume = 0u32;
        let ok = unsafe {
            EnumServicesStatusExW(manager.0, 0, SERVICE_WIN32, SERVICE_STATE_ALL, ptr,
                capacity as u32, &mut needed, &mut count, &mut resume, ptr::null())
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(234) && (needed as usize) <= 4 * 1024 * 1024 {
                capacity = (needed as usize).max(capacity * 2);
                continue;
            }
            return Err(error.into());
        }
        let max = capacity / size_of::<EnumServiceStatusProcess>();
        if count as usize > max { bail!("SCM: recuento de servicios inconsistente"); }
        let entries = unsafe {
            std::slice::from_raw_parts(ptr.cast::<EnumServiceStatusProcess>(), count as usize)
        };
        let mut by_pid: HashMap<u32, Vec<String>> = HashMap::new();
        for row in entries {
            let pid = row.state.process_id;
            if pid == 0 { continue; }
            if let Some(name) = wide_in_buffer(ptr, capacity, row.service_name) {
                by_pid.entry(pid).or_default().push(name);
            }
        }
        for names in by_pid.values_mut() { names.sort(); names.dedup(); }
        return Ok(by_pid);
    }
    bail!("SCM: no se pudo completar el catálogo de servicios")
}
