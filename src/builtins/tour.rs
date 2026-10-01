use std::{
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;

use crate::{
    adapters::persistence::AppPaths,
    core::{
        CommandContext,
        CommandOutput,
        ports::{TerminalFactory, TerminalKey},
    },
};

use super::BuiltinCommand;

pub struct TourBuiltin {
    paths: AppPaths,
    terminal: Arc<dyn TerminalFactory>,
}

impl TourBuiltin {
    pub fn new(paths: AppPaths, terminal: Arc<dyn TerminalFactory>) -> Self {
        Self { paths, terminal }
    }

    fn entries(&self) -> Result<Vec<TourEntry>> {
        let mut rows = std::fs::read_dir(self.paths.examples_dir())?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| value.eq_ignore_ascii_case("sh"))
            })
            .map(TourEntry::from_path)
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.file_name.to_ascii_lowercase().cmp(&b.file_name.to_ascii_lowercase()));
        Ok(rows)
    }

    fn render(entries: &[TourEntry], selected: usize, width: u16, height: u16) -> String {
        let width = width.max(50) as usize;
        let height = height.max(12) as usize;
        let visible = height.saturating_sub(7).max(1);

        let mut start = selected.saturating_sub(visible / 2);
        if start + visible > entries.len() {
            start = entries.len().saturating_sub(visible);
        }
        let end = (start + visible).min(entries.len());

        let mut out = String::new();
        out.push_str("SST TOUR\n");
        out.push_str(&"─".repeat(width.min(78)));
        out.push('\n');
        out.push_str("↑/↓ seleccionar · Enter ejecutar · Home/End · q/Esc salir\n\n");

        if entries.is_empty() {
            out.push_str("(no hay scripts en examples/)\n");
            return out;
        }

        for (index, entry) in entries.iter().enumerate().take(end).skip(start) {
            let marker = if index == selected { "▶" } else { " " };
            out.push_str(&format!(
                "{marker} {:<28} {}\n",
                entry.display_name,
                entry.description
            ));
        }

        out.push('\n');
        out.push_str(&format!(
            "{}/{} · {}\n",
            selected + 1,
            entries.len(),
            entries[selected].file_name
        ));
        out
    }
}

impl BuiltinCommand for TourBuiltin {
    fn name(&self) -> &'static str {
        "sst-tour-select"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["tour"]
    }

    fn hidden(&self) -> bool {
        true
    }

    fn help(&self) -> &'static str {
        "tour — menú interactivo para ejecutar los scripts de examples/"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        _args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        self.paths.ensure_layout()?;
        let entries = self.entries()?;
        if entries.is_empty() {
            return Ok(CommandOutput::error(
                format!(
                    "tour: no hay scripts en {}",
                    self.paths.examples_dir().display()
                ),
                1,
            ));
        }

        let mut terminal = self.terminal.alternate_screen()?;
        let mut selected = 0usize;

        loop {
            let (width, height) = terminal.size().unwrap_or((100, 30));
            terminal.clear()?;
            terminal.write(&Self::render(&entries, selected, width, height))?;
            terminal.flush()?;

            let Some(key) = terminal.poll_key(Duration::from_millis(200))? else {
                continue;
            };

            match key {
                TerminalKey::Up => {
                    selected = if selected == 0 {
                        entries.len() - 1
                    } else {
                        selected - 1
                    };
                }
                TerminalKey::Down => {
                    selected = (selected + 1) % entries.len();
                }
                TerminalKey::PageUp => {
                    selected = selected.saturating_sub(5);
                }
                TerminalKey::PageDown => {
                    selected = (selected + 5).min(entries.len() - 1);
                }
                TerminalKey::Home => selected = 0,
                TerminalKey::End => selected = entries.len() - 1,
                TerminalKey::Enter => {
                    // Nwash accepts Windows paths, but returning backslashes through a
                    // command substitution made the tour depend on shell quoting rules.
                    // Emit a shell-stable absolute path with forward slashes instead.
                    let path = entries[selected]
                        .path
                        .to_string_lossy()
                        .replace('\\', "/");
                    return Ok(CommandOutput::ok(format!("{path}\n")));
                }
                TerminalKey::Escape | TerminalKey::Char('q') | TerminalKey::Char('Q') => {
                    return Ok(CommandOutput::ok(""));
                }
                _ => {}
            }
        }
    }
}

struct TourEntry {
    path: PathBuf,
    file_name: String,
    display_name: String,
    description: &'static str,
}

impl TourEntry {
    fn from_path(path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("?")
            .to_owned();

        let (display_name, description) = match file_name.as_str() {
            "01-language-tour.sh" => ("01 · Lenguaje Nwash", "funciones, arrays, case, aritmética"),
            "02-windows-operator-report.sh" => ("02 · Operador Windows", "reporte de sistema y administración"),
            "03-network-discovery.sh" => ("03 · Descubrimiento de red", "scan, inventario, snapshots, diff"),
            "04-jobs-and-coproc.sh" => ("04 · Jobs y coprocesos", "background, wait, coproc, descriptores"),
            "05-pipelines-and-text.sh" => ("05 · Pipelines y texto", "grep, sed, cut, sort, tee, hashes"),
            "06-security-triage.sh" => ("06 · Security triage", "persistencia, firmas, intel, eventos"),
            "07-operator-console.sh" => ("07 · Operator console", "menú interactivo escrito en Nwash"),
            "08-config-and-backgrounds.sh" => ("08 · Config y fondos", "config bg y carrusel"),
            "09-advanced-bash.sh" => ("09 · Bash avanzado", "getopts, nameref, mapfile, printf -v"),
            "10-full-showcase.sh" => ("10 · Full showcase", "SST completo de punta a punta"),
            _ => ("Ejemplo SST", "script Nwash"),
        };

        Self {
            path,
            file_name,
            display_name: display_name.to_owned(),
            description,
        }
    }
}
