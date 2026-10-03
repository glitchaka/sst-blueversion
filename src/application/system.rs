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
        let sub = args.first().map(String::as_str).unwrap_or("info");

        match sub {
            "info" => info(),
            "processes" | "ps" => processes(),
            "top" => self.top(),
            "disks" | "df" => disks(),
            "memory" | "free" => memory(),
            "hostname" => hostname(),
            "whoami" => whoami(),
            "uname" => uname(&args[1..]),
            "kill" if args[1..].iter().any(|arg| arg == "--broker") => broker_process(crate::core::broker::Operation::Kill, &args[1..]),
            "kill" => kill_process(&args[1..]),
            "fetch" | "neofetch" | "fastfetch" => fetch(&args[1..]),
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
            "ps" => processes(),
            "top" => self.top(),
            "df" => disks(),
            "free" => memory(),
            "hostname" => hostname(),
            "whoami" => whoami(),
            "uname" => uname(args),
            "kill" => kill_process(args),
            "uptime" => uptime(),
            "fetch" | "neofetch" | "fastfetch" => fetch(args),
            _ => Ok(CommandOutput::error(
                format!("comando de sistema desconocido: {name}"),
                127,
            )),
        }
    }

    fn top(&self) -> anyhow::Result<CommandOutput> {
        let mut terminal = self.terminal.alternate_screen()?;
        let mut sort_cpu = true;

        loop {
            let mut system = System::new_all();
            system.refresh_all();

            let rows = process_tree_rows(&system, sort_cpu);
            let (width, height) = terminal.size()?;
            let max_rows = height.saturating_sub(6) as usize;
            let mut screen = String::new();

            writeln!(
                screen,
                "SST top  uptime {}s  CPU {} cores  Mem {} / {} MiB",
                System::uptime(),
                system.cpus().len(),
                system.used_memory() / 1024 / 1024,
                system.total_memory() / 1024 / 1024
            )?;
            writeln!(
                screen,
                "árbol de procesos · hermanos por {}   [c] CPU  [m] memoria  [q] salir",
                if sort_cpu { "CPU" } else { "MEM" }
            )?;
            writeln!(screen)?;
            writeln!(screen, "PID      PPID     CPU%     RAM MiB   PROCESS")?;

            for (pid, tree_prefix) in rows.into_iter().take(max_rows) {
                let Some(process) = system.process(pid) else { continue; };
                let ppid = process.parent().map(|p| p.as_u32()).unwrap_or(0);
                let mut name = format!("{tree_prefix}{}", process.name().to_string_lossy());
                let name_width = width.saturating_sub(41) as usize;

                if name.chars().count() > name_width && name_width > 3 {
                    name = name.chars().take(name_width - 3).collect();
                    name.push_str("...");
                }

                writeln!(
                    screen,
                    "{:<8} {:<8} {:>6.1} {:>10.1}   {}",
                    pid,
                    ppid,
                    process.cpu_usage(),
                    process.memory() as f64 / 1024.0 / 1024.0,
                    name
                )?;
            }

            terminal.clear()?;
            terminal.write(&screen)?;
            terminal.flush()?;

            match terminal.poll_key(Duration::from_millis(900))? {
                Some(TerminalKey::Char('q') | TerminalKey::Escape) => break,
                Some(TerminalKey::Char('c')) => sort_cpu = true,
                Some(TerminalKey::Char('m')) => sort_cpu = false,
                _ => {}
            }
        }

        Ok(CommandOutput::ok(""))
    }
}


fn system_help() -> CommandOutput {
    CommandOutput::ok(
        "sys — información y administración local de Windows\n\n\
         uso: sys SUBCOMANDO [opciones]\n\n\
         Información:\n\
           sys info                     resumen del equipo\n\
           sys processes                procesos por CPU/RAM\n\
           sys top                      monitor TUI de procesos\n\
           sys disks                    discos y uso\n\
           sys memory                   memoria y swap\n\
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
            "sys users — consulta cuentas de Windows\n\
             uso:\n\
               sys users                  cuentas locales\n\
               sys users USUARIO          detalle de una cuenta\n\
               sys users --domain         cuentas del dominio\n\
               sys users USUARIO --domain detalle de cuenta de dominio\n",
        ));
    }

    let mut native = vec!["user".to_owned()];
    if let Some(name) = args.first().filter(|arg| !arg.starts_with('-')) {
        native.push(name.clone());
    }
    if options::has(args, "--domain") {
        native.push("/domain".to_owned());
    }

    run_windows_tool("net.exe", &native)
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
\n\
             Muestra cola, servidor de impresión, recurso compartido, puerto e IP cuando\n\
             el puerto TCP/IP permite resolverla. No usa PowerShell.\n",
        ));
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

        let ip = resolve_printer_ip(&port);
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

    if options::has(args, "--pnp") {
        return run_windows_tool("pnputil.exe", &["/enum-drivers".to_owned()]);
    }

    if options::has(args, "--devices") {
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


const FETCH_LOGO_PIXELS: &[&str] = &[
    " NNNNNNNNNNNNNNNNNNNNNNNNNNNNNN ",
    "NYGGGGGGGGGGGGGGGGGGGGGGGGGGGGGN",
    "NGYGGGGGGGGGGGGGGGGGGGGGGGGGGGGN",
    "NYGGGGGGGGGGGGGGGGGGGGGGGGGGGGGN",
    "NGGGBBBBNNGGGGGGBBBNNGBBBBBBBBDN",
    "NGGBNNDDDNNGGGBBNDDDNNNNNNNDDDDN",
    "NGBNNDGGGNGGGBNNDGGGNGGGGNNDGGGN",
    "NGNNNGGGGGGGGNNNGGGGGGGGGNNDGGGN",
    "NGNNNNGGGGGGGNNNNGGGGGGGGNNDGGGN",
    "NGGNNNNGGGGGGGNNNNGGGGGGGNNDGGGN",
    "NGGNNNNNNGGGGGNNNNNNGGGGGNNDGGGN",
    "NGGGNNNNNNGGGGGNNNNNNGGGGNNDGGGN",
    "NGGGGGNNNNNGGGGGGNNNNNGGGNNDGGGN",
    "NBGGGGGGNNNGBGGGGGGNNNGGGNNDGGGN",
    "NNBGGGGGNNNGNBGGGGGNNNGGGNNDGGGN",
    "NGNBGGGNNNDGGNBGGGNNNDGGGNNDGGGN",
    "NGNNNNNNNDGGGNNNNNNNDGGGGNNDGGGN",
    "NGGGDDDDGGGGGGGDDDDGGGGGGDDDGGGN",
    "NGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGN",
    "NGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGN",
    "NGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGN",
    " NNNNNNNNNNNNNNNNNNNNNNNNNNNNNN ",
];

fn fetch_logo_color(code: u8) -> Option<(u8, u8, u8)> {
    match code {
        b'G' => Some((186, 189, 198)),
        b'N' => Some((35, 52, 106)),
        b'D' => Some((17, 30, 70)),
        b'B' => Some((6, 48, 185)),
        b'Y' => Some((251, 242, 54)),
        _ => None,
    }
}

fn render_fetch_logo(small: bool, color: bool) -> Vec<String> {
    let x_step = if small { 2 } else { 1 };
    let y_step = if small { 2 } else { 1 };
    let row_step = y_step * 2;
    let width = FETCH_LOGO_PIXELS[0].len();
    let mut lines = Vec::new();

    for top_y in (0..FETCH_LOGO_PIXELS.len()).step_by(row_step) {
        let bottom_y = (top_y + y_step).min(FETCH_LOGO_PIXELS.len() - 1);
        let top = FETCH_LOGO_PIXELS[top_y].as_bytes();
        let bottom = FETCH_LOGO_PIXELS[bottom_y].as_bytes();
        let mut line = String::new();

        for x in (0..width).step_by(x_step) {
            let top_color = fetch_logo_color(top[x]);
            let bottom_color = fetch_logo_color(bottom[x]);

            if !color {
                match (top_color, bottom_color) {
                    (None, None) => line.push(' '),
                    (Some(_), None) => line.push('▀'),
                    (None, Some(_)) => line.push('▄'),
                    (Some(_), Some(_)) => line.push('█'),
                }
                continue;
            }

            match (top_color, bottom_color) {
                (None, None) => line.push(' '),
                (Some((r, g, b)), None) => {
                    line.push_str(&format!("\x1b[38;2;{r};{g};{b}m▀\x1b[0m"));
                }
                (None, Some((r, g, b))) => {
                    line.push_str(&format!("\x1b[38;2;{r};{g};{b}m▄\x1b[0m"));
                }
                (Some((tr, tg, tb)), Some((br, bg, bb))) => {
                    line.push_str(&format!(
                        "\x1b[38;2;{tr};{tg};{tb}m\x1b[48;2;{br};{bg};{bb}m▀\x1b[0m"
                    ));
                }
            }
        }
        lines.push(line);
    }

    lines
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
            "fetch — muestra información del sistema con el logo de Shell Shock Tool\n\
             uso: fetch [--small|--full] [--plain|--stdout]\n\
             --plain/--stdout: salida sin secuencias ANSI, apta para archivos y pipes\n\
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

    let small = args.iter().any(|arg| arg == "--small");
    let plain = args.iter().any(|arg| matches!(arg.as_str(), "--plain" | "--stdout"));
    let logo = render_fetch_logo(small, !plain);
    let logo_width = if small { 16 } else { 32 };

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
            "\x1b[1;38;2;6;48;185m{user}\x1b[38;2;251;242;54m@\x1b[38;2;35;52;106m{host}\x1b[0m"
        );
        let label = "\x1b[1;38;2;251;242;54m";
        let reset = "\x1b[0m";

        vec![
            title,
            "\x1b[38;2;35;52;106m────────────────────────────────────────\x1b[0m".to_owned(),
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
            "\x1b[48;2;17;30;70m  \x1b[48;2;35;52;106m  \x1b[48;2;6;48;185m  \x1b[48;2;186;189;198m  \x1b[48;2;251;242;54m  \x1b[0m".to_owned(),
        ]
    };

    let rows = logo.len().max(info.len());
    let mut out = String::new();
    for index in 0..rows {
        if let Some(line) = logo.get(index) {
            out.push_str(line);
        } else {
            out.push_str(&" ".repeat(logo_width));
        }

        out.push_str("   ");
        if let Some(line) = info.get(index) {
            out.push_str(line);
        }
        out.push('\n');
    }

    Ok(CommandOutput::ok(out))
}

fn processes() -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_all();

    let rows = process_tree_rows(&system, true);
    let mut out = String::from("PID      PPID     CPU%     RAM MiB   PROCESS\n");

    for (pid, tree_prefix) in rows.into_iter().take(160) {
        let Some(process) = system.process(pid) else { continue; };
        let ppid = process.parent().map(|p| p.as_u32()).unwrap_or(0);
        out.push_str(&format!(
            "{:<8} {:<8} {:>6.1} {:>10.1}   {}{}\n",
            pid,
            ppid,
            process.cpu_usage(),
            process.memory() as f64 / 1024.0 / 1024.0,
            tree_prefix,
            process.name().to_string_lossy()
        ));
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
