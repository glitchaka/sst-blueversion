use crate::core::broker::{ProcessIdentity, Request};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    mem::{size_of, zeroed},
    os::windows::fs::OpenOptionsExt,
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::FILE_SHARE_READ,
    System::{Pipes::*, Services::*, Threading::*},
};

pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub(super) struct Handle(pub HANDLE);
impl Handle {
    pub fn new(handle: HANDLE) -> Result<Self> {
        ensure!(
            !handle.is_null() && handle != INVALID_HANDLE_VALUE,
            "Win32 handle error {}",
            unsafe { GetLastError() }
        );
        Ok(Self(handle))
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct ScHandle(SC_HANDLE);
impl Drop for ScHandle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

pub(super) fn identity(process: HANDLE) -> Result<ProcessIdentity> {
    let mut created = unsafe { zeroed() };
    let mut exited = unsafe { zeroed() };
    let mut kernel = unsafe { zeroed() };
    let mut user = unsafe { zeroed() };
    ensure!(
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } != 0,
        "process creation time unavailable"
    );
    let pid = unsafe { GetProcessId(process) };
    ensure!(pid != 0, "process identity unavailable");
    Ok(ProcessIdentity {
        pid,
        created: ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64,
    })
}

pub(super) fn image_path(process: HANDLE) -> Result<String> {
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    ensure!(
        unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) } != 0,
        "process image unavailable"
    );
    Ok(String::from_utf16(&buffer[..length as usize])?)
}

pub(super) fn process_token(process: HANDLE) -> Result<Handle> {
    let mut token = null_mut();
    ensure!(
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } != 0,
        "process token unavailable"
    );
    Handle::new(token)
}

fn information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    let mut size = 0;
    unsafe {
        GetTokenInformation(token, class, null_mut(), 0, &mut size);
    }
    ensure!(size > 0 && size <= 65536, "token information size invalid");
    let mut data = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    ensure!(
        unsafe { GetTokenInformation(token, class, data.as_mut_ptr().cast(), size, &mut size) }
            != 0,
        "token information unavailable"
    );
    Ok(data)
}

fn token_value<T: Copy>(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<T> {
    let data = information(token, class)?;
    ensure!(
        data.len() * size_of::<usize>() >= size_of::<T>(),
        "short token information"
    );
    Ok(unsafe { *data.as_ptr().cast::<T>() })
}

pub(super) fn sid_string(sid: PSID) -> Result<String> {
    let mut text = null_mut();
    ensure!(
        unsafe { ConvertSidToStringSidW(sid, &mut text) } != 0,
        "invalid SID"
    );
    let result = unsafe { read_wide(text) };
    unsafe {
        LocalFree(text.cast());
    }
    result
}

unsafe fn read_wide(text: *const u16) -> Result<String> {
    ensure!(!text.is_null(), "missing Windows string");
    let mut len = 0;
    while len < 32768 && unsafe { *text.add(len) } != 0 {
        len += 1;
    }
    ensure!(len < 32768, "unbounded Windows string");
    Ok(String::from_utf16(unsafe {
        std::slice::from_raw_parts(text, len)
    })?)
}

pub(super) fn validate_operator_sid(text: &str) -> Result<()> {
    ensure!(
        text.starts_with("S-1-5-21-")
            && text.len() < 192
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S'),
        "operator must be an explicit local/domain account SID"
    );
    let mut sid = null_mut();
    ensure!(
        unsafe { ConvertStringSidToSidW(wide(text).as_ptr(), &mut sid) } != 0,
        "invalid operator SID"
    );
    let canonical = sid_string(sid);
    unsafe {
        LocalFree(sid);
    }
    ensure!(canonical? == text, "noncanonical operator SID");
    Ok(())
}

pub(super) fn expose_server_identity(operator: &str) -> Result<()> {
    // Permit the designated normal user to query/pin the server process for
    // mutual authentication, without granting VM access, injection or terminate.
    let sddl = wide(&format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00101000;;;{operator})"
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
        "server process ACL unavailable"
    );
    let result = (|| -> Result<()> {
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = null_mut();
        ensure!(
            unsafe {
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)
            } != 0
                && present != 0
                && !acl.is_null(),
            "invalid server process ACL"
        );
        ensure!(
            unsafe {
                SetSecurityInfo(
                    GetCurrentProcess(),
                    SE_KERNEL_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    acl,
                    null_mut(),
                )
            } == 0,
            "server process ACL installation failed"
        );
        Ok(())
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}

pub(super) struct TokenIdentity {
    pub sid: String,
    pub session: u32,
    pub integrity: u32,
    pub administrator: bool,
    pub interactive: bool,
    pub app_container: bool,
    pub authentication: (u32, i32),
}

pub(super) fn token_identity(token: HANDLE) -> Result<TokenIdentity> {
    let user = information(token, TokenUser)?;
    let sid = sid_string(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid })?;
    let label = information(token, TokenIntegrityLevel)?;
    let integrity_sid = unsafe { (*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
    let count = unsafe { *GetSidSubAuthorityCount(integrity_sid) };
    ensure!(count > 0, "invalid integrity SID");
    let integrity = unsafe { *GetSidSubAuthority(integrity_sid, (count - 1) as u32) };
    let groups = information(token, TokenGroups)?;
    let groups_header = unsafe { &*groups.as_ptr().cast::<TOKEN_GROUPS>() };
    let count = groups_header.GroupCount as usize;
    ensure!(
        count <= 4096
            && std::mem::offset_of!(TOKEN_GROUPS, Groups) + count * size_of::<SID_AND_ATTRIBUTES>()
                <= groups.len() * size_of::<usize>(),
        "invalid token groups"
    );
    let mut administrator = false;
    let mut interactive = false;
    for group in unsafe { std::slice::from_raw_parts(groups_header.Groups.as_ptr(), count) } {
        if group.Attributes & SE_GROUP_ENABLED == 0
            || group.Attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0
        {
            continue;
        }
        let sid = sid_string(group.Sid)?;
        administrator |= sid == "S-1-5-32-544";
        interactive |= sid == "S-1-5-4";
    }
    let elevation: TOKEN_ELEVATION = token_value(token, TokenElevation)?;
    let statistics: TOKEN_STATISTICS = token_value(token, TokenStatistics)?;
    Ok(TokenIdentity {
        sid,
        session: token_value(token, TokenSessionId)?,
        integrity,
        administrator: administrator && elevation.TokenIsElevated != 0 && integrity >= 0x3000,
        interactive,
        app_container: token_value::<u32>(token, TokenIsAppContainer)? != 0,
        authentication: (
            statistics.AuthenticationId.LowPart,
            statistics.AuthenticationId.HighPart,
        ),
    })
}

pub(super) struct TrustedImage {
    path: PathBuf,
    digest: [u8; 32],
    _file: File,
}
pub(super) fn installed_executable() -> Result<PathBuf> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath},
    };
    let mut raw = null_mut();
    ensure!(
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, null_mut(), &mut raw) } >= 0,
        "Program Files known folder unavailable"
    );
    let root = unsafe { read_wide(raw) };
    unsafe {
        CoTaskMemFree(raw.cast());
    }
    let install = PathBuf::from(root?).join(super::SERVICE).canonicalize()?;
    let exe = install.join("sst.exe").canonicalize()?;
    ensure!(
        exe.parent() == Some(install.as_path()),
        "broker image escapes installation directory"
    );
    protected_acl(&install, true)?;
    protected_acl(&exe, false)?;
    Ok(exe)
}
impl TrustedImage {
    pub fn current() -> Result<Self> {
        let path = std::env::current_exe()?.canonicalize()?;
        ensure!(
            path == installed_executable()?,
            "use SST from the protected broker installation"
        );
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)?;
        let digest = hash(&mut file)?;
        Ok(Self {
            path,
            digest,
            _file: file,
        })
    }
    pub fn verify(&self, process: HANDLE) -> Result<File> {
        let path = PathBuf::from(image_path(process)?).canonicalize()?;
        ensure!(
            path.to_string_lossy()
                .eq_ignore_ascii_case(&self.path.to_string_lossy()),
            "peer must use the installed SST image"
        );
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)?;
        ensure!(
            hash(&mut file)? == self.digest,
            "peer image SHA-256 mismatch"
        );
        Ok(file)
    }
}

/// Reject writable/owner-controlled image roots even if someone manually
/// registered the service outside the supplied installer. Unrecognized ACEs
/// fail closed rather than attempting a partial effective-permissions model.
fn protected_acl(path: &std::path::Path, directory: bool) -> Result<()> {
    let mut owner = null_mut();
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    ensure!(
        unsafe {
            GetNamedSecurityInfoW(
                wide(&path.to_string_lossy()).as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        } == 0,
        "install ACL unavailable"
    );
    let result = (|| -> Result<()> {
        ensure!(
            matches!(sid_string(owner)?.as_str(), "S-1-5-18" | "S-1-5-32-544"),
            "installation owner must be SYSTEM or Administrators"
        );
        ensure!(!acl.is_null(), "null installation DACL refused");
        if directory {
            let mut control = 0;
            let mut revision = 0;
            ensure!(
                unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
                    != 0
                    && control & SE_DACL_PROTECTED != 0,
                "installation directory must have protected DACL"
            );
        }
        let mut info: ACL_SIZE_INFORMATION = unsafe { zeroed() };
        ensure!(
            unsafe {
                GetAclInformation(
                    acl,
                    (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                    size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
            } != 0,
            "installation ACL invalid"
        );
        for index in 0..info.AceCount {
            let mut raw = null_mut();
            ensure!(
                unsafe { GetAce(acl, index, &mut raw) } != 0,
                "installation ACE invalid"
            );
            let header = unsafe { &*raw.cast::<ACE_HEADER>() };
            // ACCESS_ALLOWED_ACE_TYPE=0, ACCESS_DENIED_ACE_TYPE=1.
            ensure!(header.AceType <= 1, "unsupported installation ACE");
            if header.AceType == 1 {
                continue;
            }
            let ace = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
            let dangerous = 0x10000000
                | 0x40000000
                | 0x00010000
                | 0x00040000
                | 0x00080000
                | 0x2
                | 0x4
                | 0x10
                | 0x40
                | 0x100;
            if ace.Mask & dangerous != 0 {
                let sid = sid_string((&ace.SidStart as *const u32).cast_mut().cast())?;
                ensure!(
                    matches!(sid.as_str(), "S-1-5-18" | "S-1-5-32-544"),
                    "unprivileged write access to broker installation refused"
                );
            }
        }
        Ok(())
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}

fn hash(file: &mut File) -> Result<[u8; 32]> {
    ensure!(
        file.metadata()?.len() <= 512 * 1024 * 1024,
        "image exceeds validation budget"
    );
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().into())
}

struct Revert;
impl Drop for Revert {
    fn drop(&mut self) {
        // Continuing a privileged worker after a failed revert is unsafe.
        if unsafe { RevertToSelf() } == 0 {
            std::process::abort();
        }
    }
}

pub(super) struct Client {
    pub sid: String,
    pub session: u32,
    pub administrator: bool,
    _process: Handle,
    _image: File,
}

impl Client {
    pub fn alive(&self) -> bool {
        (unsafe { WaitForSingleObject(self._process.0, 0) }) == 258
    }
}

pub(super) fn authenticate(
    pipe: HANDLE,
    request: &Request,
    operator: &str,
    image: &TrustedImage,
) -> Result<Client> {
    request.validate()?;
    let mut pid = 0;
    let mut session = 0;
    ensure!(
        unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } != 0
            && unsafe { GetNamedPipeClientSessionId(pipe, &mut session) } != 0,
        "pipe peer identity unavailable"
    );
    ensure!(
        pid == request.client.pid && session == request.session && session > 0,
        "pipe peer PID/session mismatch"
    );
    let process = Handle::new(unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    })?;
    ensure!(
        identity(process.0)? == request.client
            && unsafe { WaitForSingleObject(process.0, 0) } == 258,
        "client PID reused/exited"
    );
    // Must occur after reading the request. The identity comes from Windows,
    // not from a SID or administrator flag supplied in JSON.
    ensure!(
        unsafe { ImpersonateNamedPipeClient(pipe) } != 0,
        "pipe impersonation failed"
    );
    let revert = Revert;
    let mut token = null_mut();
    ensure!(
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } != 0,
        "pipe token unavailable"
    );
    let token = Handle::new(token)?;
    let peer = token_identity(token.0)?;
    drop(revert);
    ensure!(
        peer.sid == operator
            && peer.session == session
            && peer.interactive
            && !peer.app_container
            && peer.integrity >= 0x2000,
        "operator SID/session/integrity policy denied"
    );
    let primary = token_identity(process_token(process.0)?.0)?;
    ensure!(
        primary.sid == peer.sid
            && primary.session == peer.session
            && primary.authentication == peer.authentication
            && primary.integrity == peer.integrity
            && primary.administrator == peer.administrator,
        "pipe token differs from client process token"
    );
    let file = image.verify(process.0)?;
    Ok(Client {
        sid: peer.sid,
        session: peer.session,
        administrator: peer.administrator,
        _process: process,
        _image: file,
    })
}

/// SCM is the authority for the server PID/account; the pipe name is not trusted.
pub(super) fn verify_server(pipe: HANDLE, image: &TrustedImage) -> Result<(Handle, File)> {
    let mut pid = 0;
    ensure!(
        unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } != 0,
        "server PID unavailable"
    );
    let manager = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) };
    ensure!(!manager.is_null(), "SCM unavailable");
    let manager = ScHandle(manager);
    let service = unsafe {
        OpenServiceW(
            manager.0,
            wide(super::SERVICE).as_ptr(),
            SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG,
        )
    };
    ensure!(!service.is_null(), "broker service is not installed");
    let service = ScHandle(service);
    let mut status: SERVICE_STATUS_PROCESS = unsafe { zeroed() };
    let mut needed = 0;
    ensure!(
        unsafe {
            QueryServiceStatusEx(
                service.0,
                SC_STATUS_PROCESS_INFO,
                (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
                size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut needed,
            )
        } != 0,
        "service status unavailable"
    );
    ensure!(
        status.dwCurrentState == SERVICE_RUNNING
            && status.dwProcessId == pid
            && status.dwServiceType == SERVICE_WIN32_OWN_PROCESS,
        "pipe server is not the running broker service"
    );
    unsafe {
        QueryServiceConfigW(service.0, null_mut(), 0, &mut needed);
    }
    ensure!(
        needed > 0 && needed <= 65536,
        "invalid service configuration size"
    );
    let mut config = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    ensure!(
        unsafe { QueryServiceConfigW(service.0, config.as_mut_ptr().cast(), needed, &mut needed) }
            != 0,
        "service configuration unavailable"
    );
    let config = unsafe { &*config.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
    ensure!(
        unsafe { read_wide(config.lpServiceStartName) }?.eq_ignore_ascii_case("LocalSystem"),
        "broker service account is not LocalSystem"
    );
    let process = Handle::new(unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    })?;
    ensure!(
        unsafe { WaitForSingleObject(process.0, 0) } == 258,
        "server exited"
    );
    let file = image.verify(process.0)?;
    Ok((process, file))
}
