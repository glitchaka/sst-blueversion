use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use sysinfo::{Pid, System};

use crate::{adapters::persistence::AppPaths, core::CommandOutput};

use crate::core::triage::{
    Assessment, Evidence, Family, ObservationState, ParentProfile, Strength, correlate,
};

static SECURITY_SERVICE: OnceLock<Arc<SecurityTriageService>> = OnceLock::new();

pub fn shared_security_service(paths: AppPaths) -> Arc<SecurityTriageService> {
    SECURITY_SERVICE
        .get_or_init(|| Arc::new(SecurityTriageService::new(paths)))
        .clone()
}

pub use crate::core::triage::Classification as AttentionLevel;

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
    pub limitations: Vec<String>,
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

        let known_count = snapshots
            .iter()
            .filter(|p| !p.exe.is_empty() && !p.new_to_history)
            .count();
        let new_count = snapshots.iter().filter(|p| p.new_to_history).count();

        if history_available {
            progress(&format!(
                "[✓] historial local       {known_count} rutas vistas · {new_count} rutas nuevas\r\n"
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

        let findings = self.analyze_security(&snapshots);
        let performance = analyze_performance(&snapshots);

        let case = if findings.iter().any(|f| f.level == AttentionLevel::Alert) {
            PreloadCase::Alarm
        } else if findings.iter().any(|f| {
            matches!(
                f.level,
                AttentionLevel::Attention | AttentionLevel::Suspicious
            )
        }) {
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
                    "Revisado:\r\n  procesos               {}\r\n  rutas vistas antes     {}\r\n  procesos nuevos        {}\r\n  elementos pendientes   {}\r\n",
                    report.process_count,
                    report.known_count,
                    report.new_count,
                    report.pending_count,
                ));

                if !report.performance.is_empty() {
                    out.push_str("\r\nCarga relevante:\r\n");
                    for item in report.performance.iter().take(4) {
                        out.push_str(&format!(
                            "  {:<24}",
                            format!("{} [{}]", item.name, item.pid)
                        ));
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
                "\r\n{} colector(es) quedaron para análisis diferido.\r\n",
                report.pending_count
            ));
        }

        out
    }

    pub fn triage(&self, args: &[String]) -> Result<CommandOutput> {
        if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help" | "help")) {
            return Ok(CommandOutput::ok(
                "triage — análisis activo del equipo, no sólo el estado del preload\n\n\
                 uso:\n\
                   triage                  vuelve a revisar procesos y aprendizaje local\n\
                   triage PID              investiga un proceso: why + inspect + diff\n\
                   triage --deep           añade conexiones, inicio automático y servicios\n\
                   triage PID --deep       investigación profunda del PID y sus conexiones\n\n\
                 SST aprende relaciones padre/hijo recurrentes del mismo equipo, pero una\n\
                 relación aprendida sólo reduce la novedad de linaje; no convierte un proceso\n\
                 en confiable frente a otras señales actuales.\n",
            ));
        }

        let deep = args.iter().any(|arg| arg == "--deep");

        if let Some(raw_pid) = args.iter().find(|arg| !arg.starts_with('-')) {
            let pid = raw_pid
                .parse::<u32>()
                .with_context(|| format!("triage: PID inválido: {raw_pid}"))?;
            let mut out = format!(
                "SST TRIAGE — PROCESO {pid}\n============================================================\n\n"
            );

            let one = vec![pid.to_string()];
            let why = self.why(&one)?;
            out.push_str(&why.stdout);
            if !why.stdout.ends_with('\n') {
                out.push('\n');
            }

            out.push_str("\nINSPECCIÓN\n------------------------------------------------------------\n");
            let inspect_args = if deep {
                vec![pid.to_string(), "--deep".to_owned()]
            } else {
                vec![pid.to_string()]
            };
            let inspect = self.inspect(&inspect_args)?;
            out.push_str(&inspect.stdout);

            out.push_str("\nCAMBIOS HISTÓRICOS\n------------------------------------------------------------\n");
            let diff = self.diff(&one)?;
            out.push_str(&diff.stdout);

            if deep {
                let pids = HashSet::from([pid]);
                out.push_str("\nCONEXIONES DEL PID\n------------------------------------------------------------\n");
                out.push_str(&triage_connections(&pids));
            }

            return Ok(CommandOutput::ok(out));
        }

        // Triage manual siempre toma una muestra nueva. El preload sigue siendo rápido,
        // pero este comando no recicla last_report.
        let system = refreshed_system();
        let known = self.known_executables().unwrap_or_default();
        let snapshots = collect_processes(&system, &known);
        let findings = self.analyze_security(&snapshots);
        let performance = analyze_performance(&snapshots);
        let instances = snapshots
            .iter()
            .map(|p| (p.pid, p.start_time))
            .collect::<Vec<_>>();
        let learned = self.learned_lineages().unwrap_or_default();

        let known_count = snapshots
            .iter()
            .filter(|p| !p.exe.is_empty() && !p.new_to_history)
            .count();
        let new_count = snapshots.iter().filter(|p| p.new_to_history).count();

        let mut active_learned = 0usize;
        for process in &snapshots {
            let Some(parent) = snapshots
                .iter()
                .find(|p| p.pid == process.ppid && p.start_time <= process.start_time && !p.exe.is_empty())
            else {
                continue;
            };
            let key = (
                normalize_install_identity(&process.exe),
                normalize_install_identity(&parent.exe),
            );
            if learned.get(&key).copied().unwrap_or(0) >= local_learning_threshold(process)
                && auto_learnable_install_path(&process.exe)
                && auto_learnable_install_path(&parent.exe)
            {
                active_learned += 1;
            }
        }

        let mut out = String::from(
            "SST TRIAGE — ANÁLISIS ACTIVO\n============================================================\n",
        );
        out.push_str(&format!(
            "Procesos actuales                 {}\n\
             Rutas vistas anteriormente         {}\n\
             Rutas nuevas                       {}\n\
             Hallazgos que requieren revisión   {}\n\
             Relaciones locales aprendidas      {}\n\
             Carga relevante                    {}\n\n",
            snapshots.len(),
            known_count,
            new_count,
            findings.len(),
            active_learned,
            performance.len(),
        ));

        out.push_str("SEGURIDAD\n------------------------------------------------------------\n");
        if findings.is_empty() {
            out.push_str(
                "No hay procesos que superen las reglas actuales de atención.\n\
                 Las relaciones recurrentes sólo dejan de contarse como novedad de linaje;\n\
                 otras señales siguen pudiendo elevarlas nuevamente.\n",
            );
        } else {
            for finding in findings.iter().take(20) {
                out.push_str(&format!(
                    "\n{}  {} [{}]\n",
                    finding.level.label(),
                    finding.name,
                    finding.pid
                ));
                if let Some(process) = snapshots.iter().find(|p| p.pid == finding.pid) {
                    out.push_str(&format!("  ruta:   {}\n", display_path(&process.exe)));
                    if let Some(parent) = snapshots
                        .iter()
                        .find(|p| p.pid == process.ppid && p.start_time <= process.start_time)
                    {
                        out.push_str(&format!(
                            "  padre:  {} [{}]\n  p.ruta: {}\n",
                            parent.name,
                            parent.pid,
                            display_path(&parent.exe),
                        ));
                    }
                }
                for reason in &finding.reasons {
                    out.push_str(&format!("  razón:  {reason}\n"));
                }
                for limitation in &finding.limitations {
                    out.push_str(&format!("  falta:  {limitation}\n"));
                }
                out.push_str(&format!(
                    "  revisar: triage {} --deep | sys why {} | sys inspect {}\n",
                    finding.pid, finding.pid, finding.pid
                ));
            }
        }

        if !performance.is_empty() {
            out.push_str("\nRECURSOS\n------------------------------------------------------------\n");
            for item in performance.iter().take(6) {
                out.push_str(&format!(
                    "{} [{}]  {}\n",
                    item.name,
                    item.pid,
                    item.reasons.join(" · ")
                ));
            }
        }

        if deep {
            let pids = findings.iter().map(|f| f.pid).collect::<HashSet<_>>();
            out.push_str("\nCONEXIONES DE LOS HALLAZGOS\n------------------------------------------------------------\n");
            out.push_str(&triage_connections(&pids));

            out.push_str("\nINICIO AUTOMÁTICO\n============================================================\n");
            match self.startup() {
                Ok(section) => out.push_str(&section.stdout),
                Err(error) => out.push_str(&format!("No disponible: {error}\n")),
            }

            out.push_str("\nIMPACTO DE SERVICIOS\n============================================================\n");
            match self.services_impact() {
                Ok(section) => out.push_str(&section.stdout),
                Err(error) => out.push_str(&format!("No disponible: {error}\n")),
            }
        } else {
            out.push_str(
                "\nUsa 'triage --deep' para correlacionar además conexiones, inicio automático y servicios.\n",
            );
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn why(&self, args: &[String]) -> Result<CommandOutput> {
        let pid = parse_pid("sys why", args)?;
        let system = refreshed_system();
        let Some(process) = system.process(Pid::from_u32(pid)) else {
            return Ok(CommandOutput::error(
                format!("sys why: PID {pid} no existe"),
                1,
            ));
        };

        let known = self.known_executables().unwrap_or_default();
        let snapshots = collect_processes(&system, &known);
        let instances = snapshots
            .iter()
            .map(|p| (p.pid, p.start_time))
            .collect::<Vec<_>>();
        let history = self.historical_parents(&instances);
        let snapshot = snapshots
            .iter()
            .find(|p| p.pid == pid)
            .expect("process from same snapshot");
        let trusted = self.trusted_lineages();
        let learned = self.learned_lineages();
        let result = assess_process_with_trust(
            snapshot,
            &snapshots,
            history.as_ref().ok(),
            trusted.as_ref().ok(),
            learned.as_ref().ok(),
        );
        let mut out = format!(
            "{} {} [{pid}]\n\nWhy:\n",
            result.classification.label(),
            process.name().to_string_lossy()
        );
        if result.reasons.is_empty() {
            out.push_str(
                "  No relevant signals with available information; NORMAL does not mean safe.\n",
            );
        }
        for reason in &result.reasons {
            out.push_str(&format!("  {reason}\n"));
        }
        if !result.limitations.is_empty() {
            out.push_str("\nIncomplete:\n");
            for limitation in &result.limitations {
                out.push_str(&format!("  {limitation}\n"));
            }
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn inspect(&self, args: &[String]) -> Result<CommandOutput> {
        let pid = parse_pid("sys inspect", args)?;
        let deep = args.iter().any(|arg| arg == "--deep");
        let system = refreshed_system();
        let Some(process) = system.process(Pid::from_u32(pid)) else {
            return Ok(CommandOutput::error(
                format!("sys inspect: PID {pid} no existe"),
                1,
            ));
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
            if command.is_empty() {
                "<unavailable>"
            } else {
                &command
            },
            disk.read_bytes,
            disk.written_bytes,
        );

        if deep {
            out.push_str(
                "\nDeep view\n------------------------------------------------------------\n",
            );
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
            return Ok(CommandOutput::error(
                format!("sys diff: PID {pid} no existe"),
                1,
            ));
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

        let profiles = self.historical_parents(&[(pid, process.start_time())])?;
        let mut out = String::from(
            "Historical differences\n------------------------------------------------------------\n",
        );
        match profiles.get(&path.to_ascii_lowercase()) {
            Some(parents) => {
                let parent = process
                    .parent()
                    .and_then(|pid| system.process(pid))
                    .filter(|p| p.start_time() <= process.start_time())
                    .and_then(|p| p.exe());
                if let Some(parent) = parent {
                    let child = path.to_ascii_lowercase();
                    let current = parent.display().to_string().to_ascii_lowercase();
                    let trusted = self
                        .trusted_lineages()
                        .map(|rows| rows.contains(&(child, current.clone())))
                        .unwrap_or(false);
                    if trusted {
                        out.push_str("Parent relation is trusted by local operator policy.\n");
                    } else {
                        let evidence = parents.evidence(&current);
                        if evidence.state != ObservationState::Known {
                            out.push_str(&format!("Incomplete: {}\n", evidence.reason));
                        } else if let Some(reason) = parents.anomaly(&current) {
                            let mut expected = parents
                                .executions
                                .iter()
                                .map(|(parent, count)| format!("{parent} ({count} executions)"))
                                .collect::<Vec<_>>();
                            expected.sort();
                            out.push_str(&format!(
                                "parent executable: {} -> {}\n",
                                expected.join(", "),
                                reason
                            ));
                        } else {
                            out.push_str("No parent identity difference.\n");
                        }
                    }
                } else {
                    out.push_str("Parent identity UNKNOWN; comparison unavailable.\n");
                }
            }
            None => out.push_str("No historical parent profile available for this executable.\n"),
        }

        let conn = self.open_db()?;
        let previous_command = conn.query_row(
            "SELECT command_line FROM process_observations WHERE exe = ?1 COLLATE NOCASE
             AND command_line <> '' AND NOT (pid = ?2 AND start_time = ?3) ORDER BY observed_at DESC, id DESC LIMIT 1",
            params![path, pid, process.start_time()],
            |row| row.get::<_, String>(0),
        );
        let current_command = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        match previous_command {
            Ok(previous) if !current_command.is_empty() && previous != current_command => {
                out.push_str(&format!("command: {} -> {}\n", previous, current_command));
            }
            Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(error) => return Err(error.into()),
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn suspicious(&self) -> Result<CommandOutput> {
        let system = refreshed_system();
        let known = self.known_executables().unwrap_or_default();
        let snapshots = collect_processes(&system, &known);
        let findings = self.analyze_security(&snapshots);
        if findings.is_empty() {
            return Ok(CommandOutput::ok(
                "No hay procesos que superen las reglas actuales de atención.\n",
            ));
        }

        let mut out = String::new();
        append_findings_lf(&mut out, &findings);
        Ok(CommandOutput::ok(out))
    }

    pub fn safe(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str) {
            None | Some("list") => self.safe_list(),
            Some("remove") => self.safe_remove(&args[1..]),
            _ => self.safe_add(args),
        }
    }

    fn safe_add(&self, args: &[String]) -> Result<CommandOutput> {
        let pids = args
            .iter()
            .take_while(|arg| !arg.starts_with('-'))
            .map(|arg| {
                arg.parse::<u32>()
                    .with_context(|| format!("sys safe: PID inválido: {arg}"))
            })
            .collect::<Result<Vec<_>>>()?;

        if pids.is_empty() {
            return Ok(CommandOutput::error(
                "sys safe: uso: sys safe PID [PID ...] | sys safe list | sys safe remove ID [ID ...]",
                2,
            ));
        }

        let system = refreshed_system();
        let conn = self.open_db()?;
        let now = unix_now();
        let mut out = String::new();

        for pid in pids {
            let Some(process) = system.process(Pid::from_u32(pid)) else {
                out.push_str(&format!("PID {pid}: no existe\n"));
                continue;
            };
            let Some(child_exe) = process.exe() else {
                out.push_str(&format!("PID {pid}: ruta del ejecutable no disponible\n"));
                continue;
            };
            let Some(parent_pid) = process.parent() else {
                out.push_str(&format!("PID {pid}: proceso padre no disponible\n"));
                continue;
            };
            let Some(parent) = system.process(parent_pid) else {
                out.push_str(&format!("PID {pid}: proceso padre {} no disponible\n", parent_pid.as_u32()));
                continue;
            };
            let Some(parent_exe) = parent.exe() else {
                out.push_str(&format!("PID {pid}: ruta del ejecutable padre no disponible\n"));
                continue;
            };

            let child_text = child_exe.display().to_string();
            let parent_text = parent_exe.display().to_string();
            conn.execute(
                "INSERT INTO trusted_lineage(child_exe, parent_exe, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(child_exe, parent_exe) DO NOTHING",
                params![child_text, parent_text, now],
            )?;
            let id: i64 = conn.query_row(
                "SELECT id FROM trusted_lineage
                 WHERE child_exe = ?1 COLLATE NOCASE AND parent_exe = ?2 COLLATE NOCASE",
                params![child_text, parent_text],
                |row| row.get(0),
            )?;

            out.push_str(&format!(
                "#{id} trusted lineage: {} [{}] <- {} [{}]\n",
                process.name().to_string_lossy(),
                pid,
                parent.name().to_string_lossy(),
                parent_pid.as_u32(),
            ));
        }

        *self
            .last_report
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;

        if out.is_empty() {
            out.push_str("No se registró ninguna relación de confianza.\n");
        } else {
            out.push_str(
                "\nLa confianza solo neutraliza la anomalía de padre histórico para esa pareja exacta; otras evidencias siguen activas.\n",
            );
        }
        Ok(CommandOutput::ok(out))
    }

    fn safe_list(&self) -> Result<CommandOutput> {
        let conn = self.open_db()?;
        let mut stmt = conn.prepare(
            "SELECT id, child_exe, parent_exe, created_at
             FROM trusted_lineage ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        let mut out = String::from(
            "ID   CHILD <- PARENT\n------------------------------------------------------------\n",
        );
        let mut any = false;
        for row in rows {
            let (id, child, parent, _) = row?;
            any = true;
            out.push_str(&format!("#{id}  {child}\n     <- {parent}\n"));
        }
        if !any {
            out.push_str("(sin relaciones de confianza)\n");
        }
        Ok(CommandOutput::ok(out))
    }

    fn safe_remove(&self, args: &[String]) -> Result<CommandOutput> {
        if args.is_empty() {
            return Ok(CommandOutput::error(
                "sys safe remove: falta ID; usa 'sys safe list'",
                2,
            ));
        }

        let conn = self.open_db()?;
        let mut out = String::new();
        for raw in args {
            let id = raw
                .trim_start_matches('#')
                .parse::<i64>()
                .with_context(|| format!("sys safe remove: ID inválido: {raw}"))?;
            let changed = conn.execute(
                "DELETE FROM trusted_lineage WHERE id = ?1",
                params![id],
            )?;
            if changed == 0 {
                out.push_str(&format!("#{id}: no existe\n"));
            } else {
                out.push_str(&format!("#{id}: eliminado\n"));
            }
        }

        *self
            .last_report
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;

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
            out.push_str(
                &capture("reg.exe", &["query", key]).unwrap_or_else(|error| format!("{error}\n")),
            );
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
        let raw = capture("sc.exe", &["queryex", "type=", "service", "state=", "all"])
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
            "update" => self.intel_update(),
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
            let auth = match source_auth(&source, &self.paths) {
                Ok(Some(_)) => "auth ready",
                Ok(None) => "no auth",
                Err(_) => "auth missing",
            };
            out.push_str(&format!(
                "{:<20} {:<9} {:<18} ttl={}h  {}\n",
                source.id,
                if source.enabled {
                    "ENABLED"
                } else {
                    "DISABLED"
                },
                source.adapter,
                source.ttl_hours,
                auth,
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn intel_sources(&self) -> Result<CommandOutput> {
        let sources = self.load_sources()?;
        let mut out = format!(
            "source registry: {}\n\n",
            self.paths.security_sources_file().display()
        );
        for source in sources {
            out.push_str(&format!(
                "[{}]\n  enabled={}\n  adapter={}\n  endpoint={}\n  priority={}\n\n",
                source.id, source.enabled, source.adapter, source.endpoint, source.priority
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn intel_update(&self) -> Result<CommandOutput> {
        let sources = self.load_sources()?;
        let conn = self.open_db()?;
        let now = unix_now();
        let expired = conn.execute(
            "DELETE FROM intel_cache WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            params![now],
        )?;
        let mut out = format!("intel cache: {expired} entrada(s) expirada(s) eliminada(s)\n");
        for source in sources.into_iter().filter(|source| source.enabled) {
            let auth = match source_auth(&source, &self.paths) {
                Ok(Some(_)) => "ready",
                Ok(None) => "no-auth",
                Err(_) => "missing-auth",
            };
            let capability = match source.adapter.as_str() {
                "abusech_hash" | "abusech_ioc" | "abusech_url" => "live-lookup",
                "behavior_catalog" => "catalog",
                _ => "unsupported",
            };
            out.push_str(&format!(
                "{}: {} · {} · ttl={}h\n",
                source.id, capability, auth, source.ttl_hours
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn intel_lookup(&self, indicator: &str) -> Result<CommandOutput> {
        let indicator = indicator.trim();
        if indicator.is_empty() {
            return Ok(CommandOutput::error("intel lookup: indicador vacío", 2));
        }
        if local_indicator_match(&self.paths, indicator)? {
            return Ok(CommandOutput::ok(format!(
                "LOCAL MATCH\nindicator: {indicator}\nsource: data/intel local lists\n"
            )));
        }

        let sources = self.load_sources()?;
        let conn = self.open_db()?;
        let now = unix_now();
        let mut out = format!("indicator: {indicator}\n");
        let mut reported = false;

        for source in sources.into_iter().filter(|source| source.enabled) {
            if source.adapter == "behavior_catalog" {
                continue;
            }
            if let Some((verdict, checked_at)) = cached_intel(&conn, indicator, &source.id, now)? {
                out.push_str(&format!("{}: {} (cache, checked {})\n", source.id, verdict, checked_at));
                reported = true;
                continue;
            }

            match query_intel_source(&source, indicator, &self.paths) {
                Ok(Some(verdict)) => {
                    store_intel_cache(&conn, indicator, &source, &verdict, now)?;
                    out.push_str(&format!("{}: {} (live)\n", source.id, verdict));
                    reported = true;
                }
                Ok(None) => {
                    let verdict = "not-found".to_owned();
                    store_intel_cache(&conn, indicator, &source, &verdict, now)?;
                    out.push_str(&format!("{}: not-found (live)\n", source.id));
                    reported = true;
                }
                Err(error) => {
                    out.push_str(&format!("{}: unavailable ({})\n", source.id, error));
                    reported = true;
                }
            }
        }

        if !reported {
            out.push_str("No hay fuentes de inteligencia externas habilitadas.\n");
        }
        Ok(CommandOutput::ok(out))
    }

    fn analyze_security(&self, snapshots: &[ProcessSnapshot]) -> Vec<Finding> {
        let instances = snapshots
            .iter()
            .map(|p| (p.pid, p.start_time))
            .collect::<Vec<_>>();
        let history = self.historical_parents(&instances);
        let trusted = self.trusted_lineages();
        let learned = self.learned_lineages();
        let findings = analyze_security_with_trust(
            snapshots,
            history.as_ref().ok(),
            trusted.as_ref().ok(),
            learned.as_ref().ok(),
        );
        // Best effort: unavailable history never prevents local analysis.
        let paths = self.paths.clone();
        let observed = snapshots.to_vec();
        let classified = findings.clone();
        thread::spawn(move || {
            let _ = SecurityTriageService::new(paths).record_assessments(&observed, &classified);
        });
        findings
    }

    fn historical_parents(&self, current: &[(u32, u64)]) -> Result<HashMap<String, ParentProfile>> {
        let conn = self.open_db()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT lower(child.exe), lower(parent.exe), child.pid, child.start_time
             FROM process_observations child JOIN process_observations parent
             ON child.session_id = parent.session_id AND child.parent_pid = parent.pid
             AND parent.start_time <= child.start_time
             WHERE child.exe <> '' AND parent.exe <> '' AND child.observed_at >= ?1",
        )?;
        let mut profiles: HashMap<String, ParentProfile> = HashMap::new();
        for row in stmt.query_map(params![unix_now() - 90 * 24 * 60 * 60], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, u64>(3)?,
            ))
        })? {
            let (exe, parent, pid, start_time) = row?;
            if current.contains(&(pid, start_time)) {
                continue;
            }
            profiles.entry(exe).or_default().observe(parent);
        }
        Ok(profiles)
    }

    fn learned_lineages(&self) -> Result<HashMap<(String, String), u64>> {
        let conn = self.open_db()?;
        let mut stmt = conn.prepare(
            "SELECT child.exe, parent.exe, child.session_id
             FROM process_observations child JOIN process_observations parent
             ON child.session_id = parent.session_id AND child.parent_pid = parent.pid
             AND parent.start_time <= child.start_time
             WHERE child.exe <> '' AND parent.exe <> '' AND child.observed_at >= ?1
             GROUP BY child.exe, parent.exe, child.session_id",
        )?;

        let mut sessions: HashMap<(String, String), HashSet<i64>> = HashMap::new();
        for row in stmt.query_map(params![unix_now() - 90 * 24 * 60 * 60], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })? {
            let (child, parent, session_id) = row?;
            let key = (
                normalize_install_identity(&child),
                normalize_install_identity(&parent),
            );
            sessions.entry(key).or_default().insert(session_id);
        }

        Ok(sessions
            .into_iter()
            .map(|(key, sessions)| (key, sessions.len() as u64))
            .collect())
    }

    fn record_assessments(
        &self,
        snapshots: &[ProcessSnapshot],
        findings: &[Finding],
    ) -> Result<()> {
        let mut conn = self.open_db()?;
        let tx = conn.transaction()?;
        for process in snapshots {
            let finding = findings.iter().find(|f| f.pid == process.pid);
            let resources = analyze_performance(std::slice::from_ref(process));
            let finding = finding.or_else(|| resources.first());
            let level = finding.map(|f| f.level.label()).unwrap_or("NORMAL");
            let reasons =
                serde_json::to_string(&finding.map(|f| f.reasons.clone()).unwrap_or_default())?;
            tx.execute(
                "INSERT INTO correlation_history(pid, start_time, exe, observed_at, classification, reasons)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6 WHERE NOT EXISTS (
                    SELECT 1 FROM correlation_history WHERE id = (
                        SELECT max(id) FROM correlation_history WHERE pid = ?1 AND start_time = ?2 AND exe = ?3
                    ) AND classification = ?5 AND reasons = ?6
                 )",
                params![process.pid, process.start_time, process.exe, unix_now(), level, reasons],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn open_db(&self) -> Result<Connection> {
        open_security_db(&self.paths)
    }

    fn trusted_lineages(&self) -> Result<HashSet<(String, String)>> {
        let conn = self.open_db()?;
        let mut stmt = conn.prepare(
            "SELECT lower(child_exe), lower(parent_exe) FROM trusted_lineage",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut trusted = HashSet::new();
        for row in rows {
            trusted.insert(row?);
        }
        Ok(trusted)
    }

    fn known_executables(&self) -> Result<HashSet<String>> {
        let conn = self.open_db()?;
        let mut stmt =
            conn.prepare("SELECT DISTINCT exe FROM process_observations WHERE exe <> ''")?;
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
                new_to_history: !exe.is_empty() && !known_exes.contains(&exe.to_ascii_lowercase()),
                exe,
                cpu: process.cpu_usage(),
                memory_mib: process.memory() as f64 / 1024.0 / 1024.0,
                disk_read_bytes: disk.read_bytes,
                disk_written_bytes: disk.written_bytes,
            }
        })
        .collect()
}

fn assess_process_with_trust(
    process: &ProcessSnapshot,
    snapshots: &[ProcessSnapshot],
    history: Option<&HashMap<String, ParentProfile>>,
    trusted_lineages: Option<&HashSet<(String, String)>>,
    learned_lineages: Option<&HashMap<(String, String), u64>>,
) -> Assessment {
    let names = snapshots
        .iter()
        .map(|p| (p.pid, p.name.to_ascii_lowercase()))
        .collect::<HashMap<_, _>>();
    let name = process.name.to_ascii_lowercase();
    let cmd = process.command_line.to_ascii_lowercase();
    let path = process.exe.to_ascii_lowercase();
    let parent = snapshots
        .iter()
        .find(|p| p.pid == process.ppid && p.start_time <= process.start_time)
        .and_then(|p| names.get(&p.pid))
        .map(String::as_str)
        .unwrap_or("");

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
        "powershell.exe" | "pwsh.exe" | "cmd.exe" | "mshta.exe" | "wscript.exe" | "cscript.exe"
    );
    let user_writable =
        path.contains("\\appdata\\") || path.contains("\\temp\\") || path.contains("\\downloads\\");

    let mut evidence = Vec::new();
    if history.is_none() {
        evidence.push(Evidence {
            family: Family::History,
            state: ObservationState::Unavailable,
            strength: Strength::Context,
            reason: "Historical database".into(),
        });
    }
    if suspicious_ps {
        evidence.push(Evidence::known(
            Family::Execution,
            Strength::Anomaly,
            "PowerShell: execution pattern requires review",
        ));
    }
    if user_writable {
        evidence.push(Evidence::known(
            Family::Identity,
            Strength::Weak,
            "executable in user-writable path",
        ));
    }
    if script_parent {
        evidence.push(Evidence::known(
            Family::Lineage,
            Strength::Weak,
            "launched by command interpreter",
        ));
    }
    if let Some(current_parent) = snapshots
        .iter()
        .find(|p| p.pid == process.ppid && p.start_time <= process.start_time && !p.exe.is_empty())
    {
        let parent_path = current_parent.exe.to_ascii_lowercase();
        let trusted = trusted_lineages
            .is_some_and(|rows| rows.contains(&(path.clone(), parent_path.clone())));
        let learned_key = (
            normalize_install_identity(&process.exe),
            normalize_install_identity(&current_parent.exe),
        );
        let learned_sessions = learned_lineages
            .and_then(|rows| rows.get(&learned_key))
            .copied()
            .unwrap_or(0);
        let locally_learned = learned_sessions >= local_learning_threshold(process)
            && auto_learnable_install_path(&process.exe)
            && auto_learnable_install_path(&current_parent.exe);

        if trusted {
            evidence.push(Evidence::known(
                Family::Lineage,
                Strength::Context,
                format!("trusted parent relation: {}", current_parent.exe),
            ));
        } else if locally_learned {
            evidence.push(Evidence::known(
                Family::Lineage,
                Strength::Context,
                format!(
                    "learned local parent relation: {} (seen in {learned_sessions} previous session(s))",
                    current_parent.exe
                ),
            ));
        } else {
            let profile = history
                .and_then(|h| h.get(&path))
                .cloned()
                .unwrap_or_default();
            evidence.push(profile.evidence(&parent_path));
        }
    } else {
        evidence.push(Evidence {
            family: Family::Lineage,
            state: ObservationState::Unknown,
            strength: Strength::Context,
            reason: "Parent identity".into(),
        });
    }
    for (family, label) in [
        (Family::Network, "Network"),
        (Family::Identity, "Authenticode"),
        (Family::Identity, "SHA-256"),
        (Family::Persistence, "Persistence"),
        (Family::ExternalIntel, "ExternalIntel"),
    ] {
        evidence.push(Evidence {
            family,
            state: ObservationState::Unavailable,
            strength: Strength::Context,
            reason: format!("{label} (collector not connected)"),
        });
    }
    if process.exe.is_empty() {
        evidence.push(Evidence {
            family: Family::Identity,
            state: ObservationState::Unknown,
            strength: Strength::Context,
            reason: "Executable path".into(),
        });
    }
    if process.command_line.is_empty() {
        evidence.push(Evidence {
            family: Family::Execution,
            state: ObservationState::Unknown,
            strength: Strength::Context,
            reason: "Command line".into(),
        });
    }
    for finding in analyze_performance(std::slice::from_ref(process)) {
        for reason in finding.reasons {
            evidence.push(Evidence::known(
                Family::ResourceUsage,
                Strength::Anomaly,
                reason,
            ));
        }
    }
    correlate(&evidence)
}

fn normalize_install_identity(path: &str) -> String {
    let lower = path.replace('/', "\\").to_ascii_lowercase();

    // Microsoft Store packages carry a version/architecture segment in their
    // directory name. Learn the package family + executable, not one version.
    if let Some(pos) = lower.find("\\windowsapps\\") {
        let prefix_end = pos + "\\windowsapps\\".len();
        let prefix = &lower[..prefix_end];
        let rest = &lower[prefix_end..];
        if let Some((package_dir, tail)) = rest.split_once('\\') {
            let package = package_dir.split('_').next().unwrap_or(package_dir);
            return format!("{prefix}{package}\\{tail}");
        }
    }

    // Edge WebView2 also puts the runtime version in the executable path.
    if lower.ends_with("\\msedgewebview2.exe") {
        let marker = "\\microsoft\\edgewebview\\application\\";
        if let Some(pos) = lower.find(marker) {
            let prefix_end = pos + marker.len();
            let rest = &lower[prefix_end..];
            if let Some((_version, tail)) = rest.split_once('\\') {
                return format!("{}<version>\\{tail}", &lower[..prefix_end]);
            }
        }
    }

    lower
}

fn local_learning_threshold(process: &ProcessSnapshot) -> u64 {
    // WebView2 is a host runtime intentionally created by many Microsoft Store
    // and desktop applications. A previous local session is enough to stop
    // treating the same installed parent relation as novel. Other installed
    // parent/child pairs require two previous sessions.
    if process.name.eq_ignore_ascii_case("msedgewebview2.exe")
        && normalize_install_identity(&process.exe).contains("\\microsoft\\edgewebview\\application\\")
    {
        1
    } else {
        2
    }
}

fn auto_learnable_install_path(path: &str) -> bool {
    let lower = path.replace('/', "\\").to_ascii_lowercase();
    let user_writable = lower.contains("\\users\\")
        || lower.contains("\\appdata\\")
        || lower.contains("\\temp\\")
        || lower.contains("\\downloads\\");

    !user_writable
        && (lower.contains("\\program files\\")
            || lower.contains("\\program files (x86)\\")
            || lower.contains("\\windows\\systemapps\\")
            || lower.contains("\\windows\\system32\\"))
}

fn display_path(path: &str) -> &str {
    if path.trim().is_empty() { "<no disponible>" } else { path }
}

fn triage_connections(pids: &HashSet<u32>) -> String {
    if pids.is_empty() {
        return "No hay PIDs de seguridad que correlacionar en esta muestra.\n".to_owned();
    }

    let raw = match capture("netstat.exe", &["-ano"]) {
        Ok(raw) => raw,
        Err(error) => return format!("No disponible: {error}\n"),
    };

    let mut out = String::new();
    for line in raw.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let Some(pid) = fields.last().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        if pids.contains(&pid) {
            out.push_str(line.trim());
            out.push('\n');
        }
    }

    if out.is_empty() {
        "No se observaron sockets TCP/UDP asociados a esos PIDs.\n".to_owned()
    } else {
        out
    }
}

fn analyze_security(
    snapshots: &[ProcessSnapshot],
    history: Option<&HashMap<String, ParentProfile>>,
) -> Vec<Finding> {
    analyze_security_with_trust(snapshots, history, None, None)
}

fn analyze_security_with_trust(
    snapshots: &[ProcessSnapshot],
    history: Option<&HashMap<String, ParentProfile>>,
    trusted_lineages: Option<&HashSet<(String, String)>>,
    learned_lineages: Option<&HashMap<(String, String), u64>>,
) -> Vec<Finding> {
    let mut findings = snapshots
        .iter()
        .filter_map(|process| {
            let result = assess_process_with_trust(
                process,
                snapshots,
                history,
                trusted_lineages,
                learned_lineages,
            );
            (result.classification >= AttentionLevel::Attention).then(|| Finding {
                pid: process.pid,
                name: process.name.clone(),
                level: result.classification,
                reasons: result.reasons,
                limitations: result.limitations,
            })
        })
        .collect::<Vec<_>>();
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
                limitations: Vec::new(),
            })
        })
        .collect::<Vec<_>>();

    rows.sort_by(|a, b| {
        b.reasons
            .len()
            .cmp(&a.reasons.len())
            .then_with(|| a.pid.cmp(&b.pid))
    });
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
        for limitation in &finding.limitations {
            out.push_str(&format!("  Incomplete: {limitation}\n"));
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
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS schema_meta (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         INSERT OR IGNORE INTO schema_meta(key, value) VALUES ('schema_version', '2');
         UPDATE schema_meta SET value = '2' WHERE key = 'schema_version';

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

         CREATE TABLE IF NOT EXISTS correlation_history (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             pid INTEGER NOT NULL,
             start_time INTEGER NOT NULL,
             exe TEXT NOT NULL,
             observed_at INTEGER NOT NULL,
             classification TEXT NOT NULL,
             reasons TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_correlation_instance ON correlation_history(pid, start_time, exe, id);

         CREATE TABLE IF NOT EXISTS trusted_lineage (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             child_exe TEXT NOT NULL COLLATE NOCASE,
             parent_exe TEXT NOT NULL COLLATE NOCASE,
             created_at INTEGER NOT NULL,
             UNIQUE(child_exe, parent_exe)
         );

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

fn persist_snapshot(
    paths: &AppPaths,
    snapshots: &[ProcessSnapshot],
    report: &PreloadReport,
) -> Result<()> {
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
    let text =
        fs::read_to_string(path).with_context(|| format!("no se pudo leer {}", path.display()))?;
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

        let Some(source) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
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

fn cached_intel(
    conn: &Connection,
    indicator: &str,
    source_id: &str,
    now: i64,
) -> Result<Option<(String, i64)>> {
    match conn.query_row(
        "SELECT verdict, checked_at FROM intel_cache
         WHERE indicator = ?1 AND source_id = ?2 AND (expires_at IS NULL OR expires_at > ?3)",
        params![indicator, source_id, now],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    ) {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn store_intel_cache(
    conn: &Connection,
    indicator: &str,
    source: &SecuritySource,
    verdict: &str,
    checked_at: i64,
) -> Result<()> {
    let ttl_seconds = source.ttl_hours.saturating_mul(3600).min(i64::MAX as u64) as i64;
    let expires_at = checked_at.saturating_add(ttl_seconds);
    conn.execute(
        "INSERT INTO intel_cache(indicator, source_id, verdict, checked_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(indicator, source_id) DO UPDATE SET
             verdict=excluded.verdict,
             checked_at=excluded.checked_at,
             expires_at=excluded.expires_at",
        params![indicator, source.id, verdict, checked_at, expires_at],
    )?;
    Ok(())
}

fn query_intel_source(
    source: &SecuritySource,
    indicator: &str,
    paths: &AppPaths,
) -> Result<Option<String>> {
    match source.adapter.as_str() {
        "abusech_hash" => query_abusech_hash(source, indicator, paths),
        "abusech_ioc" => query_abusech_ioc(source, indicator, paths),
        "abusech_url" => query_abusech_url(source, indicator, paths),
        "behavior_catalog" => Ok(None),
        adapter => anyhow::bail!("adaptador no soportado: {adapter}"),
    }
}

fn curl_json_post(
    endpoint: &str,
    auth: Option<(&str, &str)>,
    fields: &[(&str, &str)],
) -> Result<serde_json::Value> {
    let mut command = Command::new("curl.exe");
    command.args(["-fsS", "--connect-timeout", "5", "--max-time", "15", "-X", "POST"]);
    command.args(["-H", "Accept: application/json"]);
    if let Some((header, value)) = auth {
        let auth_header = format!("{header}: {value}");
        command.args(["-H", auth_header.as_str()]);
    }
    for (key, value) in fields {
        command.args(["--data-urlencode", &format!("{key}={value}")]);
    }
    command.arg(endpoint);
    let output = command.output().context("no se pudo ejecutar curl.exe")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        anyhow::bail!(
            "HTTP/curl {}{}",
            output.status.code().unwrap_or(-1),
            if stderr.is_empty() { String::new() } else { format!(": {stderr}") }
        );
    }
    serde_json::from_slice(&output.stdout).context("respuesta JSON inválida")
}

fn source_auth(source: &SecuritySource, paths: &AppPaths) -> Result<Option<String>> {
    let Some(name) = source.auth_env.as_deref() else {
        return Ok(None);
    };

    if let Ok(value) = env::var(name)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }

    if let Some(value) = paths.config_value(name)?
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }

    anyhow::bail!(
        "falta credencial {name}; defínela en el entorno o en {}",
        paths.config_file().display()
    )
}

fn query_abusech_hash(
    source: &SecuritySource,
    indicator: &str,
    paths: &AppPaths,
) -> Result<Option<String>> {
    if !is_hash_indicator(indicator) {
        return Ok(None);
    }
    let auth = source_auth(source, paths)?;
    let json = curl_json_post(
        &source.endpoint,
        auth.as_deref().map(|key| ("Auth-Key", key)),
        &[("query", "get_info"), ("hash", indicator)],
    )?;
    match json.get("query_status").and_then(|v| v.as_str()).unwrap_or("") {
        "ok" => {
            let item = json.get("data").and_then(|v| v.as_array()).and_then(|v| v.first());
            let family = item.and_then(|v| v.get("signature")).and_then(|v| v.as_str());
            Ok(Some(match family {
                Some(family) if !family.is_empty() => format!("malicious ({family})"),
                _ => "malicious".to_owned(),
            }))
        }
        "hash_not_found" | "file_not_found" => Ok(None),
        status => anyhow::bail!("API status {}", if status.is_empty() { "unknown" } else { status }),
    }
}

fn query_abusech_ioc(
    source: &SecuritySource,
    indicator: &str,
    paths: &AppPaths,
) -> Result<Option<String>> {
    let auth = source_auth(source, paths)?;
    let json = curl_json_post(
        &source.endpoint,
        auth.as_deref().map(|key| ("Auth-Key", key)),
        &[("query", "search_ioc"), ("search_term", indicator), ("exact_match", "true")],
    )?;
    match json.get("query_status").and_then(|v| v.as_str()).unwrap_or("") {
        "ok" => {
            let item = json.get("data").and_then(|v| v.as_array()).and_then(|v| v.first());
            let malware = item.and_then(|v| v.get("malware_printable"))
                .or_else(|| item.and_then(|v| v.get("malware")))
                .and_then(|v| v.as_str());
            Ok(Some(match malware {
                Some(name) if !name.is_empty() => format!("malicious ({name})"),
                _ => "malicious".to_owned(),
            }))
        }
        "no_result" | "ioc_not_found" => Ok(None),
        status => anyhow::bail!("API status {}", if status.is_empty() { "unknown" } else { status }),
    }
}

fn query_abusech_url(
    source: &SecuritySource,
    indicator: &str,
    paths: &AppPaths,
) -> Result<Option<String>> {
    let auth = source_auth(source, paths)?;
    let (endpoint, field) = if is_hash_indicator(indicator) {
        (format!("{}/v1/payload/", source.endpoint.trim_end_matches('/')), "sha256_hash")
    } else if indicator.starts_with("http://") || indicator.starts_with("https://") {
        (format!("{}/v1/url/", source.endpoint.trim_end_matches('/')), "url")
    } else {
        (format!("{}/v1/host/", source.endpoint.trim_end_matches('/')), "host")
    };
    let json = curl_json_post(
        &endpoint,
        auth.as_deref().map(|key| ("Auth-Key", key)),
        &[(field, indicator)],
    )?;
    match json.get("query_status").and_then(|v| v.as_str()).unwrap_or("") {
        "ok" => {
            let threat = json.get("threat").and_then(|v| v.as_str())
                .or_else(|| json.get("signature").and_then(|v| v.as_str()));
            Ok(Some(match threat {
                Some(name) if !name.is_empty() => format!("malicious ({name})"),
                _ => "malicious".to_owned(),
            }))
        }
        "no_results" | "url_not_found" | "host_not_found" | "payload_not_found" => Ok(None),
        status => anyhow::bail!("API status {}", if status.is_empty() { "unknown" } else { status }),
    }
}

fn is_hash_indicator(value: &str) -> bool {
    matches!(value.len(), 32 | 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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
        folders.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }
    if let Some(programdata) = env::var_os("PROGRAMDATA") {
        folders.push(
            PathBuf::from(programdata).join(r"Microsoft\Windows\Start Menu\Programs\StartUp"),
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
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if text.lines().any(|line| {
            let value = line
                .split('#')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
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

#[cfg(test)]
mod correlation_tests {
    use super::*;
    fn process(pid: u32, ppid: u32, name: &str, exe: &str, command: &str) -> ProcessSnapshot {
        ProcessSnapshot {
            pid,
            ppid,
            start_time: 100,
            name: name.into(),
            exe: exe.into(),
            command_line: command.into(),
            cpu: 0.0,
            memory_mib: 0.0,
            disk_read_bytes: 0,
            disk_written_bytes: 0,
            new_to_history: true,
        }
    }
    #[test]
    fn path_or_interpreter_alone_is_not_suspicious() {
        let rows = [
            process(1, 0, "pwsh.exe", r"C:\Windows\pwsh.exe", "pwsh.exe"),
            process(
                2,
                0,
                "helper.exe",
                r"C:\Users\a\AppData\helper.exe",
                "helper",
            ),
        ];
        assert!(analyze_security(&rows, Some(&HashMap::new())).is_empty());
    }
    #[test]
    fn known_executable_is_not_immune_to_current_execution() {
        let mut child = process(
            2,
            1,
            "pwsh.exe",
            r"C:\Users\a\AppData\pwsh.exe",
            "pwsh.exe -enc abc",
        );
        child.new_to_history = false;
        let rows = [
            process(1, 0, "cmd.exe", r"C:\Windows\cmd.exe", "cmd"),
            child,
        ];
        assert_eq!(
            analyze_security(&rows, Some(&HashMap::new()))[0].level,
            AttentionLevel::Suspicious
        );
    }
    #[test]
    fn parent_identity_change_matters_not_parent_pid() {
        let rows = [
            process(7, 0, "app.exe", "app.exe", "app"),
            process(8, 7, "helper.exe", "helper.exe", "helper"),
        ];
        let mut history = HashMap::new();
        history.insert(
            "helper.exe".into(),
            ParentProfile {
                executions: std::collections::BTreeMap::from([("app.exe".into(), 20)]),
            },
        );
        assert!(analyze_security(&rows, Some(&history)).is_empty());
        history.insert(
            "helper.exe".into(),
            ParentProfile {
                executions: std::collections::BTreeMap::from([("other.exe".into(), 20)]),
            },
        );
        assert_eq!(
            analyze_security(&rows, Some(&history))[0].level,
            AttentionLevel::Attention
        );
    }
}
