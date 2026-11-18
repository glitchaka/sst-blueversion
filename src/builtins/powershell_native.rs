//! An installed PowerShell module owns a real PowerShell SDK runspace.
//! Commands and .ps1 files execute in the SST process through hostfxr.
use std::path::{Path, PathBuf};
use anyhow::Result;
use crate::core::{CommandContext, CommandOutput};
use super::BuiltinCommand;

#[cfg(windows)]
use crate::adapters::windows::powershell_host::PowerShellEngine;

const ALIASES: &[&str] = &[
    "powershell", "ps", "Get-Process", "Get-Service", "Get-NetAdapter",
    "Get-NetIPConfiguration", "Get-ComputerInfo", "Get-Printer",
    "Get-WinEvent", "Get-Disk", "Get-NetRoute", "Resolve-DnsName",
    "Get-LocalUser", "Get-PnpDevice", "Test-Connection", "Get-Date",
    "get-process", "get-service", "get-netadapter",
    "get-netipconfiguration", "get-computerinfo", "get-printer",
    "get-winevent", "get-disk", "get-netroute", "resolve-dnsname",
    "get-localuser", "get-pnpdevice", "test-connection", "get-date",
];

pub struct NativePowerShellBuiltin {
    path: PathBuf,
    #[cfg(windows)]
    engine: Result<PowerShellEngine, String>,
}

impl NativePowerShellBuiltin {
    pub fn new(path: PathBuf) -> Self {
        #[cfg(windows)]
        let engine = PowerShellEngine::load(&path).map_err(|error| format!("{error:#}"));
        Self {
            path,
            #[cfg(windows)]
            engine,
        }
    }

    fn usage() -> &'static str {
        "Motor PowerShell SDK de SST (runspace .NET embebido)\n\
         ps -Command \"Get-Service | Where-Object Status -eq Running\"\n\
         ps -File archivo.ps1 [ARGUMENTOS]\n\
         psmodule -Command '...'\n\
         psmodule -File archivo.ps1\n\
         psmodule status\n\
         Un archivo .ps1 también puede ejecutarse directamente en Nwash.\n\
         El motor no abre powershell.exe ni pwsh.exe.\n\
         Instalación: instp install powershell (reinicia SST después).\n"
    }

    #[cfg(windows)]
    fn run_engine(
        &self, kind: &str, source: &str, args: &[String], context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        let engine = match &self.engine {
            Ok(engine) => engine,
            Err(error) => return Ok(CommandOutput::error(
                format!("{error}\nReinstala con instp install powershell y reinicia SST."), 1,
            )),
        };
        engine.run(kind, source, args, context.cwd, context.stdin)
    }

    #[cfg(not(windows))]
    fn run_engine(
        &self, _kind: &str, _source: &str, _args: &[String], _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        Ok(CommandOutput::error("PowerShell embebido disponible solo en Windows", 1))
    }

    fn file_path(&self, requested: &str, cwd: &Path) -> Result<PathBuf> {
        let input = Path::new(requested);
        let path = if input.is_absolute() {
            input.to_path_buf()
        } else {
            cwd.join(input)
        };
        anyhow::ensure!(
            path.extension().and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ps1")) && path.is_file(),
            "PowerShell: el script .ps1 solicitado no existe"
        );
        Ok(path)
    }
}

impl BuiltinCommand for NativePowerShellBuiltin {
    fn name(&self) -> &'static str { "psmodule" }
    fn aliases(&self) -> &'static [&'static str] { ALIASES }
    fn help(&self) -> &'static str { Self::usage() }

    fn execute(
        &self, invoked_name: &str, args: &[String], context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if args.is_empty() && invoked_name == "psmodule"
            || args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h")
        {
            return Ok(CommandOutput::ok(Self::usage()));
        }
        if args.len() == 1 && args[0] == "status" && invoked_name == "psmodule" {
            return self.run_engine("probe", "", &[], context);
        }

        if matches!(invoked_name, "psmodule" | "powershell" | "ps") {
            return match args {
                [flag, path, rest @ ..] if flag.eq_ignore_ascii_case("-File") => {
                    let path = self.file_path(path, context.cwd)?;
                    self.run_engine("file", &path.to_string_lossy(), rest, context)
                }
                [flag, code, rest @ ..] if flag.eq_ignore_ascii_case("-Command")
                    && rest.is_empty() => self.run_engine("script", code, &[], context),
                [script, rest @ ..] if script.to_ascii_lowercase().ends_with(".ps1") => {
                    let path = self.file_path(script, context.cwd)?;
                    self.run_engine("file", &path.to_string_lossy(), rest, context)
                }
                _ => Ok(CommandOutput::error(Self::usage(), 2)),
            };
        }
        // Canonical cmdlet aliases use the same SDK engine as .ps1 execution.
        self.run_engine("command", invoked_name, args, context)
    }
}
