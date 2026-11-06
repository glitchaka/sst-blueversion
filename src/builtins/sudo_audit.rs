//! Local, non-network sudo audit spool. Never collect access tokens or argv.
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use anyhow::{Context, Result};
use chrono::Local;
use serde_json::json;

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(super) struct AuditEvent {
    id: String,
    category: &'static str,
    user: String,
    computer: String,
    privilege: String,
    command: String,
}

impl AuditEvent {
    pub(super) fn new(
        command: &str,
        args: &[String],
        user: String,
        privilege: &str,
    ) -> Self {
        let nanoseconds = SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap_or_default().as_nanos();
        Self {
            id: format!("{nanoseconds:x}-{:x}-{:x}",
                process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)),
            category: category(args),
            user,
            computer: env::var("COMPUTERNAME").unwrap_or_else(|_| "UNKNOWN".to_owned()),
            privilege: privilege.to_owned(),
            command: command.to_owned(),
        }
    }

    pub(super) fn record(&self, stage: &str, exit_status: Option<i32>) -> Result<()> {
        let now = Local::now();
        let value = json!({
            "schema": 1,
            "event_id": self.id,
            "event": "sudo",
            "stage": stage,
            "timestamp": now.to_rfc3339(),
            "date": now.format("%Y-%m-%d").to_string(),
            "time": now.format("%H:%M:%S%.3f").to_string(),
            "timezone": now.format("%:z").to_string(),
            "computer": self.computer,
            "user": self.user,
            "session_privilege": self.privilege,
            "invoked_as": self.command,
            "operation": self.category,
            "exit_status": exit_status,
            "sst_version": env!("CARGO_PKG_VERSION"),
        });
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let path = audit_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("no se pudo crear {}", parent.display()))?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)
            .with_context(|| format!("no se pudo abrir {}", path.display()))?;
        file.write_all(&bytes)?;
        file.sync_data()?;
        Ok(())
    }
}

fn audit_file() -> PathBuf {
    crate::adapters::persistence::AppPaths::detect()
        .data_dir().join("sudo-audit.jsonl")
}

fn category(args: &[String]) -> &'static str {
    match args.first().map(String::as_str) {
        None => "session_elevation",
        Some("--status") => "status",
        Some("--system") => "system_operation",
        Some("--trustedinstaller") => "trustedinstaller_operation",
        Some("-h" | "--help") => "help",
        Some(_) => "admin_operation",
    }
}

#[cfg(test)]
mod tests {
    use super::category;
    #[test]
    fn logs_category_without_secret_arguments() {
        assert_eq!(category(&[]), "session_elevation");
        assert_eq!(category(&["--system".to_owned(), "PASSWORD".to_owned()]),
                   "system_operation");
        assert_eq!(category(&["--trustedinstaller".to_owned()]),
                   "trustedinstaller_operation");
    }
}
