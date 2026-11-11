//! Adaptive, accessible rendering for human-facing interactive SST reports.
//! No changes are made to data streams, pipes, redirections, plain/CSV/JSON
//! output, scripts, or terminal applications that already supply ANSI styling.
use libcaca::{Canvas, Color};

const RESET: &str = "\x1b[0m";
// Structural headings are neutral, not cyan. The GUI recognizes this exact
// color as a semantic marker and draws restrained vector outlines at the
// corresponding terminal row, without changing the rest of the text.
const HEADING: &str = "\x1b[1;38;2;213;196;161m";
const COMMAND: &str = "\x1b[1;38;2;254;128;25m";
const DETAILS: &str = "\x1b[38;2;235;219;178m";
const LABEL: &str = "\x1b[1;38;2;189;174;147m";
// Color encodes proven state ONLY: red critical, amber warning,
// green functioning, blue genuinely excellent/optimal.
const GOOD: &str = "\x1b[1;38;2;184;187;38m";
const CAUTION: &str = "\x1b[1;38;2;250;189;47m";
const BAD: &str = "\x1b[1;38;2;251;73;52m";
const PERFECT: &str = "\x1b[1;38;2;131;165;152m";
const BULLET: &str = "\x1b[38;2;189;174;147m";


fn is_machine_or_shell_command(command: &str) -> bool {
    // Only the human-facing foreground presentation is styled. Pipelines,
    // redirects, scripts and machine-readable modes preserve their bytes.
    command.chars().any(|ch| "|;&><\n\r".contains(ch))
        || command.split_whitespace().any(|part| matches!(
            part, "--json" | "--csv" | "--plain" | "--stdout" | "--xml" | "-j"
        ))
        || crate::builtins::UNIX_COMMANDS.iter().any(|(name, _)| {
            command.split_whitespace().next() == Some(*name)
        })
}

fn is_table_header(line: &str) -> bool {
    let trimmed = line.trim();
    let columns = trimmed.split_whitespace().collect::<Vec<_>>();
    columns.len() >= 2 && trimmed.len() >= 5
        && columns.iter().all(|column| {
            column.chars().any(char::is_alphabetic)
                && !column.chars().any(char::is_lowercase)
        })
        && !trimmed.contains("://")
        && !trimmed.contains(':')
}

fn table_columns(line: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut columns = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() { pos += 1; }
        if pos >= bytes.len() { break; }
        let start = pos;
        while pos < bytes.len() && !(bytes[pos] == b' ' && bytes.get(pos + 1) == Some(&b' ')) {
            pos += 1;
        }
        let end = pos;
        columns.push((start, end));
    }
    columns
}

fn style_table_row(line: &str, status_column: Option<usize>) -> String {
    let Some(status_column) = status_column else { return line.to_owned(); };
    let columns = table_columns(line);
    let Some(&(start, end)) = columns.get(status_column) else { return line.to_owned(); };
    let value = line.get(start..end).unwrap_or("");
    let Some(color) = status_color(value) else { return line.to_owned(); };
    format!("{}{color}{value}{RESET}{}", &line[..start], &line[end..])
}

fn status_column_index(heading: &str) -> Option<usize> {
    table_columns(heading).iter().enumerate().find_map(|(index, &(start, end))| {
        let field = heading.get(start..end)?.trim().to_ascii_uppercase();
        matches!(field.as_str(), "ESTADO" | "STATUS" | "SALUD" | "HEALTH" | "RESULTADO")
            .then_some(index)
    })
}

fn suitable(command: &str, stdout: &str, registered_native: Option<&[String]>) -> bool {
    if stdout.trim().is_empty() || stdout.contains('\x1b') || stdout.contains('\0')
        || is_machine_or_shell_command(command)
    { return false; }

    // One shared presentation path for all foreground, human-readable output.
    // Regular unstructured shell text (echo, printf, cat, etc.) stays raw;
    // every structured native OR external report receives semantic formatting.
    let root = command.split_whitespace().next().unwrap_or("");
    // File viewers and UNIX text tools must never rewrite the data they show,
    // even when a file happens to contain words such as "Estado: ERROR".
    if crate::builtins::UNIX_COMMANDS.iter().any(|(name, _)| *name == root) {
        return false;
    }
    let native = registered_native.is_some_and(|names| names.iter().any(|name| name == root));
    native || stdout.lines().any(|line| {
        is_section_heading(line) || is_status_field(line)
            || looks_like_tabular_header(line) || is_table_header(line)
            || line.trim().split_once(": ").is_some_and(|(label, _)| {
                !label.is_empty() && label.len() < 42
            })
    })
}

fn is_status_field(line: &str) -> bool {
    let Some((key, _)) = line.trim().split_once(": ") else { return false; };
    matches!(key.to_ascii_lowercase().as_str(),
        "estado" | "status" | "salud" | "health" | "resultado" | "result")
}

fn looks_like_tabular_header(line: &str) -> bool {
    let text = line.trim();
    text.len() > 7 && text.split_whitespace().count() >= 3
        && text.chars().any(|c| c.is_ascii_uppercase())
        && !text.chars().any(|c| c.is_ascii_lowercase())
        && !text.contains("://")
}

/// Compose a compact, readable heading with libcaca's text canvas.
/// No ASCII boxes, fake ellipses, hidden RGB markers, or compositor changes.
fn canvas_title(command: &str, width: usize) -> Option<String> {
    if width < 20 { return None; }
    let name = command.chars().take(width.saturating_sub(5)).collect::<String>();
    let mut canvas = Canvas::new((name.chars().count() + 3) as i32, 1).ok()?;
    canvas.set_color_ansi(Color::LightGray, Color::Default).ok()?;
    canvas.put_str(1, 0, &name);
    let utf8 = String::from_utf8(canvas.export_to_memory("utf8").ok()?).ok()?;
    Some(format!("{HEADING}{}{RESET}", utf8.trim_end_matches(['\r', '\n'])))
}

fn status_color(value: &str) -> Option<&'static str> {
    // Never infer a status from incidental words in help descriptions,
    // hostnames, commands, process names or arbitrary prose.
    let value = value.trim().trim_end_matches(['.', ';']);
    if value.len() > 55 || value.split_whitespace().count() > 5 {
        return None;
    }
    let normalized = value.to_uppercase();
    let status = normalized.as_str();
    if ["CRÍTICO", "CRITICO", "ERROR", "FALLO", "FAILED",
        "DENEGADO", "DESCONECTADO", "SIN ENLACE",
        "NO DISPONIBLE", "DETENIDO", "DESHABILITADO",
        "INACTIVO", "BLOQUEADO", "DOWN", "OFFLINE"]
        .iter().any(|term| status == *term || status.starts_with(&format!("{term} ")))
    {
        Some(BAD)
    } else if ["ADVERTENCIA", "ATENCIÓN", "ATENCION",
        "PENDIENTE", "DEGRADADO", "INDETERMINADO",
        "SIN IP", "DESCONOCIDO", "WARNING"]
        .iter().any(|term| status == *term || status.starts_with(&format!("{term} ")))
    {
        Some(CAUTION)
    } else if ["PERFECTO", "PERFECTA", "ÓPTIMO", "OPTIMO",
        "ÓPTIMA", "OPTIMA", "EXCELENTE"]
        .contains(&status)
    {
        Some(PERFECT)
    } else if ["CONECTADO", "CONECTADA", "CORRECTO",
        "CORRECTA", "ACTIVO", "ACTIVA", "OPERATIVO",
        "OPERATIVA", "DISPONIBLE", "OK", "UP"]
        .contains(&status)
    {
        Some(GOOD)
    } else { None }
}

/// Recognize actual structural sections in a native SST report. We do not
/// infer "a section" from every sentence containing a colon.
fn is_section_heading(line: &str) -> bool {
    let text = line.trim();
    if text.is_empty() || text.chars().count() > 72 { return false; }
    if text.ends_with(':') && !text.contains("://") {
        let name = text.trim_end_matches(':').trim().to_ascii_lowercase();
        // A status key without a value is NOT a decorative section.
        if matches!(name.as_str(),
            "estado" | "status" | "salud" | "resultado" |
            "ip" | "mac" | "nombre" | "puerto" | "servidor" |
            "error" | "aviso" | "advertencia" | "latencia")
        { return false; }
        return text.chars().any(char::is_alphabetic)
            && text.chars().filter(|ch| *ch == ':').count() == 1;
    }
    if text.starts_with("== ") { return true; }
    looks_like_tabular_header(text)
}

// Help syntax is identified structurally, not by a hard-coded list of commands.
// New builtins receive the same formatting without editing this renderer.
fn style_command_row(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.contains(':') || trimmed.contains('=') {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let split = (1..bytes.len().saturating_sub(1))
        .find(|&i| bytes[i] == b' ' && bytes[i + 1] == b' '
            && bytes[..i].iter().any(|b| !b.is_ascii_whitespace()))?;
    let syntax = trimmed[..split].trim_end();
    let description = trimmed[split..].trim_start();
    if syntax.is_empty() || description.is_empty() || syntax.chars().count() > 60 {
        return None;
    }
    let indent = &line[..line.len() - trimmed.len()];
    Some(format!("{indent}  {COMMAND}{syntax}{RESET}  {DETAILS}{description}{RESET}"))
}

/// Generic two-column help entries used by builtins that omit the command
/// prefix on each line. This acts on human-facing help only, never data tables.
fn style_help_entry(line: &str) -> Option<String> {
    let text = line.trim_start();
    if text.is_empty() || text.starts_with('#') || text.contains('\x1b') {
        return None;
    }
    let bytes = text.as_bytes();
    let split = (1..bytes.len().saturating_sub(1))
        .find(|&i| bytes[i] == b' ' && bytes[i + 1] == b' ');
    let index = split?;
    let syntax = text[..index].trim();
    let description = text[index..].trim();
    if syntax.is_empty() || description.is_empty() || syntax.chars().count() > 45 {
        return None;
    }
    let indent = &line[..line.len() - text.len()];
    let padding = " ".repeat(34usize.saturating_sub(display_width(syntax)).max(2));
    Some(format!("{indent}  {COMMAND}{syntax}{RESET}{padding}{DETAILS}{description}{RESET}"))
}

fn style_line(line: &str) -> String {
    if line.trim().is_empty() { return line.to_owned(); }
    let stripped = line.trim_start();
    if is_section_heading(stripped) {
        // This RGB marker is rendered as a native Slint vector heading band.
        // The indentation gives the rounded border room without covering text.
        return format!("{HEADING}   {stripped}{RESET}");
    }
    if let Some((field, value)) = stripped.split_once(": ") {
        // Semantic labels and values. Do not treat a command description as a
        // status, even if it happens to contain "conectados" or "disponible".
        if !field.is_empty() && field.chars().count() <= 40
           && !field.contains("  ") && field.chars().any(char::is_alphabetic)
        {
            let indent = &line[..line.len() - stripped.len()];
            let key = field.to_ascii_lowercase();
            // Explicit diagnostic labels are authoritative; arbitrary
            // occurrences of these words in descriptions are not.
            let alarm = matches!(key.as_str(),
                "error" | "error crítico" | "error critico" | "fallo" | "crítico" | "critico");
            let warning = matches!(key.as_str(),
                "advertencia" | "aviso" | "warning" | "atención" | "atencion");
            let label_color = if alarm { BAD } else if warning { CAUTION } else { LABEL };
            let value_color = if alarm { BAD }
                else if warning { CAUTION }
                else if is_status_field(stripped) { status_color(value).unwrap_or(RESET) }
                else { RESET };
            return format!("{indent}{label_color}{field}:{RESET} {value_color}{value}{RESET}");
        }
    }
    if stripped.starts_with('•') {
        let indent = &line[..line.len() - stripped.len()];
        return format!("{indent}{BULLET}•{RESET} {}", stripped.trim_start_matches('•').trim_start());
    }
    // A bare word in arbitrary output is not a verified machine state.
    line.to_owned()
}

/// Shared presentation for incremental external-process output. Each complete
/// line is formatted in place; no extra titles or blank lines get inserted.
/// Data streams are untouched because only direct interactive GUI output uses
/// this path. Existing ANSI, CR progress bars and NUL bytes are preserved.
pub fn style_live_line(line: &str) -> String {
    if line.contains('\x1b') || line.contains('\r') || line.contains('\0') {
        return line.to_owned();
    }
    style_line(line)
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
    decorate_with_native_commands(command, stdout, cols, None)
}

/// Shared human-facing report renderer for every SST-owned built-in command,
/// including aliases added to the registry. Only machine output and external
/// commands remain byte-for-byte untouched.
pub fn decorate_registered_at_width(
    command: &str,
    stdout: &str,
    cols: u16,
    registered_native: &[String],
) -> String {
    decorate_with_native_commands(command, stdout, cols, Some(registered_native))
}

fn decorate_with_native_commands(
    command: &str, stdout: &str, cols: u16,
    registered_native: Option<&[String]>,
) -> String {
    if !suitable(command, stdout, registered_native) { return stdout.to_owned(); }
    // Leave a small right margin so the cursor never autowraps a full row.
    let width = (cols as usize).saturating_sub(3).clamp(18, 160);
    let mut result = String::new();
    let help_document = command.split_whitespace().any(|part|
        matches!(part, "help" | "--help" | "-h"))
        || stdout.lines().take(6).any(|line| line.trim_start().starts_with("uso:"));
    result.push('\n');
    if let Some(title) = canvas_title(command, width) {
        result.push_str(&title);
        if !result.ends_with('\n') { result.push('\n'); }
    } else {
        result.push_str(&format!("{HEADING}   {command}{RESET}\n"));
    }
    let mut status_column: Option<usize> = None;
    let mut first_nonempty = true;
    for original in stdout.split_inclusive('\n') {
        let line = original.strip_suffix('\n').unwrap_or(original).trim_end_matches('\r');
        if !line.trim().is_empty() && first_nonempty {
            first_nonempty = false;
            if line.trim().eq_ignore_ascii_case(command.trim()) { continue; }
        }
        if is_table_header(line) {
            status_column = status_column_index(line);
        } else if line.trim().is_empty() || is_section_heading(line) {
            status_column = None;
        }
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
            let styled = if help_document && is_section_heading(&fragment) {
                style_line(&fragment)
            } else if help_document {
                style_command_row(&fragment)
                    .or_else(|| style_help_entry(&fragment))
                    .unwrap_or_else(|| style_line(&fragment))
            } else if is_table_header(&fragment) {
                format!("{HEADING}{fragment}{RESET}")
            } else {
                style_table_row(&style_line(&fragment), status_column)
            };
            result.push_str(&styled);
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
    fn future_command_help_is_structurally_styled() {
        let formatted = decorate_at_width(
            "futuretool help",
            "uso: futuretool help\nfuturetool inspect     Consulta el sistema\n",
            100,
        );
        assert!(formatted.contains(COMMAND));
        assert!(formatted.contains("futuretool inspect"));
    }

    #[test]
    fn generic_help_has_distinct_syntax_descriptions_and_no_duplicate_title() {
        let styled = decorate_at_width(
            "invented help",
            "invented help\nuso:\ninvented scan    Detecta equipos\n", 110,
        );
        assert_eq!(styled.matches("invented help").count(), 1);
        assert!(styled.contains(COMMAND));
        assert!(styled.contains(DETAILS));
        assert!(styled.contains("Detecta equipos"));
    }

    #[test]
    fn unknown_commands_get_the_same_layout_without_command_specific_rules() {
        let output = "REDES:\nNombre: Ethernet\nEstado: OPERATIVO\n";
        let styled = decorate_at_width("futura-herramienta", output, 100);
        assert!(styled.contains(HEADING));
        assert!(styled.contains(LABEL));
        assert!(styled.contains(GOOD));
        assert_eq!(decorate_at_width("futura-herramienta --csv", output, 100), output);
    }

    #[test]
    fn table_status_color_is_column_scoped() {
        let title = "NOMBRE    ESTADO    DETALLE";
        assert!(is_table_header(title));
        assert_eq!(status_column_index(title), Some(1));
        let row = "Red A     ACTIVO    enlace conectado";
        assert!(style_table_row(row, Some(1)).contains(GOOD));
        let description = "Red A     desconocido    conectado";
        assert!(!style_table_row(description, Some(1)).contains(GOOD));
    }

    #[test]
    fn all_native_registry_commands_share_consistent_human_report_design() {
        let names = vec!["new-native-command".to_owned(), "sys".to_owned()];
        let output = "Estado: OPERATIVA\nDetalles:\nClave: valor\n";
        let formatted = decorate_registered_at_width("new-native-command", output, 95, &names);
        assert!(formatted.contains(HEADING));
        assert!(formatted.contains(GOOD));
        assert!(formatted.contains("Detalles:"));
        assert_eq!(
            decorate_registered_at_width("new-native-command --csv", output, 95, &names),
            output,
        );
        assert_eq!(
            decorate_registered_at_width("new-native-command | cat", output, 95, &names),
            output,
        );
    }
    #[test]
    fn neutral_structural_text_never_uses_cyan_without_a_real_state() {
        let help = decorate_at_width(
            "sys", "Información:\nsys drivers --devices    dispositivos PnP conectados\n", 95
        );
        assert!(help.contains(HEADING));
        assert!(help.contains(COMMAND));
        assert!(!help.contains(PERFECT));
        assert!(!help.contains(GOOD));
        assert!(!help.contains("\\x1b[38;2;152;222;247m"));
    }

    #[test]
    fn semantic_colors_require_verified_status_fields() {
        assert!(style_line("Estado: CRÍTICO").contains(BAD));
        assert!(style_line("Estado: ADVERTENCIA").contains(CAUTION));
        assert!(style_line("Estado: OPERATIVA").contains(GOOD));
        assert!(style_line("Estado: PERFECTO").contains(PERFECT));
        assert!(!style_line("Descripción: dispositivos PnP conectados").contains(GOOD));
        assert!(!style_line("Texto: ERROR en un manual").contains(BAD));
        assert!(style_line("Error: acceso rechazado").contains(BAD));
        assert!(style_line("Advertencia: revisar conexión").contains(CAUTION));
        assert!(!style_live_line("Respuesta desde 10.0.0.1: bytes=32 tiempo=1ms TTL=64").contains(PERFECT));
    }

    #[test]
    fn scripts_and_machine_output_remain_unmodified() {
        let text = "STATE   VALUE\nUP      1\n";
        for command in ["notsh interfaces | cat", "notsh interfaces > out.txt",
            "notsh interfaces --json", "notsh interfaces --csv", "echo foo", "notsh interfaces; echo ok",
            "notsh interfaces --plain", "cat report.txt", "grep error report.txt"] {
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
    fn native_title_is_not_a_dash_drawing() {
        let title = canvas_title("notsh lan show interfaces", 80).unwrap();
        assert!(title.contains("notsh lan show interfaces"));
        assert!(!title.contains("\x1b[48;2;9;47;64m"));
        assert!(!title.contains("---"));
    }
    #[test]
    fn help_sections_are_semantic_and_command_descriptions_are_not_statuses() {
        let out = decorate_at_width(
            "sys",
            "uso: sys SUBCOMANDO [opciones]\n\nInformación:\nsys info           resumen del equipo\nsys drivers --devices     dispositivos PnP conectados\n\nAdministración / auditoría de solo lectura:\n",
            110,
        );
        assert!(out.contains("Información:"));
        assert!(out.contains("Administración / auditoría"));
        assert!(out.contains(HEADING));
        assert!(out.contains(COMMAND));
        assert!(!style_line("sys drivers --devices     dispositivos PnP conectados").contains(GOOD));
    }
    #[test]
    fn preserves_fetch_logo_and_script_pipelines_as_is() {
        let fetch = "SST ASCII mascot content";
        assert_eq!(decorate_at_width("fetch", fetch, 80), fetch);
        assert_eq!(decorate_at_width("sys | cat", fetch, 80), fetch);
    }
    #[test]
    fn narrow_viewport_does_not_change_machine_reports() {
        let text = "  Estado: ACTIVO\n  Estado: SIN ENLACE\n";
        let rendered = decorate_at_width("notsh lan show interfaces", text, 42);
        assert!(rendered.contains(GOOD));
        assert!(rendered.contains(BAD));
    }
}
