mod diagnostics;
mod discovery;
mod identity;
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
            "connections" => self.diagnostics.connections(),
            "routes" => self.diagnostics.routes(),
            "dns" => self.diagnostics.dns(args.get(1..).unwrap_or_default()),
            "ping" => self.diagnostics.ping(args.get(1..).unwrap_or_default()),
            "trace" | "traceroute" => self.diagnostics.trace(args.get(1..).unwrap_or_default()),
            "neighbors" | "arp" => self.diagnostics.arp_table(),
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
uso: net monitor [RED] [--unknown]
abre una TUI; salir con q o Esc.
",
            Some("presence") => "net presence — consulta historial de presencia
uso: net presence [--json|--csv]
",
            Some("identify") => "net identify — identifica un dispositivo por IP, MAC o nombre inventariado
uso: net identify IP|MAC|NOMBRE [--json]
muestra MAC, tipo global/local, fabricante IEEE, inventario, método de detección y último avistamiento.
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
