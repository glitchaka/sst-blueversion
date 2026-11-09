mod diagnostics;
mod discovery;
pub(crate) mod identity;
mod lan_note;
mod provider;
mod traffic;

use std::sync::Arc;

use anyhow::Result;

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
            "lan-interfaces" => self.diagnostics.lan_interfaces(),
            "wlan-interfaces" => self.diagnostics.wlan_interfaces(),
            "connections" => self.diagnostics.connections(),
            "routes" => self.diagnostics.routes(),
            "dns" => self.diagnostics.dns(args.get(1..).unwrap_or_default()),
            "ping" => self.diagnostics.ping(args.get(1..).unwrap_or_default()),
            "trace" | "traceroute" => self.diagnostics.trace(args.get(1..).unwrap_or_default()),
            "neighbors" | "arp" => self.diagnostics.arp_table(),
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

    fn help(&self, topic: Option<&str>) -> Result<CommandOutput> {
        let text = match topic {
            None => "net — red, descubrimiento, tráfico y proveedores\n\nuso: net SUBCOMANDO [opciones]\n\nsubcomandos:\n  interfaces              interfaces locales\n  connections             conexiones TCP/UDP y PID\n  routes                  tabla de rutas\n  dns HOST|IP             resolución DNS directa/inversa\n  ping HOST [-c N]        eco ICMP\n  trace HOST              trazado ICMP (alias: traceroute)\n  neighbors               vecinos IP/MAC (alias: arp)\n  conflicts [IP]          detecta una IPv4 observada con múltiples MAC\n  ports HOST [LISTA]      conectividad TCP a puertos\n  scan [RED] [opciones]   descubrimiento IPv4\n  monitor [RED]           monitor TUI de presencia\n  presence                historial persistente de presencia\n  identify OBJETIVO       identifica IP, MAC o nombre inventariado\n  traffic [opciones]      tráfico y conexiones por proceso\n  usage [opciones]        consumo LAN mediante proveedor activo\n  provider SUBCOMANDO     configuración de proveedores\n\nusa 'net help SUBCOMANDO' para ver opciones específicas.\n",
            Some("scan") => "net scan — descubre equipos IPv4\nuso: net scan [RED] [--unknown|--authorized|--known] [--names] [--json|--csv]\nsin RED intenta usar la red IPv4 local; el escaneo está limitado a /20 o menor.\n",
            Some("monitor") => "net monitor — monitoriza presencia de equipos\nuso: net monitor [RED] [--unknown] [-m|--message|--say TEXTO]\ndentro del monitor: Enter abre 'Mensaje>'; escribe y pulsa Enter para enviar.\ncomandos del chat:\n  ::whoisalive\n  ::room NOMBRE [CLAVE] / ::join NOMBRE [CLAVE]\n  ::leave / ::where / ::rooms\n  ::pm IP MENSAJE\n  ::fingerprint\n  ::lore / ::peers / ::clear / ::help\nlas salas con CLAVE usan Argon2id + XChaCha20-Poly1305.\nlos PM usan X25519 + XChaCha20-Poly1305 y requieren descubrir antes al peer con ::whoisalive.\nfuera del prompt, q o Esc salen; dentro del prompt, Esc cancela la edición.\n-m/--message/--say siguen disponibles para arrancar publicando un mensaje inicial.\n",
            Some("presence") => "net presence — consulta historial de presencia\nuso: net presence [--json|--csv]\n",
            Some("identify") => "net identify — identifica un dispositivo por IP, MAC o nombre inventariado\nuso: net identify IP|MAC|NOMBRE [--json]\nmuestra MAC, tipo global/local, fabricante IEEE, inventario, método de detección y último avistamiento.\n",
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
