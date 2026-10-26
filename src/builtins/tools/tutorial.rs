use std::process::Command;

use anyhow::{Context, Result};

use crate::{
    core::CommandOutput,
    support::windows::system32_executable,
};

const TUTORIAL_BASE: &str = "https://raw.githubusercontent.com/glitchaka/adm-toolbox/main/tutorials";
const MAX_TUTORIAL_BYTES: usize = 256 * 1024;

pub(super) fn run(args: &[String]) -> Result<CommandOutput> {
    if args.len() > 1 {
        return Ok(CommandOutput::error(
            "tools tutorial: usa tools tutorial list o tools tutorial <nombre>",
            2,
        ));
    }

    let topic = args.first().map(String::as_str).unwrap_or("list");
    let file = if topic.eq_ignore_ascii_case("list") {
        "index.txt".to_owned()
    } else {
        let topic = topic.to_ascii_lowercase();
        if topic.is_empty()
            || !topic
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Ok(CommandOutput::error(
                "tools tutorial: nombre no válido; usa tools tutorial list",
                2,
            ));
        }
        format!("{topic}.txt")
    };

    let url = format!("{TUTORIAL_BASE}/{file}");
    let curl = system32_executable("curl.exe")?;
    let output = Command::new(curl)
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "--user-agent",
            "SST-Tutorials/1",
        ])
        .arg(&url)
        .output()
        .context("tools tutorial: no se pudo consultar el repositorio de tutoriales")?;

    if !output.status.success() {
        return Ok(CommandOutput::error(
            if topic.eq_ignore_ascii_case("list") {
                "tools tutorial: no se pudo cargar el índice remoto".to_owned()
            } else {
                format!("tools tutorial: no existe o no pudo cargarse el tutorial '{topic}'")
            },
            output.status.code().unwrap_or(1),
        ));
    }

    if output.stdout.len() > MAX_TUTORIAL_BYTES {
        return Ok(CommandOutput::error(
            "tools tutorial: el contenido remoto excede el límite permitido por SST",
            1,
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim_end().to_owned();
    if text.is_empty() {
        return Ok(CommandOutput::error(
            "tools tutorial: el contenido remoto está vacío",
            1,
        ));
    }

    Ok(CommandOutput::ok(format!("{text}\n")))
}
