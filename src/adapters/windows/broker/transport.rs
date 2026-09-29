use super::security::{Handle, TrustedImage, wide};
use crate::core::broker::{MAX_FRAME, Request, Response};
use anyhow::{Result, bail, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    mem::zeroed,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, SECURITY_ATTRIBUTES},
    Storage::FileSystem::*,
    System::{IO::*, Pipes::*, Threading::*},
};

pub(super) fn create(operator: &str) -> Result<Handle> {
    // Explicit read/write DATA only: generic write would also permit creating
    // another server instance (FILE_CREATE_PIPE_INSTANCE).
    let sddl = wide(&format!(
        "D:P(A;;GA;;;SY)(A;;0x00100003;;;{operator})S:(ML;;NW;;;ME)"
    ));
    let mut descriptor = null_mut();
    ensure!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } != 0,
        "pipe ACL construction failed"
    );
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let pipe = unsafe {
        CreateNamedPipeW(
            wide(super::PIPE).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            (MAX_FRAME + 4) as u32,
            (MAX_FRAME + 4) as u32,
            1000,
            &attributes,
        )
    };
    let result = Handle::new(pipe);
    unsafe {
        LocalFree(descriptor);
    }
    result
}

/// All pending I/O is cancelled AND drained before stack buffers/OVERLAPPED die.
fn complete(pipe: HANDLE, overlap: &OVERLAPPED, deadline: Instant) -> Result<u32> {
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(u32::MAX as u128) as u32;
    let wait = unsafe { WaitForSingleObject(overlap.hEvent, timeout) };
    let mut count = 0;
    if wait != WAIT_OBJECT_0 {
        unsafe {
            CancelIoEx(pipe, overlap);
            GetOverlappedResult(pipe, overlap, &mut count, 1);
        }
        bail!("broker IPC timeout/cancelled; operation outcome may be unknown; no automatic retry");
    }
    ensure!(
        unsafe { GetOverlappedResult(pipe, overlap, &mut count, 0) } != 0,
        "broker IPC failed ({})",
        unsafe { GetLastError() }
    );
    Ok(count)
}

pub(super) fn connect(pipe: HANDLE) -> Result<bool> {
    let event = Handle::new(unsafe { CreateEventW(null(), 1, 0, null()) })?;
    let mut overlap: OVERLAPPED = unsafe { zeroed() };
    overlap.hEvent = event.0;
    if unsafe { ConnectNamedPipe(pipe, &mut overlap) } != 0 {
        return Ok(true);
    }
    match unsafe { GetLastError() } {
        ERROR_PIPE_CONNECTED => Ok(true),
        ERROR_IO_PENDING => {
            match complete(pipe, &overlap, Instant::now() + Duration::from_secs(1)) {
                Ok(_) => Ok(true),
                Err(_) => {
                    unsafe {
                        DisconnectNamedPipe(pipe);
                    }
                    Ok(false)
                }
            }
        }
        error => bail!("pipe connect failed ({error})"),
    }
}

fn read_exact(pipe: HANDLE, buffer: &mut [u8], deadline: Instant) -> Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        ensure!(Instant::now() < deadline, "IPC read deadline exceeded");
        let event = Handle::new(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        let mut overlap: OVERLAPPED = unsafe { zeroed() };
        overlap.hEvent = event.0;
        let mut count = 0;
        if unsafe {
            ReadFile(
                pipe,
                buffer[offset..].as_mut_ptr(),
                (buffer.len() - offset) as u32,
                &mut count,
                &mut overlap,
            )
        } == 0
        {
            ensure!(
                unsafe { GetLastError() } == ERROR_IO_PENDING,
                "pipe read failed"
            );
            count = complete(pipe, &overlap, deadline)?;
        }
        ensure!(
            count > 0 && count as usize <= buffer.len() - offset,
            "invalid/closed pipe read"
        );
        offset += count as usize;
    }
    Ok(())
}

fn write_all(pipe: HANDLE, buffer: &[u8], deadline: Instant) -> Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        ensure!(Instant::now() < deadline, "IPC write deadline exceeded");
        let event = Handle::new(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        let mut overlap: OVERLAPPED = unsafe { zeroed() };
        overlap.hEvent = event.0;
        let mut count = 0;
        if unsafe {
            WriteFile(
                pipe,
                buffer[offset..].as_ptr(),
                (buffer.len() - offset) as u32,
                &mut count,
                &mut overlap,
            )
        } == 0
        {
            ensure!(
                unsafe { GetLastError() } == ERROR_IO_PENDING,
                "pipe write failed"
            );
            count = complete(pipe, &overlap, deadline)?;
        }
        ensure!(
            count > 0 && count as usize <= buffer.len() - offset,
            "invalid/closed pipe write"
        );
        offset += count as usize;
    }
    Ok(())
}

pub(super) fn receive<T: DeserializeOwned>(pipe: HANDLE, deadline: Instant) -> Result<T> {
    let mut prefix = [0u8; 4];
    read_exact(pipe, &mut prefix, deadline)?;
    let size = u32::from_le_bytes(prefix) as usize;
    ensure!(size > 0 && size <= MAX_FRAME, "invalid broker frame length");
    let mut buffer = vec![0u8; size];
    read_exact(pipe, &mut buffer, deadline)?;
    Ok(serde_json::from_slice(&buffer)?)
}

pub(super) fn send<T: Serialize>(pipe: HANDLE, value: &T, deadline: Instant) -> Result<()> {
    let encoded = serde_json::to_vec(value)?;
    ensure!(
        !encoded.is_empty() && encoded.len() <= MAX_FRAME,
        "broker frame too large"
    );
    let mut frame = (encoded.len() as u32).to_le_bytes().to_vec();
    frame.extend(encoded);
    write_all(pipe, &frame, deadline)
}

pub(super) fn await_ack(pipe: HANDLE, deadline: Instant) -> Result<()> {
    let mut ack = [0];
    read_exact(pipe, &mut ack, deadline)?;
    ensure!(ack == [1], "invalid broker ACK");
    Ok(())
}

pub(super) fn call(request: &Request) -> Result<Response> {
    let image = TrustedImage::current()?;
    let pipe = Handle::new(unsafe {
        CreateFileW(
            wide(super::PIPE).as_ptr(),
            FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            null_mut(),
        )
    })?;
    let _server = super::security::verify_server(pipe.0, &image)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    send(pipe.0, request, deadline)?;
    let response: Response = receive(pipe.0, deadline)?;
    response.validate(request)?;
    write_all(pipe.0, &[1], deadline)?;
    Ok(response)
}
