use flate2::read::GzDecoder;
use std::{
    collections::HashMap,
    io::Read,
    sync::OnceLock,
};

#[derive(Debug, Clone)]
pub struct MacIdentity {
    pub scope: String,
    pub vendor: Option<String>,
    pub registry: Option<String>,
}

struct Registry {
    entries: HashMap<String, String>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

const IEEE_MAC_REGISTRY_GZ: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/ieee-mac-vendors.txt.gz"));

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

    for prefix_len in [9usize, 7, 6] {
        if let Some(vendor) = registry.entries.get(&key[..prefix_len]) {
            let registry_name = match prefix_len {
                9 => "MA-S",
                7 => "MA-M",
                _ => "MA-L",
            };
            return MacIdentity {
                scope: "global".to_owned(),
                vendor: Some(vendor.clone()),
                registry: Some(registry_name.to_owned()),
            };
        }
    }

    MacIdentity {
        scope: "global".to_owned(),
        vendor: None,
        registry: None,
    }
}

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        let mut decoder = GzDecoder::new(IEEE_MAC_REGISTRY_GZ);
        let mut text = String::new();
        decoder
            .read_to_string(&mut text)
            .expect("No se pudo descomprimir el registro IEEE de fabricantes MAC embebido");

        Registry {
            entries: parse_registry(&text),
        }
    })
}

fn parse_registry(text: &str) -> HashMap<String, String> {
    let mut entries = HashMap::new();

    for line in text.lines() {
        let Some((assignment, organization)) = line.split_once('\t') else {
            continue;
        };
        let assignment = assignment.trim().to_ascii_uppercase();
        if !matches!(assignment.len(), 6 | 7 | 9)
            || !assignment.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            continue;
        }

        let organization = organization.trim();
        if !organization.is_empty() {
            entries.insert(assignment, organization.to_owned());
        }
    }

    entries
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
