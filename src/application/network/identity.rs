use std::{
    collections::HashMap,
    sync::OnceLock,
};

#[derive(Debug, Clone)]
pub struct MacIdentity {
    pub scope: String,
    pub vendor: Option<String>,
    pub registry: Option<String>,
}

struct Registry {
    ma_l: HashMap<String, String>,
    ma_m: HashMap<String, String>,
    ma_s: HashMap<String, String>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

const IEEE_MA_L: &str = include_str!(concat!(env!("OUT_DIR"), "/ieee-ma-l.csv"));
const IEEE_MA_M: &str = include_str!(concat!(env!("OUT_DIR"), "/ieee-ma-m.csv"));
const IEEE_MA_S: &str = include_str!(concat!(env!("OUT_DIR"), "/ieee-ma-s.csv"));

pub fn identify_mac(mac: &str) -> MacIdentity {
    let Some(bytes) = parse_mac(mac) else {
        return MacIdentity {
            scope: "unknown".to_owned(),
            vendor: None,
            registry: None,
        };
    };

    if bytes == [0xff; 6] {
        return MacIdentity {
            scope: "broadcast".to_owned(),
            vendor: None,
            registry: None,
        };
    }

    if bytes[0] & 0x01 != 0 {
        return MacIdentity {
            scope: "multicast".to_owned(),
            vendor: None,
            registry: None,
        };
    }

    if bytes[0] & 0x02 != 0 {
        return MacIdentity {
            scope: "local/private".to_owned(),
            vendor: None,
            registry: None,
        };
    }

    let key = bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let registry = registry();

    if let Some(vendor) = registry.ma_s.get(&key[..9]) {
        return MacIdentity {
            scope: "global".to_owned(),
            vendor: Some(vendor.clone()),
            registry: Some("MA-S".to_owned()),
        };
    }

    if let Some(vendor) = registry.ma_m.get(&key[..7]) {
        return MacIdentity {
            scope: "global".to_owned(),
            vendor: Some(vendor.clone()),
            registry: Some("MA-M".to_owned()),
        };
    }

    if let Some(vendor) = registry.ma_l.get(&key[..6]) {
        return MacIdentity {
            scope: "global".to_owned(),
            vendor: Some(vendor.clone()),
            registry: Some("MA-L".to_owned()),
        };
    }

    MacIdentity {
        scope: "global".to_owned(),
        vendor: None,
        registry: None,
    }
}

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| Registry {
        ma_l: parse_registry(IEEE_MA_L, 6),
        ma_m: parse_registry(IEEE_MA_M, 7),
        ma_s: parse_registry(IEEE_MA_S, 9),
    })
}

fn parse_registry(text: &str, prefix_len: usize) -> HashMap<String, String> {
    let mut entries = HashMap::new();

    for line in text.lines() {
        let fields = parse_csv_line(line);
        if fields.len() < 3 {
            continue;
        }

        let assignment = fields[1]
            .trim()
            .trim_start_matches('\u{feff}')
            .chars()
            .filter(|ch| ch.is_ascii_hexdigit())
            .collect::<String>()
            .to_ascii_uppercase();

        if assignment.eq_ignore_ascii_case("Assignment")
            || assignment.len() != prefix_len
            || !assignment.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            continue;
        }

        let organization = fields[2].trim();
        if !organization.is_empty() {
            entries.insert(assignment, organization.to_owned());
        }
    }

    entries
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

fn parse_mac(mac: &str) -> Option<[u8; 6]> {
    if mac.starts_with("??") {
        return None;
    }

    let normalized = mac
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .collect::<String>();

    if normalized.len() != 12 {
        return None;
    }

    let mut bytes = [0u8; 6];
    for (index, slot) in bytes.iter_mut().enumerate() {
        let start = index * 2;
        *slot = u8::from_str_radix(&normalized[start..start + 2], 16).ok()?;
    }
    Some(bytes)
}
