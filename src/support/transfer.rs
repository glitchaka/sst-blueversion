//! Live transfer indicator for native SST downloads.
//! Uses actual downloaded file bytes; never guesses a percentage or ETA when
//! the HTTP response has not supplied a known total. No terminal decorations
//! are written to redirected CLI stdout.
use std::{
    fs,
    io::IsTerminal,
    path::Path,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::adapters::terminal::io;

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn eta(seconds: f64) -> String {
    let secs = seconds.ceil().max(0.0) as u64;
    if secs >= 3600 {
        format!("{}h {:02}m", secs / 3600, secs % 3600 / 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

/// One redrawable, fixed-width status line, backed by real file-system bytes.
fn draw(label: &str, current: u64, expected: Option<u64>, rate: f64, columns: usize) -> String {
    let columns = columns.max(35);
    let width = if columns >= 105 { 22 } else if columns >= 85 { 15 } else { 10 };
    let fraction = expected.filter(|v| *v > 0)
        .map(|total| current.min(total) as f64 / total as f64);
    let filled = fraction.map(|p| (p * width as f64).round() as usize).unwrap_or(0);
    let mut bar = String::new();
    bar.push_str(&"█".repeat(filled.min(width)));
    bar.push_str(&"░".repeat(width.saturating_sub(filled)));
    let progress = fraction
        .map(|p| format!("{:>3}%", (p * 100.0).round() as u32))
        .unwrap_or_else(|| " --%".to_owned());
    let speed = if rate > 0.0 { format!("{}/s", human_bytes(rate as u64)) } else { "—/s".to_owned() };
    let remaining = match (expected, rate > 0.0) {
        (Some(total), true) if total >= current =>
            eta((total - current) as f64 / rate),
        _ => "—".to_owned(),
    };
    if columns >= 90 {
        format!("  {label}  \x1b[96m{bar}\x1b[0m  {progress}  {} / {}  {speed}  ETA {remaining}",
            human_bytes(current), expected.map(human_bytes).unwrap_or_else(|| "?".into()))
    } else {
        format!("  {label} \x1b[96m{bar}\x1b[0m {progress}  {speed}  ETA {remaining}")
    }
}

/// Execute a configured curl download and show a transfer meter only on an
/// interactive console. Commands remain hidden on Windows, and scripts
/// redirecting CLI output receive no terminal escape sequences.
pub fn run(
    command: &mut Command,
    destination: &Path,
    expected: Option<u64>,
    label: &str,
) -> Result<Output> {
    let interactive = io::output_sender().is_some() || std::io::stdout().is_terminal();
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command.spawn().context("no se pudo iniciar la transferencia")?;
    let cols = io::size().map(|(cols, _)| cols as usize).unwrap_or(100);
    let started = Instant::now();
    let mut last = Instant::now() - Duration::from_secs(1);
    if interactive {
        let _ = io::write_raw(b"\x1b[?25l\r\x1b[2K");
    }
    loop {
        if let Some(_) = child.try_wait()? { break; }
        if interactive && last.elapsed() >= Duration::from_millis(120) {
            let current = fs::metadata(destination).map(|m| m.len()).unwrap_or(0);
            let rate = current as f64 / started.elapsed().as_secs_f64().max(0.001);
            let line = draw(label, current, expected, rate, cols);
            let _ = io::write_raw(format!("\r\x1b[2K{line}").as_bytes());
            last = Instant::now();
        }
        thread::sleep(Duration::from_millis(75));
    }
    let output = child.wait_with_output()?;
    if interactive {
        let current = fs::metadata(destination).map(|m| m.len()).unwrap_or(0);
        let rate = current as f64 / started.elapsed().as_secs_f64().max(0.001);
        let line = draw(label, current, expected, rate, cols);
        let suffix = if output.status.success() { "\x1b[92m✓\x1b[0m" } else { "\x1b[91m✕\x1b[0m" };
        let _ = io::write_raw(format!("\r\x1b[2K{line} {suffix}\r\n\x1b[?25h").as_bytes());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{draw, eta, human_bytes};

    #[test]
    fn uses_actual_bytes_and_zero_guessing() {
        let title = draw("SST", 5_242_880, Some(10_485_760), 1_048_576.0, 120);
        assert!(title.contains("50%"));
        assert!(title.contains("5.0 MiB / 10.0 MiB"));
        assert!(title.contains("ETA 00:05"));
        let unknown = draw("SST", 1024, None, 0.0, 100);
        assert!(unknown.contains("--%"));
        assert!(unknown.contains("ETA —"));
        assert_eq!(human_bytes(1_048_576), "1.0 MiB");
        assert_eq!(eta(61.0), "01:01");
    }
}
