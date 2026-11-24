//! Unlock an AD user via WinLDAP and domain controller discovery.
//! Explicit domain credentials in native Windows CredUI; LDAPS only; no PowerShell.
#![cfg(windows)]
use std::{ffi::c_void, ptr};
use anyhow::{bail, Context, Result};

#[repr(C)]
struct DcInfo {
    controller_name: *mut u16,
    controller_address: *mut u16,
    address_type: u32,
    domain_guid: [u32; 4],
    domain_name: *mut u16,
    dns_forest_name: *mut u16,
    flags: u32,
    dc_site_name: *mut u16,
    client_site_name: *mut u16,
}

#[repr(C)]
struct LdapMod {
    operation: u32,
    attribute: *mut u16,
    values: *mut *mut u16,
}

#[link(name = "netapi32")]
unsafe extern "system" {
    fn DsGetDcNameW(computer: *const u16, domain: *const u16,
        domain_guid: *const c_void, site: *const u16, flags: u32,
        info: *mut *mut DcInfo) -> u32;
    fn NetApiBufferFree(buffer: *mut c_void) -> u32;
}

#[link(name = "wldap32")]
unsafe extern "system" {
    fn ldap_sslinitW(host: *const u16, port: u32, secure: i32) -> *mut c_void;
    fn ldap_set_optionW(ld: *mut c_void, option: i32, value: *const c_void) -> u32;
    fn ldap_bind_sW(ld: *mut c_void, dn: *mut u16, cred: *mut u16, method: u32) -> u32;
    fn ldap_search_sW(ld: *mut c_void, base: *mut u16, scope: u32,
        filter: *mut u16, attrs: *mut *mut u16,
        only_names: u32, result: *mut *mut c_void) -> u32;
    fn ldap_count_entries(ld: *mut c_void, result: *mut c_void) -> u32;
    fn ldap_first_entry(ld: *mut c_void, result: *mut c_void) -> *mut c_void;
    fn ldap_get_valuesW(ld: *mut c_void, entry: *mut c_void,
        name: *mut u16) -> *mut *mut u16;
    fn ldap_value_freeW(values: *mut *mut u16) -> u32;
    fn ldap_get_dnW(ld: *mut c_void, entry: *mut c_void) -> *mut u16;
    fn ldap_memfreeW(value: *mut c_void);
    fn ldap_msgfree(result: *mut c_void) -> u32;
    fn ldap_modify_sW(ld: *mut c_void, dn: *mut u16,
        modifications: *mut *mut LdapMod) -> u32;
    fn ldap_unbind(ld: *mut c_void) -> u32;
}

#[repr(C)]
struct CredUiInfo {
    size: u32,
    parent: *mut c_void,
    message: *const u16,
    caption: *const u16,
    banner: *mut c_void,
}
#[link(name = "credui")]
unsafe extern "system" {
    fn CredUIPromptForCredentialsW(
        ui: *const CredUiInfo, target: *const u16, error: u32,
        username: *mut u16, username_count: u32,
        password: *mut u16, password_count: u32,
        save: *mut i32, flags: u32,
    ) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SecureZeroMemory(memory: *mut c_void, length: usize) -> *mut c_void;
}

// CREDUI_FLAGS_DO_NOT_PERSIST | CREDUI_FLAGS_ALWAYS_SHOW_UI |
// CREDUI_FLAGS_GENERIC_CREDENTIALS. Do not offer credential persistence.
const CREDUI_FLAGS: u32 = 0x2 | 0x80 | 0x40000;
struct DomainCredentials {
    username: Vec<u16>,
    password: Vec<u16>,
}
impl Drop for DomainCredentials {
    fn drop(&mut self) {
        unsafe {
            SecureZeroMemory(self.username.as_mut_ptr().cast(),
                self.username.capacity() * std::mem::size_of::<u16>());
            SecureZeroMemory(self.password.as_mut_ptr().cast(),
                self.password.capacity() * std::mem::size_of::<u16>());
        }
    }
}
fn domain_credentials(domain_controller: &str) -> Result<DomainCredentials> {
    let target = wide(&format!("SST Active Directory - {domain_controller}"));
    let message = wide("Introduce una cuenta del dominio autorizada para desbloquear usuarios. No se guardarán las credenciales.");
    let caption = wide("SST - Autenticación Active Directory");
    let ui = CredUiInfo {
        size: std::mem::size_of::<CredUiInfo>() as u32,
        parent: ptr::null_mut(),
        message: message.as_ptr(),
        caption: caption.as_ptr(),
        banner: ptr::null_mut(),
    };
    let mut username = vec![0u16; 513];
    let mut password = vec![0u16; 513];
    let mut save = 0i32;
    let rc = unsafe {
        CredUIPromptForCredentialsW(&ui, target.as_ptr(), 0,
            username.as_mut_ptr(), username.len() as u32,
            password.as_mut_ptr(), password.len() as u32,
            &mut save, CREDUI_FLAGS)
    };
    if rc != 0 {
        unsafe {
            SecureZeroMemory(username.as_mut_ptr().cast(), username.len() * 2);
            SecureZeroMemory(password.as_mut_ptr().cast(), password.len() * 2);
        }
        if rc == 1223 { bail!("autenticación de dominio cancelada por el usuario"); }
        bail!("no se pudo obtener credenciales AD mediante Windows (Win32 {rc})");
    }
    if username.first() == Some(&0) || password.first() == Some(&0) {
        let _cleared = DomainCredentials { username, password };
        bail!("usuario y contraseña de AD son obligatorios");
    }
    Ok(DomainCredentials { username, password })
}

const LDAP_SUCCESS: u32 = 0;
const LDAP_AUTH_SIMPLE: u32 = 0x80;
const LDAP_OPT_TIMELIMIT: i32 = 0x03;
const LDAP_SCOPE_BASE: u32 = 0;
const LDAP_SCOPE_SUBTREE: u32 = 2;
const LDAP_MOD_REPLACE: u32 = 2;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn from_wide(ptr: *const u16, max: usize) -> Result<String> {
    if ptr.is_null() { bail!("Active Directory devolvió un valor vacío"); }
    let len = (0..max).find(|&i| unsafe { *ptr.add(i) } == 0)
        .context("la respuesta de AD supera el límite permitido")?;
    Ok(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }))
}

struct Session(*mut c_void);
impl Drop for Session {
    fn drop(&mut self) {
        if !self.0.is_null() { unsafe { ldap_unbind(self.0); } }
    }
}
struct Search(*mut c_void);
impl Drop for Search {
    fn drop(&mut self) {
        if !self.0.is_null() { unsafe { ldap_msgfree(self.0); } }
    }
}

fn ldap_check(code: u32, operation: &str) -> Result<()> {
    if code == LDAP_SUCCESS { return Ok(()); }
    bail!("{operation}: LDAP {code}; comprueba credenciales actuales, permisos delegados, \
          estado de la cuenta y conectividad con el controlador")
}

fn controller() -> Result<String> {
    let mut result = ptr::null_mut::<DcInfo>();
    // DS_DIRECTORY_SERVICE_REQUIRED | DS_WRITABLE_REQUIRED.
    let rc = unsafe { DsGetDcNameW(ptr::null(), ptr::null(), ptr::null(),
        ptr::null(), 0x10 | 0x1000, &mut result) };
    if rc != 0 { bail!("no se encontró un controlador AD escribible (Win32 {rc})"); }
    if result.is_null() { bail!("DsGetDcName no proporcionó un controlador"); }
    let hostname = unsafe { from_wide((*result).controller_name, 256) };
    unsafe { NetApiBufferFree(result.cast()); }
    let hostname = hostname?.trim_start_matches('\\').to_owned();
    if hostname.is_empty() { bail!("nombre de controlador vacío"); }
    Ok(hostname)
}

fn search(ld: *mut c_void, base: &str, scope: u32, filter: &str,
          attributes: &[&str]) -> Result<Search> {
    let mut b = wide(base);
    let mut f = wide(filter);
    let mut attributes_w = attributes.iter().map(|a| wide(a)).collect::<Vec<_>>();
    let mut attribute_ptrs = attributes_w.iter_mut()
        .map(|a| a.as_mut_ptr()).collect::<Vec<_>>();
    attribute_ptrs.push(ptr::null_mut());
    let mut result = ptr::null_mut();
    let rc = unsafe { ldap_search_sW(ld, b.as_mut_ptr(), scope, f.as_mut_ptr(),
        attribute_ptrs.as_mut_ptr(), 0, &mut result) };
    if rc != 0 {
        if !result.is_null() { unsafe { ldap_msgfree(result); } }
        ldap_check(rc, "consulta de AD")?;
    }
    if result.is_null() { bail!("el controlador no devolvió datos LDAP"); }
    Ok(Search(result))
}

fn property(ld: *mut c_void, entry: *mut c_void, name: &str) -> Result<String> {
    let mut name_w = wide(name);
    let values = unsafe { ldap_get_valuesW(ld, entry, name_w.as_mut_ptr()) };
    if values.is_null() { bail!("AD no devolvió el atributo {name}"); }
    let first = unsafe { *values };
    let value = unsafe { from_wide(first, 8192) };
    unsafe { ldap_value_freeW(values); }
    value
}

fn user_filter(name: &str) -> String {
    // RFC4515 LDAP filter escaping prevents wildcard and injection selectors.
    let mut escaped = String::new();
    for c in name.chars() {
        match c {
            '\\' => escaped.push_str("\\5c"),
            '*' => escaped.push_str("\\2a"),
            '(' => escaped.push_str("\\28"),
            ')' => escaped.push_str("\\29"),
            '\0' => escaped.push_str("\\00"),
            _ => escaped.push(c),
        }
    }
    format!("(&(objectCategory=person)(objectClass=user)(sAMAccountName={escaped}))")
}

/// Unlock exactly one unambiguously identified AD user. Only lockoutTime changes.
/// The DC independently enforces delegated AD rights, irrespective of local UAC.
pub fn unlock(account: &str) -> Result<String> {
    let dc = controller()?;
    // Collect separate domain credentials; local SST/UAC identity is never
    // silently substituted for a privileged Active Directory account.
    let mut credentials = domain_credentials(&dc)?;
    // Port 636 and LDAP over TLS from the first byte; no plaintext fallback.
    let session = Session(unsafe { ldap_sslinitW(wide(&dc).as_ptr(), 636, 1) });
    if session.0.is_null() { bail!("no se pudo crear conexión LDAPS con {dc}:636"); }
    let limit = 12_i32;
    ldap_check(unsafe { ldap_set_optionW(session.0, LDAP_OPT_TIMELIMIT,
        (&limit as *const i32).cast()) }, "límite de consulta LDAP")?;
    ldap_check(unsafe { ldap_bind_sW(session.0,
        credentials.username.as_mut_ptr(),
        credentials.password.as_mut_ptr(), LDAP_AUTH_SIMPLE) },
        "autenticación explícita de cuenta administradora AD mediante LDAPS")?;
    // Credentials are not needed after server bind authentication.
    drop(credentials);

    let root = search(session.0, "", LDAP_SCOPE_BASE,
        "(objectClass=*)", &["defaultNamingContext"])?;
    let entry = unsafe { ldap_first_entry(session.0, root.0) };
    if entry.is_null() { bail!("RootDSE no devolvió defaultNamingContext"); }
    let base = property(session.0, entry, "defaultNamingContext")?;
    drop(root);

    let records = search(session.0, &base, LDAP_SCOPE_SUBTREE,
        &user_filter(account), &["distinguishedName"])?;
    let count = unsafe { ldap_count_entries(session.0, records.0) };
    if count != 1 {
        bail!(if count == 0 {"el usuario no existe en el dominio"}
            else {"más de una cuenta coincide; no se efectuó ningún cambio"});
    }
    let entry = unsafe { ldap_first_entry(session.0, records.0) };
    if entry.is_null() { bail!("respuesta LDAP sin cuenta de usuario"); }
    let dn_ptr = unsafe { ldap_get_dnW(session.0, entry) };
    let dn = unsafe { from_wide(dn_ptr, 8192) };
    if !dn_ptr.is_null() { unsafe { ldap_memfreeW(dn_ptr.cast()); } }
    let mut dn = wide(&dn?);
    drop(records);

    let mut attr = wide("lockoutTime");
    let mut zero = wide("0");
    let mut values = [zero.as_mut_ptr(), ptr::null_mut()];
    let mut change = LdapMod {
        operation: LDAP_MOD_REPLACE,
        attribute: attr.as_mut_ptr(),
        values: values.as_mut_ptr(),
    };
    let mut changes = [&mut change as *mut LdapMod, ptr::null_mut()];
    ldap_check(unsafe { ldap_modify_sW(session.0, dn.as_mut_ptr(),
        changes.as_mut_ptr()) }, "desbloqueo de cuenta")?;
    Ok(dc)
}

#[cfg(test)]
mod tests {
    use super::user_filter;
    #[test]
    fn names_are_not_interpreted_as_ldap_filter_expressions() {
        assert_eq!(user_filter("john*"), "(&(objectCategory=person)(objectClass=user)(sAMAccountName=john\\2a))");
        assert_eq!(user_filter("a)(b"), "(&(objectCategory=person)(objectClass=user)(sAMAccountName=a\\29\\28b))");
    }
}
