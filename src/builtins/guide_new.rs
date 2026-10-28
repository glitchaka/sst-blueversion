use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;

use crate::core::{
    CommandContext, CommandOutput,
    ports::{TerminalFactory, TerminalKey, TerminalSession},
};

use super::{BuiltinCommand, UNIX_COMMANDS, guide_remote::ManualKind};

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

const SST_COMMANDS: &[(&str, &[&str], &str, &str, ManualKind)] = &[
    ("sys", &["ps", "top", "df", "free", "whoami", "uname", "uptime", "fetch", "neofetch", "fastfetch"], "Sistema", "Información y administración local del equipo.", ManualKind::Whole("system")),
    ("hostname", &["sys hostname"], "Sistema", "Consulta o cambia el nombre del equipo.", ManualKind::Whole("hostname")),
    ("sys domain", &["domain-admin"], "Sistema", "Estado, unión y salida de dominio Windows.", ManualKind::Whole("domain")),
    ("sys autologon", &["autologon"], "Sistema", "Inicio automático mediante Sysinternals Autologon.", ManualKind::Whole("autologon")),
    ("notsh", &["net"], "Red", "Shell de red unificada de SST.", ManualKind::Whole("network")),
    ("device", &[], "Red", "Inventario de equipos por MAC/IP.", ManualKind::Whole("device")),
    ("switch", &[], "Red", "Localiza una MAC en switch/puerto.", ManualKind::Whole("switch")),
    ("wol", &[], "Red", "Wake-on-LAN.", ManualKind::Whole("wol")),
    ("diag", &[], "Diagnóstico", "Diagnósticos compuestos.", ManualKind::Whole("diagnostics")),
    ("domain", &[], "Windows", "Consulta pertenencia a dominio local/remota.", ManualKind::Whole("domain-status")),
    ("eventlog", &[], "Windows", "Windows Event Log.", ManualKind::Whole("eventlog")),
    ("service", &[], "Windows", "Servicios Windows.", ManualKind::Whole("service")),
    ("registry", &[], "Windows", "Registro de Windows.", ManualKind::Whole("registry")),
    ("process", &[], "Windows", "Procesos Windows.", ManualKind::Whole("process")),
    ("acl", &[], "Windows", "Permisos ACL NTFS.", ManualKind::Whole("acl")),
    ("pnp", &[], "Windows", "Dispositivos Plug and Play.", ManualKind::Whole("pnp")),
    ("task", &[], "Windows", "Tareas programadas.", ManualKind::Whole("task")),
    ("session", &[], "Windows", "Sesiones locales/RDP.", ManualKind::Whole("session")),
    ("share", &[], "Windows", "Recursos compartidos SMB.", ManualKind::Whole("share")),
    ("firewall", &[], "Windows", "Windows Defender Firewall.", ManualKind::Whole("firewall")),
    ("power", &[], "Windows", "Apagado, reinicio, logoff e hibernación.", ManualKind::Whole("power")),
    ("triage", &[], "Seguridad", "Análisis de seguridad y procesos.", ManualKind::Whole("triage")),
    ("intel", &[], "Seguridad", "Fuentes y lookups de inteligencia.", ManualKind::Whole("intel")),
    ("sudo", &["runas"], "Privilegios", "Elevación de comandos y sesión.", ManualKind::Whole("sudo")),
    ("tools", &["tool", "utils"], "Herramientas", "Framework de utilidades de mantenimiento por producto/proveedor.", ManualKind::Whole("tools")),
    ("tools office diagnose", &["tools office status", "tools office verify"], "Herramientas", "Audita Office, Outlook y la cadena de autenticación.", ManualKind::Whole("office-diagnose")),
    ("tools office auth repair", &[], "Herramientas", "Re-registra AAD BrokerPlugin.", ManualKind::Whole("office-auth-repair")),
    ("tools office scrub", &["tools office sara", "tools office uninstall", "tools office remove"], "Herramientas", "Desinstalación completa con SaRACmd legacy.", ManualKind::Whole("office-sara")),
    ("tools office scrub-current", &["tools office gethelp"], "Herramientas", "Desinstalación con GetHelpCmd actual.", ManualKind::Whole("office-gethelp")),
    ("instp", &[], "Paquetes", "Gestor portable de paquetes SST.", ManualKind::Whole("instp")),
    ("arch packages", &["arch-packages", "arch portability", "arch portabilidad"], "Arch / Portabilidad", "Catálogo vivo de Arch y matriz de portabilidad a Windows/SST.", ManualKind::Whole("arch-packages-portability")),
    ("config", &[], "Configuración", "Configuración principal de SST.", ManualKind::Whole("config")),
    ("reload", &[], "Configuración", "Recarga la configuración de la shell.", ManualKind::Whole("config")),
    ("sst-path", &[], "Configuración", "Traduce rutas Unix/Windows para SST.", ManualKind::Whole("config")),
    ("update", &[], "Actualización", "Busca y prepara actualizaciones SST.", ManualKind::Whole("update")),
    ("helix", &["hx", "helix-sst"], "Editor", "Editor Helix integrado en SST.", ManualKind::Whole("helix")),
];

struct Entry {
    name: String,
    aliases: Vec<String>,
    category: &'static str,
    summary: String,
    manual: ManualKind,
    search_key: String,
}

impl Entry {
    fn new(
        name: &str,
        aliases: &[&str],
        category: &'static str,
        summary: &str,
        manual: ManualKind,
    ) -> Self {
        let aliases = aliases.iter().map(|alias| (*alias).to_owned()).collect::<Vec<_>>();
        let mut search_key = format!("{name}\n{category}\n{summary}").to_ascii_lowercase();
        for alias in &aliases {
            search_key.push('\n');
            search_key.push_str(&alias.to_ascii_lowercase());
        }
        Self {
            name: name.to_owned(),
            aliases,
            category,
            summary: summary.to_owned(),
            manual,
            search_key,
        }
    }
}

struct GuideIndex {
    entries: Vec<Entry>,
    lookup: HashMap<String, usize>,
    by_category: Vec<Vec<usize>>,
}

impl GuideIndex {
    fn build() -> Self {
        let mut entries = SST_COMMANDS
            .iter()
            .map(|(name, aliases, category, summary, manual)| {
                Entry::new(name, aliases, category, summary, *manual)
            })
            .collect::<Vec<_>>();

        for name in BASH_BUILTINS {
            entries.push(Entry::new(
                name,
                &[],
                "Nwash / Bash",
                "Builtin Bash/Nwash; abre su referencia completa de opciones y comportamiento.",
                ManualKind::Bash,
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
            entries.push(Entry::new(name, &[], "Unix", summary, ManualKind::Unix));
        }

        entries.sort_by(|left, right| {
            category_index(left.category)
                .cmp(&category_index(right.category))
                .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
        });

        let mut lookup = HashMap::new();
        let mut by_category = vec![Vec::new(); CATEGORIES.len()];
        for (index, entry) in entries.iter().enumerate() {
            lookup.insert(entry.name.to_ascii_lowercase(), index);
            for alias in &entry.aliases {
                lookup.entry(alias.to_ascii_lowercase()).or_insert(index);
            }
            let category = category_index(entry.category);
            if category < by_category.len() {
                by_category[category].push(index);
            }
        }

        Self {
            entries,
            lookup,
            by_category,
        }
    }

    fn find(&self, value: &str) -> Option<usize> {
        self.lookup.get(&value.trim().to_ascii_lowercase()).copied()
    }

    fn search(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (query.is_empty() || entry.search_key.contains(&query)).then_some(index)
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
enum View {
    Areas,
    Commands(usize),
    Search,
    Detail(usize),
}

pub struct GuideBuiltin {
    terminal: Arc<dyn TerminalFactory>,
}

impl GuideBuiltin {
    pub fn new(terminal: Arc<dyn TerminalFactory>) -> Self {
        Self { terminal }
    }

    fn run(&self, direct: Option<String>, search: Option<String>) -> Result<CommandOutput> {
        let index = GuideIndex::build();
        let mut terminal = self.terminal.alternate_screen()?;
        let mut query = search.unwrap_or_default();
        let mut search_results = index.search(&query);
        let mut view = direct
            .as_deref()
            .and_then(|topic| index.find(topic).map(View::Detail))
            .unwrap_or_else(|| if query.is_empty() { View::Areas } else { View::Search });
        let mut typing = matches!(view, View::Search) && !query.is_empty();
        let mut selected = 0usize;
        let mut scroll = 0usize;
        let mut size = terminal.size()?;
        let mut dirty = true;
        let mut manual_cache: HashMap<usize, Option<Arc<str>>> = HashMap::new();
        let mut wrapped_cache: HashMap<(usize, usize), Arc<Vec<String>>> = HashMap::new();

        loop {
            let current_size = terminal.size()?;
            if current_size != size {
                size = current_size;
                dirty = true;
            }

            if dirty {
                match view {
                    View::Areas => {
                        selected = selected.min(CATEGORIES.len().saturating_sub(1));
                        render_areas(terminal.as_mut(), size, selected)?;
                    }
                    View::Commands(area) => {
                        let area = area.min(CATEGORIES.len().saturating_sub(1));
                        let visible = &index.by_category[area];
                        selected = selected.min(visible.len().saturating_sub(1));
                        render_commands(
                            terminal.as_mut(),
                            size,
                            CATEGORIES[area].0,
                            &index.entries,
                            visible,
                            selected,
                        )?;
                    }
                    View::Search => {
                        selected = selected.min(search_results.len().saturating_sub(1));
                        render_search(
                            terminal.as_mut(),
                            size,
                            &index.entries,
                            &search_results,
                            selected,
                            &query,
                            typing,
                        )?;
                    }
                    View::Detail(entry_index) => {
                        let entry = &index.entries[entry_index];
                        let width = content_width(size.0);
                        let body = if let Some(body) = wrapped_cache.get(&(entry_index, width)).cloned() {
                            body
                        } else {
                            let manual = manual_cache
                                .entry(entry_index)
                                .or_insert_with(|| super::guide_remote::load(entry.manual, &entry.name));
                            let lines = manual
                                .as_deref()
                                .map(|text| manual_lines(text, width))
                                .unwrap_or_else(|| fallback_lines(&entry.summary, width));
                            let body = Arc::new(lines);
                            wrapped_cache.insert((entry_index, width), Arc::clone(&body));
                            body
                        };
                        let max_scroll = render_detail(
                            terminal.as_mut(),
                            size,
                            entry,
                            body.as_slice(),
                            scroll,
                        )?;
                        scroll = scroll.min(max_scroll);
                    }
                }
                dirty = false;
            }

            let Some(key) = terminal.poll_key(Duration::from_millis(100))? else {
                continue;
            };

            match view {
                View::Areas => match key {
                    TerminalKey::Enter => {
                        view = View::Commands(selected);
                        selected = 0;
                        dirty = true;
                    }
                    TerminalKey::Char('/') => {
                        query.clear();
                        search_results = index.search("");
                        typing = true;
                        selected = 0;
                        view = View::Search;
                        dirty = true;
                    }
                    TerminalKey::Up => {
                        selected = selected.saturating_sub(1);
                        dirty = true;
                    }
                    TerminalKey::Down => {
                        selected = (selected + 1).min(CATEGORIES.len().saturating_sub(1));
                        dirty = true;
                    }
                    TerminalKey::PageUp => {
                        selected = selected.saturating_sub(6);
                        dirty = true;
                    }
                    TerminalKey::PageDown => {
                        selected = (selected + 6).min(CATEGORIES.len().saturating_sub(1));
                        dirty = true;
                    }
                    TerminalKey::Home => {
                        selected = 0;
                        dirty = true;
                    }
                    TerminalKey::End => {
                        selected = CATEGORIES.len().saturating_sub(1);
                        dirty = true;
                    }
                    TerminalKey::Escape
                    | TerminalKey::Char('q')
                    | TerminalKey::Char('Q')
                    | TerminalKey::Char('x')
                    | TerminalKey::Char('X') => break,
                    _ => {}
                },
                View::Commands(area) => {
                    let area = area.min(CATEGORIES.len().saturating_sub(1));
                    let visible = &index.by_category[area];
                    match key {
                        TerminalKey::Enter => {
                            if let Some(entry_index) = visible.get(selected).copied() {
                                view = View::Detail(entry_index);
                                scroll = 0;
                                dirty = true;
                            }
                        }
                        TerminalKey::Backspace | TerminalKey::Escape => {
                            selected = area;
                            view = View::Areas;
                            dirty = true;
                        }
                        TerminalKey::Char('/') => {
                            query.clear();
                            search_results = index.search("");
                            typing = true;
                            selected = 0;
                            view = View::Search;
                            dirty = true;
                        }
                        TerminalKey::Up => {
                            selected = selected.saturating_sub(1);
                            dirty = true;
                        }
                        TerminalKey::Down => {
                            selected = (selected + 1).min(visible.len().saturating_sub(1));
                            dirty = true;
                        }
                        TerminalKey::PageUp => {
                            selected = selected.saturating_sub(8);
                            dirty = true;
                        }
                        TerminalKey::PageDown => {
                            selected = (selected + 8).min(visible.len().saturating_sub(1));
                            dirty = true;
                        }
                        TerminalKey::Home => {
                            selected = 0;
                            dirty = true;
                        }
                        TerminalKey::End => {
                            selected = visible.len().saturating_sub(1);
                            dirty = true;
                        }
                        TerminalKey::Char('q')
                        | TerminalKey::Char('Q')
                        | TerminalKey::Char('x')
                        | TerminalKey::Char('X') => break,
                        _ => {}
                    }
                }
                View::Search => {
                    if typing {
                        match key {
                            TerminalKey::Enter => {
                                typing = false;
                                dirty = true;
                            }
                            TerminalKey::Escape => {
                                query.clear();
                                typing = false;
                                selected = 0;
                                view = View::Areas;
                                dirty = true;
                            }
                            TerminalKey::Backspace => {
                                query.pop();
                                search_results = index.search(&query);
                                selected = 0;
                                dirty = true;
                            }
                            TerminalKey::Delete => {
                                query.clear();
                                search_results = index.search("");
                                selected = 0;
                                dirty = true;
                            }
                            TerminalKey::Char(ch) if !ch.is_control() => {
                                query.push(ch);
                                search_results = index.search(&query);
                                selected = 0;
                                dirty = true;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key {
                        TerminalKey::Enter => {
                            if let Some(entry_index) = search_results.get(selected).copied() {
                                view = View::Detail(entry_index);
                                scroll = 0;
                                dirty = true;
                            }
                        }
                        TerminalKey::Char('/') => {
                            typing = true;
                            dirty = true;
                        }
                        TerminalKey::Backspace | TerminalKey::Escape => {
                            query.clear();
                            selected = 0;
                            view = View::Areas;
                            dirty = true;
                        }
                        TerminalKey::Up => {
                            selected = selected.saturating_sub(1);
                            dirty = true;
                        }
                        TerminalKey::Down => {
                            selected = (selected + 1).min(search_results.len().saturating_sub(1));
                            dirty = true;
                        }
                        TerminalKey::PageUp => {
                            selected = selected.saturating_sub(8);
                            dirty = true;
                        }
                        TerminalKey::PageDown => {
                            selected = (selected + 8).min(search_results.len().saturating_sub(1));
                            dirty = true;
                        }
                        TerminalKey::Home => {
                            selected = 0;
                            dirty = true;
                        }
                        TerminalKey::End => {
                            selected = search_results.len().saturating_sub(1);
                            dirty = true;
                        }
                        TerminalKey::Char('q')
                        | TerminalKey::Char('Q')
                        | TerminalKey::Char('x')
                        | TerminalKey::Char('X') => break,
                        _ => {}
                    }
                }
                View::Detail(entry_index) => {
                    let entry = &index.entries[entry_index];
                    let width = content_width(size.0);
                    let body_len = wrapped_cache
                        .get(&(entry_index, width))
                        .map(|body| body.len())
                        .unwrap_or(0);
                    let rows = detail_rows(size.1, entry.aliases.is_empty());
                    let max_scroll = body_len.saturating_sub(rows);
                    match key {
                        TerminalKey::Backspace | TerminalKey::Enter | TerminalKey::Escape => {
                            let area = category_index(entry.category).min(CATEGORIES.len().saturating_sub(1));
                            selected = index.by_category[area]
                                .iter()
                                .position(|candidate| *candidate == entry_index)
                                .unwrap_or(0);
                            scroll = 0;
                            view = View::Commands(area);
                            dirty = true;
                        }
                        TerminalKey::Up => {
                            scroll = scroll.saturating_sub(1);
                            dirty = true;
                        }
                        TerminalKey::Down => {
                            scroll = (scroll + 1).min(max_scroll);
                            dirty = true;
                        }
                        TerminalKey::PageUp => {
                            scroll = scroll.saturating_sub(8);
                            dirty = true;
                        }
                        TerminalKey::PageDown => {
                            scroll = (scroll + 8).min(max_scroll);
                            dirty = true;
                        }
                        TerminalKey::Home => {
                            scroll = 0;
                            dirty = true;
                        }
                        TerminalKey::End => {
                            scroll = max_scroll;
                            dirty = true;
                        }
                        TerminalKey::Char('q')
                        | TerminalKey::Char('Q')
                        | TerminalKey::Char('x')
                        | TerminalKey::Char('X') => break,
                        _ => {}
                    }
                }
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
        "guide — manual interactivo completo de SST"
    }

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

fn render_areas(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    selected: usize,
) -> Result<()> {
    let mut lines = vec![
        "Elige un área de SST. Nwash/Bash y Unix aparecen al final como capas de compatibilidad.".to_owned(),
        "Cada comando abre directamente su documentación completa, con opciones, ejemplos, permisos y recuperación.".to_owned(),
        String::new(),
    ];
    for (index, (name, summary)) in CATEGORIES.iter().enumerate() {
        let marker = if index == selected { ">" } else { " " };
        lines.push(format!("{marker} {name:<18} {summary}"));
    }
    draw(
        terminal,
        size,
        "Shell Shock Tool — Guía",
        &lines,
        "↑↓ navegar · ENTER abrir · / buscar · Q salir",
    )
}

fn render_commands(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    area: &str,
    entries: &[Entry],
    visible: &[usize],
    selected: usize,
) -> Result<()> {
    let (rows, start, end) = window(visible.len(), selected, size.1, 2, 24);
    let mut lines = vec![format!("Área: {area}"), String::new()];
    for position in start..end {
        let entry = &entries[visible[position]];
        let marker = if position == selected { ">" } else { " " };
        lines.push(format!("{marker} {:<28} {}", entry.name, entry.summary));
    }
    for _ in end.saturating_sub(start)..rows {
        lines.push(String::new());
    }
    draw(
        terminal,
        size,
        "Shell Shock Tool — Guía",
        &lines,
        "ENTER abrir · Backspace/Esc áreas · / buscar · Q salir",
    )
}

fn render_search(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    entries: &[Entry],
    visible: &[usize],
    selected: usize,
    query: &str,
    typing: bool,
) -> Result<()> {
    let (rows, start, end) = window(visible.len(), selected, size.1, 2, 22);
    let prompt = if query.is_empty() { "(escribe para buscar)" } else { query };
    let mut lines = vec![format!("Buscar: {}{prompt}", if typing { "> " } else { "" }), String::new()];
    if visible.is_empty() {
        lines.push("No hay coincidencias.".to_owned());
    } else {
        for position in start..end {
            let entry = &entries[visible[position]];
            let marker = if position == selected { ">" } else { " " };
            lines.push(format!("{marker} {:<26} [{:<18}] {}", entry.name, entry.category, entry.summary));
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
    draw(terminal, size, "Shell Shock Tool — Buscar", &lines, footer)
}

fn render_detail(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    entry: &Entry,
    body: &[String],
    scroll: usize,
) -> Result<usize> {
    let rows = detail_rows(size.1, entry.aliases.is_empty());
    let max_scroll = body.len().saturating_sub(rows);
    let scroll = scroll.min(max_scroll);
    let mut lines = Vec::new();
    if !entry.aliases.is_empty() {
        lines.push(format!("Aliases: {}", entry.aliases.join(", ")));
        lines.push(String::new());
    }
    lines.extend(body.iter().skip(scroll).take(rows).cloned());
    let footer = if body.len() > rows {
        format!(
            "{}-{} de {} · ↑↓/PgUp/PgDn desplazar · Backspace volver · Q salir",
            scroll + 1,
            (scroll + rows).min(body.len()),
            body.len()
        )
    } else {
        "Backspace/Enter/Esc volver · Q salir".to_owned()
    };
    draw(
        terminal,
        size,
        &format!("SST — {}", entry.name),
        &lines,
        &footer,
    )?;
    Ok(max_scroll)
}

fn detail_rows(height: u16, no_aliases: bool) -> usize {
    let aliases = if no_aliases { 0 } else { 2 };
    panel_body_rows(height).saturating_sub(aliases).max(1)
}

fn panel_body_rows(height: u16) -> usize {
    usize::from(height.max(18))
        .saturating_sub(4)
        .clamp(8, 28)
}

fn window(
    len: usize,
    selected: usize,
    height: u16,
    reserved: usize,
    max_rows: usize,
) -> (usize, usize, usize) {
    let rows = panel_body_rows(height)
        .saturating_sub(reserved)
        .max(1)
        .min(max_rows);
    let start = selected.saturating_sub(rows.saturating_sub(1));
    let end = (start + rows).min(len);
    (rows, start, end)
}

fn draw(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    title: &str,
    lines: &[String],
    footer: &str,
) -> Result<()> {
    let tw = usize::from(size.0.max(48));
    let th = usize::from(size.1.max(18));
    let ww = if tw >= 72 {
        tw.saturating_sub(4).min(112)
    } else {
        tw.saturating_sub(2).max(44)
    };
    let body_rows = panel_body_rows(size.1);
    let panel_height = body_rows + 4;
    let top = th.saturating_sub(panel_height) / 2;

    let mut out = String::new();
    out.push_str(SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top {
        out.push('\n');
    }

    push(&mut out, tw, ww, &title_line(ww, title));
    for index in 0..body_rows {
        let line = lines.get(index).map(String::as_str).unwrap_or("");
        push(&mut out, tw, ww, &text_line(ww, line));
    }
    push(&mut out, tw, ww, &separator(ww));
    push(&mut out, tw, ww, &text_line(ww, footer));
    push(
        &mut out,
        tw,
        ww,
        &format!("└{}┘", "─".repeat(ww.saturating_sub(2))),
    );
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
    let window = if terminal >= 72 {
        terminal.saturating_sub(4).min(112)
    } else {
        terminal.saturating_sub(2).max(44)
    };
    window.saturating_sub(6)
}

fn fallback_lines(summary: &str, width: usize) -> Vec<String> {
    let mut rows = wrap(summary, width);
    rows.push(String::new());
    rows.extend(wrap(
        "No se pudo cargar la documentación extensa. Usa la ayuda integrada del comando como referencia temporal.",
        width,
    ));
    rows
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
    if out.is_empty() {
        out.push("Sin contenido.".to_owned());
    }
    out
}

fn wrap(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        let extra = word.chars().count() + usize::from(!line.is_empty());
        if !line.is_empty() && line.chars().count() + extra > width {
            rows.push(line);
            line = String::new();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() || rows.is_empty() {
        rows.push(line);
    }
    rows
}

fn clip(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut out = value.chars().take(width - 1).collect::<String>();
    out.push('…');
    out
}
