use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use sysinfo::{Pid, System};

use crate::{
    adapters::persistence::AppPaths,
    core::CommandOutput,
};

static SECURITY_SERVICE: OnceLock<Arc<SecurityTriageService>> = OnceLock::new();

pub fn shared_security_service(paths: AppPaths) -> Arc<SecurityTriageService> {
    SECURITY_SERVICE
        .get_or_init(|| Arc::new(SecurityTriageService::new(paths)))
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttentionLevel {
    Performance,
    Attention,
    Suspicious,
    Alert,
}

impl AttentionLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Performance => "PERFORMANCE",
            Self::Attention => "ATTENTION",
            Self::Suspicious => "SUSPICIOUS",
            Self::Alert => "ALERT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreloadCase {
    Normal,
    Review,
    Alarm,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub pid: u32,
    pub name: String,
    pub level: AttentionLevel,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PreloadReport {
    pub case: PreloadCase,
    pub process_count: usize,
    pub known_count: usize,
    pub new_count: usize,
    pub pending_count: usize,
    pub findings: Vec<Finding>,
    pub performance: Vec<Finding>,
    pub history_available: bool,
}

#[derive(Debug, Clone)]
struct ProcessSnapshot {
    pid: u32,
    ppid: u32,
    start_time: u64,
    name: String,
    exe: String,
    command_line: String,
    cpu: f32,
    memory_mib: f64,
    disk_read_bytes: u64,
    disk_written_bytes: u64,
    new_to_history: bool,
}

#[derive(Debug, Clone)]
struct SecuritySource {
    id: String,
    enabled: bool,
    adapter: String,
    endpoint: String,
    auth_env: Option<String>,
    ttl_hours: u64,
    priority: u32,
}

pub struct SecurityTriageService {
    paths: AppPaths,
    last_report: Mutex<Option<PreloadReport>>,
}

impl SecurityTriageService {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            last_report: Mutex::new(None),
        }
    }

    pub fn run_startup_preload<F>(&self, mut progress: F) -> PreloadReport
    where
        F: FnMut(&str),
    {
        progress("[·] procesos              revisando\r\n");

        let mut system = System::new_all();
        system.refresh_all();
        let process_count = system.processes().len();

        progress(&format!(
            "[✓] procesos              {process_count} encontrados\r\n"
        ));
        progress("[✓] árbol PID/PPID        construido\r\n");
        progress("[✓] CPU / RAM / I/O       muestra inicial\r\n");

        let known_exes = self.known_executables().ok();
        let history_available = known_exes.is_some();
        let known_exes = known_exes.unwrap_or_default();
        let snapshots = collect_processes(&system, &known_exes);

        let known_count = snapshots.iter().filter(|p| !p.new_to_history).count();
        let new_count = snapshots.len().saturating_sub(known_count);

        if history_available {
            progress(&format!(
                "[✓] historial local       {known_count} conocidos · {new_count} nuevos\r\n"
            ));
        } else {
            progress("[x] historial local       no disponible; modo temporal\r\n");
        }

        // Estas fuentes se mantienen deliberadamente fuera del fast path del esqueleto.
        // Los adaptadores nativos se conectarán después sin cambiar el contrato de preload.
        progress("[~] conexiones            diferido\r\n");
        progress("[~] inicio automático     diferido\r\n");
        progress("[~] servicios             diferido\r\n");
        progress("[~] firmas / hashes       diferido\r\n");

        let source_count = self
            .load_sources()
            .map(|sources| sources.into_iter().filter(|source| source.enabled).count())
            .unwrap_or(0);
        if source_count > 0 {
            progress(&format!(
                "[✓] inteligencia local    {source_count} fuentes registradas\r\n"
            ));
        } else {
            progress("[~] inteligencia local    sin fuentes habilitadas\r\n");
        }

        let findings = analyze_security(&snapshots, &system);
        let performance = analyze_performance(&snapshots);

        let case = if findings.iter().any(|f| f.level == AttentionLevel::Alert) {
            PreloadCase::Alarm
        } else if findings
            .iter()
            .any(|f| matches!(f.level, AttentionLevel::Attention | AttentionLevel::Suspicious))
        {
            PreloadCase::Review
        } else {
            PreloadCase::Normal
        };

        let report = PreloadReport {
            case,
            process_count,
            known_count,
            new_count,
            pending_count: 4,
            findings,
            performance,
            history_available,
        };

        *self
            .last_report
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(report.clone());

        self.persist_snapshot_async(snapshots, report.clone());
        report
    }

    pub fn render_startup(&self, report: &PreloadReport) -> String {
        let mut out = String::from("\r\n«Hola. ¿Te gustaría destruir algún mal hoy?»\r\n\r\n");

        match report.case {
            PreloadCase::Normal => {
                out.push_str("No encontré nada particularmente llamativo.\r\n\r\n");
                out.push_str(&format!(
                    "Revisado:\r\n  procesos               {}\r\n  procesos conocidos     {}\r\n  procesos nuevos        {}\r\n  elementos pendientes   {}\r\n",
                    report.process_count,
                    report.known_count,
                    report.new_count,
                    report.pending_count,
                ));

                if !report.performance.is_empty() {
                    out.push_str("\r\nCarga relevante:\r\n");
                    for item in report.performance.iter().take(4) {
                        out.push_str(&format!("  {:<24}", format!("{} [{}]", item.name, item.pid)));
                        out.push_str(&item.reasons.join(" · "));
                        out.push_str("\r\n");
                    }
                }

                out.push_str(
                    "\r\nSugerencias:\r\n  top --tree\r\n  sys startup\r\n  sys services --impact\r\n",
                );
            }
            PreloadCase::Review => {
                out.push_str(&format!(
                    "Encontré {} proceso(s) que merecen una mirada:\r\n\r\n",
                    report.findings.len()
                ));
                append_findings(&mut out, &report.findings);
                if let Some(primary) = report.findings.first() {
                    out.push_str(&format!(
                        "\r\nSugerencias para revisar la sospecha:\r\n  sys why {0}\r\n  sys inspect {0}\r\n  sys diff {0}\r\n  intel lookup <SHA256>\r\n\r\nSugerencias para revisar el equipo completo:\r\n  triage\r\n  sys suspicious\r\n  sys startup\r\n  sys services --impact\r\n",
                        primary.pid
                    ));
                }
            }
            PreloadCase::Alarm => {
                out.push_str("ALERT — encontré señales fuertes que conviene revisar.\r\n\r\n");
                append_findings(&mut out, &report.findings);
                if let Some(primary) = report.findings.first() {
                    out.push_str(&format!(
                        "\r\nProfundiza primero:\r\n  sys why {0}\r\n  sys inspect {0}\r\n  sys inspect {0} --deep\r\n  sys persistence\r\n  intel lookup <SHA256>\r\n\r\nSi necesitas privilegios adicionales:\r\n  sudo sys inspect {0}\r\n  sudo sys suspend {0}\r\n\r\nAcción destructiva sólo bajo tu decisión:\r\n  sudo sys kill {0}\r\n  sudo sys kill {0} --tree\r\n",
                        primary.pid
                    ));
                }
            }
        }

        if report.pending_count > 0 {
            out.push_str(&format!(
                "\r\n{} elemento(s) quedaron para análisis diferido.\r\n",
                report.pending_count
            ));
        }

        out
    }

    pub fn triage(&self) -> Result<CommandOutput> {
        let report = self
            .last_report
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();

        let report = match report {
            Some(report) => report,
            None => self.run_startup_preload(|_| {}),
        };

        let mut out = String::from("SST TRIAGE\n------------------------------------------------------------\n");
        out.push_str(&format!(
            "Processes        {}\nKnown            {}\nNew              {}\nPending          {}\nFindings         {}\n\n",
            report.process_count,
            report.known_count,
            report.new_count,
            report.pending_count,
            report.findings.len(),
        ));

        if report.findings.is_empty() {
            out.push_str("No hay señales de seguridad destacables en el preload actual.\n");
        } else {
            append_findings_lf(&mut out, &report.findings);
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn why(&self, args: &[String]) -> Result<CommandOutput> {
        let pid = parse_pid("sys why", args)?;
        let system = refreshed_system();
        let Some(process) = system.process(Pid::from_u32(pid)) else {
            return Ok(CommandOutput::error(format!("sys why: PID {pid} no existe"), 1));
        };

        let known = self.known_executables().unwrap_or_default();
        let snapshots = collect_processes(&system, &known);
        let findings = analyze_security(&snapshots, &system);
        let finding = findings.into_iter().find(|f| f.pid == pid);

        let mut out = format!("Why SST noticed PID {pid}\n------------------------------------------------------------\n");
        if let Some(finding) = finding {
            for (index, reason) in finding.reasons.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", index + 1, reason));
            }
        } else {
            out.push_str("No hay una señal de seguridad fuerte asociada al proceso en este momento.\n");
            out.push_str(&format!(
                "Process: {}\n",
                process.name().to_string_lossy()
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn inspect(&self, args: &[String]) -> Result<CommandOutput> {
        let pid = parse_pid("sys inspect", args)?;
        let deep = args.iter().any(|arg| arg == "--deep");
        let system = refreshed_system();
        let Some(process) = system.process(Pid::from_u32(pid)) else {
            return Ok(CommandOutput::error(format!("sys inspect: PID {pid} no existe"), 1));
        };

        let parent = process.parent().map(|p| p.as_u32()).unwrap_or(0);
        let path = process
            .exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_owned());
        let command = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        let disk = process.disk_usage();

        let mut out = format!(
            "Process\n------------------------------------------------------------\nPID:            {pid}\nPPID:           {parent}\nName:           {}\nStarted:        {}\nCPU:            {:.1}%\nRAM:            {:.1} MiB\n\nExecutable\n------------------------------------------------------------\nPath:           {path}\n\nCommand\n------------------------------------------------------------\n{}\n\nI/O sample\n------------------------------------------------------------\nRead:           {} bytes\nWritten:        {} bytes\n",
            process.name().to_string_lossy(),
            process.start_time(),
            process.cpu_usage(),
            process.memory() as f64 / 1024.0 / 1024.0,
            if command.is_empty() { "<unavailable>" } else { &command },
            disk.read_bytes,
            disk.written_bytes,
        );

        if deep {
            out.push_str("\nDeep view\n------------------------------------------------------------\n");
            out.push_str("Lineage:\n");
            let mut children = system
                .processes()
                .iter()
                .filter_map(|(child_pid, child)| {
                    (child.parent().map(|p| p.as_u32()) == Some(pid))
                        .then(|| format!("  {} [{}]", child.name().to_string_lossy(), child_pid))
                })
                .collect::<Vec<_>>();
            children.sort();
            if children.is_empty() {
                out.push_str("  no direct children observed\n");
            } else {
                for child in children {
                    out.push_str(&child);
                    out.push('\n');
                }
            }
            out.push_str("Modules/memory/handles remain deferred in the v1 skeleton.\n");
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn diff(&self, args: &[String]) -> Result<CommandOutput> {
        let pid = parse_pid("sys diff", args)?;
        let system = refreshed_system();
        let Some(process) = system.process(Pid::from_u32(pid)) else {
            return Ok(CommandOutput::error(format!("sys diff: PID {pid} no existe"), 1));
        };

        let path = process
            .exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if path.is_empty() {
            return Ok(CommandOutput::ok(
                "No se pudo obtener una identidad de archivo para comparar.\n",
            ));
        }

        let conn = self.open_db()?;
        let mut stmt = conn.prepare(
            "SELECT parent_pid, command_line, observed_at
             FROM process_observations
             WHERE exe = ?1 COLLATE NOCASE
             ORDER BY observed_at DESC
             LIMIT 1",
        )?;
        let previous = stmt.query_row(params![path], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        });

        let current_parent = process.parent().map(|p| p.as_u32()).unwrap_or(0);
        let current_cmd = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");

        let mut out = String::from(
            "Historical profile                Current\n------------------------------------------------------------\n",
        );
        match previous {
            Ok((parent, command, observed_at)) => {
                out.push_str(&format!(
                    "parent: {:<24} parent: {}{}\n",
                    parent,
                    current_parent,
                    if parent != current_parent { "  !" } else { "" }
                ));
                out.push_str(&format!(
                    "command: {:<23} command: {}{}\n",
                    shorten(&command, 23),
                    shorten(&current_cmd, 46),
                    if command != current_cmd { "  !" } else { "" }
                ));
                out.push_str(&format!("previous observation: {observed_at}\n"));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                out.push_str("No existe una observación histórica anterior para este ejecutable.\n");
            }
            Err(error) => return Err(error.into()),
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn suspicious(&self) -> Result<CommandOutput> {
        let system = refreshed_system();
        let known = self.known_executables().unwrap_or_default();
        let snapshots = collect_processes(&system, &known);
        let findings = analyze_security(&snapshots, &system);
        if findings.is_empty() {
            return Ok(CommandOutput::ok(
                "No hay procesos que superen las reglas actuales de atención.\n",
            ));
        }

        let mut out = String::new();
        append_findings_lf(&mut out, &findings);
        Ok(CommandOutput::ok(out))
    }

    pub fn startup(&self) -> Result<CommandOutput> {
        let mut out = String::from(
            "SST startup — entradas de inicio conocidas\n------------------------------------------------------------\n",
        );

        for key in [
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        ] {
            out.push_str(&format!("\n[{key}]\n"));
            out.push_str(&capture("reg.exe", &["query", key]).unwrap_or_else(|error| format!("{error}\n")));
        }

        for folder in startup_folders() {
            out.push_str(&format!("\n[{}]\n", folder.display()));
            match fs::read_dir(&folder) {
                Ok(entries) => {
                    let mut any = false;
                    for entry in entries.flatten() {
                        any = true;
                        out.push_str(&format!("{}\n", entry.path().display()));
                    }
                    if !any {
                        out.push_str("(vacío)\n");
                    }
                }
                Err(_) => out.push_str("(no disponible)\n"),
            }
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn persistence(&self) -> Result<CommandOutput> {
        let mut out = self.startup()?.stdout;
        out.push_str(
            "\nOtros puntos a revisar con comandos existentes:\n  sys tasks --verbose\n  sys services --running\n",
        );
        Ok(CommandOutput::ok(out))
    }

    pub fn services_impact(&self) -> Result<CommandOutput> {
        let raw = capture(
            "sc.exe",
            &["queryex", "type=", "service", "state=", "all"],
        )
        .context("sys services --impact: no se pudo consultar SCM")?;

        let mut services = Vec::<(String, u32)>::new();
        let mut current_name = None::<String>;
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(name) = trimmed.strip_prefix("SERVICE_NAME:") {
                current_name = Some(name.trim().to_owned());
            } else if let Some(pid_text) = trimmed.strip_prefix("PID") {
                if let Some((_, value)) = pid_text.split_once(':') {
                    if let (Some(name), Ok(pid)) =
                        (current_name.take(), value.trim().parse::<u32>())
                    {
                        if pid != 0 {
                            services.push((name, pid));
                        }
                    }
                }
            }
        }

        let system = refreshed_system();
        let mut rows = services
            .into_iter()
            .filter_map(|(name, pid)| {
                let process = system.process(Pid::from_u32(pid))?;
                Some((
                    process.memory(),
                    process.cpu_usage(),
                    name,
                    pid,
                    process.name().to_string_lossy().into_owned(),
                ))
            })
            .collect::<Vec<_>>();

        rows.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
        });

        let mut out = String::from(
            "SERVICE IMPACT\n------------------------------------------------------------\nSERVICE                         PID      CPU%     RAM MiB   PROCESS\n",
        );
        for (memory, cpu, service, pid, process) in rows.into_iter().take(40) {
            out.push_str(&format!(
                "{:<30} {:<8} {:>6.1} {:>10.1}   {}\n",
                shorten(&service, 30),
                pid,
                cpu,
                memory as f64 / 1024.0 / 1024.0,
                process,
            ));
        }
        out.push_str("\nEsto describe consumo observado; no recomienda deshabilitar servicios.\n");
        Ok(CommandOutput::ok(out))
    }

    pub fn intel(&self, args: &[String]) -> Result<CommandOutput> {
        let sub = args.first().map(String::as_str).unwrap_or("status");
        match sub {
            "status" => self.intel_status(),
            "sources" => self.intel_sources(),
            "update" => Ok(CommandOutput::ok(
                "intel update: registro cargado; adaptadores HTTP se conectarán en la siguiente fase.\nLa actualización no bloquea el preload.\n",
            )),
            "lookup" => {
                let Some(indicator) = args.get(1) else {
                    return Ok(CommandOutput::error("intel lookup: falta indicador", 2));
                };
                self.intel_lookup(indicator)
            }
            "-h" | "--help" | "help" => Ok(CommandOutput::ok(
                "intel — reputación e inteligencia complementaria\n\nuso:\n  intel status\n  intel sources\n  intel update\n  intel lookup INDICADOR\n",
            )),
            _ => Ok(CommandOutput::error(
                format!("intel: subcomando desconocido: {sub}"),
                2,
            )),
        }
    }

    fn intel_status(&self) -> Result<CommandOutput> {
        let sources = self.load_sources()?;
        let mut out = String::from(
            "INTELLIGENCE SOURCES\n------------------------------------------------------------\n",
        );
        for source in sources {
            let auth = source
                .auth_env
                .as_deref()
                .map(|name| {
                    if env::var_os(name).is_some() {
                        "auth ready"
                    } else {
                        "auth missing"
                    }
                })
                .unwrap_or("no auth");
            out.push_str(&format!(
                "{:<20} {:<9} {:<18} ttl={}h  {}\n",
                source.id,
                if source.enabled { "ENABLED" } else { "DISABLED" },
                source.adapter,
                source.ttl_hours,
                auth,
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn intel_sources(&self) -> Result<CommandOutput> {
        let sources = self.load_sources()?;
        let mut out = format!("source registry: {}\n\n", self.paths.security_sources_file().display());
        for source in sources {
            out.push_str(&format!(
                "[{}]\n  enabled={}\n  adapter={}\n  endpoint={}\n  priority={}\n\n",
                source.id, source.enabled, source.adapter, source.endpoint, source.priority
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn intel_lookup(&self, indicator: &str) -> Result<CommandOutput> {
        if local_indicator_match(&self.paths, indicator)? {
            return Ok(CommandOutput::ok(format!(
                "LOCAL MATCH\nindicator: {indicator}\nsource: data/intel local lists\n"
            )));
        }

        let conn = self.open_db()?;
        let mut stmt = conn.prepare(
            "SELECT source_id, verdict, checked_at
             FROM intel_cache
             WHERE indicator = ?1
             ORDER BY checked_at DESC",
        )?;
        let rows = stmt.query_map(params![indicator], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        let mut found = false;
        let mut out = format!("indicator: {indicator}\n");
        for row in rows {
            let (source, verdict, checked_at) = row?;
            found = true;
            out.push_str(&format!("{source}: {verdict} (checked {checked_at})\n"));
        }
        if !found {
            out.push_str("No local/cache match. External adapter lookup is not active in the skeleton yet.\n");
        }
        Ok(CommandOutput::ok(out))
    }

    fn open_db(&self) -> Result<Connection> {
        open_security_db(&self.paths)
    }

    fn known_executables(&self) -> Result<HashSet<String>> {
        let conn = self.open_db()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT exe FROM process_observations WHERE exe <> ''",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut known = HashSet::new();
        for row in rows {
            known.insert(row?.to_ascii_lowercase());
        }
        Ok(known)
    }

    fn persist_snapshot_async(&self, snapshots: Vec<ProcessSnapshot>, report: PreloadReport) {
        let paths = self.paths.clone();
        thread::spawn(move || {
            let _ = persist_snapshot(&paths, &snapshots, &report);
        });
    }

    fn load_sources(&self) -> Result<Vec<SecuritySource>> {
        parse_sources(&self.paths.security_sources_file())
    }
}

fn refreshed_system() -> System {
    let mut system = System::new_all();
    system.refresh_all();
    system
}

fn collect_processes(system: &System, known_exes: &HashSet<String>) -> Vec<ProcessSnapshot> {
    system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let exe = process
                .exe()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            let disk = process.disk_usage();
            ProcessSnapshot {
                pid: pid.as_u32(),
                ppid: process.parent().map(|p| p.as_u32()).unwrap_or(0),
                start_time: process.start_time(),
                name: process.name().to_string_lossy().into_owned(),
                command_line: process
                    .cmd()
                    .iter()
                    .map(|part| part.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" "),
                new_to_history: !exe.is_empty()
                    && !known_exes.contains(&exe.to_ascii_lowercase()),
                exe,
                cpu: process.cpu_usage(),
                memory_mib: process.memory() as f64 / 1024.0 / 1024.0,
                disk_read_bytes: disk.read_bytes,
                disk_written_bytes: disk.written_bytes,
            }
        })
        .collect()
}

fn analyze_security(snapshots: &[ProcessSnapshot], _system: &System) -> Vec<Finding> {
    let names = snapshots
        .iter()
        .map(|process| (process.pid, process.name.to_ascii_lowercase()))
        .collect::<HashMap<_, _>>();

    let mut findings = Vec::new();
    for process in snapshots {
        let name = process.name.to_ascii_lowercase();
        let cmd = process.command_line.to_ascii_lowercase();
        let path = process.exe.to_ascii_lowercase();
        let parent = names.get(&process.ppid).map(String::as_str).unwrap_or("");

        let powershell = matches!(name.as_str(), "powershell.exe" | "pwsh.exe");
        let encoded = cmd.contains("-encodedcommand")
            || cmd.contains(" -enc ")
            || cmd.ends_with(" -enc")
            || cmd.contains("frombase64string");
        let suspicious_ps = powershell
            && (encoded
                || cmd.contains("invoke-expression")
                || cmd.contains(" iex ")
                || cmd.contains("downloadstring"));

        let script_parent = matches!(
            parent,
            "powershell.exe"
                | "pwsh.exe"
                | "cmd.exe"
                | "mshta.exe"
                | "wscript.exe"
                | "cscript.exe"
        );
        let user_writable = path.contains("\\appdata\\")
            || path.contains("\\temp\\")
            || path.contains("\\downloads\\");

        let mut reasons = Vec::new();
        if suspicious_ps {
            reasons.push("PowerShell con patrón de ejecución que merece revisión".to_owned());
        }
        if process.new_to_history && user_writable && script_parent {
            reasons.push("ejecutable nuevo en ruta de usuario lanzado por intérprete".to_owned());
        }
        if process.new_to_history && script_parent {
            reasons.push("primera observación con parent de scripting".to_owned());
        }

        if reasons.is_empty() {
            continue;
        }

        let level = if suspicious_ps && process.new_to_history && user_writable {
            AttentionLevel::Suspicious
        } else {
            AttentionLevel::Attention
        };
        findings.push(Finding {
            pid: process.pid,
            name: process.name.clone(),
            level,
            reasons,
        });
    }

    findings.sort_by(|a, b| b.level.cmp(&a.level).then_with(|| a.pid.cmp(&b.pid)));
    findings
}

fn analyze_performance(snapshots: &[ProcessSnapshot]) -> Vec<Finding> {
    let mut rows = snapshots
        .iter()
        .filter_map(|process| {
            let io = process
                .disk_read_bytes
                .saturating_add(process.disk_written_bytes);
            let mut reasons = Vec::new();
            if process.cpu >= 20.0 {
                reasons.push(format!("CPU {:.1}%", process.cpu));
            }
            if process.memory_mib >= 512.0 {
                reasons.push(format!("RAM {:.0} MiB", process.memory_mib));
            }
            if io >= 32 * 1024 * 1024 {
                reasons.push(format!("I/O sample {:.0} MiB", io as f64 / 1024.0 / 1024.0));
            }
            (!reasons.is_empty()).then(|| Finding {
                pid: process.pid,
                name: process.name.clone(),
                level: AttentionLevel::Performance,
                reasons,
            })
        })
        .collect::<Vec<_>>();

    rows.sort_by(|a, b| b.reasons.len().cmp(&a.reasons.len()).then_with(|| a.pid.cmp(&b.pid)));
    rows.truncate(6);
    rows
}

fn append_findings(out: &mut String, findings: &[Finding]) {
    for finding in findings.iter().take(6) {
        out.push_str(&format!(
            "{}  {} [{}]\r\n",
            finding.level.label(),
            finding.name,
            finding.pid
        ));
        for reason in finding.reasons.iter().take(4) {
            out.push_str(&format!("           {reason}\r\n"));
        }
        out.push_str("\r\n");
    }
}

fn append_findings_lf(out: &mut String, findings: &[Finding]) {
    for finding in findings.iter().take(20) {
        out.push_str(&format!(
            "{}  {} [{}]\n",
            finding.level.label(),
            finding.name,
            finding.pid
        ));
        for reason in finding.reasons.iter().take(4) {
            out.push_str(&format!("  - {reason}\n"));
        }
        out.push('\n');
    }
}

fn parse_pid(command: &str, args: &[String]) -> Result<u32> {
    let pid = args
        .first()
        .context(format!("{command}: falta PID"))?
        .parse::<u32>()
        .context(format!("{command}: PID inválido"))?;
    Ok(pid)
}

fn open_security_db(paths: &AppPaths) -> Result<Connection> {
    fs::create_dir_all(paths.data_dir())?;
    let conn = Connection::open(paths.security_db_file())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS schema_meta (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         INSERT OR IGNORE INTO schema_meta(key, value) VALUES ('schema_version', '1');

         CREATE TABLE IF NOT EXISTS preload_sessions (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             observed_at INTEGER NOT NULL,
             process_count INTEGER NOT NULL,
             known_count INTEGER NOT NULL,
             new_count INTEGER NOT NULL,
             case_name TEXT NOT NULL
         );

         CREATE TABLE IF NOT EXISTS process_observations (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id INTEGER NOT NULL,
             observed_at INTEGER NOT NULL,
             pid INTEGER NOT NULL,
             parent_pid INTEGER NOT NULL,
             start_time INTEGER NOT NULL,
             name TEXT NOT NULL,
             exe TEXT NOT NULL,
             command_line TEXT NOT NULL,
             cpu REAL NOT NULL,
             memory_mib REAL NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_process_observations_exe
             ON process_observations(exe);
         CREATE INDEX IF NOT EXISTS idx_process_observations_pid_start
             ON process_observations(pid, start_time);
         CREATE INDEX IF NOT EXISTS idx_process_observations_observed
             ON process_observations(observed_at);

         CREATE TABLE IF NOT EXISTS intel_cache (
             indicator TEXT NOT NULL,
             source_id TEXT NOT NULL,
             verdict TEXT NOT NULL,
             checked_at INTEGER NOT NULL,
             expires_at INTEGER,
             PRIMARY KEY(indicator, source_id)
         );",
    )?;
    Ok(conn)
}

fn persist_snapshot(paths: &AppPaths, snapshots: &[ProcessSnapshot], report: &PreloadReport) -> Result<()> {
    let mut conn = open_security_db(paths)?;
    let observed_at = unix_now();
    let case_name = match report.case {
        PreloadCase::Normal => "normal",
        PreloadCase::Review => "review",
        PreloadCase::Alarm => "alarm",
    };

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO preload_sessions(observed_at, process_count, known_count, new_count, case_name)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            observed_at,
            report.process_count as i64,
            report.known_count as i64,
            report.new_count as i64,
            case_name
        ],
    )?;
    let session_id = tx.last_insert_rowid();

    {
        let mut stmt = tx.prepare(
            "INSERT INTO process_observations(
                 session_id, observed_at, pid, parent_pid, start_time,
                 name, exe, command_line, cpu, memory_mib
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;

        for process in snapshots {
            stmt.execute(params![
                session_id,
                observed_at,
                process.pid,
                process.ppid,
                process.start_time,
                process.name,
                process.exe,
                process.command_line,
                process.cpu,
                process.memory_mib,
            ])?;
        }
    }

    tx.commit()?;
    Ok(())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn parse_sources(path: &Path) -> Result<Vec<SecuritySource>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    let mut result = Vec::new();
    let mut current: Option<SecuritySource> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("[source ") && line.ends_with(']') {
            if let Some(source) = current.take() {
                result.push(source);
            }
            let id = line
                .trim_start_matches("[source ")
                .trim_end_matches(']')
                .trim()
                .to_owned();
            current = Some(SecuritySource {
                id,
                enabled: true,
                adapter: String::new(),
                endpoint: String::new(),
                auth_env: None,
                ttl_hours: 24,
                priority: 100,
            });
            continue;
        }

        let Some(source) = current.as_mut() else { continue; };
        let Some((key, value)) = line.split_once('=') else { continue; };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');

        match key {
            "enabled" => source.enabled = value.eq_ignore_ascii_case("true"),
            "adapter" => source.adapter = value.to_owned(),
            "endpoint" => source.endpoint = value.to_owned(),
            "auth_env" if !value.is_empty() => source.auth_env = Some(value.to_owned()),
            "ttl_hours" => source.ttl_hours = value.parse().unwrap_or(24),
            "priority" => source.priority = value.parse().unwrap_or(100),
            _ => {}
        }
    }

    if let Some(source) = current {
        result.push(source);
    }
    result.sort_by_key(|source| source.priority);
    Ok(result)
}

fn capture(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok(text)
}

fn startup_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(appdata) = env::var_os("APPDATA") {
        folders.push(
            PathBuf::from(appdata)
                .join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
        );
    }
    if let Some(programdata) = env::var_os("PROGRAMDATA") {
        folders.push(
            PathBuf::from(programdata)
                .join(r"Microsoft\Windows\Start Menu\Programs\StartUp"),
        );
    }
    folders
}

fn local_indicator_match(paths: &AppPaths, indicator: &str) -> Result<bool> {
    let normalized = indicator.trim().to_ascii_lowercase();
    for path in [
        paths.intel_dir().join("hashes.txt"),
        paths.intel_dir().join("domains.txt"),
        paths.intel_dir().join("ips.txt"),
    ] {
        let Ok(text) = fs::read_to_string(path) else { continue; };
        if text.lines().any(|line| {
            let value = line.split('#').next().unwrap_or("").trim().to_ascii_lowercase();
            !value.is_empty() && value == normalized
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn shorten(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width <= 3 {
        return "...".chars().take(width).collect();
    }
    let mut out = value.chars().take(width - 3).collect::<String>();
    out.push_str("...");
    out
}
