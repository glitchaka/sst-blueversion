use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::{
        device::DeviceService,
        diagnostics::DiagnosticsService,
        domain::DomainService,
        network::NetworkService,
        switch::SwitchService,
        wol::WakeOnLanService,
    },
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

pub struct NetworkBuiltin {
    service: Arc<NetworkService>,
}

impl NetworkBuiltin {
    pub fn new(service: Arc<NetworkService>) -> Self {
        Self { service }
    }

    fn interfaces(&self) -> Result<CommandOutput> {
        self.service.execute(&["interfaces".to_owned()])
    }

    fn windows_context(&self,namespace:&str,args:&[String],_cwd:&std::path::Path)->Result<CommandOutput>{
        let words=args.iter().map(String::as_str).collect::<Vec<_>>();
        match namespace {
            "wlan"=>crate::adapters::network::wlan_native::execute(args),
            "lan"=>match words.as_slice(){
                []|["help"]|["--help"]|["?"]=>Ok(CommandOutput::ok(
                    "notsh lan — Ethernet físico por IP Helper\n\
                     notsh lan show interfaces    enlace, IP y diagnóstico físico\n\
                     Los datos de autorización cableada 802.1X requieren el backend Dot3 pendiente.\n"
                )),
                ["show","interfaces"]=>{
                    let mut result=self.service.execute(&["lan-interfaces".into()])?;
                    result.stdout=format!(
                        "LAN — WINDOWS IP HELPER\n\
                         {}\n\
                         802.1X cableado: NO VERIFICADO\n\
                         NAC: NO VERIFICADO. Enlace e IP no acreditan autorización.\n\n{}",
                        crate::adapters::network::lan_native::autoconfig_state(),
                        result.stdout);
                    Ok(result)
                },
                ["show","profiles"]|["show","settings"]=>Ok(CommandOutput::error(
                    "notsh lan: el backend Dot3/802.1X aún no está implementado; no se devolverán perfiles ficticios.",2
                )),
                _=>Ok(CommandOutput::error(
                    "notsh lan: opción nativa no implementada; usa 'notsh lan help'.",2
                )),
            },
            "interface"=>match words.as_slice(){
                []|["help"]|["--help"]|["ipv4"]|["ipv6"]=>Ok(CommandOutput::ok(
                    "notsh interface ipv4 show interfaces\nnotsh interface ipv6 show interfaces\n"
                )),
                ["ipv4","show","interfaces"]=>crate::adapters::network::ip_native::show_interfaces(false),
                ["ipv6","show","interfaces"]=>crate::adapters::network::ip_native::show_interfaces(true),
                _=>Ok(CommandOutput::error("notsh interface: opción nativa no implementada.",2)),
            },
            "advfirewall"|"winhttp"|"winsock"|"http"|"trace"=>Ok(CommandOutput::error(
                format!("notsh {namespace}: falta implementar este backend nativo. No se ejecutará netsh.exe."),2
            )),
            _=>Ok(CommandOutput::error(format!("notsh: contexto desconocido: {namespace}"),2)),
        }
    }

    fn show(&self, args: &[String]) -> Result<CommandOutput> {
        match args {
            [interfaces] if interfaces.eq_ignore_ascii_case("interfaces") => self.interfaces(),
            _ => Ok(CommandOutput::error("Uso: notsh show interfaces", 2)),
        }
    }

    /// A bare "notsh" is command discovery, not an implicit enumeration of
    /// adapters. Adapter output remains available through "notsh interfaces".
    fn show_usage(args: &[String]) -> bool {
        args.is_empty()
            || (args.len() == 1
                && matches!(args[0].as_str(), "help" | "--help" | "-h"))
    }

    fn usage() -> &'static str {
        "notsh — herramientas de red SST\n\
         \n\
         Uso: notsh SUBCOMANDO [argumentos]\n\
         \n\
         INTERFACES Y CONECTIVIDAD\n\
           notsh interfaces                 listar todos los adaptadores\n\
           notsh wlan show interfaces      estado Wi-Fi, SSID y autenticación\n\
           notsh lan show interfaces       enlace físico Ethernet; 802.1X NO verificado\n\
           notsh lan show profiles         pendiente implementación nativa Dot3\n\
           notsh wlan show profiles        perfiles inalámbricos\n\
           notsh interface ipv4 show interfaces   interfaces IPv4\n\
           notsh routes                    tabla de rutas\n\
           notsh connections               conexiones TCP/UDP y PID\n\
           notsh neighbors                 vecinos IP/MAC\n\
           notsh dns HOST|IP               resolución directa/inversa\n\
           notsh ping HOST [-c N]          eco ICMP\n\
           notsh trace HOST                ruta de red\n\
           notsh ports HOST [PUERTOS]      comprobación TCP\n\
         \n\
         DIAGNÓSTICO E INVENTARIO\n\
           notsh conflicts [IP]            posibles direcciones duplicadas\n\
           notsh scan [RED]                descubrir equipos\n\
           notsh monitor [RED]             monitor de presencia\n\
           notsh presence                  historial de presencia\n\
           notsh identify OBJETIVO         datos de equipo por IP/MAC/nombre\n\
           notsh traffic                   tráfico de red por proceso\n\
           notsh usage                     consumo por proveedor\n\
           notsh provider ...              proveedores configurados\n\
         \n\
         ALIAS\n\
           link / addr -> interfaces; route -> routes\n\
           neigh -> neighbors; ss -> connections; resolve -> dns\n\
           wifi -> wlan; net -> notsh\n\
         \n\
         'notsh' muestra este índice. 'notsh interfaces' muestra los adaptadores.\n"
    }

    fn dispatch(&self, args: &[String], cwd: &std::path::Path) -> Result<CommandOutput> {
        if Self::show_usage(args) {
            return Ok(CommandOutput::ok(Self::usage()));
        }
        let sub = args[0].as_str();
        let rest = args.get(1..).unwrap_or_default();

        match sub {
            "wlan" | "wifi" | "wireless" => self.windows_context("wlan", rest, cwd),
            "lan" | "ethernet" => self.windows_context("lan", rest, cwd),
            "interface" => self.windows_context("interface", rest, cwd),
            "ipv4" | "ipv6" => self.windows_context("interface", args, cwd),
            "advfirewall" | "winhttp" | "winsock" | "http" =>
                self.windows_context(sub, rest, cwd),
            "trace" if rest.first().is_some_and(|word| matches!(
                word.as_str(), "show" | "start" | "stop" | "convert" | "diagnose" | "help" | "?"
            )) => self.windows_context("trace", rest, cwd),
            "show" => self.show(rest),
            "link" | "addr" => self.service.execute(&["interfaces".to_owned()]),
            "route" => self.service.execute(&["routes".to_owned()]),
            "neigh" | "neighbor" => self.service.execute(&["neighbors".to_owned()]),
            "ss" | "socket" | "sockets" => self.service.execute(&["connections".to_owned()]),
            "resolve" => {
                let mut mapped = Vec::with_capacity(rest.len() + 1);
                mapped.push("dns".to_owned());
                mapped.extend(rest.iter().cloned());
                self.service.execute(&mapped)
            }
            _ => self.service.execute(args),
        }
    }
}

impl BuiltinCommand for NetworkBuiltin {
    fn name(&self) -> &'static str {
        "notsh"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["net"]
    }

    fn help(&self) -> &'static str {
        "notsh — diagnóstico, configuración y herramientas de red de SST (alias: net)"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        self.dispatch(args, context.cwd)
    }
}


#[cfg(test)]
mod notsh_usage_tests {
    use super::NetworkBuiltin;

    #[test]
    fn bare_notsh_displays_command_index_instead_of_wifi_listing() {
        assert!(NetworkBuiltin::show_usage(&[]));
        assert!(NetworkBuiltin::show_usage(&["help".to_owned()]));
        assert!(NetworkBuiltin::show_usage(&["--help".to_owned()]));
        assert!(!NetworkBuiltin::show_usage(&["interfaces".to_owned()]));
        assert!(!NetworkBuiltin::show_usage(&["wlan".to_owned()]));
    }

    #[test]
    fn command_index_exposes_the_full_network_surface() {
        let usage = NetworkBuiltin::usage();
        for command in [
            "notsh interfaces",
            "notsh wlan show interfaces",
            "notsh lan show interfaces",
            "notsh routes",
            "notsh neighbors",
            "notsh scan",
            "notsh monitor",
            "notsh identify",
            "notsh traffic",
            "notsh provider",
        ] {
            assert!(usage.contains(command), "Missing command: {command}");
        }
    }
}

macro_rules! service_builtin {
    ($type:ident, $name:literal, $service:ty, $field:ident, $help:literal) => {
        pub struct $type {
            $field: Arc<$service>,
        }

        impl $type {
            pub fn new($field: Arc<$service>) -> Self {
                Self { $field }
            }
        }

        impl BuiltinCommand for $type {
            fn name(&self) -> &'static str {
                $name
            }

            fn help(&self) -> &'static str {
                $help
            }

            fn execute(
                &self,
                _invoked_name: &str,
                args: &[String],
                _context: CommandContext<'_>,
            ) -> Result<CommandOutput> {
                self.$field.execute(args)
            }
        }
    };
}

service_builtin!(
    DeviceBuiltin,
    "device",
    DeviceService,
    service,
    "device — inventario local de equipos por MAC"
);
service_builtin!(
    DomainBuiltin,
    "domain",
    DomainService,
    service,
    "domain — pertenencia a dominio local o remota"
);
service_builtin!(
    SwitchBuiltin,
    "switch",
    SwitchService,
    service,
    "switch — resolución MAC → switch → puerto mediante SNMP"
);
service_builtin!(
    WakeOnLanBuiltin,
    "wol",
    WakeOnLanService,
    service,
    "wol — envía Magic Packet por MAC o nombre inventariado"
);
service_builtin!(
    DiagnosticsBuiltin,
    "diag",
    DiagnosticsService,
    service,
    "diag — diagnósticos compuestos de red, hardware, almacenamiento y dominio"
);
