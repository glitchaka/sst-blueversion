//! Active HTTPS download throughput through WinHTTP. No curl.exe, external
//! helper, credential export or shell commands. Supports bounded active tests.
use std::{ffi::c_void, mem::size_of, ptr, time::{Duration, Instant}};
use anyhow::{bail, Result};

type HandleRaw = *mut c_void;
#[link(name = "winhttp")]
unsafe extern "system" {
    fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16,
        bypass: *const u16, flags: u32) -> HandleRaw;
    fn WinHttpConnect(session: HandleRaw, server: *const u16, port: u16,
        reserved: u32) -> HandleRaw;
    fn WinHttpOpenRequest(connection: HandleRaw, verb: *const u16,
        object: *const u16, version: *const u16, referrer: *const u16,
        accepts: *const *const u16, flags: u32) -> HandleRaw;
    fn WinHttpSetTimeouts(handle: HandleRaw, resolve: i32, connect: i32,
        send: i32, receive: i32) -> i32;
    fn WinHttpSendRequest(handle: HandleRaw, headers: *const u16, length: u32,
        optional: *const c_void, optional_length: u32, total_length: u32,
        context: usize) -> i32;
    fn WinHttpReceiveResponse(handle: HandleRaw, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(handle: HandleRaw, level: u32, name: *const u16,
        buffer: *mut c_void, length: *mut u32, index: *mut u32) -> i32;
    fn WinHttpReadData(handle: HandleRaw, buffer: *mut c_void, to_read: u32,
        bytes_read: *mut u32) -> i32;
    fn WinHttpWriteData(handle: HandleRaw, buffer: *const c_void, to_write: u32,
        bytes_written: *mut u32) -> i32;
    fn WinHttpCloseHandle(handle: HandleRaw) -> i32;
}
struct Handle(HandleRaw);
impl Handle {
    fn checked(raw: HandleRaw, action: &str) -> Result<Self> {
        if raw.is_null() { bail!("{action}: {}", std::io::Error::last_os_error()); }
        Ok(Self(raw))
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() { unsafe { WinHttpCloseHandle(self.0); } }
    }
}

pub struct DownloadMeasurement {
    pub url: String,
    pub http_status: u32,
    pub bytes: u64,
    pub elapsed_seconds: f64,
}

#[derive(Debug, PartialEq, Eq)]
struct UrlParts {
    host: String,
    port: u16,
    resource: String,
}

fn parse_https(value: &str) -> Result<UrlParts> {
    let Some(raw) = value.strip_prefix("https://") else {
        bail!("la prueba activa requiere URL HTTPS");
    };
    let (hostport, resource) = match raw.find(&['/', '?'][..]) {
        Some(i) if raw.as_bytes()[i] == b'?' => (&raw[..i], format!("/{}", &raw[i..])),
        Some(i) => (&raw[..i], raw[i..].to_owned()),
        None => (raw, "/".to_owned()),
    };
    if hostport.is_empty() || hostport.contains('@') || hostport.contains('#') {
        bail!("autoridad HTTPS inválida o con credenciales");
    }
    let (host, port) = if let Some((host, port)) = hostport.rsplit_once(':') {
        let port = port.parse::<u16>().map_err(|_| anyhow::anyhow!("puerto HTTPS inválido"))?;
        (host, port)
    } else { (hostport, 443) };
    if port == 0 || host.is_empty() || host.len() > 253
        || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        || resource.len() > 2048
        || resource.chars().any(|c| c.is_control() || c == ' ')
    {
        bail!("host, puerto o ruta HTTPS no admitidos");
    }
    Ok(UrlParts { host: host.to_owned(), port, resource })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn measure(url: &str, max_seconds: u32, max_mib: u32) -> Result<DownloadMeasurement> {
    let parts = parse_https(url)?;
    // WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY = 4; WINHTTP_FLAG_SECURE = 0x00800000.
    let session = Handle::checked(unsafe {
        WinHttpOpen(wide("SST-Netcheck/1.0").as_ptr(), 4, ptr::null(), ptr::null(), 0)
    }, "WinHttpOpen")?;
    let timeout = (max_seconds.min(30) * 1000).min(5000) as i32;
    if unsafe { WinHttpSetTimeouts(session.0, 3000, timeout, timeout, timeout) } == 0 {
        bail!("WinHttpSetTimeouts: {}", std::io::Error::last_os_error());
    }
    let connection = Handle::checked(unsafe {
        WinHttpConnect(session.0, wide(&parts.host).as_ptr(), parts.port, 0)
    }, "WinHttpConnect")?;
    let request = Handle::checked(unsafe {
        WinHttpOpenRequest(connection.0, wide("GET").as_ptr(),
            wide(&parts.resource).as_ptr(), ptr::null(), ptr::null(), ptr::null(), 0x00800000)
    }, "WinHttpOpenRequest")?;
    if unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) } == 0 {
        bail!("WinHttpSendRequest: {}", std::io::Error::last_os_error());
    }
    if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
        bail!("WinHttpReceiveResponse: {}", std::io::Error::last_os_error());
    }
    let mut code = 0u32;
    let mut length = size_of::<u32>() as u32;
    if unsafe {
        WinHttpQueryHeaders(request.0, 0x2000_0000 | 19, ptr::null(),
            (&mut code as *mut u32).cast(), &mut length, ptr::null_mut())
    } == 0 {
        bail!("WinHttpQueryHeaders: {}", std::io::Error::last_os_error());
    }
    if !(200..300).contains(&code) { bail!("HTTP {code}: transferencia no válida"); }
    let started = Instant::now();
    let time_limit = Duration::from_secs(max_seconds as u64);
    let max_bytes = u64::from(max_mib) * 1024 * 1024;
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    while started.elapsed() < time_limit && bytes < max_bytes {
        let remaining = (max_bytes - bytes).min(buffer.len() as u64) as u32;
        let mut read = 0u32;
        if unsafe {
            WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), remaining, &mut read)
        } == 0 {
            bail!("WinHttpReadData tras {bytes} bytes: {}", std::io::Error::last_os_error());
        }
        if read == 0 { break; }
        bytes += u64::from(read);
    }
    let elapsed_seconds = started.elapsed().as_secs_f64();
    if bytes == 0 { bail!("el servidor HTTPS no entregó datos medibles"); }
    Ok(DownloadMeasurement {
        url: url.to_owned(), http_status: code, bytes, elapsed_seconds
    })
}


/// A native HTTP(S) latency probe: one GET request, no ICMP or raw sockets.
/// Includes connection and TLS setup; it must not be described as ICMP ping.
pub fn latency(url: &str) -> Result<f64> {
    let parts = parse_https(url)?;
    let started = Instant::now();
    let session = Handle::checked(unsafe {
        WinHttpOpen(wide("SST-Netcheck/1.0").as_ptr(), 4, ptr::null(), ptr::null(), 0)
    }, "WinHttpOpen")?;
    if unsafe { WinHttpSetTimeouts(session.0, 3000, 4000, 4000, 4000) } == 0 {
        bail!("WinHttpSetTimeouts: {}", std::io::Error::last_os_error());
    }
    let connection = Handle::checked(unsafe {
        WinHttpConnect(session.0, wide(&parts.host).as_ptr(), parts.port, 0)
    }, "WinHttpConnect")?;
    let request = Handle::checked(unsafe {
        WinHttpOpenRequest(connection.0, wide("GET").as_ptr(), wide(&parts.resource).as_ptr(),
            ptr::null(), ptr::null(), ptr::null(), 0x00800000)
    }, "WinHttpOpenRequest")?;
    if unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) } == 0 {
        bail!("WinHttpSendRequest: {}", std::io::Error::last_os_error());
    }
    if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
        bail!("WinHttpReceiveResponse: {}", std::io::Error::last_os_error());
    }
    let millis = started.elapsed().as_secs_f64() * 1000.0;
    if !(200..300).contains(&status_code(request.0)?) {
        bail!("el servidor de latencia respondió con un estado HTTP no exitoso");
    }
    Ok(millis)
}

fn status_code(request: HandleRaw) -> Result<u32> {
    let mut code = 0u32;
    let mut len = size_of::<u32>() as u32;
    if unsafe { WinHttpQueryHeaders(request, 0x2000_0000 | 19, ptr::null(),
        (&mut code as *mut u32).cast(), &mut len, ptr::null_mut()) } == 0 {
        bail!("WinHttpQueryHeaders: {}", std::io::Error::last_os_error());
    }
    Ok(code)
}

pub struct UploadMeasurement {
    pub url: String,
    pub http_status: u32,
    pub bytes: u64,
    pub elapsed_seconds: f64,
}

/// Upload generated non-user data over a single WinHTTP HTTPS POST.
/// A response with HTTP 2xx is required before any throughput is reported.
/// If the transfer exceeds the time budget, it is explicitly a failed sample.
pub fn upload(url: &str, max_seconds: u32, max_mib: u32) -> Result<UploadMeasurement> {
    let parts = parse_https(url)?;
    let size = u64::from(max_mib.min(32)) * 1024 * 1024;
    if size == 0 { bail!("la transferencia de subida requiere al menos 1 MiB"); }
    let session = Handle::checked(unsafe {
        WinHttpOpen(wide("SST-Netcheck/1.0").as_ptr(), 4, ptr::null(), ptr::null(), 0)
    }, "WinHttpOpen")?;
    let timeout = (max_seconds.clamp(1, 30) * 1000).min(5000) as i32;
    if unsafe { WinHttpSetTimeouts(session.0, 3000, timeout, timeout, timeout) } == 0 {
        bail!("WinHttpSetTimeouts: {}", std::io::Error::last_os_error());
    }
    let connection = Handle::checked(unsafe {
        WinHttpConnect(session.0, wide(&parts.host).as_ptr(), parts.port, 0)
    }, "WinHttpConnect")?;
    let request = Handle::checked(unsafe {
        WinHttpOpenRequest(connection.0, wide("POST").as_ptr(), wide(&parts.resource).as_ptr(),
            ptr::null(), ptr::null(), ptr::null(), 0x00800000)
    }, "WinHttpOpenRequest")?;
    let headers = wide("Content-Type: application/octet-stream\r\n");
    if unsafe {
        WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX,
            ptr::null(), 0, size as u32, 0)
    } == 0 {
        bail!("WinHttpSendRequest POST: {}", std::io::Error::last_os_error());
    }
    // Non-sensitive generated bytes. No files, user information or secrets.
    let mut chunk = [0u8; 64 * 1024];
    let mut state = 0x57e5_79b9u32;
    for byte in &mut chunk {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *byte = (state & 0xff) as u8;
    }
    let mut sent = 0u64;
    let start = Instant::now();
    while sent < size {
        if start.elapsed() >= Duration::from_secs(max_seconds as u64) {
            bail!("subida incompleta: límite de tiempo alcanzado tras {sent} bytes");
        }
        let wanted = (size - sent).min(chunk.len() as u64) as u32;
        let mut written = 0u32;
        if unsafe { WinHttpWriteData(request.0, chunk.as_ptr().cast(), wanted, &mut written) } == 0 {
            bail!("WinHttpWriteData: {}", std::io::Error::last_os_error());
        }
        if written == 0 { bail!("WinHTTP no transmitió datos de subida"); }
        sent += u64::from(written);
    }
    if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
        bail!("WinHttpReceiveResponse POST: {}", std::io::Error::last_os_error());
    }
    let seconds = start.elapsed().as_secs_f64();
    let code = status_code(request.0)?;
    if !(200..300).contains(&code) { bail!("HTTP {code}: el servidor rechazó la subida"); }
    Ok(UploadMeasurement { url: url.to_owned(), http_status: code,
        bytes: sent, elapsed_seconds: seconds })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validate_user_supplied_https_endpoint() {
        assert_eq!(parse_https("https://example.net:8443/file.bin").unwrap(),
            UrlParts { host:"example.net".into(), port:8443, resource:"/file.bin".into() });
        assert!(parse_https("http://example.net/file").is_err());
        assert!(parse_https("https://user:password@example.net/").is_err());
        assert!(parse_https("https://example.net:0/").is_err());
        assert!(parse_https("https://example.net/hello world").is_err());
    }
}
