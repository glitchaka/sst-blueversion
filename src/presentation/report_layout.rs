//! Terminal-cell layout for foreground output. ANSI surfaces travel with the
//! cells through scrollback, selection, and the native and console renderers.
use super::*;

const RAIL: &str = "\x1b[48;2;254;128;25m";
const GUTTER: &str = "\x1b[48;2;43;48;48m";
const HEADER: &str = "\x1b[48;2;63;52;38m";

pub(super) fn underline(text: &str) -> bool {
    let mut chars = text.trim().chars();
    let Some(first) = chars.next() else { return false; };
    matches!(first, '-' | '=' | '─' | '━') && text.trim().chars().count() >= 3
        && chars.all(|ch| ch == first)
}

fn field(text: &str) -> Option<(&str, &str)> {
    let (label, value) = text.trim_start().split_once(':')?;
    if label.is_empty() || label.len() > 44 || !label.chars().any(char::is_alphabetic)
        || label.contains("  ") || label.contains(['/', '\\'])
        || !(value.is_empty() || value.starts_with(char::is_whitespace)) { return None; }
    Some((label,value.trim()))
}

fn band(text: &str, plain: &str, width: usize, surface: &str, rail: &str) -> String {
    let padding = width.saturating_sub(display_width(plain) + 3);
    format!("{rail} {surface}  {}{surface}{}{RESET}",text.replace(RESET,&format!("{RESET}{surface}")), " ".repeat(padding))
}

pub(super) fn fields(rows: &[&str], styles: &[&str], width: usize, out: &mut Vec<String>) {
    let gutter = rows.iter().filter_map(|r|field(r)).map(|(k,_)|display_width(k)+1).max().unwrap_or(1);
    let stacked = width < gutter + 18;
    for (i,row) in rows.iter().enumerate() {
        let (label,value) = field(row).unwrap();
        let surface = if i % 2 == 0 { SURFACE } else { SURFACE_ALT };
        let normalized = format!("{label}: {value}");
        let styled = style_line(&normalized);
        let value_tone = styled.split_once(&format!(":{RESET} ")).and_then(|(_,v)|v.split_once('m'))
            .filter(|(prefix,_)|prefix.starts_with("\x1b[")).map(|(prefix,_)|format!("{prefix}m")).unwrap_or_else(||VALUE.into());
        let label_tone = styles[i].trim_start().strip_prefix(BAD).map(|_|BAD).or_else(||styled.strip_prefix(CAUTION).map(|_|CAUTION)).unwrap_or(LABEL);
        let padded = format!("{label}:{}", " ".repeat(gutter-display_width(label)-1));
        if stacked { out.push(band(&format!("{label_tone}{label}:{RESET}"),&format!("{label}:"),width,GUTTER,RAIL)); }
        let indent = if stacked { 3 } else { gutter + 5 };
        let value = if value.is_empty() { "—" } else { value };
        for (part,fragment) in wrap_line(value,width.saturating_sub(indent).max(12)).iter().enumerate() {
            let prefix = if stacked { format!("{surface}   ") } else if part == 0 {
                format!("{GUTTER}  {label_tone}{padded}{RESET}{GUTTER} {surface}  ")
            } else { format!("{GUTTER}{}{surface}  "," ".repeat(gutter+3)) };
            let remaining = width.saturating_sub(indent+display_width(fragment));
            let gauge = if part == 0 && fragment == value { percent_metric(label,value).filter(|_|remaining >= 10).map_or_else(String::new,|(metric,n)|format!("  {}",super::meter(metric,n,remaining.min(16)-2))) } else { String::new() };
            let gauge_width = if gauge.is_empty(){0}else{remaining.min(16)};
            out.push(format!("{prefix}{value_tone}{fragment}{RESET}{surface}{gauge}{surface}{}{RESET}", " ".repeat(remaining-gauge_width)));
        }
    }
}

pub(super) fn render(styled: &str, width: usize, structured: bool) -> String {
    if styled.is_empty() || styled.contains(['\0','\r']) { return styled.into(); }
    let lines: Vec<_> = styled.lines().collect();
    let clean: Vec<_> = lines.iter().map(|s|report_text(s)).collect();
    if clean.iter().any(|s|s.contains('\x1b')) { return styled.into(); }
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = clean[i].as_ref();
        let next_rule = structured && !line.trim().is_empty() && clean.get(i+1).is_some_and(|l|underline(l));
        if lines[i].starts_with(GUTTER) || lines[i].starts_with(SURFACE) || lines[i].starts_with(SURFACE_ALT) || lines[i].starts_with(TABLE_HEADER) || lines[i].starts_with(TABLE_BAND) {
            out.push(lines[i].to_owned());
            i+=1;
            continue;
        }
        if next_rule || (structured && lines[i].trim_start().starts_with(HEADING)) {
            if !is_table_header(line) || next_rule {
                out.push(band("", "", width,HEADER,RAIL));
            }
            for part in wrap_line(line.trim(),width.saturating_sub(3)) {
                out.push(band(&Role::Title.paint(&part),&part,width,HEADER,RAIL));
            }
            i += if next_rule { 2 } else { 1 };
        } else if structured && field(line).is_some() {
            let start = i;
            i += 1;
            while i < lines.len() && field(&clean[i]).is_some() && !lines[i].trim_start().starts_with(HEADING) { i += 1; }
            let group: Vec<_> = clean[start..i].iter().map(|s|s.as_ref()).collect();
            fields(&group,&lines[start..i],width,&mut out);
        } else {
            if line.is_empty() { out.push(String::new()); }
            else if display_width(line) <= width.saturating_sub(3) {
                let surface = if i % 2 == 0 { SURFACE } else { SURFACE_ALT };
                out.push(band(lines[i],line,width,surface,GUTTER));
            } else { out.push(lines[i].to_owned()); }
            i += 1;
        }
    }
    let mut result = out.join("\n");
    if styled.ends_with('\n') { result.push('\n'); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_reports_receive_the_same_layout_as_the_printer_report() {
        let input = "Impresora\n------------------------------------------------------------\nNombre:    Equipo\nPredeterminada: sí\nServidor:  Central\n";
        for cmd in ["sys printer", "inventario", "otro comando"] {
            let out = Formatter::result(cmd,input,"",55,&[],OutputMode::Interactive).stdout;
            assert!(out.contains(HEADER) && out.contains(GUTTER));
            assert!(!out.contains("-----"));
            let plain = report_text(&out);
            let position = |word:&str|plain.lines().find_map(|l|l.find(word).map(|i|l[..i].chars().count())).unwrap();
            assert_eq!(position("Equipo"),position("Central"));
        }
    }
}
