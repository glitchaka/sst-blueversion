//! Shared, local completion grammar for SST functional commands.
//! No network requests, privileged queries, or secret completion on Tab.
const ROUTES: &[(&str, &str)] = &[
    ("sys", "info processes top disks memory sensors hostname whoami uname uptime services startup inspect why diff suspicious safe persistence suspend resume kill users printers printer drivers events registry tasks domain autologon fetch keygen help"),
    ("sys domain", "status join leave help"), ("sys autologon", "enable disable --help"),
    ("sys safe", "list remove"), ("sys registry", "query"),
    ("notsh", "interfaces lan wlan interface show ipv4 ipv6 winhttp winsock advfirewall http routes connections neighbors dns ping trace ports test conflicts scan monitor presence identify traffic usage provider help"),
    ("notsh lan", "show help"), ("notsh lan show", "interfaces profiles"),
    ("notsh wlan", "show help"), ("notsh wlan show", "interfaces profiles networks"),
    ("notsh interface", "ipv4 ipv6"), ("notsh interface ipv4", "show"), ("notsh interface ipv6", "show"),
    ("notsh interface ipv4 show", "interfaces"), ("notsh interface ipv6 show", "interfaces"),
    ("notsh ipv4", "show"), ("notsh ipv6", "show"), ("notsh ipv4 show", "interfaces"), ("notsh ipv6 show", "interfaces"),
    ("notsh show", "interfaces lan wlan wifi"), ("notsh show lan", "interfaces"), ("notsh show wlan", "interfaces"), ("notsh show wifi", "interfaces"),
    ("notsh winhttp", "show"), ("notsh winhttp show", "proxy"), ("notsh winsock", "show"), ("notsh winsock show", "catalog"),
    ("notsh advfirewall", "show"), ("notsh advfirewall show", "allprofiles currentprofile domainprofile privateprofile publicprofile"),
    ("notsh http", "show"), ("notsh http show", "iplisten urlacl"), ("notsh trace", "show"), ("notsh trace show", "sessions"),
    ("notsh test", "full speed loss stability traffic inspect packets capture help"),
    ("notsh provider", "list add remove use current path capabilities"),
    ("device", "list show add remove unknown path help"), ("domain", "status --help"),
    ("switch", "list show add remove locate capabilities path help"),
    ("intel", "status sources update lookup help"),
    ("tools", "list office --help"), ("tools office", "diagnose auth scrub scrub-current"),
    ("tools office auth", "repair"), ("tools office scrub", "all m365 2024 2021 2019 2016 2013 2010 2007"), ("tools office scrub-current", "all m365 2024 2021 2019 2016 2013 2010 2007"),
    ("instp", "search info install remove list update sst check upgrade which repo cache doctor help"),
    ("instp check", "sst"), ("instp upgrade", "sst"), ("instp repo", "list add remove update"), ("instp cache", "clean"),
    ("config", "path edit reload bg palette help"), ("config palette", "list"), ("config bg", "list carrousel carousel next off"),
    ("config bg carousel", "1 3 5 10"), ("config bg carrousel", "1 3 5 10"),
    ("demo", "graficos"), ("eventlog", "list publishers info read export clear"),
    ("service", "list status start stop pause resume restart"), ("registry", "get set set-default delete export import"),
    ("process", "list info tree kill"), ("acl", "show grant deny revoke inherit reset"),
    ("pnp", "list info enable disable restart scan"), ("task", "list show run end enable disable delete"),
    ("session", "list users logoff message"), ("share", "list sessions files add remove"),
    ("firewall", "status rules rule enable disable"), ("firewall enable", "domain private public all"), ("firewall disable", "domain private public all"),
    ("power", "shutdown restart logoff hibernate"), ("autologon", "enable disable --help"),
    ("sudo", "--status --system --trustedinstaller --help"),
    ("guide", "search"), ("unban", "--help"), ("triage", "help --find --history --deep --memory --ram --scan --refresh --status --thermal --csv --json --help"),
];
const OPTIONS: &[(&str, &str)] = &[
    ("sys uname", "-a"), ("sys services", "--running --stopped --impact"), ("sys users", "--domain --profiles"),
    ("sys printers", "--default"), ("sys printer", "--ip"), ("sys drivers", "--verbose --signed --pnp --devices --csv"),
    ("sys inspect", "--deep --broker --start-time"), ("sys kill", "--tree -t --broker --start-time"),
    ("sys suspend", "--start-time"), ("sys resume", "--start-time"),
    ("sys events", "--log --count --query --format --logs --publishers"),
    ("sys registry", "--value --default --recursive --find --keys --data"), ("sys tasks", "--verbose --csv --xml"),
    ("sys domain join", "-d --domain -u --user -ou --ou -p --password"), ("sys domain leave", "-u --user -p --password"),
    ("sys autologon", "-u --user -d --domain -p --password"), ("autologon", "-u --user -d --domain -p --password"),
    ("sys fetch", "--small --full --plain --stdout --help"), ("notsh ping", "-c"), ("notsh conflicts", "--seconds -s --json"),
    ("notsh test", "--port"), ("notsh test speed", "--passive --seconds --max-mib --download --upload"),
    ("notsh test loss", "--count --interval --timeout --seconds --tcp"), ("notsh test stability", "--count --interval --timeout --seconds --tcp"),
    ("notsh test capture", "--interface --seconds --limit --payload-bytes"),
    ("notsh test traffic", "--seconds --pid --top"), ("notsh test inspect", "--seconds --pid --top"), ("notsh test packets", "--seconds --pid --top"),
    ("notsh scan", "--unknown --authorized --known --names --json --csv"), ("notsh monitor", "--unknown -m --message --say"),
    ("notsh presence", "--json --csv"), ("notsh identify", "--json"),
    ("notsh traffic", "--watch -w --top --pid --process --background --high-usage --unsigned --connections --json --csv"),
    ("notsh usage", "--watch -w --top --device --mac --json --csv"),
    ("notsh provider add", "--host --type --user-env --secret-env --community-env"), ("notsh provider list", "--json"),
    ("device list", "--json --csv"), ("device unknown", "--json --csv"), ("device add", "--note"),
    ("switch list", "--json"), ("switch add", "--host --community-env --description"), ("switch locate", "--switch --vlan --json"),
    ("tools office diagnose", "--detail"), ("instp install", "-p --persistent"),
    ("triage", "--deep --memory --ram --scan --refresh --history --find --help"),
    ("eventlog read", "--count --query --xml"), ("eventlog export", "--query --overwrite"), ("eventlog clear", "--backup"),
    ("service list", "--running --stopped"), ("registry get", "--value --default --recursive"), ("registry set", "--type"), ("registry set-default", "--type"),
    ("registry delete", "--value --default --key"), ("registry export", "--overwrite"),
    ("process kill", "--tree --force"), ("pnp list", "--connected --disconnected --problem"),
    ("task list", "--verbose --csv"), ("task show", "--xml"),
    ("power shutdown", "--force"), ("power restart", "--force"), ("power logoff", "--force"),
];

/// Return the start of the active shell segment, ignoring quoted operators.
pub(crate) fn segment_start(line: &str) -> usize {
    let (mut quote, mut escape, mut start) = (None, false, 0);
    for (i,ch) in line.char_indices() {
        if escape { escape=false; continue; }
        if ch == '\\' && quote != Some('\'') { escape=true; continue; }
        if let Some(q)=quote { if ch==q { quote=None; } continue; }
        if matches!(ch, '\'' | '"') { quote=Some(ch); continue; }
        if matches!(ch, ';' | '|' | '&' | '(' | ')' | '\n') { start=i+ch.len_utf8(); }
    }
    start
}

fn normalized(words: &[&str]) -> Vec<String> {
    let mut parts: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
    if let Some(first)=parts.first_mut() {
        let name=match first.as_str() {
            "net" => "notsh", "tool" | "utils" => "tools", "sst-config" => "config", "runas" => "sudo", _ => first.as_str()
        }.to_owned();
        *first=name;
    }
    if parts.first().is_some_and(|p| matches!(p.as_str(), "uname" | "fetch" | "neofetch" | "fastfetch")) {
        if parts[0]!="uname" { parts[0]="fetch".into(); }
        parts.insert(0,"sys".into());
    }
    if parts.first().is_some_and(|p|p=="sudo") && parts.len()>1 {
        let skip=if matches!(parts[1].as_str(),"--system"|"--trustedinstaller"){2}else{1};
        if parts.len()>skip {
            return normalized(&words[skip..]);
        }
    }
    if parts.first().is_some_and(|p|p=="notsh") && parts.len()>1 {
        parts[1]=match parts[1].as_str() { "wifi" | "wireless" => "wlan", "ethernet"=>"lan", "traceroute"=>"trace", _=>parts[1].as_str() }.to_owned();
    }
    parts
}

/// Complete only the active argument. The caller keeps filename and programmable
/// completion available when no SST grammar applies.
pub(crate) fn candidates(before: &str) -> Option<(usize, Vec<String>)> {
    let offset=segment_start(before);
    let segment=&before[offset..];
    let start=segment.rfind(char::is_whitespace).map_or(0,|i|i+segment[i..].chars().next().unwrap().len_utf8());
    let prefix=&segment[start..];
    let words:Vec<&str>=segment[..start].split_whitespace().collect();
    if words.is_empty() {return None;}
    let parts=normalized(&words);
    let path=parts.join(" ");
    let mut values=Vec::new();
    if path=="config palette" { values.extend(crate::core::palettes::names()); }
    if matches!(path.as_str(),"sudo"|"sudo --system"|"sudo --trustedinstaller") {
        values.extend(ROUTES.iter().map(|(route,_)|*route).filter(|r|!r.contains(' ')));
        values.extend(["wol","unban","whoami","neofetch","sigsour"]);
    }
    for &(route, choices) in ROUTES { if path==route { values.extend(choices.split_whitespace()); } }
    let mut option_depth=0;
    for &(route, _) in OPTIONS {
        if path==route || path.strip_prefix(route).is_some_and(|s|s.starts_with(' ')) {
            if route.len()>option_depth { option_depth=route.len(); }
        }
    }
    // Do not propose another flag as the value of a flag taking an argument.
    let value_flags=["--count","--interval","--timeout","--seconds","--tcp","--port","-c","--pid","--top","--limit","--payload-bytes","--interface","--download","--upload","--max-mib","--host","--type","--user-env","--secret-env","--community-env","--description","--switch","--vlan","--note","--log","--query","--format","--value","--find","--start-time","--close","--device","--mac","--process","--backup","-u","--user","-d","--domain","-ou","--ou","-m","--message","--say"];
    let last=parts.last().map(String::as_str).unwrap_or("");
    if last=="--type" && path.starts_with("notsh provider add ") {
        values.extend(["generic","openwrt","opnsense","pfsense","unifi","snmp"]);
    } else if last=="--format" && path.starts_with("sys events ") {
        values.extend(["text","xml"]);
    } else if !value_flags.contains(&last) {
        for &(route,choices) in OPTIONS {
            if route.len()==option_depth && (path==route || path.strip_prefix(route).is_some_and(|s|s.starts_with(' '))) {
                values.extend(choices.split_whitespace());
            }
        }
        if parts.first().is_some_and(|s|s=="triage") && parts.get(1).is_some_and(|s|s.parse::<u32>().is_ok()) && parts.len()==2 {
            values.extend(["--plan","--close","--deep"]);
        }
    }
    let mut out:Vec<String>=values.into_iter().filter(|v|!parts.iter().any(|p|p.as_str()==*v)&&v.starts_with(&prefix.to_ascii_lowercase())).map(str::to_owned).collect();
    if path=="guide" || path.starts_with("guide ") && !path.starts_with("guide search") {
        let index:serde_json::Value=serde_json::from_str(include_str!("../../../guide/guide-index.json")).ok()?;
        let topic=parts[1..].join(" ");
        for entry in index.get("entries")?.as_array()? {
            let Some(name)=entry.get("name").and_then(|x|x.as_str()) else {continue;};
            let rest=if topic.is_empty(){name}else if let Some(rest)=name.strip_prefix(&topic).and_then(|x|x.strip_prefix(' ')){rest}else{continue;};
            if let Some(next)=rest.split_whitespace().next().filter(|n|n.starts_with(&prefix.to_ascii_lowercase())) {out.push(next.into());}
        }
    }
    out.sort();out.dedup();
    if out.is_empty(){None}else{Some((offset+start,out))}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_command_and_option_candidates() {
        for (input,expected) in [
            ("sys ser","services"),("sys services --r","--running"),("net lan sh","show"),
            ("notsh interface ipv4 show int","interfaces"),("tools office auth r","repair"),
            ("tools office diagnose --d","--detail"),("instp upgrade s","sst"),
            ("config palette sun","sunset"),("config palette mon","mondongo"),("config palette ind","industrial"),("triage 123 --p","--plan"),("guide tools office d","diagnose"),
            ("echo hi; notsh wlan show p","profiles"),("sudo sys services --r","--running"),("sudo --system sy","sys"),("sys domain join -d example -u user -","-p"),
        ] {
            let (start,choices)=candidates(input).unwrap_or_else(||panic!("No candidates for {input}"));
            assert!(choices.contains(&expected.to_owned()),"{input}: {choices:?}");
            assert!(input.is_char_boundary(start));
        }
        assert!(candidates("sys domain join -d ").is_none());
        assert!(candidates("notsh lan show sett").is_none());
    }
}
