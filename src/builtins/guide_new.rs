use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    adapters::persistence::AppPaths,
    core::{
        CommandContext, CommandOutput,
        ports::{TerminalFactory, TerminalKey, TerminalSession},
    },
};

use super::BuiltinCommand;

const SCREEN: &str = "\x1b[44;37m";
const PANEL: &str = "\x1b[47;30m";
const RESET: &str = "\x1b[0m";
const ARCH_AREA: &str = "arch";
const ARCH_REVIEW_FILE: &str = "arch-review.json";

#[derive(Clone, Deserialize)]
struct GuideUi {
    title: String,
    #[serde(default)]
    intro: Vec<String>,
    areas_footer: String,
    commands_footer: String,
    search_footer: String,
    search_typing_footer: String,
    detail_footer: String,
    detail_scroll_footer: String,
    area_label: String,
    search_label: String,
    empty_search: String,
    aliases_label: String,
}

#[derive(Clone, Deserialize)]
struct Area {
    id: String,
    name: String,
    summary: String,
}

#[derive(Clone, Deserialize)]
struct ManualSpec {
    file: String,
    #[serde(default = "default_extension")]
    extension: String,
    #[serde(default = "default_section")]
    section: String,
    #[serde(default)]
    topic: Option<String>,
}

fn default_extension() -> String {
    "txt".to_owned()
}

fn default_section() -> String {
    "whole".to_owned()
}

#[derive(Clone, Deserialize)]
struct EntrySpec {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    area: String,
    summary: String,
    manual: ManualSpec,
}

#[derive(Deserialize)]
struct CatalogSpec {
    schema: u32,
    ui: GuideUi,
    areas: Vec<Area>,
    entries: Vec<EntrySpec>,
}

#[derive(Default, Serialize, Deserialize)]
struct ArchReviewState {
    #[serde(default)]
    approved: HashSet<String>,
}

struct Entry {
    name: String,
    aliases: Vec<String>,
    area: String,
    summary: String,
    manual: ManualSpec,
    search_key: String,
}

struct GuideIndex {
    ui: GuideUi,
    areas: Vec<Area>,
    entries: Vec<Entry>,
    lookup: HashMap<String, usize>,
    by_area: Vec<Vec<usize>>,
    area_lookup: HashMap<String, usize>,
}

impl GuideIndex {
    fn from_json(raw: &str) -> Option<Self> {
        let spec: CatalogSpec = serde_json::from_str(raw).ok()?;
        if spec.schema != 1 || spec.areas.is_empty() {
            return None;
        }

        let mut area_lookup = HashMap::new();
        for (index, area) in spec.areas.iter().enumerate() {
            let id = area.id.trim().to_ascii_lowercase();
            if id.is_empty() || area_lookup.insert(id, index).is_some() {
                return None;
            }
        }

        let mut entries = Vec::with_capacity(spec.entries.len());
        let mut lookup = HashMap::new();
        let mut by_area = vec![Vec::new(); spec.areas.len()];

        for item in spec.entries {
            let name = item.name.trim().to_owned();
            if name.is_empty() {
                return None;
            }
            let area_key = item.area.trim().to_ascii_lowercase();
            let area_index = *area_lookup.get(&area_key)?;
            let aliases = item
                .aliases
                .into_iter()
                .map(|alias| alias.trim().to_owned())
                .filter(|alias| !alias.is_empty())
                .collect::<Vec<_>>();

            let mut search_key = format!(
                "{}\n{}\n{}\n{}",
                name,
                spec.areas[area_index].name,
                spec.areas[area_index].summary,
                item.summary
            )
            .to_ascii_lowercase();
            for alias in &aliases {
                search_key.push('\n');
                search_key.push_str(&alias.to_ascii_lowercase());
            }

            let entry_index = entries.len();
            if lookup
                .insert(name.to_ascii_lowercase(), entry_index)
                .is_some()
            {
                return None;
            }
            for alias in &aliases {
                lookup
                    .entry(alias.to_ascii_lowercase())
                    .or_insert(entry_index);
            }

            entries.push(Entry {
                name,
                aliases,
                area: area_key,
                summary: item.summary,
                manual: item.manual,
                search_key,
            });
            by_area[area_index].push(entry_index);
        }

        Some(Self {
            ui: spec.ui,
            areas: spec.areas,
            entries,
            lookup,
            by_area,
            area_lookup,
        })
    }

    fn find(&self, value: &str) -> Option<usize> {
        self.lookup.get(&value.trim().to_ascii_lowercase()).copied()
    }

    fn area_index(&self, area: &str) -> Option<usize> {
        self.area_lookup.get(&area.to_ascii_lowercase()).copied()
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
        let Some(raw) = super::guide_remote::load_index() else {
            return Ok(CommandOutput::error("guide: no se encontró guide/guide-index.json junto a sst.exe (instala o copia la carpeta guide del paquete de SST).", 1));
        };
        let Some(index) = GuideIndex::from_json(&raw) else {
            return Ok(CommandOutput::error("guide: el índice local guide/guide-index.json está dañado o tiene un formato incompatible.", 1));
        };

        let review_path = arch_review_path();
        let mut review = load_arch_review(&review_path);
        let mut query = search.unwrap_or_default();
        let mut search_results = index.search(&query);
        let mut view = direct
            .as_deref()
            .and_then(|topic| index.find(topic).map(View::Detail))
            .unwrap_or_else(|| if query.is_empty() { View::Areas } else { View::Search });
        let mut typing = matches!(view, View::Search) && !query.is_empty();
        let mut selected = 0usize;
        let mut scroll = 0usize;
        let mut terminal = self.terminal.alternate_screen()?;
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
                        selected = selected.min(index.areas.len().saturating_sub(1));
                        render_areas(terminal.as_mut(), size, &index, selected)?;
                    }
                    View::Commands(area) => {
                        let area = area.min(index.areas.len().saturating_sub(1));
                        let visible = &index.by_area[area];
                        selected = selected.min(visible.len().saturating_sub(1));
                        render_commands(
                            terminal.as_mut(),
                            size,
                            &index,
                            area,
                            visible,
                            selected,
                            &review.approved,
                        )?;
                    }
                    View::Search => {
                        selected = selected.min(search_results.len().saturating_sub(1));
                        render_search(
                            terminal.as_mut(),
                            size,
                            &index,
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
                            let manual = manual_cache.entry(entry_index).or_insert_with(|| {
                                super::guide_remote::load_manual(
                                    &entry.manual.file,
                                    &entry.manual.extension,
                                    &entry.manual.section,
                                    entry.manual.topic.as_deref(),
                                )
                            });
                            let lines = manual
                                .as_deref()
                                .map(|text| manual_lines(text, width))
                                .unwrap_or_default();
                            let body = Arc::new(lines);
                            wrapped_cache.insert((entry_index, width), Arc::clone(&body));
                            body
                        };
                        let approved = review.approved.contains(&review_key(entry));
                        let max_scroll = render_detail(
                            terminal.as_mut(),
                            size,
                            &index,
                            entry,
                            body.as_slice(),
                            scroll,
                            approved,
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
                        selected = (selected + 1).min(index.areas.len().saturating_sub(1));
                        dirty = true;
                    }
                    TerminalKey::PageUp => {
                        selected = selected.saturating_sub(6);
                        dirty = true;
                    }
                    TerminalKey::PageDown => {
                        selected = (selected + 6).min(index.areas.len().saturating_sub(1));
                        dirty = true;
                    }
                    TerminalKey::Home => {
                        selected = 0;
                        dirty = true;
                    }
                    TerminalKey::End => {
                        selected = index.areas.len().saturating_sub(1);
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
                    let area = area.min(index.areas.len().saturating_sub(1));
                    let visible = &index.by_area[area];
                    match key {
                        TerminalKey::Enter => {
                            if let Some(entry_index) = visible.get(selected).copied() {
                                view = View::Detail(entry_index);
                                scroll = 0;
                                dirty = true;
                            }
                        }
                        TerminalKey::Char(' ') if index.areas[area].id == ARCH_AREA => {
                            if let Some(entry_index) = visible.get(selected).copied() {
                                toggle_arch_review(&mut review, &index.entries[entry_index]);
                                save_arch_review(&review_path, &review)?;
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
                    let rows = detail_rows(size.1, entry.aliases.is_empty(), entry.area == ARCH_AREA);
                    let max_scroll = body_len.saturating_sub(rows);
                    match key {
                        TerminalKey::Char(' ') if entry.area == ARCH_AREA => {
                            toggle_arch_review(&mut review, entry);
                            save_arch_review(&review_path, &review)?;
                            dirty = true;
                        }
                        TerminalKey::Backspace | TerminalKey::Enter | TerminalKey::Escape => {
                            let area = index.area_index(&entry.area).unwrap_or(0);
                            selected = index.by_area[area]
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
        "guide — documentación interactiva local de SST, sin conexión a Internet"
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

fn arch_review_path() -> PathBuf {
    AppPaths::detect()
        .data_dir()
        .join("guide")
        .join(ARCH_REVIEW_FILE)
}

fn load_arch_review(path: &Path) -> ArchReviewState {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_arch_review(path: &Path, state: &ArchReviewState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

fn review_key(entry: &Entry) -> String {
    entry.name.trim().to_ascii_lowercase()
}

fn toggle_arch_review(state: &mut ArchReviewState, entry: &Entry) {
    let key = review_key(entry);
    if !state.approved.insert(key.clone()) {
        state.approved.remove(&key);
    }
}

fn render_areas(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    index: &GuideIndex,
    selected: usize,
) -> Result<()> {
    let mut lines = index.ui.intro.clone();
    if !lines.is_empty() {
        lines.push(String::new());
    }
    for (position, area) in index.areas.iter().enumerate() {
        let marker = if position == selected { ">" } else { " " };
        lines.push(format!("{marker} {:<18} {}", area.name, area.summary));
    }
    draw(
        terminal,
        size,
        &index.ui.title,
        &lines,
        &index.ui.areas_footer,
    )
}

fn render_commands(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    index: &GuideIndex,
    area: usize,
    visible: &[usize],
    selected: usize,
    approved: &HashSet<String>,
) -> Result<()> {
    let (rows, start, end) = window(visible.len(), selected, size.1, 2, 24);
    let arch = index.areas[area].id == ARCH_AREA;
    let mut lines = vec![
        format!("{}: {}", index.ui.area_label, index.areas[area].name),
        String::new(),
    ];
    for position in start..end {
        let entry = &index.entries[visible[position]];
        let marker = if position == selected { ">" } else { " " };
        if arch {
            let checked = if approved.contains(&review_key(entry)) { "x" } else { " " };
            lines.push(format!(
                "{marker} [{checked}] {:<24} {}",
                entry.name, entry.summary
            ));
        } else {
            lines.push(format!("{marker} {:<28} {}", entry.name, entry.summary));
        }
    }
    for _ in end.saturating_sub(start)..rows {
        lines.push(String::new());
    }
    let footer = if arch {
        "↑↓ navegar · ENTER abrir · ESPACIO aprobar/quitar · Backspace volver · Q salir"
    } else {
        &index.ui.commands_footer
    };
    draw(terminal, size, &index.ui.title, &lines, footer)
}

fn render_search(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    index: &GuideIndex,
    visible: &[usize],
    selected: usize,
    query: &str,
    typing: bool,
) -> Result<()> {
    let (rows, start, end) = window(visible.len(), selected, size.1, 2, 22);
    let prompt = if query.is_empty() { "" } else { query };
    let mut lines = vec![
        format!(
            "{}: {}{}",
            index.ui.search_label,
            if typing { "> " } else { "" },
            prompt
        ),
        String::new(),
    ];
    if visible.is_empty() {
        lines.push(index.ui.empty_search.clone());
    } else {
        for position in start..end {
            let entry = &index.entries[visible[position]];
            let area = index
                .area_index(&entry.area)
                .and_then(|area| index.areas.get(area))
                .map(|area| area.name.as_str())
                .unwrap_or("");
            let marker = if position == selected { ">" } else { " " };
            lines.push(format!(
                "{marker} {:<26} [{:<18}] {}",
                entry.name, area, entry.summary
            ));
        }
    }
    for _ in end.saturating_sub(start)..rows {
        lines.push(String::new());
    }
    let footer = if typing {
        &index.ui.search_typing_footer
    } else {
        &index.ui.search_footer
    };
    draw(terminal, size, &index.ui.title, &lines, footer)
}

fn render_detail(
    terminal: &mut dyn TerminalSession,
    size: (u16, u16),
    index: &GuideIndex,
    entry: &Entry,
    body: &[String],
    scroll: usize,
    approved: bool,
) -> Result<usize> {
    let is_arch = entry.area == ARCH_AREA;
    let rows = detail_rows(size.1, entry.aliases.is_empty(), is_arch);
    let max_scroll = body.len().saturating_sub(rows);
    let scroll = scroll.min(max_scroll);
    let mut lines = Vec::new();
    if is_arch {
        lines.push(format!("Aprobado: [{}]", if approved { "x" } else { " " }));
        lines.push(String::new());
    }
    if !entry.aliases.is_empty() {
        lines.push(format!(
            "{}: {}",
            index.ui.aliases_label,
            entry.aliases.join(", ")
        ));
        lines.push(String::new());
    }
    lines.extend(body.iter().skip(scroll).take(rows).cloned());
    let footer = if is_arch {
        if body.len() > rows {
            format!(
                "{}-{} de {} · ↑↓/PgUp/PgDn desplazar · ESPACIO aprobar/quitar · Backspace volver · Q salir",
                scroll + 1,
                (scroll + rows).min(body.len()),
                body.len()
            )
        } else {
            "ESPACIO aprobar/quitar · Backspace/Enter/Esc volver · Q salir".to_owned()
        }
    } else if body.len() > rows {
        index
            .ui
            .detail_scroll_footer
            .replace("{start}", &(scroll + 1).to_string())
            .replace("{end}", &(scroll + rows).min(body.len()).to_string())
            .replace("{total}", &body.len().to_string())
    } else {
        index.ui.detail_footer.clone()
    };
    draw(
        terminal,
        size,
        &format!("{} — {}", index.ui.title, entry.name),
        &lines,
        &footer,
    )?;
    Ok(max_scroll)
}

fn detail_rows(height: u16, no_aliases: bool, arch_review: bool) -> usize {
    let aliases = if no_aliases { 0 } else { 2 };
    let review = if arch_review { 2 } else { 0 };
    panel_body_rows(height)
        .saturating_sub(aliases + review)
        .max(1)
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
    out
}

fn wrap(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        let extra = word.chars().count() + if line.is_empty() { 0 } else { 1 };
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
