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
                     notsh lan show profiles      perfil 802.1X por Windows Dot3 API; sin secretos.\n"
                )),
                ["show","interfaces"]=>{
                    let mut result=self.service.execute(&["lan-interfaces".into()])?;
                    let dot3=crate::adapters::network::dot3_native::interfaces()?;
                    result.stdout=format!(
                        "LAN — WINDOWS IP HELPER + DOT3\n\
                         {}\n\
                         Enlace e IP no acreditan autorización NAC.\n\n{}\n{}",
                        crate::adapters::network::lan_native::autoconfig_state(),
                        result.stdout,
                        if dot3.status==0 {dot3.stdout} else {dot3.stderr});
                    Ok(result)
                },
                ["show","profiles"]=>crate::adapters::network::dot3_native::profiles(),
                ["show","settings"]=>Ok(CommandOutput::error(
                    "notsh lan: opciones avanzadas de Dot3 todavía no disponibles.",2
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
            "winhttp"=>crate::adapters::network::winhttp_native::execute(args),
            "winsock"=>crate::adapters::network::winsock_native::execute(args),
            "advfirewall"=>crate::adapters::network::firewall_native::execute(args),
            "http"=>crate::adapters::network::http_native::execute(args),
            "trace"=>crate::adapters::network::trace_native::execute(args),
            _=>Ok(CommandOutput::error(format!("notsh: contexto desconocido: {namespace}"),2)),
        }
    }

    /// Both "notsh lan show interfaces" and "notsh show lan interfaces"
    /// select the same native Windows context. Neither invokes netsh.exe.
    fn show(&self, args: &[String], cwd: &std::path::Path) -> Result<CommandOutput> {
        let words = args.iter().map(String::as_str).collect::<Vec<_>>();
        match words.as_slice() {
            ["interfaces"] => self.interfaces(),
            ["lan", "interfaces"] => self.windows_context(
                "lan", &["show".into(), "interfaces".into()], cwd),
            ["wlan", "interfaces"] | ["wifi", "interfaces"] => self.windows_context(
                "wlan", &["show".into(), "interfaces".into()], cwd),
            _ => Ok(CommandOutput::error(
                "Uso: notsh show [lan|wlan] interfaces", 2)),
        }
    }

    fn csv_scope(args: &[String]) -> Option<&'static str> {
        let words = args.iter()
            .filter(|word| word.as_str() != "--csv")
            .map(String::as_str)
            .collect::<Vec<_>>();
        match words.as_slice() {
            ["interfaces"] | ["show", "interfaces"] | ["link"] | ["addr"] =>
                Some("all"),
            ["lan", "show", "interfaces"] | ["lan", "interfaces"]
              | ["show", "lan", "interfaces"] | ["ethernet", "show", "interfaces"] =>
                Some("lan"),
            ["wlan", "show", "interfaces"] | ["wlan", "interfaces"]
              | ["show", "wlan", "interfaces"] | ["wifi", "show", "interfaces"]
              | ["wifi", "interfaces"] =>
                Some("wlan"),
            _ => None,
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
           notsh lan show profiles         consulta de perfil nativo Dot3 por Ethernet\n\
           notsh winhttp show proxy       proxy WinHTTP del equipo\n\
           notsh winsock show catalog     protocolos Winsock2 visibles\n\
           notsh advfirewall show allprofiles   perfiles Windows Firewall\n\
           notsh http show iplisten      restricciones HTTP.sys\n\
           notsh http show urlacl        reservas URL HTTP.sys\n\
           notsh wlan show profiles        perfiles inalámbricos\n\
           notsh interface ipv4 show interfaces   interfaces IPv4\n\
           notsh routes                    tabla de rutas\n\
           notsh connections               conexiones TCP/UDP y PID\n\
           notsh neighbors                 vecinos IP/MAC\n\
           notsh dns HOST|IP               resolución directa/inversa\n\
           notsh ping HOST [-c N]          eco ICMP\n\
           notsh trace HOST                ruta de red\n\
           notsh trace show sessions       sesiones de seguimiento ETW (sólo lectura)\n\
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
        // Machine-readable output is produced from structured Win32 data;
        // never return human-readable text merely because a flag was supplied.
        if args.iter().any(|word| word == "--csv") {
            return match Self::csv_scope(args) {
                Some("all") => Ok(CommandOutput::ok(
                    crate::adapters::windows::network::interfaces_csv("all")?)),
                Some("lan") => Ok(CommandOutput::ok(
                    crate::adapters::windows::network::interfaces_csv("lan")?)),
                Some("wlan") => crate::adapters::network::wlan_native::interfaces_csv(),
                // Existing network scan/traffic/presence/usage exporters must
                // continue through NetworkService; they already own their CSV.
                None if matches!(args.first().map(String::as_str),
                    Some("scan" | "traffic" | "presence" | "usage")) => {
                    self.service.execute(args)
                }
                _ => Ok(CommandOutput::error(
                    "notsh: --csv no está disponible para este subcomando.", 2)),
            };
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
            "show" => self.show(rest, cwd),
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
    fn both_lan_wlan_spellings_support_csv() {
        for (args, expected) in [
            (vec!["show", "lan", "interfaces", "--csv"], "lan"),
            (vec!["lan", "show", "interfaces", "--csv"], "lan"),
            (vec!["show", "wlan", "interfaces", "--csv"], "wlan"),
            (vec!["wlan", "show", "interfaces", "--csv"], "wlan"),
            (vec!["interfaces", "--csv"], "all"),
        ] {
            let words = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(NetworkBuiltin::csv_scope(&words), Some(expected));
        }
        assert_eq!(NetworkBuiltin::csv_scope(&["routes".into()]), None);
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
