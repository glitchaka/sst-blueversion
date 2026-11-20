use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::system::SystemService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

const ALIASES: &[&str] = &[
    "ps", "top", "df", "free", "hostname", "autologon", "whoami", "uname", "kill", "uptime",
    "fetch", "neofetch", "fastfetch", "keygen",
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

#[derive(Default)]
struct AutologonOptions<'a> {
    user: Option<&'a str>,
    domain: Option<&'a str>,
    prompt_password: bool,
}

fn parse_autologon_options(args: &[String]) -> std::result::Result<AutologonOptions<'_>, CommandOutput> {
    let mut parsed = AutologonOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-p" | "--password" if !parsed.prompt_password => {
                parsed.prompt_password = true;
                index += 1;
            }
            "-u" | "--user" | "-d" | "--domain" => {
                let option = args[index].as_str();
                let Some(value) = args.get(index + 1) else {
                    return Err(CommandOutput::error("autologon: falta un valor de opción", 2));
                };
                if value.is_empty() || value.starts_with('-') {
                    return Err(CommandOutput::error("autologon: falta un valor de opción", 2));
                }
                let slot = if matches!(option, "-u" | "--user") {
                    &mut parsed.user
                } else {
                    &mut parsed.domain
                };
                if slot.is_some() {
                    return Err(CommandOutput::error("autologon: opción duplicada", 2));
                }
                *slot = Some(value);
                index += 2;
            }
            _ => return Err(CommandOutput::error(
                "autologon: opción no admitida; -p no acepta una contraseña en texto claro", 2,
            )),
        }
    }
    Ok(parsed)
}

fn autologon_tool(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || args.len() == 1 &&
        matches!(args[0].as_str(), "-h" | "--help" | "help")
    {
        return Ok(CommandOutput::ok(
            "sys autologon enable -u CUENTA -d DOMINIO -p\n\
             sys autologon disable\n\
             autologon -u CUENTA -d DOMINIO -p\n\n\
             -p / --password abre una ventana protegida de Windows.\n\
             Nunca escribas la contraseña en el comando.\n\
             SST solicita UAC antes de solicitar la contraseña y la almacena\n\
             como secreto LSA del sistema, no en la línea de comandos ni en\n\
             el valor DefaultPassword del registro.\n\
             AVISO: el inicio automático facilita el acceso físico no autorizado.\n",
        ));
    }
    if args.first().is_some_and(|arg| arg == "__elevated") {
        if args.len() != 2 {
            return Ok(CommandOutput::error("autologon: solicitud elevada inválida", 87));
        }
        return Ok(match crate::support::autologon::run_elevated_payload(&args[1]) {
            Ok(()) => CommandOutput::ok("autologon: operación protegida completada\n"),
            Err(code) => CommandOutput::error(format!(
                "autologon: operación elevada falló (Win32 {code})"
            ), code.min(i32::MAX as u32) as i32),
        });
    }
    if args.len() == 1 && args[0] == "disable" {
        crate::support::autologon::disable()?;
        return Ok(CommandOutput::ok("autologon: inicio automático deshabilitado\n"));
    }
    let values = if args.first().is_some_and(|arg| arg == "enable") {
        &args[1..]
    } else {
        args
    };
    let parsed = match parse_autologon_options(values) {
        Ok(parsed) => parsed,
        Err(output) => return Ok(output),
    };
    let Some(user) = parsed.user else {
        return Ok(CommandOutput::error("autologon: falta -u/--user", 2));
    };
    let Some(domain) = parsed.domain else {
        return Ok(CommandOutput::error("autologon: falta -d/--domain", 2));
    };
    if !parsed.prompt_password {
        return Ok(CommandOutput::error(
            "autologon: añade -p sin valor para abrir el diálogo seguro", 2,
        ));
    }
    crate::support::autologon::configure(user, domain)?;
    Ok(CommandOutput::ok(format!(
        "autologon: configurado para {domain}\\{user}; se aplicará en el próximo inicio de sesión.\n"
    )))
}

/// Domain passwords are deliberately not accepted as command arguments.
/// -p/--password means "request a protected Windows credential dialog".
/// Never interpolate an unknown argument in errors: it may be a legacy secret.
#[derive(Default)]
struct DomainOptions<'a> {
    domain: Option<&'a str>,
    account: Option<&'a str>,
    ou: Option<&'a str>,
    prompt_password: bool,
}

fn parse_domain_options(args: &[String]) -> std::result::Result<DomainOptions<'_>, CommandOutput> {
    let mut result = DomainOptions::default();
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if matches!(arg, "-p" | "--password") {
            if result.prompt_password {
                return Err(CommandOutput::error("domain: -p repetido", 2));
            }
            result.prompt_password = true;
            index += 1;
            continue;
        }
        let slot = match arg {
            "-d" | "--domain" => &mut result.domain,
            "-u" | "--user" => &mut result.account,
            "-ou" | "--ou" => &mut result.ou,
            _ => return Err(CommandOutput::error(
                "domain: argumento no admitido. No escribas la contraseña en el comando; usa -p sin valor.", 2
            )),
        };
        if slot.is_some() {
            return Err(CommandOutput::error("domain: opción duplicada", 2));
        }
        let Some(value) = args.get(index + 1).map(String::as_str) else {
            return Err(CommandOutput::error("domain: falta valor de una opción", 2));
        };
        if value.starts_with('-') || value.trim().is_empty() {
            return Err(CommandOutput::error("domain: falta valor de una opción", 2));
        }
        *slot = Some(value);
        index += 2;
    }
    Ok(result)
}

fn domain_tool(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys domain status\n\
             sys domain join -d DOMINIO -u CUENTA [-ou RUTA_OU] -p\n\
             sys domain leave -u CUENTA -p\n\n\
             -p / --password abre el diálogo protegido de credenciales de Windows.\n\
             La contraseña NO se introduce después de -p ni como argumento.\n\
             join usa NetJoinDomain; leave usa NetUnjoinDomain.\n\
             SST solicita UAC antes de recoger la contraseña si necesita elevación.\n\
             Es necesario reiniciar Windows para completar los cambios.\n",
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
            let values = match parse_domain_options(&args[1..]) {
                Ok(values) => values,
                Err(output) => return Ok(output),
            };
            let Some(domain) = values.domain else {
                return Ok(CommandOutput::error("domain join: falta -d/--domain", 2));
            };
            let Some(user) = values.account else {
                return Ok(CommandOutput::error("domain join: falta -u/--user", 2));
            };
            if !values.prompt_password {
                return Ok(CommandOutput::error(
                    "domain join: añade -p sin valor para solicitar la contraseña de forma protegida", 2,
                ));
            }
            crate::support::domain::join(domain, user, values.ou)?;
            Ok(CommandOutput::ok(format!(
                "domain: unión a {domain} registrada correctamente; reinicio requerido.\n"
            )))
        }
        "leave" => {
            let values = match parse_domain_options(&args[1..]) {
                Ok(values) => values,
                Err(output) => return Ok(output),
            };
            if values.domain.is_some() || values.ou.is_some() {
                return Ok(CommandOutput::error("domain leave: -d y -ou no son válidos", 2));
            }
            let Some(user) = values.account else {
                return Ok(CommandOutput::error("domain leave: falta -u/--user", 2));
            };
            if !values.prompt_password {
                return Ok(CommandOutput::error(
                    "domain leave: añade -p sin valor para solicitar la contraseña de forma protegida", 2,
                ));
            }
            crate::support::domain::leave(user)?;
            Ok(CommandOutput::ok(
                "domain: salida del dominio registrada correctamente; reinicio requerido.\n",
            ))
        }
        "__elevated" => {
            if args.len() != 2 {
                return Ok(CommandOutput::error("domain: solicitud elevada inválida", 87));
            }
            Ok(match crate::support::domain::run_elevated_payload(&args[1]) {
                Ok(()) => CommandOutput::ok("domain: operación elevada completada.\n"),
                Err(code) => CommandOutput::error(
                    if code == 1223 {
                        "domain: entrada protegida cancelada".to_owned()
                    } else {
                        format!("domain: operación elevada falló (NetAPI/Win32 {code})")
                    },
                    code.min(i32::MAX as u32) as i32,
                ),
            })
        }
        _ => Ok(CommandOutput::error(
            "domain: acción desconocida; usa status, join o leave", 2,
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

#[cfg(test)]
mod domain_credential_regression_tests {
    use super::parse_domain_options;

    #[test]
    fn password_flag_is_a_prompt_switch_not_an_argument() {
        let opts = ["-d", "empresa.cl", "-u", r"EMPRESA\admin", "-p"]
            .map(str::to_string);
        let parsed = parse_domain_options(&opts).unwrap();
        assert_eq!(parsed.domain, Some("empresa.cl"));
        assert_eq!(parsed.account, Some(r"EMPRESA\admin"));
        assert!(parsed.prompt_password);
    }

    #[test]
    fn rejects_inline_secrets_without_echoing_them_back() {
        let secret = "password-only-for-test-do-not-log";
        for args in [
            vec!["-u".into(), "admin".into(), "-p".into(), secret.into()],
            vec!["-u".into(), "admin".into(), format!("--password={secret}")],
        ] {
            let output = match parse_domain_options(&args) {
                Err(output) => output,
                Ok(_) => panic!("plaintext password accepted"),
            };
            assert_eq!(output.status, 2);
            assert!(!output.stdout.contains(secret));
            assert!(!output.stderr.contains(secret));
        }
    }
}

#[cfg(test)]
mod autologon_secret_tests {
    use super::parse_autologon_options;

    #[test]
    fn secured_password_flag_never_accepts_inline_values() {
        let args = ["-u", "operator", "-d", "empresa.cl", "-p"].map(str::to_owned);
        let parsed = parse_autologon_options(&args).unwrap();
        assert!(parsed.prompt_password);
        assert_eq!(parsed.user, Some("operator"));
        assert_eq!(parsed.domain, Some("empresa.cl"));
    }

    #[test]
    fn legacy_inline_password_is_rejected_without_reflection() {
        let secret = "SST_TEST_NOT_REAL_PW_Q7";
        let opts = ["-u", "operator", "-d", "empresa.cl", "-p", secret]
            .map(str::to_owned);
        let output = match parse_autologon_options(&opts) {
            Ok(_) => panic!("plaintext argument accepted"),
            Err(output) => output,
        };
        assert_eq!(output.status, 2);
        assert!(!output.stderr.contains(secret));
    }
}
