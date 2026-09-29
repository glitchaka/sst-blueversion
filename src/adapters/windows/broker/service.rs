use super::{
    security::{self, TrustedImage, wide},
    transport,
};
use crate::core::broker::*;
use anyhow::{Result, ensure};
use serde_json::json;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::Write,
    os::windows::fs::OpenOptionsExt,
    ptr::{null, null_mut},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::FILE_SHARE_READ,
    System::{Pipes::DisconnectNamedPipe, Services::*, Threading::GetCurrentProcess},
};

static OPERATOR: OnceLock<String> = OnceLock::new();
static STOP: AtomicBool = AtomicBool::new(false);
static STATUS: AtomicUsize = AtomicUsize::new(0);

pub fn dispatch(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 3 && args[0] == "--broker-service" && args[1] == "--operator-sid",
        "invalid broker service invocation"
    );
    security::validate_operator_sid(&args[2])?;
    ensure!(
        OPERATOR.set(args[2].clone()).is_ok(),
        "broker already initialized"
    );
    let name = wide(super::SERVICE);
    let entries = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_ptr() as *mut u16,
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: null_mut(),
            lpServiceProc: None,
        },
    ];
    ensure!(
        unsafe { StartServiceCtrlDispatcherW(entries.as_ptr()) } != 0,
        "broker must be started by SCM ({})",
        unsafe { GetLastError() }
    );
    Ok(())
}

fn report(state: u32, error: u32) {
    let handle = STATUS.load(Ordering::SeqCst) as SERVICE_STATUS_HANDLE;
    if handle.is_null() {
        return;
    }
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: error,
        dwServiceSpecificExitCode: if error == ERROR_SERVICE_SPECIFIC_ERROR {
            1
        } else {
            0
        },
        dwCheckPoint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            1
        } else {
            0
        },
        dwWaitHint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            15000
        } else {
            0
        },
    };
    unsafe {
        SetServiceStatus(handle, &status);
    }
}

unsafe extern "system" fn control(
    code: u32,
    _: u32,
    _: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> u32 {
    match code {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            STOP.store(true, Ordering::SeqCst);
            report(SERVICE_STOP_PENDING, 0);
            0
        }
        SERVICE_CONTROL_INTERROGATE => 0,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let handle = unsafe {
        RegisterServiceCtrlHandlerExW(wide(super::SERVICE).as_ptr(), Some(control), null())
    };
    if handle.is_null() {
        return;
    }
    STATUS.store(handle as usize, Ordering::SeqCst);
    report(SERVICE_START_PENDING, 0);
    // Service entry never initializes GUI, shell, plugins, RC files or user data.
    let result = serve();
    report(
        SERVICE_STOPPED,
        if result.is_ok() {
            0
        } else {
            ERROR_SERVICE_SPECIFIC_ERROR
        },
    );
}

struct Audit(File);
impl Audit {
    fn open() -> Result<Self> {
        let path = std::env::current_exe()?
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing install directory"))?
            .join("broker-audit.jsonl");
        Ok(Self(
            OpenOptions::new()
                .append(true)
                .share_mode(FILE_SHARE_READ)
                .open(path)?,
        ))
    }
    fn append(&mut self, value: serde_json::Value) -> Result<()> {
        // Stop accepting actions when full; never silently overwrite audit history.
        ensure!(
            self.0.metadata()?.len() < 16 * 1024 * 1024,
            "broker audit full; administrator must archive while service is stopped"
        );
        let mut row = serde_json::to_vec(&value)?;
        row.push(b'\n');
        self.0.write_all(&row)?;
        self.0.sync_data()?;
        Ok(())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn serve() -> Result<()> {
    let own_token = security::process_token(unsafe { GetCurrentProcess() })?;
    let own = security::token_identity(own_token.0)?;
    ensure!(
        own.sid == "S-1-5-18" && own.session == 0,
        "broker requires LocalSystem in session zero"
    );
    ensure!(
        crate::support::windows::enable_privilege("SeDebugPrivilege")?,
        "SeDebugPrivilege unavailable"
    );
    let image = TrustedImage::current()?;
    let operator = OPERATOR
        .get()
        .ok_or_else(|| anyhow::anyhow!("operator not configured"))?;
    security::expose_server_identity(operator)?;
    let mut audit = Audit::open()?;
    let pipe = transport::create(operator)?;
    audit.append(json!({"time": now(), "event": "service_started", "operator": operator, "protocol": VERSION}))?;
    let mut seen = HashSet::new();
    report(SERVICE_RUNNING, 0);
    while !STOP.load(Ordering::SeqCst) {
        if !transport::connect(pipe.0)? {
            continue;
        }
        let result = handle(pipe.0, operator, &image, &mut audit, &mut seen);
        unsafe {
            DisconnectNamedPipe(pipe.0);
        }
        if let Err(error) = result {
            // No unauthenticated payload/command is written to the audit.
            audit.append(json!({"time": now(), "event": "connection_rejected", "reason": clean_error(&error)}))?;
        }
    }
    audit.append(json!({"time": now(), "event": "service_stopped"}))?;
    Ok(())
}

fn clean_error(error: &anyhow::Error) -> String {
    error
        .to_string()
        .chars()
        .filter(|c| !c.is_control())
        .take(1024)
        .collect()
}

fn handle(
    pipe: HANDLE,
    operator: &str,
    image: &TrustedImage,
    audit: &mut Audit,
    seen: &mut HashSet<[u8; 16]>,
) -> Result<()> {
    let request: Request = transport::receive(pipe, Instant::now() + Duration::from_secs(3))?;
    let client = security::authenticate(pipe, &request, operator, image)?;
    ensure!(!STOP.load(Ordering::SeqCst), "service stopping");
    ensure!(
        seen.len() < 65536 && seen.insert(request.nonce),
        "request replay or session request budget exhausted"
    );
    // Durably record intent before any privileged side effect. The request
    // identity is already bound to the OS-authenticated client here.
    audit.append(json!({"time": now(), "event": "request", "sid": client.sid, "session": client.session, "administrator": client.administrator, "request": request}))?;
    let outcome = match super::perform(&request, &client) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::Rejected {
            reason: clean_error(&error),
        },
    };
    let response = Response {
        version: VERSION,
        nonce: request.nonce,
        operation: request.operation,
        target: request.target,
        outcome,
    };
    response.validate(&request)?;
    audit.append(json!({"time": now(), "event": "response", "response": response}))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    transport::send(pipe, &response, deadline)?;
    // DisconnectNamedPipe discards unread output: wait for receipt, with a cap.
    transport::await_ack(pipe, deadline)
}
