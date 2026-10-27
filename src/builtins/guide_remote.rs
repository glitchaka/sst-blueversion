use std::{
    collections::HashMap,
    process::Command,
    sync::{Mutex, OnceLock},
};

const MANUAL_BASE: &str =
    "https://raw.githubusercontent.com/glitchaka/adm-toolbox/main/tutorials";
const MAX_MANUAL_BYTES: usize = 512 * 1024;

static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

#[derive(Clone, Copy)]
enum Section {
    Whole,
    Bash(&'static str),
    Unix(&'static str),
}

#[derive(Clone, Copy)]
struct ManualSource {
    file: &'static str,
    section: Section,
}

pub(super) fn load(topic: &str) -> Option<String> {
    let source = source_for(topic)?;
    let raw = fetch_cached(source.file)?;
    let extracted = match source.section {
        Section::Whole => raw,
        Section::Bash(name) => extract_bash(&raw, name).unwrap_or(raw),
        Section::Unix(name) => extract_unix(&raw, name).unwrap_or(raw),
    };
    Some(extracted)
}

fn fetch_cached(filename: &str) -> Option<String> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(filename)
        .cloned()
    {
        return value;
    }

    let value = fetch_file(filename);
    cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(filename.to_owned(), value.clone());
    value
}

fn fetch_file(filename: &str) -> Option<String> {
    let url = format!("{MANUAL_BASE}/{filename}.txt");

    #[cfg(windows)]
    let curl = crate::support::windows::system32_executable("curl.exe").ok()?;
    #[cfg(not(windows))]
    let curl = std::path::PathBuf::from("curl");

    let output = Command::new(curl)
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "--user-agent",
            "SST-Guide/2",
        ])
        .arg(url)
        .output()
        .ok()?;

    if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > MAX_MANUAL_BYTES {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn whole(file: &'static str) -> ManualSource {
    ManualSource {
        file,
        section: Section::Whole,
    }
}

fn bash(name: &'static str) -> ManualSource {
    ManualSource {
        file: "bash-nwash",
        section: Section::Bash(name),
    }
}

fn unix(name: &'static str) -> ManualSource {
    ManualSource {
        file: "unix-compat",
        section: Section::Unix(name),
    }
}

fn source_for(topic: &str) -> Option<ManualSource> {
    let topic = topic.trim().to_ascii_lowercase();
    Some(match topic.as_str() {
        "sys" | "ps" | "top" | "df" | "free" | "whoami" | "uname" | "uptime"
        | "fetch" | "neofetch" | "fastfetch" => whole("system"),
        "hostname" | "sys hostname" => whole("hostname"),
        "sys domain" | "domain-admin" => whole("domain"),
        "sys autologon" | "autologon" => whole("autologon"),

        "notsh" | "net" => whole("network"),
        "device" => whole("device"),
        "switch" => whole("switch"),
        "wol" => whole("wol"),
        "diag" => whole("diagnostics"),

        "domain" => whole("domain-status"),
        "eventlog" => whole("eventlog"),
        "service" => whole("service"),
        "registry" => whole("registry"),
        "process" => whole("process"),
        "acl" => whole("acl"),
        "pnp" => whole("pnp"),
        "task" => whole("task"),
        "session" => whole("session"),
        "share" => whole("share"),
        "firewall" => whole("firewall"),
        "power" => whole("power"),

        "triage" => whole("triage"),
        "intel" => whole("intel"),
        "sudo" | "runas" => whole("sudo"),
        "tools" | "tool" | "utils" => whole("tools"),
        "tools office diagnose" | "tools office status" | "tools office verify" => whole("office-diagnose"),
        "tools office auth repair" => whole("office-auth-repair"),
        "tools office scrub" | "tools office sara" | "tools office uninstall" | "tools office remove" => whole("office-sara"),
        "tools office scrub-current" | "tools office gethelp" => whole("office-gethelp"),
        "instp" => whole("instp"),
        "config" | "reload" | "sst-path" | "sst-config" => whole("config"),
        "update" => whole("update"),
        "helix" | "hx" | "helix-sst" => whole("helix"),

        "." => bash("source"),
        "[" => bash("test"),
        "readarray" => bash("mapfile"),
        "typeset" => bash("declare"),
        ":" => bash(":"),
        "alias" => bash("alias"),
        "bg" => bash("bg"),
        "bind" => bash("bind"),
        "break" => bash("break"),
        "builtin" => bash("builtin"),
        "caller" => bash("caller"),
        "cd" => bash("cd"),
        "command" => bash("command"),
        "compgen" => bash("compgen"),
        "complete" => bash("complete"),
        "compopt" => bash("compopt"),
        "continue" => bash("continue"),
        "declare" => bash("declare"),
        "dirs" => bash("dirs"),
        "disown" => bash("disown"),
        "echo" => bash("echo"),
        "enable" => bash("enable"),
        "eval" => bash("eval"),
        "exec" => bash("exec"),
        "exit" => bash("exit"),
        "export" => bash("export"),
        "false" => bash("false"),
        "fc" => bash("fc"),
        "fg" => bash("fg"),
        "getopts" => bash("getopts"),
        "hash" => bash("hash"),
        "help" => bash("help"),
        "history" => bash("history"),
        "jobs" => bash("jobs"),
        "kill" => bash("kill"),
        "let" => bash("let"),
        "local" => bash("local"),
        "logout" => bash("logout"),
        "mapfile" => bash("mapfile"),
        "popd" => bash("popd"),
        "printf" => bash("printf"),
        "pushd" => bash("pushd"),
        "pwd" => bash("pwd"),
        "read" => bash("read"),
        "readonly" => bash("readonly"),
        "return" => bash("return"),
        "set" => bash("set"),
        "shift" => bash("shift"),
        "shopt" => bash("shopt"),
        "source" => bash("source"),
        "suspend" => bash("suspend"),
        "test" => bash("test"),
        "times" => bash("times"),
        "trap" => bash("trap"),
        "true" => bash("true"),
        "type" => bash("type"),
        "ulimit" => bash("ulimit"),
        "umask" => bash("umask"),
        "unalias" => bash("unalias"),
        "unset" => bash("unset"),
        "wait" => bash("wait"),

        "env" => unix("env"),
        "clear" => unix("clear"),
        "ls" => unix("ls"),
        "cat" => unix("cat"),
        "head" => unix("head"),
        "tail" => unix("tail"),
        "grep" => unix("grep"),
        "wc" => unix("wc"),
        "sort" => unix("sort"),
        "uniq" => unix("uniq"),
        "cut" => unix("cut"),
        "xargs" => unix("xargs"),
        "tee" => unix("tee"),
        "less" => unix("less"),
        "more" => unix("more"),
        "sed" => unix("sed"),
        "awk" => unix("awk"),
        "diff" => unix("diff"),
        "sha256sum" => unix("sha256sum"),
        "base64" => unix("base64"),
        "find" => unix("find"),
        "basename" => unix("basename"),
        "dirname" => unix("dirname"),
        "realpath" => unix("realpath"),
        "date" => unix("date"),
        "sleep" => unix("sleep"),
        "touch" => unix("touch"),
        "mkdir" => unix("mkdir"),
        "rm" => unix("rm"),
        "cp" => unix("cp"),
        "mv" => unix("mv"),
        "tar" => unix("tar"),
        "gzip" => unix("gzip"),
        "gunzip" => unix("gunzip"),
        "zip" => unix("zip"),
        "unzip" => unix("unzip"),
        "which" => unix("which"),
        _ => return None,
    })
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
                    && (is_numbered_heading(candidate)
                        || is_bash_builtin_name(candidate.trim()))
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
