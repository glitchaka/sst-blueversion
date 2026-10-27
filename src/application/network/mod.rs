mod diagnostics;
mod discovery;
pub(crate) mod identity;
mod lan_note;
mod provider;
mod traffic;

use std::{env, net::Ipv4Addr, sync::Arc, time::Duration};

use anyhow::Result;
use snmp2::{Oid, SyncSession, Value};

use crate::core::CommandOutput;

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
                "IP: {ip}\nMAC: {}\nMétodo: ARP\n",
                normalize_mac_display(&mac)
            )));
        }

        let community_env = args
            .windows(2)
            .find(|pair| pair[0] == "--community-env")
            .map(|pair| pair[1].as_str())
            .unwrap_or("SST_SNMP_COMMUNITY");
        let community = env::var(community_env).unwrap_or_else(|_| "public".to_owned());

        match snmp_mac(ip, &community) {
            Ok(Some(info)) => Ok(CommandOutput::ok(format!(
                "IP: {ip}\nMAC: {}\nInterfaz: {}\nEstado: {}\nMétodo: SNMPv2c\n",
                info.mac, info.interface, info.status
            ))),
            Ok(None) => Ok(CommandOutput::error(
                format!(
                    "net mac: no se encontró una MAC para {ip} por ARP ni SNMP. La impresora puede tener SNMP deshabilitado o usar otra comunidad; configura {community_env}."
                ),
                1,
            )),
            Err(error) => Ok(CommandOutput::error(
                format!(
                    "net mac: {ip} no fue resoluble por ARP y la consulta SNMP falló: {error}"
                ),
                1,
            )),
        }
    }

    fn help(&self, topic: Option<&str>) -> Result<CommandOutput> {
        let text = match topic {
            None => "net — red, descubrimiento, tráfico y proveedores

uso: net SUBCOMANDO [opciones]

subcomandos:
  interfaces              interfaces locales
  connections             conexiones TCP/UDP y PID
  routes                  tabla de rutas
  dns HOST|IP             resolución DNS directa/inversa
  ping HOST [-c N]        eco ICMP
  trace HOST              trazado ICMP (alias: traceroute)
  neighbors               vecinos IP/MAC (alias: arp)
  mac IPv4                MAC del dispositivo; ARP local y SNMP remoto
  conflicts [IP]          detecta una IPv4 observada con múltiples MAC
  ports HOST [LISTA]      conectividad TCP a puertos
  scan [RED] [opciones]   descubrimiento IPv4
  monitor [RED]           monitor TUI de presencia
  presence                historial persistente de presencia
  identify OBJETIVO       identifica IP, MAC o nombre inventariado
  traffic [opciones]      tráfico y conexiones por proceso
  usage [opciones]        consumo LAN mediante proveedor activo
  provider SUBCOMANDO     configuración de proveedores

usa 'net help SUBCOMANDO' para ver opciones específicas.
",
            Some("scan") => "net scan — descubre equipos IPv4
uso: net scan [RED] [--unknown|--authorized|--known] [--names] [--json|--csv]
sin RED intenta usar la red IPv4 local; el escaneo está limitado a /20 o menor.
",
            Some("monitor") => "net monitor — monitoriza presencia de equipos
uso: net monitor [RED] [--unknown] [-m|--message|--say TEXTO]
dentro del monitor: Enter abre 'Mensaje>'; escribe y pulsa Enter para enviar.
comandos del chat:
  ::whoisalive
  ::room NOMBRE [CLAVE] / ::join NOMBRE [CLAVE]
  ::leave / ::where / ::rooms
  ::pm IP MENSAJE
  ::fingerprint
  ::lore / ::peers / ::clear / ::help
las salas con CLAVE usan Argon2id + XChaCha20-Poly1305.
los PM usan X25519 + XChaCha20-Poly1305 y requieren descubrir antes al peer con ::whoisalive.
fuera del prompt, q o Esc salen; dentro del prompt, Esc cancela la edición.
-m/--message/--say siguen disponibles para arrancar publicando un mensaje inicial.
",
            Some("presence") => "net presence — consulta historial de presencia
uso: net presence [--json|--csv]
",
            Some("identify") => "net identify — identifica un dispositivo por IP, MAC o nombre inventariado
uso: net identify IP|MAC|NOMBRE [--json]
muestra MAC, tipo global/local, fabricante IEEE, inventario, método de detección y último avistamiento.
",
            Some("mac") => "net mac — obtiene la MAC de un dispositivo IPv4
uso: net mac IPv4 [--community-env VARIABLE]
primero intenta ARP/SendARP. Si el dispositivo está en otra VLAN, consulta por SNMPv2c el ifIndex asociado a esa IP y luego ifPhysAddress.
por defecto usa SST_SNMP_COMMUNITY; si no existe, intenta la comunidad 'public'.
",
            Some("traffic") => "net traffic — tráfico por proceso
uso: net traffic [--watch] [--top N] [--pid PID] [--process NOMBRE] [--background] [--high-usage] [--unsigned] [--connections] [--json|--csv]
",
            Some("usage") => "net usage — consumo de Internet de la LAN
uso: net usage [--watch] [--top [N]] [--device IP] [--mac MAC] [--json|--csv]
requiere un proveedor activo que exponga contadores por cliente.
",
            Some("provider") => "net provider — fuentes externas de telemetría
uso:
  net provider list [--json]
  net provider add NOMBRE --type TIPO --host HOST [--user-env VAR] [--secret-env VAR] [--community-env VAR]
  net provider use NOMBRE
  net provider current
  net provider remove NOMBRE
  net provider capabilities [NOMBRE]
  net provider path
tipos: openwrt, opnsense, pfsense, unifi, snmp, generic
",
            Some("ping") => "net ping — eco ICMP
uso: net ping HOST [-c N]
N: 1..100.
",
            Some("conflict") | Some("conflicts") | Some("duplicate-ip") | Some("duplicates") => "net conflicts — detecta conflictos IPv4 por cambios de propietario ARP
uso:
  net conflicts
  net conflicts IP
  net conflicts IP --seconds 10
  net conflicts --json
aliases: net conflict, net duplicate-ip, net duplicates
un conflicto se marca cuando la misma IPv4 es observada con dos o más MAC durante la ventana.
una ejecución sin hallazgos no demuestra que no exista duplicidad; para una IP sospechosa usa una ventana mayor.
",
            Some("ports") => "net ports — comprueba puertos TCP
uso: net ports HOST [22,80,443,...]
",
            Some("dns") => "net dns — resolución DNS
uso: net dns HOST|IP
",
            Some("trace") | Some("traceroute") => "net trace — trazado ICMP
uso: net trace HOST
alias: net traceroute HOST
",
            Some("interfaces") => "net interfaces — muestra interfaces de red locales
uso: net interfaces
",
            Some("connections") => "net connections — muestra conexiones TCP/UDP y PID propietario
uso: net connections
",
            Some("routes") => "net routes — muestra la tabla de rutas
uso: net routes
",
            Some("neighbors") | Some("arp") => "net neighbors — muestra vecinos IP/MAC
uso: net neighbors
alias: net arp
",
            Some(other) => return Ok(CommandOutput::error(
                format!("net help: subcomando desconocido: {other}"),
                2,
            )),
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

    if bytes.len() != 6 || bytes.iter().all(|byte| *byte == 0) || bytes[0] & 1 != 0 {
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
    )?
    {
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
