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
        let label_tone=field_label_tone(label);
        let stacked=width<gutter+18||display_width(label)+1>gutter;
        let indent=if stacked{2}else{gutter+3};
        if stacked{out.push(format!("{label_tone}{label}:{RESET}"));}
        let value=if value.is_empty(){"—"}else{value};
        for (part,fragment) in wrap_line(value,width.saturating_sub(indent).max(12)).iter().enumerate(){
            let prefix=if stacked||part!=0{" ".repeat(indent)}
                else{format!("{label_tone}{label}:{RESET}{}"," ".repeat(indent.saturating_sub(display_width(label)+1)))};
            let mut line=format!("{prefix}{}",paint_field_value(label,value,fragment,part==0));
            let remaining=width.saturating_sub(indent+display_width(fragment));
            if part==0&&fragment==value&&remaining>=10 {
                if let Some((metric,n))=bounded_metric(label,value){
                    line.push_str("   ");line.push_str(&super::meter(metric,n,(remaining-3).min(18)));
                }
            }
            out.push(line);
        }
    }
}

/// Compact metadata uses a two-column editorial grid when both regions fit.
/// Long values, metric bars and statuses retain their full-width reading order.
pub(super) fn field_grid(rows:&[&str],width:usize,out:&mut Vec<String>)->bool{
    if width<78 || rows.len()<4{return false;}
    let Some(items)=rows.iter().map(|row|field_parts(row)).collect::<Option<Vec<_>>>() else{return false;};
    let gap=5;
    let region=(width-gap)/2;
    let label_width=items.iter().map(|(name,_)|display_width(name)+1).max().unwrap_or(0);
    if label_width>region/2 || items.iter().any(|(name,value)|
        value.trim().is_empty() || display_width(value)+label_width+3>region
        || Metric::named(name).is_some() || is_status_field(&format!("{name}: {value}"))
        || matches!(name.to_lowercase().as_str(),"error"|"aviso"|"advertencia"|"fallo"|"crítico"|"critico"|"warning"|"autorización efectiva")){
        return false;
    }
    for pair in items.chunks(2){
        let mut line=String::new();
        for (i,(label,value)) in pair.iter().enumerate(){
            if i>0{line.push_str(&" ".repeat(gap));}
            let padded=label_width+3-display_width(label)-1;
            line.push_str(&format!("{LABEL}{label}:{RESET}{}{}"," ".repeat(padded),paint_field_value(label,value,value,true)));
            if i==0 && pair.len()==2{
                line.push_str(&" ".repeat(region-label_width-3-display_width(value)));
            }
        }
        out.push(line);
    }
    true
}

/// Measure complete cells before wrapping. A narrow table reflows each cell
/// independently; the first cell never spills into the status or metric column.
pub(super) fn table(header:&str,rows:&[&str],width:usize)->String{
    let cells=|line:&str|table_columns(line).into_iter().map(|(a,b)|line[a..b].to_owned()).collect::<Vec<_>>();
    let labels=cells(header);
    let data=rows.iter().map(|line|cells(line)).collect::<Vec<_>>();
    let style=TableStyle::from_header(header);
    let count=labels.len();
    let mut out=String::new();
    let gap=2;
    let budget=width.saturating_sub(gap*(count-1));
    if budget<count*6 {
        // Extremely narrow: retain all data as labelled records, not truncated rows.
        out.push_str(&Role::Title.paint(header.trim()));
        out.push('\n');
        for row in &data{
            out.push('\n');
            for (i,value) in row.iter().enumerate(){
                let key=&labels[i];
                out.push_str(&Role::Label.paint(key));out.push_str(":\n");
                for fragment in wrap_line(value,width.saturating_sub(2)){
                    out.push_str("  ");out.push_str(style.tone(i,value).unwrap_or(VALUE));
                    out.push_str(fragment.trim_start());out.push_str(RESET);out.push('\n');
                }
            }
        }
        return out;
    }
    let mut widths:Vec<_>=labels.iter().map(|s|display_width(s).max(6)).collect();
    for row in &data{
        for (i,value) in row.iter().enumerate(){widths[i]=widths[i].max(display_width(value));}
    }
    while widths.iter().sum::<usize>()>budget{
        let Some((i,_))=widths.iter().enumerate().filter(|(_,w)|**w>6).max_by_key(|(_,w)|**w)else{break;};
        widths[i]-=1;
    }
    let emit=|out:&mut String,row:&[String],heading:bool,index:usize|{
        let fragments:Vec<_>=row.iter().zip(&widths).map(|(value,w)|{
            if display_width(value)<=*w{vec![value.clone()]}else{
                // Cell-local wrapping, without the report paragraph's hanging indent.
                let mut pieces=Vec::new();let mut part=String::new();
                for ch in value.chars(){
                    if display_width(&part)+display_width(&ch.to_string())>*w{
                        pieces.push(std::mem::take(&mut part));
                    }
                    part.push(ch);
                }
                if !part.is_empty(){pieces.push(part);}pieces
            }
        }).collect();
        let height=fragments.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height{
            let mut visible=String::new();
            let mut styled=String::new();
            for (i,parts) in fragments.iter().enumerate(){
                if i>0{visible.push_str("  ");styled.push_str("  ");}
                let part=parts.get(line).map(String::as_str).unwrap_or("");
                let padding=" ".repeat(widths[i].saturating_sub(display_width(part)));
                visible.push_str(part);visible.push_str(&padding);
                let tone=if heading{HEADING}else{style.tone(i,&row[i]).unwrap_or(VALUE)};
                styled.push_str(tone);styled.push_str(part);styled.push_str(RESET);styled.push_str(&padding);
            }
            if heading{
                // Apply the same shared surface as every other table header.
                styled.push_str(&" ".repeat(width.saturating_sub(display_width(&visible))));
                out.push_str(&preserve_band(&styled,TABLE_HEADER));
            }else{out.push_str(&table_body_band(&visible,&styled,width,index));}
            out.push('\n');
        }
    };
    emit(&mut out,&labels,true,0);
    // Group only a genuine relationship schema with valid destinations,
    // gateways and interface IDs. Do not infer topology from arbitrary text.
    let column=|names:&[&str]|labels.iter().position(|label|names.iter().any(|name|label.eq_ignore_ascii_case(name)));
    let schema=column(&["DESTINATION","DESTINO"]).zip(column(&["GATEWAY","NEXT HOP","PUERTA DE ENLACE"]))
        .zip(column(&["IFINDEX","INTERFAZ"]));
    if let Some(((destination,gateway),interface))=schema.filter(|_|width>=42) {
        let mut groups:std::collections::BTreeMap<(String,String),Vec<usize>>=std::collections::BTreeMap::new();
        let valid=data.iter().enumerate().all(|(index,row)|{
            let Some(dest)=row.get(destination)else{return false;};
            let Some(via)=row.get(gateway)else{return false;};
            let Some(iface)=row.get(interface)else{return false;};
            if !dest.split_once('/').is_some_and(|(ip,prefix)|ip.parse::<std::net::IpAddr>().is_ok()&&prefix.parse::<u8>().is_ok())
                || via.parse::<std::net::IpAddr>().is_err()||iface.parse::<u32>().is_err(){return false;}
            groups.entry((via.clone(),iface.clone())).or_default().push(index);true
        });
        if valid&&groups.values().any(|entries|entries.len()>=2) {
            for ((gateway,interface),entries) in groups {
                if entries.len()>=2 {
                    let label=format!("Vía {gateway} · interfaz {interface} · {} rutas",entries.len());
                    if (2..=4).contains(&entries.len())&&display_width(&label)+10<=width {
                        out.push_str(&format!("{}{}{RESET}  {}\n",background(route_marker(entries.len()))," ".repeat(8),Role::Detail.paint(&label)));
                    }else {for part in wrap_line(&label,width){out.push_str(&Role::Detail.paint(&part));out.push('\n');}}
                }
                for index in entries {emit(&mut out,&data[index],false,index);}
            }
            return out;
        }
    }
    for (index,row) in data.iter().enumerate(){emit(&mut out,row,false,index);}
    out
}

/// Scalar measurement regions use the same feature typography as bounded
/// metrics, but do not draw a gauge without a known denominator or threshold.
pub(super) fn scalar_grid(items:&[(&str,&str,&str)],width:usize)->String{
    let mut out=Vec::new();
    let required=items.iter().map(|(label,value,source)|4+display_width(label).max(display_width(value)).max(display_width(source))).max().unwrap_or(1);
    let columns=if width>=required.saturating_mul(2).saturating_add(3){2}else{1};
    for group in items.chunks(columns){
        let cell=((width-3*(group.len()-1))/group.len()).min(required.max(32));
        if required>cell || cell<22{
            for (label,value,source) in group{
                for fragment in wrap_line(label,width){out.push(Role::Label.paint(&fragment));}
                out.push(Role::Value.paint(value));
                for fragment in wrap_line(source,width){out.push(Role::Detail.paint(&fragment));}
            }
            continue;
        }
        let mut labels=String::new();let mut reserve=String::new();let mut values=String::new();let mut sources=String::new();
        for (i,(label,value,source)) in group.iter().enumerate(){
            if i>0{for row in [&mut labels,&mut reserve,&mut values,&mut sources]{row.push_str("   ");}}
            let surface=background(if i%2==0{FEATURE_RGB}else{FEATURE_ALT_RGB});
            labels.push_str(&format!("{surface}  {LABEL}{label}{RESET}{surface}{}{RESET}"," ".repeat(cell-2-display_width(label))));
            reserve.push_str(&format!("{surface}{}{RESET}"," ".repeat(cell)));
            values.push_str(&format!("{surface}  {DETAILS}{value}{RESET}{surface}{}{RESET}"," ".repeat(cell-2-display_width(value))));
            sources.push_str(&format!("  {LABEL}{source}{RESET}{}"," ".repeat(cell-2-display_width(source))));
        }
        out.extend([labels,reserve,values,sources]);
    }
    if out.is_empty(){String::new()}else{out.join("\n")+"\n"}
}

/// Display a bounded metric once, beside its exact value, with a real
/// three-row region. Columns reflow from measured content, not command names.
pub(super) fn metric_grid(items:&[(&str,&str,Metric,f32)],width:usize)->Option<Vec<String>>{
    if items.is_empty()||width<28{return None;}
    let gutter=7;
    let gap=3;
    let required=items.iter().map(|(label,value,_,_)|gutter+2+display_width(label).max(display_width(value))).max()?;
    if required>width{return None;}
    let columns=((width+gap)/(required+gap)).clamp(1,3).min(items.len());
    let mut out=Vec::new();
    for group in items.chunks(columns){
        let cell=((width-gap*(group.len()-1))/group.len()).min(required.max(34));
        if cell<required{return None;}
        let mut labels=String::new();let mut reserve=String::new();let mut values=String::new();
        for (i,(label,value,metric,n)) in group.iter().enumerate(){
            if i>0{for row in [&mut labels,&mut reserve,&mut values]{row.push_str("   ");}}
            let token=background(metric_marker(*metric,*n));
            let gauge=format!("{token}{}{RESET}"," ".repeat(gutter));
            let surface=background(if i%2==0{FEATURE_RGB}else{FEATURE_ALT_RGB});
            let body=cell-gutter;
            let role=metric.role(*n,None);
            let tone=if role==Role::Value{Role::Detail}else{role};
            labels.push_str(&format!("{gauge}{surface} {LABEL}{label}{RESET}{surface}{}{RESET}"," ".repeat(body-1-display_width(label))));
            reserve.push_str(&format!("{gauge}{surface}{}{RESET}"," ".repeat(body)));
            values.push_str(&format!("{gauge}{surface} {}{value}{RESET}{surface}{}{RESET}",tone.ansi()," ".repeat(body-1-display_width(value))));
        }
        out.extend([labels,reserve,values]);
    }
    Some(out)
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

/// Security result layout uses explicit counts supplied by the producer,
/// never invents or extrapolates findings from the displayed subset.
pub(super) fn counted_summary(text:&str,width:usize)->Option<String>{
    let mut lines=text.lines();
    let title=lines.next()?;
    let mut rest:Vec<_>=lines.filter(|l|!underline(l)).collect();
    let index=rest.iter().position(|l|is_status_field(l))?;
    let (_,state)=rest.remove(index).split_once(": ")?;
    let (label,detail)=state.split_once(": ").unwrap_or((state,""));
    let tone=status_color(label).unwrap_or(VALUE);
    let mut out=format!("{HEADING}{title}{RESET}\n");
    out.push_str(&feature_grid(&[("RESULTADO",label,tone)],width));
    if !detail.is_empty(){for line in wrap_line(detail,width){out.push_str(&Role::Detail.paint(&line));out.push('\n');}}
    if let Some(index)=rest.iter().position(|l|l.starts_with("Conteo: ")){
        let line=rest.remove(index);
        let counts:Vec<_>=line.strip_prefix("Conteo: ")?.split(" · ").filter_map(|p|p.split_once(": ")).collect();
        let styled=counts.iter().map(|(name,value)|{
            let tone=match *name{"Críticos" if *value!="0"=>BAD,"Sospechosos"|"Revisar" if *value!="0"=>CAUTION,_=>VALUE};
            format!("{LABEL}{name}{RESET} {tone}{value}{RESET}")
        }).collect::<Vec<_>>();
        if display_width(line)<=width{out.push_str(&styled.join("   │   "));out.push('\n');}
        else{for item in styled{out.push_str(&item);out.push('\n');}}
    }
    for line in rest{
        if line.trim().is_empty(){out.push('\n');continue;}
        for fragment in wrap_line(line,width){out.push_str(&style_line(&fragment));out.push('\n');}
    }
    Some(out)
}

/// Compact primary heading, with no full-width banner or extra display rows.
/// These two reserved rows are transparent layout markers, not background art.
pub(super) fn headline(text:&str,width:usize)->Vec<String>{
    let body=text.trim();
    if display_width(body)>width.saturating_sub(3){return vec![Role::Title.paint(body)];}
    vec![format!("{HEADER}  {HEADING}{body}{RESET}")]
}

/// Final composition pass. Only the first document title gets display
/// marker; section headings get compact typographic hierarchy. Existing
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
            out.extend(headline(line,width));
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

#[cfg(test)]
mod responsive_editorial_tests{
    use super::*;
    #[test]
    fn metadata_reflows_as_a_grid_without_kpi_decoration(){
        let rows=["Nombre: equipo","Modelo: estación","Dominio: oficina","Sistema: Windows"];
        let mut out=Vec::new();
        assert!(field_grid(&rows,96,&mut out));
        assert_eq!(out.len(),2);
        let plain=report_text(&out[0]);
        assert!(plain.contains("Nombre:")&&plain.contains("Modelo:"));
        assert!(out.iter().all(|line|display_width(&report_text(line))<=96));
        assert!(!field_grid(&rows,40,&mut Vec::new()));
    }
    #[test]
    fn narrow_tables_preserve_cell_boundaries_and_statuses(){
        let header="NOMBRE    ESTADO    CPU%";
        let rows=["Estación universitaria de laboratorio    ACTIVO    65"];
        for width in [24,38,96]{
            let out=table(header,&rows,width);
            assert!(out.contains(GOOD)&&out.contains(BAD));
            assert!(report_text(&out).lines().all(|line|display_width(line)<=width));
            assert!(report_text(&out).contains("65"));
        }
    }
}
