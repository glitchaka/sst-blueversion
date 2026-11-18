//! SST PowerShell plugin ABI: self-contained .NET runtime and SDK.
//! PowerShell executes inside sst.exe via hostfxr, never pwsh.exe.
use std::{
    ffi::{CStr, CString, c_void},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null, null_mut},
    sync::Mutex,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::System::{
    Com::CoTaskMemFree,
    LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    },
};
use crate::core::CommandOutput;

type FxrInit = unsafe extern "C" fn(*const u16, *const c_void, *mut *mut c_void) -> i32;
type FxrDelegate = unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> i32;
type FxrClose = unsafe extern "C" fn(*mut c_void) -> i32;
type LoadAssembly = unsafe extern "system" fn(
    *const u16, *const u16, *const u16, *const u16, *mut c_void, *mut *mut c_void,
) -> i32;
type ManagedInvoke = unsafe extern "system" fn(*const i8, *mut *mut i8) -> i32;

#[derive(Serialize)]
struct ManagedRequest<'a> {
    kind: &'a str, script: &'a str, path: &'a str, command: &'a str,
    arguments: &'a [String], cwd: &'a str, stdin: Option<&'a str>,
}
#[derive(Deserialize)]
struct ManagedReply { status: i32, stdout: String, stderr: String }

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
unsafe fn get_function(handle: *mut c_void, name: &'static [u8]) -> Result<*mut c_void> {
    let ptr = unsafe { GetProcAddress(handle, name.as_ptr()) }
        .ok_or_else(|| anyhow::anyhow!("PowerShell: falta exportación hostfxr"))?;
    Ok(ptr as *const () as *mut c_void)
}

pub struct PowerShellEngine {
    invoke: ManagedInvoke,
    _hostfxr: *mut c_void,
    serial: Mutex<()>,
}
// SAFETY: the managed runspace is serialized, and the .NET hostfxr library
// remains loaded for the entire SST process lifetime.
unsafe impl Send for PowerShellEngine {}
unsafe impl Sync for PowerShellEngine {}

impl PowerShellEngine {
    pub fn load(directory: &Path) -> Result<Self> {
        let directory = directory.canonicalize()
            .context("PowerShell: directorio de módulo no encontrado")?;
        let host = directory.join("hostfxr.dll");
        let assembly = directory.join("Sst.PowerShell.Bridge.dll");
        let runtime_config = directory.join("Sst.PowerShell.Bridge.runtimeconfig.json");
        for file in [&host, &assembly, &runtime_config] {
            if !file.is_file() {
                bail!("PowerShell: módulo incompleto; falta {}. Reinstala con instp install powershell", file.display());
            }
        }
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        // SAFETY: only the explicit absolute hostfxr path from installed package.
        let library = unsafe { LoadLibraryExW(wide(host.as_os_str()).as_ptr(), null_mut(), flags) };
        if library.is_null() {
            bail!("PowerShell: no se pudo cargar hostfxr del módulo instalado");
        }
        // hostfxr and CoreCLR must never be unloaded before process exit.
        let init: FxrInit = unsafe {
            std::mem::transmute(get_function(library, b"hostfxr_initialize_for_runtime_config\0")?)
        };
        let get_delegate: FxrDelegate = unsafe {
            std::mem::transmute(get_function(library, b"hostfxr_get_runtime_delegate\0")?)
        };
        let close: FxrClose = unsafe {
            std::mem::transmute(get_function(library, b"hostfxr_close\0")?)
        };
        let mut context = null_mut();
        let rc = unsafe {
            init(wide(runtime_config.as_os_str()).as_ptr(), null(), &mut context)
        };
        if rc != 0 || context.is_null() {
            bail!("PowerShell: hostfxr no pudo inicializar .NET (0x{:08x})", rc as u32);
        }
        let mut loader = null_mut();
        // 5 is hdt_load_assembly_and_get_function_pointer.
        let delegate_status = unsafe { get_delegate(context, 5, &mut loader) };
        unsafe { close(context); }
        if delegate_status != 0 || loader.is_null() {
            bail!("PowerShell: no se pudo cargar el delegado .NET (0x{:08x})", delegate_status as u32);
        }
        let load: LoadAssembly = unsafe { std::mem::transmute(loader) };
        let type_name = wide(std::ffi::OsStr::new(
            "Sst.PowerShell.Bridge.EntryPoint, Sst.PowerShell.Bridge"
        ));
        let method_name = wide(std::ffi::OsStr::new("Execute"));
        let mut callback = null_mut();
        // -1 is UNMANAGEDCALLERSONLY_METHOD from coreclr_delegates.h.
        let rc = unsafe {
            load(
                wide(assembly.as_os_str()).as_ptr(),
                type_name.as_ptr(), method_name.as_ptr(),
                (-1isize) as *const u16, null_mut(), &mut callback,
            )
        };
        if rc != 0 || callback.is_null() {
            bail!("PowerShell: no se pudo enlazar el runspace (0x{:08x})", rc as u32);
        }
        Ok(Self {
            invoke: unsafe { std::mem::transmute(callback) },
            _hostfxr: library,
            serial: Mutex::new(()),
        })
    }

    pub fn run(
        &self, kind: &str, source: &str, args: &[String], cwd: &Path, stdin: Option<&[u8]>,
    ) -> Result<CommandOutput> {
        let _guard = self.serial.lock().map_err(|_| anyhow::anyhow!("PowerShell: runspace bloqueado"))?;
        let cwd = cwd.to_string_lossy();
        let input = stdin.map(String::from_utf8_lossy);
        let request = ManagedRequest {
            kind,
            script: if kind == "script" { source } else { "" },
            path: if kind == "file" { source } else { "" },
            command: if kind == "command" { source } else { "" },
            arguments: args, cwd: &cwd, stdin: input.as_deref(),
        };
        let serialized = CString::new(serde_json::to_vec(&request)?)?;
        let mut result = null_mut();
        // Managed callback allocates output with CoTaskMem; free exactly once.
        let status = unsafe { (self.invoke)(serialized.as_ptr(), &mut result) };
        if status != 0 || result.is_null() {
            bail!("PowerShell: fallo de comunicación con motor .NET ({status})");
        }
        let response = unsafe {
            let bytes = CStr::from_ptr(result).to_bytes().to_vec();
            CoTaskMemFree(result.cast());
            bytes
        };
        let reply: ManagedReply = serde_json::from_slice(&response)
            .context("PowerShell: respuesta de módulo inválida")?;
        Ok(CommandOutput {
            stdout: reply.stdout, stderr: reply.stderr, status: reply.status,
        })
    }
}
