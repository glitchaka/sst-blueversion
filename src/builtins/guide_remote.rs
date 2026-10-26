use std::{
    collections::HashMap,
    process::Command,
    sync::{Mutex, OnceLock},
};

const TUTORIAL_BASE: &str =
    "https://raw.githubusercontent.com/glitchaka/adm-toolbox/main/tutorials";
const MAX_TUTORIAL_BYTES: usize = 512 * 1024;

static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

pub(super) fn catalog() -> &'static [(&'static str, &'static str)] {
    &[
        ("overview", "Inventario global de capacidades de SST."),
        ("shell-invocation", "Opciones de arranque de sst.exe/Nwash."),
        ("network", "notsh/net completo: diagnóstico, scan, monitor, tráfico y proveedores."),
        ("device", "Inventario de dispositivos por MAC/IP."),
        ("switch", "Perfiles SNMP y localización MAC a puerto."),
        ("wol", "Wake-on-LAN por MAC o equipo inventariado."),
        ("diagnostics", "Diagnósticos compuestos de red, hardware, tráfico y dominio."),
        ("system", "sys completo y sus aliases públicos."),
        ("domain-status", "Consulta local/remota de pertenencia a dominio."),
        ("sys-domain", "Administración local de dominio: status, join, leave y OU."),
        ("hostname", "Consulta y cambio de nombre del equipo."),
        ("autologon", "Sysinternals Autologon integrado en SST."),
        ("security", "triage, intel y herramientas de seguridad de sys."),
        ("windows-admin", "EventLog, servicios, Registro, procesos, ACL, PnP, tareas, sesiones, shares, firewall y energía."),
        ("sudo", "Administrador, LOCAL SYSTEM y TrustedInstaller."),
        ("instp", "Gestor de paquetes portables de SST."),
        ("config", "Configuración, fondos, carrusel y rutas SST."),
        ("update", "Estado, búsqueda e instalación de actualizaciones SST."),
        ("tools", "tools/tool/utils y herramientas de mantenimiento."),
        ("office-sara", "OfficeScrubScenario con SaRACmd legacy."),
        ("office-gethelp", "OfficeScrubScenario con GetHelpCmd actual."),
        ("tour", "Tour interactivo y ejemplos empaquetados."),
        ("helix", "Editor Helix-SST, opciones y controles."),
        ("bash-nwash", "Builtins Bash/Nwash y todas sus opciones implementadas."),
        ("unix-compat", "Comandos Unix/GNU-like y sus opciones implementadas."),
        ("guide", "Uso del manual interactivo y reglas de documentación."),
    ]
}

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
        // Direct manual chapter names. These make the Tutoriales category independently
        // navigable even when the reader does not know a command name yet.
        "overview" => "overview",
        "shell-invocation" => "shell-invocation",
        "guide" | "wiki" | "manual" => "guide",
        "network" => "network",
        "device" => "device",
        "switch" => "switch",
        "wol" => "wol",
        "diagnostics" => "diagnostics",
        "domain-status" => "domain-status",
        "sys-domain" | "domain-admin" => "domain",
        "hostname" => "hostname",
        "autologon" => "autologon",
        "system" => "system",
        "security" => "security",
        "windows-admin" => "windows-admin",
        "instp" => "instp",
        "sudo" => "sudo",
        "config" => "config",
        "update" => "update",
        "tools" => "tools",
        "office-sara" => "office-sara",
        "office-gethelp" => "office-gethelp",
        "tour" => "tour",
        "helix" => "helix",
        "bash-nwash" => "bash-nwash",
        "unix-compat" => "unix-compat",

        // Command/alias -> manual chapter mappings.
        "notsh" | "net" => "network",
        "diag" => "diagnostics",

        "domain" => "domain-status",
        "ps" | "top" | "df" | "free" | "whoami" | "uname" | "uptime"
        | "fetch" | "neofetch" | "fastfetch" | "sys" => "system",

        "triage" | "intel" => "security",

        "eventlog" | "service" | "registry" | "process" | "acl" | "pnp" | "task"
        | "session" | "share" | "firewall" | "power" => "windows-admin",

        "runas" => "sudo",
        "tool" | "utils" => "tools",

        "hx" | "helix-sst" => "helix",
        "sst-config" | "reload" | "sst-path" => "config",

        ":" | "." | "[" | "alias" | "bg" | "bind" | "break" | "builtin"
        | "caller" | "cd" | "command" | "compgen" | "complete" | "compopt"
        | "continue" | "declare" | "dirs" | "disown" | "enable" | "eval" | "exec"
        | "exit" | "export" | "fc" | "fg" | "getopts" | "hash" | "help" | "history"
        | "jobs" | "kill" | "let" | "local" | "logout" | "mapfile" | "popd"
        | "read" | "readarray" | "readonly" | "return" | "set" | "shift" | "shopt"
        | "source" | "suspend" | "test" | "times" | "trap" | "typeset" | "ulimit"
        | "umask" | "unalias" | "unset" | "wait" => "bash-nwash",

        // These names are both Bash builtins and Unix-like names. In Nwash the builtin
        // path wins, so Guide follows the effective execution path.
        "echo" | "printf" | "pwd" | "true" | "false" | "type" => "bash-nwash",

        "env" | "clear" | "ls" | "cat" | "head" | "tail" | "grep" | "wc" | "sort"
        | "uniq" | "cut" | "xargs" | "tee" | "less" | "more" | "sed" | "awk"
        | "diff" | "sha256sum" | "base64" | "find" | "basename" | "dirname"
        | "realpath" | "date" | "sleep" | "touch" | "mkdir" | "rm" | "cp" | "mv"
        | "tar" | "gzip" | "gunzip" | "zip" | "unzip" | "which" => "unix-compat",

        _ => return None,
    })
}
