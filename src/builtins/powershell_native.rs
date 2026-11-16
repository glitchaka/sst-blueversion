//! SST-native administrative cmdlets. This is deliberately not a PowerShell
//! language interpreter, nor a launcher for powershell.exe or pwsh.exe.
use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::{network::NetworkService, system::SystemService},
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

const ALIASES: &[&str] = &[
    "Get-Process", "Get-Service", "Get-NetAdapter",
    "Get-NetIPConfiguration", "Get-ComputerInfo", "Get-Printer",
    "Get-WinEvent", "Get-Disk", "Get-NetRoute", "Resolve-DnsName",
    "Get-LocalUser", "Get-PnpDevice", "Test-Connection", "Get-Date",
];

pub struct NativePowerShellBuiltin {
    system: Arc<SystemService>,
    network: Arc<NetworkService>,
}

impl NativePowerShellBuiltin {
    pub fn new(system: Arc<SystemService>, network: Arc<NetworkService>) -> Self {
        Self { system, network }
    }

    fn help_text() -> &'static str {
        "Módulo PowerShell nativo de SST (solo cmdlets implementados)\n\
         No ejecuta powershell.exe ni pwsh.exe. No interpreta scripts .ps1.\n\
         Cmdlets: Get-Process [-Id PID], Get-Service [-Name NOMBRE],\n\
         Get-NetAdapter, Get-NetIPConfiguration, Get-ComputerInfo,\n\
         Get-Printer [-Name NOMBRE], Get-WinEvent [-LogName LOG],\n\
         Get-Disk, Get-NetRoute, Resolve-DnsName -Name HOST,\n\
         Get-LocalUser, Get-PnpDevice, Test-Connection -TargetName HOST, Get-Date.\n\
         Instalar o desinstalar con instp install/remove powershell.\n"
    }

    fn single_parameter(args: &[String], flag: &str) -> std::result::Result<Option<String>, CommandOutput> {
        if args.is_empty() { return Ok(None); }
        if args.len() == 2 && args[0].eq_ignore_ascii_case(flag) && !args[1].trim().is_empty() {
            return Ok(Some(args[1].clone()));
        }
        Err(CommandOutput::error(
            format!("cmdlet: uso esperado [{flag} VALOR]"), 2,
        ))
    }

    fn no_args(args: &[String], command: &str) -> std::result::Result<(), CommandOutput> {
        if args.is_empty() { Ok(()) }
        else { Err(CommandOutput::error(format!("{command}: no admite esos parámetros"), 2)) }
    }

    fn execute_cmdlet(&self, invoked_name: &str, args: &[String]) -> Result<CommandOutput> {
        let name = invoked_name.to_ascii_lowercase();
        let result = match name.as_str() {
            "get-process" => {
                match Self::single_parameter(args, "-Id") {
                    Ok(Some(pid)) => self.system.execute(&["inspect".into(), pid]),
                    Ok(None) => self.system.execute(&["processes".into()]),
                    Err(error) => return Ok(error),
                }
            }
            "get-service" => {
                match Self::single_parameter(args, "-Name") {
                    Ok(Some(service)) => self.system.execute(&["services".into(), service]),
                    Ok(None) => self.system.execute(&["services".into()]),
                    Err(error) => return Ok(error),
                }
            }
            "get-netadapter" | "get-netipconfiguration" => {
                if let Err(error) = Self::no_args(args, invoked_name) { return Ok(error); }
                self.network.execute(&["interfaces".into()])
            }
            "get-computerinfo" => {
                if let Err(error) = Self::no_args(args, invoked_name) { return Ok(error); }
                self.system.execute(&["info".into()])
            }
            "get-printer" => {
                match Self::single_parameter(args, "-Name") {
                    Ok(Some(printer)) => self.system.execute(&["printer".into(), printer]),
                    Ok(None) => self.system.execute(&["printers".into()]),
                    Err(error) => return Ok(error),
                }
            }
            "get-winevent" => {
                match Self::single_parameter(args, "-LogName") {
                    Ok(Some(log)) => self.system.execute(&["events".into(), log]),
                    Ok(None) => self.system.execute(&["events".into()]),
                    Err(error) => return Ok(error),
                }
            }
            "get-disk" => {
                if let Err(error) = Self::no_args(args, invoked_name) { return Ok(error); }
                self.system.execute(&["disks".into()])
            }
            "get-netroute" => {
                if let Err(error) = Self::no_args(args, invoked_name) { return Ok(error); }
                self.network.execute(&["routes".into()])
            }
            "resolve-dnsname" => {
                match Self::single_parameter(args, "-Name") {
                    Ok(Some(host)) => self.network.execute(&["dns".into(), host]),
                    Ok(None) => return Ok(CommandOutput::error(
                        "Resolve-DnsName: falta -Name HOST", 2,
                    )),
                    Err(error) => return Ok(error),
                }
            }
            "get-localuser" => {
                if let Err(error) = Self::no_args(args, invoked_name) { return Ok(error); }
                self.system.execute(&["users".into()])
            }
            "get-pnpdevice" => {
                if let Err(error) = Self::no_args(args, invoked_name) { return Ok(error); }
                self.system.execute(&["drivers".into(), "--devices".into()])
            }
            "test-connection" => {
                match Self::single_parameter(args, "-TargetName") {
                    Ok(Some(host)) => self.network.execute(&["ping".into(), host]),
                    Ok(None) => return Ok(CommandOutput::error(
                        "Test-Connection: falta -TargetName HOST", 2,
                    )),
                    Err(error) => return Ok(error),
                }
            }
            "get-date" => {
                if let Err(error) = Self::no_args(args, invoked_name) { return Ok(error); }
                return Ok(CommandOutput::ok(format!(
                    "{}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z"),
                )));
            }
            _ => return Ok(CommandOutput::error(
                format!("PowerShell SST: cmdlet no implementado: {invoked_name}"), 2,
            )),
        };
        result
    }
}

impl BuiltinCommand for NativePowerShellBuiltin {
    fn name(&self) -> &'static str { "psmodule" }
    fn aliases(&self) -> &'static [&'static str] { ALIASES }
    fn help(&self) -> &'static str { Self::help_text() }

    fn execute(
        &self, name: &str, args: &[String], _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if name == "psmodule" || args.len() == 1
            && matches!(args[0].as_str(), "-h" | "--help")
        {
            return Ok(CommandOutput::ok(Self::help_text()));
        }
        self.execute_cmdlet(name, args)
    }
}
