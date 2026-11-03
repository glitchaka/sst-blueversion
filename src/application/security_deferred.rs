//! Colectores diferidos de seguridad. Se ejecutan fuera del camino critico
//! del prompt; sus ausencias nunca se convierten en detecciones de malware.
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
    thread,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use std::{process::{Command, Stdio}, time::{Duration, Instant}};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    AppPaths, AttentionLevel, Evidence, Family, Finding, ProcessSnapshot,
    SecurityTriageService, Strength, assess_process_with_extra, capture,
    open_security_db, startup_folders, unix_now,
};

#[derive(Clone, Default, Serialize)]
pub(super) struct Stage {
    pub name: String,
    pub state: String,
    pub detail: String,
}

#[derive(Clone)]
pub(super) struct EnrichedFinding {
    pub pid: u32,
    pub start_time: u64,
    pub exe: String,
    pub finding: Finding,
}

#[derive(Clone, Default)]
pub(super) struct DeferredState {
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub stages: Vec<Stage>,
    pub findings: Vec<EnrichedFinding>,
}

impl DeferredState {
    fn new() -> Self {
        Self {
            started_at: unix_now(),
            finished_at: None,
            stages: ["Conexiones TCP/UDP", "Inicio automatico", "Servicios", "Firmas y SHA-256"]
                .iter()
                .map(|name| Stage {
                    name: (*name).to_owned(),
                    state: "PENDING".to_owned(),
                    detail: "sin ejecutar".to_owned(),
                })
                .collect(),
            findings: Vec::new(),
        }
    }

    fn format(&self) -> String {
        let mut out = format!(
            "COLECTORES DIFERIDOS — {}\nMuestra inicial (unix): {}\n",
            if self.finished_at.is_some() { "COMPLETADOS" } else { "EN PROCESO" },
            self.started_at
        );
        for stage in &self.stages {
            out.push_str(&format!("  [{:<11}] {:<21} {}\n", stage.state, stage.name, stage.detail));
        }
        if let Some(done) = self.finished_at {
            out.push_str(&format!("Finalizado (unix): {done}\n"));
            out.push_str(&format!(
                "Procesos con hallazgos enriquecidos: {}\n",
                self.findings.len()
            ));
            for result in self.findings.iter().take(12) {
                out.push_str(&format!(
                    "  {} {} [{}]\n",
                    result.finding.level.label(),
                    result.finding.name,
                    result.pid
                ));
                for reason in result.finding.reasons.iter().take(3) {
                    out.push_str(&format!("    {reason}\n"));
                }
            }
        }
        out
    }
}

fn stage(store: &Arc<Mutex<DeferredState>>, index: usize, state: &str, detail: impl Into<String>) {
    let mut report = store.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = report.stages.get_mut(index) {
        entry.state = state.to_owned();
        entry.detail = detail.into();
    }
}

pub(super) fn start(
    paths: AppPaths,
    snapshots: Vec<ProcessSnapshot>,
    base_findings: Vec<Finding>,
    store: Arc<Mutex<DeferredState>>,
) {
    *store.lock().unwrap_or_else(|error| error.into_inner()) = DeferredState::new();
    thread::spawn(move || {
        let work = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            collect(&paths, &snapshots, &base_findings, &store)
        }));
        match work {
            Ok(findings) => {
                // Conservar en el historial tanto las señales iniciales como las
                // independientes obtenidas en segundo plano.
                let mut complete = base_findings.clone();
                for extra in &findings {
                    if let Some(item) = complete.iter_mut().find(|f| f.pid == extra.pid) {
                        item.level = item.level.max(extra.finding.level);
                        for reason in &extra.finding.reasons {
                            if !item.reasons.contains(reason) {
                                item.reasons.push(reason.clone());
                            }
                        }
                    } else {
                        complete.push(extra.finding.clone());
                    }
                }
                let service = SecurityTriageService::new(paths.clone());
                let _ = service.record_assessments(&snapshots, &complete);
                let mut report = store.lock().unwrap_or_else(|error| error.into_inner());
                report.findings = findings;
                report.finished_at = Some(unix_now());
            }
            Err(_) => {
                stage(&store, 3, "ERROR", "fallo inesperado de un colector");
                store.lock().unwrap_or_else(|error| error.into_inner()).finished_at =
                    Some(unix_now());
            }
        }
        let text = store.lock().unwrap_or_else(|error| error.into_inner()).format();
        let _ = persist_report(&paths, &text);
    });
}

pub(super) fn status(
    paths: &AppPaths,
    store: &Arc<Mutex<DeferredState>>,
) -> String {
    let current = store.lock().unwrap_or_else(|error| error.into_inner()).clone();
    if current.started_at > 0 {
        return current.format();
    }
    if let Ok(conn) = open_security_db(paths) {
        if conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS deferred_reports (
               observed_at INTEGER NOT NULL, summary TEXT NOT NULL
             );",
        ).is_ok() {
            if let Ok(Some(report)) = conn.query_row(
                "SELECT summary FROM deferred_reports ORDER BY observed_at DESC, rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            ).optional() {
                return format!("Ultimo informe guardado (no es una lectura actual):\n{report}");
            }
        }
    }
    "Los colectores diferidos aun no se ejecutaron en esta sesion.\n".to_owned()
}

pub(super) fn for_process(
    store: &Arc<Mutex<DeferredState>>,
    pid: u32,
    start_time: u64,
    exe: &str,
) -> Option<Finding> {
    let report = store.lock().unwrap_or_else(|error| error.into_inner());
    if report.finished_at.is_none() {
        return None;
    }
    report.findings.iter().find(|row| {
        row.pid == pid
            && row.start_time == start_time
            && row.exe.eq_ignore_ascii_case(exe)
    }).map(|row| row.finding.clone())
}

pub(super) fn enrich(
    findings: &mut Vec<Finding>,
    snapshots: &[ProcessSnapshot],
    store: &Arc<Mutex<DeferredState>>,
) {
    let scan = store.lock().unwrap_or_else(|error| error.into_inner()).clone();
    if scan.finished_at.is_none() {
        return;
    }
    for observation in scan.findings {
        // No aplicar datos de un PID reutilizado ni de un ejecutable cambiado.
        if !snapshots.iter().any(|row| {
            row.pid == observation.pid
                && row.start_time == observation.start_time
                && row.exe.eq_ignore_ascii_case(&observation.exe)
        }) {
            continue;
        }
        if let Some(existing) = findings.iter_mut().find(|row| row.pid == observation.pid) {
            if observation.finding.level > existing.level {
                existing.level = observation.finding.level;
            }
            for reason in observation.finding.reasons {
                if !existing.reasons.contains(&reason) {
                    existing.reasons.push(reason);
                }
            }
            for limitation in observation.finding.limitations {
                if !existing.limitations.contains(&limitation) {
                    existing.limitations.push(limitation);
                }
            }
        } else {
            findings.push(observation.finding);
        }
    }
    findings.sort_by(|a, b| b.level.cmp(&a.level).then(a.pid.cmp(&b.pid)));
}

fn persist_report(paths: &AppPaths, summary: &str) -> anyhow::Result<()> {
    let conn = open_security_db(paths)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deferred_reports (
            observed_at INTEGER NOT NULL, summary TEXT NOT NULL
         );",
    )?;
    conn.execute(
        "INSERT INTO deferred_reports(observed_at, summary) VALUES (?1, ?2)",
        params![unix_now(), summary],
    )?;
    conn.execute(
        "DELETE FROM deferred_reports WHERE rowid NOT IN
            (SELECT rowid FROM deferred_reports ORDER BY observed_at DESC, rowid DESC LIMIT 30)",
        [],
    )?;
    Ok(())
}

fn collect(
    paths: &AppPaths,
    snapshots: &[ProcessSnapshot],
    base_findings: &[Finding],
    store: &Arc<Mutex<DeferredState>>,
) -> Vec<EnrichedFinding> {
    let mut extra: HashMap<u32, Vec<Evidence>> = HashMap::new();
    let by_pid = snapshots.iter().map(|p| (p.pid, p)).collect::<HashMap<_, _>>();

    stage(store, 0, "RUNNING", "consultando sockets del sistema");
    match capture("netstat.exe", &["-ano"]) {
        Ok(raw) if raw.lines().any(|line| line.trim_start().starts_with("TCP") || line.trim_start().starts_with("UDP")) => {
            let mut seen = HashSet::new();
            for line in raw.lines() {
                let parts = line.split_whitespace().collect::<Vec<_>>();
                if parts.len() < 4 || !parts[0].eq_ignore_ascii_case("TCP") {
                    continue;
                }
                let Some(pid) = parts.last().and_then(|text| text.parse::<u32>().ok()) else {
                    continue;
                };
                let Some(row) = by_pid.get(&pid) else { continue; };
                if !parts.iter().any(|item| item.eq_ignore_ascii_case("ESTABLISHED")) {
                    continue;
                }
                if !seen.insert(pid) {
                    continue;
                }
                // Una conexion normal no es prueba de ataque. Solo aporta contexto
                // debil cuando concurre con un ejecutable en ruta de usuario.
                let suspicious_location = user_writable(&row.exe);
                extra.entry(pid).or_default().push(Evidence::known(
                    Family::Network,
                    if suspicious_location { Strength::Weak } else { Strength::Context },
                    "TCP conectado en la captura de inicio",
                ));
            }
            stage(store, 0, "OK", format!("{} procesos con TCP establecido", seen.len()));
        }
        Ok(_) => stage(store, 0, "UNAVAILABLE", "netstat no devolvio sockets interpretables"),
        Err(error) => stage(store, 0, "UNAVAILABLE", error.to_string()),
    }

    stage(store, 1, "RUNNING", "consultando Run/RunOnce y carpetas Startup");
    let keys = [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce",
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
    ];
    let mut autoruns = String::new();
    let mut queried = 0;
    for key in keys {
        if let Ok(text) = capture("reg.exe", &["query", key]) {
            // Claves ausentes no cuentan como fallo de seguridad.
            if text.contains("REG_SZ") || text.contains("REG_EXPAND_SZ") {
                autoruns.push_str(&text.to_ascii_lowercase());
                autoruns.push('\n');
                queried += 1;
            }
        }
    }
    let mut startup_entries = 0;
    for folder in startup_folders() {
        if let Ok(entries) = std::fs::read_dir(&folder) {
            startup_entries += entries.flatten().count();
        }
    }
    let mut matched = 0;
    for row in snapshots {
        if row.exe.len() < 7 || !autoruns.contains(&row.exe.to_ascii_lowercase()) {
            continue;
        }
        matched += 1;
        extra.entry(row.pid).or_default().push(Evidence::known(
            Family::Persistence,
            if user_writable(&row.exe) { Strength::Weak } else { Strength::Context },
            "ruta del ejecutable presente en clave Run/RunOnce",
        ));
    }
    stage(store, 1, "OK", format!(
        "{queried} claves con valores; {startup_entries} entradas Startup; {matched} rutas coincidentes"
    ));

    stage(store, 2, "RUNNING", "consultando Service Control Manager");
    match capture("sc.exe", &["queryex", "type=", "service", "state=", "all"]) {
        Ok(raw) if raw.contains("SERVICE_NAME:") => {
            let mut current = String::new();
            let mut members: HashMap<u32, Vec<String>> = HashMap::new();
            for line in raw.lines() {
                let trimmed = line.trim();
                if let Some(name) = trimmed.strip_prefix("SERVICE_NAME:") {
                    current = name.trim().to_owned();
                } else if let Some(pid) = trimmed.strip_prefix("PID") {
                    let Some(pid) = pid.split(':').nth(1).and_then(|v| v.trim().parse::<u32>().ok()) else { continue; };
                    if pid > 0 && by_pid.contains_key(&pid) {
                        members.entry(pid).or_default().push(current.clone());
                    }
                }
            }
            for (pid, names) in &members {
                extra.entry(*pid).or_default().push(Evidence::known(
                    Family::Persistence, Strength::Context,
                    format!("servicio activo: {}", names.join(", ")),
                ));
            }
            stage(store, 2, "OK", format!("{} PIDs asociados a servicios", members.len()));
        }
        Ok(_) => stage(store, 2, "UNAVAILABLE", "SCM no devolvio servicios interpretables"),
        Err(error) => stage(store, 2, "UNAVAILABLE", error.to_string()),
    }

    stage(store, 3, "RUNNING", "comprobando firma y huella SHA-256 de ejecutables relevantes");
    let interesting_pids = base_findings.iter().map(|f| f.pid).collect::<HashSet<_>>();
    let mut relevant = snapshots.iter()
        .filter(|p| !p.exe.is_empty() && Path::new(&p.exe).is_file())
        .filter(|p| interesting_pids.contains(&p.pid) || p.new_to_history || user_writable(&p.exe))
        .collect::<Vec<_>>();
    // Priorizar lo observado como anomalo en el preload: el orden interno
    // de sysinfo no debe determinar que ejecutables se investigan.
    relevant.sort_by_key(|p| (
        if interesting_pids.contains(&p.pid) { 0 } else if user_writable(&p.exe) { 1 } else { 2 },
        !p.new_to_history,
        p.pid,
    ));
    relevant.truncate(16);
    let mut unique = HashMap::<String, Vec<&ProcessSnapshot>>::new();
    for row in relevant {
        unique.entry(row.exe.to_ascii_lowercase()).or_default().push(row);
    }
    let paths_list = unique.values().map(|rows| rows[0].exe.clone()).collect::<Vec<_>>();
    let (signatures, signature_error) = match signatures(&paths_list) {
        Ok(s) => (Some(s), None),
        Err(error) => (None, Some(error.to_string())),
    };

    let conn = open_security_db(paths).ok();
    if let Some(conn) = &conn {
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS executable_digests (
                exe TEXT PRIMARY KEY COLLATE NOCASE,
                sha256 TEXT NOT NULL,
                observed_at INTEGER NOT NULL
             );",
        );
    }
    let mut hashed = 0;
    let mut changed = 0;
    for (path, rows) in unique {
        if let Some(signatures) = &signatures {
            if let Some(signature) = signatures.get(&path) {
                if signature != "Valid" && signature != "NotSigned" {
                    for process in &rows {
                        extra.entry(process.pid).or_default().push(Evidence::known(
                            Family::Identity,
                            if user_writable(&process.exe) { Strength::Weak } else { Strength::Context },
                            format!("estado Authenticode: {signature}"),
                        ));
                    }
                }
            }
        }
        let Ok(Some(digest)) = sha256_file(&rows[0].exe) else { continue; };
        hashed += 1;
        if let Some(conn) = &conn {
            let previous: Option<String> = conn.query_row(
                "SELECT sha256 FROM executable_digests WHERE exe = ?1",
                params![&path],
                |row| row.get(0),
            ).optional().unwrap_or(None);
            if let Some(previous) = previous {
                if !previous.eq_ignore_ascii_case(&digest) {
                    changed += 1;
                    for process in &rows {
                        extra.entry(process.pid).or_default().push(Evidence::known(
                            Family::Identity, Strength::Strong,
                            "SHA-256 distinto al ultimo hash observado en esta ruta",
                        ));
                    }
                }
            }
            let _ = conn.execute(
                "INSERT INTO executable_digests(exe,sha256,observed_at) VALUES (?1,?2,?3)
                 ON CONFLICT(exe) DO UPDATE SET sha256=excluded.sha256, observed_at=excluded.observed_at",
                params![path, digest, unix_now()],
            );
        }
    }
    let mut detail = format!(
        "{} firmas consultadas; {hashed} hashes SHA-256; {changed} cambios historicos; maximo 16 rutas relevantes",
        signatures.as_ref().map_or(0, HashMap::len)
    );
    if let Some(error) = &signature_error {
        detail.push_str(&format!("; Authenticode no disponible: {error}"));
    }
    if conn.is_none() {
        detail.push_str("; historial de hashes no disponible");
    }
    stage(store, 3, if signature_error.is_some() || conn.is_none() { "PARTIAL" } else { "OK" }, detail);

    let service = SecurityTriageService::new(paths.clone());
    let instances = snapshots.iter().map(|p| (p.pid, p.start_time)).collect::<Vec<_>>();
    let history = service.historical_parents(&instances).ok();
    let trusted = service.trusted_lineages().ok();
    let learned = service.learned_lineages().ok();
    let mut results = Vec::new();
    for process in snapshots {
        let Some(items) = extra.get(&process.pid) else { continue; };
        let assessment = assess_process_with_extra(
            process, snapshots, history.as_ref(),
            trusted.as_ref(), learned.as_ref(), items,
        );
        if assessment.classification < AttentionLevel::Attention {
            continue;
        }
        results.push(EnrichedFinding {
            pid: process.pid,
            start_time: process.start_time,
            exe: process.exe.clone(),
            finding: Finding {
                pid: process.pid,
                name: process.name.clone(),
                level: assessment.classification,
                reasons: assessment.reasons,
                limitations: assessment.limitations,
            },
        });
    }
    results.sort_by(|a, b| b.finding.level.cmp(&a.finding.level).then(a.pid.cmp(&b.pid)));
    results
}

fn user_writable(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains(r"\appdata\") || lower.contains(r"\temp\") || lower.contains(r"\downloads\")
}

fn sha256_file(path: &str) -> anyhow::Result<Option<String>> {
    let meta_before = std::fs::metadata(path)?;
    if !meta_before.is_file() || meta_before.len() > 64 * 1024 * 1024 {
        return Ok(None);
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 { break; }
        digest.update(&buffer[..read]);
    }
    let meta_after = std::fs::metadata(path)?;
    if meta_before.len() != meta_after.len()
        || meta_before.modified().ok() != meta_after.modified().ok() {
        return Ok(None);
    }
    Ok(Some(format!("{:x}", digest.finalize())))
}

#[cfg(windows)]
fn signatures(paths: &[String]) -> anyhow::Result<HashMap<String, String>> {
    if paths.is_empty() { return Ok(HashMap::new()); }
    let list = paths.iter()
        .map(|p| format!("'{}'", p.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n\
         $rows = New-Object 'System.Collections.Generic.List[object]'\n\
         foreach ($p in @({list})) {{\n\
           try {{\n\
             $s = Get-AuthenticodeSignature -LiteralPath $p -ErrorAction Stop\n\
             $rows.Add([pscustomobject]@{{ path=[string]$p; status=[string]$s.Status }})\n\
           }} catch {{ }}\n\
         }}\n\
         [Console]::Out.Write((ConvertTo-Json -InputObject @($rows.ToArray()) -Depth 3 -Compress))\n"
    );
    let executable = crate::support::windows::system32_executable(
        r"WindowsPowerShell\v1.0\powershell.exe",
    )?;
    let wide = script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, wide);
    let mut command = Command::new(executable);
    command.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000);
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if child.try_wait()?.is_some() { break; }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Authenticode excedio 12 segundos");
        }
        thread::sleep(Duration::from_millis(80));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() { anyhow::bail!("Authenticode fallo"); }
    if output.stdout.len() > 256 * 1024 { anyhow::bail!("salida Authenticode excesiva"); }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut result = HashMap::new();
    for row in json.as_array().into_iter().flatten() {
        let Some(path) = row.get("path").and_then(serde_json::Value::as_str) else { continue; };
        let Some(status) = row.get("status").and_then(serde_json::Value::as_str) else { continue; };
        result.insert(path.to_ascii_lowercase(), status.to_owned());
    }
    Ok(result)
}

#[cfg(not(windows))]
fn signatures(_paths: &[String]) -> anyhow::Result<HashMap<String, String>> {
    anyhow::bail!("Authenticode solo disponible en Windows")
}
