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

    fn filter_interfaces(&self, interface_type: &str, context: &str) -> Result<CommandOutput> {
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
                format!("notsh {context}: no se encontró una interfaz {interface_type}"),
                1,
            ));
        }

        Ok(CommandOutput::ok(filtered))
    }

    fn wlan(&self, args: &[String]) -> Result<CommandOutput> {
        match args {
            [] => self.filter_interfaces("Wi-Fi", "wlan"),
            [show, interfaces]
                if show.eq_ignore_ascii_case("show")
                    && interfaces.eq_ignore_ascii_case("interfaces") =>
            {
                // Deliberately use SST's IP Helper based interface probe instead
                // of Native Wi-Fi WlanQueryInterface. Recent Windows releases can
                // gate SSID/BSSID behind the Location permission; adapter state,
                // link state, speed, MAC and assigned IPs do not require those
                // location-sensitive WLAN fields.
                self.filter_interfaces("Wi-Fi", "wlan")
            }
            _ => Ok(CommandOutput::error(
                "Uso: notsh wlan show interfaces",
                2,
            )),
        }
    }

    fn lan(&self, args: &[String]) -> Result<CommandOutput> {
        match args {
            [] => self.filter_interfaces("Ethernet", "lan"),
            [show, interfaces]
                if show.eq_ignore_ascii_case("show")
                    && interfaces.eq_ignore_ascii_case("interfaces") =>
            {
                self.filter_interfaces("Ethernet", "lan")
            }
            _ => Ok(CommandOutput::error(
                "Uso: notsh lan show interfaces",
                2,
            )),
        }
    }

    fn show(&self, args: &[String]) -> Result<CommandOutput> {
        match args {
            [interfaces] if interfaces.eq_ignore_ascii_case("interfaces") => self.interfaces(),
            _ => Ok(CommandOutput::error("Uso: notsh show interfaces", 2)),
        }
    }

    fn dispatch(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(sub) = args.first().map(String::as_str) else {
            return self.service.execute(&[]);
        };
        let rest = args.get(1..).unwrap_or_default();

        if matches!(sub, "help" | "--help" | "-h") {
            return Ok(CommandOutput::ok(
                "notsh — shell de red de SST\n\n\
                 Sintaxis compatible con netsh:\n\
                   notsh wlan show interfaces   estado Wi-Fi sin consultar SSID/BSSID\n\
                   notsh lan show interfaces    estado Ethernet/cableado\n\
                   notsh show interfaces        todas las interfaces\n\n\
                 Sintaxis tipo Unix/SST:\n\
                   notsh link                   interfaces y enlace\n\
                   notsh addr                   direcciones e interfaces\n\
                   notsh route                  tabla de rutas\n\
                   notsh neigh                  vecinos IP/MAC\n\
                   notsh ss                     sockets TCP/UDP y PID\n\
                   notsh dns HOST|IP            resolución DNS\n\
                   notsh ping HOST [-c N]       eco ICMP\n\
                   notsh trace HOST             trazado\n\
                   notsh scan [RED]             descubrimiento SST\n\
                   notsh monitor [RED]          monitor SST\n\n\
                 'net' se conserva como alias de compatibilidad.\n",
            ));
        }

        match sub {
            "wlan" | "wifi" | "wireless" => self.wlan(rest),
            "lan" | "ethernet" => self.lan(rest),
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
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        self.dispatch(args)
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
