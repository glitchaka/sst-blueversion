//! Unlock an AD user via WinLDAP and domain controller discovery.
//! Explicit domain credentials through SST terminal and sealed SSPI Negotiate LDAP; no PowerShell.
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
    fn ldap_initW(host: *const u16, port: u32) -> *mut c_void;
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
    fn ldap_get_optionW(ld: *mut c_void, option: i32, value: *mut c_void) -> u32;
}

struct DomainCredentials {
    username: Vec<u16>,
    password: Vec<u16>,
}
impl Drop for DomainCredentials {
    fn drop(&mut self) {
        for value in &mut self.username {
            unsafe { std::ptr::write_volatile(value, 0); }
        }
        for value in &mut self.password {
            unsafe { std::ptr::write_volatile(value, 0); }
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
fn domain_credentials(terminal: &mut dyn crate::core::ports::TerminalSession, dc: &str, domain: &str) -> Result<DomainCredentials> {
    use crate::core::ports::TerminalKey;
    use std::time::Duration;
    let mut credentials = DomainCredentials { username: Vec::with_capacity(1025), password: Vec::with_capacity(1025) };
    let mut password_field = false;
    loop {
        let user = String::from_utf16_lossy(&credentials.username);
        let mask = "*".repeat(credentials.password.len());
        terminal.clear()?;
        terminal.write(&format!("\x1b[1;38;2;250;189;47mAUTENTICACIÓN ACTIVE DIRECTORY\x1b[0m\r\n\r\nControlador: {dc}\r\nDominio: {domain}\r\nFormato: DOMINIO\\usuario o usuario@dominio\r\n\r\n{} Usuario administrador: {user}\r\n{} Contraseña: {mask}\r\n\r\nEnter: continuar / confirmar    Esc: cancelar\r\n", if password_field {" "} else {">"}, if password_field {">"} else {" "}))?;
        terminal.flush()?;
        match terminal.poll_key(Duration::from_millis(100))? {
            Some(TerminalKey::Escape) => bail!("autenticación cancelada"),
            Some(TerminalKey::Enter) if !password_field && !credentials.username.is_empty() => password_field = true,
            Some(TerminalKey::Enter) if password_field && !credentials.password.is_empty() => break,
            Some(TerminalKey::Backspace) => {
                let field = if password_field { &mut credentials.password } else { &mut credentials.username };
                if let Some(last) = field.last_mut() {
                    let surrogate = (0xdc00..=0xdfff).contains(last);
                    unsafe { std::ptr::write_volatile(last, 0); }
                    field.pop();
                    if surrogate {
                        if let Some(last) = field.last_mut() {
                            unsafe { std::ptr::write_volatile(last, 0); }
                            field.pop();
                        }
                    }
                }
            }
            Some(key @ (TerminalKey::Char(_) | TerminalKey::Space)) => {
                let ch = match key { TerminalKey::Char(ch) => ch, _ => ' ' };
                let field = if password_field { &mut credentials.password } else { &mut credentials.username };
                if !ch.is_control() && field.len() < 1022 {
                    let mut buf = [0u16; 2];
                    field.extend_from_slice(ch.encode_utf16(&mut buf));
                    for value in &mut buf { unsafe { std::ptr::write_volatile(value, 0); } }
                }
            }
            _ => {}
        }
    }
    credentials.username.push(0);
    credentials.password.push(0);
    terminal.clear()?;
    terminal.write("AUTENTICACIÓN ACTIVE DIRECTORY\r\n[2/3] Autenticando mediante LDAP firmado y cifrado...\r\n")?;
    terminal.flush()?;
    Ok(credentials)
}

// SSPI expects explicit user/domain/password in this C ABI layout.
#[repr(C)]
struct SecWinntAuthIdentityW {
    user: *mut u16,
    user_length: u32,
    domain: *mut u16,
    domain_length: u32,
    password: *mut u16,
    password_length: u32,
    flags: u32,
}
const SEC_WINNT_AUTH_IDENTITY_UNICODE: u32 = 2;

const LDAP_SUCCESS: u32 = 0;
const LDAP_AUTH_NEGOTIATE: u32 = 0x0486;
const LDAP_OPT_ENCRYPT: i32 = 0x96;
const LDAP_OPT_SIGN: i32 = 0x95;
const LDAP_OPT_TIMELIMIT: i32 = 0x03;
const LDAP_OPT_PROTOCOL_VERSION: i32 = 0x11;
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
    bail!("{operation}: LDAP {code}; comprueba credenciales administrativas y permisos delegados, \
          estado de la cuenta y conectividad con el controlador")
}

fn controller() -> Result<(String, String)> {
    let mut result = ptr::null_mut::<DcInfo>();
    // DS_DIRECTORY_SERVICE_REQUIRED | DS_WRITABLE_REQUIRED.
    let rc = unsafe { DsGetDcNameW(ptr::null(), ptr::null(), ptr::null(),
        ptr::null(), 0x10 | 0x1000 | 0x40000000, &mut result) };
    if rc != 0 { bail!("no se encontró un controlador AD escribible (Win32 {rc})"); }
    if result.is_null() { bail!("DsGetDcName no proporcionó un controlador"); }
    let hostname = unsafe { from_wide((*result).controller_name, 256) };
    let domain = unsafe { from_wide((*result).domain_name, 256) };
    unsafe { NetApiBufferFree(result.cast()); }
    let hostname = hostname?.trim_start_matches('\\').to_owned();
    if hostname.is_empty() { bail!("nombre de controlador vacío"); }
    let domain = domain?;
    if domain.trim().is_empty() { bail!("el controlador no devolvió el dominio AD"); }
    Ok((hostname, domain))
}

fn administrative_identity(typed: &str, default_domain: &str) -> Result<(String, String)> {
    let typed = typed.trim();
    let (user, domain) = if let Some((realm, name)) = typed.split_once('\\') {
        if realm.is_empty() || name.is_empty() || name.contains('\\') || name.contains('@') {
            bail!("usa DOMINIO\\usuario o usuario@dominio");
        }
        (name.to_owned(), realm.to_owned())
    } else if let Some((name, realm)) = typed.split_once('@') {
        if name.is_empty() || realm.is_empty() || realm.contains('@') {
            bail!("UPN administrativo inválido");
        }
        (typed.to_owned(), String::new())
    } else {
        (typed.to_owned(), default_domain.to_owned())
    };
    if user.is_empty() || user.chars().any(char::is_control) || domain.chars().any(char::is_control) {
        bail!("identidad administrativa inválida");
    }
    Ok((user, domain))
}

fn bind_error(ld: *mut c_void, code: u32) -> anyhow::Error {
    // LDAP_OPT_SERVER_ERROR is a WinLDAP-owned diagnostic string.
    let mut diagnostic = ptr::null_mut::<u16>();
    let status = unsafe { ldap_get_optionW(ld, 0x33, (&mut diagnostic as *mut *mut u16).cast()) };
    let mut subcode = None;
    if status == LDAP_SUCCESS && !diagnostic.is_null() {
        if let Ok(message) = unsafe { from_wide(diagnostic, 8192) } {
            let words = message.split_whitespace().collect::<Vec<_>>();
            subcode = words.windows(2).find(|pair| pair[0].eq_ignore_ascii_case("data"))
                .and_then(|pair| u32::from_str_radix(pair[1].trim_matches(|c: char| !c.is_ascii_hexdigit()), 16).ok());
        }
    }
    if !diagnostic.is_null() { unsafe { ldap_memfreeW(diagnostic.cast()); } }
    let reason = if code == 49 {
        match subcode {
            Some(0x525) => "cuenta administrativa no encontrada",
            Some(0x52e) => "identidad o contraseña rechazadas",
            Some(0x530) => "horario de inicio de sesión restringido",
            Some(0x531) => "equipo no autorizado para iniciar sesión",
            Some(0x532) => "contraseña expirada",
            Some(0x533) => "cuenta administrativa deshabilitada",
            Some(0x701) => "cuenta administrativa expirada",
            Some(0x773) => "se requiere cambiar la contraseña",
            Some(0x775) => "cuenta administrativa bloqueada",
            _ => "el controlador rechazó la autenticación; verifica la identidad y las restricciones de la cuenta",
        }
    } else {
        "falló la autenticación LDAP firmada y cifrada"
    };
    match subcode {
        Some(data) => anyhow::anyhow!("LDAP {code}, data {data:x}: {reason}. No se modificó ninguna cuenta."),
        None => anyhow::anyhow!("LDAP {code}: {reason}. No se modificó ninguna cuenta."),
    }
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
pub fn unlock(account: &str, terminal_factory: &dyn crate::core::ports::TerminalFactory) -> Result<(String,bool)> {
    use std::io::IsTerminal;
    let (dc, dns_domain) = controller()?;
    let embedded = crate::adapters::terminal::io::output_sender().is_some();
    let interactive_console = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !embedded && !interactive_console {
        bail!("unban necesita una terminal interactiva SST; no admite credenciales redirigidas");
    }
    let mut credentials = {
        let mut terminal=terminal_factory.alternate_screen()
            .context("no se pudo abrir el formulario de SST")?;
        domain_credentials(&mut *terminal,&dc,&dns_domain)?
    };
    let typed_len = credentials.username.iter().position(|ch| *ch == 0)
        .unwrap_or(credentials.username.len());
    let typed = String::from_utf16_lossy(&credentials.username[..typed_len]);
    let (user, domain) = administrative_identity(&typed, &dns_domain)?;
    if user.trim().is_empty() || user.chars().any(|ch| ch.is_control())
        || domain.chars().any(|ch| ch.is_control())
    {
        bail!("identidad administrativa de Active Directory inválida");
    }
    let mut user_w = wide(&user);
    let mut domain_w = wide(&domain);
    let mut identity = SecWinntAuthIdentityW {
        user: user_w.as_mut_ptr(),
        user_length: (user_w.len() - 1) as u32,
        domain: domain_w.as_mut_ptr(),
        domain_length: (domain_w.len() - 1) as u32,
        password: credentials.password.as_mut_ptr(),
        password_length: credentials.password.iter().position(|ch| *ch == 0)
            .unwrap_or(credentials.password.len()) as u32,
        flags: SEC_WINNT_AUTH_IDENTITY_UNICODE,
    };

    // Kerberos/Negotiate signs and encrypts LDAP packets without requiring
    // a separately provisioned LDAPS certificate on the domain controller.
    let session = Session(unsafe { ldap_initW(wide(&dc).as_ptr(), 389) });
    if session.0.is_null() { bail!("no se pudo conectar con controlador AD {dc}"); }
    let version=3_u32;
    ldap_check(unsafe{ldap_set_optionW(session.0,LDAP_OPT_PROTOCOL_VERSION,(&version as *const u32).cast())},"LDAP v3")?;
    ldap_check(unsafe { ldap_set_optionW(session.0, LDAP_OPT_SIGN,
        1usize as *const c_void) }, "firma LDAP")?;
    ldap_check(unsafe { ldap_set_optionW(session.0, LDAP_OPT_ENCRYPT,
        1usize as *const c_void) }, "cifrado LDAP")?;
    let limit = 12_i32;
    ldap_check(unsafe { ldap_set_optionW(session.0, LDAP_OPT_TIMELIMIT,
        (&limit as *const i32).cast()) }, "límite de consulta LDAP")?;
    let bind_status = unsafe { ldap_bind_sW(session.0, ptr::null_mut(),
        (&mut identity as *mut SecWinntAuthIdentityW).cast(), LDAP_AUTH_NEGOTIATE) };
    drop(credentials);
    // Wipe the temporary SSPI identity buffers before further operations.
    for ch in user_w.iter_mut().chain(domain_w.iter_mut()) {
        unsafe{std::ptr::write_volatile(ch,0);}
    }
    if bind_status != LDAP_SUCCESS {
        return Err(bind_error(session.0, bind_status));
    }

    let root = search(session.0, "", LDAP_SCOPE_BASE,
        "(objectClass=*)", &["defaultNamingContext"])?;
    let entry = unsafe { ldap_first_entry(session.0, root.0) };
    if entry.is_null() { bail!("RootDSE no devolvió defaultNamingContext"); }
    let base = property(session.0, entry, "defaultNamingContext")?;
    drop(root);

    let records = search(session.0, &base, LDAP_SCOPE_SUBTREE,
        &user_filter(account), &["distinguishedName","lockoutTime"])?;
    let count = unsafe { ldap_count_entries(session.0, records.0) };
    if count != 1 {
        bail!(if count == 0 {"el usuario no existe en el dominio"}
            else {"más de una cuenta coincide; no se efectuó ningún cambio"});
    }
    let entry = unsafe { ldap_first_entry(session.0, records.0) };
    if entry.is_null() { bail!("respuesta LDAP sin cuenta de usuario"); }
    let before=property(session.0,entry,"lockoutTime").unwrap_or_default();
    if before=="0"{return Ok((dc,false));}
    let dn_ptr = unsafe { ldap_get_dnW(session.0, entry) };
    let dn = unsafe { from_wide(dn_ptr, 8192) };
    if !dn_ptr.is_null() { unsafe { ldap_memfreeW(dn_ptr.cast()); } }
    let dn=dn?;
    let mut dn_w = wide(&dn);
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
    ldap_check(unsafe { ldap_modify_sW(session.0, dn_w.as_mut_ptr(),
        changes.as_mut_ptr()) }, "desbloqueo de cuenta")?;
    // Confirm the write against the same writable DC before reporting success.
    let verification=search(session.0,&dn,LDAP_SCOPE_BASE,"(objectClass=*)",&["lockoutTime"])
        .context("AD aceptó la modificación, pero no se pudo verificar su resultado")?;
    let entry=unsafe{ldap_first_entry(session.0,verification.0)};
    if entry.is_null(){bail!("AD aceptó la modificación, pero no devolvió la cuenta para verificarla");}
    let lockout=property(session.0,entry,"lockoutTime")
        .context("AD aceptó la modificación, pero no se pudo leer lockoutTime")?;
    if lockout!="0"{bail!("el controlador no confirmó lockoutTime=0; resultado no verificado");}
    Ok((dc,true))
}

#[cfg(test)]
mod tests {
    use super::{administrative_identity, user_filter};
    #[test]
    fn admin_identity_uses_discovered_domain_and_preserves_explicit_formats() {
        assert_eq!(administrative_identity("admin", "corp.example").unwrap(), ("admin".into(), "corp.example".into()));
        assert_eq!(administrative_identity("CORP\\admin", "other.example").unwrap(), ("admin".into(), "CORP".into()));
        assert_eq!(administrative_identity("admin@corp.example", "other.example").unwrap(), ("admin@corp.example".into(), String::new()));
        assert!(administrative_identity("CORP\\", "corp.example").is_err());
    }
    #[test]
    fn names_are_not_interpreted_as_ldap_filter_expressions() {
        assert_eq!(user_filter("john*"), "(&(objectCategory=person)(objectClass=user)(sAMAccountName=john\\2a))");
        assert_eq!(user_filter("a)(b"), "(&(objectCategory=person)(objectClass=user)(sAMAccountName=a\\29\\28b))");
    }
}
