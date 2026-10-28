use std::{sync::Arc, time::Duration};

use anyhow::Result;

use crate::core::{
    CommandContext, CommandOutput,
    ports::{TerminalFactory, TerminalKey, TerminalSession},
};

use super::{BuiltinCommand, UNIX_COMMANDS};

const SCREEN: &str = "\x1b[44;37m";
const PANEL: &str = "\x1b[47;30m";
const RESET: &str = "\x1b[0m";

const CATEGORIES: &[(&str, &str)] = &[
    ("Sistema", "Equipo, procesos, memoria, discos, hostname, dominio y Autologon."),
    ("Red", "Interfaces, rutas, DNS, vecinos, scan, monitor, inventario, switches y WOL."),
    ("Diagnóstico", "Diagnósticos compuestos del equipo y la red."),
    ("Windows", "Event Log, servicios, Registro, procesos, ACL, PnP, tareas, sesiones, SMB, firewall y energía."),
    ("Seguridad", "Triage, inteligencia, persistencia e inspección de procesos."),
    ("Privilegios", "Administrador, LOCAL SYSTEM y TrustedInstaller."),
    ("Herramientas", "Framework de diagnóstico, reparación y remoción por producto/proveedor."),
    ("Paquetes", "Paquetes portables instalados dentro de SST."),
    ("Arch / Portabilidad", "Catálogo de Arch Linux y evaluación de qué proyectos pueden portarse a SST/Windows."),
    ("Configuración", "Configuración, fondos, rutas y recarga."),
    ("Actualización", "Comprobación, descarga y aplicación de versiones SST."),
    ("Editor", "Helix-SST."),
    ("Nwash / Bash", "Builtins Bash implementados por Nwash."),
    ("Unix", "Comandos Unix/GNU-like reimplementados en SST."),
];

const BASH_BUILTINS: &[&str] = &[
    ":", ".", "[", "alias", "bg", "bind", "break", "builtin", "caller", "cd",
    "command", "compgen", "complete", "compopt", "continue", "declare", "dirs",
    "disown", "echo", "enable", "eval", "exec", "exit", "export", "false", "fc",
    "fg", "getopts", "hash", "help", "history", "jobs", "kill", "let", "local",
    "logout", "mapfile", "popd", "printf", "pushd", "pwd", "read", "readarray",
    "readonly", "return", "set", "shift", "shopt", "source", "suspend", "test",
    "times", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias",
    "unset", "wait",
];

const SST_COMMANDS: &[(&str, &[&str], &str, &str)] = &[
    ("sys", &["ps", "top", "df", "free", "whoami", "uname", "uptime", "fetch", "neofetch", "fastfetch"], "Sistema", "Información y administración local del equipo."),
    ("hostname", &["sys hostname"], "Sistema", "Consulta o cambia el nombre del equipo."),
    ("sys domain", &["domain-admin"], "Sistema", "Estado, unión y salida de dominio Windows."),
    ("sys autologon", &["autologon"], "Sistema", "Inicio automático mediante Sysinternals Autologon."),
    ("notsh", &["net"], "Red", "Shell de red unificada de SST."),
    ("device", &[], "Red", "Inventario de equipos por MAC/IP."),
    ("switch", &[], "Red", "Localiza una MAC en switch/puerto."),
    ("wol", &[], "Red", "Wake-on-LAN."),
    ("diag", &[], "Diagnóstico", "Diagnósticos compuestos."),
    ("domain", &[], "Windows", "Consulta pertenencia a dominio local/remota."),
    ("eventlog", &[], "Windows", "Windows Event Log."),
    ("service", &[], "Windows", "Servicios Windows."),
    ("registry", &[], "Windows", "Registro de Windows."),
    ("process", &[], "Windows", "Procesos Windows."),
    ("acl", &[], "Windows", "Permisos ACL NTFS."),
    ("pnp", &[], "Windows", "Dispositivos Plug and Play."),
    ("task", &[], "Windows", "Tareas programadas."),
    ("session", &[], "Windows", "Sesiones locales/RDP."),
    ("share", &[], "Windows", "Recursos compartidos SMB."),
    ("firewall", &[], "Windows", "Windows Defender Firewall."),
    ("power", &[], "Windows", "Apagado, reinicio, logoff e hibernación."),
    ("triage", &[], "Seguridad", "Análisis de seguridad y procesos."),
    ("intel", &[], "Seguridad", "Fuentes y lookups de inteligencia."),
    ("sudo", &["runas"], "Privilegios", "Elevación de comandos y sesión."),
    ("tools", &["tool", "utils"], "Herramientas", "Framework de utilidades de mantenimiento por producto/proveedor."),
    ("tools office diagnose", &["tools office status", "tools office verify"], "Herramientas", "Audita Office, Outlook y la cadena de autenticación."),
    ("tools office auth repair", &[], "Herramientas", "Re-registra AAD BrokerPlugin."),
    ("tools office scrub", &["tools office sara", "tools office uninstall", "tools office remove"], "Herramientas", "Desinstalación completa con SaRACmd legacy."),
    ("tools office scrub-current", &["tools office gethelp"], "Herramientas", "Desinstalación con GetHelpCmd actual."),
    ("instp", &[], "Paquetes", "Gestor portable de paquetes SST."),
    ("arch packages", &["arch-packages", "arch portability", "arch portabilidad"], "Arch / Portabilidad", "Catálogo vivo de Arch y matriz de portabilidad a Windows/SST."),
    ("config", &[], "Configuración", "Configuración principal de SST."),
    ("reload", &[], "Configuración", "Recarga la configuración de la shell."),
    ("sst-path", &[], "Configuración", "Traduce rutas Unix/Windows para SST."),
    ("update", &[], "Actualización", "Busca y prepara actualizaciones SST."),
    ("helix", &["hx", "helix-sst"], "Editor", "Editor Helix integrado en SST."),
];

#[derive(Clone)]
struct Entry {
    name: String,
    aliases: Vec<String>,
    category: String,
    summary: String,
}

impl Entry {
    fn new(name: &str, aliases: &[&str], category: &str, summary: &str) -> Self {
        Self {
            name: name.to_owned(),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            category: category.to_owned(),
            summary: summary.to_owned(),
        }
    }
}

#[derive(Clone)]
enum View {
    Areas,
    Commands(usize),
    Search,
    Detail(String),
}

pub struct GuideBuiltin {
    terminal: Arc<dyn TerminalFactory>,
}

impl GuideBuiltin {
    pub fn new(terminal: Arc<dyn TerminalFactory>) -> Self {
        Self { terminal }
    }

    fn entries() -> Vec<Entry> {
        let mut rows = SST_COMMANDS
            .iter()
            .map(|(name, aliases, category, summary)| Entry::new(name, aliases, category, summary))
            .collect::<Vec<_>>();

        for name in BASH_BUILTINS {
            rows.push(Entry::new(
                name,
                &[],
                "Nwash / Bash",
                "Builtin Bash/Nwash; abre su referencia completa de opciones y comportamiento.",
            ));
        }

        for &(name, help) in UNIX_COMMANDS {
            if BASH_BUILTINS.contains(&name) {
                continue;
            }
            let summary = help
                .split_once('—')
                .map(|(_, value)| value.trim())
                .unwrap_or(help);
            rows.push(Entry::new(name, &[], "Unix", summary));
        }

        rows.sort_by(|left, right| {
            category_index(&left.category)
                .cmp(&category_index(&right.category))
                .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
        });
        rows
    }

    fn run(&self, direct: Option<String>, search: Option<String>) -> Result<CommandOutput> {
        let entries = Self::entries();
        let mut terminal = self.terminal.alternate_screen()?;
        let mut view = direct.map(View::Detail).unwrap_or_else(|| {
            if search.is_some() { View::Search } else { View::Areas }
        });
        let mut query = search.unwrap_or_default();
        let mut typing = !query.is_empty();
        let mut selected = 0usize;
        let mut scroll = 0usize;

        loop {
            match view.clone() {
                View::Areas => {
                    selected = selected.min(CATEGORIES.len().saturating_sub(1));
                    render_areas(terminal.as_mut(), selected)?;
                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else { continue };
                    match key {
                        TerminalKey::Enter => { view = View::Commands(selected); selected = 0; }
                        TerminalKey::Char('/') => { query.clear(); typing = true; selected = 0; view = View::Search; }
                        TerminalKey::Up => selected = selected.saturating_sub(1),
                        TerminalKey::Down => selected = (selected + 1).min(CATEGORIES.len().saturating_sub(1)),
                        TerminalKey::PageUp => selected = selected.saturating_sub(6),
                        TerminalKey::PageDown => selected = (selected + 6).min(CATEGORIES.len().saturating_sub(1)),
                        TerminalKey::Home => selected = 0,
                        TerminalKey::End => selected = CATEGORIES.len().saturating_sub(1),
                        TerminalKey::Escape | TerminalKey::Char('q') | TerminalKey::Char('Q')
                        | TerminalKey::Char('x') | TerminalKey::Char('X') => break,
                        _ => {}
                    }
                }
                View::Commands(area) => {
                    let area = area.min(CATEGORIES.len().saturating_sub(1));
                    let name = CATEGORIES[area].0;
                    let visible = entries.iter().filter(|entry| entry.category == name).collect::<Vec<_>>();
                    selected = selected.min(visible.len().saturating_sub(1));
                    render_commands(terminal.as_mut(), name, &visible, selected)?;
                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else { continue };
                    match key {
                        TerminalKey::Enter => {
                            if let Some(entry) = visible.get(selected) {
                                view = View::Detail(entry.name.clone());
                                scroll = 0;
                            }
                        }
                        TerminalKey::Backspace | TerminalKey::Escape => { selected = area; view = View::Areas; }
                        TerminalKey::Char('/') => { query.clear(); typing = true; selected = 0; view = View::Search; }
                        TerminalKey::Up => selected = selected.saturating_sub(1),
                        TerminalKey::Down => selected = (selected + 1).min(visible.len().saturating_sub(1)),
                        TerminalKey::PageUp => selected = selected.saturating_sub(8),
                        TerminalKey::PageDown => selected = (selected + 8).min(visible.len().saturating_sub(1)),
                        TerminalKey::Home => selected = 0,
                        TerminalKey::End => selected = visible.len().saturating_sub(1),
                        TerminalKey::Char('q') | TerminalKey::Char('Q')
                        | TerminalKey::Char('x') | TerminalKey::Char('X') => break,
                        _ => {}
                    }
                }
                View::Search => {
                    let visible = search_entries(&entries, &query);
                    selected = selected.min(visible.len().saturating_sub(1));
                    render_search(terminal.as_mut(), &visible, selected, &query, typing)?;
                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else { continue };
                    if typing {
                        match key {
                            TerminalKey::Enter => typing = false,
                            TerminalKey::Escape => { query.clear(); typing = false; selected = 0; view = View::Areas; }
                            TerminalKey::Backspace => { query.pop(); selected = 0; }
                            TerminalKey::Delete => { query.clear(); selected = 0; }
                            TerminalKey::Char(ch) if !ch.is_control() => { query.push(ch); selected = 0; }
                            _ => {}
                        }
                        continue;
                    }
                    match key {
                        TerminalKey::Enter => {
                            if let Some(entry) = visible.get(selected) {
                                view = View::Detail(entry.name.clone());
                                scroll = 0;
                            }
                        }
                        TerminalKey::Char('/') => typing = true,
                        TerminalKey::Backspace | TerminalKey::Escape => { query.clear(); selected = 0; view = View::Areas; }
                        TerminalKey::Up => selected = selected.saturating_sub(1),
                        TerminalKey::Down => selected = (selected + 1).min(visible.len().saturating_sub(1)),
                        TerminalKey::PageUp => selected = selected.saturating_sub(8),
                        TerminalKey::PageDown => selected = (selected + 8).min(visible.len().saturating_sub(1)),
                        TerminalKey::Home => selected = 0,
                        TerminalKey::End => selected = visible.len().saturating_sub(1),
                        TerminalKey::Char('q') | TerminalKey::Char('Q')
                        | TerminalKey::Char('x') | TerminalKey::Char('X') => break,
                        _ => {}
                    }
                }
                View::Detail(topic) => {
                    let Some(entry) = find_entry(&entries, &topic) else {
                        view = View::Areas;
                        selected = 0;
                        continue;
                    };
                    let manual = super::guide_remote::load(&entry.name)
                        .or_else(|| entry.aliases.iter().find_map(|alias| super::guide_remote::load(alias)));
                    let max_scroll = render_detail(terminal.as_mut(), entry, manual.as_deref(), scroll)?;
                    scroll = scroll.min(max_scroll);
                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else { continue };
                    match key {
                        TerminalKey::Backspace | TerminalKey::Enter | TerminalKey::Escape => {
                            let area = category_index(&entry.category).min(CATEGORIES.len().saturating_sub(1));
                            selected = entries
                                .iter()
                                .filter(|candidate| candidate.category == entry.category)
                                .position(|candidate| candidate.name == entry.name)
                                .unwrap_or(0);
                            scroll = 0;
                            view = View::Commands(area);
                        }
                        TerminalKey::Up => scroll = scroll.saturating_sub(1),
                        TerminalKey::Down => scroll = (scroll + 1).min(max_scroll),
                        TerminalKey::PageUp => scroll = scroll.saturating_sub(8),
                        TerminalKey::PageDown => scroll = (scroll + 8).min(max_scroll),
                        TerminalKey::Home => scroll = 0,
                        TerminalKey::End => scroll = max_scroll,
                        TerminalKey::Char('q') | TerminalKey::Char('Q')
                        | TerminalKey::Char('x') | TerminalKey::Char('X') => break,
                        _ => {}
                    }
                }
            }
        }

        Ok(CommandOutput::ok(""))
    }
}

impl BuiltinCommand for GuideBuiltin {
    fn name(&self) -> &'static str { "guide" }

    fn help(&self) -> &'static str { "guide — manual interactivo completo de SST" }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if args.first().map(String::as_str) == Some("search") {
            return self.run(None, Some(args.get(1..).unwrap_or_default().join(" ")));
        }
        if args.is_empty() || matches!(args.first().map(String::as_str), Some("-h") | Some("--help")) {
            return self.run(None, None);
        }
        self.run(Some(args.join(" ")), None)
    }
}

fn category_index(category: &str) -> usize {
    CATEGORIES
        .iter()
        .position(|(name, _)| name.eq_ignore_ascii_case(category))
        .unwrap_or(CATEGORIES.len())
}

fn find_entry<'a>(entries: &'a [Entry], value: &str) -> Option<&'a Entry> {
    entries.iter().find(|entry| entry.name.eq_ignore_ascii_case(value)).or_else(|| {
        entries.iter().find(|entry| entry.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(value)))
    })
}

fn search_entries<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let query = query.trim().to_ascii_lowercase();
    let mut rows = entries.iter().filter(|entry| {
        query.is_empty()
            || entry.name.to_ascii_lowercase().contains(&query)
            || entry.aliases.iter().any(|alias| alias.to_ascii_lowercase().contains(&query))
            || entry.category.to_ascii_lowercase().contains(&query)
            || entry.summary.to_ascii_lowercase().contains(&query)
    }).collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        category_index(&left.category)
            .cmp(&category_index(&right.category))
            .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
    });
    rows
}

fn render_areas(terminal: &mut dyn TerminalSession, selected: usize) -> Result<()> {
    let mut lines = vec![
        "Elige un área de SST. Nwash/Bash y Unix aparecen al final como capas de compatibilidad.".to_owned(),
        "Cada comando abre directamente su documentación completa, con opciones, ejemplos, permisos y recuperación.".to_owned(),
        String::new(),
    ];
    for (index, (name, summary)) in CATEGORIES.iter().enumerate() {
        let marker = if index == selected { ">" } else { " " };
        lines.push(format!("{marker} {name:<16} {summary}"));
    }
    draw(terminal, "Shell Shock Tool — Guía", &lines, "↑↓ navegar · ENTER abrir · / buscar · Q salir")
}

fn render_commands(terminal: &mut dyn TerminalSession, area: &str, entries: &[&Entry], selected: usize) -> Result<()> {
    let (rows, start, end) = window(entries.len(), selected, terminal.size()?.1, 2, 20);
    let mut lines = vec![format!("Área: {area}"), String::new()];
    for index in start..end {
        let entry = entries[index];
        let marker = if index == selected { ">" } else { " " };
        lines.push(format!("{marker} {:<28} {}", entry.name, entry.summary));
    }
    for _ in end.saturating_sub(start)..rows {
        lines.push(String::new());
    }
    draw(terminal, "Shell Shock Tool — Guía", &lines, "ENTER abrir · Backspace/Esc áreas · / buscar · Q salir")
}

fn render_search(terminal: &mut dyn TerminalSession, entries: &[&Entry], selected: usize, query: &str, typing: bool) -> Result<()> {
    let (rows, start, end) = window(entries.len(), selected, terminal.size()?.1, 2, 18);
    let prompt = if query.is_empty() { "(escribe para buscar)" } else { query };
    let mut lines = vec![format!("Buscar: {}{prompt}", if typing { "> " } else { "" }), String::new()];
    if entries.is_empty() {
        lines.push("No hay coincidencias.".to_owned());
    } else {
        for index in start..end {
            let entry = entries[index];
            let marker = if index == selected { ">" } else { " " };
            lines.push(format!("{marker} {:<26} [{:<13}] {}", entry.name, entry.category, entry.summary));
        }
    }
    for _ in end.saturating_sub(start)..rows {
        lines.push(String::new());
    }
    let footer = if typing {
        "Escribe · ENTER terminar búsqueda · Esc cancelar"
    } else {
        "ENTER abrir · / editar · Backspace/Esc áreas · Q salir"
    };
    draw(terminal, "Shell Shock Tool — Buscar", &lines, footer)
}

fn render_detail(terminal: &mut dyn TerminalSession, entry: &Entry, manual: Option<&str>, scroll: usize) -> Result<usize> {
    let (columns, height) = terminal.size()?;
    let body_rows = panel_body_rows(height);
    let alias_rows = if entry.aliases.is_empty() { 0 } else { 2 };
    let rows = body_rows.saturating_sub(alias_rows).max(1);
    let width = content_width(columns);
    let body = manual.map(|text| manual_lines(text, width)).unwrap_or_else(|| {
        let mut rows = wrap(&entry.summary, width);
        rows.push(String::new());
        rows.extend(wrap("No se pudo cargar la documentación extensa. Usa la ayuda integrada del comando como referencia temporal.", width));
        rows
    });
    let max_scroll = body.len().saturating_sub(rows);
    let scroll = scroll.min(max_scroll);
    let mut lines = Vec::new();
    if !entry.aliases.is_empty() {
        lines.push(format!("Aliases: {}", entry.aliases.join(", ")));
        lines.push(String::new());
    }
    lines.extend(body.iter().skip(scroll).take(rows).cloned());
    let footer = if body.len() > rows {
        format!("{}-{} de {} · ↑↓/PgUp/PgDn desplazar · Backspace volver · Q salir", scroll + 1, (scroll + rows).min(body.len()), body.len())
    } else {
        "Backspace/Enter/Esc volver · Q salir".to_owned()
    };
    draw(terminal, &format!("SST — {}", entry.name), &lines, &footer)?;
    Ok(max_scroll)
}

fn panel_body_rows(height: u16) -> usize {
    let available = usize::from(height.max(18)).saturating_sub(4).max(8);
    (CATEGORIES.len() + 3).min(available).max(8)
}

fn window(len: usize, selected: usize, height: u16, reserved: usize, max_rows: usize) -> (usize, usize, usize) {
    let rows = panel_body_rows(height)
        .saturating_sub(reserved)
        .max(1)
        .min(max_rows);
    let start = selected.saturating_sub(rows.saturating_sub(1));
    let end = (start + rows).min(len);
    (rows, start, end)
}

fn draw(terminal: &mut dyn TerminalSession, title: &str, lines: &[String], footer: &str) -> Result<()> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(48));
    let th = usize::from(th.max(18));
    let ww = if tw >= 72 { tw.saturating_sub(4).min(112) } else { tw.saturating_sub(2).max(44) };
    let body_rows = panel_body_rows(th as u16);
    let panel_height = body_rows + 4;
    let top = th.saturating_sub(panel_height) / 2;

    terminal.clear()?;
    let mut out = String::new();
    out.push_str(SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top { out.push('\n'); }

    push(&mut out, tw, ww, &title_line(ww, title));
    for index in 0..body_rows {
        let line = lines.get(index).map(String::as_str).unwrap_or("");
        push(&mut out, tw, ww, &text_line(ww, line));
    }
    push(&mut out, tw, ww, &separator(ww));
    push(&mut out, tw, ww, &text_line(ww, footer));
    push(&mut out, tw, ww, &format!("└{}┘", "─".repeat(ww.saturating_sub(2))));
    out.push_str(RESET);
    terminal.write(&out)?;
    terminal.flush()
}

fn push(out: &mut String, terminal_width: usize, width: usize, line: &str) {
    let margin = terminal_width.saturating_sub(width) / 2;
    out.push_str(SCREEN);
    out.push_str(&" ".repeat(margin));
    out.push_str(PANEL);
    out.push_str(line);
    out.push_str(SCREEN);
    out.push('\n');
}

fn title_line(width: usize, title: &str) -> String {
    let inner = width.saturating_sub(2);
    let title = clip(&format!(" {title} "), inner);
    let rest = inner.saturating_sub(title.chars().count());
    let left = rest / 2;
    let right = rest.saturating_sub(left);
    format!("┌{}{}{}┐", "─".repeat(left), title, "─".repeat(right))
}

fn separator(width: usize) -> String {
    format!("├{}┤", "─".repeat(width.saturating_sub(2)))
}

fn text_line(width: usize, text: &str) -> String {
    let inner = width.saturating_sub(2);
    let text = clip(text, inner.saturating_sub(2));
    let used = text.chars().count();
    format!("│ {}{} │", text, " ".repeat(inner.saturating_sub(used + 2)))
}

fn content_width(columns: u16) -> usize {
    let terminal = usize::from(columns.max(48));
    let window = if terminal >= 72 { terminal.saturating_sub(4).min(112) } else { terminal.saturating_sub(2).max(44) };
    window.saturating_sub(6)
}

fn manual_lines(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let indent = raw.chars().take_while(|ch| ch.is_whitespace()).count().min(8);
        let prefix = " ".repeat(indent);
        let available = width.saturating_sub(indent).max(8);
        for line in wrap(raw.trim(), available) {
            out.push(format!("{prefix}{line}"));
        }
    }
    if out.is_empty() { out.push("Sin contenido.".to_owned()); }
    out
}

fn wrap(value: &str, width: usize) -> Vec<String> {
    if width == 0 { return vec![String::new()]; }
    let mut rows = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        let extra = word.chars().count() + if line.is_empty() { 0 } else { 1 };
        if !line.is_empty() && line.chars().count() + extra > width {
            rows.push(line);
            line = String::new();
        }
        if !line.is_empty() { line.push(' '); }
        line.push_str(word);
    }
    if !line.is_empty() || rows.is_empty() { rows.push(line); }
    rows
}

fn clip(value: &str, width: usize) -> String {
    if value.chars().count() <= width { return value.to_owned(); }
    if width <= 1 { return "…".chars().take(width).collect(); }
    let mut out = value.chars().take(width - 1).collect::<String>();
    out.push('…');
    out
}