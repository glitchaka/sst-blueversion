use std::sync::Arc;

use anyhow::{Result, bail};

use crate::{
    application::system::SystemService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

const ALIASES: &[&str] = &[
    "ps", "top", "df", "free", "hostname", "autologon", "whoami", "uname", "kill", "uptime",
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
        "sys — sistema y administración local: procesos, servicios, usuarios, dominio, autologon, eventos, registro y tareas"
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
            if args.first().is_some_and(|arg| arg.eq_ignore_ascii_case("domain")) {
                return domain_tool(&args[1..]);
            }
            if args.first().is_some_and(|arg| arg.eq_ignore_ascii_case("autologon")) {
                return autologon_tool(&args[1..]);
            }
            self.service.execute(args)
        } else if invoked_name.eq_ignore_ascii_case("hostname") && !args.is_empty() {
            hostname_set(args)
        } else if invoked_name.eq_ignore_ascii_case("autologon") {
            autologon_tool(args)
        } else {
            self.service.execute_alias(invoked_name, args)
        }
    }
}

fn option_value<'a>(args: &'a [String], short: &str, long: &str) -> Result<Option<&'a str>> {
    let mut index = 0usize;
    while index < args.len() {
        if args[index] == short || args[index] == long {
            let Some(value) = args.get(index + 1) else {
                bail!("falta valor para {}", args[index]);
            };
            return Ok(Some(value.as_str()));
        }
        index += 1;
    }
    Ok(None)
}

fn autologon_tool(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys autologon -u CUENTA -d DOMINIO -p PASSWORD\n\
             sys autologon enable -u CUENTA -d DOMINIO -p PASSWORD\n\n\
             Configura inicio automático con Microsoft Sysinternals Autologon sin abrir GUI.\n\
             SST descarga el paquete oficial, valida la firma Microsoft y ejecuta Autologon\n\
             elevado con /accepteula. La contraseña indicada con -p queda en el historial\n\
             del comando; no se guarda en archivos de SST.\n",
        ));
    }

    let values = if args.first().is_some_and(|arg| arg.eq_ignore_ascii_case("enable")) {
        &args[1..]
    } else {
        args
    };

    let user = match option_value(values, "-u", "--user")? {
        Some(value) => value,
        None => return Ok(CommandOutput::error("autologon: falta -u/--user", 2)),
    };
    let domain = match option_value(values, "-d", "--domain")? {
        Some(value) => value,
        None => return Ok(CommandOutput::error("autologon: falta -d/--domain", 2)),
    };
    let password = match option_value(values, "-p", "--password")? {
        Some(value) => value,
        None => return Ok(CommandOutput::error("autologon: falta -p/--password", 2)),
    };

    crate::support::autologon::configure(user, domain, password)?;
    Ok(CommandOutput::ok(format!(
        "autologon: configurado para {domain}\\{user}; se aplicará en el próximo inicio de sesión.\n"
    )))
}

fn domain_tool(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys domain status\n\
             sys domain join -d DOMINIO -u CUENTA -p PASSWORD [-ou RUTA_OU]\n\
             sys domain leave -u CUENTA -p PASSWORD\n\n\
             join usa NetJoinDomain y crea la cuenta de equipo si es necesario.\n\
             leave usa NetUnjoinDomain. Ambas operaciones elevan por UAC cuando hace falta\n\
             y requieren reiniciar Windows para quedar efectivas.\n",
        ));
    }

    match args[0].to_ascii_lowercase().as_str() {
        "status" => {
            if args.len() != 1 {
                return Ok(CommandOutput::error("domain status: demasiados argumentos", 2));
            }
            Ok(CommandOutput::ok(format!("domain: {}\n", crate::support::domain::status()?)))
        }
        "join" => {
            let values = &args[1..];
            let domain = match option_value(values, "-d", "--domain")? {
                Some(value) => value,
                None => return Ok(CommandOutput::error("domain join: falta -d/--domain", 2)),
            };
            let user = match option_value(values, "-u", "--user")? {
                Some(value) => value,
                None => return Ok(CommandOutput::error("domain join: falta -u/--user", 2)),
            };
            let password = match option_value(values, "-p", "--password")? {
                Some(value) => value,
                None => return Ok(CommandOutput::error("domain join: falta -p/--password", 2)),
            };
            let ou = option_value(values, "-ou", "--ou")?;
            crate::support::domain::join(domain, user, password, ou)?;
            Ok(CommandOutput::ok(format!(
                "domain: unión a {domain} registrada correctamente; reinicio requerido.\n"
            )))
        }
        "leave" => {
            let values = &args[1..];
            let user = match option_value(values, "-u", "--user")? {
                Some(value) => value,
                None => return Ok(CommandOutput::error("domain leave: falta -u/--user", 2)),
            };
            let password = match option_value(values, "-p", "--password")? {
                Some(value) => value,
                None => return Ok(CommandOutput::error("domain leave: falta -p/--password", 2)),
            };
            crate::support::domain::leave(user, password)?;
            Ok(CommandOutput::ok(
                "domain: salida del dominio registrada correctamente; reinicio requerido.\n",
            ))
        }
        "__elevated" => {
            if args.len() != 2 {
                return Ok(CommandOutput::error("domain: payload elevado inválido", 87));
            }
            Ok(match crate::support::domain::run_elevated_payload(&args[1]) {
                Ok(()) => CommandOutput::ok("domain: operación elevada completada.\n"),
                Err(code) => CommandOutput::error(
                    format!("domain: operación elevada falló (NetAPI {code})"),
                    code.min(i32::MAX as u32) as i32,
                ),
            })
        }
        other => Ok(CommandOutput::error(
            format!("domain: acción desconocida: {other}; use status, join o leave"),
            2,
        )),
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
