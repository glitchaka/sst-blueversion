include!("wiki.rs");

const G_SCREEN: &str = "\x1b[44;37m";
const G_PANEL: &str = "\x1b[47;30m";
const G_YELLOW: &str = "\x1b[47;93m";
const G_RED: &str = "\x1b[47;91m";
const G_SELECTED: &str = "\x1b[44;97m";
const G_RESET: &str = "\x1b[0m";

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
        let mut detail_name = open_detail
            .then(|| find_entry(&entries, initial_query).map(|entry| entry.name.clone()))
            .flatten();
        let mut detail_scroll = 0usize;

        loop {
            if let Some(name) = detail_name.clone() {
                let Some(entry) = find_entry(&entries, &name) else {
                    detail_name = None;
                    continue;
                };
                let max_scroll = render_detail(terminal.as_mut(), entry, detail_scroll)?;
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

            render_menu(
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
                        if visible[index].name.to_ascii_lowercase().starts_with(needle) {
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
            Some("search") => self.run_ui(&args.get(1..).unwrap_or_default().join(" "), false),
            Some(name) => self.run_ui(name, true),
        }
    }
}

fn render_menu(
    terminal: &mut dyn TerminalSession,
    entries: &[&WikiEntry],
    selected: usize,
    category: &str,
    query: &str,
    search_mode: bool,
) -> Result<()> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(40));
    let th = usize::from(th.max(18));
    let ww = window_width(tw);
    let outer_inner = ww.saturating_sub(2);
    let menu_width = ww.saturating_sub(6);
    let menu_inner = menu_width.saturating_sub(2);
    let command_width = 18usize.min(menu_inner.saturating_sub(18));
    let description_width = menu_inner.saturating_sub(command_width + 2);
    let rows = th.saturating_sub(14).clamp(5, 14);
    let start = selected.saturating_sub(rows.saturating_sub(1));
    let end = (start + rows).min(entries.len());
    let top_padding = th.saturating_sub(rows + 13) / 2;

    begin_screen(terminal, &mut String::new())?;
    let mut out = String::new();
    out.push_str(G_SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top_padding {
        out.push('\n');
    }

    push_window(&mut out, tw, ww, &title_border(ww, "Shell Shock Tool - Command Guide"));

    let intro = "Welcome to the Shell Shock Tool command guide. Select a command below using the arrow keys or type the first character of its name. Press [ENTER] to open its documentation; press [X] to exit.";
    let mut intro_lines = wrap(intro, outer_inner.saturating_sub(4));
    intro_lines.truncate(4);
    while intro_lines.len() < 4 {
        intro_lines.push(String::new());
    }
    for line in intro_lines {
        push_window(&mut out, tw, ww, &outer_line(ww, &format!("  {line}")));
    }

    let filter = format!(
        "Category: {}    Search: {}{}",
        category,
        if search_mode { "> " } else { "" },
        if query.is_empty() { "(none)" } else { query }
    );
    push_window(&mut out, tw, ww, &outer_line(ww, &filter));
    push_window(&mut out, tw, ww, &inner_top(ww));

    let header = format!(
        "  │{}{:command_width$}{}  {:description_width$}│",
        G_YELLOW,
        "Command",
        G_PANEL,
        "Quick reference - what this command does",
        command_width = command_width,
        description_width = description_width,
    );
    push_window(&mut out, tw, ww, &outer_ansi_line(ww, &header));
    push_window(&mut out, tw, ww, &inner_separator(ww));

    for row in 0..rows {
        let index = start + row;
        let content = if index < end {
            let entry = entries[index];
            let name = clip(&entry.name, command_width);
            let description = clip(&entry.summary, description_width);
            let name_cell = if index == selected {
                format!("{}{:command_width$}{}", G_SELECTED, name, G_PANEL, command_width = command_width)
            } else {
                format!("{}{:command_width$}{}", G_YELLOW, name, G_PANEL, command_width = command_width)
            };
            format!(
                "  │{name_cell}  {:description_width$}│",
                description,
                description_width = description_width,
            )
        } else {
            format!("  │{}│", " ".repeat(menu_inner))
        };
        push_window(&mut out, tw, ww, &outer_ansi_line(ww, &content));
    }

    push_window(&mut out, tw, ww, &inner_bottom(ww));
    push_window(&mut out, tw, ww, &outer_separator(ww));
    let buttons = format!(
        "{}[ Select ]{}    {}X{} Exit Guide    {}C{} Category    / Search",
        G_SELECTED, G_PANEL, G_RED, G_PANEL, G_YELLOW, G_PANEL,
    );
    push_window(&mut out, tw, ww, &centered_ansi_line(ww, &buttons));
    push_window(&mut out, tw, ww, &bottom_border(ww));
    out.push_str(G_RESET);

    terminal.write(&out)?;
    terminal.flush()
}

fn render_detail(
    terminal: &mut dyn TerminalSession,
    entry: &WikiEntry,
    scroll: usize,
) -> Result<usize> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(40));
    let th = usize::from(th.max(18));
    let ww = window_width(tw);
    let outer_inner = ww.saturating_sub(2);
    let body_width = outer_inner.saturating_sub(6);
    let rows = th.saturating_sub(9).clamp(8, 20);
    let aliases = if entry.aliases.is_empty() { "-".to_owned() } else { entry.aliases.join(", ") };

    let mut body = Vec::new();
    detail_section(&mut body, "Command", &entry.name, body_width);
    detail_section(&mut body, "Category", &entry.category, body_width);
    detail_section(&mut body, "Aliases", &aliases, body_width);
    detail_section(&mut body, "What it does", &entry.summary, body_width);
    detail_section(&mut body, "When to use it", &entry.use_case, body_width);
    detail_section(&mut body, "Syntax", &entry.syntax.replace('\n', "  |  "), body_width);
    detail_section(&mut body, "Origin / inheritance", &entry.origin, body_width);
    detail_section(&mut body, "Compatibility / notes", &entry.compatibility, body_width);

    let max_scroll = body.len().saturating_sub(rows);
    let scroll = scroll.min(max_scroll);
    let top_padding = th.saturating_sub(rows + 8) / 2;

    let mut out = String::new();
    out.push_str(G_SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top_padding {
        out.push('\n');
    }
    push_window(&mut out, tw, ww, &title_border(ww, &format!("SST Guide - {}", entry.name)));
    push_window(&mut out, tw, ww, &outer_line(ww, "  Command reference. Use Up/Down or PgUp/PgDn to scroll."));
    push_window(&mut out, tw, ww, &inner_top(ww));

    for line in body.iter().skip(scroll).take(rows) {
        let content = format!("  │ {:<body_width$} │", clip(line, body_width), body_width = body_width);
        push_window(&mut out, tw, ww, &outer_ansi_line(ww, &content));
    }
    for _ in body.len().saturating_sub(scroll).min(rows)..rows {
        let content = format!("  │ {:<body_width$} │", "", body_width = body_width);
        push_window(&mut out, tw, ww, &outer_ansi_line(ww, &content));
    }

    push_window(&mut out, tw, ww, &inner_bottom(ww));
    push_window(&mut out, tw, ww, &outer_separator(ww));
    let buttons = format!("{}[ Back ]{}    {}X{} Exit Guide    PgUp/PgDn Scroll", G_SELECTED, G_PANEL, G_RED, G_PANEL);
    push_window(&mut out, tw, ww, &centered_ansi_line(ww, &buttons));
    push_window(&mut out, tw, ww, &bottom_border(ww));
    out.push_str(G_RESET);

    terminal.write(&out)?;
    terminal.flush()?;
    Ok(max_scroll)
}

fn detail_section(lines: &mut Vec<String>, label: &str, value: &str, width: usize) {
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(format!("{label}:"));
    for line in wrap(value, width.saturating_sub(2)) {
        lines.push(format!("  {line}"));
    }
}

fn begin_screen(terminal: &mut dyn TerminalSession, _out: &mut String) -> Result<()> {
    terminal.clear()
}

fn window_width(terminal_width: usize) -> usize {
    if terminal_width >= 68 {
        terminal_width.saturating_sub(4).min(96)
    } else {
        terminal_width.saturating_sub(2).max(38)
    }
}

fn push_window(out: &mut String, terminal_width: usize, window_width: usize, line: &str) {
    let margin = terminal_width.saturating_sub(window_width) / 2;
    out.push_str(G_SCREEN);
    out.push_str(&" ".repeat(margin));
    out.push_str(G_PANEL);
    out.push_str(line);
    out.push_str(G_SCREEN);
    out.push('\n');
}

fn title_border(width: usize, title: &str) -> String {
    let inner = width.saturating_sub(2);
    let title = clip(&format!(" {title} "), inner);
    let remaining = inner.saturating_sub(title.chars().count());
    let left = remaining / 2;
    let right = remaining.saturating_sub(left);
    format!("┌{}{}{}{}{}┐", "─".repeat(left), G_YELLOW, title, G_PANEL, "─".repeat(right))
}

fn bottom_border(width: usize) -> String {
    format!("└{}┘", "─".repeat(width.saturating_sub(2)))
}

fn outer_separator(width: usize) -> String {
    format!("├{}┤", "─".repeat(width.saturating_sub(2)))
}

fn outer_line(width: usize, text: &str) -> String {
    let inner = width.saturating_sub(2);
    let text = clip(text, inner);
    format!("│{:<inner$}│", text, inner = inner)
}

fn outer_ansi_line(width: usize, content: &str) -> String {
    let inner = width.saturating_sub(2);
    let visible = ansi_visible_width(content).min(inner);
    format!("│{content}{}│", " ".repeat(inner.saturating_sub(visible)))
}

fn centered_ansi_line(width: usize, content: &str) -> String {
    let inner = width.saturating_sub(2);
    let visible = ansi_visible_width(content).min(inner);
    let left = inner.saturating_sub(visible) / 2;
    let right = inner.saturating_sub(visible + left);
    format!("│{}{}{}│", " ".repeat(left), content, " ".repeat(right))
}

fn ansi_visible_width(value: &str) -> usize {
    let mut count = 0usize;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek().is_some_and(|next| *next == '[') {
            chars.next();
            for code in chars.by_ref() {
                if code == 'm' {
                    break;
                }
            }
        } else {
            count += 1;
        }
    }
    count
}

fn inner_top(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    outer_line(width, &format!("  ┌{}┐", "─".repeat(menu_width.saturating_sub(2))))
}

fn inner_separator(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    outer_line(width, &format!("  ├{}┤", "─".repeat(menu_width.saturating_sub(2))))
}

fn inner_bottom(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    outer_line(width, &format!("  └{}┘", "─".repeat(menu_width.saturating_sub(2))))
}
