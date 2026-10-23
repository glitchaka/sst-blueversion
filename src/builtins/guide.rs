include!("wiki.rs");

pub struct GuideBuiltin {
    terminal: Arc<dyn TerminalFactory>,
}

impl GuideBuiltin {
    pub fn new(terminal: Arc<dyn TerminalFactory>) -> Self {
        Self { terminal }
    }

    fn entries() -> Vec<WikiEntry> {
        let mut entries = wiki_entries();
        for entry in &mut entries {
            if entry.name.eq_ignore_ascii_case("wiki") {
                entry.name = "guide".to_owned();
                entry.aliases.clear();
                entry.summary = "Guía interactiva local de comandos de SST.".to_owned();
                entry.use_case = "Descubrir qué comandos existen, cuándo se usan, su sintaxis y de qué herramienta heredan comportamiento.".to_owned();
                entry.syntax = "guide\nguide COMANDO\nguide search TEXTO".to_owned();
                entry.origin = "SST nativo.".to_owned();
                entry.compatibility = "No necesita Internet y documenta también compatibilidad Bash/Unix.".to_owned();
            } else if entry.name.eq_ignore_ascii_case("help") {
                entry.use_case = "Consultar sintaxis corta del intérprete; para documentación contextual usa guide.".to_owned();
            }
        }
        entries.sort_by(|left, right| {
            left.category
                .to_ascii_lowercase()
                .cmp(&right.category.to_ascii_lowercase())
                .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
        });
        entries
    }

    fn run_ui(&self, initial_query: &str) -> Result<CommandOutput> {
        let entries = Self::entries();
        let mut terminal = self.terminal.alternate_screen()?;
        let mut query = initial_query.to_owned();
        let mut search_mode = false;
        let mut category_index = 0usize;
        let mut selected = 0usize;

        loop {
            let categories = categories(&entries);
            if category_index >= categories.len() {
                category_index = 0;
            }
            let category = categories
                .get(category_index)
                .map(String::as_str)
                .unwrap_or("todos");
            let visible = visible_entries(&entries, &query, category);
            if visible.is_empty() {
                selected = 0;
            } else if selected >= visible.len() {
                selected = visible.len() - 1;
            }

            render_guide(
                terminal.as_mut(),
                &visible,
                selected,
                &query,
                search_mode,
                category,
            )?;

            let Some(key) = terminal.poll_key(Duration::from_millis(200))? else {
                continue;
            };

            if search_mode {
                match key {
                    TerminalKey::Escape | TerminalKey::Enter => search_mode = false,
                    TerminalKey::Backspace => {
                        query.pop();
                        selected = 0;
                    }
                    TerminalKey::Delete => {
                        query.clear();
                        selected = 0;
                    }
                    TerminalKey::Char(ch) if !ch.is_control() => {
                        query.push(ch);
                        selected = 0;
                    }
                    _ => {}
                }
                continue;
            }

            match key {
                TerminalKey::Escape | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
                TerminalKey::Up => selected = selected.saturating_sub(1),
                TerminalKey::Down => {
                    if selected + 1 < visible.len() {
                        selected += 1;
                    }
                }
                TerminalKey::PageUp => selected = selected.saturating_sub(10),
                TerminalKey::PageDown => {
                    if !visible.is_empty() {
                        selected = (selected + 10).min(visible.len() - 1);
                    }
                }
                TerminalKey::Home => selected = 0,
                TerminalKey::End => {
                    if !visible.is_empty() {
                        selected = visible.len() - 1;
                    }
                }
                TerminalKey::Char('/') => search_mode = true,
                TerminalKey::Char('c') | TerminalKey::Char('C') => {
                    category_index = (category_index + 1) % categories.len().max(1);
                    selected = 0;
                }
                _ => {}
            }
        }

        Ok(CommandOutput::ok(""))
    }
}

impl BuiltinCommand for GuideBuiltin {
    fn name(&self) -> &'static str {
        "guide"
    }

    fn help(&self) -> &'static str {
        "guide — guía interactiva local de todos los comandos de SST"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        match args.first().map(String::as_str) {
            None | Some("--help") | Some("-h") => self.run_ui(""),
            Some("search") => {
                let query = args.get(1..).unwrap_or_default().join(" ");
                self.run_ui(&query)
            }
            Some(name) => self.run_ui(name),
        }
    }
}

fn render_guide(
    terminal: &mut dyn TerminalSession,
    entries: &[&WikiEntry],
    selected: usize,
    query: &str,
    search_mode: bool,
    category: &str,
) -> Result<()> {
    let (width, height) = terminal.size()?;
    let width = usize::from(width.max(60));
    let height = usize::from(height.max(18));
    let list_rows = (height / 3).clamp(5, 12);
    let start = if selected >= list_rows {
        selected + 1 - list_rows
    } else {
        0
    };
    let end = (start + list_rows).min(entries.len());

    terminal.clear()?;
    let mut out = String::new();
    out.push_str("SST GUIDE · comandos de la consola\n");
    out.push_str(&format!(
        "Categoría: {:<14} Comandos: {:<3} Buscar: {}{}\n",
        category,
        entries.len(),
        if search_mode { "> " } else { "" },
        if query.is_empty() { "-" } else { query }
    ));
    out.push_str(&format!("{}\n", "─".repeat(width.min(120))));

    if entries.is_empty() {
        out.push_str("  No hay comandos que coincidan con el filtro.\n");
        for _ in 1..list_rows {
            out.push('\n');
        }
    } else {
        for index in start..end {
            let entry = entries[index];
            let marker = if index == selected { '>' } else { ' ' };
            let line = format!(
                "{} {:<18} {:<14} {}",
                marker,
                clip(&entry.name, 18),
                clip(&entry.category, 14),
                entry.summary
            );
            out.push_str(&clip(&line, width));
            out.push('\n');
        }
        for _ in end - start..list_rows {
            out.push('\n');
        }
    }

    out.push_str(&format!("{}\n", "─".repeat(width.min(120))));
    if let Some(entry) = entries.get(selected).copied() {
        let aliases = if entry.aliases.is_empty() {
            "-".to_owned()
        } else {
            entry.aliases.join(", ")
        };
        push_wrapped(&mut out, &format!("{}  [{}]  aliases: {}", entry.name, entry.category, aliases), width, 2);
        push_wrapped(&mut out, &format!("Qué hace: {}", entry.summary), width, 2);
        push_wrapped(&mut out, &format!("Cuándo: {}", entry.use_case), width, 2);
        let syntax = entry.syntax.replace('\n', "  ·  ");
        push_wrapped(&mut out, &format!("Sintaxis: {syntax}"), width, 3);
        push_wrapped(&mut out, &format!("Origen: {}", entry.origin), width, 2);
        push_wrapped(&mut out, &format!("Notas: {}", entry.compatibility), width, 2);
    } else {
        out.push_str("Sin selección.\n");
    }

    out.push_str(&format!("{}\n", "─".repeat(width.min(120))));
    out.push_str(&clip(
        "↑↓ navegar · PgUp/PgDn · / buscar · c categoría · q/Esc salir · guide COMANDO abre su ficha",
        width,
    ));
    out.push('\n');

    terminal.write(&out)?;
    terminal.flush()
}
