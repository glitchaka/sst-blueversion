use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::security::SecurityTriageService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

pub struct TriageBuiltin {
    service: Arc<SecurityTriageService>,
}

impl TriageBuiltin {
    pub fn new(service: Arc<SecurityTriageService>) -> Self {
        Self { service }
    }
}

impl BuiltinCommand for TriageBuiltin {
    fn name(&self) -> &'static str {
        "triage"
    }

    fn help(&self) -> &'static str {
        "triage — resumen del preload y señales que merecen revisión"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        _args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        self.service.triage()
    }
}

pub struct IntelBuiltin {
    service: Arc<SecurityTriageService>,
}

impl IntelBuiltin {
    pub fn new(service: Arc<SecurityTriageService>) -> Self {
        Self { service }
    }
}

impl BuiltinCommand for IntelBuiltin {
    fn name(&self) -> &'static str {
        "intel"
    }

    fn help(&self) -> &'static str {
        "intel — estado, fuentes y lookup de inteligencia de seguridad"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        self.service.intel(args)
    }
}
