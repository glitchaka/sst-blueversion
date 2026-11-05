use std::{collections::{HashMap, VecDeque}, path::Path, sync::Mutex};
use anyhow::Result;
use super::{ExecutionResult, Interpreter, ShellCommandHost};

struct DiagnosticHost {
    answers: Mutex<VecDeque<String>>,
}

impl ShellCommandHost for DiagnosticHost {
    fn read_input(
        &self, _: &str, _: bool, _: char,
        _: Option<usize>, _: Option<std::time::Duration>, _: &str,
    ) -> Result<Option<String>> {
        Ok(self.answers.lock().unwrap().pop_front())
    }

    fn execute_builtin(
        &self, name: &str, args: &[String], _: &Path, _: Option<&[u8]>,
    ) -> Result<Option<ExecutionResult>> {
        let result = match (name, args.first().map(String::as_str)) {
            ("sys", Some("printers")) => Some("IMPRESORAS\n* Impresora de red\n  servidor: \\\\PRINT-SRV\n  puerto: WSD-123\n  IP: <no resuelta>\n".to_owned()),
            ("sys", Some("processes")) => {
                assert_eq!(args, &["processes", "--sort", "ram", "--top", "8"]
                    .map(str::to_owned));
                Some("PID PPID CPU% RAM MiB PROCESS\n100 1 0.0 899.0 proceso-test.exe\n".to_owned())
            }
            ("sys", Some("hostname")) => Some("EQUIPO-IT\n".to_owned()),
            ("sys", Some("whoami")) => Some("DOMINIO\\usuario\n".to_owned()),
            ("net", Some("interfaces")) => Some("Ethernet\n  IP: 192.168.1.70/24\n  MAC: AA:BB:CC:DD:EE:FF\n  Enlace: UP\n".to_owned()),
            _ => None,
        };
        Ok(result.map(|stdout| ExecutionResult::from_parts(stdout, String::new(), 0)))
    }

    fn execute_external(
        &self, program: &str, _: &[String], _: &Path, _: &HashMap<String, String>,
        _: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        let output = if program.ends_with("ipconfig.exe") {
            "Ethernet adapter Ethernet:\r\n    Physical Address . . . . . : AA-BB-CC-DD-EE-FF\r\n    IPv4 Address . . . : 192.168.1.70\r\n"
        } else {
            ""
        };
        Ok(ExecutionResult::from_parts(output.to_owned(), String::new(), 0))
    }
}

#[test]
fn diagnostic_keeps_name_and_executes_all_four_options_before_exit() {
    // The actual published sample runs against scripted keyboard input here.
    // Option 4 is exercised explicitly so printer errors cannot be overlooked.
    let host = DiagnosticHost {
        answers: Mutex::new(
            ["Alejandro", "1", "x", "2", "x", "3", "x", "4", "x", "0"]
                .into_iter().map(str::to_owned).collect(),
        ),
    };
    let mut shell = Interpreter::new(Box::new(host));
    let script = include_str!("../../../examples/11-diagnostico-interactivo.sh");
    let result = shell.execute_text(script).expect("Nwash must execute the diagnostic");
    assert_eq!(result.status, 0, "{}", result.stderr);
    assert!(result.stdout.contains("¡Hola, Alejandro! Bienvenido a SST"), "{:?}", result.stdout);
    assert!(result.stdout.contains("IP: 192.168.1.70/24"), "{:?}", result.stdout);
    assert!(result.stdout.contains("MAC: AA:BB:CC:DD:EE:FF"), "{:?}", result.stdout);
    assert!(result.stdout.contains("EQUIPO-IT"), "{:?}", result.stdout);
    assert!(result.stdout.contains("DOMINIO\\usuario"), "{:?}", result.stdout);
    assert!(result.stdout.contains("899.0 proceso-test.exe"), "{:?}", result.stdout);
    assert!(result.stdout.contains("Impresora de red"), "{:?}", result.stdout);
    assert!(result.stdout.contains("Hasta pronto, Alejandro"), "{:?}", result.stdout);
}
