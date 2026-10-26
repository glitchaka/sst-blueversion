use std::{
    collections::HashMap,
    process::Command,
    sync::{Mutex, OnceLock},
};

const TUTORIAL_BASE: &str =
    "https://raw.githubusercontent.com/glitchaka/adm-toolbox/main/tutorials";
const MAX_TUTORIAL_BYTES: usize = 512 * 1024;

static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

pub(super) fn load(topic: &str) -> Option<String> {
    let filename = filename_for(topic)?;
    let key = filename.to_owned();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return value;
    }

    let value = fetch_file(filename);
    cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, value.clone());
    value
}

fn fetch_file(filename: &str) -> Option<String> {
    let url = format!("{TUTORIAL_BASE}/{filename}.txt");

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
            "SST-Guide/1",
        ])
        .arg(url)
        .output()
        .ok()?;

    if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > MAX_TUTORIAL_BYTES {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

fn filename_for(topic: &str) -> Option<&'static str> {
    let topic = topic.trim().to_ascii_lowercase();
    Some(match topic.as_str() {
        "overview" => "overview",
        "shell-invocation" => "shell-invocation",
        "guide" | "wiki" | "manual" => "guide",

        "notsh" | "net" => "network",
        "device" => "device",
        "switch" => "switch",
        "wol" => "wol",
        "diag" => "diagnostics",

        "domain" => "domain-status",
        "sys-domain" => "domain",
        "hostname" => "hostname",
        "autologon" => "autologon",
        "sys" | "ps" | "top" | "df" | "free" | "whoami" | "uname" | "uptime"
        | "fetch" | "neofetch" | "fastfetch" => "system",

        "triage" | "intel" => "security",

        "eventlog" | "service" | "registry" | "process" | "acl" | "pnp" | "task"
        | "session" | "share" | "firewall" | "power" => "windows-admin",

        "instp" => "instp",
        "sudo" | "runas" => "sudo",
        "update" => "update",
        "tools" | "tool" | "utils" => "tools",
        "office-sara" => "office-sara",
        "office-gethelp" => "office-gethelp",

        "helix" | "hx" | "helix-sst" => "helix",
        "sst-config" | "config" | "reload" | "sst-path" => "config",
        "tour" => "tour",

        ":" | "." | "[" | "alias" | "bg" | "bind" | "break" | "builtin"
        | "caller" | "cd" | "command" | "compgen" | "complete" | "compopt"
        | "continue" | "declare" | "dirs" | "disown" | "enable" | "eval" | "exec"
        | "exit" | "export" | "fc" | "fg" | "getopts" | "hash" | "help" | "history"
        | "jobs" | "kill" | "let" | "local" | "logout" | "mapfile" | "popd"
        | "read" | "readarray" | "readonly" | "return" | "set" | "shift" | "shopt"
        | "source" | "suspend" | "test" | "times" | "trap" | "typeset" | "ulimit"
        | "umask" | "unalias" | "unset" | "wait" => "bash-nwash",

        // These names are both Bash builtins and Unix-like commands in SST. Guide's local
        // command table treats them as Bash/Nwash first, so the remote chapter follows that.
        "echo" | "printf" | "pwd" | "true" | "false" | "type" => "bash-nwash",

        "env" | "clear" | "ls" | "cat" | "head" | "tail" | "grep" | "wc" | "sort"
        | "uniq" | "cut" | "xargs" | "tee" | "less" | "more" | "sed" | "awk"
        | "diff" | "sha256sum" | "base64" | "find" | "basename" | "dirname"
        | "realpath" | "date" | "sleep" | "touch" | "mkdir" | "rm" | "cp" | "mv"
        | "tar" | "gzip" | "gunzip" | "zip" | "unzip" | "which" => "unix-compat",

        _ => return None,
    })
}
