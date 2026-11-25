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
        let mut music = crate::application::keygen_audio::KeygenMusic::new(first)?;

        #[cfg(windows)]
        if env::var_os("SST_KEYGEN_TERMINAL").is_none() {
            return keygen_sdl(music);
        }

        let mut terminal = self.terminal.alternate_screen()?;
        let mut system = System::new_all();
        system.refresh_all();
        let mut frame = 0u64;
        let mut glyph_physics = KeygenPhysics::default();
        loop {
            if frame % 15 == 0 && !music.is_paused() {
                system.refresh_cpu_all();
                system.refresh_memory();
            }
            let (cols, rows) = terminal.size()?;
            let facts = keygen_system_facts(&system);
            let mut screen = keygen_frame(cols, rows, frame, music.is_paused(), &facts, &mut glyph_physics);
            // Keep the player state visible even when changing tracks.
            let status = format!("  [MP3 {} / 2]  1/2: pista  Espacio: pausa  ESC/Q: salir",
                music.track_number());
            screen.push_str(&format!("\x1b[{};1H\x1b[0m\x1b[38;2;189;174;147m{}\x1b[0m",
                rows, status.chars().take(usize::from(cols.saturating_sub(1))).collect::<String>()));
            terminal.write(&screen)?;
            terminal.flush()?;
            match terminal.poll_key(Duration::from_millis(85))? {
                Some(TerminalKey::Escape | TerminalKey::Char('q') | TerminalKey::Char('Q')) => break,
                Some(TerminalKey::Space | TerminalKey::Char(' ')) =>
                    music.pause_resume()?,
                Some(TerminalKey::Char('1')) => music.select(0)?,
                Some(TerminalKey::Char('2')) => music.select(1)?,
                _ => {}
            }
            if !music.is_paused() {
                frame = frame.wrapping_add(1);
                glyph_physics.tick(frame);
            }
            if frame % 5 == 0 { music.tick()?; }
        }
        // Both resources are dropped here; the music handle is closed even on
        // early exit (including resize/terminal errors).
        Ok(CommandOutput::ok(""))
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
/// Render into the native alternate-screen terminal, not a GUI overlay.
fn keygen_glyph_color(frame: u64, index: usize, row: usize) -> (u8, u8, u8) {
    let phase = frame as f32 * 0.115 + index as f32 * 0.19 + row as f32 * 0.55;
    let chan = |shift: f32| -> u8 {
        (118.0 + 115.0 * (phase + shift).sin()).clamp(0.0, 255.0) as u8
    };
    (chan(0.0), chan(2.094), chan(4.188))
}


#[derive(Clone, Copy)]
struct KeygenGlyphBody {
    x: f32,
    y: f32,
    z: f32,
    vx: f32,
    vy: f32,
    vz: f32,
    // Orientation and angular velocity of each independent 3D rigid glyph.
    rx: f32,
    ry: f32,
    rz: f32,
    wx: f32,
    wy: f32,
    wz: f32,
}

impl KeygenGlyphBody {
    fn new(line: usize, index: usize) -> Self {
        let seed = (index.wrapping_mul(17) + line.wrapping_mul(29)) % 23;
        Self {
            x: 0.0,
            y: -((seed % 7) as f32 * 0.28),
            z: (seed % 9) as f32 * 0.13 - 0.52,
            vx: (seed % 5) as f32 * 0.16 - 0.32,
            vy: -1.5 - (seed % 8) as f32 * 0.29,
            vz: 0.5 - (seed % 6) as f32 * 0.17,
            rx: seed as f32 * 0.045,
            ry: (seed % 8) as f32 * 0.13,
            rz: -(seed as f32) * 0.025,
            wx: 0.13 - (seed % 4) as f32 * 0.07,
            wy: (seed % 5) as f32 * 0.14 - 0.28,
            wz: (seed % 7) as f32 * 0.09 - 0.27,
        }
    }
}

#[derive(Default)]
struct KeygenPhysics {
    bodies: HashMap<(usize, usize), KeygenGlyphBody>,
    rendered_angles: HashMap<(usize, usize), f64>,
}

impl KeygenPhysics {
    fn tick(&mut self, frame: u64) {
        // Fixed-step integration: the letters really retain positions and
        // velocities between frames, rather than sampling a scrolling wave.
        let dt = 0.085_f32;
        for (&(line, index), body) in &mut self.bodies {
            let period = 34 + ((index * 3 + line * 11) % 23) as u64;
            if frame % period == (index as u64 + line as u64 * 3) % period {
                body.vy -= 2.4 + (index % 6) as f32 * 0.27;
                body.vx += if (index + line) % 2 == 0 { 0.9 } else { -0.9 };
                body.vz += if index % 2 == 0 { 0.75 } else { -0.75 };
                body.wy += if index % 2 == 0 { 0.55 } else { -0.55 };
                body.wz += if line % 2 == 0 { 0.2 } else { -0.2 };
            }
            // Damped springs on X/Z, gravity and restitution on Y.
            body.vx += (-3.2 * body.x - 1.0 * body.vx) * dt;
            body.vy += 6.8 * dt;
            body.vz += (-2.4 * body.z - 0.8 * body.vz) * dt;
            body.x += body.vx * dt;
            body.y += body.vy * dt;
            body.z += body.vz * dt;
            // Rotations are stateful, with angular inertia and air drag.
            body.rx += body.wx * dt;
            body.ry += body.wy * dt;
            body.rz += body.wz * dt;
            body.wx *= 0.997;
            body.wy *= 0.997;
            body.wz *= 0.997;
            if body.y >= 0.0 {
                body.y = 0.0;
                let impact = body.vy.abs();
                body.vy = -impact * 0.58;
                if impact > 0.15 {
                    body.wx += body.vz * 0.07;
                    body.wz -= body.vx * 0.09;
                }
                if body.vy.abs() < 0.08 { body.vy = 0.0; }
            }
        }
        // Resolve neighboring letters colliding along their horizontal axes.
        // Apply equal-and-opposite impulses, preserving independent motion.
        let keys = self.bodies.keys().copied().collect::<Vec<_>>();
        let mut impulses = Vec::<((usize, usize), f32)>::new();
        for &(line, index) in &keys {
            let Some(left) = self.bodies.get(&(line, index)) else { continue; };
            let Some(right) = self.bodies.get(&(line, index + 1)) else { continue; };
            let separation = 1.0 + right.x - left.x;
            if separation < 0.68 && (left.y - right.y).abs() < 0.9
                && (left.z - right.z).abs() < 1.0
            {
                let impulse = (0.68 - separation) * 1.2;
                impulses.push(((line, index), -impulse));
                impulses.push(((line, index + 1), impulse));
            }
        }
        for (key, impulse) in impulses {
            if let Some(body) = self.bodies.get_mut(&key) {
                body.vx += impulse;
                body.wy += impulse * 0.18;
                body.wz += impulse * 0.11;
            }
        }
    }

    fn body(&mut self, line: usize, index: usize) -> KeygenGlyphBody {
        *self.bodies.entry((line,index))
            .or_insert_with(|| KeygenGlyphBody::new(line,index))
    }
}


#[cfg(windows)]
fn keygen_sdl(
    mut music: crate::application::keygen_audio::KeygenMusic,
) -> anyhow::Result<CommandOutput> {
    use sdl2::{
        event::Event, keyboard::Keycode,
        pixels::{Color, PixelFormatEnum},
        rect::Rect, render::BlendMode,
    };
    use std::time::Instant;

    // The original SST animation/physics runs unchanged. SDL2 owns the native
    // window and renders actual pixel glyphs; libcaca owns the character
    // composition layer. No subprocess, SDL2.dll or external player.
    let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
    let video = sdl.video().map_err(anyhow::Error::msg)?;
    let window = video.window("SST KEYGEN — libcaca / SDL2", 1152, 672)
        .position_centered().resizable().build()
        .map_err(|e| anyhow::anyhow!("keygen: SDL2 window: {e}"))?;
    let mut renderer = window.into_canvas().software().build()
        .map_err(|e| anyhow::anyhow!("keygen: SDL2 software renderer: {e}"))?;
    renderer.set_blend_mode(BlendMode::Blend);
    let creator = renderer.texture_creator();
    let font_bytes = [
        r"C:\Windows\Fonts\consola.ttf",
        r"C:\Windows\Fonts\lucon.ttf",
        r"C:\Windows\Fonts\cour.ttf",
    ].into_iter().find_map(|p| std::fs::read(p).ok())
        .ok_or_else(|| anyhow::anyhow!("keygen: falta una fuente monoespaciada Windows"))?;
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!("keygen: fuente Windows inválida: {e}"))?;
    // Cache the actual glyph bitmaps. Per-frame only texture color and
    // position change, so animations do not rasterize fonts repeatedly.
    let mut glyph_cache = HashMap::new();
    let mut events = sdl.event_pump().map_err(anyhow::Error::msg)?;
    let mut system = System::new_all();
    system.refresh_all();
    let mut physics = KeygenPhysics::default();
    let mut frame = 0u64;
    let mut last_redraw = Instant::now();
    'running: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown { keycode: Some(Keycode::Escape | Keycode::Q), .. } => {
                    break 'running;
                }
                Event::KeyDown { keycode: Some(Keycode::Space), repeat: false, .. } => {
                    music.pause_resume()?;
                }
                Event::KeyDown { keycode: Some(Keycode::Num1 | Keycode::Kp1), repeat: false, .. } => {
                    music.select(0)?;
                }
                Event::KeyDown { keycode: Some(Keycode::Num2 | Keycode::Kp2), repeat: false, .. } => {
                    music.select(1)?;
                }
                _ => {}
            }
        }
        if last_redraw.elapsed() < Duration::from_millis(33) {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        last_redraw = Instant::now();
        if frame % 15 == 0 && !music.is_paused() {
            system.refresh_cpu_all();
            system.refresh_memory();
        }
        let (w, h) = renderer.output_size().map_err(anyhow::Error::msg)?;
        let cols = (w / 14).clamp(48, 150) as u16;
        let rows = (h / 26).clamp(18, 60) as u16;
        let cell_w = (w / u32::from(cols)).max(1);
        let cell_h = (h / u32::from(rows)).max(1);

        let facts = keygen_system_facts(&system);
        let frame_ansi = keygen_frame(
            cols, rows, frame, music.is_paused(), &facts, &mut physics,
        );
        // Decode the exact 24-bit color and geometry of the SST frame.
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(frame_ansi.as_bytes());
        let screen = parser.screen();
        let mut ascii = libcaca::Canvas::new(i32::from(cols), i32::from(rows))
            .map_err(|e| anyhow::anyhow!("keygen: libcaca canvas: {e}"))?;
        let mut colors = HashMap::new();
        for row in 0..rows {
            for col in 0..cols {
                if let Some(cell) = screen.cell(row, col)
                    && let Some(ch) = cell.contents().chars().next()
                    && !ch.is_control() && ch != ' '
                {
                    ascii.put_char(i32::from(col), i32::from(row), ch as u32);
                    let rgb = match cell.fgcolor() {
                        vt100::Color::Rgb(r, g, b) => (r, g, b),
                        _ => (189, 174, 147),
                    };
                    colors.insert((row, col), rgb);
                }
            }
        }
        renderer.set_draw_color(Color::RGB(12, 12, 12));
        renderer.clear();
        for row in 0..rows {
            for col in 0..cols {
                let code = ascii.get_char(i32::from(col), i32::from(row));
                let Some(ch) = char::from_u32(code) else { continue };
                if ch == ' ' || ch.is_control() { continue; }
                if !glyph_cache.contains_key(&ch) {
                    let (metrics, bitmap) = font.rasterize(ch, 25.0);
                    let tw = metrics.width.max(1) as u32;
                    let th = metrics.height.max(1) as u32;
                    let mut texture = creator.create_texture_streaming(
                        PixelFormatEnum::RGBA8888, tw, th,
                    ).map_err(|e| anyhow::anyhow!("keygen: textura SDL2: {e}"))?;
                    texture.set_blend_mode(BlendMode::Blend);
                    texture.with_lock(None, |buffer, pitch| {
                        for y in 0..metrics.height {
                            for x in 0..metrics.width {
                                let alpha = bitmap[y * metrics.width + x];
                                let off = y * pitch + x * 4;
                                // SDL RGBA8888 is ABGR bytes in little-endian.
                                buffer[off..off + 4].copy_from_slice(&[alpha, 255, 255, 255]);
                            }
                        }
                    }).map_err(anyhow::Error::msg)?;
                    glyph_cache.insert(ch, (texture, tw, th));
                }
                let Some((texture, tw, th)) = glyph_cache.get_mut(&ch) else { continue };
                let (r, g, b) = colors.get(&(row, col)).copied()
                    .unwrap_or((189, 174, 147));
                texture.set_color_mod(r, g, b);
                let x = i32::from(col) * cell_w as i32
                    + (cell_w as i32 - *tw as i32) / 2;
                let y = i32::from(row) * cell_h as i32
                    + (cell_h as i32 - *th as i32) / 2;
                let angle = physics.rendered_angles
                    .get(&(row as usize, col as usize)).copied().unwrap_or(0.0);
                renderer.copy_ex(texture, None, Rect::new(x, y, *tw, *th),
                    angle, None, false, false).map_err(anyhow::Error::msg)?;
            }
        }
        renderer.present();
        if !music.is_paused() {
            frame = frame.wrapping_add(1);
            physics.tick(frame);
        }
        if frame % 5 == 0 { music.tick()?; }
    }
    // SDL renderer and MCI audio both have scoped ownership, so closing the
    // window stops playback and returns to SST without an orphan process.
    Ok(CommandOutput::ok(""))
}

fn keygen_frame(
    cols: u16, rows: u16, frame: u64, paused: bool, facts: &[String],
    physics: &mut KeygenPhysics,
) -> String {
    let width = usize::from(cols);
    let height = usize::from(rows);
    physics.rendered_angles.clear();
    let mut output = String::from("\x1b[0m\x1b[2J\x1b[H");
    if width < 22 || height < 7 {
        output.push_str("KEYGEN SST - amplia la ventana");
        return output;
    }
    let title = "S H E L L   S H O C K   T O O L";
    let subtitle = "S S T  /  K E Y G E N   2 0 0 0";
    let mid = height / 2;
    let title_y = mid.saturating_sub((facts.len() + 8) / 2 + 3)
        .saturating_add(4).min(height.saturating_sub(12)).max(3);
    let mut draw = |line: &str, line_id: usize, base_y: usize, arc: f32| {
        let chars: Vec<char> = line.chars().take(width.saturating_sub(2)).collect();
        let count = chars.len() as f32;
        let center = (width as f32 - 1.0) * 0.5;
        let half = (count * 0.5).max(1.0);
        for (index, ch) in chars.into_iter().enumerate() {
            if ch.is_control() || ch == ' ' { continue; }
            let body = physics.body(line_id,index);
            let rest_x = index as f32 - (count - 1.0) * 0.5;
            let arc_lift = (1.0 - (rest_x / half).powi(2)).max(0.0) * arc * 2.7;
            // Perspective projection from real 3D state onto terminal cells.
            let distance = (3.0 - body.z).max(1.0);
            let projection = 2.5 / distance;
            let rx = body.rx.sin() * 0.45;
            let ry = body.ry.sin() * 0.7;
            let rz = body.rz.sin() * 0.35;
            let px = (center + (rest_x + body.x + ry) * projection).round() as isize;
            let py = (base_y as f32 + body.y * 1.45 - arc_lift
                - body.z * 0.7 + rx + rz).round() as isize;
            if px < 0 || px >= width as isize || py < 1 || py >= height as isize-1 {
                continue;
            }
            physics.rendered_angles.insert((py as usize, px as usize), f64::from(body.rz.to_degrees()));
            let (r0,g0,b0) = keygen_glyph_color(frame,index,base_y);
            let light = projection.clamp(0.4,1.3);
            let tone = |channel:u8| -> u8 {
                ((channel as f32 * light).clamp(15.0,255.0)) as u8
            };
            let (r,g,b) = (tone(r0),tone(g0),tone(b0));
            if body.y < -0.3 && base_y < height.saturating_sub(1) {
                let shadow_x = (px + (body.z * 1.2).round() as isize)
                    .clamp(0,width.saturating_sub(1) as isize);
                output.push_str(&format!(
                    "\x1b[{};{}H\x1b[38;2;66;61;58m.",
                    base_y,shadow_x+1
                ));
            }
            // A depth layer moves with the yaw and Z coordinate so letters
            // have visible volume, not just a sinusoidal 2D position offset.
            let depth_x = if body.ry.cos() >= 0.0 { 1 } else { -1 };
            let back_x = px + depth_x;
            let back_y = py + 1 + if body.rx.sin() < -0.7 { -1 } else { 0 };
            if back_x >= 0 && back_x < width as isize
                && back_y >= 1 && back_y < height as isize - 1
            {
                let dim = |value: u8| -> u8 { (value as f32 * 0.38).round() as u8 };
                let (sr, sg, sb) = (dim(r), dim(g), dim(b));
                output.push_str(&format!(
                    "\x1b[{};{}H\x1b[38;2;{sr};{sg};{sb}m▓",
                    back_y + 1, back_x + 1
                ));
            }
            output.push_str(&format!(
                "\x1b[{};{}H\x1b[1;38;2;{r};{g};{b}m{ch}",
                py+1,px+1
            ));
        }
    };
    draw(title,0,title_y,2.0);
    draw(subtitle,1,title_y+4,1.0);
    let available = height.saturating_sub(title_y+8);
    let gap = if available >= facts.len()*2+2 {2} else {1};
    for (i,fact) in facts.iter().enumerate() {
        let row = title_y+7+i*gap;
        if row >= height.saturating_sub(2) {break;}
        draw(fact,2+i,row,0.0);
    }
    let footer = if paused {"PAUSA  [SPACE] continuar  [ESC/Q] salir"}
        else {"SST KEYGEN  [SPACE] pausa  [ESC/Q] salir"};
    output.push_str(&format!(
        "\x1b[{};1H\x1b[0m\x1b[38;2;189;174;147m{}",
        height,footer.chars().take(width.saturating_sub(1)).collect::<String>()
    ));
    output.push_str("\x1b[0m");
    output
}


#[cfg(test)]
mod keygen_motion_regressions {
    use super::{KeygenPhysics, keygen_frame};

    #[test]
    fn letters_keep_independent_finite_velocity_and_bounce_above_floor() {
        let mut physics = KeygenPhysics::default();
        physics.body(0, 0);
        physics.body(0, 1);
        physics.body(1, 0);
        for frame in 0..400 {
            physics.tick(frame);
            for body in physics.bodies.values() {
                assert!(body.x.is_finite() && body.y.is_finite() && body.z.is_finite());
                assert!(body.vx.is_finite() && body.vy.is_finite() && body.vz.is_finite());
                assert!(body.y <= 0.001, "glyph moved through the virtual floor");
                assert!(body.rx.is_finite() && body.ry.is_finite() && body.rz.is_finite());
                assert!(body.wx.is_finite() && body.wy.is_finite() && body.wz.is_finite());
            }
        }
        let one = physics.bodies.get(&(0,0)).expect("first glyph");
        let two = physics.bodies.get(&(0,1)).expect("second glyph");
        assert!((one.x - two.x).abs() > 0.0001 || (one.z - two.z).abs() > 0.0001);
        assert!((one.ry - two.ry).abs() > 0.0001 || (one.rz - two.rz).abs() > 0.0001);
    }

    #[test]
    fn projected_frame_keeps_controls_and_real_glyphs() {
        let mut physics = KeygenPhysics::default();
        let frames = ["CPU :: 23%".to_owned(),"RAM :: 4 GB".to_owned()];
        let rendered = keygen_frame(112,31,0,false,&frames,&mut physics);
        assert!(rendered.contains("\x1b[0m"));
        assert!(rendered.contains("ESC/Q"));
        assert!(rendered.contains("\x1b[1;38;2;"));
        assert!(!physics.bodies.is_empty());
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

/// Dense percent marks are the lettering/frame; the hyphen field is blue.
/// Shade marks retain intermediate colors without altering the source.
fn sst_icon_rgb(glyph: char) -> Option<(u8, u8, u8)> {
    let index = match glyph {
        '-' | ':' => 0,
        '=' | '+' => 1,
        '*' | '#' => 2,
        '%' => 3,
        _ => return None,
    };
    Some(SST_LOGO_COLORS[index])
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

fn paint_logo_row(cells: impl IntoIterator<Item = (char, Option<(u8, u8, u8)>)>) -> String {
    let mut out = String::new();
    let mut current = None;
    for (glyph, color) in cells {
        if color != current {
            if let Some((r, g, b)) = color {
                out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            } else if current.is_some() {
                out.push_str("\x1b[0m");
            }
            current = color;
        }
        out.push(glyph);
    }
    if current.is_some() { out.push_str("\x1b[0m"); }
    out
}

fn sst_icon_truecolor(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|line| {
        paint_logo_row(line.chars().map(|glyph| (glyph, sst_icon_rgb(glyph))))
    }).collect()
}

/// The compact renderer scales an ASCII-cell grid, not a PNG or ICO.
/// Each ASCII character has a unique cell ID so SDL2 cannot invert the
/// letters and background or mix their colors during the resize.
const SST_ASCII_GLYPHS: [char; 8] = [' ', '-', ':', '=', '+', '*', '#', '%'];

fn compact_ascii_cells(width: usize, height: usize) -> Vec<char> {
    let source_width = FETCH_LOGO_ASCII[0].len();
    let source_height = FETCH_LOGO_ASCII.len();
    let grid = image::RgbaImage::from_fn(source_width as u32, source_height as u32, |x, y| {
        let glyph = char::from(FETCH_LOGO_ASCII[y as usize].as_bytes()[x as usize]);
        let index = SST_ASCII_GLYPHS.iter().position(|&ch| ch == glyph)
            .expect("unexpected character in original SST ASCII");
        image::Rgba([index as u8, 0, 0, 255])
    });
    #[cfg(windows)]
    if let Some(pixels) = sst_logo_sdl_pixels(&grid, width, height) {
        return pixels.into_iter().map(|(id, _, _, _)|
            SST_ASCII_GLYPHS[usize::from(id).min(SST_ASCII_GLYPHS.len() - 1)]).collect();
    }
    image::imageops::resize(&grid, width as u32, height as u32,
        image::imageops::FilterType::Nearest).pixels()
        .map(|p| SST_ASCII_GLYPHS[usize::from(p[0])]).collect()
}

fn caca_ansi(canvas: &libcaca::Canvas) -> anyhow::Result<String> {
    Ok(String::from_utf8(canvas.export_to_memory("utf8")?)?)
}

/// Render compact text from the author's original ASCII via SDL2 and libcaca.
fn fetch_compact_logo(plain: bool, columns: usize) -> anyhow::Result<(Vec<String>, usize)> {
    let width = if columns < 70 { 30usize } else { 36usize };
    let height = 12usize;
    let cells = compact_ascii_cells(width, height);
    let mut cv = libcaca::Canvas::new(width as i32, height as i32)?;
    cv.set_color_ansi(libcaca::Color::Default, libcaca::Color::Default)?;
    for (i, &glyph) in cells.iter().enumerate() {
        cv.put_char((i % width) as i32, (i / width) as i32, glyph as u32);
    }
    let rendered = caca_ansi(&cv)?;
    let mut rendered_rows = rendered.lines();
    let rows = (0..height).map(|_| {
        rendered_rows.next().unwrap_or("").chars()
            .chain(std::iter::repeat(' ')).take(width).collect::<String>()
    }).collect::<Vec<_>>();
    Ok((if plain { rows } else { sst_icon_truecolor(rows) }, width))
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
    use super::{fetch_compact_logo, fetch_logo, sst_icon_rgb, sst_icon_truecolor};

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
        assert_eq!(sst_icon_rgb('%'), Some((186, 189, 198)));
        assert_eq!(sst_icon_rgb('-'), Some((35, 52, 106)));
        assert_eq!(sst_icon_rgb(' '), None);
        let painted = sst_icon_truecolor(vec!["%-%".to_owned()]).join("\n");
        assert!(painted.contains("\x1b[38;2;186;189;198m%"));
        assert!(painted.contains("\x1b[38;2;35;52;106m-"));
    }

    #[test]
    fn compact_logo_geometry_comes_only_from_original_ascii() {
        let (rows, width) = fetch_compact_logo(true, 80).unwrap();
        let cells = super::compact_ascii_cells(width, rows.len());
        assert_eq!(rows.iter().flat_map(|row| row.chars()).collect::<Vec<_>>(), cells);
        assert!(cells.iter().all(|ch| super::SST_ASCII_GLYPHS.contains(ch)));
        assert!(cells.contains(&'%'));
        assert!(cells.contains(&'-'));
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
