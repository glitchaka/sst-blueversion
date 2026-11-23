use std::{net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs},sync::Arc,time::Duration};
use anyhow::Result;
use crate::{core::{CommandContext, CommandOutput, ports::NetworkProbe},builtins::BuiltinCommand};

pub struct NetcheckBuiltin { probe: Arc<dyn NetworkProbe> }
impl NetcheckBuiltin {
    pub fn new(probe: Arc<dyn NetworkProbe>) -> Self { Self { probe } }
    fn diagnose(&self, args: &[String]) -> Result<CommandOutput> {
        if args.iter().any(|s| matches!(s.as_str(), "-h" | "--help" | "help")) {
            return Ok(CommandOutput::ok(
                "netcheck — diagnóstico ordenado de conectividad (solo lectura)\n\
                 Uso: netcheck [HOST] [--port PUERTO]\n\
                 Predeterminado: HOST=example.com, PUERTO=443\n\
                 Comprueba enlace, IP/APIPA, gateway, DNS, ICMP y TCP.\n\
                 No modifica DHCP, DNS, firewall, VLAN ni perfiles 802.1X.\n"));
        }
        let mut host = "example.com".to_owned();
        let mut explicit_host = false;
        let mut port = 443u16;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--port" => {
                    let Some(value) = args.get(i + 1) else {
                        return Ok(CommandOutput::error("netcheck: --port requiere número 1..65535", 2));
                    };
                    let Ok(value) = value.parse::<u16>() else {
                        return Ok(CommandOutput::error("netcheck: puerto inválido", 2));
                    };
                    if value == 0 { return Ok(CommandOutput::error("netcheck: puerto cero inválido", 2)); }
                    port = value; i += 2;
                }
                name if !name.starts_with('-') && !explicit_host => {
                    if name.len() > 253 || name.chars().any(|c| c.is_control() || c.is_whitespace()) {
                        return Ok(CommandOutput::error("netcheck: destino inválido", 2));
                    }
                    host = name.to_owned(); explicit_host = true; i += 1;
                }
                _ => return Ok(CommandOutput::error("Uso: netcheck [HOST] [--port PUERTO]", 2)),
            }
        }
        let mut output = format!("DIAGNÓSTICO DE RED\nDestino: {host}:{port}\nModo: solo lectura\n");
        let mut findings = Vec::<(&'static str, String)>::new();
        // Restrict the assessment to physical wired and Wi-Fi adapters.
        // Virtual NICs, tunnels and NDIS filters must not mask a dead cable.
        let physical = self.probe.lan_interfaces().and_then(|lan| {
            self.probe.wlan_interfaces().map(|wlan| format!("{lan}\n{wlan}"))
        });
        match physical {
            Ok(interfaces) => {
                let (status, evidence, issue) = evaluate_interfaces(&interfaces);
                output.push_str(&format!("\n1. INTERFACES FÍSICAS\n  Estado: {status}\n  Evidencia: {evidence}\n"));
                if let Some(issue) = issue { findings.push(issue); }
            }
            Err(error) => {
                output.push_str(&format!("\n1. INTERFACES FÍSICAS\n  Estado: INDETERMINADO\n  Error: {error}\n"));
            }
        }
        // Wired AutoConfig is a different question from physical link.
        // A configured 802.1X profile is NOT proof that RADIUS authorized it.
        match crate::adapters::network::dot3_native::interfaces() {
            Ok(dot3) if dot3.status == 0 => {
                let enabled = dot3.stdout.contains("802.1X configurado: SÍ");
                let managed = dot3.stdout.contains("Perfil: configurado");
                output.push_str(&format!(
                    "\n1b. NAC / 802.1X CABLEADO\n  Estado: {}\n  Autorización efectiva: NO VERIFICADA\n",
                    if enabled { "802.1X CONFIGURADO" }
                    else if managed { "PERFIL PRESENTE; MODO INDETERMINADO" }
                    else { "SIN PERFIL DOT3 CONFIRMADO" },
                ));
            }
            Ok(dot3) => output.push_str(&format!(
                "\n1b. NAC / 802.1X CABLEADO\n  Estado: NO EVALUADO\n  Motivo: {}\n",
                dot3.stderr.trim(),
            )),
            Err(error) => output.push_str(&format!(
                "\n1b. NAC / 802.1X CABLEADO\n  Estado: INDETERMINADO\n  Motivo: {error}\n",
            )),
        }
        let gateway = match self.probe.routes() {
            Ok(routes) => {
                let gateway = ipv4_default_gateway(&routes);
                if let Some(ip) = gateway {
                    output.push_str(&format!("\n2. RUTA IPv4\n  Estado: PRESENTE\n  Gateway: {ip}\n"));
                } else {
                    output.push_str("\n2. RUTA IPv4\n  Estado: SIN GATEWAY POR DEFECTO\n");
                    findings.push(("ADVERTENCIA", "No se encontró gateway IPv4. Verificar ruta y DHCP; puede existir conectividad IPv6.".to_owned()));
                }
                gateway
            }
            Err(error) => {
                output.push_str(&format!("\n2. RUTA IPv4\n  Estado: INDETERMINADO\n  Error: {error}\n"));
                None
            }
        };
        if let Some(gateway) = gateway {
            match self.probe.echo(gateway, 64, Duration::from_millis(900)) {
                Ok(reply) if reply.status == 0 =>
                    output.push_str(&format!("\n3. GATEWAY\n  Estado: ICMP RESPONDE\n  Latencia: {} ms\n", reply.elapsed_ms)),
                Ok(reply) => {
                    output.push_str(&format!("\n3. GATEWAY\n  Estado: ICMP NO RESPONDE (código {})\n", reply.status));
                    findings.push(("ADVERTENCIA", "Gateway sin respuesta ICMP. Puede estar filtrado; comprobar ARP y 802.1X antes de concluir caída de enlace.".to_owned()));
                }
                Err(error) => output.push_str(&format!("\n3. GATEWAY\n  Estado: INDETERMINADO\n  Error: {error}\n")),
            }
        } else { output.push_str("\n3. GATEWAY\n  Estado: NO EVALUADO\n"); }

        let addresses = if let Ok(ip) = host.parse::<IpAddr>() {
            output.push_str(&format!("\n4. DNS\n  Estado: NO REQUERIDO ({ip})\n"));
            vec![ip]
        } else {
            match (host.as_str(), port).to_socket_addrs() {
                Ok(items) => {
                    let mut addresses = items.map(|addr| addr.ip()).collect::<Vec<_>>();
                    addresses.sort(); addresses.dedup();
                    if addresses.is_empty() {
                        output.push_str("\n4. DNS\n  Estado: SIN DIRECCIONES\n");
                        findings.push(("CRÍTICO", "El resolvedor no devolvió direcciones. Revisar DNS configurado.".to_owned()));
                    } else {
                        output.push_str(&format!("\n4. DNS\n  Estado: RESUELTO\n  Direcciones: {}\n",
                            addresses.iter().take(6).map(ToString::to_string).collect::<Vec<_>>().join(", ")));
                    }
                    addresses
                }
                Err(error) => {
                    output.push_str(&format!("\n4. DNS\n  Estado: ERROR\n  Evidencia: {error}\n"));
                    findings.push(("CRÍTICO", "Error de resolución DNS: comparar con acceso a dirección IP y verificar políticas DNS.".to_owned()));
                    Vec::new()
                }
            }
        };
        if let Some(ip) = addresses.iter().find_map(|ip| if let IpAddr::V4(v4) = ip { Some(*v4) } else { None }) {
            match self.probe.echo(ip, 64, Duration::from_millis(900)) {
                Ok(reply) if reply.status == 0 => output.push_str(&format!("\n5. ICMP DESTINO\n  Estado: RESPONDE\n  Latencia: {} ms\n", reply.elapsed_ms)),
                Ok(reply) => output.push_str(&format!("\n5. ICMP DESTINO\n  Estado: NO RESPONDE (código {})\n  Nota: ICMP puede estar filtrado.\n", reply.status)),
                Err(error) => output.push_str(&format!("\n5. ICMP DESTINO\n  Estado: INDETERMINADO\n  Error: {error}\n")),
            }
        } else { output.push_str("\n5. ICMP DESTINO\n  Estado: NO EVALUADO (sin IPv4)\n"); }

        let mut connected = false;
        if addresses.is_empty() {
            output.push_str("\n6. TCP\n  Estado: NO EVALUADO\n");
        } else {
            let mut error = String::new();
            for ip in addresses.iter().take(4) {
                let socket = SocketAddr::new(*ip, port);
                match TcpStream::connect_timeout(&socket, Duration::from_millis(1200)) {
                    Ok(stream) => {
                        drop(stream);
                        connected = true;
                        output.push_str(&format!("\n6. TCP\n  Estado: CONECTADO\n  Extremo: {socket}\n"));
                        break;
                    }
                    Err(e) => error = e.to_string(),
                }
            }
            if !connected {
                output.push_str(&format!("\n6. TCP\n  Estado: SIN CONEXIÓN\n  Error: {error}\n"));
                findings.push(("ADVERTENCIA", format!("No conecta TCP {port}. Revisar servicio remoto, firewall y ruta; no confirma avería física.")));
            }
        }
        output.push_str("\nCONCLUSIÓN\n");
        if connected {
            output.push_str("  Estado: CONECTIVIDAD TCP CONFIRMADA\n  La prueba no acredita acceso a otros servicios ni autorización NAC.\n");
        } else if findings.is_empty() {
            output.push_str("  Estado: INDETERMINADO\n  Evidencia insuficiente para atribuir una causa.\n");
        } else { output.push_str("  Estado: REQUIERE REVISIÓN\n"); }
        for (severity, finding) in findings {
            output.push_str(&format!("  {severity}: {finding}\n"));
        }
        output.push_str("\nEvidencia detallada: notsh lan show interfaces; notsh wlan show interfaces; net routes\n");
        Ok(CommandOutput::ok(output))
    }
}

fn ipv4_default_gateway(text: &str) -> Option<Ipv4Addr> {
    text.lines().skip(1).find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != "0.0.0.0/0" { return None; }
        let gateway = fields.next()?.parse::<Ipv4Addr>().ok()?;
        (gateway != Ipv4Addr::UNSPECIFIED).then_some(gateway)
    })
}

fn evaluate_interfaces(text: &str) -> (&'static str, &'static str, Option<(&'static str, String)>) {
    let apipa = text.lines().any(|line| line.trim_start().starts_with("IP: 169.254."));
    // Prefer verified physical Ethernet or Wi-Fi evidence over virtual NICs.
    let connected = text.lines().any(|line| line.trim() == "Estado: ENLACE ACTIVO")
        || (text.contains("Tipo: Wi-Fi") && text.lines().any(|line| line.trim() == "Estado: OPERATIVA"));
    if apipa {
        ("ADVERTENCIA", "IPv4 APIPA 169.254.x.x: no se obtuvo configuración IPv4 administrada.",
            Some(("ADVERTENCIA", "Revisar DHCP, VLAN y autorización NAC 802.1X.".to_owned())))
    } else if connected {
        ("ENLACE DETECTADO", "Interfaz operativa; no confirma acceso autorizado a la LAN.", None)
    } else if text.contains("Estado: SIN ENLACE") {
        ("SIN ENLACE", "No existe enlace Ethernet físico en al menos una interfaz.",
            Some(("CRÍTICO", "Revisar cable, toma de red, NIC y puerto del switch.".to_owned())))
    } else if text.contains("Estado: ENLACE SIN IP") {
        ("SIN IP", "Hay enlace físico pero no dirección IP.",
            Some(("ADVERTENCIA", "Revisar DHCP, VLAN, IP estática y NAC 802.1X.".to_owned())))
    } else {
        ("INDETERMINADO", "No hay evidencia suficiente de conectividad.", None)
    }
}

impl BuiltinCommand for NetcheckBuiltin {
    fn name(&self) -> &'static str { "netcheck" }
    fn help(&self) -> &'static str {
        "netcheck — diagnóstico de enlace, APIPA, DHCP, gateway, DNS, ICMP y TCP"
    }
    fn execute(&self, _invoked_name: &str, args: &[String], _context: CommandContext<'_>) -> Result<CommandOutput> {
        self.diagnose(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gateway_is_not_an_on_link_route() {
        let routes = "DESTINATION GATEWAY IFINDEX METRIC\n0.0.0.0/0 0.0.0.0 2 1\n0.0.0.0/0 10.0.0.1 2 1";
        assert_eq!(ipv4_default_gateway(routes), Some(Ipv4Addr::new(10, 0, 0, 1)));
    }
    #[test]
    fn apipa_is_not_reported_as_success() {
        let (status, _, finding) = evaluate_interfaces("[1] Ethernet\n  Estado: ENLACE ACTIVO\n  IP: 169.254.5.7/16\n");
        assert_eq!(status, "ADVERTENCIA");
        assert!(finding.expect("diagnosis").1.contains("DHCP"));
    }
    #[test]
    fn unplugged_cable_is_not_called_dns_failure() {
        let (status, _, finding) = evaluate_interfaces("[1] Ethernet\n  Estado: SIN ENLACE\n");
        assert_eq!(status, "SIN ENLACE");
        assert!(finding.expect("diagnosis").1.contains("cable"));
    }
}
