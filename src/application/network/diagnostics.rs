use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use dns_lookup::lookup_addr;
use ipnet::Ipv4Net;
use serde::Serialize;

use crate::core::{
    CommandOutput,
    ports::NetworkProbe,
};

use super::identity::identify_mac;

#[derive(Debug, Clone)]
pub struct HostEvidence {
    pub method: String,
    pub mac: Option<String>,
    pub latency_ms: Option<u32>,
}

pub struct NetworkDiagnosticsService {
    probe: Arc<dyn NetworkProbe>,
}

#[derive(Debug, Clone, Serialize)]
struct IpConflictObservation {
    ip: Ipv4Addr,
    macs: Vec<String>,
}

impl NetworkDiagnosticsService {
    pub fn new(probe: Arc<dyn NetworkProbe>) -> Self {
        Self { probe }
    }

    pub fn diagnose(&self) -> Result<CommandOutput> {
        let mut out = String::new();
        out.push_str("== interfaces ==\n");
        out.push_str(&self.interfaces()?.stdout);
        out.push_str("\n== vecinos ==\n");
        out.push_str(&self.arp_table()?.stdout);
        out.push_str("\n== conexiones ==\n");
        out.push_str(&self.connections()?.stdout);
        Ok(CommandOutput::ok(out))
    }

    pub fn interfaces(&self) -> Result<CommandOutput> {
        Ok(CommandOutput::ok(self.probe.interfaces()?))
    }

    pub fn connections(&self) -> Result<CommandOutput> {
        let mut output = String::from("PROTO LOCAL                         REMOTE                        STATE          PID\n");
        for row in self.probe.connections()? {
            output.push_str(&format!("{:<5} {:<29} {:<29} {:<14} {}\n", row.protocol, row.local, row.remote, row.state, row.pid));
        }
        Ok(CommandOutput::ok(output))
    }

    pub fn routes(&self) -> Result<CommandOutput> {
        Ok(CommandOutput::ok(self.probe.routes()?))
    }

    pub fn dns(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error("net dns: uso: net dns HOST|IP", 2));
        };

        if let Ok(ip) = target.parse::<IpAddr>() {
            let name = lookup_addr(&ip).unwrap_or_else(|_| "-".to_owned());
            return Ok(CommandOutput::ok(format!(
                "address: {ip}\nname: {name}\n"
            )));
        }

        let mut addresses: Vec<IpAddr> = (target.as_str(), 0)
            .to_socket_addrs()?
            .map(|socket| socket.ip())
            .collect();

        addresses.sort();
        addresses.dedup();

        if addresses.is_empty() {
            return Ok(CommandOutput::error(
                format!("net dns: no se pudo resolver {target}"),
                1,
            ));
        }

        let mut out = format!("name: {target}\n");
        for address in addresses {
            out.push_str(&format!("address: {address}\n"));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn ping(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error("net ping: uso: net ping HOST", 2));
        };

        let count = args
            .windows(2)
            .find(|pair| pair[0] == "-c")
            .map(|pair| pair[1].as_str())
            .unwrap_or("4");

        let count: u32 = count.parse()?;
        if count == 0 || count > 100 { return Ok(CommandOutput::error("net ping: -c debe estar entre 1 y 100", 2)); }
        let ip = resolve_ipv4(target)?;
        let mut output = format!("PING {target} ({ip})\n");
        let mut received = 0;
        for _ in 0..count {
            let reply = self.probe.echo(ip, 128, Duration::from_secs(1))?;
            if reply.status == 0 {
                received += 1;
                output.push_str(&format!("{}: {} ms\n", reply.address, reply.elapsed_ms));
            } else { output.push_str(&format!("{ip}: sin respuesta (estado {})\n", reply.status)); }
        }
        output.push_str(&format!("Enviados: {count}; recibidos: {received}\n"));
        Ok(CommandOutput { stdout: output, stderr: String::new(), status: if received > 0 { 0 } else { 1 } })
    }

    pub fn trace(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error("net trace: uso: net trace HOST", 2));
        };

        let ip = resolve_ipv4(target)?;
        let mut output = format!("TRACE {target} ({ip}), máximo 30 saltos\n");
        let mut reached = false;
        for ttl in 1..=30 {
            let reply = self.probe.echo(ip, ttl, Duration::from_millis(700))?;
            if reply.status == 0 || reply.status == 11013 {
                output.push_str(&format!("{ttl:<3} {:<16} {} ms\n", reply.address, reply.elapsed_ms));
            } else { output.push_str(&format!("{ttl:<3} * (estado {})\n", reply.status)); }
            if reply.status == 0 { reached = true; break; }
        }
        Ok(CommandOutput { stdout: output, stderr: String::new(), status: if reached { 0 } else { 1 } })
    }

    pub fn arp_map(&self) -> Result<HashMap<Ipv4Addr, String>> {
        self.probe.neighbors()
    }

    pub fn arp_table(&self) -> Result<CommandOutput> {
        let mut rows: Vec<_> = self.arp_map()?.into_iter().collect();
        rows.sort_by_key(|row| row.0);

        let mut out = String::from(
            "IP               MAC                 TYPE           VENDOR\n",
        );
        for (ip, mac) in rows {
            let identity = identify_mac(&mac);
            out.push_str(&format!(
                "{:<16} {:<19} {:<14} {}\n",
                ip,
                mac,
                identity.scope,
                identity.vendor.as_deref().unwrap_or("-")
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn conflicts(&self, args: &[String]) -> Result<CommandOutput> {
        let json = args.iter().any(|arg| arg == "--json");
        let seconds = args
            .windows(2)
            .find(|pair| pair[0] == "--seconds" || pair[0] == "-s")
            .map(|pair| pair[1].parse::<u64>())
            .transpose()?
            .unwrap_or(5);

        if !(1..=30).contains(&seconds) {
            return Ok(CommandOutput::error(
                "net conflicts: --seconds debe estar entre 1 y 30",
                2,
            ));
        }

        let target = args
            .iter()
            .find(|arg| !arg.starts_with('-') && arg.parse::<u64>().is_err())
            .map(|arg| arg.parse::<Ipv4Addr>())
            .transpose()?;

        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut seen: HashMap<Ipv4Addr, HashSet<String>> = HashMap::new();
        let mut samples = 0u32;

        loop {
            if let Some(ip) = target {
                // SendARP actively asks for the owner of this IPv4. Repeating it
                // gives Windows a chance to observe ownership changes caused by
                // two hosts answering for the same address.
                if let Some(mac) = self.resolve_neighbor(ip) {
                    seen.entry(ip)
                        .or_default()
                        .insert(mac.to_ascii_uppercase());
                }
            } else if let Ok(snapshot) = self.arp_map() {
                for (ip, mac) in snapshot {
                    seen.entry(ip)
                        .or_default()
                        .insert(mac.to_ascii_uppercase());
                }
            }

            samples = samples.saturating_add(1);
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        let mut conflicts = seen
            .into_iter()
            .filter_map(|(ip, macs)| {
                if macs.len() < 2 {
                    return None;
                }
                let mut macs = macs.into_iter().collect::<Vec<_>>();
                macs.sort();
                Some(IpConflictObservation { ip, macs })
            })
            .collect::<Vec<_>>();
        conflicts.sort_by_key(|row| row.ip);

        if json {
            return Ok(CommandOutput {
                stdout: format!("{}\n", serde_json::to_string_pretty(&conflicts)?),
                stderr: String::new(),
                status: if conflicts.is_empty() { 0 } else { 1 },
            });
        }

        let mut out = format!(
            "IP duplicate check: {samples} muestras durante {seconds}s\n"
        );
        if conflicts.is_empty() {
            out.push_str(
                "No se observó una IPv4 respondiendo con más de una MAC durante la ventana.\n",
            );
            out.push_str(
                "Nota: esto no demuestra ausencia de conflicto; una caché ARP estable puede ocultar al segundo equipo. Para una IP sospechosa usa: net conflicts IP --seconds 10\n",
            );
            return Ok(CommandOutput::ok(out));
        }

        out.push_str("CONFLICTO        MACS OBSERVADAS\n");
        for row in &conflicts {
            out.push_str(&format!("{:<16} {}\n", row.ip, row.macs.join(", ")));
            for mac in &row.macs {
                let identity = identify_mac(mac);
                out.push_str(&format!(
                    "                 {:<19} {}\n",
                    mac,
                    identity.vendor.as_deref().unwrap_or(&identity.scope)
                ));
            }
        }

        Ok(CommandOutput {
            stdout: out,
            stderr: String::new(),
            status: 1,
        })
    }

    pub fn ports(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(host) = args.first() else {
            return Ok(CommandOutput::error(
                "net ports: uso: net ports HOST 22,80,443",
                2,
            ));
        };

        let port_text = args.get(1).map(String::as_str).unwrap_or("22,80,443,445,3389");
        let ports: Vec<u16> = port_text
            .split(',')
            .filter_map(|value| value.trim().parse().ok())
            .collect();

        let mut out = String::from("PORT     STATE\n");

        for port in ports {
            let address = format!("{host}:{port}");
            let socket = address
                .to_socket_addrs()
                .ok()
                .and_then(|mut values| values.next());

            let open = socket
                .and_then(|socket| TcpStream::connect_timeout(&socket, Duration::from_millis(500)).ok())
                .is_some();

            out.push_str(&format!(
                "{:<8} {}\n",
                port,
                if open { "open" } else { "closed/filtered" }
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn resolve_neighbor(&self, ip: Ipv4Addr) -> Option<String> {
        self.probe.resolve_neighbor(ip).ok().flatten()
    }

    pub fn probe_host(&self, ip: Ipv4Addr, timeout: Duration) -> Option<HostEvidence> {
        let arp_started = Instant::now();
        if let Some(mac) = self.resolve_neighbor(ip) {
            return Some(HostEvidence {
                method: "arp".to_owned(),
                mac: Some(mac),
                latency_ms: Some(duration_ms(arp_started.elapsed())),
            });
        }

        if let Ok(reply) = self.probe.echo(ip, 128, timeout)
            && reply.status == 0
        {
            return Some(HostEvidence {
                method: "icmp".to_owned(),
                mac: None,
                latency_ms: Some(reply.elapsed_ms),
            });
        }

        for port in [445_u16, 3389, 80, 443, 135, 22] {
            let started = Instant::now();
            if TcpStream::connect_timeout(&SocketAddr::new(IpAddr::V4(ip), port), timeout).is_ok() {
                return Some(HostEvidence {
                    method: format!("tcp:{port}"),
                    mac: None,
                    latency_ms: Some(duration_ms(started.elapsed())),
                });
            }
        }

        None
    }

    pub fn default_ipv4_network(&self) -> Result<Ipv4Net> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect("8.8.8.8:80")?;

        let IpAddr::V4(ip) = socket.local_addr()?.ip() else {
            anyhow::bail!("no se pudo determinar una IPv4 local");
        };

        Ok(Ipv4Net::new(ip, 24)?)
    }

}

fn duration_ms(duration: Duration) -> u32 {
    duration.as_millis().min(u32::MAX as u128) as u32
}

fn resolve_ipv4(host: &str) -> Result<Ipv4Addr> {
    (host, 0).to_socket_addrs()?.find_map(|address| match address.ip() {
        IpAddr::V4(ip) => Some(ip), _ => None,
    }).ok_or_else(|| anyhow::anyhow!("No se encontró una dirección IPv4 para {host}"))
}
