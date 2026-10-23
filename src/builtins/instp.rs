use std::sync::Arc;

use anyhow::{Context, Result};

use crate::{
    application::packages::PackageService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

pub struct InstpBuiltin {
    service: Arc<PackageService>,
}

impl InstpBuiltin {
    pub fn new(service: Arc<PackageService>) -> Self {
        Self { service }
    }

    fn usage() -> &'static str {
        "Uso:\n\
         instp search [TEXTO]\n\
         instp info PAQUETE\n\
         instp install PAQUETE\n\
         instp remove PAQUETE\n\
         instp list\n\
         instp update\n\
         instp which COMANDO\n\
         instp repo list\n\
         instp repo add NOMBRE URL [PRIORIDAD]\n\
         instp repo remove NOMBRE\n\
         instp repo update\n\
         instp cache clean\n\
         instp doctor\n"
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
        let Some(id) = args.first() else {
            return Ok(CommandOutput::error("instp install: falta PAQUETE", 2));
        };

        match self.service.install(id) {
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
                     carpeta: {}\n\
                     comandos: {}\n\
                     fuente: {}\n",
                    report.package.name,
                    report.package.version,
                    report.package.portability.to_ascii_uppercase(),
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
                "{:<18} {:<20} {:<10} {}\n",
                package.id,
                package.version,
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

impl BuiltinCommand for InstpBuiltin {
    fn name(&self) -> &'static str {
        "instp"
    }

    fn help(&self) -> &'static str {
        "instp — gestor de paquetes portables y aislados de SST"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        match args.first().map(String::as_str) {
            None | Some("help") | Some("--help") | Some("-h") => {
                Ok(CommandOutput::ok(Self::usage()))
            }
            Some("search") => self.search(args.get(1..).unwrap_or_default()),
            Some("info") | Some("show") => self.info(args.get(1..).unwrap_or_default()),
            Some("install") | Some("add") => self.install(args.get(1..).unwrap_or_default()),
            Some("remove") | Some("rm") | Some("uninstall") => {
                self.remove(args.get(1..).unwrap_or_default())
            }
            Some("list") | Some("ls") => self.list(),
            Some("update") => self.update(),
            Some("which") => self.which(args.get(1..).unwrap_or_default()),
            Some("repo") => self.repo(args.get(1..).unwrap_or_default()),
            Some("cache") => self.cache(args.get(1..).unwrap_or_default()),
            Some("doctor") => self.doctor(),
            Some(other) => Ok(CommandOutput::error(
                format!("instp: subcomando desconocido: {other}\n{}", Self::usage()),
                2,
            )),
        }
    }
}
