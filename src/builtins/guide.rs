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
                &categories,
                category_index,
                &visible,
                selected,
                &query,
                search_mode,
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
    categories: &[String],
    category_index: usize,
    entries: &[&WikiEntry],
    selected: usize,
    query: &str,
    search_mode: bool,
) -> Result<()> {
    let (terminal_width, terminal_height) = terminal.size()?;
    let width = usize::from(terminal_width.max(72)).min(120);
    let height = usize::from(terminal_height.max(22));
    let inner = width.saturating_sub(2);
    let left_width = 20usize.min(inner.saturating_sub(36));
    let right_width = inner.saturating_sub(left_width + 1);
    let list_rows = height.saturating_sub(15).clamp(6, 14);

    let start = if selected >= list_rows {
        selected + 1 - list_rows
    } else {
        0
    };
    let end = (start + list_rows).min(entries.len());

    terminal.clear()?;
    let mut out = String::new();

    // Classic DOS/ncurses installer frame.
    out.push_str(&top_border(width, " SST COMMAND GUIDE "));
    out.push('\n');
    out.push_str(&boxed_line(
        width,
        &format!(
            "  Navegación: ↑↓   Categoría: C   Buscar: /   Salir: Esc/Q{}",
            if search_mode { "   [MODO BÚSQUEDA]" } else { "" }
        ),
    ));
    out.push('\n');
    out.push_str(&boxed_line(
        width,
        &format!(
            "  Filtro: {}{}",
            if search_mode { "> " } else { "" },
            if query.is_empty() { "(ninguno)" } else { query }
        ),
    ));
    out.push('\n');
    out.push_str(&middle_border(width));
    out.push('\n');

    out.push_str(&format!(
        "║{:<left$}│{:<right$}║\n",
        " CATEGORÍAS",
        " COMANDOS",
        left = left_width,
        right = right_width,
    ));
    out.push_str(&format!(
        "║{}┼{}║\n",
        "─".repeat(left_width),
        "─".repeat(right_width),
    ));

    for row in 0..list_rows {
        let category_cell = categories
            .get(row)
            .map(|value| {
                let label = clip(value, left_width.saturating_sub(2));
                if row == category_index {
                    format!("\x1b[7m {:<w$} \x1b[0m", label, w = left_width.saturating_sub(2))
                } else {
                    format!(" {:<w$} ", label, w = left_width.saturating_sub(2))
                }
            })
            .unwrap_or_else(|| " ".repeat(left_width));

        let command_cell = if let Some(index) = (start + row < end).then_some(start + row) {
            let entry = entries[index];
            let name_width = 17usize.min(right_width.saturating_sub(6));
            let summary_width = right_width.saturating_sub(name_width + 4);
            let plain = format!(
                " {:<namew$}  {}",
                clip(&entry.name, name_width),
                clip(&entry.summary, summary_width),
                namew = name_width,
            );
            let padded = format!("{:<w$}", clip(&plain, right_width), w = right_width);
            if index == selected {
                format!("\x1b[7m{padded}\x1b[0m")
            } else {
                padded
            }
        } else {
            " ".repeat(right_width)
        };

        out.push_str(&format!("║{category_cell}│{command_cell}║\n"));
    }

    out.push_str(&middle_border(width));
    out.push('\n');

    if let Some(entry) = entries.get(selected).copied() {
        let aliases = if entry.aliases.is_empty() {
            "-".to_owned()
        } else {
            entry.aliases.join(", ")
        };
        let detail_lines = [
            format!(" {}  [{}]  aliases: {}", entry.name, entry.category, aliases),
            format!(" Qué hace: {}", entry.summary),
            format!(" Cuándo usarlo: {}", entry.use_case),
            format!(" Sintaxis: {}", entry.syntax.replace('\n', "  |  ")),
            format!(" Origen: {}", entry.origin),
            format!(" Notas: {}", entry.compatibility),
        ];

        for line in detail_lines {
            for wrapped in wrap(&line, inner.saturating_sub(2)).into_iter().take(2) {
                out.push_str(&boxed_line(width, &wrapped));
                out.push('\n');
            }
        }
    } else {
        out.push_str(&boxed_line(width, "  No hay comandos que coincidan con el filtro."));
        out.push('\n');
        for _ in 0..5 {
            out.push_str(&boxed_line(width, ""));
            out.push('\n');
        }
    }

    out.push_str(&middle_border(width));
    out.push('\n');
    out.push_str(&boxed_line(
        width,
        "  [↑/↓] Seleccionar   [PgUp/PgDn] Página   [C] Categoría   [/] Buscar   [Esc/Q] Salir",
    ));
    out.push('\n');
    out.push_str(&bottom_border(width));
    out.push('\n');

    terminal.write(&out)?;
    terminal.flush()
}

fn top_border(width: usize, title: &str) -> String {
    let inner = width.saturating_sub(2);
    let title = clip(title, inner);
    let remaining = inner.saturating_sub(title.chars().count());
    let left = remaining / 2;
    let right = remaining.saturating_sub(left);
    format!("╔{}{}{}╗", "═".repeat(left), title, "═".repeat(right))
}

fn middle_border(width: usize) -> String {
    format!("╠{}╣", "═".repeat(width.saturating_sub(2)))
}

fn bottom_border(width: usize) -> String {
    format!("╚{}╝", "═".repeat(width.saturating_sub(2)))
}

fn boxed_line(width: usize, value: &str) -> String {
    let inner = width.saturating_sub(2);
    let clipped = clip(value, inner);
    format!("║{:<inner$}║", clipped, inner = inner)
}
