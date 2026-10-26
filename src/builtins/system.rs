use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::system::SystemService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

const ALIASES: &[&str] = &[
    "ps", "top", "df", "free", "hostname", "whoami", "uname", "kill", "uptime",
    "fetch", "neofetch", "fastfetch",
];

pub struct SystemBuiltin {
    service: Arc<SystemService>,
}

impl SystemBuiltin {
    pub fn new(service: Arc<SystemService>) -> Self {
        Self { service }
    }
}

impl BuiltinCommand for SystemBuiltin {
    fn name(&self) -> &'static str {
        "sys"
    }

    fn aliases(&self) -> &'static [&'static str] {
        ALIASES
    }

    fn help(&self) -> &'static str {
        "sys — sistema y administración local: procesos, servicios, usuarios, eventos, registro y tareas"
    }

    fn execute(
        &self,
        invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if invoked_name == "sys" {
            if args.first().is_some_and(|arg| arg.eq_ignore_ascii_case("hostname"))
                && args.len() > 1
            {
                return hostname_set(&args[1..]);
            }
            self.service.execute(args)
        } else if invoked_name.eq_ignore_ascii_case("hostname") && !args.is_empty() {
            hostname_set(args)
        } else {
            self.service.execute_alias(invoked_name, args)
        }
    }
}

fn hostname_set(args: &[String]) -> Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "hostname [NUEVO_NOMBRE]\n\
             sys hostname [NUEVO_NOMBRE]\n\n\
             Sin argumentos muestra el nombre actual. Con NUEVO_NOMBRE cambia el nombre\n\
             del equipo local. En equipos unidos a dominio también renombra la cuenta del\n\
             equipo en Active Directory usando el contexto administrativo actual.\n\
             El cambio queda efectivo después de reiniciar Windows.\n",
        ));
    }

    let elevated_internal = args.iter().any(|arg| arg == "--elevated-internal");
    let names = args
        .iter()
        .filter(|arg| arg.as_str() != "--elevated-internal")
        .collect::<Vec<_>>();

    if names.len() != 1 {
        return Ok(CommandOutput::error(
            "hostname: usa exactamente un nombre: hostname NUEVO_NOMBRE",
            2,
        ));
    }

    let new_name = names[0].trim();
    let old_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "<actual>".to_owned());
    if old_name.eq_ignore_ascii_case(new_name) {
        return Ok(CommandOutput::ok(format!(
            "hostname: el equipo ya se llama {old_name}\n"
        )));
    }

    if elevated_internal {
        return Ok(match crate::support::computer_name::rename_computer_current_token(new_name) {
            Ok(()) => CommandOutput::ok(format!(
                "hostname: {old_name} -> {new_name}; reinicio requerido\n"
            )),
            Err(code) => CommandOutput::error(
                format!("hostname: Windows no pudo renombrar el equipo (NetAPI/Win32 {code})"),
                code.min(i32::MAX as u32) as i32,
            ),
        });
    }

    crate::support::computer_name::rename_computer(new_name)?;
    Ok(CommandOutput::ok(format!(
        "hostname: {old_name} -> {new_name}\n\
         cambio registrado correctamente; reinicia Windows para completar el renombrado.\n"
    )))
}
