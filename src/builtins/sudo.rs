use std::{
    env,
    ffi::OsStr,
    fs,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    path::Path,
    process::Command,
    ptr::{null, null_mut},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result};
use sysinfo::System;
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError},
    Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation,
        LookupPrivilegeNameW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_MANDATORY_LABEL,
        TOKEN_QUERY, TokenElevation, TokenIntegrityLevel, TokenPrivileges,
    },
    System::Threading::{
        CreateProcessWithTokenW, GetCurrentProcess, GetExitCodeProcess, OpenProcess,
        OpenProcessToken, WaitForSingleObject, CREATE_NO_WINDOW, PROCESS_INFORMATION,
        PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
    },
    UI::{
        Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW},
        WindowsAndMessaging::SW_HIDE,
    },
};

use crate::core::{CommandContext, CommandOutput};

use super::BuiltinCommand;

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
const INFINITE_WAIT: u32 = 0xffff_ffff;
const WAIT_OBJECT_0_VALUE: u32 = 0;

pub struct SudoBuiltin;

impl BuiltinCommand for SudoBuiltin {
    fn name(&self) -> &'static str {
        "sudo"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["runas"]
    }

    fn help(&self) -> &'static str {
        "sudo — inspecciona o eleva comandos SST mediante UAC nativo"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
            return Ok(CommandOutput::ok(
                "sudo — elevación nativa activa de Shell Shock Tool\n\n\
                 uso:\n\
                   sudo                      eleva la sesión SST de forma persistente\n\
                   sudo --status             muestra integridad y privilegios del token\n\
                   sudo COMANDO [...]        ejecuta un comando SST como Administrador\n\
                   sudo --system COMANDO    ejecuta un comando SST como LOCAL SYSTEM\n\
                   runas [...]               alias de sudo; no invoca runas.exe\n",
            ));
        }

        if args.is_empty() {
            return activate_shell(context.cwd);
        }

        if args.first().is_some_and(|arg| arg == "--status") {
            return token_status_output();
        }

        if args.first().is_some_and(|arg| arg == "--admin-internal") {
            let Some(command) = args.get(1) else {
                return Ok(CommandOutput::error("sudo: falta comando interno", 2));
            };
            enable_standard_privileges();
            return execute_as_current_token(command, context.cwd);
        }

        if args.first().is_some_and(|arg| arg == "--system-internal") {
            let Some(command) = args.get(1) else {
                return Ok(CommandOutput::error("sudo: falta comando SYSTEM interno", 2));
            };
            enable_standard_privileges();
            return execute_as_system(command, context.cwd);
        }

        if args.first().is_some_and(|arg| arg == "--system") {
            if args.len() < 2 {
                return Ok(CommandOutput::error("sudo --system: falta COMANDO", 2));
            }

            let command = args[1..]
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ");

            let status = current_token_status()?;
            if status.integrity == "System" {
                enable_standard_privileges();
                return execute_as_current_token(&command, context.cwd);
            }
            if status.elevated {
                enable_standard_privileges();
                return execute_as_system(&command, context.cwd);
            }

            let internal = format!(
                "sudo --system-internal {}",
                shell_quote(&command)
            );
            return run_via_uac(&internal, context.cwd);
        }

        let command = args.iter().map(|arg| shell_quote(arg)).collect::<Vec<_>>().join(" ");
        execute_with_elevation(&command, context.cwd)
    }
}

fn activate_shell(cwd: &Path) -> Result<CommandOutput> {
    let status = current_token_status()?;

    if status.elevated {
        let enabled = crate::support::windows::enable_all_token_privileges()?;
        return Ok(CommandOutput::ok(format!(
            "sudo: sesión ya elevada ({}) · {enabled} privilegio(s) Se* activo(s)\n",
            status.integrity
        )));
    }

    launch_elevated_shell(cwd)?;
    crate::support::windows::request_shell_handoff();

    Ok(CommandOutput::ok(
        "sudo: elevando sesión SST mediante UAC...\n"
    ))
}

fn launch_elevated_shell(cwd: &Path) -> Result<()> {
    let exe = env::current_exe()?;
    let verb = wide("runas");
    let exe_w = wide(exe.as_os_str());
    let params_w = wide("--gui");
    let cwd_w = wide(cwd.as_os_str());

    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.lpDirectory = cwd_w.as_ptr();
    info.nShow = windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if error == 1223 {
            anyhow::bail!("sudo: elevación cancelada por el usuario");
        }
        anyhow::bail!(
            "sudo: no se pudo elevar la sesión (ShellExecuteExW, Win32 {error})"
        );
    }

    if !info.hProcess.is_null() {
        unsafe { CloseHandle(info.hProcess); }
    }
    Ok(())
}

fn execute_with_elevation(command: &str, cwd: &Path) -> Result<CommandOutput> {
    if current_token_status()?.elevated {
        enable_standard_privileges();
        return execute_as_current_token(command, cwd);
    }

    let internal = format!("sudo --admin-internal {}", shell_quote(command));
    run_via_uac(&internal, cwd)
}

fn enable_standard_privileges() {
    let _ = crate::support::windows::enable_all_token_privileges();
}

fn execute_as_current_token(command: &str, cwd: &Path) -> Result<CommandOutput> {
    let output = Command::new(env::current_exe()?)
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("sudo: no se pudo ejecutar: {command}"))?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code().unwrap_or(1),
    })
}

fn execute_as_system(command: &str, cwd: &Path) -> Result<CommandOutput> {
    enable_standard_privileges();

    let mut system = System::new_all();
    system.refresh_all();

    let system_pid = ["services.exe", "winlogon.exe"]
        .into_iter()
        .find_map(|wanted| {
            system.processes().iter().find_map(|(pid, process)| {
                process
                    .name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(wanted)
                    .then_some(pid.as_u32())
            })
        })
        .context("sudo --system: no se encontró un proceso LOCAL SYSTEM")?;

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, system_pid);
        if process.is_null() {
            anyhow::bail!(
                "sudo --system: OpenProcess({system_pid}) falló con error Win32 {}",
                GetLastError()
            );
        }

        let mut token = null_mut();
        let token_ok = OpenProcessToken(
            process,
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            &mut token,
        );
        CloseHandle(process);

        if token_ok == 0 {
            anyhow::bail!(
                "sudo --system: OpenProcessToken falló con error Win32 {}",
                GetLastError()
            );
        }

        let result = run_with_system_token(token, command, cwd);
        CloseHandle(token);
        result
    }
}

unsafe fn run_with_system_token(
    token: *mut core::ffi::c_void,
    command: &str,
    cwd: &Path,
) -> Result<CommandOutput> {
    let tag = format!(
        "system-{}-{}",
        std::process::id(),
        REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    );
    let base = env::temp_dir();
    let stdout_path = base.join(format!("sst-sudo-{tag}.out"));
    let stderr_path = base.join(format!("sst-sudo-{tag}.err"));

    let wrapped = format!(
        "{{ {command}; }} > {} 2> {}",
        shell_quote(&stdout_path.to_string_lossy()),
        shell_quote(&stderr_path.to_string_lossy())
    );

    let exe = env::current_exe()?;
    let exe_w = wide(exe.as_os_str());
    let cwd_w = wide(cwd.as_os_str());
    let command_line = format!(
        "{} -c {}",
        windows_quote(&exe.to_string_lossy()),
        windows_quote(&wrapped)
    );
    let mut command_line_w = wide(command_line);

    let mut startup: STARTUPINFOW = zeroed();
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    let mut process_info: PROCESS_INFORMATION = zeroed();

    let launched = CreateProcessWithTokenW(
        token,
        0,
        exe_w.as_ptr(),
        command_line_w.as_mut_ptr(),
        CREATE_NO_WINDOW,
        null(),
        cwd_w.as_ptr(),
        &startup,
        &mut process_info,
    );

    if launched == 0 {
        let error = GetLastError();
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        anyhow::bail!(
            "sudo --system: CreateProcessWithTokenW falló con error Win32 {error}"
        );
    }

    if !process_info.hThread.is_null() {
        CloseHandle(process_info.hThread);
    }

    let wait = WaitForSingleObject(process_info.hProcess, INFINITE_WAIT);
    if wait != WAIT_OBJECT_0_VALUE {
        CloseHandle(process_info.hProcess);
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        anyhow::bail!(
            "sudo --system: error esperando el proceso SYSTEM ({wait})"
        );
    }

    let mut status = 1u32;
    let got_status = GetExitCodeProcess(process_info.hProcess, &mut status);
    CloseHandle(process_info.hProcess);

    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);

    if got_status == 0 {
        anyhow::bail!(
            "sudo --system: GetExitCodeProcess falló con error Win32 {}",
            GetLastError()
        );
    }

    Ok(CommandOutput {
        stdout,
        stderr,
        status: status as i32,
    })
}

fn run_via_uac(command: &str, cwd: &Path) -> Result<CommandOutput> {
    let tag = format!(
        "{}-{}",
        std::process::id(),
        REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    );
    let base = env::temp_dir();
    let stdout_path = base.join(format!("sst-sudo-{tag}.out"));
    let stderr_path = base.join(format!("sst-sudo-{tag}.err"));

    let wrapped = format!(
        "{{ {command}; }} > {} 2> {}",
        shell_quote(&stdout_path.to_string_lossy()),
        shell_quote(&stderr_path.to_string_lossy())
    );

    let exe = env::current_exe()?;
    let params = format!("-c {}", windows_quote(&wrapped));
    let verb = wide("runas");
    let exe_w = wide(exe.as_os_str());
    let params_w = wide(&params);
    let cwd_w = wide(cwd.as_os_str());

    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.lpDirectory = cwd_w.as_ptr();
    info.nShow = SW_HIDE;

    let launched = unsafe { ShellExecuteExW(&mut info) };
    if launched == 0 {
        let error = unsafe { GetLastError() };
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        return Ok(CommandOutput::error(
            if error == 1223 {
                "sudo: elevación cancelada por el usuario".to_owned()
            } else {
                format!("sudo: ShellExecuteExW falló con error Win32 {error}")
            },
            1,
        ));
    }

    if info.hProcess.is_null() {
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        return Ok(CommandOutput::error(
            "sudo: Windows no devolvió un handle del proceso elevado",
            1,
        ));
    }

    let wait = unsafe { WaitForSingleObject(info.hProcess, INFINITE_WAIT) };
    if wait != WAIT_OBJECT_0_VALUE {
        unsafe { CloseHandle(info.hProcess); }
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        return Ok(CommandOutput::error(
            format!("sudo: error esperando el proceso elevado ({wait})"),
            1,
        ));
    }

    let mut status = 1u32;
    let got_status = unsafe { GetExitCodeProcess(info.hProcess, &mut status) };
    unsafe { CloseHandle(info.hProcess); }

    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);

    if got_status == 0 {
        return Ok(CommandOutput::error(
            format!("sudo: GetExitCodeProcess falló con error Win32 {}", unsafe { GetLastError() }),
            1,
        ));
    }

    Ok(CommandOutput {
        stdout,
        stderr,
        status: status as i32,
    })
}

struct TokenStatus {
    elevated: bool,
    integrity: String,
    privileges: Vec<(String, bool)>,
}

fn token_status_output() -> Result<CommandOutput> {
    let status = current_token_status()?;
    let user = env::var("USERNAME").unwrap_or_else(|_| "desconocido".to_owned());
    let domain = env::var("USERDOMAIN").unwrap_or_default();
    let identity = if domain.is_empty() {
        user
    } else {
        format!("{domain}\\{user}")
    };

    let mut out = format!(
        "identity:  {identity}\n\
         elevated: {}\n\
         integrity: {}\n\
         privileges:\n",
        if status.elevated { "yes" } else { "no" },
        status.integrity,
    );

    for (name, enabled) in status.privileges {
        out.push_str(&format!("  {:<36} {}\n", name, if enabled { "enabled" } else { "disabled" }));
    }

    Ok(CommandOutput::ok(out))
}

fn current_token_status() -> Result<TokenStatus> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            anyhow::bail!("sudo: OpenProcessToken falló con error Win32 {}", GetLastError());
        }

        let result = (|| -> Result<TokenStatus> {
            let mut elevation: TOKEN_ELEVATION = zeroed();
            let mut returned = 0u32;
            if GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut _,
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            ) == 0
            {
                anyhow::bail!("sudo: TokenElevation falló con error Win32 {}", GetLastError());
            }

            let integrity = token_integrity(token)?;
            let privileges = token_privileges(token)?;

            Ok(TokenStatus {
                elevated: elevation.TokenIsElevated != 0,
                integrity,
                privileges,
            })
        })();

        CloseHandle(token);
        result
    }
}

unsafe fn token_integrity(token: *mut core::ffi::c_void) -> Result<String> {
    let mut size = 0u32;
    unsafe {
        GetTokenInformation(token, TokenIntegrityLevel, null_mut(), 0, &mut size);
    }
    if size == 0 {
        anyhow::bail!("sudo: no se pudo consultar el tamaño de TokenIntegrityLevel");
    }

    let mut buffer = vec![0u8; size as usize];
    if unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buffer.as_mut_ptr() as *mut _,
            size,
            &mut size,
        )
    } == 0
    {
        anyhow::bail!("sudo: TokenIntegrityLevel falló con error Win32 {}", unsafe { GetLastError() });
    }

    let label = unsafe { &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL) };
    let count_ptr = unsafe { GetSidSubAuthorityCount(label.Label.Sid) };
    if count_ptr.is_null() {
        anyhow::bail!("sudo: SID de integridad inválido");
    }
    let count = unsafe { *count_ptr } as u32;
    if count == 0 {
        anyhow::bail!("sudo: SID de integridad sin subautoridades");
    }

    let rid_ptr = unsafe { GetSidSubAuthority(label.Label.Sid, count - 1) };
    if rid_ptr.is_null() {
        anyhow::bail!("sudo: RID de integridad inválido");
    }
    let rid = unsafe { *rid_ptr };

    Ok(match rid {
        0x0000..=0x0fff => "Untrusted",
        0x1000..=0x1fff => "Low",
        0x2000..=0x2fff => "Medium",
        0x3000..=0x3fff => "High",
        0x4000..=0x4fff => "System",
        _ => "Protected",
    }
    .to_owned())
}

unsafe fn token_privileges(token: *mut core::ffi::c_void) -> Result<Vec<(String, bool)>> {
    let mut size = 0u32;
    unsafe {
        GetTokenInformation(token, TokenPrivileges, null_mut(), 0, &mut size);
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];
    if unsafe {
        GetTokenInformation(
            token,
            TokenPrivileges,
            buffer.as_mut_ptr() as *mut _,
            size,
            &mut size,
        )
    } == 0
    {
        anyhow::bail!("sudo: TokenPrivileges falló con error Win32 {}", unsafe { GetLastError() });
    }

    let header = unsafe { &*(buffer.as_ptr() as *const windows_sys::Win32::Security::TOKEN_PRIVILEGES) };
    let count = header.PrivilegeCount as usize;
    let first = header.Privileges.as_ptr();
    let entries = unsafe { std::slice::from_raw_parts(first, count) };

    let mut result = Vec::with_capacity(count);
    for entry in entries {
        if let Some(name) = unsafe { privilege_name(entry) } {
            result.push((name, entry.Attributes & SE_PRIVILEGE_ENABLED != 0));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

unsafe fn privilege_name(entry: &LUID_AND_ATTRIBUTES) -> Option<String> {
    let mut chars = 0u32;
    unsafe {
        LookupPrivilegeNameW(null(), &entry.Luid, null_mut(), &mut chars);
    }
    if chars == 0 {
        return None;
    }

    let mut buffer = vec![0u16; chars as usize + 1];
    if unsafe {
        LookupPrivilegeNameW(null(), &entry.Luid, buffer.as_mut_ptr(), &mut chars)
    } == 0
    {
        return None;
    }

    Some(String::from_utf16_lossy(&buffer[..chars as usize]))
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || "_-./:\\".contains(ch))
    {
        return value.to_owned();
    }

    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn windows_quote(value: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0usize;

    for ch in value.chars() {
        match ch {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                slashes = 0;
                out.push(ch);
            }
        }
    }

    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}
