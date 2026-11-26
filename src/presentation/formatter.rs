//! Central SST output formatter. The shell keeps original data; only the
//! presentation boundary chooses the human theme, machine passthrough or
//! terminal-control passthrough. No command owns a visual palette.

#[path = "report_layout.rs"]
mod report_layout;

pub const RESET: &str = "\x1b[0m";

// Generate the terminal escape and native renderer color from the same RGB.
macro_rules! tone {
    ($ansi:ident, $rgb:ident, $weight:literal, $r:literal, $g:literal, $b:literal) => {
        pub const $rgb: (u8, u8, u8) = ($r, $g, $b);
        pub const $ansi: &str = concat!("\x1b[", $weight, "38;2;", stringify!($r),
            ";", stringify!($g), ";", stringify!($b), "m");
    };
}
tone!(HEADING, TITLE_RGB, "1;", 250, 189, 47);
tone!(COMMAND, COMMAND_RGB, "1;", 254, 128, 25);
tone!(DETAILS, DETAIL_RGB, "", 235, 219, 178);
tone!(LABEL, LABEL_RGB, "", 131, 165, 152);
tone!(VALUE, DEFAULT_FOREGROUND, "", 184, 187, 38);
tone!(GOOD, SUCCESS_RGB, "1;", 142, 192, 124);
tone!(CAUTION, WARNING_RGB, "1;", 255, 163, 61);
tone!(BAD, FAILURE_RGB, "1;", 251, 73, 52);
tone!(PERFECT, OPTIMAL_RGB, "1;", 131, 165, 205);
const BULLET: &str = LABEL;
pub const DEFAULT_FOREGROUND_HEX: &str = "#B8BB26";
pub const REPORT_HEADER_RGB: (u8,u8,u8) = (63,52,38);
// Separate semantic surfaces identify the reserved display-size value region.
pub const FEATURE_RGB: (u8,u8,u8) = (31,34,32);
pub const FEATURE_ALT_RGB: (u8,u8,u8) = (35,38,35);

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Role { Title, Command, Label, Detail, Value, Success, Warning, Failure, Optimal }
#[derive(Clone,Copy,Debug)]
pub struct TextStyle { pub rgb:(u8,u8,u8),pub scale:f32,pub bold:bool,pub section:bool }
impl Role {
    pub const fn style(self)->TextStyle {
        let (rgb,scale,bold,section)=match self {
            Self::Title=>(TITLE_RGB,1.08,true,true),
            Self::Command=>(COMMAND_RGB,1.0,true,false),
            Self::Label=>(LABEL_RGB,0.90,false,false),
            Self::Detail=>(DETAIL_RGB,0.93,false,false),
            Self::Value=>(DEFAULT_FOREGROUND,1.0,false,false),
            Self::Success=>(SUCCESS_RGB,1.0,true,false),
            Self::Warning=>(WARNING_RGB,1.0,true,false),
            Self::Failure=>(FAILURE_RGB,1.0,true,false),
            Self::Optimal=>(OPTIMAL_RGB,1.0,true,false),
        };
        TextStyle{rgb,scale,bold,section}
    }
    pub const fn ansi(self)->&'static str {
        match self{
            Self::Title=>HEADING,Self::Command=>COMMAND,Self::Label=>LABEL,Self::Detail=>DETAILS,
            Self::Value=>VALUE,Self::Success=>GOOD,Self::Warning=>CAUTION,Self::Failure=>BAD,Self::Optimal=>PERFECT,
        }
    }
    pub fn from_rgb(rgb:(u8,u8,u8))->Option<Self>{
        [Self::Title,Self::Command,Self::Label,Self::Detail,Self::Value,Self::Success,Self::Warning,Self::Failure,Self::Optimal]
            .into_iter().find(|r|r.style().rgb==rgb)
    }
    pub fn paint(self,text:&str)->String{format!("{}{text}{RESET}",self.ansi())}
}
/// Interaction surfaces are shared by the native viewport and VT monitors.
pub const SELECTED_RGB: (u8, u8, u8) = (21, 62, 58);
pub const SURFACE: &str = "\x1b[48;2;29;32;33m";
pub const SURFACE_ALT: &str = "\x1b[48;2;40;40;40m";
pub const SURFACE_HEAD: &str = "\x1b[48;2;60;56;54m";
pub fn selected_surface() -> String {
    let (r,g,b) = SELECTED_RGB;
    format!("\x1b[48;2;{r};{g};{b}m")
}

/// One cycle per 1.6 seconds. A focused caret always remains visible.
pub fn caret_color(step: u8) -> (u8,u8,u8) {
    let level = 8 - (step % 16).abs_diff(8);
    let mix = |a:u8,b:u8| ((u16::from(a)*(8-u16::from(level)) + u16::from(b)*u16::from(level))/8) as u8;
    (mix(COMMAND_RGB.0,TITLE_RGB.0),mix(COMMAND_RGB.1,TITLE_RGB.1),mix(COMMAND_RGB.2,TITLE_RGB.2))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric { Cpu, Utilization, Loss }
impl Metric {
    fn named(label:&str)->Option<Self>{
        match label.trim().to_lowercase().as_str(){
            "cpu"|"cpu%"|"cpu %"|"cpu global"|"uso de cpu"|"cpu uso" => Some(Self::Cpu),
            "ram"|"ram %"|"memoria usada"|"uso de memoria"|"disk usage"|"uso de disco" => Some(Self::Utilization),
            "pérdida"|"perdida"|"packet loss"|"pérdida de paquetes" => Some(Self::Loss),
            _ => None,
        }
    }

    pub fn role(self, value:f32, previous:Option<f32>)->Role{
        if !value.is_finite() || value < 0.0 { return Role::Value; }
        let (warning, critical) = match self {
            Self::Cpu => (25.0,50.0),
            Self::Utilization => (75.0,90.0),
            Self::Loss => (0.1,5.0),
        };
        if value >= critical || (self == Self::Cpu && previous.is_some_and(|old|value-old >= 25.0)) { Role::Failure }
        else if value >= warning { Role::Warning } else { Role::Value }
    }
    fn color(self, text:&str, implicit_percent:bool)->Option<&'static str>{
        let value = text.trim();
        let number = value.strip_suffix('%').or_else(|| implicit_percent.then_some(value))?;
        Some(self.role(number.trim().replace(',',".").parse().ok()?,None).ansi())
    }
}

/// Typography uses the same monospace grid for all output. Larger headings
/// and smaller secondary text may vary within the cell bounds, never overlap.
pub fn font_pixels(role:Option<Role>,base:f32,cell_width:f32,cell_height:f32)->f32{
    let scale=role.map_or(1.,|r|r.style().scale);
    (base*scale).min(cell_width/0.60).min(cell_height*0.91).max(1.0)
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum OutputMode { Interactive, Machine, Terminal }
#[derive(Debug,PartialEq,Eq)]
pub struct FormattedOutput { pub stdout:String,pub stderr:String }

/// Remove legacy styling from a human report. A cursor, OSC or other terminal
/// protocol makes the entire input opaque so stripping cannot corrupt it.
fn report_text(text: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if !text.contains('\x1b') { return Cow::Borrowed(text); }
    static SGR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let sgr = SGR.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;:]*m").unwrap());
    let clean = sgr.replace_all(text, "");
    if clean.contains('\x1b') { Cow::Borrowed(text) } else { clean }
}

/// Explicit presentation contract for complete command results. Machine
/// data and terminal protocols go through this same entrypoint unmodified.
pub struct Formatter;
impl Formatter {
    pub fn mode(command:&str,interactive:bool)->OutputMode{
        if !interactive || is_machine_or_shell_command(command){return OutputMode::Machine;}
        let root=command.split_whitespace().next().unwrap_or("");
        if matches!(root,"fetch"|"neofetch"|"fastfetch"|"keygen"|"helix"|"helix-sst"|"clear"|"reset"){
            return OutputMode::Terminal;
        }
        OutputMode::Interactive
    }
    #[cfg(test)]
    pub fn result(_command:&str,stdout:&str,stderr:&str,cols:u16,_native:&[String],mode:OutputMode)->FormattedOutput{
        Self::present(stdout,stderr,cols,mode)
    }
    /// The production presentation API accepts content, width and transport only.
    pub fn present(stdout:&str,stderr:&str,cols:u16,mode:OutputMode)->FormattedOutput{
        if mode!=OutputMode::Interactive{
            return FormattedOutput{stdout:stdout.to_owned(),stderr:stderr.to_owned()};
        }
        let width = usize::from(cols.saturating_sub(3)).clamp(18,160);
        let clean = report_text(stdout);
        // Native Windows tools normally emit CRLF. Normalize only line endings,
        // never a standalone carriage return used by an interactive protocol.
        let clean = if clean.contains(['\x1b', '\0', '\u{8}']) { clean } else if clean.contains("\r\n") {
            std::borrow::Cow::Owned(clean.replace("\r\n", "\n"))
        } else { clean };
        let kind=classify(&clean);
        let text = compose_text(&clean,cols,kind);
        FormattedOutput {
            stdout:if kind==LayoutKind::Reference{text}else{report_layout::render(&text,width,kind!=LayoutKind::Plain)},
            stderr:report_layout::render(&Self::diagnostic(&stderr.replace("\r\n","\n")),width,true),
        }
    }
    /// Only completed human-readable output can acquire a reading anchor.
    /// Streams, raw screen applications and machine bytes retain VT behavior.
    pub fn readable_result(result:&FormattedOutput,mode:OutputMode)->bool{
        mode==OutputMode::Interactive
            && (!result.stdout.trim().is_empty()||!result.stderr.trim().is_empty())
            && [&result.stdout,&result.stderr].iter().all(|s|{
                let clean=report_text(s);
                !clean.contains(['\x1b','\0','\u{8}']) && !clean.replace("\r\n","\n").contains('\r')
            })
    }
    /// Estimate row advances only when the finite VT history is saturated.
    /// Normally the actual scrollback growth determines the viewport anchor.
    pub fn visual_row_advances(bytes:&[u8],cols:u16)->usize{
        let raw=String::from_utf8_lossy(bytes);
        let clean=report_text(&raw);
        let cols=usize::from(cols.max(1));
        clean.split_inclusive('\n').map(|line|{
            let body=line.trim_end_matches(['\r','\n']);
            display_width(body).saturating_sub(1)/cols+usize::from(line.ends_with('\n'))
        }).sum()
    }
    pub fn diagnostic(text:&str)->String{
        let clean = report_text(text);
        let text = clean.as_ref();
        if text.is_empty()||text.contains('\x1b')||text.contains('\0')||text.contains('\r'){return text.to_owned();}
        let mut out=String::new();
        for line in text.split_inclusive('\n'){
            let body=line.strip_suffix('\n').unwrap_or(line);
            if body.is_empty(){out.push_str(line);continue;}
            // Preserve diagnostic text; only the first label/short line uses
            // the failure role. No extra headings or invented exit status.
            let styled=if let Some((label,value))=body.split_once(": "){
                if label.chars().count()<=40{format!("{}: {value}",Role::Failure.paint(label))}
                else{Role::Failure.paint(body)}
            }else{Role::Failure.paint(body)};
            out.push_str(&styled);if line.ends_with('\n'){out.push('\n');}
        }
        out
    }
    pub fn live(text:&str)->String{style_live_line(&report_text(text))}
    pub fn live_layout(styled:&str,_cols:u16)->String {
        // Streaming lines retain semantic roles without reserving extra rows.
        styled.to_owned()
    }
    pub fn terminal(text:&str)->String{text.to_owned()}
    pub fn stream_bytes(bytes:&[u8],terminal:bool)->Vec<u8>{
        // Binary/partial UTF-8, ANSI protocols and async chunks are passthrough.
        // The complete-line relay opts into semantic styling only after framing.
        if terminal{return bytes.to_vec();}
        std::str::from_utf8(bytes).map_or_else(|_|bytes.to_vec(),|s|Self::live(s).into_bytes())
    }
}

/// Explicit semantic document API for new native reports. The legacy text
/// adapter below remains available during migration; all visual roles and
/// the rendering of both kinds live here, not in command implementations.
#[derive(Default)]
pub struct Document { rows:Vec<(Role,String)> }
impl Document{
    pub fn title(&mut self,text:impl Into<String>){self.rows.push((Role::Title,text.into()));}
    pub fn line(&mut self,role:Role,text:impl Into<String>){self.rows.push((role,text.into()));}
    pub fn field(&mut self,name:&str,value:&str){
        self.line(Role::Value,format!("{name}: {value}"));
    }
    pub fn render(&self,mode:OutputMode)->String{
        let mut out=String::new();
        for (role,text) in &self.rows{
            if mode==OutputMode::Interactive{
                if *role==Role::Value{out.push_str(&style_line(text));}
                else{out.push_str(&role.paint(text));}
            }else{out.push_str(text);}
            out.push('\n');
        }
        out
    }
}

fn is_machine_or_shell_command(command: &str) -> bool {
    // Only the human-facing foreground presentation is styled. Pipelines,
    // redirects, scripts and machine-readable modes preserve their bytes.
    command.chars().any(|ch| "|;&><\n\r".contains(ch))
        || command.split_whitespace().any(|part| matches!(
            part, "--json" | "--csv" | "--plain" | "--stdout" | "--xml" | "-j"
        ))

}

fn is_table_header(line: &str) -> bool {
    let trimmed = line.trim();
    let columns = trimmed.split_whitespace().collect::<Vec<_>>();
    columns.len() >= 2 && trimmed.len() >= 5
        && columns.iter().all(|column| {
            if matches!(*column,"MiB"|"GiB"|"KiB"|"%") { return true; }
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

#[derive(Default)]
struct TableStyle {
    status: Option<usize>,
    metrics: Vec<(usize, Metric, bool)>,
}
impl TableStyle {
    fn from_header(line: &str) -> Self {
        let mut style = Self::default();
        for (i,(a,b)) in table_columns(line).into_iter().enumerate() {
            let label = line[a..b].trim();
            if matches!(label.to_ascii_uppercase().as_str(),"ESTADO"|"STATUS"|"SALUD"|"HEALTH"|"RESULTADO") {
                style.status = Some(i);
            } else if let Some(metric) = Metric::named(label) {
                style.metrics.push((i,metric,label.contains('%')));
            }
        }
        style
    }
    fn tone(&self, index:usize, value:&str)->Option<&'static str>{
        if self.status==Some(index){status_color(value)}else{
            self.metrics.iter().find(|(i,_,_)|*i==index)
                .and_then(|(_,metric,implicit)|metric.color(value,*implicit))
        }
    }
    fn row(&self, line:&str) -> String {
        if self.status.is_none() && self.metrics.is_empty() { return style_line(line); }
        let mut out = String::new();
        let mut last = 0;
        for (i,(a,b)) in table_columns(line).into_iter().enumerate() {
            let value = &line[a..b];
            let tone = self.tone(i,value);
            if let Some(color) = tone {
                out.push_str(&line[last..a]);
                out.push_str(color);
                out.push_str(value);
                out.push_str(RESET);
                last = b;
            }
        }
        out.push_str(&line[last..]);
        out
    }
}

#[derive(Default)]
pub struct LiveReport { table: TableStyle, header: Option<String> }
impl LiveReport {
    pub fn line(&mut self, line:&str)->String {
        let text = report_text(line);
        if text.contains(['\r','\0','\x1b']) { return text.into_owned(); }
        let columns=table_columns(&text).len();
        // A numeric PID and an uppercase state are a data row, not a title.
        // Preserve the live schema until a real boundary or a repeated header.
        if let Some(header)=&self.header {
            if text.as_ref()!=header.as_str() && columns==table_columns(header).len() {
                return self.table.row(&text);
            }
        }
        if is_table_header(&text) && columns>=2 {
            self.table=TableStyle::from_header(&text);
            self.header=Some(text.to_string());
            return Role::Title.paint(&text);
        }
        self.header=None;
        self.table=TableStyle::default();
        stream_line(&text)
    }
}

/// Classify only the *output*. Invocation and registry information are not
/// permitted here. They are relevant only to machine/VT transport routing.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum LayoutKind { Plain, Reference, Assessment, Summary, Report }
fn classify(text:&str)->LayoutKind{
    if text.trim().is_empty()||text.contains(['\x1b','\0']){return LayoutKind::Plain;}
    if help_document(text){return LayoutKind::Reference;}
    if text.lines().any(|l|matches!(l.trim().to_uppercase().as_str(),"CONCLUSIÓN"|"CONCLUSION"))
        && text.lines().any(|l|numbered_section(l).is_some()){return LayoutKind::Assessment;}
    if text.lines().any(|l|l.starts_with("Conteo: "))
        && text.lines().any(is_status_field){return LayoutKind::Summary;}
    if text.lines().any(|l|is_section_heading(l)||is_status_field(l)||looks_like_tabular_header(l)
        ||is_table_header(l)||field_parts(l).is_some()||report_layout::underline(l)){
        LayoutKind::Report
    }else{LayoutKind::Plain}
}
fn numbered_section(line:&str)->Option<&str>{
    let (number,title)=line.trim().split_once(". ")?;
    (number.chars().next().is_some_and(|c|c.is_ascii_digit())
        && number.chars().all(|c|c.is_ascii_alphanumeric())).then_some(title)
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
        "NO DISPONIBLE", "DETENIDO", "DESHABILITADO", "SIN CONEXIÓN", "ALERTA",
        "INACTIVO", "BLOQUEADO", "DOWN", "OFFLINE"]
        .iter().any(|term| status == *term || status.starts_with(&format!("{term} ")))
    {
        Some(BAD)
    } else if ["ADVERTENCIA", "ATENCIÓN", "ATENCION",
        "PENDIENTE", "DEGRADADO", "INDETERMINADO",
        "SIN IP", "DESCONOCIDO", "WARNING", "REQUIERE REVISIÓN", "NO VERIFICADA",
        "SIN PERFIL DOT3 CONFIRMADO", "SIN GATEWAY POR DEFECTO", "SIN DIRECCIONES", "ICMP NO RESPONDE", "NO RESPONDE", "SOSPECHA", "REVISAR", "RECURSOS"]
        .iter().any(|term| status == *term || status.starts_with(&format!("{term} ")))
    {
        Some(CAUTION)
    } else if ["PERFECTO", "PERFECTA", "ÓPTIMO", "OPTIMO",
        "ÓPTIMA", "OPTIMA", "EXCELENTE"]
        .contains(&status)
    {
        Some(PERFECT)
    } else if ["ESTABLISHED", "LISTENING", "CONECTADO", "CONECTADA", "CORRECTO",
        "CORRECTA", "ACTIVO", "ACTIVA", "OPERATIVO",
        "OPERATIVA", "DISPONIBLE", "OK", "UP", "ENLACE DETECTADO", "PRESENTE",
        "ICMP RESPONDE", "RESPONDE", "RESUELTO", "CONECTIVIDAD TCP CONFIRMADA", "CUENTA DESBLOQUEADA", "CUENTA SIN BLOQUEO PENDIENTE"]
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
    if let Some((number, title)) = text.split_once(". ") {
        if number.chars().next().is_some_and(|c|c.is_ascii_digit())
            && number.chars().all(|c|c.is_ascii_alphanumeric())
            && title.chars().any(char::is_alphabetic) { return true; }
    }
    !text.contains(':') && text.chars().any(char::is_alphabetic)
        && !text.chars().any(char::is_lowercase)
}

fn style_line(line: &str) -> String {
    if line.trim().is_empty() { return line.to_owned(); }
    let stripped = line.trim_start();
    if let Some(rest)=stripped.strip_prefix('[') {
        if let Some((severity,detail))=rest.split_once("] ") {
            let tone=match severity {"CRÍTICO"=>Some(BAD),"SOSPECHOSO"|"REVISAR"=>Some(CAUTION),"OBSERVAR"=>Some(LABEL),_=>None};
            if let Some(tone)=tone{return format!("{tone}● {severity}{RESET}  {VALUE}{detail}{RESET}");}
        }
    }
    if is_section_heading(stripped) {
        return format!("{HEADING}{stripped}{RESET}");
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
            let value_color = if alarm || warning { VALUE }
                else if is_status_field(stripped) || key == "autorización efectiva" { status_color(value).unwrap_or(VALUE) }
                else { Metric::named(&key).and_then(|metric|metric.color(value,key.contains('%'))).unwrap_or(VALUE) };
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
    if line.contains('\n') {
        line.split_inclusive('\n').map(|part| {
            let body=part.strip_suffix('\n').unwrap_or(part);
            let mut styled=stream_line(body);
            if part.ends_with('\n'){styled.push('\n');}
            styled
        }).collect()
    } else { stream_line(line) }
}

fn stream_line(line:&str)->String{
    if line.trim().is_empty(){return line.to_owned();}
    if is_section_heading(line){return Role::Title.paint(line);}
    // A stream is a running transcript, not a sequence of report headers.
    style_line(line)
}

/// Display completion alternatives on a measured grid, without editing
/// the input or interpreting file-name control characters as terminal commands.
pub fn completion_catalogue(matches:&[String],cols:u16)->String {
    let width=usize::from(cols.saturating_sub(3)).max(8);
    let items:Vec<String>=matches.iter().map(|s|s.chars().map(|c|if c.is_control(){'�'}else{c}).collect()).collect();
    let cell=items.iter().map(|s|display_width(s)).max().unwrap_or(1).saturating_add(3);
    let columns=(width/cell.max(1)).clamp(1,4);
    let mut out=format!("\r\n{}\r\n",Role::Title.paint("SUGERENCIAS"));
    for row in items.chunks(columns) {
        if columns==1 {
            for item in row {
                for part in wrap_line(item,width) {
                    out.push_str(&Role::Command.paint(&part));out.push_str("\r\n");
                }
            }
        } else {
            for (i,item) in row.iter().enumerate() {
                if i>0 {out.push_str("  ");}
                out.push_str(&Role::Command.paint(item));
                if i+1<row.len(){out.push_str(&" ".repeat(cell.saturating_sub(2+display_width(item))));}
            }
            out.push_str("\r\n");
        }
    }
    out
}
#[cfg(test)]
mod completion_catalogue_tests {
    use super::*;
    #[test]
    fn candidates_remain_readable_and_control_bytes_do_not_escape(){
        let items=vec!["interfaces".into(),"profiles".into(),"networks".into(),"unsafe\x1bname".into()];
        for cols in [20,80,120] {
            let out=completion_catalogue(&items,cols);
            let clean=report_text(&out);
            assert!(clean.contains("interfaces")&&clean.contains("profiles")&&clean.contains("networks"));
            assert!(!clean.contains('\x1b'));
            assert!(clean.lines().all(|line|display_width(line.trim_end_matches('\r'))<=usize::from(cols)));
        }
    }
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

/// Reorder the network assessment only for the interactive display. Raw output,
/// CSV, scripts and redirects retain the original evidence and layout.

// Reusable report components. Layout is computed in visible cells before
// styling, so both native and console frontends receive identical content.
const TABLE_BAND: &str = "\x1b[48;2;36;39;38m";
const TABLE_HEADER: &str = "\x1b[48;2;56;54;46m";
const METER_TRACK: &str = "\x1b[48;2;60;56;55m";
pub const METER_OK_RGB:(u8,u8,u8)=(103,145,85);
pub const METER_WARN_RGB:(u8,u8,u8)=(184,108,35);
pub const METER_BAD_RGB:(u8,u8,u8)=(179,57,43);

fn background(rgb:(u8,u8,u8))->String{
    format!("\x1b[48;2;{};{};{}m",rgb.0,rgb.1,rgb.2)
}
fn preserve_band(text:&str,band:&str)->String {
    // Existing semantic foreground spans must not reset the row background.
    let mut out=String::from(band);
    out.push_str(&text.replace(RESET,&format!("{RESET}{band}")));
    out.push_str(RESET);
    out
}
fn field_parts(line:&str)->Option<(&str,&str)>{
    let (name,value)=line.trim_start().split_once(": ")?;
    if name.is_empty()||name.chars().count()>40||name.contains("  ")
        || !name.chars().any(char::is_alphabetic){return None;}
    Some((name,value))
}
fn percent_metric(name:&str,value:&str)->Option<(Metric,f32)>{
    let metric=Metric::named(name)?;
    let value=value.trim();
    let raw=value.strip_suffix('%').or_else(||name.contains('%').then_some(value))?;
    let amount=raw.trim().replace(',',".").parse::<f32>().ok()?;
    if !amount.is_finite()||!(0.0..=100.0).contains(&amount){return None;}
    Some((metric,amount))
}
fn meter(metric:Metric,value:f32,width:usize)->String{
    let filled=(width as f32*value/100.0).round() as usize;
    let rgb=match metric.role(value,None){
        Role::Failure=>METER_BAD_RGB,Role::Warning=>METER_WARN_RGB,_=>METER_OK_RGB,
    };
    format!("{}{}{RESET}{METER_TRACK}{}{RESET}",background(rgb),
        " ".repeat(filled)," ".repeat(width-filled))
}
fn render_field_group(lines:&[&str],width:usize)->String {
    let styles:Vec<_>=lines.iter().map(|s|style_line(s)).collect();
    let styles:Vec<_>=styles.iter().map(String::as_str).collect();
    let mut out=Vec::new();
    if !report_layout::field_grid(lines,width,&mut out){
        report_layout::fields(lines,&styles,width,&mut out);
    }
    out.push(String::new());
    out.join("\n")
}
fn table_header_band(line:&str,width:usize)->String {
    let padded=format!("{HEADING}{line}{}"," ".repeat(width.saturating_sub(display_width(line))));
    preserve_band(&padded,TABLE_HEADER)
}
fn table_body_band(line:&str,styled:&str,width:usize,index:usize)->String{
    if index%2==0 {
        let padded=format!("{styled}{}"," ".repeat(width.saturating_sub(display_width(line))));
        preserve_band(&padded,TABLE_BAND)
    } else { styled.to_owned() }
}

fn compose_assessment(stdout:&str,width:usize)->Option<String>{
    let mut source=stdout.lines().map(str::trim).filter(|l|!l.is_empty());
    let title=source.next()?;
    let mut blocks:Vec<(String,Vec<String>)>=Vec::new();
    let mut meta=Vec::new();
    let mut trailing=Vec::new();
    for line in source{
        if line.starts_with("Evidencia detallada: "){trailing.push(line);continue;}
        if matches!(line.to_uppercase().as_str(),"CONCLUSIÓN"|"CONCLUSION") {
            blocks.push((line.to_owned(),Vec::new()));
        }else if let Some(name)=numbered_section(line){
            blocks.push((name.to_owned(),Vec::new()));
        }else if let Some((_,rows))=blocks.last_mut(){rows.push(line.to_owned());}
        else{meta.push(line.to_owned());}
    }
    let index=blocks.iter().position(|(n,_)|matches!(n.to_uppercase().as_str(),"CONCLUSIÓN"|"CONCLUSION"))?;
    let (caption,summary)=blocks.remove(index);
    let mut doc=Document::default();doc.title(title);
    let mut out=doc.render(OutputMode::Interactive);
    let emit=|out:&mut String,line:&str|{for part in wrap_line(line,width){out.push_str(&style_line(&part));out.push('\n');}};
    out.push('\n');
    for line in summary{
        if is_status_field(&line){
            if let Some((_,status))=line.split_once(": "){
                out.push_str(&report_layout::feature_grid(&[(&caption,status,status_color(status).unwrap_or(VALUE))],width));
                continue;
            }
        }
        emit(&mut out,&line);
    }
    if !meta.is_empty(){out.push('\n');}
    let mut metadata=Document::default();
    for line in &meta{
        if let Some((name,value))=field_parts(line){metadata.field(name,value);}
        else{metadata.line(Role::Detail,line.as_str());}
    }
    out.push_str(&metadata.render(OutputMode::Interactive));
    out.push_str(&format!("\n{HEADING}Comprobaciones{RESET}\n"));
    let label_width=blocks.iter().map(|(n,_)|display_width(n)).max().unwrap_or(0).min(width/2);
    for (name,rows) in blocks{
        let status=rows.iter().find(|s|is_status_field(s)).and_then(|s|s.split_once(": ").map(|(_,v)|v));
        if let Some(status)=status{
            let gap=label_width.saturating_sub(display_width(&name))+3;
            let color=status_color(status).unwrap_or(VALUE);
            if display_width(&name)+gap+display_width(status)<=width{
                out.push_str(&format!("{LABEL}{name}{RESET}{}{color}{status}{RESET}\n"," ".repeat(gap)));
            }else{emit(&mut out,&name);emit(&mut out,&format!("  Estado: {status}"));}
        }else{emit(&mut out,&name);}
        let mut removed_status=false;
        for row in rows{
            if !removed_status&&is_status_field(&row){removed_status=true;continue;}
            emit(&mut out,&format!("    {row}"));
        }
    }
    if !trailing.is_empty(){out.push('\n');for line in trailing{emit(&mut out,line);}}
    Some(out)
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
#[allow(dead_code)] // legacy adapter; production uses Formatter::present
pub fn decorate_registered_at_width(
    command: &str,
    stdout: &str,
    cols: u16,
    registered_native: &[String],
) -> String {
    decorate_with_native_commands(command, stdout, cols, Some(registered_native))
}


fn help_document(stdout:&str)->bool{
    if stdout.lines().take(10).any(|line|{
        let lower=line.trim_start().to_ascii_lowercase();
        lower.starts_with("uso:")||lower.starts_with("usage:")
    }){return true;}
    // A syntax/description pair under a heading can identify a reference
    // even without a usage line. A tabular header rules out that inference.
    let lines:Vec<_>=stdout.lines().collect();
    if lines.iter().any(|l|is_table_header(l)&&table_columns(l).len()>=2){return false;}
    lines.iter().any(|l|is_section_heading(l))&&lines.iter().any(|l|{
        help_pair(l).is_some_and(|(syntax,_)|syntax.contains(" --")||syntax.contains('['))
    })
}
fn help_pair(line:&str)->Option<(&str,&str)>{
    let line=line.trim();
    let i=line.find("  ")?;
    let syntax=line[..i].trim();
    let description=line[i..].trim();
    if syntax.is_empty()||description.is_empty()||display_width(syntax)>65||syntax.ends_with(':'){return None;}
    Some((syntax,description))
}
/// Help is an indexed reference, not an inventory or a diagnosis. The
/// descriptions of an entire document share one left edge.
fn compact_help(stdout:&str,width:usize)->String{
    let root=stdout.lines().find_map(|line|help_pair(line).and_then(|(syntax,_)|syntax.split_whitespace().next())).unwrap_or("");
    let label_width=stdout.lines().filter_map(help_pair).map(|(k,_)|display_width(k))
        .max().unwrap_or(0).min(width.saturating_sub(22)).min(40);
    let mut result=Vec::new();
    let mut first_content=true;
    for line in stdout.lines(){
        let trimmed=line.trim();
        if trimmed.is_empty(){
            if result.last().is_some_and(|l:&String|!l.is_empty()){result.push(String::new());}
        }else if first_content && (trimmed.contains(" — ") || trimmed.contains(" – "))
            && display_width(trimmed)<=width {
            result.extend(report_layout::headline(trimmed,width));
        }else if let Some((key,value))=trimmed.split_once(": ")
            && matches!(key.to_ascii_lowercase().as_str(),"uso"|"usage"|"ejemplo"|"example"|"ejemplos"|"examples")
        {
            result.push(format!("{HEADING}{key}:{RESET} {COMMAND}{value}{RESET}"));
        }else if is_section_heading(trimmed){
            result.push(Role::Title.paint(trimmed));
        }else if let Some((syntax,description))=help_pair(line){
            let indent=label_width+4;
            if display_width(syntax)>label_width||width<36{
                result.push(format!("  {}",Role::Command.paint(syntax)));
                for part in wrap_line(description,width.saturating_sub(4)){result.push(format!("    {}",Role::Detail.paint(&part)));}
            }else{
                for (i,part) in wrap_line(description,width.saturating_sub(indent)).iter().enumerate(){
                    if i==0{
                        result.push(format!("  {COMMAND}{syntax}{RESET}{}{DETAILS}{part}{RESET}"," ".repeat(indent-2-display_width(syntax))));
                    }else{result.push(format!("{}{DETAILS}{part}{RESET}"," ".repeat(indent)));}
                }
            }
        }else if !root.is_empty()&&trimmed.starts_with(&format!("{root} "))&&!trimmed.contains(" — "){
            result.push(format!("  {}",Role::Command.paint(trimmed)));
        }else{
            for part in wrap_line(trimmed,width){result.push(Role::Detail.paint(&part));}
        }
        if !trimmed.is_empty(){first_content=false;}
    }
    while result.last().is_some_and(String::is_empty){result.pop();}
    if result.is_empty(){String::new()}else{result.join("\n")+"\n"}
}


fn decorate_with_native_commands(
    command: &str, stdout: &str, cols: u16,
    _registered_native: Option<&[String]>,
) -> String {
    if Formatter::mode(command,true)!=OutputMode::Interactive || stdout.contains(['\x1b','\0']) {return stdout.to_owned();}
    compose_text(stdout,cols,classify(stdout))
}

fn compose_text(stdout:&str,cols:u16,kind:LayoutKind)->String{
    if kind==LayoutKind::Plain{return stdout.to_owned();}
    // Leave a small right margin so the cursor never autowraps a full row.
    let width = (cols as usize).saturating_sub(3).clamp(18, 160);
    match kind {
        LayoutKind::Reference=>return compact_help(stdout,width),
        LayoutKind::Assessment=>if let Some(summary)=compose_assessment(stdout,width){return summary;},
        LayoutKind::Summary=>if let Some(summary)=report_layout::counted_summary(stdout,width){return summary;},
        _=>{}
    }
    let mut result=String::new();
    // The prompt already shows the command. Do not echo it in every report.
    let mut table = TableStyle::default();
    let mut table_active=false;
    let mut table_index=0usize;
    let mut previous_blank = true;
    let lines:Vec<&str>=stdout.lines().collect();
    let mut cursor=0;
    while cursor<lines.len(){
        let line=lines[cursor].trim_end_matches('\r');
        cursor+=1;
        if !line.trim().is_empty() && lines.get(cursor).is_some_and(|s|report_layout::underline(s)) {
            cursor+=1;
            result.push_str(&Role::Title.paint(line.trim()));
            result.push('\n');
            previous_blank=false;
            table=TableStyle::default();
            table_active=false;
            continue;
        }
        // A run of fields becomes a component, not a series of unrelated
        // coloured strings. Help and fixed-width tables retain their syntax.
        if !table_active && field_parts(line).is_some(){
            let start=cursor-1;
            while cursor<lines.len()&&field_parts(lines[cursor]).is_some(){cursor+=1;}
            let group=&lines[start..cursor];
            // Ordinary metadata does not become a display-size KPI merely
            // because there are two fields. Only explicit outcome fields lead.
            let top_level=start>0 && lines[..start].iter().filter(|l|!l.trim().is_empty()&&!report_layout::underline(l)).count()==1;
            if top_level && group.len()>=3 {
                if let Some(index)=group.iter().position(|line|is_status_field(line)
                    && field_parts(line).is_some_and(|(_,value)|status_color(value).is_some())) {
                    let (label,value)=field_parts(group[index]).unwrap();
                    result.push_str(&report_layout::feature_grid(&[(label,value,status_color(value).unwrap_or(VALUE))],width));
                    result.push('\n');
                    let rest:Vec<_>=group.iter().enumerate().filter_map(|(i,row)|(i!=index).then_some(*row)).collect();
                    result.push_str(&render_field_group(&rest,width));
                } else {result.push_str(&render_field_group(group,width));}
            } else {result.push_str(&render_field_group(group,width));}
            previous_blank=false;
            continue;
        }
        if is_table_header(line) && table_columns(line).len()>=2 {
            let count=table_columns(line).len();
            let start=cursor;
            while cursor<lines.len() && !lines[cursor].trim().is_empty()
                && lines[cursor]!=line && table_columns(lines[cursor]).len()==count
                && field_parts(lines[cursor]).is_none() && !report_layout::underline(lines[cursor]) {
                cursor+=1;
            }
            if cursor>start {
                result.push_str(&report_layout::table(line,&lines[start..cursor],width));
                previous_blank=false;
                table=TableStyle::default();
                table_active=false;
                continue;
            }
            table = TableStyle::from_header(line);
            table_active=true;table_index=0;
            if display_width(line)<=width{
                result.push_str(&table_header_band(line,width));result.push('\n');
                previous_blank=false;
                continue;
            }
        } else if line.trim().is_empty() || (is_section_heading(line) && table_columns(line).len() <= 1) {
            table = TableStyle::default();table_active=false;
        }
        if table_active && display_width(line)<=width && !is_table_header(line){
            result.push_str(&table_body_band(line,&table.row(line),width,table_index));
            result.push('\n');table_index+=1;previous_blank=false;continue;
        }
        if line.trim().is_empty() {
            if !previous_blank { result.push('\n'); }
            previous_blank = true;
            continue;
        }
        if is_section_heading(line) && !is_table_header(line) && !previous_blank {
            if !result.ends_with("\n\n"){result.push('\n');}
        }
        previous_blank = false;
        let is_bullet = line.trim_start().starts_with("+ ");
        let fragments = if is_bullet || display_width(line) > width {
            wrap_line(line, width)
        } else {
            vec![line.to_owned()]
        };
        for fragment in fragments {
            let styled = if is_table_header(&fragment) {
                format!("{HEADING}{fragment}{RESET}")
            } else {table.row(&fragment)};
            result.push_str(&styled);
            result.push('\n');
        }
    }
    result.truncate(result.trim_end_matches('\n').len());
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
        let style = TableStyle::from_header(title);
        assert_eq!(style.status, Some(1));
        let row = "Red A     ACTIVO    enlace conectado";
        assert!(style.row(row).contains(GOOD));
        let description = "Red A     desconocido    conectado";
        assert!(!style.row(description).contains(GOOD));
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
    fn report_headings_and_commands_have_distinct_roles() {
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
            "notsh interfaces --json", "notsh interfaces --csv", "notsh interfaces; echo ok",
            "notsh interfaces --plain", "cat report.txt | more", "grep error report.txt > hits.txt"] {
            assert_eq!(decorate(command, text), text, "{command}");
        }
    }
    #[test]
    fn interactive_reports_use_the_same_identity_for_all_command_names() {
        let text = "STATE   VALUE\nUP      1\n";
        let expected = decorate("sys",text);
        assert_ne!(expected,text);
        for command in ["echo foo","cat report.txt","grep error report.txt"] {
            assert_eq!(decorate(command,text),expected);
            let mode = Formatter::mode(command,false);
            assert_eq!(Formatter::present(text,"",80,mode).stdout,text);
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
    fn diagnosis_prioritizes_findings_and_preserves_evidence(){
        let input="DIAGNÓSTICO DE RED\nDestino: 10.6.11.7:443\nModo: solo lectura\n\n1. INTERFACES FÍSICAS\n  Estado: ENLACE DETECTADO\n  Evidencia: enlace físico\n\n1b. NAC / 802.1X CABLEADO\n  Estado: SIN PERFIL DOT3 CONFIRMADO\n  Autorización efectiva: NO VERIFICADA\n\n6. TCP\n  Estado: SIN CONEXIÓN\n  Error: connection timed out\n\nCONCLUSIÓN\n  Estado: REQUIERE REVISIÓN\n  ADVERTENCIA: No conecta TCP 443.\n\nEvidencia detallada: net routes\n";
        let out=decorate_at_width("notsh test 10.6.11.7",input,100);
        assert!(out.find("REQUIERE REVISIÓN").unwrap()<out.find("INTERFACES FÍSICAS").unwrap());
        assert!(out.contains(GOOD)&&out.contains(CAUTION)&&out.contains(BAD));
        assert!(out.contains("connection timed out")&&out.contains("NO VERIFICADA"));
        assert!(!out.contains("notsh test 10.6.11.7"));
        assert_eq!(decorate_at_width("notsh test 10.6.11.7 --csv",input,100),input);
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

// Alternate-screen monitor layouts share the same visual roles.
use crate::core::ports::TerminalPanel;
pub const ACCENT:&str=HEADING;
pub const WARN:&str=CAUTION;
#[allow(dead_code)] // compatibility for legacy monitor adapters
pub const CONTENT:&str=DETAILS;
/// Style a whole terminal row, including empty cells. This ensures the
/// alternate-screen TUI has readable contrast independently of the wallpaper.
pub fn monitor_row(
    buffer: &mut String,
    text: &str,
    width: usize,
    foreground: &str,
    background: &str,
) {
    let visible: String = text.chars().take(width).collect();
    buffer.push_str(background);
    buffer.push_str(foreground);
    buffer.push_str(&visible);
    buffer.push_str(&" ".repeat(width.saturating_sub(visible.chars().count())));
    buffer.push_str("\x1b[0m\r\n");
}


/// Fixed-width live rows preserve alignment, background and per-cell role.
pub fn monitor_cells(buffer:&mut String,cells:&[(&str,Role)],width:usize,background:&str){
    let mut used=0;
    buffer.push_str(background);
    for (text,role) in cells{
        if used>=width{break;}
        let part=shorten(text,width-used);
        used+=display_width(&part);
        buffer.push_str(role.ansi());buffer.push_str(&part);
    }
    buffer.push_str(&" ".repeat(width.saturating_sub(used)));
    buffer.push_str("\x1b[0m\r\n");
}

pub struct LiveScreen {
    text: String,
    line: usize,
    panels: Vec<TerminalPanel>,
}

impl LiveScreen {
    pub fn new() -> Self {
        Self { text: String::new(), line: 0, panels: Vec::new() }
    }

    pub fn section(&mut self, title: &str, body: &str) {
        let start = self.line;
        self.text.push_str("  ");
        self.text.push_str(ACCENT);
        self.text.push_str(title);
        self.text.push_str(RESET);
        self.text.push('\n');
        self.line += 1;

        let mut table = TableStyle::default();
        let mut table_active=false;
        let mut index=0;
        for line in body.trim_end_matches('\n').split('\n') {
            if line.is_empty() && body.trim().is_empty() { break; }
            self.text.push_str("  ");
            if line.contains('\x1b') {
                self.text.push_str(line);
            } else if is_table_header(line) {
                table = TableStyle::from_header(line);
                table_active=true;index=0;
                self.text.push_str(&table_header_band(line,display_width(line)));
            } else {
                if line.trim().is_empty(){table_active=false;table=TableStyle::default();}
                let styled=table.row(line);
                if table_active{self.text.push_str(&table_body_band(line,&styled,display_width(line),index));index+=1;}
                else{self.text.push_str(&styled);}
            }
            self.text.push('\n');
            self.line += 1;
        }

        if self.panels.len() < 4 {
            self.panels.push(TerminalPanel {
                start_row: start.min(u16::MAX as usize) as u16,
                rows: (self.line - start).min(u16::MAX as usize) as u16,
            });
        }
        self.text.push('\n');
        self.line += 1;
    }

    pub fn footer(&mut self, text: &str) {
        self.text.push_str("  ");
        self.text.push_str(LABEL);
        self.text.push_str(text);
        self.text.push_str(RESET);
        self.text.push('\n');
        self.line += 1;
    }

    pub fn finish(self) -> (String, Vec<TerminalPanel>) {
        (self.text, self.panels)
    }
}

/// Monitors must fit within terminal cells, not pixel width. ANSI escape
/// sequences do not contribute to the visible width.
pub fn shorten(value: &str, max: usize) -> String {
    let max = max.max(1);
    if value.chars().count() <= max { return value.to_owned(); }
    if max <= 2 { return value.chars().take(max).collect(); }
    format!("{}…", value.chars().take(max - 1).collect::<String>())
}

#[cfg(test)]
mod monitor_tests {
    use super::*;

    #[test]
    fn metrics_work_for_unrelated_reports_tables_and_streams() {
        for name in ["inventario", "reporte-nuevo"] {
            let body = "PID      CPU%     ESTADO\n431      72.2     ACTIVO\n";
            let report = Formatter::result(name,body,"",100,&[name.into()],OutputMode::Interactive);
            assert!(report.stdout.contains(&format!("{BAD}72.2{RESET}")));
            assert!(report.stdout.contains(&format!("{GOOD}ACTIVO{RESET}")));
            assert_eq!(Formatter::result(name,body,"",100,&[],OutputMode::Machine).stdout,body);
        }
        let mut stream = LiveReport::default();
        stream.line("PID      CPU%     ESTADO");
        assert!(stream.line("431      35.2     ACTIVO").contains(CAUTION));
        assert!(stream.line("431      85.2     ACTIVO").contains(BAD));
        assert!(!stream.line("431      5.2      ACTIVO").contains(BAD));
        stream.line("");
        assert!(stream.line("Pérdida: 10%").contains(BAD));
        assert!(!stream.line("RAM: 90 MiB").contains(BAD));
        assert_eq!(Metric::Cpu.role(32.0,Some(3.0)), Role::Failure);
        assert_eq!(Metric::Cpu.role(8.0,Some(32.0)), Role::Value);
    }

    #[test]
    fn sections_report_real_cell_coordinates_without_character_frames() {
        let mut screen = LiveScreen::new();
        screen.section("RED", "escaneando");
        screen.section("DISPOSITIVOS", "IP  NOMBRE\n10.0.0.3  PC");
        let (text, panels) = screen.finish();
        assert_eq!(panels.len(), 2);
        assert_eq!(panels[0].start_row, 0);
        assert_eq!(panels[0].rows, 2);
        assert_eq!(panels[1].start_row, 3);
        assert_eq!(panels[1].rows, 3);
        assert!(text.contains(ACCENT));
        assert!(!text.contains("┌"));
        assert!(!text.contains("└"));
        assert!(!text.contains("──"));
    }

    #[test]
    fn clipped_display_columns_are_measured_by_characters() {
        assert_eq!(shorten("aaaaaaaaaa", 6), "aaaaa…");
        assert_eq!(shorten("SST", 8), "SST");
    }
}

#[cfg(test)]
mod formatter_contract_tests {
    use super::*;
    #[test]
    fn all_channels_use_one_contract_and_keep_machine_bytes(){
        let native=vec!["future".to_owned()];
        let stdout="DIAGNÓSTICO:\nEstado: OPERATIVO\nNombre: host\n";
        let stderr="Error: detalle del fallo\n";
        let out=Formatter::result("future",stdout,stderr,90,&native,OutputMode::Interactive);
        assert!(out.stdout.contains(HEADING)&&out.stdout.contains(GOOD)&&out.stdout.contains(LABEL));
        assert!(out.stderr.contains(BAD)&&out.stderr.contains("detalle del fallo"));
        for mode in [OutputMode::Machine,OutputMode::Terminal]{
            let raw=Formatter::result("future",stdout,stderr,90,&native,mode);
            assert_eq!(raw.stdout,stdout);assert_eq!(raw.stderr,stderr);
        }
        assert_eq!(Formatter::mode("future --csv",true),OutputMode::Machine);
        assert_eq!(Formatter::mode("future | cat",true),OutputMode::Machine);
        assert_eq!(Formatter::mode("fetch",true),OutputMode::Terminal);
        assert_eq!(Formatter::stream_bytes(b"\xff",false),b"\xff");
    }
    #[test]
    fn human_reports_restyle_legacy_gray_and_preserve_terminal_controls() {
        let data = "\x1b[37mNombre: equipo\x1b[0m\n";
        let commands = vec!["sys".to_owned()];
        let human = Formatter::result("sys info",data,data,100,&commands,OutputMode::Interactive);
        assert!(human.stdout.contains(LABEL) && human.stdout.contains(VALUE));
        assert!(human.stderr.contains(BAD));
        assert!(!human.stdout.contains("\x1b[37m"));
        assert!(Formatter::live(data).contains(LABEL));
        let control = "\x1b[2J\x1b[37mredraw\x1b[0m";
        assert_eq!(Formatter::live(control),control);
        assert_eq!(Formatter::result("sys --csv",data,data,100,&commands,OutputMode::Machine).stdout,data);
        let (r,g,b) = DEFAULT_FOREGROUND;
        assert_eq!(format!("#{r:02X}{g:02X}{b:02X}"), DEFAULT_FOREGROUND_HEX);
    }
    #[test]
    fn typography_and_document_share_roles(){
        let mut d=Document::default();
        d.title("Sistema");d.field("CPU","Intel");d.line(Role::Warning,"Revisar");
        let human=d.render(OutputMode::Interactive);
        assert!(human.contains(HEADING)&&human.contains(LABEL)&&human.contains(CAUTION));
        assert_eq!(d.render(OutputMode::Machine),"Sistema\nCPU: Intel\nRevisar\n");
        assert!(font_pixels(Some(Role::Title),13.,8.,17.)>font_pixels(Some(Role::Label),13.,8.,17.));
        assert!(font_pixels(Some(Role::Title),32.,8.,17.)<=8./0.60);
        for role in [Role::Title,Role::Label,Role::Success,Role::Warning,Role::Optimal]{
            assert_eq!(Role::from_rgb(role.style().rgb),Some(role));
        }
    }
}

#[cfg(test)]
mod component_layout_tests{
    use super::*;
    #[test]
    fn independent_commands_share_field_and_meter_layout(){
        let data="RECURSOS\nCPU: 65%\nRAM: 81%\nNombre: equipo\n";
        for root in ["inventario","diagnostico-nuevo"] {
            let out=Formatter::result(root,data,"",92,&[root.into()],OutputMode::Interactive).stdout;
            assert!(out.contains(METER_TRACK));
            assert!(out.contains(&background(METER_BAD_RGB)));
            assert!(out.contains(&background(METER_WARN_RGB)));
            assert!(out.contains("65%")&&out.contains("81%")&&out.contains("equipo"));
            assert_eq!(Formatter::result(root,data,"",92,&[],OutputMode::Machine).stdout,data);
        }
        assert!(percent_metric("RAM","81 MiB").is_none());
        assert!(percent_metric("CPU","120%").is_none());
    }
    #[test]
    fn tables_have_actual_backgrounds_and_preserve_statuses(){
        let raw="PID     ESTADO    CPU%\n431     ACTIVO    65\n432     ACTIVO    12\n";
        let out=Formatter::result("inventario",raw,"",85,&["inventario".into()],OutputMode::Interactive).stdout;
        assert!(out.contains(TABLE_HEADER)&&out.contains(TABLE_BAND));
        assert!(out.contains(GOOD)&&out.contains(BAD));
        assert!(out.contains("431")&&out.contains("432"));
    }
}

#[cfg(test)]
mod editorial_reference_tests{
    use super::*;
    #[test]
    fn reference_help_has_aligned_descriptions_without_report_bands(){
        let raw="sys — información\nuso: sys SUBCOMANDO\n\nInformación:\nsys info      resumen\nsys processes [NOMBRE]   listado detallado\n";
        let out=Formatter::result("sys",raw,"",100,&["sys".into()],OutputMode::Interactive).stdout;
        let clean=report_text(&out);
        let x=|word:&str|clean.lines().find_map(|l|l.find(word)).unwrap();
        assert_eq!(x("resumen"),x("listado detallado"));
        // A primary headline reserves transparent native display rows.
        // Help must never acquire filled table/feature panels.
        assert!(out.contains(&background(REPORT_HEADER_RGB)));
        assert!(!out.contains(TABLE_HEADER)&&!out.contains(TABLE_BAND));
        assert!(!out.contains(&background(FEATURE_RGB))&&!out.contains(&background(FEATURE_ALT_RGB)));
    }
    #[test]
    fn live_headings_keep_semantic_title_without_reserving_rows(){
        let line="Haciendo ping a 8.8.8.8 con 32 bytes de datos:";
        let out=Formatter::live_layout(&Formatter::live(line),80);
        assert_eq!(report_text(&out),line);
        assert!(out.contains(HEADING)&&!out.contains("\x1b[48;"));
    }
    #[test]
    fn monitor_cpu_alert_is_scoped_to_its_column(){
        let mut row=String::new();
        monitor_cells(&mut row,&[("431 ",Role::Label),("65.0",Role::Failure),(" firefox.exe",Role::Value)],80,SURFACE);
        assert!(row.contains(&format!("{BAD}65.0{VALUE} firefox.exe")));
    }
}

#[cfg(test)]
mod content_contract{
    use super::*;
    #[test]
    fn same_text_always_has_the_same_presentation(){
        let fixtures=[
            "uso: herramienta [opciones]\nComandos:\nlistar    muestra datos\n",
            "RECURSOS\nCPU: 65%\nNombre: equipo\n",
            "INVENTARIO\nNOMBRE    ESTADO\nEquipo    ACTIVO\n",
            "AUDITORÍA\nEstado: ALERTA\nConteo: Hallazgos: 1 · Críticos: 1\n[CRÍTICO] señal\n",
            "REVISIÓN\n1. ENLACE\nEstado: ACTIVO\nCONCLUSIÓN\nEstado: CORRECTO\n",
            "Operación terminada.\n",
        ];
        for input in fixtures{
            let first=Formatter::result("sys",input,"",100,&["sys".into()],OutputMode::Interactive);
            for name in ["otra-herramienta","net","plugin-desconocido","triage --help"]{
                assert_eq!(Formatter::result(name,input,"",100,&[],OutputMode::Interactive),first);
            }
        }
    }
}

#[cfg(test)]
mod windows_report_regression {
    use super::*;
    #[test]
    fn crlf_reports_have_same_layout_and_reading_eligibility_as_lf() {
        let lf = "Inventario:\nNombre: Equipo\nEstado: Activo\n";
        let a = Formatter::present(lf, "", 80, OutputMode::Interactive);
        let b = Formatter::present(&lf.replace("\n","\r\n"), "", 80, OutputMode::Interactive);
        assert_eq!(a,b);
        assert!(Formatter::readable_result(&b,OutputMode::Interactive));
    }
}
