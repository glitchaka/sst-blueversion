include!("wiki.rs");

const GUIDE_SCREEN: &str = "\x1b[44;37m";
const GUIDE_PANEL: &str = "\x1b[47;30m";
const GUIDE_YELLOW: &str = "\x1b[47;93m";
const GUIDE_RED: &str = "\x1b[47;91m";
const GUIDE_SELECTED: &str = "\x1b[44;97m";
const GUIDE_RESET: &str = "\x1b[0m";

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

    fn run_ui(&self, initial_query: &str, open_detail: bool) -> Result<CommandOutput> {
        let entries = Self::entries();
        let mut terminal = self.terminal.alternate_screen()?;
        let mut query = initial_query.to_owned();
        let mut search_mode = false;
        let mut category_index = 0usize;
        let mut selected = 0usize;
        let mut detail_name = if open_detail {
            find_entry(&entries, initial_query).map(|entry| entry.name.clone())
        } else {
            None
        };
        let mut detail_scroll = 0usize;

        loop {
            if let Some(name) = detail_name.clone() {
                let Some(entry) = find_entry(&entries, &name) else {
                    detail_name = None;
                    continue;
                };
                let max_scroll = render_guide_detail(
                    terminal.as_mut(),
                    entry,
                    detail_scroll,
                )?;
                detail_scroll = detail_scroll.min(max_scroll);

                let Some(key) = terminal.poll_key(Duration::from_millis(200))? else {
                    continue;
                };
                match key {
                    TerminalKey::Escape | TerminalKey::Enter | TerminalKey::Backspace => {
                        detail_name = None;
                        detail_scroll = 0;
                    }
                    TerminalKey::Char('x') | TerminalKey::Char('X')
                    | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
                    TerminalKey::Up => detail_scroll = detail_scroll.saturating_sub(1),
                    TerminalKey::Down => detail_scroll = (detail_scroll + 1).min(max_scroll),
                    TerminalKey::PageUp => detail_scroll = detail_scroll.saturating_sub(5),
                    TerminalKey::PageDown => detail_scroll = (detail_scroll + 5).min(max_scroll),
                    TerminalKey::Home => detail_scroll = 0,
                    TerminalKey::End => detail_scroll = max_scroll,
                    _ => {}
                }
                continue;
            }

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

            render_guide_menu(
                terminal.as_mut(),
                &visible,
                selected,
                category,
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
                TerminalKey::Enter => {
                    if let Some(entry) = visible.get(selected).copied() {
                        detail_name = Some(entry.name.clone());
                        detail_scroll = 0;
                    }
                }
                TerminalKey::Escape
                | TerminalKey::Char('x') | TerminalKey::Char('X')
                | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
                TerminalKey::Up => selected = selected.saturating_sub(1),
                TerminalKey::Down => {
                    if selected + 1 < visible.len() {
                        selected += 1;
                    }
                }
                TerminalKey::PageUp => selected = selected.saturating_sub(8),
                TerminalKey::PageDown => {
                    if !visible.is_empty() {
                        selected = (selected + 8).min(visible.len() - 1);
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
                TerminalKey::Char(ch) if ch.is_ascii_alphanumeric() && !visible.is_empty() => {
                    let needle = ch.to_ascii_lowercase();
                    for offset in 1..=visible.len() {
                        let index = (selected + offset) % visible.len();
                        if visible[index]
                            .name
                            .to_ascii_lowercase()
                            .starts_with(needle)
                        {
                            selected = index;
                            break;
                        }
                    }
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
            None | Some("--help") | Some("-h") => self.run_ui("", false),
            Some("search") => {
                let query = args.get(1..).unwrap_or_default().join(" ");
                self.run_ui(&query, false)
            }
            Some(name) => self.run_ui(name, true),
        }
    }
}

fn render_guide_menu(
    terminal: &mut dyn TerminalSession,
    entries: &[&WikiEntry],
    selected: usize,
    category: &str,
    query: &str,
    search_mode: bool,
) -> Result<()> {
    let (terminal_width, terminal_height) = terminal.size()?;
    let terminal_width = usize::from(terminal_width.max(40));
    let terminal_height = usize::from(terminal_height.max(18));
    let window_width = guide_window_width(terminal_width);
    let outer_inner = window_width.saturating_sub(2);
    let menu_width = window_width.saturating_sub(6);
    let menu_inner = menu_width.saturating_sub(2);
    let command_width = 18usize.min(menu_inner.saturating_sub(18));
    let description_width = menu_inner.saturating_sub(command_width + 2);
    let list_rows = terminal_height.saturating_sub(14).clamp(5, 14);
    let start = if selected >= list_rows {
        selected + 1 - list_rows
    } else {
        0
    };
    let end = (start + list_rows).min(entries.len());
    let window_lines = list_rows + 13;
    let top_padding = terminal_height.saturating_sub(window_lines) / 2;

    terminal.clear()?;
    let mut out = String::new();
    out.push_str(GUIDE_SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top_padding {
        out.push('\n');
    }

    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_title_border(window_width, "Shell Shock Tool - Command Guide"),
    );

    let intro = "Welcome to the Shell Shock Tool command guide. Select a command below using the arrow keys or type the first character of its name. Press [ENTER] to open its documentation; press [X] to exit.";
    let mut intro_lines = wrap(intro, outer_inner.saturating_sub(4));
    intro_lines.truncate(4);
    while intro_lines.len() < 4 {
        intro_lines.push(String::new());
    }
    for line in intro_lines {
        guide_push_window_line(
            &mut out,
            terminal_width,
            window_width,
            &guide_outer_line(window_width, &format!("  {line}")),
        );
    }

    let filter = format!(
        "Category: {}    Search: {}{}",
        category,
        if search_mode { "> " } else { "" },
        if query.is_empty() { "(none)" } else { query }
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_outer_line(window_width, &filter),
    );

    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_inner_top(window_width),
    );

    let header = format!(
        "  │{}{:command_width$}{}  {:description_width$}│",
        GUIDE_YELLOW,
        "Command",
        GUIDE_PANEL,
        "Quick reference - what this command does",
        command_width = command_width,
        description_width = description_width,
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_outer_line_ansi(window_width, &header),
    );

    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_inner_separator(window_width),
    );

    for row in 0..list_rows {
        let index = start + row;
        let content = if index < end {
            let entry = entries[index];
            let name = clip(&entry.name, command_width);
            let description = clip(&entry.summary, description_width);
            let name_cell = if index == selected {
                format!(
                    "{}{:command_width$}{}",
                    GUIDE_SELECTED,
                    name,
                    GUIDE_PANEL,
                    command_width = command_width,
                )
            } else {
                format!(
                    "{}{:command_width$}{}",
                    GUIDE_YELLOW,
                    name,
                    GUIDE_PANEL,
                    command_width = command_width,
                )
            };
            format!(
                "  │{name_cell}  {:description_width$}│",
                description,
                description_width = description_width,
            )
        } else {
            format!("  │{}│", " ".repeat(menu_inner))
        };
        guide_push_window_line(
            &mut out,
            terminal_width,
            window_width,
            &guide_outer_line_ansi(window_width, &content),
        );
    }

    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_inner_bottom(window_width),
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_outer_separator(window_width),
    );

    let buttons = format!(
        "{}[ Select ]{}    {}X{} Exit Guide    {}C{} Category    / Search",
        GUIDE_SELECTED,
        GUIDE_PANEL,
        GUIDE_RED,
        GUIDE_PANEL,
        GUIDE_YELLOW,
        GUIDE_PANEL,
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_centered_outer_line_ansi(window_width, &buttons, 53),
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_bottom_border(window_width),
    );
    out.push_str(GUIDE_RESET);

    terminal.write(&out)?;
    terminal.flush()
}

fn render_guide_detail(
    terminal: &mut dyn TerminalSession,
    entry: &WikiEntry,
    scroll: usize,
) -> Result<usize> {
    let (terminal_width, terminal_height) = terminal.size()?;
    let terminal_width = usize::from(terminal_width.max(40));
    let terminal_height = usize::from(terminal_height.max(18));
    let window_width = guide_window_width(terminal_width);
    let outer_inner = window_width.saturating_sub(2);
    let body_width = outer_inner.saturating_sub(6);
    let body_rows = terminal_height.saturating_sub(9).clamp(8, 20);

    let aliases = if entry.aliases.is_empty() {
        "-".to_owned()
    } else {
        entry.aliases.join(", ")
    };
    let mut body = Vec::new();
    guide_detail_section(&mut body, "Command", &entry.name, body_width);
    guide_detail_section(&mut body, "Category", &entry.category, body_width);
    guide_detail_section(&mut body, "Aliases", &aliases, body_width);
    guide_detail_section(&mut body, "What it does", &entry.summary, body_width);
    guide_detail_section(&mut body, "When to use it", &entry.use_case, body_width);
    guide_detail_section(&mut body, "Syntax", &entry.syntax.replace('\n', "  |  "), body_width);
    guide_detail_section(&mut body, "Origin / inheritance", &entry.origin, body_width);
    guide_detail_section(&mut body, "Compatibility / notes", &entry.compatibility, body_width);

    let max_scroll = body.len().saturating_sub(body_rows);
    let scroll = scroll.min(max_scroll);
    let window_lines = body_rows + 8;
    let top_padding = terminal_height.saturating_sub(window_lines) / 2;

    terminal.clear()?;
    let mut out = String::new();
    out.push_str(GUIDE_SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top_padding {
        out.push('\n');
    }

    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_title_border(window_width, &format!("SST Guide - {}", entry.name)),
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_outer_line(window_width, "  Command reference. Use Up/Down or PgUp/PgDn to scroll."),
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_inner_top(window_width),
    );

    for line in body.iter().skip(scroll).take(body_rows) {
        let content = format!("  │ {:<width$} │", clip(line, body_width), width = body_width);
        guide_push_window_line(
            &mut out,
            terminal_width,
            window_width,
            &guide_outer_line_ansi(window_width, &content),
        );
    }
    for _ in body.len().saturating_sub(scroll).min(body_rows)..body_rows {
        let content = format!("  │ {:<width$} │", "", width = body_width);
        guide_push_window_line(
            &mut out,
            terminal_width,
            window_width,
            &guide_outer_line_ansi(window_width, &content),
        );
    }

    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_inner_bottom(window_width),
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_outer_separator(window_width),
    );
    let buttons = format!(
        "{}[ Back ]{}    {}X{} Exit Guide    PgUp/PgDn Scroll",
        GUIDE_SELECTED,
        GUIDE_PANEL,
        GUIDE_RED,
        GUIDE_PANEL,
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_centered_outer_line_ansi(window_width, &buttons, 46),
    );
    guide_push_window_line(
        &mut out,
        terminal_width,
        window_width,
        &guide_bottom_border(window_width),
    );
    out.push_str(GUIDE_RESET);

    terminal.write(&out)?;
    terminal.flush()?;
    Ok(max_scroll)
}

fn guide_detail_section(lines: &mut Vec<String>, label: &str, value: &str, width: usize) {
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(format!("{label}:"));
    for line in wrap(value, width.saturating_sub(2)) {
        lines.push(format!("  {line}"));
    }
}

fn guide_window_width(terminal_width: usize) -> usize {
    if terminal_width >= 68 {
        terminal_width.saturating_sub(4).min(96)
    } else {
        terminal_width.saturating_sub(2).max(38)
    }
}

fn guide_push_window_line(
    out: &mut String,
    terminal_width: usize,
    window_width: usize,
    line: &str,
) {
    let margin = terminal_width.saturating_sub(window_width) / 2;
    out.push_str(GUIDE_SCREEN);
    out.push_str(&" ".repeat(margin));
    out.push_str(GUIDE_PANEL);
    out.push_str(line);
    out.push_str(GUIDE_SCREEN);
    out.push('\n');
}

fn guide_title_border(width: usize, title: &str) -> String {
    let inner = width.saturating_sub(2);
    let title = format!(" {title} ");
    let title_width = title.chars().count().min(inner);
    let title = clip(&title, inner);
    let remaining = inner.saturating_sub(title_width);
    let left = remaining / 2;
    let right = remaining.saturating_sub(left);
    format!(
        "┌{}{}{}{}{}┐",
        "─".repeat(left),
        GUIDE_YELLOW,
        title,
        GUIDE_PANEL,
        "─".repeat(right),
    )
}

fn guide_bottom_border(width: usize) -> String {
    format!("└{}┘", "─".repeat(width.saturating_sub(2)))
}

fn guide_outer_separator(width: usize) -> String {
    format!("├{}┤", "─".repeat(width.saturating_sub(2)))
}

fn guide_outer_line(width: usize, text: &str) -> String {
    let inner = width.saturating_sub(2);
    let text = clip(text, inner);
    format!("│{:<inner$}│", text, inner = inner)
}

fn guide_outer_line_ansi(width: usize, content: &str) -> String {
    format!("│{content}{}│", "  ".repeat(0)).replace("││", &format!("│{}│", " ".repeat(width.saturating_sub(2))))
}

fn guide_inner_top(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    guide_outer_line(width, &format!("  ┌{}┐", "─".repeat(menu_width.saturating_sub(2))))
}

fn guide_inner_separator(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    guide_outer_line(width, &format!("  ├{}┤", "─".repeat(menu_width.saturating_sub(2))))
}

fn guide_inner_bottom(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    guide_outer_line(width, &format!("  └{}┘", "─".repeat(menu_width.saturating_sub(2))))
}

fn guide_centered_outer_line_ansi(width: usize, text: &str, visible_width: usize) -> String {
    let inner = width.saturating_sub(2);
    let visible_width = visible_width.min(inner);
    let left = inner.saturating_sub(visible_width) / 2;
    let right = inner.saturating_sub(visible_width + left);
    format!("│{}{}{}│", " ".repeat(left), text, " ".repeat(right))
}
