//! Native Active Directory account unlock over Windows LDAP (Wldap32).
//! No PowerShell module, shell commands, passwords in argv, or unauthenticated binds.
//! Uses the current elevated SST token via SSPI Negotiate with LDAP sealing.
use anyhow::Result;
use crate::core::{CommandContext, CommandOutput};
use super::BuiltinCommand;

pub struct UnbanBuiltin;

impl BuiltinCommand for UnbanBuiltin {
    fn name(&self) -> &'static str { "unban" }
    fn help(&self) -> &'static str {
        "unban USUARIO — desbloquea una cuenta de Active Directory mediante LDAP nativo. \
         Requiere SST elevado con sudo y permisos delegados de desbloqueo en AD. \
         Nunca recibe contraseñas como argumentos."
    }
    fn execute(&self, _invoked_name: &str, args: &[String], _context: CommandContext<'_>)
        -> Result<CommandOutput>
    {
        if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "help") {
            return Ok(CommandOutput::ok(self.help()));
        }
        // Check the token *before* any domain access; never trust a --sudo flag.
        #[cfg(windows)]
        if !super::sudo::is_sudo_active()? {
            return Ok(CommandOutput::error(
                "unban: SST no está en modo sudo (token elevado). \
                 Ejecuta sudo para abrir SST con privilegios administrativos.", 5));
        }
        #[cfg(not(windows))]
        return Ok(CommandOutput::error("unban: requiere SST en Windows unido a AD", 1));

        #[cfg(windows)]
        {
            if args.len() != 1 {
                return Ok(CommandOutput::error("Uso: unban [nombre de usuario]", 2));
            }
            let user = args[0].trim();
            if let Err(message) = validate_account(user) {
                return Ok(CommandOutput::error(format!("unban: {message}"), 2));
            }
            let result = crate::adapters::windows::ad_unlock::unlock(user);
            // Log outcome without password, DN or command arguments.
            let stage = if result.is_ok() { "success" } else { "failure" };
            let audit = super::sudo_audit::AuditEvent::new(
                "unban", &[], std::env::var("USERNAME").unwrap_or_default(), "ADMIN");
            audit.record(stage, Some(if result.is_ok() {0} else {1}))?;
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
