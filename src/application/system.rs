use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::Write as _,
    mem::size_of,
    net::IpAddr,
    process::Command,
    sync::Arc,
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use sysinfo::{Disks, Pid, System};
use crate::application::keygen_audio::VisualBeat;

#[path = "keygen_mesh.rs"]
mod keygen_mesh;
#[path = "keygen_demo.rs"]
mod keygen_demo;

pub(crate) const SST_TERMINAL_FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"), "/JetBrainsMonoNerdFontMono-Regular.ttf"
));

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError},
    Graphics::Printing::{
        EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
        PRINTER_INFO_2W,
    },
    System::Threading::{
        GetExitCodeProcess, OpenProcess, TerminateProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    },
};

use crate::{
    core::{
        CommandOutput,
        ports::{TerminalFactory, TerminalKey},
    },
    support::options,
    application::security::SecurityTriageService,
};

pub struct SystemService {
    terminal: Arc<dyn TerminalFactory>,
    security: Arc<SecurityTriageService>,
}

impl SystemService {
    pub fn new(
        terminal: Arc<dyn TerminalFactory>,
        security: Arc<SecurityTriageService>,
    ) -> Self {
        Self { terminal, security }
    }

    pub fn execute(&self, args: &[String]) -> anyhow::Result<CommandOutput> {
        let sub = args.first().map(String::as_str).unwrap_or("overview");

        match sub {
            "overview" => Ok(system_help()),
            "info" => info(),
            "processes" | "ps" => processes(&args[1..]),
            "top" => self.top(&args[1..]),
            "disks" | "df" => disks(),
            "memory" | "free" => memory(),
            "sensors" | "temperature" | "temperatures" | "thermal" => Ok(crate::application::thermal::report()),
            "hostname" => hostname(),
            "whoami" => whoami(),
            "uname" => uname(&args[1..]),
            "kill" if args[1..].iter().any(|arg| arg == "--broker") => broker_process(crate::core::broker::Operation::Kill, &args[1..]),
            "kill" => kill_process(&args[1..]),
            "fetch" | "neofetch" | "fastfetch" => fetch(&args[1..]),
            "keygen" => self.keygen(&args[1..]),
            "uptime" => uptime(),
            "services" if args[1..].iter().any(|arg| arg == "--impact") => {
                self.security.services_impact()
            }
            "services" => services(&args[1..]),
            "startup" => self.security.startup(),
            "inspect" if args[1..].iter().any(|arg| arg == "--broker") => broker_process(crate::core::broker::Operation::Inspect, &args[1..]),
            "inspect" => self.security.inspect(&args[1..]),
            "why" => self.security.why(&args[1..]),
            "diff" => self.security.diff(&args[1..]),
            "suspicious" => self.security.suspicious(),
            "safe" => self.security.safe(&args[1..]),
            "persistence" => self.security.persistence(),
            "suspend" => broker_process(crate::core::broker::Operation::Suspend, &args[1..]),
            "resume" => broker_process(crate::core::broker::Operation::Resume, &args[1..]),
            "users" => users(&args[1..]),
            "printers" => printers(&args[1..]),
            "printer" => printer(&args[1..]),
            "drivers" => drivers(&args[1..]),
            "events" => events(&args[1..]),
            "registry" | "reg" => registry(&args[1..]),
            "tasks" | "scheduled-tasks" => scheduled_tasks(&args[1..]),
            "help" | "-h" | "--help" => Ok(system_help()),
            _ => Ok(CommandOutput::error(
                format!("sys: subcomando desconocido: {sub}\nusa 'sys --help' para ver los subcomandos disponibles"),
                2,
            )),
        }
    }

    pub fn execute_alias(&self, name: &str, args: &[String]) -> anyhow::Result<CommandOutput> {
        match name {
            "ps" => processes(args),
            "top" => self.top(args),
            "df" => disks(),
            "free" => memory(),
            "hostname" => hostname(),
            "whoami" => whoami(),
            "uname" => uname(args),
            "kill" => kill_process(args),
            "uptime" => uptime(),
            "fetch" | "neofetch" | "fastfetch" => fetch(args),
            "keygen" => self.keygen(args),
            _ => Ok(CommandOutput::error(
                format!("comando de sistema desconocido: {name}"),
                127,
            )),
        }
    }

    fn keygen(&self, args: &[String]) -> anyhow::Result<CommandOutput> {
        if args.iter().any(|arg| matches!(arg.as_str(), "--help" | "-h")) {
            return Ok(CommandOutput::ok(
                "keygen — intro animada y banda sonora MP3 SST\n\
                 Reproduce automáticamente los dos temas en bucle desde data/.sst/media/keygen/.\n\
                 1/2: seleccionar tema | Espacio: pausa/reanuda | Esc/q: salir\n",
            ));
        }
        let first = match args {
            [] => 0,
            [flag, value] if flag == "--track" && matches!(value.as_str(), "1" | "2") =>
                value.parse::<usize>()?.saturating_sub(1),
            _ => return Ok(CommandOutput::error(
                "keygen: uso: keygen [--track 1|2] | keygen --help", 2)),
        };
        // Start sound before taking over the screen. A missing MP3 reports a
        // useful error at the normal prompt, not a mute keygen animation.
        let music = crate::application::keygen_audio::KeygenMusic::new(first)?;
        keygen_sdl(music)
    }

    fn top(&self, args: &[String]) -> anyhow::Result<CommandOutput> {
        if args.iter().any(|arg| matches!(arg.as_str(), "--help" | "-h")) {
            return Ok(CommandOutput::ok(
                "sys top [NOMBRE | --find NOMBRE]\n\
                 c: ordenar por CPU | m: RAM | t: árbol | flechas/PgUp/PgDown: mover | q/Esc: salir\n\
                 Ejemplo: sys top outlook\n",
            ));
        }
        let filter = match args {
            [] => None,
            [name] if !name.starts_with('-') && !name.trim().is_empty() => Some(name.as_str()),
            [flag, name] if flag == "--find" && !name.starts_with('-') && !name.trim().is_empty() => Some(name.as_str()),
            _ => return Ok(CommandOutput::error("sys top: uso: sys top [NOMBRE | --find NOMBRE]", 2)),
        };
        let mut terminal = self.terminal.alternate_screen()?;
        let mut sort_cpu = true;
        let mut tree = false;
        let mut offset = 0usize;
        // Two samples initialize meaningful CPU utilization, and then the same
        // sampler is refreshed after each 900-ms frame.
        let mut system = sampled_processes();

        loop {
            let mut rows = if tree {
                process_tree_rows(&system, sort_cpu)
            } else {
                let mut pids = system.processes().keys().copied().collect::<Vec<_>>();
                sort_process_pids(&system, &mut pids, sort_cpu);
                pids.into_iter().map(|pid| (pid, String::new())).collect::<Vec<_>>()
            };
            if let Some(query) = filter {
                rows.retain(|(pid, _)| process_matches_filter(&system, *pid, query));
            }
            let (columns, height) = terminal.size()?;
            // Leave a column free to prevent the terminal's automatic wrap.
            let width = usize::from(columns.saturating_sub(1)).max(1);
            let max_rows = usize::from(height.saturating_sub(7));
            offset = offset.min(rows.len().saturating_sub(max_rows.max(1)));
            let mut screen = String::new();

            top_screen_line(
                &mut screen,
                "  SST  /  MONITOR DE PROCESOS",
                width,
                "\x1b[1;38;2;235;219;178m",
                "\x1b[48;2;60;56;54m",
            );
            let used = system.used_memory() / 1024 / 1024;
            let total = system.total_memory() / 1024 / 1024;
            top_screen_line(
                &mut screen,
                &format!(
                    "  Procesos: {}   ·   CPU global: {:.1}%   ·   RAM: {} / {} MiB   ·   Uptime: {}m",
                    system.processes().len(),
                    system.global_cpu_usage(),
                    used,
                    total,
                    System::uptime() / 60
                ),
                width,
                "\x1b[38;2;235;219;178m",
                "\x1b[48;2;40;40;40m",
            );
            top_screen_line(
                &mut screen,
                &format!(
                    "  {}  ·  Orden: {}  ·  [c] CPU [m] RAM [t] Árbol [↑↓] mover [q] salir",
                    filter.map(|v| format!("Filtro: {v}")).unwrap_or_else(|| "Todos los procesos".to_owned()),
                    if sort_cpu { "CPU" } else { "RAM" }
                ),
                width,
                "\x1b[38;2;189;174;147m",
                "\x1b[48;2;40;40;40m",
            );
            top_screen_line(
                &mut screen,
                "",
                width,
                "\x1b[38;2;235;219;178m",
                "\x1b[48;2;29;32;33m",
            );
            top_screen_line(
                &mut screen,
                "  PID      PPID     CPU %     RAM MiB   PROCESO / JERARQUÍA",
                width,
                "\x1b[1;38;2;250;189;47m",
                "\x1b[48;2;60;56;54m",
            );

            let mut shown = 0usize;
            for (pid, tree_prefix) in rows.iter().skip(offset).take(max_rows) {
                let Some(process) = system.process(*pid) else { continue; };
                let ppid = process.parent().map(|p| p.as_u32()).unwrap_or(0);
                let mut name = format!("{tree_prefix}{}", process.name().to_string_lossy());
                let name_width = width.saturating_sub(43);
                if name.chars().count() > name_width && name_width > 3 {
                    name = name.chars().take(name_width - 1).collect();
                    name.push('…');
                }

                let cpu = process.cpu_usage();
                let memory = process.memory() as f64 / 1024.0 / 1024.0;
                let content = format!(
                    "  {:<8} {:<8} {:>6.1} {:>11.1}   {}",
                    pid, ppid, cpu, memory, name
                );
                let foreground = if cpu >= 50.0 {
                    "\x1b[1;38;2;251;73;52m"
                } else if memory >= 1024.0 {
                    "\x1b[38;2;184;187;38m"
                } else {
                    "\x1b[38;2;235;219;178m"
                };
                let background = if shown % 2 == 0 {
                    "\x1b[48;2;29;32;33m"
                } else {
                    "\x1b[48;2;40;40;40m"
                };
                top_screen_line(&mut screen, &content, width, foreground, background);
                shown += 1;
            }
            // Draw a full opaque alternate screen; the desktop wallpaper must
            // not obscure process names, headings, or empty parts of the table.
            for _ in shown..max_rows {
                top_screen_line(
                    &mut screen, "", width,
                    "\x1b[38;2;235;219;178m",
                    "\x1b[48;2;29;32;33m",
                );
            }
            top_screen_line(
                &mut screen, "", width,
                "\x1b[38;2;235;219;178m",
                "\x1b[48;2;29;32;33m",
            );
            top_screen_line(
                &mut screen,
                "  SST top   ·   Actualización automática de métricas   ·   q para cerrar",
                width,
                "\x1b[38;2;189;174;147m",
                "\x1b[48;2;60;56;54m",
            );

            terminal.clear()?;
            terminal.write(&screen)?;
            terminal.flush()?;

            match terminal.poll_key(Duration::from_millis(900))? {
                Some(TerminalKey::Char('q') | TerminalKey::Escape) => break,
                Some(TerminalKey::Char('c')) => { sort_cpu = true; offset = 0; }
                Some(TerminalKey::Char('m')) => { sort_cpu = false; offset = 0; }
                Some(TerminalKey::Char('t')) => { tree = !tree; offset = 0; }
                Some(TerminalKey::Up) => offset = offset.saturating_sub(1),
                Some(TerminalKey::Down) => offset = (offset + 1).min(rows.len().saturating_sub(max_rows.max(1))),
                Some(TerminalKey::PageUp) => offset = offset.saturating_sub(max_rows.max(1)),
                Some(TerminalKey::PageDown) => offset = offset.saturating_add(max_rows.max(1)).min(rows.len().saturating_sub(max_rows.max(1))),
                Some(TerminalKey::Home) => offset = 0,
                Some(TerminalKey::End) => offset = rows.len().saturating_sub(max_rows.max(1)),
                _ => {}
            }
            system.refresh_all();
        }

        Ok(CommandOutput::ok(""))
    }
}

/// Style a whole terminal row, including empty cells. This ensures the
/// alternate-screen TUI has readable contrast independently of the wallpaper.
fn top_screen_line(
    buffer: &mut String,
    text: &str,
    width: usize,
    foreground: &str,
    background: &str,
) {
    let visible: String = text.chars().take(width).collect();
    buffer.push_str(background);
    buffer.push_str(foreground);
    buffer.push_str(&visible);
    buffer.push_str(&" ".repeat(width.saturating_sub(visible.chars().count())));
    buffer.push_str("\x1b[0m\r\n");
}

fn system_help() -> CommandOutput {
    CommandOutput::ok(
        "sys — información y administración local de Windows\n\n\
         uso: sys SUBCOMANDO [opciones]\n\n\
         Información:\n\
           sys info                     resumen del equipo\n\
           sys processes [NOMBRE]       buscar o listar procesos por CPU/RAM\n\
           sys top [NOMBRE]             monitor TUI filtrable por CPU/RAM\n\
           sys disks                    discos y uso\n\
           sys memory                   memoria y swap\n\
           sys sensors                  temperaturas de hardware disponibles\n\
           sys uptime                   tiempo desde el último arranque\n\
           sys hostname                 nombre del equipo\n\
           sys whoami                   usuario actual\n\
           sys uname [-a]               identificación del sistema\n\
           sys kill PID [--tree]         termina un proceso o su árbol\n\
           sys inspect PID --broker     consulta mediante broker autenticado\n\
           sys suspend PID --start-time FILETIME\n\
           sys resume PID --start-time FILETIME\n\
           sys kill PID --broker --start-time FILETIME\n\
                                        broker; requiere token admin para modificar\n\n\
         Administración / auditoría de solo lectura:\n\
           sys services [NOMBRE]        servicios de Windows\n\
           sys users [USUARIO]          cuentas locales (o --domain)\n\
           sys printers                 impresoras, servidor, puerto e IP\n\
           sys printers --default       impresora predeterminada\n\
           sys printer [NOMBRE]         detalle de una impresora\n\
           sys printer NOMBRE --ip      servidor, puerto e IP\n\
           sys drivers                  controladores instalados\n\
           sys drivers --pnp            paquetes de controladores PnP\n\
           sys drivers --devices        dispositivos PnP conectados\n\
           sys events [LOG]             últimos eventos (System por defecto)\n\
           sys registry CLAVE           consulta del Registro de Windows\n\
           sys tasks [NOMBRE]           tareas programadas\n\n\
         Estas vistas usan las utilidades nativas de Windows como backend y no modifican\n\
         servicios, cuentas, registro, drivers, eventos ni tareas programadas.\n",
    )
}

fn uptime() -> anyhow::Result<CommandOutput> {
    let seconds = System::uptime();
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    Ok(CommandOutput::ok(format!(
        "uptime: {days}d {hours}h {minutes}m {secs}s\nseconds: {seconds}\n"
    )))
}

fn services(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys services — consulta servicios de Windows\n\
             uso:\n\
               sys services              todos los servicios\n\
               sys services --running    solo servicios activos\n\
               sys services --stopped    servicios detenidos\n\
               sys services NOMBRE       detalle de un servicio\n",
        ));
    }

    let mut native = vec!["query".to_owned()];
    if let Some(name) = args.first().filter(|arg| !arg.starts_with('-')) {
        native.push(name.clone());
    } else if options::has(args, "--running") {
        native.push("state=".to_owned());
        native.push("active".to_owned());
    } else if options::has(args, "--stopped") {
        native.push("state=".to_owned());
        native.push("inactive".to_owned());
    } else {
        native.push("state=".to_owned());
        native.push("all".to_owned());
    }

    run_windows_tool("sc.exe", &native)
}

fn users(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys users — cuentas y perfiles (objetos distintos)\n\
             sys users                 cuentas locales en SAM (net user)\n\
             sys users NOMBRE          detalles de una cuenta local\n\
             sys users --domain        cuentas del dominio\n\
             sys users --profiles      carpetas de perfiles en Windows (no equivale a cuentas)\n",
        ));
    }

    if options::has(args, "--profiles") {
        if args.len() != 1 {
            return Ok(CommandOutput::error(
                "sys users --profiles no acepta nombres ni --domain", 2,
            ));
        }
        return user_profile_directories();
    }
    if args.iter().any(|arg| arg.starts_with('-') && arg != "--domain") ||
        args.iter().filter(|arg| !arg.starts_with('-')).count() > 1
    {
        return Ok(CommandOutput::error("sys users: argumentos incorrectos", 2));
    }
    let mut native = vec!["user".to_owned()];
    if let Some(name) = args.first().filter(|arg| !arg.starts_with('-')) {
        native.push(name.clone());
    }
    if options::has(args, "--domain") {
        native.push("/domain".to_owned());
    }
    // net user reports SAM/AD account objects. Do not equate its output with
    // on-disk C:\\Users profile folders, which may be orphaned or relocated.
    run_windows_tool("net.exe", &native)
}

fn user_profile_directories() -> anyhow::Result<CommandOutput> {
    use std::path::PathBuf;
    let root = env::var_os("PUBLIC")
        .map(PathBuf::from)
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .or_else(|| env::var_os("SystemDrive").map(|drive| PathBuf::from(drive).join("Users")))
        .unwrap_or_else(|| PathBuf::from(r"C:\Users"));
    let mut profiles = std::fs::read_dir(&root)?
        .filter_map(|entry| entry.ok())
        .filter_map(|item| {
            let kind = item.file_type().ok()?;
            kind.is_dir().then_some(item.file_name().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    profiles.sort_by_key(|name| name.to_ascii_lowercase());
    let mut out = format!(
        "CARPETAS DE PERFILES — {}\n         Estas carpetas NO equivalen a cuentas activas de Windows.\n         La lista de cuentas SAM se consulta con sys users.\n\n",
        root.display(),
    );
    if profiles.is_empty() {
        out.push_str("(No hay subcarpetas visibles)\n");
    } else {
        for name in profiles {
            writeln!(&mut out, "{name}")?;
        }
    }
    Ok(CommandOutput::ok(out))
}


#[derive(Debug, Clone)]
struct PrinterRow {
    name: String,
    server: String,
    share: String,
    port: String,
    driver: String,
    location: String,
    is_default: bool,
    ip: Option<String>,
}

fn printers(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys printers — impresoras instaladas en el usuario/equipo\n\
             uso:\n\
               sys printers             todas las impresoras instaladas\n\
               sys printers NOMBRE      detalle de la impresora indicada\n\
               sys printers --default   sólo la impresora predeterminada\n\
               sys printers --csv       exportación CSV estructurada\n\
\n\
             Muestra cola, servidor de impresión, recurso compartido, puerto e IP cuando\n\
             el puerto TCP/IP o la ubicación WSD permiten resolverla. No usa PowerShell.\n",
        ));
    }

    for arg in args {
        if arg.starts_with('-') && !matches!(arg.as_str(), "--default" | "--csv") {
            return Ok(CommandOutput::error(
                format!("sys printers: opción desconocida: {arg}"), 2,
            ));
        }
    }

    // Si llega un nombre, nunca caer al listado general: delegar en la misma
    // ruta de detalle usada por `sys printer NOMBRE`. Esto evita la regresión
    // donde el argumento se ignoraba y se mostraban todas las impresoras.
    if args.iter().any(|arg| !arg.starts_with('-')) {
        return printer(args);
    }

    let only_default = options::has(args, "--default");
    let mut rows = enumerate_printers()?;
    if only_default {
        rows.retain(|row| row.is_default);
    }

    let csv = options::has(args, "--csv");
    if rows.is_empty() && csv {
        return Ok(CommandOutput::ok(
            "Nombre,Predeterminada,Servidor,RecursoCompartido,Puerto,IP\n"
        ));
    }
    if rows.is_empty() {
        return Ok(CommandOutput::ok(if only_default {
            "No hay una impresora predeterminada configurada.\n"
        } else {
            "No hay impresoras instaladas para el usuario/equipo actual.\n"
        }));
    }

    rows.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });

    if csv {
        let mut out = String::from(
            "Nombre,Predeterminada,Servidor,RecursoCompartido,Puerto,IP\n"
        );
        for row in rows {
            let columns = [
                row.name,
                row.is_default.to_string(),
                row.server,
                row.share,
                row.port,
                row.ip.unwrap_or_default(),
            ];
            out.push_str(&columns.iter()
                .map(|field| crate::support::csv::escape(field))
                .collect::<Vec<_>>().join(","));
            out.push('\n');
        }
        return Ok(CommandOutput::ok(out));
    }

    let mut out = String::from(
        "IMPRESORAS\n----------------------------------------------------------------------------------------------------\n",
    );
    for row in rows {
        writeln!(
            out,
            "{} {}\n  servidor: {}\n  compartida: {}\n  puerto: {}\n  IP: {}\n",
            if row.is_default { "*" } else { " " },
            row.name,
            display_or_dash(&row.server),
            display_or_dash(&row.share),
            display_or_dash(&row.port),
            row.ip.as_deref().unwrap_or("<no resuelta>"),
        )?;
    }
    Ok(CommandOutput::ok(out))
}

fn printer(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys printer — detalle de una impresora\n\
             uso:\n\
               sys printer                  impresora predeterminada\n\
               sys printer NOMBRE           detalle por nombre o recurso compartido\n\
               sys printer NOMBRE --ip      muestra servidor, puerto e IP\n",
        ));
    }

    let ip_only = options::has(args, "--ip");
    let requested = args
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    let requested = requested.trim();

    let rows = enumerate_printers()?;
    let selected = if requested.is_empty() {
        rows.into_iter().find(|row| row.is_default)
    } else {
        find_printer(rows, requested)
    };

    let Some(row) = selected else {
        return Ok(CommandOutput::error(
            if requested.is_empty() {
                "sys printer: no hay una impresora predeterminada configurada".to_owned()
            } else {
                format!("sys printer: no se encontró '{requested}'")
            },
            1,
        ));
    };

    if ip_only {
        return Ok(CommandOutput::ok(format!(
            "Impresora: {}\nServidor:  {}\nPuerto:    {}\nIP:        {}\n",
            row.name,
            display_or_dash(&row.server),
            display_or_dash(&row.port),
            row.ip.as_deref().unwrap_or("<no resuelta>"),
        )));
    }

    Ok(CommandOutput::ok(format!(
        "Impresora\n------------------------------------------------------------\n\
         Nombre:      {}\n\
         Predeterminada: {}\n\
         Servidor:    {}\n\
         Compartida:  {}\n\
         Puerto:      {}\n\
         IP:          {}\n\
         Driver:      {}\n\
         Ubicación:   {}\n",
        row.name,
        if row.is_default { "sí" } else { "no" },
        display_or_dash(&row.server),
        display_or_dash(&row.share),
        display_or_dash(&row.port),
        row.ip.as_deref().unwrap_or("<no resuelta>"),
        display_or_dash(&row.driver),
        display_or_dash(&row.location),
    )))
}

fn find_printer(rows: Vec<PrinterRow>, requested: &str) -> Option<PrinterRow> {
    let wanted = requested.trim().to_ascii_lowercase();
    rows.into_iter().find(|row| {
        row.name.eq_ignore_ascii_case(requested)
            || (!row.share.is_empty() && row.share.eq_ignore_ascii_case(requested))
            || row
                .name
                .rsplit('\\')
                .next()
                .is_some_and(|tail| tail.to_ascii_lowercase() == wanted)
    })
}

fn display_or_dash(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

#[cfg(windows)]
fn enumerate_printers() -> anyhow::Result<Vec<PrinterRow>> {
    let default_name = default_printer_name()?;
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed = 0u32;
    let mut returned = 0u32;

    unsafe {
        let _ = EnumPrintersW(
            flags,
            std::ptr::null(),
            2,
            std::ptr::null_mut(),
            0,
            &mut needed,
            &mut returned,
        );
    }

    if needed == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; needed as usize];
    let ok = unsafe {
        EnumPrintersW(
            flags,
            std::ptr::null(),
            2,
            buffer.as_mut_ptr(),
            needed,
            &mut needed,
            &mut returned,
        )
    };
    if ok == 0 {
        anyhow::bail!(
            "sys printers: EnumPrintersW falló con error Win32 {}",
            unsafe { GetLastError() }
        );
    }

    let mut rows = Vec::with_capacity(returned as usize);
    for index in 0..returned as usize {
        let info = unsafe {
            std::ptr::read_unaligned(
                buffer
                    .as_ptr()
                    .add(index * size_of::<PRINTER_INFO_2W>())
                    .cast::<PRINTER_INFO_2W>(),
            )
        };

        let name = unsafe { wide_ptr_string(info.pPrinterName) };
        let mut server = unsafe { wide_ptr_string(info.pServerName) };
        let mut share = unsafe { wide_ptr_string(info.pShareName) };
        let port = unsafe { wide_ptr_string(info.pPortName) };
        let driver = unsafe { wide_ptr_string(info.pDriverName) };
        let location = unsafe { wide_ptr_string(info.pLocation) };

        if name.starts_with(r"\\") {
            let mut parts = name.trim_start_matches('\\').splitn(2, '\\');
            if server.is_empty() {
                if let Some(host) = parts.next() {
                    server = format!(r"\\{host}");
                }
            } else {
                let _ = parts.next();
            }
            if share.is_empty() {
                if let Some(queue) = parts.next() {
                    share = queue.to_owned();
                }
            }
        }

        let ip = resolve_printer_ip(&port)
            .or_else(|| resolve_printer_location_ip(&location));
        let is_default = default_name
            .as_deref()
            .is_some_and(|default| default.eq_ignore_ascii_case(&name));

        rows.push(PrinterRow {
            name,
            server,
            share,
            port,
            driver,
            location,
            is_default,
            ip,
        });
    }
    Ok(rows)
}

#[cfg(not(windows))]
fn enumerate_printers() -> anyhow::Result<Vec<PrinterRow>> {
    anyhow::bail!("sys printers sólo está disponible en Windows")
}

#[cfg(windows)]
fn default_printer_name() -> anyhow::Result<Option<String>> {
    let mut chars = 0u32;
    unsafe {
        let _ = GetDefaultPrinterW(std::ptr::null_mut(), &mut chars);
    }
    if chars == 0 {
        return Ok(None);
    }

    let mut buffer = vec![0u16; chars as usize];
    let ok = unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &mut chars) };
    if ok == 0 {
        anyhow::bail!(
            "sys printers: GetDefaultPrinterW falló con error Win32 {}",
            unsafe { GetLastError() }
        );
    }

    let len = buffer.iter().position(|ch| *ch == 0).unwrap_or(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..len])))
}

#[cfg(windows)]
unsafe fn wide_ptr_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

fn resolve_printer_location_ip(location: &str) -> Option<String> {
    let raw = location.trim();
    if raw.is_empty() {
        return None;
    }

    let authority = raw
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(raw)
        .split('/')
        .next()
        .unwrap_or("");

    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map(|(host, _)| host).unwrap_or(rest)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if port.parse::<u16>().is_ok() { host } else { authority }
    } else {
        authority
    };

    resolve_printer_ip(host)
}

fn resolve_printer_ip(port: &str) -> Option<String> {
    let raw = port.trim();
    if raw.is_empty() {
        return None;
    }

    for candidate in [raw, raw.strip_prefix("IP_").unwrap_or(raw)] {
        if let Ok(ip) = candidate.parse::<IpAddr>() {
            return Some(ip.to_string());
        }
    }

    let upper = raw.to_ascii_uppercase();
    if upper.starts_with("WSD-")
        || upper.starts_with("USB")
        || upper.starts_with("LPT")
        || upper.starts_with("COM")
        || upper == "FILE:"
        || upper == "PORTPROMPT:"
        || raw.starts_with(r"\\")
    {
        return None;
    }

    // Muchos servidores usan el hostname/DNS de la impresora como nombre de
    // puerto TCP/IP. Resolverlo aquí evita depender de PowerShell o WMI.
    if raw.bytes().all(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')
    }) {
        if let Ok(addresses) = dns_lookup::lookup_host(raw) {
            let fallback = addresses.first().copied();
            if let Some(ip) = addresses
                .into_iter()
                .find(|ip| matches!(ip, IpAddr::V4(_)))
                .or(fallback)
            {
                return Some(ip.to_string());
            }
        }
    }

    None
}

fn drivers(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys drivers — consulta controladores y dispositivos PnP\n\
             uso:\n\
               sys drivers                listado de drivers\n\
               sys drivers --verbose      información detallada\n\
               sys drivers --signed       información de firma\n\
               sys drivers --csv          salida CSV\n\
               sys drivers --pnp          paquetes de drivers PnP\n\
               sys drivers --devices      dispositivos PnP conectados\n",
        ));
    }

    for option in args {
        if !matches!(option.as_str(), "--pnp" | "--devices" | "--verbose" | "--signed" | "--csv") {
            return Ok(CommandOutput::error(
                format!("sys drivers: opción desconocida: {option}"), 2,
            ));
        }
    }
    let pnp_mode = options::has(args, "--pnp");
    let device_mode = options::has(args, "--devices");
    if pnp_mode && device_mode {
        return Ok(CommandOutput::error(
            "sys drivers: --pnp y --devices son modos excluyentes", 2,
        ));
    }
    if (pnp_mode || device_mode)
        && ["--csv", "--verbose", "--signed"].iter().any(|flag| options::has(args, flag))
    {
        return Ok(CommandOutput::error(
            "sys drivers: --csv, --verbose y --signed no se admiten con --pnp ni --devices", 2,
        ));
    }

    if pnp_mode {
        return run_windows_tool("pnputil.exe", &["/enum-drivers".to_owned()]);
    }

    if device_mode {
        return run_windows_tool(
            "pnputil.exe",
            &["/enum-devices".to_owned(), "/connected".to_owned()],
        );
    }

    let mut native = Vec::new();
    if options::has(args, "--verbose") {
        native.push("/V".to_owned());
    }
    if options::has(args, "--signed") {
        native.push("/SI".to_owned());
    }
    native.push("/FO".to_owned());
    native.push(if options::has(args, "--csv") {
        "CSV".to_owned()
    } else {
        "TABLE".to_owned()
    });

    run_windows_tool("driverquery.exe", &native)
}

fn events(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys events — consulta Windows Event Log\n\
             uso:\n\
               sys events                       últimos 20 eventos de System\n\
               sys events Application           últimos eventos de Application\n\
               sys events --log Security        selecciona un log\n\
               sys events --count 50            cantidad (1..500)\n\
               sys events --query XPATH         filtro XPath de wevtutil\n\
               sys events --format text|xml     formato de salida\n\
               sys events --logs                lista logs disponibles\n\
               sys events --publishers          lista publishers\n",
        ));
    }

    if options::has(args, "--logs") {
        return run_windows_tool("wevtutil.exe", &["el".to_owned()]);
    }
    if options::has(args, "--publishers") {
        return run_windows_tool("wevtutil.exe", &["ep".to_owned()]);
    }

    let positional_log = args.first().filter(|arg| !arg.starts_with('-')).map(String::as_str);
    let log = options::value(args, "--log")
        .or(positional_log)
        .unwrap_or("System");

    let count = options::value(args, "--count")
        .unwrap_or("20")
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("sys events: --count debe ser un número"))?;

    if !(1..=500).contains(&count) {
        return Ok(CommandOutput::error(
            "sys events: --count debe estar entre 1 y 500",
            2,
        ));
    }

    let format = options::value(args, "--format").unwrap_or("text");
    if !matches!(format, "text" | "xml") {
        return Ok(CommandOutput::error(
            "sys events: --format debe ser text o xml",
            2,
        ));
    }

    let mut native = vec![
        "qe".to_owned(),
        log.to_owned(),
        format!("/c:{count}"),
        "/rd:true".to_owned(),
        format!("/f:{format}"),
    ];

    if let Some(query) = options::value(args, "--query") {
        native.push(format!("/q:{query}"));
    }

    run_windows_tool("wevtutil.exe", &native)
}

fn registry(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys registry — consulta de solo lectura del Registro de Windows\n\
             uso:\n\
               sys registry CLAVE\n\
               sys registry query CLAVE\n\
               sys registry CLAVE --value NOMBRE\n\
               sys registry CLAVE --default\n\
               sys registry CLAVE --recursive\n\
               sys registry CLAVE --find TEXTO [--keys|--data]\n\
             ejemplo:\n\
               sys registry \"HKLM\\\\SOFTWARE\\\\Microsoft\\\\Windows NT\\\\CurrentVersion\"\n",
        ));
    }

    let offset = if args.first().is_some_and(|arg| arg == "query") { 1 } else { 0 };
    let Some(key) = args.get(offset) else {
        return Ok(CommandOutput::error("sys registry: falta CLAVE", 2));
    };
    if key.starts_with('-') {
        return Ok(CommandOutput::error(
            "sys registry: la CLAVE debe ir antes de las opciones",
            2,
        ));
    }

    let mut native = vec!["query".to_owned(), key.clone()];

    if let Some(value) = options::value(args, "--value") {
        native.push("/v".to_owned());
        native.push(value.to_owned());
    } else if options::has(args, "--default") {
        native.push("/ve".to_owned());
    }

    if options::has(args, "--recursive") {
        native.push("/s".to_owned());
    }

    if let Some(find) = options::value(args, "--find") {
        native.push("/f".to_owned());
        native.push(find.to_owned());
        if options::has(args, "--keys") {
            native.push("/k".to_owned());
        }
        if options::has(args, "--data") {
            native.push("/d".to_owned());
        }
    }

    run_windows_tool("reg.exe", &native)
}

fn scheduled_tasks(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys tasks — consulta tareas programadas de Windows\n\
             uso:\n\
               sys tasks                  listado en tabla\n\
               sys tasks --verbose        listado detallado\n\
               sys tasks --csv            salida CSV\n\
               sys tasks NOMBRE           detalle de una tarea\n\
               sys tasks NOMBRE --xml     definición XML de una tarea\n",
        ));
    }

    let name = args.first().filter(|arg| !arg.starts_with('-'));
    let mut native = vec!["/Query".to_owned()];

    if let Some(name) = name {
        native.push("/TN".to_owned());
        native.push(name.clone());

        if options::has(args, "--xml") {
            native.push("/XML".to_owned());
            return run_windows_tool("schtasks.exe", &native);
        }

        native.push("/FO".to_owned());
        native.push(if options::has(args, "--csv") {
            "CSV".to_owned()
        } else {
            "LIST".to_owned()
        });
        native.push("/V".to_owned());
    } else {
        native.push("/FO".to_owned());
        native.push(if options::has(args, "--csv") {
            "CSV".to_owned()
        } else {
            "TABLE".to_owned()
        });
        if options::has(args, "--verbose") {
            native.push("/V".to_owned());
        }
    }

    run_windows_tool("schtasks.exe", &native)
}

fn run_windows_tool(program: &str, args: &[String]) -> anyhow::Result<CommandOutput> {
    #[cfg(windows)]
    let executable = crate::support::windows::system32_executable(program)?;
    #[cfg(not(windows))]
    let executable = std::path::PathBuf::from(program);

    let mut command = Command::new(&executable);
    command.args(args);

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().map_err(|error| {
        anyhow::anyhow!(
            "{}: no se pudo ejecutar la utilidad nativa de Windows: {error}",
            executable.display()
        )
    })?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code().unwrap_or(1),
    })
}

fn info() -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_all();

    Ok(CommandOutput::ok(format!(
        "hostname: {}\nos: {}\nkernel: {}\nuptime: {} s\ncpus: {}\nmemory_total: {} MiB\nmemory_used: {} MiB\n",
        System::host_name().unwrap_or_else(|| "desconocido".to_owned()),
        System::long_os_version().unwrap_or_else(|| "Windows".to_owned()),
        System::kernel_version().unwrap_or_else(|| "desconocido".to_owned()),
        System::uptime(),
        system.cpus().len(),
        system.total_memory() / 1024 / 1024,
        system.used_memory() / 1024 / 1024,
    )))
}



fn keygen_system_facts(system: &System) -> Vec<String> {
    let username = env::var("USERNAME").unwrap_or_else(|_| "usuario".to_owned());
    let host = System::host_name().unwrap_or_else(|| "equipo".to_owned());
    let os = System::long_os_version().unwrap_or_else(|| "Windows".to_owned());
    let kernel = System::kernel_version().unwrap_or_else(|| "desconocido".to_owned());
    let cpu = system.cpus().first().map(|cpu| cpu.brand().trim().to_owned())
        .unwrap_or_else(|| "CPU desconocida".to_owned());
    let uptime = System::uptime();
    let used = system.used_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let total = system.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    vec![
        format!("{username} @ {host}"),
        format!("OS  :: {os}"),
        format!("HOST :: {host}"),
        format!("KERNEL :: {kernel}"),
        format!("UPTIME :: {}h {}m", uptime / 3600, (uptime % 3600) / 60),
        format!("SHELL :: Shell Shock Tool / Nwash"),
        format!("VERSION :: {}", env!("CARGO_PKG_VERSION")),
        "TERMINAL :: SST Native".to_owned(),
        format!("CPU :: {cpu}"),
        format!("CPU USO :: {:.1}%", system.global_cpu_usage()),
        format!("RAM :: {used:.2} / {total:.2} GiB"),
        format!("ARCH :: {}", env::consts::ARCH),
    ]
}

/// Colors are per glyph, never one flat shade across the entire artwork.
/// Colors are used directly by the SDL scene renderer.
fn keygen_glyph_color(beat: VisualBeat, index: usize, row: usize) -> (u8, u8, u8) {
    // Original November 23 chromatic cycle, now clocked by the audio.
    let phase = beat.seconds*1.6+(beat.beat_count as f32+beat.beat_phase)*0.30+index as f32*0.24+row as f32*0.55;
    let chan = |shift: f32| -> u8 {
        (128.0 + 118.0 * (phase + shift).sin()+beat.pulse*15.0).clamp(25.0,255.0) as u8
    };
    (chan(0.0), chan(2.094), chan(4.188))
}


// Each character owns an absolute world-space rigid-body transform.
const KEYGEN_TITLES:[(&str,f32,f32,f32);2]=[
    // Restore the compact two-arc composition of the 23 November intro.
    // Spacing deliberately exceeds the original font advance.
    ("SHELL SHOCK TOOL",144.0,28.0,32.0),
    ("SST / KEYGEN",229.0,25.0,28.0),
];
const KEYGEN_PERIOD:f32=1280.0;

#[derive(Clone,Copy)]
struct KeygenGlyphBody{
    x:f32,y:f32,z:f32,vx:f32,vy:f32,vz:f32,
    rx:f32,ry:f32,rz:f32,wx:f32,wy:f32,wz:f32,
    speed:f32,rest_y:f32,seed:f32,orbit:f32,
    trail:[[f32;3];8],
}
impl KeygenGlyphBody{
    fn new(line:usize,index:usize)->Self{
        let (text,y,spacing,_)=KEYGEN_TITLES[line];
        let seed=((index*17+line*29)%23) as f32/22.0;
        let x=(1152.0-(text.chars().count()-1) as f32*spacing)*0.5
            +index as f32*spacing-line as f32*100.0;
        let half=((text.chars().count()-1) as f32*0.5).max(1.0);
        let u=(index as f32-half)/half;
        let lift=(1.0-u*u).max(0.0)*if line==0{88.0}else{57.0};
        let rest_y=y-lift;
        Self{x,y:rest_y+(seed-0.5)*26.0,z:(seed-0.5)*100.0,
            vx:-83.0-seed*22.0,vy:(seed-0.5)*58.0,vz:(0.5-seed)*55.0,
            rx:(seed-0.5)*0.65,ry:(0.5-seed)*0.85,rz:(seed-0.5)*0.40,
            wx:(seed-0.5)*1.2,wy:(0.5-seed)*1.6,wz:(seed-0.5)*0.8,
            speed:83.0+seed*22.0,rest_y,seed,
            orbit:seed*std::f32::consts::TAU+index as f32*0.29,
            trail:[[x,rest_y,0.0];8]}
    }
}

#[derive(Default)]
struct KeygenPhysics{
    bodies:HashMap<(usize,usize),KeygenGlyphBody>,
    pulse:f32,
    cue:u8,
    cue_time:f32,
    sample_tick:u8,
}
impl KeygenPhysics{
    // The director follows decoded musical onsets, not render frames.
    // A complete sequence lasts 120 detected transients; quiet passages
    // hold the current formation rather than inventing a beat.
    fn direct(&mut self,beat:VisualBeat){
        let musical=beat.beat_count as f32+beat.beat_phase;
        let cycle=musical.rem_euclid(120.0);
        self.cue=(cycle/24.0).floor().min(4.0) as u8;
        self.cue_time=(cycle%24.0)/24.0;
    }

    fn target(cue:u8,t:f32,line:usize,index:usize,body:&KeygenGlyphBody)->[f32;3]{
        let (text,_,spacing,_)=KEYGEN_TITLES[line];
        let half=(text.chars().count()-1) as f32*0.5;
        let u=(index as f32-half)/half.max(1.0);
        let tau=std::f32::consts::TAU;
        let theta=u*std::f32::consts::PI+line as f32*std::f32::consts::PI;
        match cue{
            // Counter-rotating rings of letters, with genuine depth.
            1=>{
                let a=theta+(if line==0{1.0}else{-1.0})*t*tau;
                let r=if line==0{345.0}else{206.0};
                [576.0+r*a.cos(),142.0+77.0*a.sin(),110.0*a.sin()+body.seed*30.0]
            }
            // A perspective spiral: outer letters pass the near plane
            // while the inner characters travel into the scene.
            2=>{
                let a=theta+t*tau*1.35+u*1.4;
                let r=100.0+u.abs()*245.0;
                [576.0+r*a.cos(),148.0+82.0*a.sin(),
                 95.0*(a+body.seed).cos()+70.0*(1.0-u.abs())]
            }
            // Two crossing formations pass through different depth planes.
            3=>{
                let a=t*tau+u*1.8+line as f32*std::f32::consts::PI;
                let direction=if line==0{1.0}else{-1.0};
                [576.0+u*360.0+direction*92.0*a.cos(),
                 150.0+direction*(40.0+48.0*a.sin()),105.0*a.cos()]
            }
            // Reassemble the original readable composition before the
            // individual continuous scroll resumes.
            _=>[576.0+u*half*spacing,body.rest_y,0.0],
        }
    }
    fn react(&mut self,beat:usize,strength:f32){
        self.pulse=strength.clamp(0.0,1.0);
        for (&(line,index),body) in &mut self.bodies{
            // Different impulses on each axis for every object. No shared
            // sinusoidal title transform or prescribed string shape.
            let r=((beat*13+index*7+line*11)%19) as f32/18.0;
            // Let the kick travel through characters rather than shaking
            // every letter in opposite directions at each onset.
            let accents=KEYGEN_TITLES[line].0.chars().count();
            let step=beat%accents;
            let d=index.abs_diff(step).min(accents-index.abs_diff(step));
            let accent=if d==0{1.0}else if d<=2{0.62}else{0.19};
            let p=self.pulse*accent;
            body.vy-= (28.0+54.0*r)*p;
            body.vx+=(r-0.5)*38.0*p;
            body.vz+=(0.5-r)*95.0*p;
            body.wx+=(r-0.5)*1.9*p;
            body.wy+=(r-0.5)*3.2*p;
            body.wz+=(r-0.5)*1.2*p;
        }
    }
    fn tick(&mut self){
        let dt=1.0/60.0;
        self.pulse*=0.965;
        let cue=self.cue;
        let cue_time=self.cue_time;
        self.sample_tick=self.sample_tick.wrapping_add(1);
        let sample=self.sample_tick%4==0;
        for (&(line,index),body) in &mut self.bodies{
            let old_x=body.x;
            // Integrate each position, linear momentum and angular momentum.
            // Only a soft individual vertical restoring force keeps the
            // upper scene clear of the fixed system information below.
            body.orbit=(body.orbit+dt*(1.4+body.seed*0.75)).rem_euclid(std::f32::consts::TAU);
            // Individual orbital targets and momentum give the arcing
            // letters independent height and depth, not a shared wave.
            let target_y=body.rest_y+body.orbit.sin()*19.0;
            let target_z=body.orbit.cos()*38.0;
            if cue==0{
                body.vx+=(-body.speed-12.0*self.pulse-body.vx)*1.1*dt;
                body.vy+=(-4.5*(body.y-target_y)-1.6*body.vy)*dt;
                body.vz+=(-3.8*(body.z-target_z)-1.5*body.vz)*dt;
                body.vx=body.vx.clamp(-190.0,-45.0);
                body.x=keygen_wrap_glyph(body.x+body.vx*dt);
                body.y+=body.vy*dt;body.z+=body.vz*dt;
                let limit=if body.rest_y<150.0{42.0}else{33.0};
                let dy=body.y-body.rest_y;
                if dy.abs()>limit{body.y=body.rest_y+dy.signum()*limit;body.vy=-body.vy*0.72;}
                if body.z.abs()>95.0{body.z=body.z.signum()*95.0;body.vz=-body.vz*0.7;}
            }else{
                let [tx,ty,tz]=Self::target(cue,cue_time,line,index,body);
                let stiffness=if cue==4{12.0}else{7.0+body.seed*2.0};
                let damping=if cue==4{6.5}else{4.0};
                body.vx+=(stiffness*(tx-body.x)-damping*body.vx)*dt;
                body.vy+=(stiffness*(ty-body.y)-damping*body.vy)*dt;
                body.vz+=(stiffness*(tz-body.z)-damping*body.vz)*dt;
                body.vx=body.vx.clamp(-800.0,800.0);
                body.vy=body.vy.clamp(-350.0,350.0);
                body.vz=body.vz.clamp(-400.0,400.0);
                body.x=(body.x+body.vx*dt).clamp(-64.0,1216.0);
                body.y=(body.y+body.vy*dt).clamp(22.0,246.0);
                body.z=(body.z+body.vz*dt).clamp(-180.0,180.0);
            }
            if (body.x-old_x).abs()>1000.0{
                // Never draw a line across the screen when a glyph wraps.
                body.trail=[[body.x,body.y,body.z];8];
            }else if sample{
                body.trail.rotate_left(1);
                body.trail[7]=[body.x,body.y,body.z];
            }
            let torque=4.0+body.seed*1.5;
            let tilt=if cue==0||cue==4{0.0}else{0.55};
            let tx=tilt*body.orbit.sin();
            let ty=tilt*(body.orbit+cue_time*std::f32::consts::TAU).cos();
            let tz=tilt*0.55*(body.orbit+body.seed).sin();
            body.wx+=(torque*(tx-body.rx)-1.7*body.wx)*dt;
            body.wy+=(torque*(ty-body.ry)-1.5*body.wy)*dt;
            body.wz+=(torque*(tz-body.rz)-1.8*body.wz)*dt;
            body.rx=(body.rx+body.wx*dt).clamp(-0.85,0.85);
            body.ry=(body.ry+body.wy*dt).clamp(-1.0,1.0);
            body.rz=(body.rz+body.wz*dt).clamp(-0.65,0.65);
        }
        // Loose spacing constraints between neighboring objects, including
        // across the screen seam. Spaces remain spaces. This prevents H/E
        // overlap or runaway gaps without attaching glyphs to a rigid strip.
        if cue!=0{return;}
        for (line,(text,_,spacing,_)) in KEYGEN_TITLES.iter().enumerate(){
            let ids:Vec<_>=text.chars().enumerate().filter(|(_,ch)|*ch!=' ').map(|(i,_)|i).collect();
            for pair in ids.windows(2){
                let ka=(line,pair[0]);let kb=(line,pair[1]);
                let (Some(a),Some(b))=(self.bodies.get(&ka).copied(),self.bodies.get(&kb).copied())else{continue;};
                let nominal=*spacing*(pair[1]-pair[0]) as f32;
                let gap=(b.x-a.x+KEYGEN_PERIOD*0.5).rem_euclid(KEYGEN_PERIOD)-KEYGEN_PERIOD*0.5;
                let min=nominal*0.73;let max=nominal*1.48;
                let error=gap-gap.clamp(min,max);
                if error.abs()>0.001{
                    let correction=error*0.40;
                    let impulse=(error*2.0+(b.vx-a.vx)*0.25).clamp(-32.0,32.0);
                    if let Some(a)=self.bodies.get_mut(&ka){a.x=keygen_wrap_glyph(a.x+correction);a.vx+=impulse;}
                    if let Some(b)=self.bodies.get_mut(&kb){b.x=keygen_wrap_glyph(b.x-correction);b.vx-=impulse;}
                }
            }
        }
    }
    fn body(&mut self,line:usize,index:usize)->KeygenGlyphBody{
        *self.bodies.entry((line,index)).or_insert_with(||KeygenGlyphBody::new(line,index))
    }
}

/// A glyph is an independent object in continuous 3D scene coordinates.
/// No ANSI frame, terminal cells, or vt100 parser participate.
#[derive(Clone, Copy)]
struct KeygenSprite {
    glyph: char,
    x: f32,
    y: f32,
    z: f32,
    rx: f32,
    ry: f32,
    rz: f32,
    size: f32,
    color: (u8, u8, u8),
    depth: u8,
}

/// Wrap one glyph center after its whole mesh clears the left edge.
/// The period is independent of the length of any text or word. Neighboring
/// letters cross at different instants and appear on the opposite side while
/// the remainder of the phrase is still visible at the first edge.
fn keygen_wrap_glyph(x:f32)->f32{
    const MARGIN:f32=64.0;
    -MARGIN+(x+MARGIN).rem_euclid(1152.0+2.0*MARGIN)
}

fn keygen_scene(physics:&mut KeygenPhysics,beat:VisualBeat,facts:&[String],
    paused:bool,track:usize)->Vec<KeygenSprite>{
    let mut sprites=Vec::new();
    for (line,(text,_,_,size)) in KEYGEN_TITLES.iter().enumerate(){
        for (index,glyph) in text.chars().enumerate(){
            if glyph==' '{continue;}
            let body=physics.body(line,index);
            sprites.push(KeygenSprite{
                glyph,x:body.x,y:body.y,z:body.z,
                rx:body.rx,ry:body.ry,rz:body.rz,
                size:*size,color:keygen_glyph_color(beat,index,line),depth:2,
            });
        }
    }
    let text_line=|sprites:&mut Vec<KeygenSprite>,text:&str,y:f32,size:f32,animated:bool,row:usize|{
        let chars:Vec<char>=text.chars().take(100).collect();
        let spacing=size*0.60;
        let start=if animated {576.0-(chars.len().saturating_sub(1) as f32)*spacing*0.5}else{12.0};
        for (i,glyph) in chars.into_iter().enumerate(){
            if glyph==' '||glyph.is_control(){continue;}
            let color=if animated{
                let (r,g,b)=keygen_glyph_color(beat,i,row+2);
                // Preserve saturated color bands, but keep a readable
                // luminance floor on the original wallpaper.
                let bright=|v:u8| 40+(f32::from(v)*0.84) as u8;
                (bright(r),bright(g),bright(b))
            }else{(189,174,147)};
            sprites.push(KeygenSprite{glyph,x:start+i as f32*spacing,
                y,z:0.0,rx:0.0,ry:0.0,rz:0.0,size,color,depth:1});
        }
    };
    for (i,fact) in facts.iter().enumerate(){
        let col=i/6;let row=i%6;
        let size=(500.0/(fact.chars().count().clamp(1,100) as f32*0.60)).min(19.0);
        let first=sprites.len();
        text_line(&mut sprites,fact,460.0+row as f32*26.0,size,true,i);
        let shift=if col==0{-278.0}else{278.0};
        for sprite in &mut sprites[first..]{sprite.x+=shift;}
    }
    let state=if paused{"PAUSA"}else{"PLAY"};
    text_line(&mut sprites,&format!("[MP3 {track}/2]  {state}   1/2: pista   Espacio: pausa   ESC/Q: salir"),
        650.0,16.0,false,0);
    sprites
}

/// Euler rotations are applied to each corner in 3D before perspective.
/// The renderer receives floating-point vertices, not rounded cell positions.
fn keygen_projector(sprite:&KeygenSprite)->impl Fn([f32;3])->(f32,f32,f32)+'_{
    let (sx,cx)=sprite.rx.sin_cos();let (sy,cy)=sprite.ry.sin_cos();let (sz,cz)=sprite.rz.sin_cos();
    move |[x,y,z]|{
        let (y,z)=(y*cx-z*sx,y*sx+z*cx);
        let (x,z)=(x*cy+z*sy,-x*sy+z*cy);
        let (x,y)=(x*cz-y*sz,x*sz+y*cz);
        let scale=620.0/(620.0-sprite.z-z).clamp(250.0,1250.0);
        (sprite.x+x*scale,sprite.y+y*scale,sprite.z+z)
    }
}

#[cfg(test)]
fn keygen_project(sprite:&KeygenSprite,width:f32,height:f32,layer:f32)->[(f32,f32);4]{
    let project=keygen_projector(sprite);
    [(-0.5,-0.5),(0.5,-0.5),(0.5,0.5),(-0.5,0.5)].map(|(u,v)|{
        let (x,y,_)=project([u*width,v*height,layer]);(x,y)
    })
}

// Only ordinary, flat status text uses a texture. The animated title uses
// an untextured, closed solid mesh generated from the actual font outline.
#[cfg(test)]
fn keygen_draw_glyph<T: sdl2::render::RenderTarget>(
    canvas: &mut sdl2::render::Canvas<T>, texture: &sdl2::render::Texture<'_>,
    sprite: &KeygenSprite, width: f32, height: f32,
) -> anyhow::Result<()> {
    use sdl2::{pixels::Color, rect::FPoint, render::Vertex};
    const INDICES: [u8; 6] = [0,1,2,0,2,3];
    const UV: [(f32,f32); 4] = [(0.0,0.0),(1.0,0.0),(1.0,1.0),(0.0,1.0)];
    let points = keygen_project(sprite,width,height,0.0);
    let vertices = std::array::from_fn::<_,4,_>(|i| Vertex {
        position: FPoint::new(points[i].0, points[i].1),
        color: Color::RGB(sprite.color.0,sprite.color.1,sprite.color.2),
        tex_coord: FPoint::new(UV[i].0, UV[i].1),
    });
    canvas.render_geometry(&vertices,Some(texture),&INDICES[..]).map_err(anyhow::Error::msg)
}

/// Transparent SDL surface; the existing SST wallpaper is the backdrop.
fn keygen_backdrop<T: sdl2::render::RenderTarget>(
    canvas: &mut sdl2::render::Canvas<T>,
) {
    canvas.set_draw_color(sdl2::pixels::Color::RGBA(0,0,0,0));
    canvas.clear();
}

// A light trace is generated from the actual motion history of each glyph.
// It is intentionally absent from the calm readable scroll and reassembly.
fn keygen_motion_traces<T:sdl2::render::RenderTarget>(
    canvas:&mut sdl2::render::Canvas<T>,physics:&KeygenPhysics,beat:VisualBeat,
)->anyhow::Result<()>{
    use sdl2::{pixels::Color,rect::Point};
    if !(1..=3).contains(&physics.cue){return Ok(());}
    for (&(line,index),body) in &physics.bodies{
        let (r,g,b)=keygen_glyph_color(beat,index,line);
        let mut history=body.trail;
        history.rotate_left(1);
        history[7]=[body.x,body.y,body.z];
        for (i,pair) in history.windows(2).enumerate(){
            let a=pair[0];let bpt=pair[1];
            let distance=(a[0]-bpt[0]).hypot(a[1]-bpt[1]);
            if !(0.8..=100.0).contains(&distance){continue;}
            let strength=(i+1) as f32/7.0;
            let alpha=(18.0+strength*86.0+beat.pulse*22.0) as u8;
            canvas.set_draw_color(Color::RGBA(r,g,b,alpha));
            canvas.draw_line(Point::new(a[0] as i32,(30.0+a[1]*1.42) as i32),
                Point::new(bpt[0] as i32,(30.0+bpt[1]*1.42) as i32)).map_err(anyhow::Error::msg)?;
        }
    }
    Ok(())
}

pub(crate) enum KeygenFrame { Draw(slint::SharedPixelBuffer<slint::Rgba8Pixel>), Closed }
pub(crate) static KEYGEN_FRAME: std::sync::Mutex<Option<KeygenFrame>> = std::sync::Mutex::new(None);
struct KeygenView;
impl Drop for KeygenView {
    fn drop(&mut self) {
        crate::adapters::terminal::io::leave_raw();
        *KEYGEN_FRAME.lock().unwrap_or_else(|e| e.into_inner()) = Some(KeygenFrame::Closed);
    }
}

fn keygen_blit_text(pixels:&mut [slint::Rgba8Pixel],width:u32,height:u32,
    sprite:&KeygenSprite,metrics:&fontdue::Metrics,alpha:&[u8]){
    let origin=(sprite.x-sprite.size*0.30).round() as i32+metrics.xmin;
    let baseline=(sprite.y+sprite.size*0.32).round() as i32;
    let top=baseline-metrics.ymin-metrics.height as i32;
    for gy in 0..metrics.height{for gx in 0..metrics.width{
        let x=origin+gx as i32;let y=top+gy as i32;
        if x<0||y<0||x>=width as i32||y>=height as i32{continue;}
        let a=u32::from(alpha[gy*metrics.width+gx]);
        if a==0{continue;}
        let p=&mut pixels[y as usize*width as usize+x as usize];
        let da=u32::from(p.a);
        let out=a+(da*(255-a)+127)/255;
        let channel=|src:u8,dst:u8|((u32::from(src)*a+u32::from(dst)*da*(255-a)/255+out/2)/out).min(255) as u8;
        *p=slint::Rgba8Pixel{r:channel(sprite.color.0,p.r),g:channel(sprite.color.1,p.g),
            b:channel(sprite.color.2,p.b),a:out as u8};
    }}
}

fn keygen_sdl(mut music: crate::application::keygen_audio::KeygenMusic)
    -> anyhow::Result<CommandOutput> {
    use crate::adapters::terminal::io;
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    use sdl2::{pixels::PixelFormatEnum, render::BlendMode, surface::Surface};
    use std::time::Instant;
    if io::output_sender().is_none() {
        anyhow::bail!("keygen: abre el comando en la terminal gráfica de SST");
    }
    io::enter_raw()?;
    let _view = KeygenView;
    let surface = Surface::new(1152, 672, PixelFormatEnum::RGBA32).map_err(anyhow::Error::msg)?;
    let mut renderer = surface.into_canvas().map_err(anyhow::Error::msg)?;
    renderer.set_blend_mode(BlendMode::Blend);
    let font = fontdue::Font::from_bytes(SST_TERMINAL_FONT_BYTES,fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!("keygen: fuente integrada SST inválida: {e}"))?;
    let display_face = ttf_parser::Face::parse(SST_TERMINAL_FONT_BYTES,0)
        .map_err(|e| anyhow::anyhow!("keygen: contorno de la fuente SST inválido: {e:?}"))?;
    let mut mesh_cache=HashMap::new();
    let interrupt=io::interrupt_flag();
    let abort=io::force_abort_flag();
    let mut system=System::new_all();
    system.refresh_all();
    let mut facts=keygen_system_facts(&system);
    let mut physics=KeygenPhysics::default();
    let mut glyph_cache=HashMap::<(char,u32),(fontdue::Metrics,Vec<u8>)>::new();
    let mut mesh_vertices=Vec::with_capacity(16384);
    let mut stage=keygen_demo::Stage::default();
    let mut last=Instant::now();
    let mut metrics_at=Instant::now();
    let mut audio_at=Instant::now();
    let mut next_frame=Instant::now();
    let mut last_beat=(music.track_number(),0usize);
    let mut accumulator=0.0_f32;
    const STEP:f32=1.0/60.0;
    const FRAME:Duration=Duration::from_nanos(16_666_667);
    'running: loop {
        if interrupt.load(std::sync::atomic::Ordering::Relaxed) ||
            abort.load(std::sync::atomic::Ordering::Relaxed) { break; }
        while io::poll(Duration::ZERO)? {
            let Event::Key(key) = io::read()? else { continue };
            if key.kind != KeyEventKind::Press { continue; }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q' | 'Q') => break 'running,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break 'running,
                KeyCode::Char(' ') => music.pause_resume()?,
                KeyCode::Char('1') => music.select(0)?,
                KeyCode::Char('2') => music.select(1)?,
                _ => {}
            }
        }
        let now=Instant::now();
        if now<next_frame{
            std::thread::sleep((next_frame-now).min(Duration::from_millis(1)));
            continue;
        }
        // Maintain absolute frame deadlines. A slow frame cannot slow music
        // or scroll; no extra full-frame sleep after rendering.
        next_frame=(next_frame+FRAME).max(now);
        let elapsed=now.duration_since(last);
        last=now;
        if audio_at.elapsed()>=Duration::from_millis(80){
            music.tick()?;
            audio_at=Instant::now();
        }
        let beat=music.visual();
        let beat_id=(music.track_number(),beat.beat_count);
        if !music.is_paused(){
            physics.direct(beat);
            if beat_id!=last_beat{
                if beat.beat_count>0{physics.react(beat.beat_count,beat.pulse);}
                last_beat=beat_id;
            }
            accumulator+=elapsed.as_secs_f32().min(0.10);
            while accumulator>=STEP{physics.tick();accumulator-=STEP;}
        }else{accumulator=0.0;}
        if metrics_at.elapsed() >= Duration::from_secs(1) {
            system.refresh_cpu_all();
            system.refresh_memory();
            facts = keygen_system_facts(&system);
            metrics_at = Instant::now();
        }
        let mut sprites=keygen_scene(&mut physics,beat,&facts,music.is_paused(),music.track_number());
        sprites.sort_by(|a,b|a.z.total_cmp(&b.z));
        keygen_backdrop(&mut renderer);
        stage.draw(&mut renderer,beat,physics.cue,physics.cue_time)?;
        keygen_motion_traces(&mut renderer,&physics,beat)?;
        mesh_vertices.clear();
        use sdl2::render::VertexIndices;
        for sprite in sprites.iter().filter(|s|s.depth>1){
            if sprite.x < -80.0 || sprite.x > 1232.0 {continue;}
            if let std::collections::hash_map::Entry::Vacant(entry)=mesh_cache.entry(sprite.glyph){
                entry.insert(keygen_mesh::GlyphMesh::from_font(&display_face,sprite.glyph)?);
            }
            let mut placed=*sprite;
            placed.y=30.0+placed.y*1.42;
            mesh_cache[&sprite.glyph].append_vertices(&mut mesh_vertices,placed.size,
                placed.color,keygen_projector(&placed));
        }
        if !mesh_vertices.is_empty(){
            renderer.render_geometry(&mesh_vertices,None,VertexIndices::Sequential).map_err(anyhow::Error::msg)?;
        }
        mesh_vertices.clear();
        // A restrained solid SST signature anchors the changing stage.
        for (i,glyph) in "SST".chars().enumerate(){
            let sprite=KeygenSprite{glyph,x:576.0+(i as f32-1.0)*95.0,
                y:212.0,z:24.0+beat.pulse*12.0,rx:0.10*(beat.seconds*0.9).sin(),
                ry:0.16*(beat.seconds*0.72).sin(),rz:0.0,size:142.0,
                color:(202+(beat.pulse*32.0) as u8,213,220),depth:2};
            if let std::collections::hash_map::Entry::Vacant(entry)=mesh_cache.entry(glyph){
                entry.insert(keygen_mesh::GlyphMesh::from_font(&display_face,glyph)?);
            }
            mesh_cache[&glyph].append_vertices(&mut mesh_vertices,sprite.size,sprite.color,keygen_projector(&sprite));
        }
        renderer.render_geometry(&mesh_vertices,None,VertexIndices::Sequential).map_err(anyhow::Error::msg)?;
        renderer.present();
        // SDL renders directly into a CPU surface. Copy it once into the
        // thread-safe Slint buffer, avoiding read_pixels plus a second GUI copy.
        let surface=renderer.surface();
        let mut buffer=if surface.pitch()==1152*4{
            surface.with_lock(|bytes|slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(bytes,1152,672))
        }else{
            let pixels=renderer.read_pixels(None,PixelFormatEnum::RGBA32).map_err(anyhow::Error::msg)?;
            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&pixels,1152,672)
        };
        // Rasterize flat information at its actual pixel size and baseline.
        // No SDL texture atlas, resampling, or triangle edge seams in text.
        for sprite in sprites.iter().filter(|s|s.depth==1){
            let key=(sprite.glyph,sprite.size.round() as u32);
            let glyph=glyph_cache.entry(key).or_insert_with(||font.rasterize(key.0,key.1 as f32));
            keygen_blit_text(buffer.make_mut_slice(),1152,672,sprite,&glyph.0,&glyph.1);
        }
        *KEYGEN_FRAME.lock().unwrap_or_else(|e|e.into_inner())=Some(KeygenFrame::Draw(buffer));
    }
    Ok(CommandOutput::ok(""))
}

#[cfg(test)]
mod keygen_motion_regressions {
    use super::{KeygenPhysics, VisualBeat, keygen_scene, keygen_project, keygen_draw_glyph, keygen_backdrop, keygen_wrap_glyph, keygen_blit_text};
    fn time(frame:u64)->VisualBeat{VisualBeat{seconds:frame as f32/60.0,beat_count:(frame/30) as usize,beat_phase:(frame%30) as f32/30.0,pulse:0.5}}

    #[test]
    fn letters_keep_independent_finite_velocity_and_bounce_above_floor() {
        let mut physics = KeygenPhysics::default();
        let _ = keygen_scene(&mut physics,time(0),&[],false,1);
        for frame in 0..900 {
            if frame%30==0{physics.react((frame/30) as usize,0.7);} physics.tick();
            for body in physics.bodies.values() {
                assert!(body.x.is_finite() && body.y.is_finite() && body.z.is_finite());
                assert!(body.vx.is_finite() && body.vy.is_finite() && body.vz.is_finite());
                assert!((body.y-body.rest_y).abs()<=52.001);
            }
        }
        let one = physics.bodies.get(&(0,0)).unwrap();
        let two = physics.bodies.get(&(0,1)).unwrap();
        assert!((one.x-two.x).abs()>0.0001 || (one.z-two.z).abs()>0.0001);
        assert!((one.ry-two.ry).abs()>0.0001 || (one.rz-two.rz).abs()>0.0001);
    }

    #[test]
    fn projected_letters_have_continuous_motion_and_distinct_depth_layers() {
        let mut physics = KeygenPhysics::default();
        let initial = keygen_scene(&mut physics,time(0),&[],false,1)[0];
        for _ in 0..37 { physics.tick(); }
        let next = keygen_scene(&mut physics,time(37),&[],false,1)[0];
        assert_ne!((initial.x,initial.y,initial.z),(next.x,next.y,next.z));
        let front = keygen_project(&next,40.0,56.0,0.0);
        let back = keygen_project(&next,40.0,56.0,-8.0);
        assert_ne!(front,back);
        assert!(front.iter().all(|p| p.0.is_finite() && p.1.is_finite()));
    }

    #[test]
    fn glyphs_cross_edges_individually_and_remain_front_facing() {
        let mut physics=KeygenPhysics::default();
        let mut previous=keygen_scene(&mut physics,time(0),&[],false,1);
        let mut crossings=0;
        let mut split=false;
        let mut flexible=false;
        for frame in 1..3600 {
            if frame%30==0{physics.react((frame/30) as usize,0.7);}
            physics.tick();
            let sprites=keygen_scene(&mut physics,time(frame),&[],false,1);
            // Insertion order identifies each body, even when spatial order
            // changes at the seam. No requirement that a whole title wraps.
            let title_len="SHELLSHOCKTOOL".len();
            let total=title_len+"SST/KEYGEN".len();
            let old=&previous[..total];let new=&sprites[..total];
            assert_eq!(new.iter().map(|s|s.glyph).collect::<String>(),"SHELLSHOCKTOOLSST/KEYGEN");
            let crossed:Vec<_>=old.iter().zip(new).filter(|(a,b)|b.x-a.x>1000.0).collect();
            if !crossed.is_empty(){
                crossings+=crossed.len();
                assert!(crossed.len()<title_len);
                assert!(crossed.iter().all(|(a,b)|a.x< -40.0 && b.x>1190.0));
            }
            split|=new[..title_len].iter().any(|s|s.x<120.0)
                && new[..title_len].iter().any(|s|s.x>1032.0);
            let dx0=keygen_wrap_glyph(new[0].x-old[0].x+576.0)-576.0;
            let dx1=keygen_wrap_glyph(new[1].x-old[1].x+576.0)-576.0;
            flexible|=(dx0-dx1).abs()>0.005;
            for sprite in new {
                let corners=keygen_project(sprite,28.0,36.0,0.0);
                assert!(corners.iter().all(|p|p.0.is_finite()&&p.1.is_finite()));
            }
            previous=sprites;
        }
        assert!(crossings>25);
        assert!(split,"a title must be simultaneously visible on opposite edges");
        assert!(flexible,"individual horizontal velocities must differ");
    }

    #[test]
    fn each_glyph_wraps_only_when_its_mesh_is_offscreen(){
        assert_eq!(keygen_wrap_glyph(-64.0),-64.0);
        assert!(keygen_wrap_glyph(-64.01)>1215.0);
        assert_eq!(keygen_wrap_glyph(12.0),12.0);
        assert_eq!(keygen_wrap_glyph(12.0-1280.0),12.0);
    }

    #[test]
    fn scroll_does_not_reset_when_track_position_resets(){
        let mut physics=KeygenPhysics::default();
        let _=keygen_scene(&mut physics,time(0),&[],false,1);
        for _ in 0..120{physics.tick();}
        let first=keygen_scene(&mut physics,time(120),&[],false,1)[0];
        let next=keygen_scene(&mut physics,time(0),&[],false,2)[0];
        assert_eq!(first.x,next.x);
    }

    #[test]
    fn reference_layout_keeps_compact_arcs_and_independent_depth(){
        let mut physics=KeygenPhysics::default();
        let first=keygen_scene(&mut physics,time(0),&[],false,1);
        let title=&first[.. "SHELLSHOCKTOOL".len()];
        let left=title.first().unwrap();
        let right=title.last().unwrap();
        let center=&title[title.len()/2];
        assert!(right.x-left.x<450.0);
        assert!(center.y+50.0<left.y && center.y+50.0<right.y);
        assert!(title.iter().all(|s|s.size<36.0));
        assert!(title.iter().any(|s|(s.z-left.z).abs()>30.0));
        for _ in 0..60{physics.tick();}
        let later=keygen_scene(&mut physics,time(60),&[],false,1);
        assert!((later[0].z-first[0].z-later[1].z+first[1].z).abs()>0.1);
    }

    #[test]
    fn musical_director_changes_the_geometry_and_reassembles(){
        let mut physics=KeygenPhysics::default();
        let original=keygen_scene(&mut physics,time(0),&[],false,1);
        let mut positions=Vec::new();
        for cue in 1..=4{
            for frame in 0..240{
                let musical=cue*24+frame/10;
                let beat=VisualBeat{beat_count:musical,beat_phase:(frame%10) as f32/10.0,
                    seconds:frame as f32/60.0,pulse:0.7};
                physics.direct(beat);
                if frame%10==0{physics.react(musical,0.7);}
                physics.tick();
            }
            let sprites=keygen_scene(&mut physics,time(0),&[],false,1);
            assert!(sprites.iter().all(|s|s.x.is_finite()&&s.y.is_finite()&&s.z.is_finite()));
            positions.push(sprites[0].x);
        }
        assert!(positions.windows(2).any(|p|(p[0]-p[1]).abs()>60.0));
        assert!((physics.bodies[&(0,0)].x-original[0].x).abs()<30.0);
        assert!(physics.bodies.values().all(|b|b.trail.iter().flatten().all(|v|v.is_finite())));
        physics.direct(VisualBeat{beat_count:120,..VisualBeat::default()});
        assert_eq!(physics.cue,0);
    }

    #[test]
    fn independent_bodies_and_status_glyphs(){
        let mut physics=KeygenPhysics::default();
        let sprites=keygen_scene(&mut physics,time(0),&["HOST :: TEST".to_owned()],false,1);
        let old=physics.bodies[&(0,0)];
        let second=physics.bodies[&(0,1)];
        physics.bodies.get_mut(&(0,1)).unwrap().vy+=120.0;
        for _ in 0..10{physics.tick();}
        let a=physics.bodies[&(0,0)];let b=physics.bodies[&(0,1)];
        assert!(((a.y-old.y)-(b.y-second.y)).abs()>5.0);
        let font=fontdue::Font::from_bytes(super::SST_TERMINAL_FONT_BYTES,fontdue::FontSettings::default()).unwrap();
        let mut pixels=vec![slint::Rgba8Pixel::default();128*128];
        let mut glyph=*sprites.iter().find(|s|s.depth==1&&s.glyph=='H').unwrap();
        glyph.x=64.0;glyph.y=64.0;
        let (metrics,alpha)=font.rasterize('H',glyph.size);
        keygen_blit_text(&mut pixels,128,128,&glyph,&metrics,&alpha);
        assert_eq!(pixels.iter().filter(|p|p.a>0).count(),alpha.iter().filter(|&&a|a>0).count());
    }

    #[test]
    fn native_intro_preserves_wallpaper_and_console_font() {
        use sdl2::{pixels::PixelFormatEnum,surface::Surface};
        let mut canvas=Surface::new(32,32,PixelFormatEnum::RGBA32).unwrap().into_canvas().unwrap();
        keygen_backdrop(&mut canvas);
        let pixels=canvas.read_pixels(None,PixelFormatEnum::RGBA32).unwrap();
        assert!(pixels.chunks_exact(4).all(|p|p[3]==0));
        let face=ttf_parser::Face::parse(super::SST_TERMINAL_FONT_BYTES,0).unwrap();
        assert!(face.glyph_index('S').is_some());
        let mut physics=KeygenPhysics::default();
        let facts=vec!["CPU :: TEST".to_owned()];
        let first=keygen_scene(&mut physics,time(0),&facts,false,1);
        let later=keygen_scene(&mut physics,time(120),&facts,false,1);
        assert!(first.iter().zip(later).any(|(a,b)| a.depth==1 && a.color!=b.color));
    }

    #[test]
    fn sdl_software_canvas_draws_projected_textured_triangles() {
        use sdl2::{pixels::{Color,PixelFormatEnum},surface::Surface,render::BlendMode};
        let surface = Surface::new(128,128,PixelFormatEnum::RGBA32).unwrap();
        let mut canvas = surface.into_canvas().unwrap();
        let creator = canvas.texture_creator();
        let mut texture = creator.create_texture_streaming(PixelFormatEnum::RGBA32,4,4).unwrap();
        texture.set_blend_mode(BlendMode::Blend);
        texture.with_lock(None,|buf,_| buf.fill(255)).unwrap();
        canvas.set_draw_color(Color::RGB(0,0,0));
        canvas.clear();
        let mut physics = KeygenPhysics::default();
        let mut sprite = keygen_scene(&mut physics,time(0),&[],false,1)[0];
        sprite.x=64.0; sprite.y=64.0; sprite.z=0.0;
        sprite.color=(255,80,20);
        keygen_draw_glyph(&mut canvas,&texture,&sprite,40.0,48.0).unwrap();
        canvas.present();
        let pixels = canvas.read_pixels(None,PixelFormatEnum::RGBA32).unwrap();
        assert!(pixels.chunks_exact(4).any(|p| p[0]>100 && p[1]>10 && p[2]<100));
    }
}

/// El logo tipográfico SST proviene del ascii-art.txt proporcionado por el autor.
/// Se conserva íntegro y sólo se reduce su representación durante el arranque.
pub(crate) const FETCH_LOGO_ASCII: &[&str] = &["   %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%    ","   %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%    ","%%%-:::-----------------------------------------------------------------------------------------%%%%","%%%-:::-----------------------------------------------------------------------------------------%%%%","%%%=--::::--------------------------------------------------------------------------------------%%%%","%%%-:::-----------------------------------------------------------------------------------------%%%%","%%%-:::-----------------------------------------------------------------------------------------%%%%","%%%=---------%%%%%%%%%%%%%%%%%%-------------------%%%%%%%%%%%%%%%---#%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%","%%%=---------%%%%%%%%%%%%%%%%%%-------------------%%%%%%%%%%%%%%%---#%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%","%%%=------%%%%%%%%%%%%%%%%%%%%%%%%----------%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%","%%%=--+%%%%%%%%%%%%---------%%%---------=%%%%%%%%%%%%---------%%%-------------%%%%%%%%%---------%%%%","%%%=--+%%%%%%%%%%%%---------%%%---------=%%%%%%%%%%%%---------%%%-------------%%%%%%%%%---------%%%%","%%%=--+%%%%%%%%%------------------------=%%%%%%%%%----------------------------%%%%%%%%%---------%%%%","%%%=--+%%%%%%%%%------------------------=%%%%%%%%%----------------------------%%%%%%%%%---------%%%%","%%%=--+%%%%%%%%%%%%---------------------=%%%%%%%%%%%%-------------------------%%%%%%%%%---------%%%%","%%%=------%%%%%%%%%%%%----------------------%%%%%%%%%%%%----------------------%%%%%%%%%---------%%%%","%%%=------%%%%%%%%%%%%----------------------%%%%%%%%%%%%----------------------%%%%%%%%%---------%%%%","%%%=------%%%%%%%%%%%%%%%%%%----------------%%%%%%%%%%%%%%%%%%----------------%%%%%%%%%---------%%%%","%%%=------%%%%%%%%%%%%%%%%%%----------------%%%%%%%%%%%%%%%%%%----------------%%%%%%%%%---------%%%%","%%%=---------%%%%%%%%%%%%%%%%%%----------------%%%%%%%%%%%%%%%%%%-------------%%%%%%%%%---------%%%%","%%%=---------------%%%%%%%%%%%%%%%-------------------%%%%%%%%%%%%%%%=---------%%%%%%%%%---------%%%%","%%%=---------------%%%%%%%%%%%%%%%-------------------%%%%%%%%%%%%%%%=---------%%%%%%%%%---------%%%%","%%%%%%*------------------%%%%%%%%%---*%%*------------------%%%%%%%%%=---------%%%%%%%%%---------%%%%","%%%%%%*------------------%%%%%%%%%---*%%*------------------%%%%%%%%%=---------%%%%%%%%%---------%%%%","%%%%%%%%%#---------------%%%%%%%%%---*%%%%%%---------------%%%%%%%%%=---------%%%%%%%%%---------%%%%","%%%=--+%%%%%%---------%%%%%%%%%%%%=-----=%%%%%%---------%%%%%%%%%%%%=---------%%%%%%%%%---------%%%%","%%%=--+%%%%%%---------%%%%%%%%%%%%=-----=%%%%%%---------%%%%%%%%%%%%=---------%%%%%%%%%---------%%%%","%%%=--+%%%%%%%%%%%%%%%%%%%%%%%%---------=%%%%%%%%%%%%%%%%%%%%%%%%-------------%%%%%%%%%---------%%%%","%%%=--+%%%%%%%%%%%%%%%%%%%%%%%%---------=%%%%%%%%%%%%%%%%%%%%%%%%-------------%%%%%%%%%---------%%%%","%%%=---------%%%%%%%%%%%%----------------------%%%%%%%%%%%%-------------------%%%%%%%%%---------%%%%","%%%=--------------------------------------------------------------------------------------------%%%%","%%%=--------------------------------------------------------------------------------------------%%%%","%%%=--------------------------------------------------------------------------------------------%%%%","%%%=--------------------------------------------------------------------------------------------%%%%","%%%=--------------------------------------------------------------------------------------------%%%%","   %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%    ","   %%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%    "];

/// The established SST blue/silver palette. Geometry comes exclusively
/// from ascii-art.txt; no raster icon is used to construct the logo.
const SST_LOGO_COLORS: [(u8, u8, u8); 4] = [
    (35, 52, 106), (54, 76, 119), (109, 143, 171), (186, 189, 198),
];

fn sst_icon_ansi(index: usize) -> String {
    let (r, g, b) = SST_LOGO_COLORS[index.min(3)];
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Conservar la geometría renderizada por libcaca y emitir el RGB original
/// en ANSI TrueColor. libcaca::Color sólo ofrece una paleta aproximada de 16
/// colores, por lo que no debe sustituir los valores del icono oficial.
/// SDL2 resamples the source ASCII character-ID grid for compact display.
/// This is an offscreen software surface: the logo remains inside the SST
/// terminal instead of opening an unrelated second window.
#[cfg(windows)]
fn sst_logo_sdl_pixels(
    icon: &image::RgbaImage, width: usize, height: usize,
) -> Option<Vec<(u8, u8, u8, u8)>> {
    use sdl2::{pixels::PixelFormatEnum, render::BlendMode, surface::Surface};
    if width == 0 || height == 0 || icon.width() == 0 || icon.height() == 0 {
        return None;
    }
    let mut source = Surface::new(icon.width(), icon.height(), PixelFormatEnum::ABGR8888).ok()?;
    let src_pitch = source.pitch() as usize;
    source.with_lock_mut(|buf| {
        for y in 0..icon.height() as usize {
            let start = y * src_pitch;
            let src = &icon.as_raw()[y * icon.width() as usize * 4..
                (y + 1) * icon.width() as usize * 4];
            buf[start..start + src.len()].copy_from_slice(src);
        }
    });
    source.set_blend_mode(BlendMode::None).ok()?;
    let mut scaled = Surface::new(width as u32, height as u32,
        PixelFormatEnum::ABGR8888).ok()?;
    source.blit_scaled(None, &mut scaled, None).ok()?;
    let pitch = scaled.pitch() as usize;
    let mut pixels = Vec::with_capacity(width * height);
    scaled.with_lock(|buf| {
        for y in 0..height {
            for x in 0..width {
                let p = y * pitch + x * 4;
                pixels.push((buf[p], buf[p + 1], buf[p + 2], buf[p + 3]));
            }
        }
    });
    Some(pixels)
}

fn sst_icon_truecolor(lines: Vec<String>) -> Vec<String> {
    let height = lines.len();
    let width = lines.iter().map(|s| s.chars().count()).max().unwrap_or(0);
    if width == 0 { return lines; }
    // The actual plate occupies rows 5..27 of the transparent 32px icon.
    // Crop its padding before mapping highlights onto the ASCII lettering.
    let icon = image::load_from_memory(include_bytes!("../../assets/sst-icon.png"))
        .expect("embedded SST icon").crop_imm(0, 5, 32, 22).to_rgba8();
    #[cfg(windows)]
    let pixels = sst_logo_sdl_pixels(&icon, width, height);
    #[cfg(not(windows))]
    let pixels: Option<Vec<(u8,u8,u8,u8)>> = None;
    let silver = (186, 189, 198);
    let navy = (35, 52, 106);
    let yellow = (251, 242, 54);
    lines.into_iter().enumerate().map(|(y, row)| {
        let mut out = String::new();
        let mut current = None;
        for (x, glyph) in row.chars().enumerate() {
            let (r,g,b,a) = pixels.as_ref().map(|p| p[y * width + x]).unwrap_or_else(|| {
                let sx = (2*x+1) * 32 / (2*width);
                let sy = (2*y+1) * 22 / (2*height);
                let [r,g,b,a] = icon.get_pixel(sx as u32, sy as u32).0;
                (r,g,b,a)
            });
            let sample = (r,g,b);
            let ink = matches!(glyph, '█'|'▓'|'▒'|'░'|'%'|'#'|'*'|'+'|'=');
            let style = if a == 0 && glyph == ' ' { None } else {
                let fg = if !ink { silver } else if a == 0 || sample == silver { navy } else { sample };
                Some((fg, if sample == yellow { yellow } else { silver }))
            };
            if style != current {
                if let Some(((r,g,b),(br,bg,bb))) = style {
                    out.push_str(&format!("\x1b[38;2;{r};{g};{b}m\x1b[48;2;{br};{bg};{bb}m"));
                } else { out.push_str("\x1b[0m"); }
                current = style;
            }
            out.push(glyph);
        }
        if current.is_some() { out.push_str("\x1b[0m"); }
        out
    }).collect()
}

fn caca_ansi(canvas: &libcaca::Canvas) -> anyhow::Result<String> {
    Ok(String::from_utf8(canvas.export_to_memory("utf8")?)?)
}

/// Render compact text from the author's original ASCII via SDL2 and libcaca.
fn fetch_compact_logo(plain: bool, columns: usize) -> anyhow::Result<(Vec<String>, usize)> {
    let width = if columns < 70 { 30usize } else { 36usize };
    let height = 12usize;
    let source_height = FETCH_LOGO_ASCII.len();
    let source_width = FETCH_LOGO_ASCII.iter().map(|s| s.len()).max().unwrap_or(100);
    let mut cv = libcaca::Canvas::new(width as i32, height as i32)?;
    let mut plain_rows = vec![String::new(); height];
    for y in 0..height {
        for x in 0..width {
            let x0 = x * source_width / width;
            let x1 = ((x + 1) * source_width / width).max(x0 + 1);
            let y0 = y * source_height / height;
            let y1 = ((y + 1) * source_height / height).max(y0 + 1);
            let mut ink = 0usize;
            let mut total = 0usize;
            for sy in y0..y1.min(source_height) {
                for ch in FETCH_LOGO_ASCII[sy].as_bytes().iter().skip(x0).take(x1 - x0) {
                    total += 1;
                    if matches!(*ch, b'%' | b'#' | b'*' | b'+') { ink += 1; }
                }
            }
            let coverage = ink as f32 / total.max(1) as f32;
            let glyph = if coverage > 0.72 { '█' }
                else if coverage > 0.48 { '▓' }
                else if coverage > 0.25 { '▒' }
                else if coverage > 0.09 { '░' }
                else { ' ' };
            plain_rows[y].push(glyph);
            cv.set_color_ansi(libcaca::Color::Default, libcaca::Color::Default)?;
            cv.put_char(x as i32, y as i32, glyph as u32);
        }
    }
    if plain { return Ok((plain_rows, width)); }
    let rendered = caca_ansi(&cv)?;
    Ok((sst_icon_truecolor(rendered.lines().map(str::to_owned).collect()), width))
}

fn fetch_logo(plain: bool) -> anyhow::Result<(Vec<String>, usize)> {
    let width = FETCH_LOGO_ASCII.iter().map(|s| s.len()).max().unwrap_or(0);
    if plain {
        return Ok((FETCH_LOGO_ASCII.iter().map(|line| (*line).to_owned()).collect(), width));
    }
    let mut cv = libcaca::Canvas::new(width as i32, FETCH_LOGO_ASCII.len() as i32)?;
    for (y, line) in FETCH_LOGO_ASCII.iter().enumerate() {
        for (x, glyph) in line.chars().enumerate() {
            cv.set_color_ansi(libcaca::Color::Default, libcaca::Color::Default)?;
            cv.put_char(x as i32, y as i32, glyph as u32);
        }
    }
    let rendered = caca_ansi(&cv)?;
    Ok((sst_icon_truecolor(rendered.lines().map(str::to_owned).collect()), width))
}

fn truncate_fetch(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let mut result = value.chars().take(max.saturating_sub(1)).collect::<String>();
    result.push('…');
    result
}

fn fetch(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "fetch — muestra información del sistema con SST como texto nativo\n\
             uso: fetch [--small|--full] [--plain|--stdout]\n\
             --small: logo SST compacto, visible al iniciar la terminal\n\
             --full: logo ASCII SST original de 37 líneas\n\
             --plain/--stdout: ASCII original sin ANSI, para archivos y pipes\n\
             alias: neofetch, fastfetch\n",
        ));
    }

    let mut system = System::new_all();
    system.refresh_all();

    let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
    let host = System::host_name().unwrap_or_else(|| "windows".to_owned());
    let os = System::long_os_version().unwrap_or_else(|| "Windows".to_owned());
    let kernel = System::kernel_version().unwrap_or_else(|| "desconocido".to_owned());
    let arch = env::consts::ARCH;
    let uptime = System::uptime();
    let days = uptime / 86_400;
    let hours = (uptime % 86_400) / 3_600;
    let minutes = (uptime % 3_600) / 60;
    let cpu = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{} cores", system.cpus().len()));
    let used = system.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let total = system.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let uptime_text = if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else {
        format!("{hours}h {minutes}m")
    };

    let _startup = args.iter().any(|arg| arg == "--startup");
    let plain = args.iter().any(|arg| matches!(arg.as_str(), "--plain" | "--stdout"));
    let full_logo = plain || args.iter().any(|arg| arg == "--full");
    let columns = args.iter().find_map(|arg| arg.strip_prefix("--startup-cols="))
        .and_then(|text| text.parse::<usize>().ok())
        .unwrap_or(80)
        .clamp(35, 240);

    let icon_mid = sst_icon_ansi(2);
    let icon_light = sst_icon_ansi(3);
    let info = if plain {
        vec![
            format!("{user}@{host}"),
            "────────────────────────────────────────".to_owned(),
            format!("OS: {}", truncate_fetch(&os, 58)),
            format!("Host: {host}"),
            format!("Kernel: {kernel}"),
            format!("Uptime: {uptime_text}"),
            "Shell: Shell Shock Tool / Nwash".to_owned(),
            format!("Version: {}", env!("CARGO_PKG_VERSION")),
            "Terminal: Shell Shock Native Terminal".to_owned(),
            format!("CPU: {}", truncate_fetch(&cpu, 58)),
            format!("Memory: {used:.2} GiB / {total:.2} GiB"),
            format!("Arch: {arch}"),
        ]
    } else {
        let title = format!(
            "\x1b[1m{icon_light}{user}{icon_mid}@{icon_light}{host}\x1b[0m"
        );
        let label = format!("\x1b[1m{icon_light}");
        let reset = "\x1b[0m";

        vec![
            title,
            format!("{icon_light}────────────────────────────────────────\x1b[0m"),
            format!("{label}OS:{reset} {}", truncate_fetch(&os, 58)),
            format!("{label}Host:{reset} {host}"),
            format!("{label}Kernel:{reset} {kernel}"),
            format!("{label}Uptime:{reset} {uptime_text}"),
            format!("{label}Shell:{reset} Shell Shock Tool / Nwash"),
            format!("{label}Version:{reset} {}", env!("CARGO_PKG_VERSION")),
            format!("{label}Terminal:{reset} Shell Shock Native Terminal"),
            format!("{label}CPU:{reset} {}", truncate_fetch(&cpu, 58)),
            format!("{label}Memory:{reset} {used:.2} GiB / {total:.2} GiB"),
            format!("{label}Arch:{reset} {arch}"),
        ]
    };


    if full_logo {
        // Keep the exact 37-line author-supplied artwork for --full and
        // --plain. It cannot fit inside a short interactive GUI viewport.
        let (logo_lines, _) = fetch_logo(plain)?;
        let mut out = logo_lines.join("\n");
        out.push('\n');
        out.push('\n');
        for line in &info {
            out.push_str(line);
            out.push('\n');
        }
        return Ok(CommandOutput::ok(out));
    }

    // The interactive startup must fit into the available terminal HEIGHT:
    // putting the logo above twelve metadata rows scrolls it out of sight
    // before the prompt is shown. Display the compact view *beside* seven
    // essential live facts; --full retains all metadata and the original art.
    let (logo_lines, logo_width) = fetch_compact_logo(false, columns)?;
    let compact_facts = [
        format!("{user}@{host}"),
        format!("OS: {os}"),
        format!("Kernel: {kernel} / {arch}"),
        format!("Uptime: {uptime_text}"),
        format!("Shell: SST/Nwash {}", env!("CARGO_PKG_VERSION")),
        format!("CPU: {cpu}"),
        format!("RAM: {used:.2} / {total:.2} GiB"),
        "Terminal: SST Native".to_owned(),
    ];
    let fact_width = columns.saturating_sub(logo_width + 2).max(8);
    let mut out = String::new();
    for (index, logo) in logo_lines.iter().enumerate() {
        out.push_str(logo);
        if let Some(fact) = compact_facts.get(index) {
            let fact = truncate_fetch(fact, fact_width);
            out.push_str("  ");
            if index == 0 {
                out.push_str(&format!("\x1b[1m{icon_light}{fact}\x1b[0m"));
            } else if let Some((name, value)) = fact.split_once(':') {
                out.push_str(&format!(
                    "\x1b[1m{icon_light}{name}:\x1b[0m{value}"
                ));
            } else {
                out.push_str(&fact);
            }
        }
        out.push('\n');
    }
    Ok(CommandOutput::ok(out))
}

/// Sample twice before reporting CPU, otherwise the first percentages are zero.
fn sampled_processes() -> System {
    let mut system = System::new_all();
    system.refresh_all();
    std::thread::sleep(Duration::from_millis(350));
    system.refresh_all();
    system
}

fn process_name_matches(name: &str, query: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let query = query.trim().to_ascii_lowercase();
    !query.is_empty()
        && (name.contains(&query)
            || (matches!(query.as_str(), "outlook" | "outlook new" | "outlook (new)")
                && matches!(name.as_str(), "olk.exe" | "outlook.exe")))
}

/// Match a process or any of its ancestors. Outlook's WebView2 children can
/// therefore be found without grouping unrelated Teams/Copilot WebView2s.
pub(crate) fn process_matches_filter(system: &System, pid: Pid, query: &str) -> bool {
    if query.trim().is_empty() { return false; }
    let mut candidate = Some(pid);
    let mut seen = HashSet::new();
    while let Some(id) = candidate {
        if !seen.insert(id) { break; }
        let Some(process) = system.process(id) else { break; };
        if process_name_matches(&process.name().to_string_lossy(), query)
            || process.exe().is_some_and(|path|
                path.to_string_lossy().to_ascii_lowercase().contains(&query.to_ascii_lowercase()))
        {
            return true;
        }
        candidate = process.parent();
    }
    false
}

fn sort_process_pids(system: &System, rows: &mut [Pid], sort_cpu: bool) {
    rows.sort_by(|a, b| {
        let (Some(pa), Some(pb)) = (system.process(*a), system.process(*b)) else {
            return a.as_u32().cmp(&b.as_u32());
        };
        let order = if sort_cpu {
            pb.cpu_usage().total_cmp(&pa.cpu_usage()).then_with(|| pb.memory().cmp(&pa.memory()))
        } else {
            pb.memory().cmp(&pa.memory()).then_with(|| pb.cpu_usage().total_cmp(&pa.cpu_usage()))
        };
        order.then_with(|| a.as_u32().cmp(&b.as_u32()))
    });
}

fn processes(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys processes [NOMBRE | --find NOMBRE] [--sort cpu|ram] [--top N] [--tree] [--csv]\n\
             Ejemplo: sys processes outlook | sys ps --find olk | sys ps --sort ram\n",
        ));
    }
    let mut limit = 50usize;
    let mut sort_cpu = true;
    let mut tree = false;
    let mut csv = false;
    let mut filter: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--csv" => csv = true,
            "--tree" => tree = true,
            "--find" => {
                i += 1;
                let query = args.get(i).map(String::as_str)
                    .filter(|value| !value.starts_with('-') && !value.trim().is_empty())
                    .ok_or_else(|| anyhow::anyhow!("sys processes: --find requiere un nombre"))?;
                anyhow::ensure!(filter.is_none(), "sys processes: filtro duplicado");
                filter = Some(query);
            }
            "--sort" => {
                i += 1;
                let sort = args.get(i).map(String::as_str)
                    .ok_or_else(|| anyhow::anyhow!("sys processes: --sort requiere cpu o ram"))?;
                anyhow::ensure!(matches!(sort, "cpu" | "ram"), "sys processes: orden inválido: {sort}");
                sort_cpu = sort == "cpu";
            }
            "--top" => {
                i += 1;
                let count = args.get(i)
                    .ok_or_else(|| anyhow::anyhow!("sys processes: --top requiere un número"))?;
                limit = count.parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("sys processes: --top inválido: {count}"))?;
                anyhow::ensure!((1..=1000).contains(&limit),
                    "sys processes: --top debe estar entre 1 y 1000");
            }
            name if !name.starts_with('-') => {
                anyhow::ensure!(filter.is_none(), "sys processes: filtro duplicado");
                filter = Some(name);
            }
            other => anyhow::bail!("sys processes: opción desconocida: {other}"),
        }
        i += 1;
    }
    let system = sampled_processes();
    let mut rows = if tree {
        process_tree_rows(&system, sort_cpu)
    } else {
        let mut pids = system.processes().keys().copied().collect::<Vec<_>>();
        sort_process_pids(&system, &mut pids, sort_cpu);
        pids.into_iter().map(|pid| (pid, String::new())).collect::<Vec<_>>()
    };
    if let Some(query) = filter {
        rows.retain(|(pid, _)| process_matches_filter(&system, *pid, query));
    }
    let total = rows.len();
    let mut out = if csv {
        String::from("PID,PPID,CPU_pct,RAM_MiB,Nombre\n")
    } else {
        format!("PROCESOS ACTUALES: {}{}\nPID      PPID     CPU%     RAM MiB   PROCESO\n",
            total, filter.map(|q| format!(" · filtro: {q}")).unwrap_or_default())
    };
    if total == 0 && !csv {
        out.push_str("Sin coincidencias entre los procesos actuales.\n");
    }
    for (pid, prefix) in rows.into_iter().take(limit) {
        let Some(process) = system.process(pid) else { continue; };
        let ppid = process.parent().map(|p| p.as_u32()).unwrap_or(0);
        if csv {
            writeln!(out, "{},{},{:.2},{:.2},{}",
                pid.as_u32(), ppid, process.cpu_usage(),
                process.memory() as f64 / 1024.0 / 1024.0,
                crate::support::csv::escape(&process.name().to_string_lossy()))?;
        } else {
            writeln!(out, "{:<8} {:<8} {:>6.1} {:>10.1}   {}{}",
                pid, ppid, process.cpu_usage(),
                process.memory() as f64 / 1024.0 / 1024.0,
                prefix, process.name().to_string_lossy())?;
        }
    }
    if total > limit && !csv {
        writeln!(out, "Mostrando {limit} de {total}; usa --top N para ampliar.")?;
    }
    Ok(CommandOutput::ok(out))
}

fn process_tree_rows(system: &System, sort_cpu: bool) -> Vec<(Pid, String)> {
    let existing = system.processes().keys().copied().collect::<HashSet<_>>();
    let mut children = HashMap::<Pid, Vec<Pid>>::new();
    let mut roots = Vec::new();

    for (pid, process) in system.processes() {
        match process.parent() {
            Some(parent) if parent != *pid && existing.contains(&parent) => {
                children.entry(parent).or_default().push(*pid);
            }
            _ => roots.push(*pid),
        }
    }

    let sort_pids = |pids: &mut Vec<Pid>| {
        pids.sort_by(|a, b| {
            let Some(pa) = system.process(*a) else { return std::cmp::Ordering::Greater; };
            let Some(pb) = system.process(*b) else { return std::cmp::Ordering::Less; };
            if sort_cpu {
                pb.cpu_usage()
                    .partial_cmp(&pa.cpu_usage())
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.as_u32().cmp(&b.as_u32()))
            } else {
                pb.memory()
                    .cmp(&pa.memory())
                    .then_with(|| a.as_u32().cmp(&b.as_u32()))
            }
        });
    };

    sort_pids(&mut roots);
    for values in children.values_mut() {
        sort_pids(values);
    }

    fn walk(
        pid: Pid,
        children: &HashMap<Pid, Vec<Pid>>,
        output: &mut Vec<(Pid, String)>,
        ancestor_last: &mut Vec<bool>,
        connector: Option<bool>,
        seen: &mut HashSet<Pid>,
    ) {
        if !seen.insert(pid) {
            return;
        }

        let mut prefix = String::new();
        if let Some(last) = connector {
            for ancestor_is_last in ancestor_last.iter().take(ancestor_last.len().saturating_sub(1)) {
                prefix.push_str(if *ancestor_is_last { "   " } else { "│  " });
            }
            prefix.push_str(if last { "└─ " } else { "├─ " });
        }
        output.push((pid, prefix));

        let Some(kids) = children.get(&pid) else { return; };
        for (index, child) in kids.iter().enumerate() {
            let is_last = index + 1 == kids.len();
            ancestor_last.push(is_last);
            walk(*child, children, output, ancestor_last, Some(is_last), seen);
            ancestor_last.pop();
        }
    }

    let mut output = Vec::with_capacity(system.processes().len());
    let mut seen = HashSet::new();
    for root in roots {
        walk(
            root,
            &children,
            &mut output,
            &mut Vec::new(),
            None,
            &mut seen,
        );
    }

    // Defensive fallback for malformed/cyclic parent relations.
    for pid in existing {
        if !seen.contains(&pid) {
            output.push((pid, "?! ".to_owned()));
        }
    }

    output
}

fn disks() -> anyhow::Result<CommandOutput> {
    let disks = Disks::new_with_refreshed_list();
    let mut out = String::from("MOUNT                 TOTAL GiB   USED GiB   FREE GiB   USE%   FS\n");

    for disk in disks.list() {
        let total = disk.total_space();
        let free = disk.available_space();
        let used = total.saturating_sub(free);
        let percent = if total == 0 {
            0.0
        } else {
            used as f64 * 100.0 / total as f64
        };

        out.push_str(&format!(
            "{:<20} {:>9.1} {:>9.1} {:>10.1} {:>5.1}%   {}\n",
            disk.mount_point().display(),
            total as f64 / 1024.0 / 1024.0 / 1024.0,
            used as f64 / 1024.0 / 1024.0 / 1024.0,
            free as f64 / 1024.0 / 1024.0 / 1024.0,
            percent,
            disk.file_system().to_string_lossy()
        ));
    }

    Ok(CommandOutput::ok(out))
}

fn memory() -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_memory();

    Ok(CommandOutput::ok(format!(
        "              total        used        free\nMem:      {:>10}  {:>10}  {:>10} MiB\nSwap:     {:>10}  {:>10}  {:>10} MiB\n",
        system.total_memory() / 1024 / 1024,
        system.used_memory() / 1024 / 1024,
        system.available_memory() / 1024 / 1024,
        system.total_swap() / 1024 / 1024,
        system.used_swap() / 1024 / 1024,
        system.total_swap().saturating_sub(system.used_swap()) / 1024 / 1024,
    )))
}

fn hostname() -> anyhow::Result<CommandOutput> {
    Ok(CommandOutput::ok(format!(
        "{}\n",
        System::host_name().unwrap_or_else(|| "desconocido".to_owned())
    )))
}

fn whoami() -> anyhow::Result<CommandOutput> {
    let user = env::var("USERNAME").unwrap_or_else(|_| "desconocido".to_owned());
    let domain = env::var("USERDOMAIN").unwrap_or_default();

    if domain.is_empty() || domain.eq_ignore_ascii_case(&user) {
        Ok(CommandOutput::ok(format!("{user}\n")))
    } else {
        Ok(CommandOutput::ok(format!(
            "{}\\{}\n",
            domain.to_ascii_lowercase(),
            user.to_ascii_lowercase()
        )))
    }
}

fn uname(args: &[String]) -> anyhow::Result<CommandOutput> {
    let all = args.iter().any(|arg| arg == "-a");
    let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_owned());
    let host = System::host_name().unwrap_or_else(|| "unknown".to_owned());
    let os = System::name().unwrap_or_else(|| "Windows".to_owned());
    let arch = env::consts::ARCH;

    if all {
        Ok(CommandOutput::ok(format!(
            "SST-Windows {host} {kernel} {arch} {os}\n"
        )))
    } else {
        Ok(CommandOutput::ok("SST-Windows\n"))
    }
}

fn broker_process(operation: crate::core::broker::Operation, args: &[String]) -> anyhow::Result<CommandOutput> {
    anyhow::ensure!(args.iter().filter(|arg| arg.as_str() == "--broker").count() <= 1, "duplicate --broker flag");
    let args = args.iter().filter(|arg| arg.as_str() != "--broker").cloned().collect::<Vec<_>>();
    #[cfg(windows)]
    { crate::adapters::windows::broker::command(operation, &args) }
    #[cfg(not(windows))]
    { let _ = (operation, args); Ok(CommandOutput::error("broker requires Windows", 3)) }
}

fn kill_process(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys kill — termina procesos mediante la API Win32\n\
             uso:\n\
               sys kill PID               termina exactamente ese PID\n\
               sys kill PID --tree        termina el PID y sus descendientes\n\
               sys kill NOMBRE            termina todos los procesos con ese nombre\n\
               sys kill NOMBRE --tree     termina todos los procesos con ese nombre y sus descendientes\n\
             ejemplos:\n\
               sys kill 14508 --tree\n\
               sys kill msedge.exe --tree\n"
        ));
    }

    let Some(target) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return Ok(CommandOutput::error(
            "kill: uso: sys kill PID|NOMBRE [--tree]",
            2,
        ));
    };

    let tree = args.iter().any(|arg| matches!(arg.as_str(), "--tree" | "-t"));
    let system = System::new_all();

    if let Ok(pid_value) = target.parse::<u32>() {
        let pid = Pid::from_u32(pid_value);

        let Some(process) = system.process(pid) else {
            return Ok(CommandOutput::error(
                format!("kill: no existe el proceso {pid_value}"),
                1,
            ));
        };

        let process_name = process.name().to_string_lossy().into_owned();

        if tree {
            let mut descendants = process_descendants(&system, pid);
            descendants.reverse();

            for child in descendants {
                match terminate_pid_native(child.as_u32()) {
                    Ok(()) => {}
                    Err(_) if process_is_gone(child.as_u32()) => {}
                    Err(error) => {
                        return Ok(CommandOutput::error(
                            format!(
                                "kill: no se pudo terminar el proceso hijo {} de {}: {error}",
                                child.as_u32(),
                                pid_value
                            ),
                            1,
                        ));
                    }
                }
            }
        }

        if let Err(error) = terminate_pid_native(pid_value) {
            return Ok(CommandOutput::error(
                format!("kill: no se pudo terminar {pid_value} ({process_name}): {error}"),
                1,
            ));
        }

        return Ok(CommandOutput::ok(format!(
            "terminated: {} {}{}\n",
            pid_value,
            process_name,
            if tree { " (process tree)" } else { "" }
        )));
    }

    let roots = matching_process_pids(&system, target);

    if roots.is_empty() {
        return Ok(CommandOutput::error(
            format!("kill: no hay procesos llamados {target}"),
            1,
        ));
    }

    let mut targets = std::collections::HashSet::new();
    for root in &roots {
        targets.insert(*root);
        if tree {
            for child in process_descendants(&system, *root) {
                targets.insert(child);
            }
        }
    }

    // Kill descendants before roots. A process that disappears as a side effect
    // of terminating another Edge/Chromium process is considered already done.
    let mut ordered = targets.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|pid| std::cmp::Reverse(process_depth(&system, *pid)));

    let mut killed = 0usize;
    let mut failures = Vec::new();
    for pid in ordered {
        match terminate_pid_native(pid.as_u32()) {
            Ok(()) => killed += 1,
            Err(_) if process_is_gone(pid.as_u32()) => {}
            Err(error) => failures.push(format!("{}: {error}", pid.as_u32())),
        }
    }

    // Chromium/Edge may create or respawn sibling processes while the first
    // batch is being terminated. Re-scan a few times until the requested name
    // is actually gone, or report exactly which PIDs survived.
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(80));
        let mut refreshed = System::new_all();
        refreshed.refresh_all();
        let remaining = matching_process_pids(&refreshed, target);

        if remaining.is_empty() {
            if !failures.is_empty() {
                failures.clear();
            }
            return Ok(CommandOutput::ok(format!(
                "terminated: {killed} process(es) matching {}{}\n",
                target,
                if tree { " (including process trees)" } else { "" }
            )));
        }

        for pid in remaining {
            match terminate_pid_native(pid.as_u32()) {
                Ok(()) => killed += 1,
                Err(_) if process_is_gone(pid.as_u32()) => {}
                Err(error) => failures.push(format!("{}: {error}", pid.as_u32())),
            }
        }
    }

    let mut refreshed = System::new_all();
    refreshed.refresh_all();
    let survivors = matching_process_pids(&refreshed, target);

    if survivors.is_empty() {
        Ok(CommandOutput::ok(format!(
            "terminated: {killed} process(es) matching {}{}\n",
            target,
            if tree { " (including process trees)" } else { "" }
        )))
    } else {
        Ok(CommandOutput::error(
            format!(
                "kill: aún siguen activos {} proceso(s) {}: {}{}",
                survivors.len(),
                target,
                survivors.iter().map(|pid| pid.as_u32().to_string()).collect::<Vec<_>>().join(", "),
                if failures.is_empty() {
                    String::new()
                } else {
                    format!("; errores: {}", failures.join("; "))
                }
            ),
            1,
        ))
    }
}

fn matching_process_pids(system: &System, target: &str) -> Vec<Pid> {
    let wanted = target.trim_end_matches(".exe");
    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let name = process.name().to_string_lossy();
            let normalized = name.trim_end_matches(".exe");
            (name.eq_ignore_ascii_case(target) || normalized.eq_ignore_ascii_case(wanted))
                .then_some(*pid)
        })
        .collect()
}

fn process_depth(system: &System, pid: Pid) -> usize {
    let mut depth = 0usize;
    let mut current = pid;
    let mut seen = std::collections::HashSet::new();

    while seen.insert(current) {
        let Some(parent) = system.process(current).and_then(|process| process.parent()) else {
            break;
        };
        depth += 1;
        current = parent;
    }

    depth
}

#[cfg(windows)]
fn process_is_gone(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return true;
        }
        CloseHandle(handle);
        false
    }
}

#[cfg(not(windows))]
fn process_is_gone(_pid: u32) -> bool {
    false
}

fn process_descendants(system: &System, root: Pid) -> Vec<Pid> {
    let mut result = Vec::new();
    let mut pending = vec![root];

    while let Some(parent) = pending.pop() {
        let children: Vec<Pid> = system
            .processes()
            .iter()
            .filter_map(|(pid, process)| (process.parent() == Some(parent)).then_some(*pid))
            .collect();

        for child in children {
            result.push(child);
            pending.push(child);
        }
    }

    result
}

#[cfg(windows)]
fn terminate_pid_native(pid: u32) -> anyhow::Result<()> {
    const STILL_ACTIVE_CODE: u32 = 259;

    unsafe {
        let handle = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );

        if handle.is_null() {
            let error = GetLastError();
            if error == 5 {
                anyhow::bail!(
                    "acceso denegado (Win32 5); usa sudo sys kill {pid}"
                );
            }
            anyhow::bail!("OpenProcess falló con error Win32 {error}");
        }

        if TerminateProcess(handle, 1) == 0 {
            let error = GetLastError();
            CloseHandle(handle);
            anyhow::bail!("TerminateProcess falló con error Win32 {error}");
        }

        let mut terminated = false;
        for _ in 0..20 {
            let mut exit_code = STILL_ACTIVE_CODE;
            if GetExitCodeProcess(handle, &mut exit_code) == 0 {
                let error = GetLastError();
                CloseHandle(handle);
                anyhow::bail!("GetExitCodeProcess falló con error Win32 {error}");
            }

            if exit_code != STILL_ACTIVE_CODE {
                terminated = true;
                break;
            }

            std::thread::sleep(Duration::from_millis(25));
        }

        CloseHandle(handle);

        if !terminated {
            anyhow::bail!("el proceso sigue activo después de TerminateProcess");
        }
    }

    Ok(())
}

#[cfg(not(windows))]
fn terminate_pid_native(pid: u32) -> anyhow::Result<()> {
    anyhow::bail!("la terminación nativa por PID solo está disponible en Windows: {pid}")
}

#[cfg(test)]
mod fetch_text_logo_tests {
    use super::{fetch_compact_logo, fetch_logo, sst_icon_truecolor};

    #[test]
    fn compact_logo_fits_both_viewports() {
        let (narrow, width) = fetch_compact_logo(true, 58).unwrap();
        assert_eq!(narrow.len(), 12);
        assert_eq!(width, 30);
        assert!(narrow.iter().all(|line| line.chars().count() == width));
        assert!(narrow.iter().any(|line| line.chars().any(|ch| ch != ' ')));
        let (wide, wide_width) = fetch_compact_logo(true, 80).unwrap();
        assert_eq!(wide.len(), 12);
        assert_eq!(wide_width, 36);
        assert!(wide.iter().all(|line| line.chars().count() == wide_width));
    }

    #[test]
    fn original_logo_remains_unchanged_in_plain_mode() {
        let (lines, width) = fetch_logo(true).unwrap();
        assert_eq!(lines.len(), 37);
        assert_eq!(width, 100);
        assert!(lines.iter().all(|line| line.chars().count() == width));
        assert!(lines.iter().any(|line| line.contains("%%%%")));
    }

    #[test]
    fn original_ascii_asset_is_preserved_byte_for_byte() {
        let original = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ascii-art.txt"));
        let (lines, _) = fetch_logo(true).unwrap();
        assert_eq!(lines.join("\n"), original.trim_end_matches(['\r', '\n']).replace("\r\n", "\n"));
    }

    #[test]
    fn original_blue_silver_palette_is_applied_to_ascii_marks() {
        for columns in [58, 80] {
            let (plain, width) = fetch_compact_logo(true, columns).unwrap();
            let rows = sst_icon_truecolor(plain.clone());
            let mut term = vt100::Parser::new(13, 48, 0);
            term.process(rows.join("\r\n").as_bytes());
            assert!(plain.iter().any(|row| row.contains('█')));
            for (y, row) in plain.iter().enumerate() {
                for (x, glyph) in row.chars().enumerate() {
                    assert_eq!(term.screen().cell(y as u16, x as u16).unwrap().contents(),
                               glyph.to_string());
                }
            }
            assert_eq!(term.screen().cell(2, width as u16).unwrap().bgcolor(), vt100::Color::Default);
            let ansi = rows.join("");
            for color in ["38;2;35;52;106", "48;2;186;189;198", "48;2;251;242;54",
                          "38;2;6;48;185", "38;2;17;30;70"] {
                assert!(ansi.contains(color), "{columns} columns missing {color}");
            }
        }
    }

    #[test]
    fn compact_fetch_keeps_geometry_and_uses_icon_colors() {
        let (plain, width) = fetch_compact_logo(true, 80).unwrap();
        let (colored, colored_width) = fetch_compact_logo(false, 80).unwrap();
        assert_eq!(width, colored_width);
        assert_eq!(plain.len(), 12);
        assert_eq!(colored.len(), plain.len());
        assert!(colored.join("\n").contains("\x1b[38;2;"));
    }

    #[test]
    fn colored_logo_renders_with_libcaca() {
        let (lines, width) = fetch_logo(false).unwrap();
        assert_eq!(width, 100);
        assert!(!lines.is_empty());
        assert!(lines.join("\n").contains('%'));
    }
}

#[cfg(test)]
mod process_filter_tests {
    use super::process_name_matches;

    #[test]
    fn outlook_alias_matches_olk_but_not_unrelated_webview() {
        assert!(process_name_matches("olk.exe", "outlook"));
        assert!(process_name_matches("OUTLOOK.EXE", "Outlook"));
        assert!(!process_name_matches("msedgewebview2.exe", "outlook"));
        assert!(!process_name_matches("ms-teams.exe", "outlook"));
        assert!(process_name_matches("msedgewebview2.exe", "webview"));
    }
}
