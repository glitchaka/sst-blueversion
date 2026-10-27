use std::{sync::Arc, time::Duration};

use anyhow::Result;

use crate::core::{
    CommandContext, CommandOutput,
    ports::{TerminalFactory, TerminalKey, TerminalSession},
};

use super::{BuiltinCommand, UNIX_COMMANDS};

const G_SCREEN: &str = "\x1b[44;37m";
const G_PANEL: &str = "\x1b[47;30m";
const G_RESET: &str = "\x1b[0m";

const CATEGORY_ORDER: &[(&str, &str)] = &[
    ("Sistema", "Información del equipo, procesos, memoria, discos, nombre, dominio y Autologon."),
    ("Red", "Interfaces, rutas, DNS, vecinos, descubrimiento, tráfico, inventario, switches y Wake-on-LAN."),
    ("Diagnóstico", "Revisiones compuestas para detectar rápidamente problemas de red y del equipo."),
    ("Windows", "Servicios, Event Log, Registro, procesos, ACL, PnP, tareas, sesiones, SMB, firewall y energía."),
    ("Seguridad", "Triage, inteligencia, inspección de procesos, persistencia y relaciones de confianza."),
    ("Privilegios", "Elevación a Administrador, LOCAL SYSTEM y TrustedInstaller."),
    ("Herramientas", "Herramientas auxiliares integradas en SST."),
    ("Office", "Diagnóstico, autenticación y desinstalación completa de Microsoft Office."),
    ("Paquetes", "Instalación y gestión de herramientas portables dentro de SST."),
    ("Configuración", "Configuración de SST, fondos, rutas y recarga de preferencias."),
    ("Actualización", "Estado, búsqueda, descarga y aplicación de nuevas versiones de SST."),
    ("Editor", "Helix-SST y sus controles de edición."),
    ("Nwash / Bash", "Builtins y comportamiento de compatibilidad con Bash implementados por Nwash."),
    ("Unix", "Comandos Unix/GNU-like reimplementados dentro de SST."),
    ("Ejemplos", "Ejemplos prácticos para aprender a usar SST y Nwash."),
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

const SST_ENTRIES: &[(&str, &[&str], &str, &str)] = &[
    ("sys", &["ps", "top", "df", "free", "whoami", "uname", "uptime", "fetch", "neofetch", "fastfetch"], "Sistema", "Información y administración local del equipo."),
    ("hostname", &["sys hostname"], "Sistema", "Consulta o cambia el nombre del equipo."),
    ("sys domain", &["domain-admin"], "Sistema", "Consulta, une o saca el equipo de un dominio Windows."),
    ("sys autologon", &["autologon"], "Sistema", "Configura inicio automático mediante Sysinternals Autologon."),
    ("notsh", &["net"], "Red", "Shell de red unificada de SST."),
    ("device", &[], "Red", "Inventario local de equipos por MAC/IP."),
    ("switch", &[], "Red", "Localiza una MAC en switch y puerto mediante perfiles SNMP."),
    ("wol", &[], "Red", "Envía Wake-on-LAN por MAC o equipo inventariado."),
    ("diag", &[], "Diagnóstico", "Ejecuta diagnósticos compuestos del equipo y la red."),
    ("domain", &[], "Windows", "Consulta pertenencia a dominio local o remota."),
    ("eventlog", &[], "Windows", "Consulta, exporta y limpia Windows Event Log."),
    ("service", &[], "Windows", "Consulta y controla servicios de Windows."),
    ("registry", &[], "Windows", "Consulta y modifica el Registro de Windows."),
    ("process", &[], "Windows", "Lista, inspecciona y termina procesos Windows."),
    ("acl", &[], "Windows", "Consulta y modifica permisos ACL NTFS."),
    ("pnp", &[], "Windows", "Consulta y controla dispositivos Plug and Play."),
    ("task", &[], "Windows", "Consulta y controla tareas programadas."),
    ("session", &[], "Windows", "Consulta y administra sesiones locales/RDP."),
    ("share", &[], "Windows", "Consulta y administra recursos compartidos SMB."),
    ("firewall", &[], "Windows", "Consulta y cambia perfiles de Windows Defender Firewall."),
    ("power", &[], "Windows", "Apaga, reinicia, cierra sesión o hiberna el equipo."),
    ("triage", &[], "Seguridad", "Analiza procesos y señales de seguridad del equipo."),
    ("intel", &[], "Seguridad", "Consulta fuentes e indicadores de inteligencia de seguridad."),
    ("sudo", &["runas"], "Privilegios", "Eleva comandos o sesiones con privilegios Windows."),
    ("tools", &["tool", "utils"], "Herramientas", "Catálogo de herramientas auxiliares integradas."),
    ("tools office diagnose", &["tools office status", "tools office verify"], "Office", "Audita instalación, Click-to-Run, WAM, WebView2, Entra y cachés de autenticación."),
    ("tools office auth repair", &[], "Office", "Repara el registro de AAD BrokerPlugin para autenticación de Office."),
    ("tools office scrub", &["tools office sara", "tools office uninstall", "tools office remove"], "Office", "Desinstala Office mediante SaRACmd legacy y audita residuos."),
    ("tools office scrub-current", &["tools office gethelp"], "Office", "Desinstala Office mediante GetHelpCmd actual."),
    ("instp", &[], "Paquetes", "Gestiona paquetes portables y aislados de SST."),
    ("config", &[], "Configuración", "Consulta y modifica la configuración de SST."),
    ("reload", &[], "Configuración", "Recarga la configuración sin cerrar SST."),
    ("sst-path", &[], "Configuración", "Traduce rutas estilo Unix al modelo de rutas de SST."),
    ("update", &[], "Actualización", "Consulta, busca y prepara actualizaciones de SST."),
    ("helix", &["hx", "helix-sst"], "Editor", "Editor Helix adaptado e integrado en SST."),
    ("tour", &[], "Ejemplos", "Ejemplos prácticos y demostraciones de uso de SST/Nwash."),
];

#[derive(Clone)]
struct GuideEntry {
    name: String,
    aliases: Vec<String>,
    category: String,
    summary: String,
}

impl GuideEntry {
    fn new(name: &str, aliases: &[&str], category: &str, summary: &str) -> Self {
        Self {
            name: name.to_owned(),
            aliases: aliases.iter().map(|value| (*value).to_owned()).collect(),
            category: category.to_owned(),
            summary: summary.to_owned(),
        }
    }
}

#[derive(Clone)]
enum View {
    Categories,
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

    fn entries() -> Vec<GuideEntry> {
        let mut rows = SST_ENTRIES
            .iter()
            .map(|(name, aliases, category, summary)| GuideEntry::new(name, aliases, category, summary))
            .collect::<Vec<_>>();

        for name in BASH_BUILTINS {
            rows.push(GuideEntry::new(
                name,
                &[],
                "Nwash / Bash",
                bash_summary(name),
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
            rows.push(GuideEntry::new(name, &[], "Unix", summary));
        }

        rows.sort_by(|left, right| {
            category_rank(&left.category)
                .cmp(&category_rank(&right.category))
                .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
        });
        rows
    }

    fn run_ui(&self, initial_topic: Option<String>, initial_search: Option<String>) -> Result<CommandOutput> {
        let entries = Self::entries();
        let mut terminal = self.terminal.alternate_screen()?;
        let mut view = if let Some(topic) = initial_topic {
            View::Detail(topic)
        } else if initial_search.is_some() {
            View::Search
        } else {
            View::Categories
        };
        let mut query = initial_search.unwrap_or_default();
        let mut search_input = !query.is_empty();
        let mut selected = 0usize;
        let mut detail_scroll = 0usize;

        loop {
            match view.clone() {
                View::Detail(topic) => {
                    let Some(item) = find_entry(&entries, &topic) else {
                        view = View::Categories;
                        selected = 0;
                        continue;
                    };
                    let manual = super::guide_remote::load(&item.name)
                        .or_else(|| item.aliases.iter().find_map(|alias| super::guide_remote::load(alias)));
                    let max_scroll = render_detail(
                        terminal.as_mut(),
                        item,
                        manual.as_deref(),
                        detail_scroll,
                    )?;
                    detail_scroll = detail_scroll.min(max_scroll);

                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else {
                        continue;
                    };
                    match key {
                        TerminalKey::Escape | TerminalKey::Enter | TerminalKey::Backspace => {
                            view = View::Commands(category_rank(&item.category));
                            selected = index_in_category(&entries, &item.category, &item.name);
                            detail_scroll = 0;
                        }
                        TerminalKey::Char('x') | TerminalKey::Char('X')
                        | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
                        TerminalKey::Up => detail_scroll = detail_scroll.saturating_sub(1),
                        TerminalKey::Down => detail_scroll = (detail_scroll + 1).min(max_scroll),
                        TerminalKey::PageUp => detail_scroll = detail_scroll.saturating_sub(8),
                        TerminalKey::PageDown => detail_scroll = (detail_scroll + 8).min(max_scroll),
                        TerminalKey::Home => detail_scroll = 0,
                        TerminalKey::End => detail_scroll = max_scroll,
                        _ => {}
                    }
                }
                View::Categories => {
                    selected = selected.min(CATEGORY_ORDER.len().saturating_sub(1));
                    render_categories(terminal.as_mut(), selected)?;
                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else {
                        continue;
                    };
                    match key {
                        TerminalKey::Enter => {
                            view = View::Commands(selected);
                            selected = 0;
                        }
                        TerminalKey::Char('/') => {
                            query.clear();
                            search_input = true;
                            selected = 0;
                            view = View::Search;
                        }
                        TerminalKey::Escape
                        | TerminalKey::Char('x') | TerminalKey::Char('X')
                        | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
                        TerminalKey::Up => selected = selected.saturating_sub(1),
                        TerminalKey::Down => selected = (selected + 1).min(CATEGORY_ORDER.len().saturating_sub(1)),
                        TerminalKey::PageUp => selected = selected.saturating_sub(6),
                        TerminalKey::PageDown => selected = (selected + 6).min(CATEGORY_ORDER.len().saturating_sub(1)),
                        TerminalKey::Home => selected = 0,
                        TerminalKey::End => selected = CATEGORY_ORDER.len().saturating_sub(1),
                        _ => {}
                    }
                }
                View::Commands(category_index) => {
                    let category_index = category_index.min(CATEGORY_ORDER.len().saturating_sub(1));
                    let category = CATEGORY_ORDER[category_index].0;
                    let visible = entries
                        .iter()
                        .filter(|item| item.category == category)
                        .collect::<Vec<_>>();
                    selected = selected.min(visible.len().saturating_sub(1));
                    render_commands(terminal.as_mut(), category, &visible, selected)?;
                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else {
                        continue;
                    };
                    match key {
                        TerminalKey::Enter => {
                            if let Some(item) = visible.get(selected) {
                                view = View::Detail(item.name.clone());
                                detail_scroll = 0;
                            }
                        }
                        TerminalKey::Escape | TerminalKey::Backspace => {
                            selected = category_index;
                            view = View::Categories;
                        }
                        TerminalKey::Char('/') => {
                            query.clear();
                            search_input = true;
                            selected = 0;
                            view = View::Search;
                        }
                        TerminalKey::Char('x') | TerminalKey::Char('X')
                        | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
                        TerminalKey::Up => selected = selected.saturating_sub(1),
                        TerminalKey::Down => selected = (selected + 1).min(visible.len().saturating_sub(1)),
                        TerminalKey::PageUp => selected = selected.saturating_sub(8),
                        TerminalKey::PageDown => selected = (selected + 8).min(visible.len().saturating_sub(1)),
                        TerminalKey::Home => selected = 0,
                        TerminalKey::End => selected = visible.len().saturating_sub(1),
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
                View::Search => {
                    let visible = search_entries(&entries, &query);
                    selected = selected.min(visible.len().saturating_sub(1));
                    render_search(terminal.as_mut(), &visible, selected, &query, search_input)?;
                    let Some(key) = terminal.poll_key(Duration::from_millis(200))? else {
                        continue;
                    };

                    if search_input {
                        match key {
                            TerminalKey::Escape => {
                                query.clear();
                                search_input = false;
                                selected = 0;
                                view = View::Categories;
                            }
                            TerminalKey::Enter => search_input = false,
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
                            if let Some(item) = visible.get(selected) {
                                view = View::Detail(item.name.clone());
                                detail_scroll = 0;
                            }
                        }
                        TerminalKey::Char('/') => search_input = true,
                        TerminalKey::Escape | TerminalKey::Backspace => {
                            query.clear();
                            selected = 0;
                            view = View::Categories;
                        }
                        TerminalKey::Char('x') | TerminalKey::Char('X')
                        | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
                        TerminalKey::Up => selected = selected.saturating_sub(1),
                        TerminalKey::Down => selected = (selected + 1).min(visible.len().saturating_sub(1)),
                        TerminalKey::PageUp => selected = selected.saturating_sub(8),
                        TerminalKey::PageDown => selected = (selected + 8).min(visible.len().saturating_sub(1)),
                        TerminalKey::Home => selected = 0,
                        TerminalKey::End => selected = visible.len().saturating_sub(1),
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
            return self.run_ui(None, Some(args.get(1..).unwrap_or_default().join(" ")));
        }
        if args.is_empty() || matches!(args.first().map(String::as_str), Some("--help") | Some("-h")) {
            return self.run_ui(None, None);
        }
        self.run_ui(Some(args.join(" ")), None)
    }
}

fn category_rank(category: &str) -> usize {
    CATEGORY_ORDER
        .iter()
        .position(|(name, _)| name.eq_ignore_ascii_case(category))
        .unwrap_or(CATEGORY_ORDER.len())
}

fn find_entry<'a>(entries: &'a [GuideEntry], name: &str) -> Option<&'a GuideEntry> {
    entries
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            entries.iter().find(|item| {
                item.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(name))
            })
        })
}

fn index_in_category(entries: &[GuideEntry], category: &str, name: &str) -> usize {
    entries
        .iter()
        .filter(|item| item.category == category)
        .position(|item| item.name == name)
        .unwrap_or(0)
}

fn search_entries<'a>(entries: &'a [GuideEntry], query: &str) -> Vec<&'a GuideEntry> {
    let needle = query.trim().to_ascii_lowercase();
    let mut rows = entries
        .iter()
        .filter(|item| {
            needle.is_empty()
                || item.name.to_ascii_lowercase().contains(&needle)
                || item.aliases.iter().any(|alias| alias.to_ascii_lowercase().contains(&needle))
                || item.category.to_ascii_lowercase().contains(&needle)
                || item.summary.to_ascii_lowercase().contains(&needle)
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        category_rank(&left.category)
            .cmp(&category_rank(&right.category))
            .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
    });
    rows
}

fn bash_summary(name: &str) -> &'static str {
    match name {
        ":" => "Builtin nulo; devuelve éxito.",
        "." | "source" => "Ejecuta un archivo en la shell actual.",
        "[" | "test" => "Evalúa expresiones condicionales.",
        "alias" => "Define o consulta aliases.",
        "bg" => "Reanuda trabajos en segundo plano.",
        "bind" => "Gestiona bindings de edición de línea.",
        "break" => "Sale de uno o más bucles.",
        "builtin" => "Ejecuta explícitamente un builtin.",
        "caller" => "Muestra contexto de llamada de funciones.",
        "cd" => "Cambia el directorio actual.",
        "command" => "Ejecuta o resuelve un comando ignorando funciones cuando corresponde.",
        "compgen" => "Genera candidatos de autocompletado.",
        "complete" => "Define reglas de autocompletado.",
        "compopt" => "Ajusta opciones de autocompletado.",
        "continue" => "Continúa la siguiente iteración de un bucle.",
        "declare" | "typeset" => "Declara variables y atributos.",
        "dirs" => "Muestra la pila de directorios.",
        "disown" => "Desasocia trabajos de la shell.",
        "echo" => "Imprime argumentos y escapes soportados.",
        "enable" => "Habilita o deshabilita builtins.",
        "eval" => "Evalúa texto como código shell.",
        "exec" => "Ejecuta un comando en el contexto actual.",
        "exit" | "logout" => "Termina la shell o sesión.",
        "export" => "Exporta variables o funciones al entorno.",
        "false" => "Devuelve estado 1.",
        "fc" => "Lista, edita o repite comandos del historial.",
        "fg" => "Trae un trabajo al primer plano.",
        "getopts" => "Procesa opciones cortas en scripts.",
        "hash" => "Gestiona la caché de resolución de comandos.",
        "help" => "Muestra ayuda de builtins Nwash/Bash.",
        "history" => "Consulta o modifica el historial.",
        "jobs" => "Lista trabajos administrados por la shell.",
        "kill" => "Envía señales a procesos o trabajos.",
        "let" => "Evalúa expresiones aritméticas.",
        "local" => "Declara variables locales de función.",
        "mapfile" | "readarray" => "Lee líneas en un array indexado.",
        "popd" => "Elimina una entrada de la pila de directorios.",
        "printf" => "Imprime datos con formato.",
        "pushd" => "Añade o rota la pila de directorios.",
        "pwd" => "Muestra el directorio actual.",
        "read" => "Lee entrada y la asigna a variables.",
        "readonly" => "Marca variables o funciones como solo lectura.",
        "return" => "Retorna de una función o archivo source.",
        "set" => "Configura opciones y parámetros de la shell.",
        "shift" => "Desplaza parámetros posicionales.",
        "shopt" => "Configura opciones específicas de Bash/Nwash.",
        "suspend" => "Suspende la shell cuando el host lo permite.",
        "times" => "Muestra tiempos acumulados de CPU.",
        "trap" => "Gestiona traps y señales.",
        "true" => "Devuelve estado 0.",
        "type" => "Describe cómo se resuelve un nombre.",
        "ulimit" => "Consulta o configura límites compatibles de la shell.",
        "umask" => "Consulta o configura la máscara de permisos.",
        "unalias" => "Elimina aliases.",
        "unset" => "Elimina variables o funciones.",
        "wait" => "Espera procesos o trabajos.",
        _ => "Builtin compatible implementado por Nwash.",
    }
}

fn render_categories(terminal: &mut dyn TerminalSession, selected: usize) -> Result<()> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(48));
    let th = usize::from(th.max(18));
    let ww = window_width(tw);
    let rows = th.saturating_sub(8).clamp(6, CATEGORY_ORDER.len());
    let start = selected.saturating_sub(rows.saturating_sub(1));
    let end = (start + rows).min(CATEGORY_ORDER.len());

    let mut lines = Vec::new();
    lines.push("Elige un área. SST aparece primero; Nwash/Bash y Unix están separados como capas de compatibilidad.".to_owned());
    lines.push("ENTER abre el área · / busca en toda la guía · Esc/Q sale".to_owned());
    lines.push(String::new());
    for index in start..end {
        let (name, description) = CATEGORY_ORDER[index];
        let marker = if index == selected { ">" } else { " " };
        lines.push(format!("{marker} {name:<16} {description}"));
    }

    render_box(terminal, tw, th, ww, "Shell Shock Tool — Guía", &lines, "↑↓ navegar · ENTER abrir · / buscar · Q salir")
}

fn render_commands(
    terminal: &mut dyn TerminalSession,
    category: &str,
    entries: &[&GuideEntry],
    selected: usize,
) -> Result<()> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(48));
    let th = usize::from(th.max(18));
    let ww = window_width(tw);
    let rows = th.saturating_sub(7).clamp(5, 20);
    let start = selected.saturating_sub(rows.saturating_sub(1));
    let end = (start + rows).min(entries.len());

    let mut lines = Vec::new();
    lines.push(format!("Área: {category}"));
    lines.push(String::new());
    for index in start..end {
        let item = entries[index];
        let marker = if index == selected { ">" } else { " " };
        lines.push(format!("{marker} {:<26} {}", item.name, item.summary));
    }

    render_box(terminal, tw, th, ww, "Shell Shock Tool — Guía", &lines, "ENTER manual completo · Backspace/Esc áreas · / buscar · Q salir")
}

fn render_search(
    terminal: &mut dyn TerminalSession,
    entries: &[&GuideEntry],
    selected: usize,
    query: &str,
    input: bool,
) -> Result<()> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(48));
    let th = usize::from(th.max(18));
    let ww = window_width(tw);
    let rows = th.saturating_sub(8).clamp(5, 18);
    let start = selected.saturating_sub(rows.saturating_sub(1));
    let end = (start + rows).min(entries.len());

    let mut lines = Vec::new();
    lines.push(format!("Buscar: {}{}", if input { "> " } else { "" }, if query.is_empty() { "(escribe para buscar)" } else { query }));
    lines.push(String::new());
    if entries.is_empty() {
        lines.push("No hay coincidencias.".to_owned());
    } else {
        for index in start..end {
            let item = entries[index];
            let marker = if index == selected { ">" } else { " " };
            lines.push(format!("{marker} {:<26} [{:<13}] {}", item.name, item.category, item.summary));
        }
    }

    let footer = if input {
        "Escribe · ENTER terminar búsqueda · Esc cancelar"
    } else {
        "ENTER abrir · / editar búsqueda · Backspace/Esc áreas · Q salir"
    };
    render_box(terminal, tw, th, ww, "Shell Shock Tool — Buscar", &lines, footer)
}

fn render_detail(
    terminal: &mut dyn TerminalSession,
    item: &GuideEntry,
    manual: Option<&str>,
    scroll: usize,
) -> Result<usize> {
    let (tw, th) = terminal.size()?;
    let tw = usize::from(tw.max(48));
    let th = usize::from(th.max(18));
    let ww = window_width(tw);
    let inner = ww.saturating_sub(6);
    let rows = th.saturating_sub(6).clamp(8, 30);

    let body = manual
        .map(|text| manual_lines(text, inner))
        .unwrap_or_else(|| fallback_lines(item, inner));
    let max_scroll = body.len().saturating_sub(rows);
    let scroll = scroll.min(max_scroll);

    let mut lines = Vec::new();
    let aliases = if item.aliases.is_empty() {
        String::new()
    } else {
        format!("Aliases: {}", item.aliases.join(", "))
    };
    if !aliases.is_empty() {
        lines.push(aliases);
        lines.push(String::new());
    }
    lines.extend(body.iter().skip(scroll).take(rows).cloned());

    let position = if body.len() > rows {
        format!("líneas {}-{} de {} · ↑↓/PgUp/PgDn desplazar · Backspace volver · Q salir", scroll + 1, (scroll + rows).min(body.len()), body.len())
    } else {
        "Backspace/Enter/Esc volver · Q salir".to_owned()
    };
    render_box(terminal, tw, th, ww, &format!("SST — {}", item.name), &lines, &position)?;
    Ok(max_scroll)
}

fn fallback_lines(item: &GuideEntry, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(wrap(&item.summary, width));
    out.push(String::new());
    out.extend(wrap(
        "No se pudo cargar la documentación completa en este momento. La referencia del comando sigue disponible desde su ayuda integrada.",
        width,
    ));
    out
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

fn render_box(
    terminal: &mut dyn TerminalSession,
    tw: usize,
    th: usize,
    ww: usize,
    title: &str,
    lines: &[String],
    footer: &str,
) -> Result<()> {
    let content_rows = th.saturating_sub(4).max(8);
    let visible = lines.len().min(content_rows);
    let top_padding = th.saturating_sub(visible + 4) / 2;

    terminal.clear()?;
    let mut out = String::new();
    out.push_str(G_SCREEN);
    out.push_str("\x1b[2J\x1b[H");
    for _ in 0..top_padding {
        out.push('\n');
    }

    push_window(&mut out, tw, ww, &title_border(ww, title));
    for line in lines.iter().take(content_rows) {
        push_window(&mut out, tw, ww, &frame_line(ww, line));
    }
    for _ in visible..content_rows.min(lines.len().max(visible)) {
        push_window(&mut out, tw, ww, &frame_line(ww, ""));
    }
    push_window(&mut out, tw, ww, &separator(ww));
    push_window(&mut out, tw, ww, &frame_line(ww, footer));
    push_window(&mut out, tw, ww, &bottom_border(ww));
    out.push_str(G_RESET);

    terminal.write(&out)?;
    terminal.flush()
}

fn window_width(terminal_width: usize) -> usize {
    if terminal_width >= 72 {
        terminal_width.saturating_sub(4).min(112)
    } else {
        terminal_width.saturating_sub(2).max(44)
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
    format!("┌{}{}{}┐", "─".repeat(left), title, "─".repeat(right))
}

fn separator(width: usize) -> String {
    format!("├{}┤", "─".repeat(width.saturating_sub(2)))
}

fn bottom_border(width: usize) -> String {
    format!("└{}┘", "─".repeat(width.saturating_sub(2)))
}

fn frame_line(width: usize, value: &str) -> String {
    let inner = width.saturating_sub(2);
    let value = clip(value, inner.saturating_sub(2));
    let visible = value.chars().count();
    format!("│ {}{} │", value, " ".repeat(inner.saturating_sub(visible + 2)))
}

fn wrap(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        let extra = if line.is_empty() {
            word.chars().count()
        } else {
            1 + word.chars().count()
        };
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
