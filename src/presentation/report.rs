//! Adaptive, accessible rendering for human-facing interactive SST reports.
//! No changes are made to data streams, pipes, redirections, plain/CSV/JSON
//! output, scripts, or terminal applications that already supply ANSI styling.
use libcaca::{Canvas, Color};

const RESET: &str = "\x1b[0m";
const HEADING: &str = "\x1b[1;38;5;117m";
const LABEL: &str = "\x1b[38;5;252m";
const GOOD: &str = "\x1b[1;38;5;114m";
const CAUTION: &str = "\x1b[1;38;5;221m";
const BAD: &str = "\x1b[1;38;5;203m";
const BULLET: &str = "\x1b[38;5;117m";

fn suitable(command: &str, stdout: &str) -> bool {
    if stdout.trim().is_empty()
        || stdout.contains('\x1b')
        || command.chars().any(|ch| "|;&><\n\r".contains(ch))
        || command.split_whitespace().any(|part|
            matches!(part, "--json" | "--csv" | "--plain" | "--stdout" | "--xml" | "-j"))
    {
        return false;
    }
    matches!(
        command.split_whitespace().next().unwrap_or(""),
        "notsh" | "net" | "instp" | "sys" | "ps" | "fetch" | "help"
          | "diag" | "diagnose" | "domain" | "device" | "tools"
          | "service" | "process" | "firewall" | "eventlog" | "update"
    )
}

/// Use actual libcaca canvas cells for a compact frame. A full-width frame is
/// deliberately avoided: it adds noise to narrow terminals and wrapped text.
fn canvas_title(command: &str, width: usize) -> Option<String> {
    if width < 28 { return None; }
    let name = command.chars().take(width.saturating_sub(8)).collect::<String>();
    let frame_width = (name.chars().count() + 6).clamp(28, width) as i32;
    let mut canvas = Canvas::new(frame_width, 3).ok()?;
    canvas.set_color_ansi(Color::LightCyan, Color::Default).ok()?;
    canvas.draw_thin_box(0, 0, frame_width - 1, 2);
    canvas.set_color_ansi(Color::White, Color::Default).ok()?;
    canvas.put_str(2, 1, &name);
    let ansi = canvas.export_to_memory("ansi").ok()?;
    String::from_utf8(ansi).ok()
}

fn status_color(value: &str) -> Option<&'static str> {
    // Match complete states rather than substring "CONECTADO" in
    // "DESCONECTADO" or "ACTIVO" in "INACTIVO".
    let upper = value.trim().to_uppercase();
    if ["ERROR", "FALLO", "DENEGAD", "SIN ENLACE", "NO DISPONIBLE",
        "DESCONECTADO", "DETENIDO", "DESHABILITAD", "INACTIVO", "NO OPERATIV"]
        .iter().any(|term| upper.contains(term))
    {
        Some(BAD)
    } else if ["ADVERTENCIA", "PENDIENTE", "INDETERMINADO", "SIN IP",
        "NO VERIFICADO", "NO SE PUDO", "NO CONFIRMA", "DESCONOCIDO"]
        .iter().any(|term| upper.contains(term))
    {
        Some(CAUTION)
    } else if ["CONECTADO", "CORRECTO", "ESTABLECIDO", "OPERATIVA",
        "ACTIVO", "DISPONIBLE", "EJECUTÁNDOSE"]
        .iter().any(|term| upper.contains(term))
    {
        Some(GOOD)
    } else {
        None
    }
}

fn style_line(line: &str) -> String {
    if line.trim().is_empty() { return line.to_owned(); }
    let stripped = line.trim_start();
    if let Some((field, value)) = stripped.split_once(": ") {
        // Distinguish actual key/value fields, not paragraphs containing a colon.
        if !field.is_empty() && field.chars().count() <= 40
           && !field.contains("  ") && field.chars().any(char::is_alphabetic)
        {
            let indent = &line[..line.len() - stripped.len()];
            let value_color = status_color(value).unwrap_or(RESET);
            return format!("{indent}{LABEL}{field}:{RESET} {value_color}{value}{RESET}");
        }
    }
    if stripped.starts_with('•') {
        let indent = &line[..line.len() - stripped.len()];
        return format!("{indent}{BULLET}•{RESET} {}", stripped.trim_start_matches('•').trim_start());
    }
    if stripped.eq_ignore_ascii_case("Cambios:")
        || stripped.starts_with("──")
        || stripped.starts_with("== ")
        || stripped.starts_with('[')
        || (stripped.chars().any(char::is_alphabetic)
            && stripped.chars().all(|ch| !ch.is_alphabetic() || ch.is_uppercase())
            && stripped.chars().count() < 90)
    {
        return format!("{HEADING}{line}{RESET}");
    }
    if let Some(color) = status_color(stripped) {
        // Avoid painting entire prose sentences for a single word. Only short
        // state lines have semantic colors.
        if stripped.chars().count() < 65 {
            return format!("{color}{line}{RESET}");
        }
    }
    line.to_owned()
}

fn display_width(s: &str) -> usize {
    // SST's embedded terminal primarily uses monospace Latin text and box
    // drawing. Estimate display columns to prevent viewport overflow.
    s.chars().map(|c| if c.is_ascii() || ('\u{2500}'..='\u{257f}').contains(&c) {
        1
    } else if ('\u{1100}'..='\u{115f}').contains(&c)
        || ('\u{2e80}'..='\u{a4cf}').contains(&c)
        || ('\u{ac00}'..='\u{d7a3}').contains(&c)
        || ('\u{f900}'..='\u{faff}').contains(&c)
        || ('\u{fe10}'..='\u{fe6f}').contains(&c)
        || ('\u{ff00}'..='\u{ff60}').contains(&c)
    { 2 } else { 1 }).sum()
}

/// Soft-wrap at word boundaries before passing text to vt100. Keeping complete
/// fragments with their own indentation prevents one long release note from
/// destroying the surrounding report structure.
fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if display_width(line) <= width { return vec![line.to_owned()]; }
    let leading = line.chars().take_while(|c| c.is_whitespace()).count().min(8);
    let trimmed = line.trim();
    let (first_indent, continuation) = if let Some(text) = trimmed.strip_prefix("+ ") {
        let _ = text;
        ("  • ".to_owned(), "    ".to_owned())
    } else if let Some(text) = trimmed.strip_prefix("• ") {
        let _ = text;
        ("  • ".to_owned(), "    ".to_owned())
    } else {
        (" ".repeat(leading), " ".repeat((leading + 2).min(10)))
    };
    let text = trimmed.strip_prefix("+ ").or_else(|| trimmed.strip_prefix("• ")).unwrap_or(trimmed);
    let available = width.max(12);
    let mut lines = Vec::new();
    let mut current = first_indent;
    for word in text.split_whitespace() {
        // A single long URL or hash also needs a bounded fallback; do not drop
        // or truncate any content.
        let mut word_chunk = String::new();
        for ch in word.chars() {
            let next = display_width(&word_chunk) + display_width(&ch.to_string());
            if next + display_width(&continuation) > available && !word_chunk.is_empty() {
                if !current.trim().is_empty() {
                    if display_width(&current) + display_width(&word_chunk) + 1 <= available {
                        if !current.ends_with(' ') { current.push(' '); }
                        current.push_str(&word_chunk);
                    } else {
                        lines.push(current);
                        current = format!("{continuation}{word_chunk}");
                    }
                } else {
                    current.push_str(&word_chunk);
                }
                if display_width(&current) >= available {
                    lines.push(current);
                    current = continuation.clone();
                }
                word_chunk.clear();
            }
            word_chunk.push(ch);
        }
        if word_chunk.is_empty() { continue; }
        let separator = if current.trim().is_empty() || current.ends_with(' ') { 0 } else { 1 };
        if display_width(&current) + separator + display_width(&word_chunk) > available
            && !current.trim().is_empty()
        {
            lines.push(current);
            current = continuation.clone();
        }
        if !current.ends_with(' ') && !current.trim().is_empty() { current.push(' '); }
        current.push_str(&word_chunk);
    }
    if !current.trim().is_empty() { lines.push(current); }
    lines
}

/// Called only for a direct, interactive GUI command. Uses the actual terminal
/// width, including resized windows. Raw Bash redirections and pipes stay
/// completely byte-for-byte intact.
pub fn decorate_at_width(command: &str, stdout: &str, cols: u16) -> String {
    if !suitable(command, stdout) { return stdout.to_owned(); }
    // Leave a small right margin so the cursor never autowraps a full row.
    let width = (cols as usize).saturating_sub(3).clamp(18, 160);
    let mut result = String::new();
    result.push('\n');
    if let Some(title) = canvas_title(command, width) {
        result.push_str(&title);
        if !result.ends_with('\n') { result.push('\n'); }
    } else {
        result.push_str(&format!("{HEADING}── {command} ──{RESET}\n"));
    }
    for original in stdout.split_inclusive('\n') {
        let line = original.strip_suffix('\n').unwrap_or(original).trim_end_matches('\r');
        if line.trim().is_empty() {
            result.push('\n');
            continue;
        }
        let is_bullet = line.trim_start().starts_with("+ ");
        let fragments = if is_bullet || display_width(line) > width {
            wrap_line(line, width)
        } else {
            vec![line.to_owned()]
        };
        for fragment in fragments {
            result.push_str(&style_line(&fragment));
            result.push('\n');
        }
    }
    result.push('\n');
    result
}

/// Backward-compatible entrypoint for non-GUI callers and unit tests.
#[allow(dead_code)]
pub fn decorate(command: &str, stdout: &str) -> String {
    decorate_at_width(command, stdout, 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts_and_machine_output_remain_unmodified() {
        let text = "STATE   VALUE\nUP      1\n";
        for command in ["notsh interfaces | cat", "notsh interfaces > out.txt",
            "notsh interfaces --json", "echo foo", "notsh interfaces; echo ok",
            "notsh interfaces --plain"] {
            assert_eq!(decorate(command, text), text, "{command}");
        }
    }
    #[test]
    fn preserves_precoloured_terminal_data() {
        let ansi = "\x1b[31mError\x1b[0m\n";
        assert_eq!(decorate("sys info", ansi), ansi);
    }
    #[test]
    fn fields_keep_labels_distinct_from_values() {
        let rendered = style_line("  Estado: DESCONECTADO");
        assert!(rendered.contains(LABEL));
        assert!(rendered.contains(BAD));
        let rendered = style_line("  Estado: CONECTADO");
        assert!(rendered.contains(GOOD));
        assert!(!rendered.contains(BAD));
    }
    #[test]
    fn long_release_notes_wrap_before_terminal_edge() {
        let source = "  + Se instala un motor de informes nativos que consulta las APIs de Windows y produce resultados comprensibles sin perder datos de la conexión ni desbordar la pantalla.";
        let lines = wrap_line(source, 49);
        assert!(lines.len() > 2);
        assert!(lines.iter().all(|line| display_width(line) <= 49), "{lines:?}");
        assert!(lines.concat().contains("informes") && lines.concat().contains("pantalla"));
    }
    #[test]
    fn narrow_viewport_does_not_change_machine_reports() {
        let text = "  Estado: ACTIVO\n  Estado: SIN ENLACE\n";
        let rendered = decorate_at_width("notsh lan show interfaces", text, 42);
        assert!(rendered.contains(GOOD));
        assert!(rendered.contains(BAD));
    }
}
