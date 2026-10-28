use std::{
    collections::HashMap,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const MANUAL_BASE: &str =
    "https://raw.githubusercontent.com/glitchaka/adm-toolbox/main/tutorials";
const SST_DOC_BASE: &str =
    "https://raw.githubusercontent.com/glitchaka/sst-blueversion/main/docs";
const ARCH_PORTABILITY_DOC: &str = "arch-packages-portability";
const MAX_MANUAL_BYTES: usize = 512 * 1024;

static FILE_CACHE: OnceLock<Mutex<HashMap<&'static str, Option<Arc<str>>>>> = OnceLock::new();
static TOPIC_CACHE: OnceLock<Mutex<HashMap<(ManualKind, String), Option<Arc<str>>>>> =
    OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum ManualKind {
    Whole(&'static str),
    Bash,
    Unix,
}

pub(super) fn load(kind: ManualKind, topic: &str) -> Option<Arc<str>> {
    let topic = canonical_topic(kind, topic);
    let key = (kind, topic.clone());
    let cache = TOPIC_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return value;
    }

    let file = match kind {
        ManualKind::Whole(file) => file,
        ManualKind::Bash => "bash-nwash",
        ManualKind::Unix => "unix-compat",
    };
    let raw = fetch_cached(file)?;
    let value = match kind {
        ManualKind::Whole(_) => Some(raw),
        ManualKind::Bash => extract_bash(&raw, &topic).map(Arc::<str>::from),
        ManualKind::Unix => extract_unix(&raw, &topic).map(Arc::<str>::from),
    };

    cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, value.clone());
    value
}

fn canonical_topic(kind: ManualKind, topic: &str) -> String {
    let topic = topic.trim().to_ascii_lowercase();
    match kind {
        ManualKind::Bash => match topic.as_str() {
            "." => "source".to_owned(),
            "[" => "test".to_owned(),
            "readarray" => "mapfile".to_owned(),
            "typeset" => "declare".to_owned(),
            _ => topic,
        },
        _ => topic,
    }
}

fn fetch_cached(filename: &'static str) -> Option<Arc<str>> {
    let cache = FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(filename)
        .cloned()
    {
        return value;
    }

    // Never hold the mutex while curl is running. A slow network request for one
    // manual must not block access to a manual already present in memory.
    let value = fetch_file(filename).map(Arc::<str>::from);
    cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(filename, value.clone());
    value
}

fn fetch_file(filename: &str) -> Option<String> {
    let url = if filename == ARCH_PORTABILITY_DOC {
        format!("{SST_DOC_BASE}/ARCH_PACKAGES_PORTABILITY.md")
    } else {
        format!("{MANUAL_BASE}/{filename}.txt")
    };

    #[cfg(windows)]
    let curl = crate::support::windows::system32_executable("curl.exe").ok()?;
    #[cfg(not(windows))]
    let curl = std::path::PathBuf::from("curl");

    let mut command = Command::new(curl);
    command
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "2",
            "--max-time",
            "8",
            "--user-agent",
            "SST-Guide/3",
        ])
        .arg(url);

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command.output().ok()?;
    if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > MAX_MANUAL_BYTES {
        return None;
    }
    String::from_utf8(output.stdout).ok()
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

    extract_numbered(raw, topic)
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
