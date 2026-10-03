//! LocalSystem service and its strictly typed client. Never enters the shell engine.
mod security;
mod service;
mod transport;

use crate::core::{CommandOutput, broker::*};
use anyhow::{Result, bail, ensure};
use security::{Handle, identity};
use std::{mem::{size_of, zeroed}, ptr::null_mut};
use windows_sys::Win32::{
    Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
    System::Threading::*,
};

pub use service::dispatch;
pub(super) const SERVICE: &str = "SSTPrivilegedBroker";
pub(super) const PIPE: &str = r"\\.\pipe\SSTPrivilegedBroker";

/// Mutations require an explicit identity obtained from a prior inspect. In
/// particular, UAC must not recapture a possibly reused PID after consent.
pub fn command(operation: Operation, args: &[String]) -> Result<CommandOutput> {
    let Some(pid) = args.first() else {
        bail!("broker: expected PID [--start-time FILETIME]");
    };
    let pid: u32 = pid.parse()?;
    let created = match &args[1..] {
        [] if operation == Operation::Inspect => None,
        [flag, value] if flag == "--start-time" => Some(value.parse::<u64>()?),
        _ => bail!(
            "broker: use PID --start-time FILETIME (from sys inspect PID --broker); no names/tree/force flags"
        ),
    };
    let target = if let Some(created) = created {
        ProcessIdentity { pid, created }
    } else {
        // INSPECT is the bootstrap operation: the LocalSystem broker resolves
        // the exact FILETIME while holding the target handle. Mutations still
        // require the exact identity returned by a prior inspect.
        ProcessIdentity { pid, created: 0 }
    };
    // The portable GUI stays unelevated and need not live in Program Files.
    // Launch only our ACL-protected, fixed-purpose client entrypoint, with the
    // original target identity, under the SAME token. This never invokes a shell.
    let installed = security::installed_executable()?;
    if std::env::current_exe()?.canonicalize()? != installed {
        use std::os::windows::process::CommandExt;
        let operation = match operation {
            Operation::Inspect => "inspect",
            Operation::Suspend => "suspend",
            Operation::Resume => "resume",
            Operation::Kill => "kill",
        };
        let output = std::process::Command::new(installed)
            .args([
                "--broker-client",
                operation,
                &target.pid.to_string(),
                "--start-time",
                &target.created.to_string(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;
        ensure!(
            output.stdout.len() <= MAX_FRAME && output.stderr.len() <= MAX_FRAME,
            "invalid broker client output"
        );
        return Ok(CommandOutput {
            stdout: String::from_utf8(output.stdout)?,
            stderr: String::from_utf8(output.stderr)?,
            status: output.status.code().unwrap_or(1),
        });
    }
    let mut nonce = [0u8; 16];
    ensure!(
        unsafe {
            BCryptGenRandom(
                null_mut(),
                nonce.as_mut_ptr(),
                16,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        } >= 0,
        "nonce generation failed"
    );
    let session =
        security::token_identity(security::process_token(unsafe { GetCurrentProcess() })?.0)?
            .session;
    let request = Request {
        version: VERSION,
        nonce,
        client: identity(unsafe { GetCurrentProcess() })?,
        session,
        operation,
        target,
    };
    request.validate()?;
    let response = transport::call(&request)?;
    response.validate(&request)?;
    match response.outcome {
        Outcome::Inspected {
            identity,
            image,
            critical,
            protection,
            session,
        } => Ok(CommandOutput::ok(format!(
            "PID: {}\nStart time (FILETIME): {}\nImage: {}\nSession: {}\nCritical: {}\nProtection: {}\n",
            identity.pid, identity.created, image, session, critical, protection
        ))),
        Outcome::Completed => Ok(CommandOutput::ok(format!(
            "{:?} completed for PID {} / {}\n",
            operation, target.pid, target.created
        ))),
        Outcome::Rejected { reason } => Ok(CommandOutput::error(format!("broker: {reason}"), 1)),
    }
}

fn change_suspension(process: windows_sys::Win32::Foundation::HANDLE, suspend: bool) -> Result<()> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    let module = unsafe { GetModuleHandleW(security::wide("ntdll.dll").as_ptr()) };
    ensure!(!module.is_null(), "system ntdll unavailable");
    let name: &[u8] = if suspend {
        b"NtSuspendProcess\0"
    } else {
        b"NtResumeProcess\0"
    };
    let address = unsafe { GetProcAddress(module, name.as_ptr()) }.ok_or_else(|| {
        anyhow::anyhow!("native suspend/resume unavailable on this Windows version")
    })?;
    // Only these two constant, already-loaded system exports are callable.
    type NtProcessAction =
        unsafe extern "system" fn(windows_sys::Win32::Foundation::HANDLE) -> i32;

    // SAFETY:
    // - GetProcAddress is restricted above to the already-loaded system ntdll.dll.
    // - name is one of the two constant exports NtSuspendProcess/NtResumeProcess.
    // - Both exports use the NTAPI/system calling convention and the signature
    //   NTSTATUS Fn(HANDLE) on supported Windows versions.
    // - address was checked for null before this conversion.
    //
    // Keep this signature synchronized with the native Windows declaration if
    // this code is ever moved to a different API or architecture.
    let action: NtProcessAction = unsafe { std::mem::transmute(address) };
    ensure!(
        unsafe { action(process) } >= 0,
        "native suspend/resume failed"
    );
    Ok(())
}

fn perform(request: &Request, client: &security::Client) -> Result<Outcome> {
    ensure!(client.alive(), "authenticated client exited");
    ensure!(
        request.target.pid != unsafe { GetCurrentProcessId() },
        "broker cannot target itself"
    );
    let mutation = request.operation != Operation::Inspect;
    ensure!(
        !mutation || client.administrator,
        "operation requires elevated Administrators token"
    );
    let rights = PROCESS_QUERY_LIMITED_INFORMATION
        | PROCESS_SYNCHRONIZE
        | match request.operation {
            Operation::Inspect => 0,
            Operation::Suspend | Operation::Resume => PROCESS_SUSPEND_RESUME,
            Operation::Kill => PROCESS_TERMINATE,
        };
    // This handle is never reopened by PID after identity validation.
    let process = Handle::new(unsafe { OpenProcess(rights, 0, request.target.pid) })?;
    let actual_identity = identity(process.0)?;
    if request.target.created != 0 {
        ensure!(
            actual_identity == request.target,
            "target PID was reused or creation time differs"
        );
    }
    ensure!(
        unsafe { WaitForSingleObject(process.0, 0) } == 258,
        "target already exited"
    );
    let token = security::process_token(process.0)?;
    let owner = security::token_identity(token.0)?;
    ensure!(
        client.administrator || (owner.sid == client.sid && owner.session == client.session),
        "target outside client user/session"
    );
    let mut critical = 0;
    ensure!(
        unsafe { IsProcessCritical(process.0, &mut critical) } != 0,
        "critical status unavailable"
    );
    let mut protection: PROCESS_PROTECTION_LEVEL_INFORMATION = unsafe { zeroed() };
    ensure!(
        unsafe {
            GetProcessInformation(
                process.0,
                ProcessProtectionLevelInfo,
                (&mut protection as *mut PROCESS_PROTECTION_LEVEL_INFORMATION).cast(),
                size_of::<PROCESS_PROTECTION_LEVEL_INFORMATION>() as u32,
            )
        } != 0,
        "protection status unavailable"
    );
    let image = security::image_path(process.0)?;
    if !mutation {
        return Ok(Outcome::Inspected {
            identity: actual_identity,
            image,
            critical: critical != 0,
            protection: protection.ProtectionLevel,
            session: owner.session,
        });
    }
    ensure!(
        critical == 0 && protection.ProtectionLevel == PROTECTION_LEVEL_NONE,
        "critical/protected target refused; no override"
    );
    // Conservative extra protection even when Windows does not mark a core service critical.
    let basename = image.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
    ensure!(
        !matches!(
            basename.as_str(),
            "smss.exe"
                | "csrss.exe"
                | "wininit.exe"
                | "services.exe"
                | "lsass.exe"
                | "winlogon.exe"
                | "svchost.exe"
        ),
        "core system process refused"
    );
    match request.operation {
        Operation::Suspend => change_suspension(process.0, true)?,
        Operation::Resume => change_suspension(process.0, false)?,
        Operation::Kill => {
            ensure!(
                unsafe { TerminateProcess(process.0, 1) } != 0,
                "terminate failed"
            );
            ensure!(
                unsafe { WaitForSingleObject(process.0, 2000) } == 0,
                "termination requested but exit not confirmed; do not retry automatically"
            );
        }
        Operation::Inspect => unreachable!(),
    }
    Ok(Outcome::Completed)
}
