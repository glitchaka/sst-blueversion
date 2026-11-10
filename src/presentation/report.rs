//! Visual treatment for direct interactive SST reports.
//! This layer never changes the shell's data stream, files, redirections or pipes.
//! libcaca renders the frame; Slint/vt100 remains the terminal.
use libcaca::{Canvas, Color};

const RESET: &str = "\x1b[0m";
const HEADER: &str = "\x1b[1;38;5;117m";
const MUTED: &str = "\x1b[38;5;247m";
const SUCCESS: &str = "\x1b[1;38;5;114m";
const WARNING: &str = "\x1b[1;38;5;221m";
const FAILURE: &str = "\x1b[1;38;5;203m";

fn suitable(command: &str, stdout: &str) -> bool {
    if stdout.is_empty() || stdout.contains('\x1b')
        || command.chars().any(|ch| "|;&><\n\r".contains(ch))
        || command.split_whitespace().any(|part| matches!(part, "--json" | "--csv" | "-j"))
    {
        return false;
    }
    matches!(
        command.split_whitespace().next().unwrap_or(""),
        "notsh" | "net" | "sys" | "fetch" | "help" | "diag" | "diagnose"
            | "domain" | "device" | "tools" | "sigsour" | "service"
            | "process" | "firewall" | "eventlog"
    )
}

/// Actual libcaca canvas: border and text are rasterized into ANSI terminal cells,
/// not a fake text banner or an additional window.
fn canvas_title(command: &str) -> Option<String> {
    let title: String = command.chars().take(62).collect();
    let width = (title.chars().count() as i32 + 6).clamp(36, 76);
    let mut canvas = Canvas::new(width, 3).ok()?;
    canvas.set_color_ansi(Color::LightCyan, Color::Default).ok()?;
    canvas.draw_thin_box(0, 0, width - 1, 2);
    canvas.set_color_ansi(Color::White, Color::Default).ok()?;
    canvas.put_str(2, 1, &title);
    let ansi = canvas.export_to_memory("ansi").ok()?;
    String::from_utf8(ansi).ok()
}

// Keep the value legible and emphasize only its real state; muted entire rows
// made the operational details harder to read against SST's dark theme.
fn render_field(line: &str) -> Option<String> {
    let (field, value) = line.split_once(": ")?;
    let label = field.trim_start();
    if label.is_empty() || label.len() > 42 || label.contains("  ") {
        return None;
    }
    let indent_len = field.len() - label.len();
    let style = emphasis(value).unwrap_or(RESET);
    let mut result = String::new();
    result.push_str(&line[..indent_len]);
    result.push_str(MUTED);
    result.push_str(label);
    result.push(':');
    result.push_str(RESET);
    result.push(' ');
    if style != RESET { result.push_str(style); }
    result.push_str(value);
    if style != RESET { result.push_str(RESET); }
    Some(result)
}

fn emphasis(line: &str) -> Option<&'static str> {
    let trimmed = line.trim();
    if trimmed.is_empty() { return None; }
    let uppercase = trimmed.to_uppercase();
    if uppercase.contains("ERROR") || uppercase.contains("FALLO")
        || uppercase.contains("SIN ENLACE") || uppercase.contains("DENEGAD")
        || uppercase.contains("DESHABILITAD") || uppercase.contains("NO DISPONIBLE")
        || uppercase.contains("DESCONECTADO") || uppercase.contains("DETENIDO")
        || uppercase.contains("NO OPERATIV") || uppercase.contains("NO ACTIVO")
    {
        Some(FAILURE)
    } else if uppercase.contains("ADVERTENCIA") || uppercase.contains("PENDIENTE")
        || uppercase.contains("INDETERMINADO") || uppercase.contains("SIN IP")
        || uppercase.contains("NO CONFIRMA") || uppercase.contains("NO SE PUDO")
        || uppercase.contains("NO VERIFICADO")
    {
        Some(WARNING)
    } else if uppercase.contains("ACTIVO") || uppercase.contains("CONECTADO")
        || uppercase.contains("CORRECTO") || uppercase.contains("ESTABLECIDO")
        || uppercase.contains("OPERATIVA") || uppercase.contains("UP ")
    {
        Some(SUCCESS)
    } else if trimmed.starts_with("== ") || trimmed.starts_with('[')
        || (trimmed.chars().any(|ch| ch.is_alphabetic())
            && trimmed.chars().all(|ch| !ch.is_alphabetic() || ch.is_uppercase())
            && trimmed.chars().count() < 90)
    {
        Some(HEADER)
    } else {
        None
    }
}

/// Only call from the interactive GUI after a command has completed.
/// Raw CLI mode, scripts, redirections, pipes, JSON and CSV are unchanged.
pub fn decorate(command: &str, stdout: &str) -> String {
    if !suitable(command, stdout) { return stdout.to_owned(); }
    let mut result = canvas_title(command).unwrap_or_else(|| {
        format!("{HEADER}── {command} ──{RESET}\n")
    });
    if !result.ends_with('\n') { result.push('\n'); }
    for chunk in stdout.split_inclusive('\n') {
        let (line, newline) = match chunk.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (chunk, ""),
        };
        if let Some(field) = render_field(line) {
            result.push_str(&field);
        } else if let Some(color) = emphasis(line) {
            result.push_str(color);
            result.push_str(line);
            result.push_str(RESET);
        } else {
            result.push_str(line);
        }
        result.push_str(newline);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts_and_machine_output_remain_unmodified() {
        let text = "STATE   VALUE\nUP      1\n";
        for command in ["notsh interfaces | cat", "notsh interfaces > out.txt",
            "notsh interfaces --json", "echo foo", "notsh interfaces; echo ok"] {
            assert_eq!(decorate(command, text), text, "{command}");
        }
    }
    #[test]
    fn preserves_precoloured_terminal_data() {
        let ansi = "\x1b[31mError\x1b[0m\n";
        assert_eq!(decorate("sys info", ansi), ansi);
    }
    #[test]
    fn field_labels_are_muted_but_readings_remain_legible() {
        let rendered = render_field("  Señal: 82%").expect("field parsed");
        assert!(rendered.starts_with(&format!("  {MUTED}Señal:")));
        assert!(rendered.ends_with(" 82%"));
        assert!(!rendered.ends_with(MUTED));
        let state = render_field("  Estado: SIN ENLACE").expect("status parsed");
        assert!(state.contains(FAILURE));
    }

    #[test]
    fn visually_distinguishes_diagnostics_without_destroying_columns() {
        let text = "ESTADO   DETALLE\n  Estado: ACTIVO\n  Estado: SIN ENLACE\n";
        let rendered = decorate("notsh interfaces", text);
        assert!(rendered.contains("ESTADO   DETALLE"));
        assert!(rendered.contains(SUCCESS));
        assert!(rendered.contains(FAILURE));
    }
}
