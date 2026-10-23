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
        "notsh — red local unificada de SST\n\
\n\
Sintaxis compatible con netsh:\n\
  notsh wlan show interfaces   estado Wi-Fi sin consultar SSID/BSSID\n\
  notsh lan show interfaces    estado Ethernet/cableado\n\
  notsh show interfaces        todas las interfaces\n\
\n\
Sintaxis corta:\n\
  notsh link                   estado/enlace de interfaces\n\
  notsh addr                   direcciones e interfaces\n\
  notsh route                  tabla de rutas\n\
  notsh neigh                  vecinos IP/MAC\n\
  notsh ss                     sockets TCP/UDP y PID propietario\n\
  notsh wifi                   estado de adaptadores Wi-Fi\n\
  notsh dns HOST|IP            resolución DNS\n\
  notsh ping HOST [-c N]       eco ICMP\n\
  notsh trace HOST             trazado de ruta\n\
  notsh ports HOST [LISTA]     prueba de conectividad TCP\n\
  notsh conflicts [IP]         observa cambios de propietario ARP\n\
  notsh diagnose               resumen de interfaces, vecinos y sockets\n\
\n\
notsh usa APIs/servicios de SST. No ejecuta netsh.exe para consultar interfaces.\n"
    }

    fn interfaces(&self) -> Result<CommandOutput> {
        self.service.execute(&["interfaces".to_owned()])
    }

    fn filter_interfaces(&self, interface_type: &str, label: &str) -> Result<CommandOutput> {
        let output = self.interfaces()?;
        if output.status != 0 {
            return Ok(output);
        }

        let needle = format!("Tipo: {interface_type}");
        let mut filtered = String::new();
        for block in output.stdout.split("\n\n") {
            if block.contains(&needle) {
                if !filtered.is_empty() {
                    filtered.push('\n');
                }
                filtered.push_str(block.trim_end());
                filtered.push('\n');
            }
        }

        if filtered.is_empty() {
            return Ok(CommandOutput::error(
                format!("notsh {label}: no se encontró una interfaz {interface_type}"),
                1,
            ));
        }

        Ok(CommandOutput::ok(filtered))
    }

    fn wlan(&self, args: &[String]) -> Result<CommandOutput> {
        match args {
            [show, interfaces] if show.eq_ignore_ascii_case("show") && interfaces.eq_ignore_ascii_case("interfaces") => {
                let mut output = self.filter_interfaces("Wi-Fi", "wlan")?;
                if output.status == 0 {
                    output.stdout.push_str(
                        "\nSSID/BSSID no se consultan: Windows protege esos datos detrás del permiso de ubicación.\n",
                    );
                }
                Ok(output)
            }
            [] => self.filter_interfaces("Wi-Fi", "wlan"),
            _ => Ok(CommandOutput::error(
                "Uso: notsh wlan show interfaces",
                2,
            )),
        }
    }

    fn lan(&self, args: &[String]) -> Result<CommandOutput> {
        match args {
            [show, interfaces] if show.eq_ignore_ascii_case("show") && interfaces.eq_ignore_ascii_case("interfaces") => {
                self.filter_interfaces("Ethernet", "lan")
            }
            [] => self.filter_interfaces("Ethernet", "lan"),
            _ => Ok(CommandOutput::error(
                "Uso: notsh lan show interfaces",
                2,
            )),
        }
    }

    fn show(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str) {
            Some("interfaces") => self.interfaces(),
            _ => Ok(CommandOutput::error(
                "Uso: notsh show interfaces",
                2,
            )),
        }
    }

    fn dispatch(&self, sub: &str, rest: &[String]) -> Result<CommandOutput> {
        match sub {
            "wlan" | "wifi" | "wireless" => return self.wlan(rest),
            "lan" | "ethernet" => return self.lan(rest),
            "show" => return self.show(rest),
            "diagnose" | "diag" => return self.service.diagnose(),
            _ => {}
        }

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
        "notsh — interfaz de red SST compatible con netsh y herramientas Unix"
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
