use std::sync::Arc;

use anyhow::{Context, Result};

use crate::{
    adapters::persistence::AppPaths,
    application::{packages::PackageService, update},
    core::{
        CommandContext, CommandOutput,
    },
};

use super::BuiltinCommand;

pub struct InstpBuiltin {
    service: Arc<PackageService>,
    paths: AppPaths,
}

impl InstpBuiltin {
    pub fn new(service: Arc<PackageService>, paths: AppPaths) -> Self {
        Self { service, paths }
    }

    fn usage() -> &'static str {
        "Uso:\n\
         instp                         muestra ayuda (CLI, no TUI)\n\
         instp PAQUETE                 consulta el estado sin instalar\n\
         instp search [TEXTO]\n\
         instp info PAQUETE\n\
         instp install PAQUETE [-p|--persistent]\n\
         instp remove PAQUETE\n\
         instp list\n\
         instp update                  actualiza SOLO índices de paquetes\n\
         instp sst                     estado de la instalación SST\n\
         instp check sst               comprueba versiones de SST\n\
         instp upgrade sst             descarga y verifica SST; aplica al cerrar\n\
         instp upgrade [PAQUETE]       obtiene la versión más reciente\n\
         instp which COMANDO\n\
         instp repo list\n\
         instp repo add NOMBRE URL [PRIORIDAD]\n\
         instp repo remove NOMBRE\n\
         instp repo update\n\
         instp cache clean\n\
         instp doctor\n\
         sigsour                         TUI independiente de paquetes\n"
    }

    fn search(&self, args: &[String]) -> Result<CommandOutput> {
        let term = args.first().map(String::as_str);
        let packages = self.service.search(term)?;
        if packages.is_empty() {
            return Ok(CommandOutput::ok("No se encontraron paquetes.\n"));
        }

        let mut out = String::new();
        for package in packages {
            out.push_str(&format!(
                "{:<18} {:<12} {:<10} {}\n",
                package.id,
                package.category,
                package.portability.to_ascii_uppercase(),
                package.description
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn info(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(id) = args.first() else {
            return Ok(CommandOutput::error("instp info: falta PAQUETE", 2));
        };
        if id.eq_ignore_ascii_case("sst") {
            return self.status(id);
        }
        let Some(package) = self.service.info(id)? else {
            return Ok(CommandOutput::error(
                format!("instp info: paquete no encontrado: {id}"),
                1,
            ));
        };

        let exposed = if package.install.expose.is_empty() {
            "(ninguno)".to_owned()
        } else {
            package.install.expose.join(", ")
        };
        let source = match package.source.kind.as_str() {
            "github_release" => package
                .source
                .repository
                .as_deref()
                .map(|repo| format!("GitHub Releases: {repo}"))
                .unwrap_or_else(|| "GitHub Releases".to_owned()),
            "url" => package
                .source
                .url
                .clone()
                .unwrap_or_else(|| "URL".to_owned()),
            other => other.to_owned(),
        };

        Ok(CommandOutput::ok(format!(
            "{} ({})\n\
             id: {}\n\
             categoría: {}\n\
             portabilidad: {}\n\
             fuente: {}\n\
             instalación: {}\n\
             expone: {}\n\
             {}\n",
            package.name,
            package.version,
            package.id,
            package.category,
            package.portability.to_ascii_uppercase(),
            source,
            package.install.kind,
            exposed,
            package.description
        )))
    }

    fn install(&self, args: &[String]) -> Result<CommandOutput> {
        let (id, persistent) = match parse_install_args(args) {
            Ok(parsed) => parsed,
            Err(error) => return Ok(CommandOutput::error(error, 2)),
        };

        if id.eq_ignore_ascii_case("sst") {
            return Ok(CommandOutput::error(
                "Para actualizar SST utiliza 'instp upgrade sst'; no se instala como paquete externo.", 2
            ));
        }

        let installation = if persistent {
            self.service.install_with_persistence(id, true)
        } else {
            self.service.install(id)
        };
        match installation {
            Ok(report) => {
                let commands = if report.package.commands.is_empty() {
                    "(ninguno)".to_owned()
                } else {
                    report
                        .package
                        .commands
                        .iter()
                        .map(|entry| entry.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                };

                let mut out = format!(
                    "Instalado: {} {}\n\
                     ámbito: SST solamente ({})\n\
                     retención: {}\n\
                     carpeta: {}\n\
                     comandos: {}\n\
                     fuente: {}\n",
                    report.package.name,
                    report.package.version,
                    report.package.portability.to_ascii_uppercase(),
                    if report.package.persistent { "PERSISTENTE" } else { "TEMPORAL (hasta próxima generación funcional)" },
                    report.package.install_dir,
                    commands,
                    report.source_url
                );

                if !report.missing_exposed_commands.is_empty() {
                    out.push_str(&format!(
                        "Aviso: no se encontraron en el paquete: {}\n",
                        report.missing_exposed_commands.join(", ")
                    ));
                }
                if !report.package.commands.is_empty() {
                    out.push_str(
                        "Reinicia SST para incorporar los nuevos comandos al PATH de esta sesión.\n",
                    );
                }
                Ok(CommandOutput::ok(out))
            }
            Err(error) => Ok(CommandOutput::error(format!("{error:#}"), 1)),
        }
    }

    fn remove(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(id) = args.first() else {
            return Ok(CommandOutput::error("instp remove: falta PAQUETE", 2));
        };
        if self.service.remove(id)? {
            Ok(CommandOutput::ok(format!(
                "Eliminado de SST: {id}\nReinicia SST para retirar sus rutas de la sesión actual.\n"
            )))
        } else {
            Ok(CommandOutput::error(
                format!("instp remove: {id} no está instalado"),
                1,
            ))
        }
    }

    fn list(&self) -> Result<CommandOutput> {
        let packages = self.service.installed()?;
        if packages.is_empty() {
            return Ok(CommandOutput::ok("No hay paquetes instalados por instp.\n"));
        }
        let mut out = String::new();
        for package in packages {
            out.push_str(&format!(
                "{:<18} {:<20} {:<12} {:<10} {}\n",
                package.id,
                package.version,
                if package.persistent { "PERSISTENTE" } else { "TEMPORAL" },
                package.portability.to_ascii_uppercase(),
                package.install_dir
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn update(&self) -> Result<CommandOutput> {
        match self.service.update_repositories() {
            Ok(updated) if updated.is_empty() => {
                Ok(CommandOutput::ok("No hay repositorios remotos habilitados.\n"))
            }
            Ok(updated) => Ok(CommandOutput::ok(format!(
                "Repositorios actualizados:\n  {}\n",
                updated.join("\n  ")
            ))),
            Err(error) => Ok(CommandOutput::error(format!("{error:#}"), 1)),
        }
    }

    fn status(&self, id: &str) -> Result<CommandOutput> {
        if id.eq_ignore_ascii_case("sst") {
            return Ok(match update::status(&self.paths) {
                Ok(message) => CommandOutput::ok(message),
                Err(error) => CommandOutput::error(format!("{error:#}"), 1),
            });
        }
        let installed = self.service.installed()?.into_iter()
            .find(|pkg| pkg.id.eq_ignore_ascii_case(id));
        let available = self.service.info(id)?;
        match (installed, available) {
            (Some(found), available) => {
                let version = available.map(|pkg| pkg.version)
                    .unwrap_or_else(|| "(ya no está en el catálogo)".to_owned());
                Ok(CommandOutput::ok(format!(
                    "{}: instalado\n  versión: {}\n  catálogo: {}\n  retención: {}\n",
                    found.id, found.version, version,
                    if found.persistent { "PERSISTENTE" } else { "TEMPORAL" }
                )))
            }
            (None, Some(pkg)) => Ok(CommandOutput::ok(format!(
                "{}: disponible, no instalado (versión {})\n", pkg.id, pkg.version
            ))),
            (None, None) => Ok(CommandOutput::error(
                format!("instp: paquete no encontrado: {id}"), 1
            )),
        }
    }

    fn upgrade(&self, args: &[String]) -> Result<CommandOutput> {
        if args.len() == 1 && args[0].eq_ignore_ascii_case("sst") {
            return Ok(match update::check_now(&self.paths, true) {
                Ok(message) => CommandOutput::ok(message),
                Err(error) => CommandOutput::error(format!("{error:#}"), 1),
            });
        }
        if args.len() > 1 {
            return Ok(CommandOutput::error("Uso: instp upgrade [PAQUETE]", 2));
        }
        match self.service.upgrade(args.first().map(String::as_str)) {
            Ok(rows) if rows.is_empty() => Ok(CommandOutput::ok("No hay paquetes instalados.\n")),
            Ok(rows) => Ok(CommandOutput::ok(format!("{}\n", rows.join("\n")))),
            Err(error) => Ok(CommandOutput::error(format!("{error:#}"), 1)),
        }
    }

    fn check_sst(&self) -> Result<CommandOutput> {
        Ok(match update::check_now(&self.paths, false) {
            Ok(message) => CommandOutput::ok(message),
            Err(error) => CommandOutput::error(format!("{error:#}"), 1),
        })
    }

    fn which(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(command) = args.first() else {
            return Ok(CommandOutput::error("instp which: falta COMANDO", 2));
        };
        match self.service.which(command)? {
            Some(path) => Ok(CommandOutput::ok(format!("{}\n", path.display()))),
            None => Ok(CommandOutput::error(
                format!("instp which: {command} no pertenece a un paquete instalado"),
                1,
            )),
        }
    }

    fn repo(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("list") {
            "list" => {
                let repos = self.service.repositories()?;
                let mut out = String::new();
                for repo in repos {
                    out.push_str(&format!(
                        "{:<16} prio={:<4} {:<8} {}\n",
                        repo.name,
                        repo.priority,
                        if repo.enabled { "enabled" } else { "disabled" },
                        repo.url
                    ));
                }
                Ok(CommandOutput::ok(out))
            }
            "add" => {
                let Some(name) = args.get(1) else {
                    return Ok(CommandOutput::error(
                        "instp repo add: falta NOMBRE",
                        2,
                    ));
                };
                let Some(url) = args.get(2) else {
                    return Ok(CommandOutput::error("instp repo add: falta URL", 2));
                };
                let priority = args
                    .get(3)
                    .map(|value| value.parse::<i32>())
                    .transpose()
                    .context("instp repo add: PRIORIDAD debe ser un entero")?
                    .unwrap_or(50);
                self.service.add_repository(name, url, priority)?;
                Ok(CommandOutput::ok(format!("Repositorio agregado: {name}\n")))
            }
            "remove" | "rm" => {
                let Some(name) = args.get(1) else {
                    return Ok(CommandOutput::error(
                        "instp repo remove: falta NOMBRE",
                        2,
                    ));
                };
                if self.service.remove_repository(name)? {
                    Ok(CommandOutput::ok(format!("Repositorio eliminado: {name}\n")))
                } else {
                    Ok(CommandOutput::error(
                        format!("instp repo remove: no existe {name}"),
                        1,
                    ))
                }
            }
            "update" => self.update(),
            other => Ok(CommandOutput::error(
                format!("instp repo: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    fn cache(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str) {
            Some("clean") => {
                let removed = self.service.clean_cache()?;
                Ok(CommandOutput::ok(format!(
                    "Cache de instp limpiada: {removed} archivo(s).\n"
                )))
            }
            _ => Ok(CommandOutput::error("Uso: instp cache clean", 2)),
        }
    }

    fn doctor(&self) -> Result<CommandOutput> {
        Ok(CommandOutput::ok(format!(
            "instp doctor\n  {}\n",
            self.service.doctor()?.join("\n  ")
        )))
    }
}

fn parse_install_args(args: &[String]) -> std::result::Result<(&str, bool), &'static str> {
    let mut package = None;
    let mut persistent = false;
    for arg in args {
        match arg.as_str() {
            "-p" | "--persistent" => persistent = true,
            value if value.starts_with('-') => return Err(
                "instp install: modificador desconocido (válidos: -p, --persistent)"
            ),
            name if package.is_none() => package = Some(name),
            _ => return Err("Uso: instp install PAQUETE [-p|--persistent]"),
        }
    }
    Ok((
        package.ok_or("Uso: instp install PAQUETE [-p|--persistent]")?,
        persistent
    ))
}

impl BuiltinCommand for InstpBuiltin {
    fn name(&self) -> &'static str {
        "instp"
    }

    fn help(&self) -> &'static str {
        "instp — gestor exclusivamente CLI de paquetes de SST"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        match args.first().map(String::as_str) {
            None => Ok(CommandOutput::ok(Self::usage())),
            Some("browse") | Some("catalog") => Ok(CommandOutput::error(
                "instp no abre una TUI; utiliza sigsour.", 2
            )),
            Some("help") | Some("--help") | Some("-h") => {
                Ok(CommandOutput::ok(Self::usage()))
            }
            Some("search") => self.search(args.get(1..).unwrap_or_default()),
            Some("info") | Some("show") => self.info(args.get(1..).unwrap_or_default()),
            Some("install") | Some("add") => self.install(args.get(1..).unwrap_or_default()),
            Some("remove") | Some("rm") | Some("uninstall") => {
                self.remove(args.get(1..).unwrap_or_default())
            }
            Some("list") | Some("ls") => self.list(),
            Some("update") if args.len() == 1 => self.update(),
            Some("upgrade") => self.upgrade(args.get(1..).unwrap_or_default()),
            Some("check") if args.len() == 2 && args[1].eq_ignore_ascii_case("sst") => {
                self.check_sst()
            }
            Some("which") => self.which(args.get(1..).unwrap_or_default()),
            Some("repo") => self.repo(args.get(1..).unwrap_or_default()),
            Some("cache") => self.cache(args.get(1..).unwrap_or_default()),
            Some("doctor") if args.len() == 1 => self.doctor(),
            Some(other) if args.len() == 1 && !other.starts_with('-') =>
                self.status(other),
            _ => Ok(CommandOutput::error(Self::usage(), 2)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_temporary_by_default() {
        let args = vec!["ripgrep".to_owned()];
        assert_eq!(parse_install_args(&args), Ok(("ripgrep", false)));
    }

    #[test]
    fn persistent_flag_accepted_before_or_after_name() {
        for args in [
            vec!["ripgrep".to_owned(), "-p".to_owned()],
            vec!["--persistent".to_owned(), "ripgrep".to_owned()],
        ] {
            assert_eq!(parse_install_args(&args), Ok(("ripgrep", true)));
        }
    }

    #[test]
    fn sst_updates_are_only_reached_through_instp() {
        let wiring = include_str!("../composition/mod.rs");
        assert!(!wiring.contains("UpdateBuiltin"));
        assert!(wiring.contains("InstpBuiltin::new"));
        assert!(InstpBuiltin::usage().contains("instp upgrade sst"));
    }

    #[test]
    fn invalid_install_flags_are_rejected() {
        assert!(parse_install_args(&["ripgrep".into(), "--force".into()]).is_err());
        assert!(parse_install_args(&["one".into(), "two".into()]).is_err());
        assert!(parse_install_args(&[]).is_err());
    }
}
