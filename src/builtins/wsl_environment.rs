//! WSL environment adapter. Arch Linux runs in its official Linux subsystem,
//! not as an emulated Windows process or a second interactive CMD window.
use anyhow::Result;
use crate::core::{CommandContext, CommandOutput};
use super::BuiltinCommand;

pub struct WslEnvironmentBuiltin {
    distribution: String,
}

impl WslEnvironmentBuiltin {
    pub fn new(distribution: String) -> Self {
        Self { distribution }
    }
}

impl BuiltinCommand for WslEnvironmentBuiltin {
    fn name(&self) -> &'static str { "arch" }
    fn help(&self) -> &'static str {
        concat!(
            "arch COMANDO [ARGS] — ejecuta comandos en Arch Linux por WSL2\n",
            "Requiere instp install arch y distribución Arch registrada\n",
            "Ejemplo: arch pacman -Q; arch bash -lc 'uname -a'\n",
        )
    }

    fn execute(
        &self, _name: &str, args: &[String], context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if args.is_empty() || args == ["--help"] {
            return Ok(CommandOutput::ok(self.help()));
        }
        #[cfg(windows)]
        {
            use std::process::{Command, Stdio};
            use std::io::Write;
            use std::os::windows::process::CommandExt;

            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let program = crate::support::windows::system32_executable("wsl.exe")?;
            let mut command = Command::new(program);
            command.arg("--distribution").arg(&self.distribution)
                .arg("--exec").args(args).current_dir(context.cwd)
                .stdin(if context.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
                .stdout(Stdio::piped()).stderr(Stdio::piped())
                .creation_flags(CREATE_NO_WINDOW);
            let mut child = command.spawn()?;
            if let (Some(input), Some(mut pipe)) = (context.stdin, child.stdin.take()) {
                pipe.write_all(input)?;
            }
            let result = child.wait_with_output()?;
            Ok(CommandOutput {
                stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
                status: result.status.code().unwrap_or(1),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = context;
            Ok(CommandOutput::error("Arch WSL requiere Windows", 1))
        }
    }
}
