use anyhow::Result;

use crate::{
    adapters::persistence::AppPaths,
    application::update,
    core::{CommandContext, CommandOutput},
};

use super::{tools::ToolsBuiltin, BuiltinCommand};

pub struct UpdateBuiltin {
    paths: AppPaths,
}

impl UpdateBuiltin {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    fn usage() -> &'static str {
        "update [status|check|install]\n\
         update status   muestra versión y estado del canal\n\
         update check    consulta si existe una versión nueva\n\
         update install  descarga/verifica y la aplica al cerrar SST\n"
    }
}

impl BuiltinCommand for UpdateBuiltin {
    fn name(&self) -> &'static str {
        "update"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["tools", "tool", "utils"]
    }

    fn help(&self) -> &'static str {
        "Comprueba, prepara y aplica actualizaciones oficiales de SST."
    }

    fn execute(
        &self,
        invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if !invoked_name.eq_ignore_ascii_case("update") {
            return ToolsBuiltin.execute(invoked_name, args, context);
        }

        match args.first().map(String::as_str) {
            None | Some("status") => Ok(CommandOutput::ok(update::status(&self.paths)?)),
            Some("check") if args.len() == 1 => {
                Ok(CommandOutput::ok(update::check_now(&self.paths, false)?))
            }
            Some("install") if args.len() == 1 => {
                Ok(CommandOutput::ok(update::check_now(&self.paths, true)?))
            }
            Some("help") | Some("--help") | Some("-h") => {
                Ok(CommandOutput::ok(Self::usage()))
            }
            _ => Ok(CommandOutput::error(Self::usage(), 2)),
        }
    }
}
