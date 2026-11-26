use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
};

use crate::adapters::persistence::AppPaths;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const INDEX_FILE: &str = "guide-index.json";
const UPSTREAM_BASE: &str = "https://raw.githubusercontent.com/glitchaka/adm-toolbox/main/tutorials";
const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;
const MAX_MANUAL_BYTES: usize = 512 * 1024;

static FILE_CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<str>>>>> = OnceLock::new();
static TOPIC_CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<str>>>>> = OnceLock::new();

/// El catálogo completo procede del repositorio original, incluido Arch.
/// Si no hay red, se reutiliza la última copia válida y, finalmente, el catálogo SST integrado.
pub(super) fn load_index() -> Option<String> {
    let local = read_local_or_bundled(INDEX_FILE, MAX_INDEX_BYTES);
    let remote = fetch_upstream(INDEX_FILE, MAX_INDEX_BYTES, true)
        .or_else(|| read_upstream_cache(INDEX_FILE, MAX_INDEX_BYTES));
    match (local, remote) {
        (Some(local), Some(remote)) => Some(merge_guide_indexes(&local, &remote).unwrap_or(local)),
        (local, remote) => local.or(remote),
    }
}

// SST owns its command documentation; upstream contributes additional topics.
fn merge_guide_indexes(local: &str, remote: &str) -> Option<String> {
    let mut local: serde_json::Value = serde_json::from_str(local).ok()?;
    let remote: serde_json::Value = serde_json::from_str(remote).ok()?;
    for (section, key) in [("areas", "id"), ("entries", "name")] {
        let entries = local.get_mut(section)?.as_array_mut()?;
        if let Some(extra) = remote.get(section).and_then(|v| v.as_array()) {
            for entry in extra {
                let Some(name) = entry.get(key).and_then(|v| v.as_str()) else { continue; };
                if section == "entries" && matches!(name, "diag" | "netcheck" | "netprobe" | "tools office verify") { continue; }
                if !entries.iter().any(|old| old.get(key).and_then(|v| v.as_str()) == Some(name)) {
                    entries.push(entry.clone());
                }
            }
        }
    }
    serde_json::to_string(&local).ok()
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
    // Los manuales se obtienen bajo demanda del origen y se conservan para uso sin red.
    // No descargar otra vez si ya se recuperó el mismo fichero.
    read_local_or_bundled(filename, max_bytes)
        .or_else(|| read_upstream_cache(filename, max_bytes))
        .or_else(|| fetch_upstream(filename, max_bytes, false))
}

/// Official Arch repository metadata for choosing functionality to adapt.
/// This is RESEARCH ONLY: it never installs, executes or imports Arch packages.
pub(super) fn arch_package_details(name: &str) -> Option<String> {
    if name.is_empty() || name.len() > 128 ||
        !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
    {
        return None;
    }

    // Reuse downloaded metadata offline; do not require another request on
    // every redraw, resize or return to the same package.
    let cache = AppPaths::detect().root_dir()
        .join("guide").join("upstream").join("arch-meta")
        .join(format!("{name}.json"));
    if let Some(raw) = read_text(&cache, 256 * 1024) {
        if let Some(details) = parse_arch_package_details(name, &raw) {
            return Some(details);
        }
    }

    #[cfg(windows)]
    let executable = crate::support::windows::system32_executable("curl.exe").ok()?;
    #[cfg(not(windows))]
    let executable = PathBuf::from("curl");

    let mut request = Command::new(executable);
    request.args([
        "--location", "--fail", "--silent", "--show-error",
        "--proto", "=https", "--tlsv1.2", "--connect-timeout", "3",
        "--max-time", "8", "--retry", "0", "--max-filesize", "262144",
    ]);
    request.arg(format!("https://archlinux.org/packages/search/json/?name={name}"));
    #[cfg(windows)]
    request.creation_flags(0x0800_0000);
    let response = request.output().ok()?;
    if !response.status.success() || response.stdout.len() > 256 * 1024 {
        return None;
    }
    let raw = String::from_utf8(response.stdout).ok()?;
    let details = parse_arch_package_details(name, &raw)?;
    if let Some(parent) = cache.parent() {
        if fs::create_dir_all(parent).is_ok() {
            let _ = fs::write(&cache, raw);
        }
    }
    Some(details)
}

fn parse_arch_package_details(name: &str, raw: &str) -> Option<String> {
    let payload: serde_json::Value = serde_json::from_str(raw).ok()?;
    let results = payload.get("results")?.as_array()?;
    let entry = results.iter()
        .filter(|value| value.get("pkgname").and_then(|v| v.as_str()) == Some(name))
        .max_by_key(|value| {
            let arch = value.get("arch").and_then(|v| v.as_str()).unwrap_or("");
            let repo = value.get("repo").and_then(|v| v.as_str()).unwrap_or("");
            (u8::from(arch == "x86_64"), u8::from(repo == "extra"))
        })?;
    let str_field = |field: &str| entry.get(field).and_then(|value| value.as_str()).unwrap_or("");
    let description = str_field("pkgdesc");
    if description.trim().is_empty() { return None; }
    let mut details = format!(
        "Descripción oficial (Arch Linux): {description}\n\
         Paquete: {name}\n\
         Versión en Arch: {}\n\
         Repositorio: {}\n\
         Arquitectura: {}\n",
        str_field("pkgver"), str_field("repo"), str_field("arch")
    );
    let url = str_field("url");
    if !url.is_empty() { details.push_str(&format!("Proyecto: {url}\n")); }
    for (field, caption) in [
        ("licenses", "Licencias"),
        ("depends", "Dependencias"),
        ("optdepends", "Dependencias opcionales"),
    ] {
        if let Some(values) = entry.get(field).and_then(|v| v.as_array()) {
            let values = values.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>();
            if !values.is_empty() {
                details.push_str(&format!("{caption}: {}\n", values.join(", ")));
            }
        }
    }
    details.push_str(
        "\nReferencia técnica para decidir una adaptación nativa a SST.\n\
         Aprobar este elemento NO instala ni ejecuta paquetes de Arch.\n\
         Fuente: https://archlinux.org/packages/\n"
    );
    Some(details)
}

fn read_text(path: &std::path::Path, max_bytes: usize) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes as u64 {
        return None;
    }
    String::from_utf8(fs::read(path).ok()?).ok()
}

fn upstream_path(filename: &str) -> Option<PathBuf> {
    if !valid_filename(filename) {
        return None;
    }
    Some(AppPaths::detect().root_dir().join("guide").join("upstream").join(filename))
}

fn valid_filename(filename: &str) -> bool {
    filename == INDEX_FILE
        || filename.strip_suffix(".txt").is_some_and(valid_component)
        || filename.strip_suffix(".md").is_some_and(valid_component)
}

fn valid_upstream_text(filename: &str, text: &str) -> bool {
    if filename != INDEX_FILE {
        return !text.is_empty();
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    json.get("schema").and_then(|v| v.as_u64()) == Some(1)
        && json.get("areas").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty())
        && json.get("entries").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty())
}

fn read_upstream_cache(filename: &str, max_bytes: usize) -> Option<String> {
    let path = upstream_path(filename)?;
    let text = read_text(&path, max_bytes)?;
    valid_upstream_text(filename, &text).then_some(text)
}

fn fetch_upstream(filename: &str, max_bytes: usize, refresh: bool) -> Option<String> {
    let path = upstream_path(filename)?;
    if !refresh {
        if let Some(text) = read_upstream_cache(filename, max_bytes) {
            return Some(text);
        }
    }
    let parent = path.parent()?;
    fs::create_dir_all(parent).ok()?;
    let temporary = parent.join(format!(".{filename}.{}.part", std::process::id()));
    let _ = fs::remove_file(&temporary);

    #[cfg(windows)]
    let executable = crate::support::windows::system32_executable("curl.exe").ok()?;
    #[cfg(not(windows))]
    let executable = PathBuf::from("curl");

    let mut command = Command::new(executable);
    command.args([
        "--location", "--fail", "--silent", "--show-error", "--proto", "=https",
        "--tlsv1.2", "--connect-timeout", "3", "--max-time", "15",
        "--retry", "0", "--max-filesize", &max_bytes.to_string(), "--output",
    ]);
    command.arg(&temporary).arg(format!("{UPSTREAM_BASE}/{filename}"));
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);

    let result = (|| {
        if !command.status().ok()?.success() {
            return None;
        }
        let text = read_text(&temporary, max_bytes)?;
        if !valid_upstream_text(filename, &text) {
            return None;
        }
        // Es una caché de origen, separada de los manuales locales del usuario.
        // Sustituir únicamente después de descargar y validar el archivo completo.
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
        let _ = fs::rename(&temporary, &path);
        Some(text)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn read_local_or_bundled(filename: &str, max_bytes: usize) -> Option<String> {
    if !valid_filename(filename) {
        return None;
    }
    let file = AppPaths::detect().root_dir().join("guide").join(filename);
    crate::adapters::persistence::bundled_guide_asset(filename)
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= max_bytes)
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
        .or_else(|| read_text(&file, max_bytes))
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


#[cfg(test)]
mod arch_package_metadata_tests {
    use super::parse_arch_package_details;

    #[test]
    fn shows_official_description_and_dependencies() {
        let payload = r#"{"results":[{"pkgname":"0ad","pkgver":"0.28.0-3","pkgdesc":"A historically-based real-time strategy game","repo":"extra","arch":"x86_64","url":"https://example.org/0ad","licenses":["GPL-2.0-or-later"],"depends":["sdl2","zlib"]}]}"#;
        let output = parse_arch_package_details("0ad", payload).expect("Arch package details");
        assert!(output.contains("A historically-based real-time strategy game"));
        assert!(output.contains("Dependencias: sdl2, zlib"));
        assert!(output.contains("Licencias: GPL-2.0-or-later"));
        assert!(output.contains("NO instala ni ejecuta paquetes de Arch"));
    }

    #[test]
    fn does_not_assign_a_description_to_a_different_package() {
        let payload = r#"{"results":[{"pkgname":"other","pkgdesc":"Unrelated tool","pkgver":"1","repo":"extra","arch":"x86_64"}]}"#;
        assert!(parse_arch_package_details("0ad", payload).is_none());
    }

    #[test]
    fn missing_description_is_not_fabricated() {
        let payload = r#"{"results":[{"pkgname":"0ad","pkgver":"0.28.0-3","repo":"extra","arch":"x86_64"}]}"#;
        assert!(parse_arch_package_details("0ad", payload).is_none());
    }
}
