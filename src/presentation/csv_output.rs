//! Universal --csv output modifier applied at the shell execution boundary.
//! Every captured command uses the same five-column schema, with no special
//! cases or per-command formatting logic.
use crate::support::csv::escape;

pub fn csv_command(line: &str) -> Option<&str> {
    let trimmed = line.trim_end();
    let base = trimmed.strip_suffix("--csv")?;
    if !base.chars().last().is_some_and(char::is_whitespace) { return None; }
    let base = base.trim_end();
    if base.is_empty() { return None; }

    // Do not rewrite shell constructs, quoted strings or scripts. Their
    // argument/stream semantics belong to the Bash engine.
    let mut quote = None;
    let mut escaped = false;
    for ch in base.chars() {
        if escaped { escaped = false; continue; }
        if ch == '\\' && quote != Some('\'') { escaped = true; continue; }
        match (quote, ch) {
            (None, '\'') | (None, '"') => quote = Some(ch),
            (Some(q), c) if q == c => quote = None,
            (None, '|' | '&' | ';' | '<' | '>' | '\r' | '\n') => return None,
            _ => {}
        }
    }
    if quote.is_some() || escaped { None } else { Some(base) }
}

fn clean_ansi(value: &str) -> String {
    let mut clean = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            if ch != '\0' && ch != '\r' { clean.push(ch); }
            continue;
        }
        if chars.next_if_eq(&'[').is_some() {
            // Strip CSI sequences up to their final byte. Cursor motion,
            // colors and erase controls are not printable CSV data.
            for ch in chars.by_ref() {
                if ('@'..='~').contains(&ch) { break; }
            }
        }
    }
    clean
}

fn aligned_fields(value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut token = String::new();
    let mut spaces = 0;
    for ch in value.trim().chars() {
        if ch == ' ' || ch == '\t' {
            spaces += 1;
        } else {
            if spaces >= 2 && !token.is_empty() {
                fields.push(token.trim().to_owned());
                token.clear();
            } else if spaces > 0 && !token.is_empty() {
                token.push(' ');
            }
            spaces = 0;
            token.push(ch);
        }
    }
    if !token.trim().is_empty() { fields.push(token.trim().to_owned()); }
    fields
}

fn record(out: &mut String, kind: &str, section: &str, row: usize, field: &str, value: &str) {
    let values = [kind.to_owned(), section.to_owned(), row.to_string(),
        field.to_owned(), value.to_owned()];
    out.push_str(&values.iter().map(|s| escape(s)).collect::<Vec<_>>().join(","));
    out.push('\n');
}

/// Normalized long-form CSV: every output line or table cell has a stable
/// type, section, row index and field name. No guessing different schemas
/// for commands that happen to print different reports.
pub fn format_output(stdout: &str) -> String {
    let plain = clean_ansi(stdout);
    let mut out = String::from("tipo,seccion,fila,campo,valor\n");
    let mut section = String::new();
    let mut headers: Vec<String> = Vec::new();
    let mut row = 0usize;
    for line in plain.lines() {
        let line = line.trim();
        if line.is_empty() { headers.clear(); continue; }
        if line.chars().all(|ch| ch == '-' || ch == '=' || ch == '─') {
            continue;
        }
        if line.ends_with(':') && line.len() < 100 && !line.contains("://") {
            section = line.trim_end_matches(':').to_owned();
            headers.clear();
            continue;
        }
        row += 1;
        if let Some((key, value)) = line.split_once(": ") {
            if !key.is_empty() && key.len() <= 80 && !key.contains("  ") {
                headers.clear();
                record(&mut out, "campo", &section, row, key.trim(), value.trim());
                continue;
            }
        }
        let fields = aligned_fields(line);
        if fields.len() >= 2 {
            if headers.is_empty() && fields.iter().all(|field| {
                field.chars().any(char::is_alphabetic)
                    && !field.chars().any(char::is_lowercase)
            }) {
                headers = fields;
                continue;
            }
            if headers.len() == fields.len() {
                for (header, value) in headers.iter().zip(fields.iter()) {
                    record(&mut out, "tabla", &section, row, header, value);
                }
                continue;
            }
        }
        headers.clear();
        record(&mut out, "texto", &section, row, "linea", line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_switch_applies_to_any_simple_command() {
        assert_eq!(csv_command("sys info --csv"), Some("sys info"));
        assert_eq!(csv_command("whoami --csv"), Some("whoami"));
        assert_eq!(csv_command("external.exe --csv"), Some("external.exe"));
        assert_eq!(csv_command("sys info | cat --csv"), None);
        assert_eq!(csv_command("echo \"--csv\""), None);
    }

    #[test]
    fn values_quote_commas_quotes_and_line_breaks() {
        let output = format_output("HOST: PC, oficina\nOwner: \"Luis\"\n");
        assert!(output.contains("HOST,\"PC, oficina\""));
        assert!(output.contains("Owner,\"\"\"Luis\"\"\""));
        assert!(!output.contains('\x1b'));
    }

    #[test]
    fn aligned_rows_are_normalized_without_command_specific_code() {
        let output = format_output("NOMBRE    ESTADO\nLAN       ACTIVO\nWAN       INACTIVO\n");
        assert!(output.contains("tabla,,2,NOMBRE,LAN"));
        assert!(output.contains("tabla,,3,ESTADO,INACTIVO"));
    }
}
