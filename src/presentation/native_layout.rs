//! Formatter-owned native composition. The GUI only projects these cell-space marks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::presentation) struct ReportGraphic {
    pub row: u16, pub rows: u16, pub start: u16, pub end: u16, pub kind: i32, pub ratio: f32, pub tone: i32,
}


fn screen_row_text(screen:&vt100::Screen,row:u16,cols:u16)->String {
    (0..cols).filter_map(|c|screen.cell(row,c)).map(|cell|{
        let value=cell.contents(); if value.is_empty() {" ".to_owned()}else{value.to_owned()}
    }).collect()
}

fn network_report_graphics(screen:&vt100::Screen)->Vec<ReportGraphic>{
    let (rows,cols)=screen.size();
    let mut result=Vec::new();
    let mut samples:Vec<bool>=Vec::new();
    let mut ping=false;
    let mut native=false;
    let mut routes=false;
    let mut groups:std::collections::BTreeMap<(std::net::IpAddr,u32),Vec<(u16,u16)>>=std::collections::BTreeMap::new();
    let flush=|out:&mut Vec<ReportGraphic>,samples:&mut Vec<bool>,anchor:Option<(u16,u16)>|{
        if let Some((row,end))=anchor {
            let available=cols.saturating_sub(end.saturating_add(16));
            let count=samples.len();
            if count>=2 && count<=24 && usize::from(available)>=count {
                out.push(ReportGraphic{row,rows:1,start:end+3,end:end+15,kind:18,ratio:0.,tone:0});
                let step=(available/(count as u16)).clamp(1,2);
                for (i,lost) in samples.iter().enumerate(){
                    let start=end+16+i as u16*step;
                    out.push(ReportGraphic{row,rows:1,start,end:start+step,kind:10,ratio:0.,tone:if *lost{2}else{0}});
                }
            }
        }
        samples.clear();
    };
    let flush_routes=|out:&mut Vec<ReportGraphic>,groups:&mut std::collections::BTreeMap<(std::net::IpAddr,u32),Vec<(u16,u16)>>|{
        for entries in groups.values(){
            if entries.len()<2 {continue;}
            if let Some(&(row,end))=entries.last(){
                if cols.saturating_sub(end)>=18 {
                    // Retain the underlying routing rows; no unexplained
                    // count or topology is appended to an individual route.
                    let _=(&out,row);
                }
            }
        }
        groups.clear();
    };
    for row in 0..rows{
        let raw=screen_row_text(screen,row,cols);
        let line=raw.trim();
        let lower=line.to_lowercase();
        let end=(0..cols).rev().find(|&c|screen.cell(row,c).is_some_and(|v|!v.contents().trim().is_empty())).unwrap_or(0);
        let new_ping=lower.starts_with("haciendo ping a ")||lower.starts_with("pinging ")||line.starts_with("PING ");
        if new_ping {
            samples.clear();
            ping=true;native=line.starts_with("PING ");
        } else if line.starts_with("$ ") || line.starts_with("$") {
            samples.clear();
            ping=false; native=false;
        } else if ping {
            let ok=(lower.starts_with("respuesta desde ")||lower.starts_with("reply from ")) && lower.contains("ttl=") && lower.contains("bytes=");
            let native_ok=native && line.split_once(": ").is_some_and(|(ip,value)|ip.parse::<std::net::IpAddr>().is_ok()&&value.strip_suffix(" ms").is_some_and(|n|n.parse::<u64>().is_ok()));
            let lost=lower.contains("tiempo de espera agotado")||lower.contains("request timed out")||lower.contains("destino inaccesible")||lower.contains("destination host unreachable")
                ||(native&&lower.contains(": sin respuesta (estado "));
            if ok||native_ok||lost {
                samples.push(lost);
            } else if lower.starts_with("estadísticas de ping")||lower.starts_with("ping statistics")||lower.starts_with("enviados:") {
                flush(&mut result,&mut samples,Some((row,end)));ping=false;native=false;
            }
        }
        if line.starts_with("DESTINATION")&&line.contains("GATEWAY")&&line.contains("IFINDEX")&&line.contains("METRIC") {
            flush_routes(&mut result,&mut groups); routes=true;
        }else if routes{
            let fields=line.split_whitespace().collect::<Vec<_>>();
            if fields.len()==4 && fields[0].split_once('/').is_some_and(|(ip,prefix)|ip.parse::<std::net::IpAddr>().is_ok()&&prefix.parse::<u8>().is_ok()){
                if let (Ok(gateway),Ok(index),Ok(_))=(fields[1].parse::<std::net::IpAddr>(),fields[2].parse::<u32>(),fields[3].parse::<u32>()){
                    groups.entry((gateway,index)).or_default().push((row,end));continue;
                }
            }
            if !line.is_empty(){flush_routes(&mut result,&mut groups);routes=false;}
        }
    }
    flush_routes(&mut result,&mut groups);
    result
}

fn feature_dividers(screen:&vt100::Screen)->Vec<ReportGraphic>{
    use crate::presentation::formatter::{FEATURE_RGB,FEATURE_ALT_RGB};
    let (rows,cols)=screen.size();
    let surface=|r,c|screen.cell(r,c).and_then(|cell|match cell.bgcolor(){vt100::Color::Rgb(a,b,d)=>Some((a,b,d)),_=>None});
    let mut marks=Vec::new();
    for row in 0..rows.saturating_sub(2){
        let a=(0..cols).rev().find(|&c|surface(row,c)==Some(FEATURE_RGB));
        let b=(0..cols).find(|&c|surface(row,c)==Some(FEATURE_ALT_RGB));
        if let (Some(a),Some(b))=(a,b){
            if b>a+1 && surface(row+1,a)==Some(FEATURE_RGB) && surface(row+2,a)==Some(FEATURE_RGB)
                && surface(row+1,b)==Some(FEATURE_ALT_RGB) && surface(row+2,b)==Some(FEATURE_ALT_RGB)
                && (0..cols).any(|c|screen.cell(row,c).is_some_and(|v|!v.contents().trim().is_empty()))
                && (0..cols).all(|c|screen.cell(row+1,c).is_none_or(|v|v.contents().trim().is_empty())) {
                marks.push(ReportGraphic{row,rows:3,start:a+1,end:b,kind:14,ratio:0.,tone:0});
            }
        }
    }
    marks
}

// Instrument the native process monitor, not the top island or other apps.
// Decode formatter-owned component roles, independent of the originating
// command, language, labels, and number of fields.
pub(in crate::presentation) fn semantic_components(screen:&vt100::Screen)->Vec<ReportGraphic>{
    use crate::presentation::formatter::{EXPANSION_RGB,GROUP_FIELDS_RGB};
    let (rows,cols)=screen.size();
    let surface=|r,c|screen.cell(r,c).and_then(|cell|match cell.bgcolor(){vt100::Color::Rgb(a,b,d)=>Some((a,b,d)),_=>None});
    let mut marks=Vec::new();
    // Each region is explicitly reserved by the formatter. The exact same
    // component is selected for foreground reports and live monitors.
    for row in 0..rows {
        let mut col=0;
        while col<cols {
            let Some(rgb)=surface(row,col)else{col+=1;continue;};
            let Some((metric,value))=super::decode_metric_marker(rgb)else{col+=1;continue;};
            let start=col;
            while col<cols&&surface(row,col)==Some(rgb){col+=1;}
            if row>0&&surface(row-1,start)==Some(rgb){continue;}
            let mut height=1;
            while row+height<rows&&(start..col).all(|c|surface(row+height,c)==Some(rgb)){height+=1;}
            if height<2||col-start<4{continue;}
            let kind=match metric{super::Metric::Cpu=>15,super::Metric::Utilization=>16,super::Metric::Loss=>19};
            let tone=match metric.role(value,None){super::Role::Failure=>2,super::Role::Warning=>1,_=>0};
            marks.push(ReportGraphic{row,rows:height,start,end:col,kind,ratio:value/100.,tone});
        }
    }
    let mut row=0;
    while row<rows {
        if cols>=6 && surface(row,1)==Some(EXPANSION_RGB) {
            let end=(1..cols).take_while(|&c|surface(row,c)==Some(EXPANSION_RGB)).last().unwrap_or(1)+1;
            marks.push(ReportGraphic{row,rows:1,start:2,end,kind:17,ratio:0.,tone:0});
        }
        if let Some(col)=(0..cols.saturating_sub(2)).find(|&c|surface(row,c)==Some(GROUP_FIELDS_RGB)&&surface(row,c+1)==Some(GROUP_FIELDS_RGB)) {
            let first=row;
            while row<rows && surface(row,col)==Some(GROUP_FIELDS_RGB)&&surface(row,col+1)==Some(GROUP_FIELDS_RGB){row+=1;}
            if row-first>=2 {marks.push(ReportGraphic{row:first,rows:row-first,start:col+2,end:col+2,kind:1,ratio:0.,tone:0});}
        }else{row+=1;}
    }
    marks
}

pub(in crate::presentation) fn report_graphics(screen: &vt100::Screen) -> Vec<ReportGraphic> {
    use crate::presentation::formatter::{Role, REPORT_HEADER_RGB, METER_OK_RGB, METER_WARN_RGB, METER_BAD_RGB};
    let (rows, cols) = screen.size();
    let role = |r,c| screen.cell(r,c).and_then(|v| match v.fgcolor() {
        vt100::Color::Rgb(a,b,d) => Role::from_rgb((a,b,d)), _ => None,
    });
    let bg = |r,c| screen.cell(r,c).and_then(|v| match v.bgcolor() {
        vt100::Color::Rgb(a,b,d) => Some((a,b,d)), _ => None,
    });
    let has_text = |r,c| screen.cell(r,c).is_some_and(|v| !v.contents().trim().is_empty());
    let mut result = Vec::new();
    let mut group: Option<(u16,u16)> = None;
    for row in 0..rows {
        let first = (0..cols).find(|&c| has_text(row,c));
        let last = (0..cols).rev().find(|&c| has_text(row,c));
        let table = (0..cols).any(|c| bg(row,c)==Some((56,54,46)));
        let title = first.zip(last).filter(|&(a,b)| {
            (a..=b).all(|c| !has_text(row,c)||role(row,c)==Some(Role::Title))
                && (a..=b).any(|c|screen.cell(row,c).is_some_and(|v|v.contents().chars().any(char::is_alphabetic)))
        });
        if let Some((start,last)) = title {
            if table {
                result.push(ReportGraphic{row,rows:1,start,end:last.saturating_add(1),kind:3,ratio:0.0,tone:0});
            } else {
                let primary=bg(row,0)==Some(REPORT_HEADER_RGB);
                result.push(ReportGraphic{row,rows:1,start,end:last.saturating_add(1).min(cols),kind:0,ratio:0.0,tone:if primary {1}else{0}});
            }
        }

        let field = first.is_some_and(|c|c>=2)
            && !matches!(bg(row,0),Some(crate::presentation::formatter::GROUP_FIELDS_RGB))
            && first.is_some_and(|c| role(row,c)==Some(Role::Label))
            && (0..cols).any(|c| role(row,c)==Some(Role::Label) && screen.cell(row,c).is_some_and(|v|v.contents().contains(':')))
            && (0..cols).any(|c|has_text(row,c)&&matches!(role(row,c),Some(Role::Value|Role::Success|Role::Warning|Role::Failure|Role::Optimal)));
        if field {
            let start=first.unwrap_or(0);
            if let Some((_,left))=group.as_mut(){*left=(*left).min(start);} else {group=Some((row,start));}
        } else if let Some((begin,left))=group.take() {
            if row-begin>=2 {result.push(ReportGraphic{row:begin,rows:row-begin,start:left,end:left,kind:1,ratio:0.0,tone:0});}
        }
        // Only actual formatter status spans are framed; incidental prose and
        // numbers painted with warning tones are not converted into badges.
        if let Some((a,b))=first.zip(last) {
            let mut c=a;
            while c<=b {
                let Some(r)=role(row,c) else {c+=1;continue;};
                let start=c;
                while c<=b && role(row,c)==Some(r) {c+=1;}
                let text=(start..c).filter_map(|i|screen.cell(row,i)).map(|v|v.contents()).collect::<String>();
                let clean=text.trim();
                let tone=match (r,crate::presentation::formatter::status_color(clean)) {
                    (Role::Success,Some(crate::presentation::formatter::GOOD))=>Some(0),
                    (Role::Warning,Some(crate::presentation::formatter::CAUTION))=>Some(1),
                    (Role::Failure,Some(crate::presentation::formatter::BAD))=>Some(2),
                    (Role::Label|Role::Value|Role::Detail,_) if matches!(clean.to_uppercase().as_str(),"SIN DATOS"|"NO HAY DATOS"|"N/D")=>Some(3),
                    _=>None,
                };
                if let Some(tone)=tone {
                    result.push(ReportGraphic{row,rows:1,start,end:c,kind:4,ratio:0.0,tone});
                    if tone==1 && c.saturating_add(2)<cols && (c..c+2).all(|i|!has_text(row,i)) {
                        result.push(ReportGraphic{row,rows:1,start:c+1,end:c+2,kind:5,ratio:0.0,tone});
                    }
                }
            }
            let line=(a..=b).filter_map(|i|screen.cell(row,i)).map(|v|v.contents()).collect::<String>();
            if (line.trim_start().to_lowercase().starts_with("evidencia:") || line.trim_start().to_lowercase().starts_with("evidencia detallada:")) && b.saturating_add(2)<cols {
                result.push(ReportGraphic{row,rows:1,start:b+1,end:b+3,kind:6,ratio:0.0,tone:0});
            }
        }
        // Metric geometry is chosen from the producer's existing metric semantics.
        let line=screen_row_text(screen,row,cols);
        let exact_metric=line.split_once(':').and_then(|(name,value)|crate::presentation::formatter::bounded_metric(name.trim(),value.trim()));
        let metric=exact_metric.map(|(m,_)|m);
        let is_meter=|c| bg(row,c).is_some_and(|color|[(60,56,55),METER_OK_RGB,METER_WARN_RGB,METER_BAD_RGB].contains(&color)) && !has_text(row,c);
        let mut col=0;
        while col<cols {
            if !is_meter(col){col+=1;continue;}
            let start=col;
            let mut filled=0;
            let mut tone=0;
            while col<cols && is_meter(col) {
                match bg(row,col) {
                    Some(METER_OK_RGB)=>filled+=1,
                    Some(METER_WARN_RGB)=>{filled+=1;tone=tone.max(1);},
                    Some(METER_BAD_RGB)=>{filled+=1;tone=2;},
                    _=>{},
                }
                col+=1;
            }
            if col-start>=3 {
                let kind=match metric {Some(crate::presentation::formatter::Metric::Loss)=>7,Some(crate::presentation::formatter::Metric::Cpu)=>8,Some(crate::presentation::formatter::Metric::Utilization)=>9,None=>2};
                let ratio=exact_metric.map_or(filled as f32/f32::from(col-start),|(_,v)|v/100.0);
                if let Some((metric,n))=exact_metric {tone=match metric.role(n,None){Role::Failure=>2,Role::Warning=>1,_=>0};}
                result.push(ReportGraphic{row,rows:1,start,end:col,kind:if col-start<7 && matches!(kind,8|9){2}else{kind},ratio,tone});
            }
        }
    }
    if let Some((begin,left))=group {
        if rows-begin>=2 {result.push(ReportGraphic{row:begin,rows:rows-begin,start:left,end:left,kind:1,ratio:0.0,tone:0});}
    }
    if !screen.alternate_screen() {result.extend(network_report_graphics(screen));}
    result.extend(feature_dividers(screen));
    result
}

