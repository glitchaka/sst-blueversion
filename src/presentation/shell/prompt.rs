use std::{env, path::Path};

use crate::support::path;
use crate::presentation::formatter::Role;

pub fn render(cwd: &Path) -> String {
    let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
    let host = env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".to_owned());
    let cwd = path::display(cwd);

    format!("{}  {}\n{}", Role::Label.paint(&format!("\u{f007} {user}@{host}")),
        Role::Detail.paint(&format!("\u{f07c} {cwd}")), input_marker())
}

pub fn banner() -> &'static str {
    "\x1b[1;38;5;117m\u{f120} Shell Shock Tool\x1b[0m\n\x1b[38;5;250mAdministración y diagnóstico en terreno · terminal portable\x1b[0m\nEscribe help para ver los comandos. Editor de scripts: helix archivo.sh · Guía: help helix\n"
}

/// The editable prompt and its first render share one semantic marker.
pub fn input_marker() -> String {
    format!("{} ", Role::Command.paint("$"))
}
