//! Native Active Directory account unlock over Windows LDAP (Wldap32).
//! No PowerShell module, shell commands, passwords in argv, or unauthenticated binds.
//! Solicits Active Directory credentials in the integrated SST terminal, without local elevation.
use anyhow::Result;
use crate::core::{CommandContext, CommandOutput};
use super::BuiltinCommand;

pub struct UnbanBuiltin { terminal: std::sync::Arc<dyn crate::core::ports::TerminalFactory> }
impl UnbanBuiltin {
    pub fn new(terminal: std::sync::Arc<dyn crate::core::ports::TerminalFactory>) -> Self { Self { terminal } }
}

impl BuiltinCommand for UnbanBuiltin {
    fn name(&self) -> &'static str { "unban" }
    fn help(&self) -> &'static str {
        "unban USUARIO — desbloquea una cuenta de Active Directory mediante LDAP nativo. \
         No requiere sudo ni elevación local. Solicita una sola credencial de AD; el controlador exige permisos de desbloqueo. \
         Nunca recibe contraseñas como argumentos."
    }
    fn execute(&self, _invoked_name: &str, args: &[String], _context: CommandContext<'_>)
        -> Result<CommandOutput>
    {
        if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "help") {
            return Ok(CommandOutput::ok(self.help()));
        }
        #[cfg(not(windows))]
        return Ok(CommandOutput::error("unban: requiere Windows y acceso al dominio AD", 1));

        #[cfg(windows)]
        {
            self.execute_domain_authorized(args)
        }
    }
}

#[cfg(windows)]
impl UnbanBuiltin {
    // No local privilege change: the domain controller enforces its own ACL.
    fn execute_domain_authorized(&self,args:&[String])->Result<CommandOutput>{
        if args.len()!=1{
            return Ok(CommandOutput::error("Uso: unban USUARIO",2));
        }
        let user=args[0].trim();
        if let Err(message)=validate_account(user){
            return Ok(CommandOutput::error(format!("unban: {message}"),2));
        }
        let result=crate::adapters::windows::ad_unlock::unlock(user,&*self.terminal);
        if let Err(error)=audit_unban(user,result.is_ok()){
            eprintln!("unban: no se pudo registrar la auditoría: {error:#}");
        }
        match result{
            Ok((dc,changed))=>Ok(CommandOutput::ok(format!(
                "ACTIVE DIRECTORY\nEstado: {}\nUsuario: {user}\nControlador: {dc}\nEvidencia: lockoutTime=0 (verificado en el controlador)\n",
                if changed{"Cuenta desbloqueada"}else{"Cuenta sin bloqueo pendiente"}
            ))),
            Err(error)=>Ok(CommandOutput::error(format!(
                "ACTIVE DIRECTORY\nEstado: ERROR\nUsuario: {user}\nError: {error:#}\n"),1)),
        }
    }
}

#[cfg(windows)]
fn audit_unban(account: &str, success: bool) -> Result<()> {
    use std::{fs::{self, OpenOptions}, io::Write};
    let path = crate::adapters::persistence::AppPaths::detect()
        .data_dir().join("ad-unban-audit.jsonl");
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let event = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "event": "ad_unban",
        "operator": std::env::var("USERNAME").unwrap_or_default(),
        "target_account": account,
        "result": if success { "success" } else { "failure" },
        "sst_version": env!("CARGO_PKG_VERSION"),
    });
    let mut handle = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(handle, "{}", event)?;
    handle.sync_data()?;
    Ok(())
}

#[cfg(windows)]
fn validate_account(user: &str) -> std::result::Result<(), &'static str> {
    if user.is_empty() || user.len() > 256 || user.chars().any(|c| c.is_control()
        || c == '/' || c == '\\' || c == '@')
    {
        return Err("indica exclusivamente el sAMAccountName (sin dominio, barras ni espacios de control)");
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::validate_account;
    #[test]
    fn rejects_ambiguous_or_dangerous_accounts() {
        for value in ["", "corp\\user", "user@corp.local", "bad\nuser", "a/b"] {
            assert!(validate_account(value).is_err());
        }
        assert!(validate_account("j.perez").is_ok());
    }
}
