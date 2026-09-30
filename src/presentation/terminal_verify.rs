use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::adapters::terminal::embedded::EmbeddedSession;

pub fn verify_transport() -> Result<()> {
    let mut pty = EmbeddedSession::start(112, 31)?;
    let mut parser = vt100::Parser::new(31, 112, 1000);
    let started = Instant::now();
    let mut sent = false;

    loop {
        if started.elapsed() > Duration::from_secs(15) {
            bail!("El intérprete no respondió. Pantalla: {}", parser.screen().contents());
        }
        if let Ok(bytes) = pty.output.recv_timeout(Duration::from_millis(100)) {
            parser.process(&bytes);
        }
        let contents = parser.screen().contents();
        if !sent && (contents.contains("❯") || contents.contains("$ ")) {
            pty.write(b"echo SST_NATIVE_OK\r")?;
            sent = true;
        }
        if sent && contents.matches("SST_NATIVE_OK").count() >= 2 {
            pty.write(b"exit\r")?;
            println!("Shell Shock Tool: prompt, entrada, ejecución y salida correctos.");
            return Ok(());
        }
    }
}
