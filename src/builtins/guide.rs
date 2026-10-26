include!("wiki.rs");

const G_SCREEN: &str = "\x1b[44;37m";
const G_PANEL: &str = "\x1b[47;30m";
const G_PANEL_BOLD: &str = "\x1b[47;30;1m";
const G_TITLE: &str = "\x1b[44;93;1m";
const G_RED: &str = "\x1b[47;31;1m";
const G_SELECTED: &str = "\x1b[44;97;1m";
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
                entry.summary = "Guía interactiva de comandos y manuales completos de SST.".to_owned();
                entry.use_case = "Descubrir qué comandos existen, abrir su referencia rápida local y, cuando hay conectividad, cargar el manual operativo completo mantenido en adm-toolbox.".to_owned();
                entry.syntax = "guide\nguide COMANDO\nguide search TEXTO".to_owned();
                entry.origin = "SST nativo + manuales SST remotos.".to_owned();
                entry.compatibility = "La ficha local funciona sin Internet. Los manuales extensos se obtienen por HTTPS en memoria y no se guardan como archivos locales.".to_owned();
            } else if entry.name.eq_ignore_ascii_case("help") {
                entry.use_case = "Consultar sintaxis corta del intérprete; para documentación contextual y manuales extensos usa guide.".to_owned();
            }
        }

        entries.extend([
            entry(
                "overview",
                &[],
                "Tutoriales",
                "Mapa completo de capacidades públicas de SST.",
                "Entender rápidamente qué familias de herramientas ofrece SST y dónde está documentada cada una.",
                "guide overview",
                "Manual SST remoto.",
                "Se carga por HTTPS en memoria; fallback local si la red no está disponible.",
            ),
            entry(
                "shell-invocation",
                &[],
                "Tutoriales",
                "Opciones de arranque de sst.exe/Nwash.",
                "Consultar -c, -i, -l, -s, -o/+o, -O/+O, rcfiles y flags de shell.",
                "guide shell-invocation",
                "Manual SST remoto.",
                "Documenta la interfaz pública de invocación, no helpers internos de GUI/broker.",
            ),
            entry(
                "sys-domain",
                &[],
                "Tutoriales",
                "Administración local de dominio: status, join y leave.",
                "Unir o sacar el equipo de Active Directory y usar OU explícita.",
                "guide sys-domain",
                "SST nativo sobre NetJoinDomain/NetUnjoinDomain.",
                "No confundir con `domain`, que es la consulta local/remota de pertenencia.",
            ),
            entry(
                "office-sara",
                &[],
                "Tutoriales",
                "Desinstalación profunda de Office mediante SaRACmd legacy.",
                "Usar OfficeScrubScenario, interpretar códigos y auditar residuos.",
                "guide office-sara",
                "SST Tools + SaRACmd Enterprise legacy firmado por Microsoft.",
                "Incluye flujo de recuperación y comparación con GetHelpCmd.",
            ),
            entry(
                "office-gethelp",
                &[],
                "Tutoriales",
                "Desinstalación profunda de Office mediante GetHelpCmd.",
                "Usar el backend actual de Microsoft y compararlo con SaRA legacy.",
                "guide office-gethelp",
                "SST Tools + GetHelpCmd de Microsoft.",
                "Incluye versiones, códigos y auditoría posterior.",
            ),
            entry(
                "windows-admin",
                &[],
                "Tutoriales",
                "Manual conjunto de administración Windows/Nwash.",
                "Consultar todas las opciones de eventlog, service, registry, process, acl, pnp, task, session, share, firewall y power.",
                "guide windows-admin",
                "Wrappers/implementaciones SST sobre utilidades nativas de Windows.",
                "Las mutaciones pueden requerir sudo/elevación.",
            ),
        ]);

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
        // Preserve the text requested by the user instead of immediately replacing it
        // with the canonical entry name. This lets `guide hostname` resolve the `sys`
        // entry locally but stream hostname.txt remotely.
        let mut detail_name = open_detail
            .then(|| find_entry(&entries, initial_query).map(|_| initial_query.to_owned()))
            .flatten();
        let mut detail_scroll = 0usize;

        loop {
            if let Some(topic) = detail_name.clone() {
                let Some(entry) = find_entry(&entries, &topic) else {
                    detail_name = None;
                    continue;
                };
                let remote = super::guide_remote::load(&topic)
                    .or_else(|| super::guide_remote::load(&entry.name));
                let max_scroll = render_detail(
                    terminal.as_mut(),
                    entry,
                    &topic,
                    remote.as_deref(),
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
        "guide — guía interactiva de comandos y manuales completos de SST"
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

    terminal.clear()?;
    let mut out = String::new();
    out.push_str(G_SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top_padding {
        out.push('\n');
    }

    push_window(&mut out, tw, ww, &title_border(ww, "Shell Shock Tool - Command Guide"));

    let intro = "Select a command and press ENTER. SST will show the full streamed manual when available; otherwise it falls back to the local reference. Press / to search and C to change category.";
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
        "  │{}{:command_width$}  {:description_width$}{}│",
        G_PANEL_BOLD,
        "Command",
        "Quick reference - what this command does",
        G_PANEL,
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
            let row_text = format!(
                "{:command_width$}  {:description_width$}",
                name,
                description,
                command_width = command_width,
                description_width = description_width,
            );
            if index == selected {
                format!("  │{G_SELECTED}{row_text}{G_PANEL}│")
            } else {
                format!("  │{row_text}│")
            }
        } else {
            format!("  │{}│", " ".repeat(menu_inner))
        };
        push_window(&mut out, tw, ww, &outer_ansi_line(ww, &content));
    }

    push_window(&mut out, tw, ww, &inner_bottom(ww));
    push_window(&mut out, tw, ww, &outer_separator(ww));
    let buttons = format!(
        "{}[ Select ]{}    {}X{} Exit Guide    {}C{} Category    / Search",
        G_SELECTED, G_PANEL, G_RED, G_PANEL, G_PANEL_BOLD, G_PANEL,
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
    topic: &str,
    remote: Option<&str>,
    scroll: usize,
) -> Result<usize> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(40));
    let th = usize::from(th.max(18));
    let ww = window_width(tw);
    let outer_inner = ww.saturating_sub(2);
    let body_width = outer_inner.saturating_sub(6);
    let rows = th.saturating_sub(9).clamp(8, 20);

    let body = if let Some(text) = remote {
        remote_lines(text, body_width)
    } else {
        local_detail_lines(entry, body_width)
    };

    let max_scroll = body.len().saturating_sub(rows);
    let scroll = scroll.min(max_scroll);
    let top_padding = th.saturating_sub(rows + 8) / 2;

    terminal.clear()?;
    let mut out = String::new();
    out.push_str(G_SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top_padding {
        out.push('\n');
    }

    push_window(
        &mut out,
        tw,
        ww,
        &title_border(ww, &format!("SST Guide - {topic}")),
    );
    let source = if remote.is_some() {
        "  Full SST manual · HTTPS/in-memory · Up/Down or PgUp/PgDn to scroll."
    } else {
        "  Local reference (remote manual unavailable) · Up/Down or PgUp/PgDn to scroll."
    };
    push_window(&mut out, tw, ww, &outer_line(ww, source));
    push_window(&mut out, tw, ww, &inner_top(ww));

    for line in body.iter().skip(scroll).take(rows) {
        let content = format!(
            "  │ {:<body_width$} │",
            clip(line, body_width),
            body_width = body_width
        );
        push_window(&mut out, tw, ww, &outer_ansi_line(ww, &content));
    }
    for _ in body.len().saturating_sub(scroll).min(rows)..rows {
        let content = format!("  │ {:<body_width$} │", "", body_width = body_width);
        push_window(&mut out, tw, ww, &outer_ansi_line(ww, &content));
    }

    push_window(&mut out, tw, ww, &inner_bottom(ww));
    push_window(&mut out, tw, ww, &outer_separator(ww));
    let buttons = format!(
        "{}[ Back ]{}    {}X{} Exit Guide    PgUp/PgDn Scroll",
        G_SELECTED, G_PANEL, G_RED, G_PANEL
    );
    push_window(&mut out, tw, ww, &centered_ansi_line(ww, &buttons));
    push_window(&mut out, tw, ww, &bottom_border(ww));
    out.push_str(G_RESET);

    terminal.write(&out)?;
    terminal.flush()?;
    Ok(max_scroll)
}

fn local_detail_lines(entry: &WikiEntry, body_width: usize) -> Vec<String> {
    let aliases = if entry.aliases.is_empty() {
        "-".to_owned()
    } else {
        entry.aliases.join(", ")
    };
    let mut body = Vec::new();
    detail_section(&mut body, "Command", &entry.name, body_width);
    detail_section(&mut body, "Category", &entry.category, body_width);
    detail_section(&mut body, "Aliases", &aliases, body_width);
    detail_section(&mut body, "What it does", &entry.summary, body_width);
    detail_section(&mut body, "When to use it", &entry.use_case, body_width);
    detail_section(&mut body, "Syntax", &entry.syntax.replace('\n', "  |  "), body_width);
    detail_section(&mut body, "Origin / inheritance", &entry.origin, body_width);
    detail_section(&mut body, "Compatibility / notes", &entry.compatibility, body_width);
    body
}

fn remote_lines(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let indent = raw.chars().take_while(|ch| ch.is_whitespace()).count().min(8);
        let prefix = " ".repeat(indent);
        let available = width.saturating_sub(indent).max(8);
        let trimmed = raw.trim();
        let wrapped = wrap(trimmed, available);
        for line in wrapped {
            out.push(format!("{prefix}{line}"));
        }
    }
    if out.is_empty() {
        out.push("(manual vacío)".to_owned());
    }
    out
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
    format!(
        "┌{}{}{}{}{}┐",
        "─".repeat(left),
        G_TITLE,
        title,
        G_PANEL,
        "─".repeat(right)
    )
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
    outer_line(
        width,
        &format!("  ┌{}┐", "─".repeat(menu_width.saturating_sub(2))),
    )
}

fn inner_separator(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    outer_line(
        width,
        &format!("  ├{}┤", "─".repeat(menu_width.saturating_sub(2))),
    )
}

fn inner_bottom(width: usize) -> String {
    let menu_width = width.saturating_sub(6);
    outer_line(
        width,
        &format!("  └{}┘", "─".repeat(menu_width.saturating_sub(2))),
    )
}
