//! Fail-closed public GitHub issue receipts for high-privilege SST requests.
//! GitHub Issues are an interim audit receiver, NOT an immutable audit ledger.
//! Authentication is read from Windows Credential Manager, never a USB file,
//! environment variable, command argument or executable-embedded token.
use std::{
    io::Write,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    process::{Command, Stdio},
    ptr::null_mut,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use windows_sys::Win32::{
    Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC},
    System::Threading::CREATE_NO_WINDOW,
};

use super::sudo_audit::AuditEvent;

const ISSUE_ENDPOINT: &str = "https://api.github.com/repos/glitchaka/adm-toolbox/issues";
const CREDENTIAL_TARGET: &str = "SST/GitHubAudit";

pub(super) struct RemoteReceipt {
    number: u64,
    event_id: String,
}

pub(super) fn authorize(event: &AuditEvent) -> Result<RemoteReceipt> {
    let payload = event.event_json("authorization_requested", None);
    let request = json!({
        "title": format!("[SST audit] {} {}", event.operation(), event.event_id()),
        "body": format!(
            "Registro de solicitud de elevación de Shell Shock Tool.\n\n{}",
            serde_json::to_string_pretty(&payload)?
        ),
    });
    let response = authenticated_post(ISSUE_ENDPOINT, &request)?;
    let number = response.get("number").and_then(Value::as_u64)
        .filter(|number| *number > 0)
        .context("GitHub no confirmó un número de registro de auditoría")?;
    let expected_prefix = "https://github.com/glitchaka/adm-toolbox/issues/";
    ensure!(
        response.get("html_url").and_then(Value::as_str)
            .is_some_and(|uri| uri.starts_with(expected_prefix)),
        "GitHub devolvió una confirmación de auditoría inválida"
    );
    Ok(RemoteReceipt { number, event_id: event.event_id().to_owned() })
}

pub(super) fn report_result(
    event: &AuditEvent, receipt: &RemoteReceipt, status: i32,
) -> Result<()> {
    ensure!(receipt.event_id == event.event_id(), "identificador de auditoría inconsistente");
    let payload = event.event_json("finished", Some(status));
    let endpoint = format!("{ISSUE_ENDPOINT}/{}/comments", receipt.number);
    let request = json!({
        "body": format!("Resultado de ejecución SST:\n\n{}", serde_json::to_string_pretty(&payload)?)
    });
    let response = authenticated_post(&endpoint, &request)?;
    ensure!(response.get("id").and_then(Value::as_u64).is_some(),
        "GitHub no confirmó el registro del resultado");
    Ok(())
}

fn authenticated_post(endpoint: &str, data: &Value) -> Result<Value> {
    ensure!(endpoint == ISSUE_ENDPOINT ||
        endpoint.starts_with(&format!("{ISSUE_ENDPOINT}/")) &&
        endpoint.ends_with("/comments"),
        "destino de auditoría no permitido");
    let token = read_windows_credential()?;
    let body = serde_json::to_string(data)?;
    // --config - consumes stdin; no credentials ever appear in argv or files.
    // -q disables any ambient curlrc configuration. No redirects are followed.
    let config = format!(
        "url = \"{}\"\nrequest = \"POST\"\n\
         header = \"Authorization: Bearer {}\"\n\
         header = \"Accept: application/vnd.github+json\"\n\
         header = \"Content-Type: application/json\"\n\
         header = \"X-GitHub-Api-Version: 2022-11-28\"\n\
         header = \"User-Agent: ShellShockTool-Audit\"\n\
         data-binary = \"{}\"\n\
         proto = \"=https\"\n\
         connect-timeout = 5\nmax-time = 15\nsilent\nshow-error\nfail\n",
        endpoint, token, curl_config_escape(&body),
    );
    let curl = crate::support::windows::system32_executable("curl.exe")
        .context("no se pudo localizar curl nativo de Windows")?;
    let mut child = Command::new(curl)
        .args(["-q", "--config", "-"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn().context("no se pudo iniciar transmisión de auditoría")?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(config.as_bytes())
            .context("no se pudo enviar la solicitud de auditoría")?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "auditoría no confirmada por GitHub (código de conexión {}); \
             SYSTEM/TrustedInstaller denegado",
            output.status.code().unwrap_or(-1),
        );
    }
    let response: Value = serde_json::from_slice(&output.stdout)
        .context("GitHub no devolvió una respuesta JSON válida")?;
    Ok(response)
}

/// curl's quoted config-file values decode escaped backslashes and quotes.
fn curl_config_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
        .replace('\r', "\\r").replace('\n', "\\n")
}

fn read_windows_credential() -> Result<String> {
    let name: Vec<u16> = std::ffi::OsStr::new(CREDENTIAL_TARGET)
        .encode_wide().chain(Some(0)).collect();
    let mut credential: *mut CREDENTIALW = null_mut();
    let ok = unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if ok == 0 || credential.is_null() {
        bail!(
            "falta la credencial {} en el Administrador de credenciales \
             del usuario elevado; SYSTEM/TrustedInstaller denegado",
            CREDENTIAL_TARGET
        );
    }
    let raw = unsafe {
        let info = &*credential;
        if info.CredentialBlob.is_null() || info.CredentialBlobSize == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(
                info.CredentialBlob, info.CredentialBlobSize as usize,
            ).to_vec()
        }
    };
    unsafe { CredFree(credential.cast()); }
    let token = decode_credential(&raw)?;
    ensure!(
        token.len() >= 20 && token.len() <= 512 &&
        token.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "credencial de GitHub inválida; SYSTEM/TrustedInstaller denegado"
    );
    Ok(token)
}

fn decode_credential(raw: &[u8]) -> Result<String> {
    if raw.len() % 2 == 0 && raw.chunks_exact(2).any(|pair| pair[1] == 0) {
        let units = raw.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        Ok(String::from_utf16(&units)?.trim_end_matches('\0').to_owned())
    } else {
        Ok(String::from_utf8(raw.to_vec())?.trim_end_matches('\0').to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curl_config_quotes_are_escaped() {
        assert_eq!(curl_config_escape("{\"ok\":true}"), "{\\\"ok\\\":true}");
    }
    #[test]
    fn accepts_utf8_or_utf16_credential_blobs() {
        let secret = "github_pat_12345678901234567890";
        assert_eq!(decode_credential(secret.as_bytes()).unwrap(), secret);
        let utf16 = secret.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        assert_eq!(decode_credential(&utf16).unwrap(), secret);
    }
}
