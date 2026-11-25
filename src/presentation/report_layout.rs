//! Terminal-cell layout for foreground output. ANSI surfaces travel with the
//! cells through scrollback, selection, and the native and console renderers.
use super::*;

// Reserved display rows carry a layout marker, not a painted card.
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

/// Typography and whitespace define the field grid. Ordinary labels and
/// evidence never acquire their own filled rectangle.
pub(super) fn fields(rows: &[&str], styles: &[&str], width: usize, out: &mut Vec<String>) {
    let gutter=rows.iter().filter_map(|r|field(r)).map(|(k,_)|display_width(k)+1)
        .max().unwrap_or(1).min((width/3).max(8));
    for (i,row) in rows.iter().enumerate(){
        let Some((label,value))=field(row)else{out.push(styles[i].to_owned());continue;};
        let normalized=format!("{label}: {value}");
        let styled=style_line(&normalized);
        let tone=styled.split_once(&format!(":{RESET} ")).and_then(|(_,v)|v.split_once('m'))
            .filter(|(prefix,_)|prefix.starts_with("\x1b[")).map(|(prefix,_)|format!("{prefix}m"))
            .unwrap_or_else(||VALUE.into());
        let label_tone=if styled.starts_with(BAD){BAD}else if styled.starts_with(CAUTION){CAUTION}else{LABEL};
        let stacked=width<gutter+18||display_width(label)+1>gutter;
        let indent=if stacked{2}else{gutter+3};
        if stacked{out.push(format!("{label_tone}{label}:{RESET}"));}
        let value=if value.is_empty(){"—"}else{value};
        for (part,fragment) in wrap_line(value,width.saturating_sub(indent).max(12)).iter().enumerate(){
            let prefix=if stacked||part!=0{" ".repeat(indent)}
                else{format!("{label_tone}{label}:{RESET}{}"," ".repeat(indent.saturating_sub(display_width(label)+1)))};
            let mut line=format!("{prefix}{tone}{fragment}{RESET}");
            let remaining=width.saturating_sub(indent+display_width(fragment));
            if part==0&&fragment==value&&remaining>=10 {
                if let Some((metric,n))=percent_metric(label,value){
                    line.push_str("   ");line.push_str(&super::meter(metric,n,(remaining-3).min(18)));
                }
            }
            out.push(line);
        }
    }
}

/// Editorial feature strip: short values receive display-size typography,
/// labels sit above them, and columns stack when there is insufficient space.
/// The reserved row travels with the value through the VT scrollback.
pub(super) fn feature_grid(items:&[(&str,&str,&str)],width:usize)->String {
    let mut out=Vec::new();
    let columns=if width>=54 {2}else{1};
    for group in items.chunks(columns){
        let cell_width=(width.saturating_sub(3*(group.len()-1)))/group.len();
        if group.iter().any(|(label,value,_)|display_width(label)+4>cell_width||display_width(value)+4>cell_width){
            for (label,value,_) in group{
                for fragment in wrap_line(&format!("{label}: {value}"),width){out.push(style_line(&fragment));}
            }
            continue;
        }
        let mut labels=String::new();let mut reserve=String::new();let mut values=String::new();
        for (index,(label,value,tone)) in group.iter().enumerate(){
            if index>0{labels.push_str("   ");reserve.push_str("   ");values.push_str("   ");}
            let surface=background(if index%2==0{FEATURE_RGB}else{FEATURE_ALT_RGB});
            labels.push_str(&format!("{surface}  {LABEL}{label}{RESET}{surface}{}{RESET}"," ".repeat(cell_width-display_width(label)-2)));
            reserve.push_str(&format!("{surface}{}{RESET}"," ".repeat(cell_width)));
            values.push_str(&format!("{surface}  {tone}{value}{RESET}{surface}{}{RESET}"," ".repeat(cell_width-display_width(value)-2)));
        }
        out.push(labels);out.push(reserve);out.push(values);
    }
    if out.is_empty(){String::new()}else{out.join("\n")+"\n"}
}

/// Final composition pass. Only the first document title gets display
/// typography; section headings get an unobtrusive native rule. Existing
/// field groups, tables, prose and live terminal data are not boxed again.
pub(super) fn render(styled:&str,width:usize,structured:bool)->String{
    if !structured||styled.is_empty()||styled.contains(['\0','\r']){return styled.into();}
    let lines:Vec<_>=styled.lines().collect();
    let clean:Vec<_>=lines.iter().map(|s|report_text(s)).collect();
    if clean.iter().any(|s|s.contains('\x1b')){return styled.into();}
    let mut out=Vec::new();
    let mut i=0;
    let mut first_title=true;
    while i<lines.len(){
        let line=clean[i].as_ref();
        let next_rule=!line.trim().is_empty()&&clean.get(i+1).is_some_and(|l|underline(l));
        let is_title=next_rule||lines[i].trim_start().starts_with(HEADING);
        if is_title&&first_title&&!is_table_header(line)&&display_width(line.trim())<=width.saturating_sub(3){
            let body=line.trim();
            // This marker is invisible in the native viewport. The blank
            // row is reserved for the ascenders of the display-size title.
            out.push(format!("{HEADER}{}{RESET}"," ".repeat(width)));
            out.push(format!("{HEADER}  {HEADING}{body}{RESET}{HEADER}{}{RESET}",
                " ".repeat(width.saturating_sub(display_width(body)+2))));
            first_title=false;
        }else if next_rule{
            out.push(Role::Title.paint(line.trim()));
            first_title=false;
        }else{
            out.push(lines[i].to_owned());
            if is_title||lines[i].starts_with(TABLE_HEADER){first_title=false;}
        }
        i+=if next_rule{2}else{1};
    }
    let mut result=out.join("\n");
    if styled.ends_with('\n'){result.push('\n');}
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
            assert!(out.contains(HEADER));
            assert!(!out.contains("\x1b[48;2;43;48;48m"));
            assert!(!out.contains("-----"));
            let plain = report_text(&out);
            let position = |word:&str|plain.lines().find_map(|l|l.find(word).map(|i|l[..i].chars().count())).unwrap();
            assert_eq!(position("Equipo"),position("Central"));
        }
    }
}

#[cfg(test)]
mod feature_tests {
    use super::*;
    #[test]
    fn feature_layout_is_responsive_and_preserves_values(){
        let items=[("DESTINO","10.6.11.7",VALUE),("MODO","solo lectura",VALUE)];
        for width in [34,80,120]{
            let out=feature_grid(&items,width);
            let plain=report_text(&out);
            assert!(plain.contains("10.6.11.7")&&plain.contains("solo lectura"));
            assert!(plain.lines().all(|line|display_width(line)<=width));
            assert_eq!(plain.lines().count(),if width>=54{3}else{6});
            assert_eq!(render(&out,width,true),out);
        }
    }
}

#[cfg(test)]
mod editorial_regressions{
    use super::*;
    #[test]
    fn ordinary_text_and_fields_do_not_become_background_panels(){
        let lines=["Nombre: equipo","Modelo: estación","CPU: 12%"];
        let styled:Vec<_>=lines.iter().map(|l|style_line(l)).collect();
        let refs:Vec<_>=styled.iter().map(String::as_str).collect();
        let mut out=Vec::new();fields(&lines,&refs,80,&mut out);
        assert!(!out[0].contains("\x1b[48;")&&!out[1].contains("\x1b[48;"));
        assert!(out[2].contains(METER_TRACK));
        let text="Un texto sin estructura\nOtra línea\n";
        assert_eq!(render(text,80,false),text);
        assert_eq!(render(text,80,true),text);
    }
}
