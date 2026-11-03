use std::{
    collections::HashMap,
    fs,
    sync::{Arc, Mutex, OnceLock},
};

use crate::adapters::persistence::AppPaths;

const INDEX_FILE: &str = "guide-index.json";
const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;
const MAX_MANUAL_BYTES: usize = 512 * 1024;

static FILE_CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<str>>>>> = OnceLock::new();
static TOPIC_CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<str>>>>> = OnceLock::new();

/// Catálogo SST local. No descarga índices ni incorpora Arch en esta edición.
pub(super) fn load_index() -> Option<String> {
    fetch_repo_file(INDEX_FILE, MAX_INDEX_BYTES)
}

pub(super) fn load_manual(
    file: &str,
    extension: &str,
    section: &str,
    topic: Option<&str>,
) -> Option<Arc<str>> {
    if !valid_component(file) || !valid_extension(extension) {
        return None;
    }

    let topic = topic.unwrap_or_default().trim().to_owned();
    let key = format!("{file}.{extension}\u{1f}{section}\u{1f}{topic}");
    let cache = TOPIC_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return value;
    }

    let raw = fetch_cached(file, extension)?;
    let value = match section {
        "whole" => Some(raw),
        "bash" => extract_bash(&raw, &topic).map(Arc::<str>::from),
        "unix" => extract_unix(&raw, &topic).map(Arc::<str>::from),
        _ => None,
    };

    cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, value.clone());
    value
}

fn fetch_cached(file: &str, extension: &str) -> Option<Arc<str>> {
    let filename = format!("{file}.{extension}");
    let cache = FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&filename)
        .cloned()
    {
        return value;
    }

    // Mantener la caché de fichas: ahora el origen es un archivo local.
    let value = fetch_repo_file(&filename, MAX_MANUAL_BYTES).map(Arc::<str>::from);
    cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(filename, value.clone());
    value
}

fn fetch_repo_file(filename: &str, max_bytes: usize) -> Option<String> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return None;
    }

    let file = AppPaths::detect().root_dir().join("guide").join(filename);
    if let Ok(meta) = fs::metadata(&file) {
        if meta.is_file() && meta.len() > 0 && meta.len() <= max_bytes as u64 {
            if let Ok(bytes) = fs::read(&file) {
                if let Ok(text) = String::from_utf8(bytes) {
                    return Some(text);
                }
            }
        }
    }
    // La descarga es asíncrona: guide funciona inmediatamente incluso si
    // falta aún el fichero físico o el equipo no tiene conectividad.
    crate::adapters::persistence::bundled_guide_asset(filename)
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= max_bytes)
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_extension(value: &str) -> bool {
    matches!(value, "txt" | "md")
}

fn extract_bash(raw: &str, topic: &str) -> Option<String> {
    if matches!(topic, ":" | "true" | "false") {
        return extract_numbered(raw, "Builtins mínimos");
    }

    let lines = raw.lines().collect::<Vec<_>>();
    let start_floor = lines
        .iter()
        .position(|line| line.trim_start().starts_with("2."))
        .unwrap_or(0);

    for index in start_floor..lines.len() {
        let line = lines[index];
        if line == line.trim() && line.eq_ignore_ascii_case(topic) {
            let mut end = lines.len();
            for next in index + 1..lines.len() {
                let candidate = lines[next];
                if candidate == candidate.trim()
                    && (is_numbered_heading(candidate) || is_bash_builtin_name(candidate.trim()))
                {
                    end = next;
                    break;
                }
            }
            return Some(lines[index..end].join("\n").trim().to_owned());
        }
    }

    extract_numbered(raw, topic).or_else(|| extract_bash_reference(raw, topic))
}

fn extract_bash_reference(raw: &str, topic: &str) -> Option<String> {
    // Numerosos builtins comparten una sección (ej. break/continue/return)
    // y no tienen un encabezado individual. Buscar el inicio de su sintaxis
    // dentro de cada sección numerada evita devolver una ficha vacía.
    let lines = raw.lines().collect::<Vec<_>>();
    let headings = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| is_numbered_heading(line).then_some(index))
        .collect::<Vec<_>>();
    for (position, &start) in headings.iter().enumerate().skip(1) {
        let end = headings.get(position + 1).copied().unwrap_or(lines.len());
        if lines[start + 1..end].iter().any(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with(topic) {
                return false;
            }
            trimmed[topic.len()..].chars().next().is_none_or(|next| {
                next.is_whitespace() || matches!(next, '[' | ':' | '/' | '(' | '=')
            })
        }) {
            return Some(lines[start..end].join("\n").trim().to_owned());
        }
    }
    // Un builtin que solo figura en el inventario conserva acceso a la
    // referencia general, en lugar de una pantalla en blanco.
    extract_numbered(raw, "Alcance")
}

fn extract_unix(raw: &str, topic: &str) -> Option<String> {
    if matches!(topic, "env" | "clear") {
        return extract_numbered(raw, "Básicos de UnixService");
    }
    extract_numbered(raw, topic)
}

fn extract_numbered(raw: &str, keyword: &str) -> Option<String> {
    let lines = raw.lines().collect::<Vec<_>>();
    let keyword_tokens = keyword
        .split_whitespace()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let start = lines.iter().position(|line| {
        if !is_numbered_heading(line) {
            return false;
        }
        let tokens = heading_tokens(line);
        keyword_tokens
            .iter()
            .all(|part| tokens.iter().any(|token| token == part))
    })?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| is_numbered_heading(line).then_some(index))
        .unwrap_or(lines.len());
    Some(lines[start..end].join("\n").trim().to_owned())
}

fn is_numbered_heading(line: &str) -> bool {
    let line = line.trim();
    let Some((number, rest)) = line.split_once('.') else {
        return false;
    };
    !number.is_empty()
        && number.chars().all(|ch| ch.is_ascii_digit())
        && rest.starts_with(' ')
}

fn heading_tokens(line: &str) -> Vec<String> {
    line.split_once('.')
        .map(|(_, rest)| rest)
        .unwrap_or(line)
        .split(|ch: char| !ch.is_alphanumeric() && ch != ':')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_matches(':').to_ascii_lowercase())
        .collect()
}

fn is_bash_builtin_name(value: &str) -> bool {
    matches!(
        value,
        ":" | "." | "[" | "alias" | "bg" | "bind" | "break" | "builtin" | "caller"
        | "cd" | "command" | "compgen" | "complete" | "compopt" | "continue" | "declare"
        | "dirs" | "disown" | "echo" | "enable" | "eval" | "exec" | "exit" | "export"
        | "false" | "fc" | "fg" | "getopts" | "hash" | "help" | "history" | "jobs"
        | "kill" | "let" | "local" | "logout" | "mapfile" | "popd" | "printf" | "pushd"
        | "pwd" | "read" | "readarray" | "readonly" | "return" | "set" | "shift" | "shopt"
        | "source" | "suspend" | "test" | "times" | "trap" | "true" | "type" | "typeset"
        | "ulimit" | "umask" | "unalias" | "unset" | "wait"
    )
}
