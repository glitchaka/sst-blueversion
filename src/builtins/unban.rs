//! Native Active Directory account unlock over Windows LDAP (Wldap32).
//! No PowerShell module, shell commands, passwords in argv, or unauthenticated binds.
//! Solicits separate Active Directory credentials in a masked Windows dialog.
use anyhow::Result;
use crate::core::{CommandContext, CommandOutput};
use super::BuiltinCommand;

pub struct UnbanBuiltin;

impl BuiltinCommand for UnbanBuiltin {
    fn name(&self) -> &'static str { "unban" }
    fn help(&self) -> &'static str {
        "unban USUARIO — desbloquea una cuenta de Active Directory mediante LDAP nativo. \
         Requiere elevación mediante sudo o una sesión ya elevada; solicita credenciales del dominio y exige permisos de desbloqueo en AD. \
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
            if args.len() != 1 {
                return Ok(CommandOutput::error("Uso: unban [nombre de usuario]", 2));
            }
            if !super::sudo::is_elevated()? {
                return Ok(CommandOutput::error("unban: requiere elevación; usa sudo unban USUARIO", 5));
            }
            let user = args[0].trim();
            if let Err(message) = validate_account(user) {
                return Ok(CommandOutput::error(format!("unban: {message}"), 2));
            }
            let result = crate::adapters::windows::ad_unlock::unlock(user);
            // Independent AD operation audit, never mixed with sudo/UAC logs.
            if let Err(error) = audit_unban(user, result.is_ok()) {
                eprintln!("unban: no se pudo registrar la auditoría: {error:#}");
            }
            match result {
                Ok(dc) => Ok(CommandOutput::ok(format!(
                    "AD: usuario '{user}' desbloqueado correctamente.\n\
                     Controlador de dominio: {dc}\n"
                ))),
                Err(error) => Ok(CommandOutput::error(
                    format!("unban: no se pudo desbloquear '{user}': {error:#}"), 1)),
            }
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
