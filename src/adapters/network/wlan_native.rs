//! Implementación nativa de Wi-Fi para NOTSH. No ejecuta netsh, CMD ni PowerShell.
//! Las restricciones de privacidad y los errores de WLAN AutoConfig se respetan.
use crate::core::CommandOutput;
use anyhow::Result;

pub fn execute(args: &[String]) -> Result<CommandOutput> {
    let words: Vec<_> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["help"] | ["--help"] | ["?"] => Ok(CommandOutput::ok(
            "NOTSH WLAN — Windows WLAN API\n\
             notsh wlan show interfaces    conexión, SSID/BSSID, señal, 802.1X configurado\n\
             notsh wlan show profiles      nombres de perfiles sin contraseñas\n\
             notsh wlan show networks      redes visibles sujetas a permisos Windows\n"
        )),
        ["show", "interfaces"] => platform::interfaces(),
        ["show", "profiles"] => platform::profiles(),
        ["show", "networks"] => platform::networks(),
        _ => Ok(CommandOutput::error(
            "notsh wlan: opción no implementada de forma nativa; utiliza 'notsh wlan help'.", 2
        )),
    }
}
/// CSV produced from WLAN API attributes, with no ANSI or report decorations.
pub fn interfaces_csv() -> Result<CommandOutput> {
    platform::interfaces_csv()
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub fn interfaces() -> Result<CommandOutput> { Ok(no_windows()) }
    pub fn interfaces_csv() -> Result<CommandOutput> { Ok(no_windows()) }
    pub fn profiles() -> Result<CommandOutput> { Ok(no_windows()) }
    pub fn networks() -> Result<CommandOutput> { Ok(no_windows()) }
    fn no_windows() -> CommandOutput { CommandOutput::error("notsh wlan: requiere Windows", 2) }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{ffi::c_void, ptr};
    use windows_sys::Win32::{Foundation::HANDLE, NetworkManagement::WiFi::*};

    struct Client(HANDLE);
    impl Client {
        fn new() -> std::result::Result<Self, CommandOutput> {
            let (mut ver, mut handle) = (0u32, ptr::null_mut());
            let rc = unsafe { WlanOpenHandle(2, ptr::null(), &mut ver, &mut handle) };
            if rc != 0 { Err(failure("WlanOpenHandle", rc)) } else { Ok(Self(handle)) }
        }
        fn adapters(&self) -> std::result::Result<Vec<WLAN_INTERFACE_INFO>, CommandOutput> {
            let mut data: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
            let rc = unsafe { WlanEnumInterfaces(self.0, ptr::null(), &mut data) };
            if rc != 0 { return Err(failure("WlanEnumInterfaces", rc)); }
            if data.is_null() { return Ok(Vec::new()); }
            let result = unsafe {
                let list = &*data;
                std::slice::from_raw_parts(list.InterfaceInfo.as_ptr(),
                    (list.dwNumberOfItems as usize).min(512)).to_vec()
            };
            unsafe { WlanFreeMemory(data.cast()); }
            Ok(result)
        }
    }
    impl Drop for Client {
        fn drop(&mut self) { unsafe { WlanCloseHandle(self.0, ptr::null()); } }
    }
    fn failure(operation: &str, rc: u32) -> CommandOutput {
        let info = match rc {
            5 => "Acceso denegado; comprueba permisos y Ubicación.",
            1062 => "El servicio WLAN AutoConfig está detenido.",
            _ => "Consulta rechazada por Windows; no se inventará información alternativa.",
        };
        CommandOutput::error(format!("notsh wlan: {operation} (Win32 {rc}). {info}"), 1)
    }
    fn wide(value: &[u16]) -> String {
        String::from_utf16_lossy(&value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())])
    }
    fn ssid(value: &DOT11_SSID) -> String {
        let len = (value.uSSIDLength as usize).min(value.ucSSID.len());
        String::from_utf8_lossy(&value.ucSSID[..len]).into_owned()
    }
    fn session() -> std::result::Result<(Client, Vec<WLAN_INTERFACE_INFO>), CommandOutput> {
        let client = Client::new()?;
        let adapters = client.adapters()?;
        Ok((client, adapters))
    }
    fn none() -> CommandOutput {
        CommandOutput::ok("No se detectaron interfaces Wi-Fi administradas por Windows WLAN API.\n")
    }

    pub fn interfaces() -> Result<CommandOutput> {
        let (client, adapters) = match session() { Ok(s) => s, Err(e) => return Ok(e) };
        if adapters.is_empty() { return Ok(none()); }
        let mut output = format!("NOTSH WLAN — {} interfaces Wi-Fi\n", adapters.len());
        let mut errors = 0;
        for adapter in &adapters {
            output.push_str(&format!("\n[{}]\n", wide(&adapter.strInterfaceDescription)));
            let (mut size, mut buffer, mut kind) = (0u32, ptr::null_mut::<c_void>(), 0);
            let rc = unsafe { WlanQueryInterface(
                client.0, &adapter.InterfaceGuid, wlan_intf_opcode_current_connection,
                ptr::null(), &mut size, &mut buffer, &mut kind
            ) };
            if rc != 0 {
                errors += 1;
                output.push_str(&format!("  Estado: NO DISPONIBLE (Win32 {rc})\n"));
                continue;
            }
            if buffer.is_null() || size < std::mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>() as u32 {
                if !buffer.is_null() { unsafe { WlanFreeMemory(buffer); } }
                errors += 1;
                output.push_str("  Estado: informe de Windows incompleto\n");
                continue;
            }
            let attributes = unsafe { (buffer as *const WLAN_CONNECTION_ATTRIBUTES).read_unaligned() };
            unsafe { WlanFreeMemory(buffer); }
            if attributes.isState != wlan_interface_state_connected {
                output.push_str("  Estado: DESCONECTADO\n");
                continue;
            }
            let association = &attributes.wlanAssociationAttributes;
            let bssid = association.dot11Bssid.iter()
                .map(|v| format!("{v:02X}")).collect::<Vec<_>>().join(":");
            output.push_str("  Estado: CONECTADO\n");
            output.push_str(&format!("  Perfil: {}\n", wide(&attributes.strProfileName)));
            output.push_str(&format!("  SSID: {}\n", ssid(&association.dot11Ssid)));
            output.push_str(&format!("  BSSID: {bssid}\n"));
            output.push_str(&format!("  Señal: {}%\n", association.wlanSignalQuality.min(100)));
            output.push_str(&format!("  Velocidad RX/TX: {} / {} Kbps\n",
                association.ulRxRate, association.ulTxRate));
            output.push_str(&format!("  Algoritmo autenticación: {}\n",
                attributes.wlanSecurityAttributes.dot11AuthAlgorithm));
            output.push_str(&format!("  802.1X configurado: {}\n",
                if attributes.wlanSecurityAttributes.bOneXEnabled != 0 { "SÍ" } else { "NO" }));
            output.push_str("  NAC: no se determina por la conexión Wi-Fi.\n");
        }
        Ok(CommandOutput {stdout:output,stderr:String::new(),status:if errors==adapters.len(){1}else{0}})
    }

    pub fn interfaces_csv() -> Result<CommandOutput> {
        let mut output = String::from(
            "Adaptador,Estado,Perfil,SSID,BSSID,Senal_pct,RX_Kbps,TX_Kbps,Autenticacion,8021X\n"
        );
        // Some Windows Server / CI hosts have no WLAN AutoConfig service. A
        // failed query must preserve its diagnostic and exit status, but must
        // never substitute an error paragraph for machine-readable stdout.
        let (client, adapters) = match session() {
            Ok(session) => session,
            Err(error) => return Ok(CommandOutput {
                stdout: output,
                stderr: if error.stderr.is_empty() { error.stdout } else { error.stderr },
                status: error.status.max(1),
            }),
        };
        let mut errors = 0usize;
        for adapter in &adapters {
            let mut fields = vec![
                wide(&adapter.strInterfaceDescription),
                "DESCONECTADO".to_owned(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
            ];
            if adapter.isState == wlan_interface_state_connected {
                let (mut size, mut buffer, mut kind) = (0u32, ptr::null_mut::<c_void>(), 0);
                let rc = unsafe { WlanQueryInterface(
                    client.0, &adapter.InterfaceGuid, wlan_intf_opcode_current_connection,
                    ptr::null(), &mut size, &mut buffer, &mut kind
                ) };
                if rc != 0 {
                    errors += 1;
                    fields[1] = format!("NO DISPONIBLE (Win32 {rc})");
                } else if buffer.is_null()
                    || size < std::mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>() as u32 {
                    errors += 1;
                    fields[1] = "INFORME INCOMPLETO".to_owned();
                } else {
                    let attributes = unsafe {
                        (buffer as *const WLAN_CONNECTION_ATTRIBUTES).read_unaligned()
                    };
                    if attributes.isState == wlan_interface_state_connected {
                        let assoc = &attributes.wlanAssociationAttributes;
                        fields[1] = "CONECTADO".to_owned();
                        fields[2] = wide(&attributes.strProfileName);
                        fields[3] = ssid(&assoc.dot11Ssid);
                        fields[4] = assoc.dot11Bssid.iter()
                            .map(|byte| format!("{byte:02X}"))
                            .collect::<Vec<_>>().join(":");
                        fields[5] = assoc.wlanSignalQuality.min(100).to_string();
                        fields[6] = assoc.ulRxRate.to_string();
                        fields[7] = assoc.ulTxRate.to_string();
                        fields[8] = attributes.wlanSecurityAttributes.dot11AuthAlgorithm.to_string();
                        fields[9] = if attributes.wlanSecurityAttributes.bOneXEnabled != 0 {
                            "SI".to_owned()
                        } else {
                            "NO".to_owned()
                        };
                    }
                }
                if !buffer.is_null() { unsafe { WlanFreeMemory(buffer); } }
            }
            output.push_str(&fields.iter()
                .map(|value| crate::support::csv::escape(value))
                .collect::<Vec<_>>().join(","));
            output.push('\n');
        }
        Ok(CommandOutput {
            stdout: output,
            stderr: String::new(),
            status: if adapters.is_empty() || errors < adapters.len() { 0 } else { 1 },
        })
    }

    pub fn profiles() -> Result<CommandOutput> {
        let (client, adapters) = match session() { Ok(s) => s, Err(e) => return Ok(e) };
        if adapters.is_empty() { return Ok(none()); }
        let mut output=String::from("NOTSH WLAN — perfiles guardados (sin claves)\n");
        let mut errors=0;
        for adapter in &adapters {
            output.push_str(&format!("\n[{}]\n",wide(&adapter.strInterfaceDescription)));
            let mut buffer: *mut WLAN_PROFILE_INFO_LIST=ptr::null_mut();
            let rc=unsafe{WlanGetProfileList(client.0,&adapter.InterfaceGuid,ptr::null(),&mut buffer)};
            if rc!=0 { errors+=1;output.push_str(&format!("  No disponible (Win32 {rc})\n"));continue;}
            if buffer.is_null(){output.push_str("  Sin perfiles\n");continue;}
            unsafe {
                let list=&*buffer;
                let count=(list.dwNumberOfItems as usize).min(4096);
                for item in std::slice::from_raw_parts(list.ProfileInfo.as_ptr(),count) {
                    output.push_str(&format!("  • {}\n",wide(&item.strProfileName)));
                }
                if count==0 {output.push_str("  Sin perfiles\n");}
                WlanFreeMemory(buffer.cast());
            }
        }
        Ok(CommandOutput{stdout:output,stderr:String::new(),status:if errors==adapters.len(){1}else{0}})
    }
    pub fn networks() -> Result<CommandOutput> {
        let (client, adapters) = match session() { Ok(s) => s, Err(e) => return Ok(e) };
        if adapters.is_empty() { return Ok(none()); }
        let mut output=String::from("NOTSH WLAN — redes visibles (permisos de Ubicación Windows)\n");
        let mut errors=0;
        for adapter in &adapters {
            output.push_str(&format!("\n[{}]\n",wide(&adapter.strInterfaceDescription)));
            let mut buffer: *mut WLAN_AVAILABLE_NETWORK_LIST=ptr::null_mut();
            let rc=unsafe{WlanGetAvailableNetworkList(client.0,&adapter.InterfaceGuid,0,ptr::null(),&mut buffer)};
            if rc!=0 { errors+=1;output.push_str(&format!("  No disponible (Win32 {rc}); comprueba Ubicación.\n"));continue;}
            if buffer.is_null(){output.push_str("  Sin redes\n");continue;}
            unsafe {
                let list=&*buffer;
                let count=(list.dwNumberOfItems as usize).min(4096);
                for item in std::slice::from_raw_parts(list.Network.as_ptr(),count) {
                    let name=ssid(&item.dot11Ssid);
                    output.push_str(&format!("  {:<32} {:>3}%  {}{}\n",
                        if name.is_empty(){"<oculta>"}else{&name},
                        item.wlanSignalQuality.min(100),
                        if item.bSecurityEnabled!=0{"protegida"}else{"abierta"},
                        if item.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED !=0 {" · conectada"} else {""}
                    ));
                }
                if count==0{output.push_str("  Sin redes\n");}
                WlanFreeMemory(buffer.cast());
            }
        }
        Ok(CommandOutput{stdout:output,stderr:String::new(),status:if errors==adapters.len(){1}else{0}})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_mutation_is_rejected() {
        let result=execute(&["set".into(),"profile".into()]).unwrap();
        assert_eq!(result.status,2);
    }
}
