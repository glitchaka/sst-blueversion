//! Separate interactive package UI. Uses the SAME PackageService as instp.
//! This is not an alternate installation engine.
use std::sync::Arc;
use anyhow::Result;
use crate::{
    application::packages::PackageService,
    core::{
        CommandContext, CommandOutput,
        ports::TerminalFactory,
    },
};
use super::{BuiltinCommand, sigsour_ui};

pub struct SigSourBuiltin {
    service: Arc<PackageService>,
    terminal: Arc<dyn TerminalFactory>,
}

impl SigSourBuiltin {
    pub fn new(service: Arc<PackageService>, terminal: Arc<dyn TerminalFactory>) -> Self {
        Self { service, terminal }
    }
}

impl BuiltinCommand for SigSourBuiltin {
    fn name(&self) -> &'static str { "sigsour" }

    fn help(&self) -> &'static str {
        "sigsour — catálogo interactivo de paquetes; utiliza el motor instp"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if !args.is_empty() {
            return Ok(CommandOutput::error("Uso: sigsour", 2));
        }
        sigsour_ui::run_catalog(self.service.as_ref(), self.terminal.as_ref())
    }
}
