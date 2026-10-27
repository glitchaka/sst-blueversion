mod diagnostics;
mod discovery;
pub(crate) mod identity;
mod lan_note;
mod provider;
mod traffic;

use std::{
    env,
    fs,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use snmp2::{Oid, SyncSession, Value};

use crate::core::{
    CommandOutput,
    models::switch::SwitchProfile,
};

pub use diagnostics::NetworkDiagnosticsService;
pub use discovery::NetworkDiscoveryService;
pub use provider::NetworkProviderService;
pub use traffic::NetworkTrafficService;

pub struct NetworkService {
    diagnostics: Arc<NetworkDiagnosticsService>,
    discovery: Arc<NetworkDiscoveryService>,
    traffic: Arc<NetworkTrafficService>,
    providers: Arc<NetworkProviderService>,
}

impl NetworkService {
    pub fn new(
        diagnostics: Arc<NetworkDiagnosticsService>,
        discovery: Arc<NetworkDiscoveryService>,
        traffic: Arc<NetworkTrafficService>,
        providers: Arc<NetworkProviderService>,
    ) -> Self {
        Self {
            diagnostics,
            discovery,
            traffic,
            providers,
        }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        let sub = args.first().map(String::as_str).unwrap_or("interfaces");

        if matches!(sub, "help" | "--help" | "-h") {
            return self.help(args.get(1).map(String::as_str));
        }

        match sub {
            "interfaces" => self.diagnostics.interfaces(),
            "connections" => self.diagnostics.connections(),
            "routes" => self.diagnostics.routes(),
            "dns" => self.diagnostics.dns(args.get(1..).unwrap_or_default()),
            "ping" => self.diagnostics.ping(args.get(1..).unwrap_or_default()),
            "trace" | "traceroute" => self.diagnostics.trace(args.get(1..).unwrap_or_default()),
            "neighbors" | "arp" => self.diagnostics.arp_table(),
            "mac" => self.mac(args.get(1..).unwrap_or_default()),
            "conflict" | "conflicts" | "duplicate-ip" | "duplicates" => {
                self.diagnostics.conflicts(args.get(1..).unwrap_or_default())
            }
            "ports" => self.diagnostics.ports(args.get(1..).unwrap_or_default()),
            "scan" => self.discovery.scan(args.get(1..).unwrap_or_default()),
            "monitor" => self.discovery.monitor(args.get(1..).unwrap_or_default()),
            "presence" => self.discovery.presence(args.get(1..).unwrap_or_default()),
            "identify" => self.discovery.identify(args.get(1..).unwrap_or_default()),
            "traffic" => self.traffic.execute(args.get(1..).unwrap_or_default()),
            "usage" => self.providers.usage(args.get(1..).unwrap_or_default()),
            "provider" => self.providers.execute(args.get(1..).unwrap_or_default()),
            other => Ok(CommandOutput::error(
                format!("net: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    fn mac(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error(
                "net mac: uso: net mac IPv4 [--community-env VARIABLE]",
                2,
            ));
        };
        let ip = match target.parse::<Ipv4Addr>() {
            Ok(ip) => ip,
            Err(_) => {
                return Ok(CommandOutput::error(
                    format!("net mac: IPv4 inválida: {target}"),
                    2,
                ));
            }
        };

        if let Some(mac) = self.diagnostics.resolve_neighbor(ip) {
            return Ok(CommandOutput::ok(format!(
                "IP: {ip}\nMAC: {}\nMétodo: ARP local\n",
                normalize_mac_display(&mac)
            )));
        }

        let mut errors = Vec::new();
        let switches = configured_switches().unwrap_or_else(|error| {
            errors.push(format!("perfiles de switch: {error}"));
            Vec::new()
        });

        for profile in &switches {
            match snmp_switch_ip_mac(profile, ip) {
                Ok(Some(mac)) => {
                    return Ok(CommandOutput::ok(format!(
                        "IP: {ip}\nMAC: {mac}\nOrigen: {} ({})\nMétodo: tabla ARP/IP-MIB por SNMPv2c\n",
                        profile.name, profile.host
                    )));
                }
                Ok(None) => {}
                Err(error) => errors.push(format!("{}: {error}", profile.name)),
            }
        }

        let community_env = args
            .windows(2)
            .find(|pair| pair[0] == "--community-env")
            .map(|pair| pair[1].as_str())
            .unwrap_or("SST_SNMP_COMMUNITY");
        let community = env::var(community_env).unwrap_or_else(|_| "public".to_owned());

        match snmp_mac(ip, &community) {
            Ok(Some(info)) => Ok(CommandOutput::ok(format!(
                "IP: {ip}\nMAC: {}\nInterfaz: {}\nEstado: {}\nMétodo: SNMPv2c directo al dispositivo\n",
                info.mac, info.interface, info.status
            ))),
            Ok(None) => {
                let mut message = format!(
                    "net mac: no se encontró una MAC para {ip} por ARP local, tablas ARP/IP-MIB configuradas ni SNMP directo."
                );
                if switches.is_empty() {
                    message.push_str(
                        "\nNo hay switches/gateways SNMP configurados en SST; para resolver una IP de otra VLAN agrega el equipo L3 con 'switch add'.",
                    );
                }
                if !errors.is_empty() {
                    message.push_str("\nConsultas SNMP con error:\n");
                    message.push_str(&errors.join("\n"));
                }
                Ok(CommandOutput::error(message, 1))
            }
            Err(error) => {
                errors.push(format!("dispositivo {ip}: {error}"));
                let mut message = format!(
                    "net mac: {ip} no es resoluble por ARP local y el dispositivo no respondió a SNMP."
                );
                if switches.is_empty() {
                    message.push_str(
                        "\nPara una IP en otra VLAN SST necesita consultar el switch/router L3 que mantiene su tabla ARP. Configúralo con 'switch add'.",
                    );
                }
                if !errors.is_empty() {
                    message.push_str("\nConsultas SNMP con error:\n");
                    message.push_str(&errors.join("\n"));
                }
                Ok(CommandOutput::error(message, 1))
            }
        }
    }

    fn help(&self, topic: Option<&str>) -> Result<CommandOutput> {
        let text = match topic {
            None => "net — red, descubrimiento, tráfico y proveedores\n\nuso: net SUBCOMANDO [opciones]\n\nsubcomandos:\n  interfaces              interfaces locales\n  connections             conexiones TCP/UDP y PID\n  routes                  tabla de rutas\n  dns HOST|IP             resolución DNS directa/inversa\n  ping HOST [-c N]        eco ICMP\n  trace HOST              trazado ICMP (alias: traceroute)\n  neighbors               vecinos IP/MAC (alias: arp)\n  mac IPv4                MAC del dispositivo; ARP local, switch L3 y SNMP directo\n  conflicts [IP]          detecta una IPv4 observada con múltiples MAC\n  ports HOST [LISTA]      conectividad TCP a puertos\n  scan [RED] [opciones]   descubrimiento IPv4\n  monitor [RED]           monitor TUI de presencia\n  presence                historial persistente de presencia\n  identify OBJETIVO       identifica IP, MAC o nombre inventariado\n  traffic [opciones]      tráfico y conexiones por proceso\n  usage [opciones]        consumo LAN mediante proveedor activo\n  provider SUBCOMANDO     configuración de proveedores\n\nusa 'net help SUBCOMANDO' para ver opciones específicas.\n",
            Some("scan") => "net scan — descubre equipos IPv4\nuso: net scan [RED] [--unknown|--authorized|--known] [--names] [--json|--csv]\nsin RED intenta usar la red IPv4 local; el escaneo está limitado a /20 o menor.\n",
            Some("monitor") => "net monitor — monitoriza presencia de equipos\nuso: net monitor [RED] [--unknown] [-m|--message|--say TEXTO]\ndentro del monitor: Enter abre 'Mensaje>'; escribe y pulsa Enter para enviar.\ncomandos del chat:\n  ::whoisalive\n  ::room NOMBRE [CLAVE] / ::join NOMBRE [CLAVE]\n  ::leave / ::where / ::rooms\n  ::pm IP MENSAJE\n  ::fingerprint\n  ::lore / ::peers / ::clear / ::help\nlas salas con CLAVE usan Argon2id + XChaCha20-Poly1305.\nlos PM usan X25519 + XChaCha20-Poly1305 y requieren descubrir antes al peer con ::whoisalive.\nfuera del prompt, q o Esc salen; dentro del prompt, Esc cancela la edición.\n-m/--message/--say siguen disponibles para arrancar publicando un mensaje inicial.\n",
            Some("presence") => "net presence — consulta historial de presencia\nuso: net presence [--json|--csv]\n",
            Some("identify") => "net identify — identifica un dispositivo por IP, MAC o nombre inventariado\nuso: net identify IP|MAC|NOMBRE [--json]\nmuestra MAC, tipo global/local, fabricante IEEE, inventario, método de detección y último avistamiento.\n",
            Some("mac") => "net mac — obtiene la MAC de un dispositivo IPv4\nuso: net mac IPv4 [--community-env VARIABLE]\norden de resolución:\n  1. ARP/SendARP local.\n  2. tablas ARP/IP-MIB de los switches/gateways configurados con 'switch add'.\n  3. SNMPv2c directo al dispositivo como último recurso.\npara una IP en otra VLAN, el método fiable es consultar el equipo L3 que mantiene la tabla IP↔MAC.\n",
            Some("traffic") => "net traffic — tráfico por proceso\nuso: net traffic [--watch] [--top N] [--pid PID] [--process NOMBRE] [--background] [--high-usage] [--unsigned] [--connections] [--json|--csv]\n",
            Some("usage") => "net usage — consumo de Internet de la LAN\nuso: net usage [--watch] [--top [N]] [--device IP] [--mac MAC] [--json|--csv]\nrequiere un proveedor activo que exponga contadores por cliente.\n",
            Some("provider") => "net provider — fuentes externas de telemetría\nuso:\n  net provider list [--json]\n  net provider add NOMBRE --type TIPO --host HOST [--user-env VAR] [--secret-env VAR] [--community-env VAR]\n  net provider use NOMBRE\n  net provider current\n  net provider remove NOMBRE\n  net provider capabilities [NOMBRE]\n  net provider path\ntipos: openwrt, opnsense, pfsense, unifi, snmp, generic\n",
            Some("ping") => "net ping — eco ICMP\nuso: net ping HOST [-c N]\nN: 1..100.\n",
            Some("conflict") | Some("conflicts") | Some("duplicate-ip") | Some("duplicates") => "net conflicts — detecta conflictos IPv4 por cambios de propietario ARP\nuso:\n  net conflicts\n  net conflicts IP\n  net conflicts IP --seconds 10\n  net conflicts --json\naliases: net conflict, net duplicate-ip, net duplicates\nun conflicto se marca cuando la misma IPv4 es observada con dos o más MAC durante la ventana.\nuna ejecución sin hallazgos no demuestra que no exista duplicidad; para una IP sospechosa usa una ventana mayor.\n",
            Some("ports") => "net ports — comprueba puertos TCP\nuso: net ports HOST [22,80,443,...]\n",
            Some("dns") => "net dns — resolución DNS\nuso: net dns HOST|IP\n",
            Some("trace") | Some("traceroute") => "net trace — trazado ICMP\nuso: net trace HOST\nalias: net traceroute HOST\n",
            Some("interfaces") => "net interfaces — muestra interfaces de red locales\nuso: net interfaces\n",
            Some("connections") => "net connections — muestra conexiones TCP/UDP y PID propietario\nuso: net connections\n",
            Some("routes") => "net routes — muestra la tabla de rutas\nuso: net routes\n",
            Some("neighbors") | Some("arp") => "net neighbors — muestra vecinos IP/MAC\nuso: net neighbors\nalias: net arp\n",
            Some(other) => {
                return Ok(CommandOutput::error(
                    format!("net help: subcomando desconocido: {other}"),
                    2,
                ));
            }
        };
        Ok(CommandOutput::ok(text))
    }

    pub fn diagnose(&self) -> Result<CommandOutput> {
        self.diagnostics.diagnose()
    }
}

struct SnmpMacInfo {
    mac: String,
    interface: String,
    status: String,
}

fn configured_switches() -> Result<Vec<SwitchProfile>> {
    let Some(root) = env::current_exe()?
        .parent()
        .map(std::path::Path::to_path_buf)
    else {
        return Ok(Vec::new());
    };
    let path = root.join("data").join("switches.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&text)?)
}

fn snmp_switch_ip_mac(profile: &SwitchProfile, ip: Ipv4Addr) -> Result<Option<String>> {
    let community = env::var(&profile.community_env).map_err(|_| {
        anyhow::anyhow!(
            "falta la variable de entorno {} con la comunidad SNMP",
            profile.community_env
        )
    })?;
    let address = snmp_address(&profile.host);
    let mut session = SyncSession::new_v2c(
        address.as_str(),
        community.as_bytes(),
        Some(Duration::from_millis(1500)),
        0,
    )
    .map_err(|error| anyhow::anyhow!("SNMP: {error}"))?;

    if let Some(mac) = snmp_walk_ip_mac(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 4, 35, 1, 4],
        ip,
        true,
    )? {
        return Ok(Some(mac));
    }

    snmp_walk_ip_mac(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 4, 22, 1, 2],
        ip,
        false,
    )
}

fn snmp_walk_ip_mac(
    session: &mut SyncSession,
    base_components: &[u64],
    ip: Ipv4Addr,
    modern: bool,
) -> Result<Option<String>> {
    let base = Oid::from(base_components)
        .map_err(|error| anyhow::anyhow!("OID inválido: {error:?}"))?;
    let base_len = base_components.len();
    let mut cursor = base.to_owned();
    let octets = ip.octets();
    let ipv4_suffix = [
        1_u64,
        4,
        octets[0] as u64,
        octets[1] as u64,
        octets[2] as u64,
        octets[3] as u64,
    ];

    for _ in 0..4096 {
        let mut response = session
            .getnext(&cursor)
            .map_err(|error| anyhow::anyhow!("SNMP GETNEXT: {error}"))?;
        let Some((name, value)) = response.varbinds.next() else {
            break;
        };
        if !name.starts_with(&base) {
            break;
        }

        let Some(arcs) = name.iter().map(|iter| iter.collect::<Vec<_>>()) else {
            break;
        };
        let suffix = &arcs[base_len..];
        let matches_ip = if modern {
            suffix.ends_with(&ipv4_suffix)
        } else {
            suffix.len() >= 4
                && suffix[suffix.len() - 4..]
                    == [
                        octets[0] as u64,
                        octets[1] as u64,
                        octets[2] as u64,
                        octets[3] as u64,
                    ]
        };

        if matches_ip
            && let Value::OctetString(bytes) = value
            && is_valid_unicast_mac(bytes)
        {
            return Ok(Some(format_mac_bytes(bytes)));
        }

        cursor = name.to_owned();
    }

    Ok(None)
}

fn snmp_mac(ip: Ipv4Addr, community: &str) -> Result<Option<SnmpMacInfo>> {
    let address = format!("{ip}:161");
    let mut session = SyncSession::new_v2c(
        address.as_str(),
        community.as_bytes(),
        Some(Duration::from_millis(1500)),
        0,
    )
    .map_err(|error| anyhow::anyhow!("SNMP: {error}"))?;

    let octets = ip.octets();
    let ip_ifindex_oid = [
        1_u64,
        3,
        6,
        1,
        2,
        1,
        4,
        20,
        1,
        2,
        octets[0] as u64,
        octets[1] as u64,
        octets[2] as u64,
        octets[3] as u64,
    ];

    if let Some(if_index) = snmp_get_integer(&mut session, &ip_ifindex_oid)?
        && if_index > 0
        && let Some(info) = snmp_mac_for_index(&mut session, if_index as u64)?
    {
        return Ok(Some(info));
    }

    let if_count = snmp_get_integer(&mut session, &[1, 3, 6, 1, 2, 1, 2, 1, 0])?
        .unwrap_or(0)
        .clamp(0, 256) as u64;

    let mut fallback = None;
    for if_index in 1..=if_count {
        if let Some(info) = snmp_mac_for_index(&mut session, if_index)? {
            if info.status == "up" {
                return Ok(Some(info));
            }
            fallback.get_or_insert(info);
        }
    }

    Ok(fallback)
}

fn snmp_mac_for_index(session: &mut SyncSession, if_index: u64) -> Result<Option<SnmpMacInfo>> {
    let Some(bytes) = snmp_get_octets(
        session,
        &[1, 3, 6, 1, 2, 1, 2, 2, 1, 6, if_index],
    )?
    else {
        return Ok(None);
    };

    if !is_valid_unicast_mac(&bytes) {
        return Ok(None);
    }

    let interface = snmp_get_text(
        session,
        &[1, 3, 6, 1, 2, 1, 2, 2, 1, 2, if_index],
    )?
    .unwrap_or_else(|| format!("ifIndex {if_index}"));
    let status = match snmp_get_integer(
        session,
        &[1, 3, 6, 1, 2, 1, 2, 2, 1, 8, if_index],
    )? {
        Some(1) => "up".to_owned(),
        Some(2) => "down".to_owned(),
        Some(3) => "testing".to_owned(),
        Some(4) => "unknown".to_owned(),
        Some(5) => "dormant".to_owned(),
        Some(6) => "notPresent".to_owned(),
        Some(7) => "lowerLayerDown".to_owned(),
        Some(value) => value.to_string(),
        None => "-".to_owned(),
    };

    Ok(Some(SnmpMacInfo {
        mac: format_mac_bytes(&bytes),
        interface,
        status,
    }))
}

fn snmp_get_integer(session: &mut SyncSession, components: &[u64]) -> Result<Option<i64>> {
    let oid = Oid::from(components)
        .map_err(|error| anyhow::anyhow!("OID inválido: {error:?}"))?;
    let mut response = session
        .get(&oid)
        .map_err(|error| anyhow::anyhow!("SNMP GET: {error}"))?;
    let Some((_name, value)) = response.varbinds.next() else {
        return Ok(None);
    };
    match value {
        Value::Integer(value) => Ok(Some(value)),
        Value::Unsigned32(value) => Ok(Some(value as i64)),
        Value::Counter32(value) => Ok(Some(value as i64)),
        Value::NoSuchInstance | Value::NoSuchObject | Value::EndOfMibView => Ok(None),
        other => Err(anyhow::anyhow!("SNMP devolvió un tipo inesperado: {other:?}")),
    }
}

fn snmp_get_octets(session: &mut SyncSession, components: &[u64]) -> Result<Option<Vec<u8>>> {
    let oid = Oid::from(components)
        .map_err(|error| anyhow::anyhow!("OID inválido: {error:?}"))?;
    let mut response = session
        .get(&oid)
        .map_err(|error| anyhow::anyhow!("SNMP GET: {error}"))?;
    let Some((_name, value)) = response.varbinds.next() else {
        return Ok(None);
    };
    match value {
        Value::OctetString(value) => Ok(Some(value.to_vec())),
        Value::NoSuchInstance | Value::NoSuchObject | Value::EndOfMibView => Ok(None),
        _ => Ok(None),
    }
}

fn snmp_get_text(session: &mut SyncSession, components: &[u64]) -> Result<Option<String>> {
    Ok(snmp_get_octets(session, components)?
        .map(|value| String::from_utf8_lossy(&value).trim().to_owned())
        .filter(|value| !value.is_empty()))
}

fn snmp_address(host: &str) -> String {
    if host.parse::<SocketAddr>().is_ok() {
        host.to_owned()
    } else if host.contains(':') {
        format!("[{host}]:161")
    } else {
        format!("{host}:161")
    }
}

fn is_valid_unicast_mac(bytes: &[u8]) -> bool {
    bytes.len() == 6
        && !bytes.iter().all(|byte| *byte == 0)
        && bytes[0] & 1 == 0
}

fn format_mac_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn normalize_mac_display(mac: &str) -> String {
    let compact = mac
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase();
    if compact.len() != 12 {
        return mac.to_ascii_uppercase();
    }
    compact
        .as_bytes()
        .chunks(2)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(":")
}
