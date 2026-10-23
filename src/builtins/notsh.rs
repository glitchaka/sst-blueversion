use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::network::NetworkService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

pub struct NotshBuiltin {
    service: Arc<NetworkService>,
}

impl NotshBuiltin {
    pub fn new(service: Arc<NetworkService>) -> Self {
        Self { service }
    }

    fn help_text() -> &'static str {
        "notsh — red local unificada al estilo de las herramientas Unix\n\
\n\
uso: notsh SUBCOMANDO [opciones]\n\
\n\
subcomandos:\n\
  link                     estado/enlace de interfaces (ip link + ethtool)\n\
  addr                     direcciones e interfaces (ip addr)\n\
  route                    tabla de rutas (ip route)\n\
  neigh                    vecinos IP/MAC (ip neigh)\n\
  ss                       sockets TCP/UDP y PID propietario (ss)\n\
  wifi                     estado de adaptadores Wi-Fi sin exigir SSID/BSSID\n\
  dns HOST|IP              resolución DNS (resolvectl)\n\
  ping HOST [-c N]         eco ICMP\n\
  trace HOST               trazado de ruta\n\
  ports HOST [LISTA]       prueba de conectividad TCP\n\
  conflicts [IP]           observa cambios de propietario ARP\n\
  diagnose                 resumen de interfaces, vecinos y sockets\n\
\n\
aliases aceptados:\n\
  interfaces -> link\n\
  routes -> route\n\
  neighbors|arp -> neigh\n\
  sockets|connections -> ss\n\
  resolve -> dns\n\
\n\
notsh usa APIs/servicios de SST; no es un wrapper de netsh.exe.\n"
    }

    fn dispatch(&self, sub: &str, rest: &[String]) -> Result<CommandOutput> {
        let mapped = match sub {
            "link" | "addr" | "interfaces" => "interfaces",
            "route" | "routes" => "routes",
            "neigh" | "neighbor" | "neighbors" | "arp" => "neighbors",
            "ss" | "socket" | "sockets" | "connections" => "connections",
            "dns" | "resolve" => "dns",
            "ping" => "ping",
            "trace" | "traceroute" => "trace",
            "ports" => "ports",
            "conflict" | "conflicts" | "duplicate-ip" | "duplicates" => "conflicts",
            "diagnose" | "diag" => return self.service.diagnose(),
            "wifi" | "wlan" | "wireless" => {
                let output = self.service.execute(&["interfaces".to_owned()])?;
                if output.status != 0 {
                    return Ok(output);
                }
                let mut wifi = String::new();
                for block in output.stdout.split("\n\n") {
                    if block.contains("Tipo: Wi-Fi") {
                        if !wifi.is_empty() {
                            wifi.push('\n');
                        }
                        wifi.push_str(block.trim_end());
                        wifi.push('\n');
                    }
                }
                if wifi.is_empty() {
                    return Ok(CommandOutput::error(
                        "notsh wifi: no se encontró un adaptador Wi-Fi",
                        1,
                    ));
                }
                wifi.push_str(
                    "\nNota: SSID/BSSID no se consultan aquí; Windows puede proteger esos datos detrás del permiso de ubicación.\n",
                );
                return Ok(CommandOutput::ok(wifi));
            }
            other => {
                return Ok(CommandOutput::error(
                    format!("notsh: subcomando desconocido: {other}\n\n{}", Self::help_text()),
                    2,
                ));
            }
        };

        let mut args = Vec::with_capacity(rest.len() + 1);
        args.push(mapped.to_owned());
        args.extend(rest.iter().cloned());
        self.service.execute(&args)
    }
}

impl BuiltinCommand for NotshBuiltin {
    fn name(&self) -> &'static str {
        "notsh"
    }

    fn help(&self) -> &'static str {
        "notsh — interfaz de red local unificada inspirada en ip/iw/nmcli/ethtool/ss/resolvectl"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        match args.first().map(String::as_str) {
            None | Some("help") | Some("--help") | Some("-h") => {
                Ok(CommandOutput::ok(Self::help_text()))
            }
            Some(sub) => self.dispatch(sub, args.get(1..).unwrap_or_default()),
        }
    }
}
