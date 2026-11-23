//! Diagnostic measurements for SST: native ICMP loss/stability, IP Helper
//! interface throughput and ETW per-process traffic. Never reports synthetic
//! bandwidth or treats encrypted transport bytes as readable payload.
use std::{
    collections::{HashMap, HashSet},
    net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};
use anyhow::Result;
use sysinfo::System;
use crate::{
    adapters::windows::network::interface_counters,
    core::{CommandOutput, ports::{NetworkProbe, TrafficMonitorFactory}},
};

pub fn execute(
    args: &[String],
    probe: &dyn NetworkProbe,
    traffic: &dyn TrafficMonitorFactory,
) -> Result<CommandOutput> {
    let Some((command, rest)) = args.split_first() else {
        return Ok(CommandOutput::error("netcheck: falta subcomando", 2));
    };
    match command.as_str() {
        "loss" => loss(rest, probe, false),
        "stability" => loss(rest, probe, true),
        "speed" => speed(rest),
        "capture" => capture(rest, probe),
        "traffic" | "inspect" | "packets" => traffic_report(rest, probe, traffic, command),
        _ => Ok(CommandOutput::error("netcheck: subcomando desconocido", 2)),
    }
}

fn option_u32(args: &[String], flag: &str, default: u32, min: u32, max: u32) -> Result<u32, String> {
    let Some(index) = args.iter().position(|arg| arg == flag) else { return Ok(default); };
    let Some(value) = args.get(index + 1) else {
        return Err(format!("{flag}: falta valor ({min}..{max})"));
    };
    let Ok(parsed) = value.parse::<u32>() else {
        return Err(format!("{flag}: se esperaba un entero ({min}..{max})"));
    };
    if !(min..=max).contains(&parsed) {
        return Err(format!("{flag}: fuera de rango ({min}..{max})"));
    }
    Ok(parsed)
}

fn option_string<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let position = args.iter().position(|arg| arg == flag)?;
    args.get(position + 1).map(String::as_str)
}

fn unknown_arguments(args: &[String], options: &[&str], positionals: usize) -> Option<String> {
    let mut count = 0;
    let mut i = 0;
    while i < args.len() {
        if options.contains(&args[i].as_str()) {
            if i + 1 >= args.len() { return Some(format!("{} requiere un valor", args[i])); }
            i += 2;
        } else if args[i].starts_with('-') {
            return Some(format!("opción desconocida: {}", args[i]));
        } else {
            count += 1;
            if count > positionals { return Some(format!("argumento inesperado: {}", args[i])); }
            i += 1;
        }
    }
    None
}

fn resolve_ipv4(host: &str) -> Result<Ipv4Addr, String> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() { return Ok(ip); }
    (host, 0).to_socket_addrs()
        .map_err(|error| format!("DNS: {error}"))?
        .find_map(|address| match address.ip() {
            std::net::IpAddr::V4(ip) => Some(ip),
            std::net::IpAddr::V6(_) => None,
        })
        .ok_or_else(|| "DNS: no hay dirección IPv4; ICMPv6 aún no está soportado".to_owned())
}

#[derive(Debug, Default)]
struct Samples {
    sent: u32,
    received: u32,
    rejected: u32,
    times: Vec<f64>,
    longest_failures: u32,
    failures_in_row: u32,
}
impl Samples {
    fn success(&mut self, millis: f64) {
        self.sent += 1;
        self.received += 1;
        self.failures_in_row = 0;
        self.times.push(millis);
    }
    fn missed(&mut self) {
        self.sent += 1;
        self.failures_in_row += 1;
        self.longest_failures = self.longest_failures.max(self.failures_in_row);
    }
    fn unknown(&mut self) {
        self.rejected += 1;
        self.failures_in_row = 0;
    }
    fn report(&self, title: &str, host: &str, protocol: &str) -> String {
        let lost = self.sent.saturating_sub(self.received);
        let loss = if self.sent == 0 { 0.0 } else {
            100.0 * f64::from(lost) / f64::from(self.sent)
        };
        let mut out = format!(
            "{title}\nDestino: {host}\nPrueba: {protocol}\n\
             Intentos válidos: {}\nRecibidos: {}\nSin respuesta: {lost}\n\
             Pérdida: {loss:.1}%\nErrores locales de la API: {}\n\
             Mayor secuencia de fallos: {}\n",
            self.sent, self.received, self.rejected, self.longest_failures
        );
        if !self.times.is_empty() {
            let minimum = self.times.iter().copied().fold(f64::INFINITY, f64::min);
            let maximum = self.times.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mean = self.times.iter().sum::<f64>() / self.times.len() as f64;
            let jitter = if self.times.len() >= 2 {
                self.times.windows(2).map(|pair| (pair[1] - pair[0]).abs()).sum::<f64>()
                    / (self.times.len() - 1) as f64
            } else { 0.0 };
            out.push_str(&format!(
                "RTT mínimo/promedio/máximo: {minimum:.1}/{mean:.1}/{maximum:.1} ms\n\
                 Variación media entre respuestas: {jitter:.1} ms\n"
            ));
        }
        if self.sent == 0 {
            out.push_str("Estado: INDETERMINADO — sin intentos ICMP/TCP evaluables\n");
        } else if lost > 0 {
            out.push_str("Estado: PÉRDIDA O FALLO DE RESPUESTA DETECTADO\n");
        } else {
            out.push_str("Estado: SIN PÉRDIDAS OBSERVADAS EN LA MUESTRA\n");
        }
        out
    }
}

fn loss(args: &[String], probe: &dyn NetworkProbe, stability: bool) -> Result<CommandOutput> {
    let options = ["--count","--interval","--timeout","--seconds","--tcp"];
    if let Some(problem) = unknown_arguments(args, &options, 1) {
        return Ok(CommandOutput::error(format!("netcheck: {problem}"), 2));
    }
    let parse = |flag, default, min, max| option_u32(args, flag, default, min, max);
    let count = match parse("--count", if stability { 0 } else { 20 }, 0, 120) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let seconds = match parse("--seconds", 10, 1, 120) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let interval = match parse("--interval", if stability { 1000 } else { 400 }, 100, 5000) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let timeout = match parse("--timeout", 800, 200, 5000) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let tcp = if args.iter().any(|arg| arg == "--tcp") {
        match parse("--tcp", 443, 1, 65535) {
            Ok(port) => Some(port as u16),
            Err(error) => return Ok(CommandOutput::error(error, 2)),
        }
    } else { None };
    if !stability && count == 0 {
        return Ok(CommandOutput::error("netcheck loss: --count debe ser 1..120", 2));
    }

    let host = args.iter().enumerate()
        .find(|(i, value)| !value.starts_with('-') && (*i == 0
            || !options.contains(&args[i - 1].as_str())))
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| {
            let default_gateway = probe.routes().ok().and_then(|routes|
                routes.lines().skip(1).find_map(|line| {
                    let mut fields = line.split_whitespace();
                    if fields.next()? != "0.0.0.0/0" { return None; }
                    let ip = fields.next()?.parse::<Ipv4Addr>().ok()?;
                    (ip != Ipv4Addr::UNSPECIFIED).then_some(ip.to_string())
                })
            );
            default_gateway.unwrap_or_default()
        });
    if host.is_empty() {
        return Ok(CommandOutput::error(
            "netcheck: especifica HOST; no existe gateway IPv4 predeterminado", 2));
    }
    let ip = match resolve_ipv4(&host) {
        Ok(ip) => ip,
        Err(error) => return Ok(CommandOutput::error(format!("netcheck: {error}"), 1)),
    };
    let planned = if stability {
        ((seconds * 1000) / interval).max(1).min(120)
    } else { count };
    let clock = Instant::now();
    let mut samples = Samples::default();
    let mut events = String::new();
    for index in 0..planned {
        let attempt = Instant::now();
        let outcome = if let Some(port) = tcp {
            match TcpStream::connect_timeout(&SocketAddr::new(ip.into(), port), Duration::from_millis(timeout as u64)) {
                Ok(stream) => { drop(stream); Some(attempt.elapsed().as_secs_f64() * 1000.0) },
                Err(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::HostUnreachable) => Some(-1.0),
                Err(_) => None,
            }
        } else {
            match probe.echo(ip, 64, Duration::from_millis(timeout as u64)) {
                Ok(reply) => Some(if reply.status == 0 { f64::from(reply.elapsed_ms) } else { -1.0 }),
                Err(_) => None,
            }
        };
        match outcome {
            Some(ms) if ms >= 0.0 => {
                samples.success(ms);
            }
            Some(_) => samples.missed(),
            None => samples.unknown(),
        }
        if index < 24 {
            let state = match outcome { Some(ms) if ms >= 0.0 => "OK", Some(_) => "SIN RESPUESTA", None => "ERROR LOCAL" };
            events.push_str(&format!("  Muestra {:>3}: {state}\n", index + 1));
        }
        let remaining = if stability { clock.elapsed() < Duration::from_secs(seconds as u64) }
            else { true };
        if index + 1 < planned && remaining {
            let wait = Duration::from_millis(interval as u64).saturating_sub(attempt.elapsed());
            thread::sleep(wait);
        }
    }
    let mut out = samples.report(if stability { "ESTABILIDAD DE RED" } else { "PÉRDIDA DE PAQUETES" },
        &format!("{host} ({ip})"), if tcp.is_some() { "Conexión TCP (no mide pérdida de paquetes IP)" } else { "ICMP Echo" });
    out.push_str(&format!("Duración: {:.1} s\n", clock.elapsed().as_secs_f64()));
    if tcp.is_some() {
        out.push_str("Nota: los fallos TCP no son porcentaje de pérdida de paquetes; se muestra disponibilidad del servicio.\n");
    } else {
        out.push_str("Nota: si el destino filtra ICMP, una pérdida del 100% no demuestra una caída de red.\n");
    }
    out.push_str(&events);
    Ok(CommandOutput::ok(out))
}

fn capture(args: &[String], probe: &dyn NetworkProbe) -> Result<CommandOutput> {
    let options = ["--interface","--seconds","--limit","--payload-bytes"];
    if let Some(problem) = unknown_arguments(args, &options, 0) {
        return Ok(CommandOutput::error(format!("netcheck capture: {problem}"), 2));
    }
    let Some(address) = option_string(args, "--interface") else {
        return Ok(CommandOutput::error(
            "netcheck capture: indica --interface IPv4_LOCAL (obligatoria)\n\
             Ejemplo: netcheck capture --interface 192.168.1.50 --seconds 5 --limit 100\n\
             Requiere Administrador. --payload-bytes N muestra N bytes hexadecimales (0..64).", 2));
    };
    let Ok(local) = address.parse::<Ipv4Addr>() else {
        return Ok(CommandOutput::error("netcheck capture: --interface requiere IPv4 local", 2));
    };
    let seconds = match option_u32(args, "--seconds", 5, 1, 30) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let limit = match option_u32(args, "--limit", 100, 1, 500) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let payload = match option_u32(args, "--payload-bytes", 0, 0, 64) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let connections = probe.connections().unwrap_or_default();
    let mut system = System::new_all();
    system.refresh_all();
    let names = system.processes().iter().map(|(pid, process)| (
        pid.as_u32(), process.name().to_string_lossy().into_owned()
    )).collect::<HashMap<_,_>>();
    let services = crate::adapters::windows::service_map::services_by_pid().unwrap_or_default();
    let mut owners = Vec::new();
    for row in &connections {
        if !matches!(row.protocol.as_str(), "TCP" | "UDP") { continue; }
        let Ok(local_addr) = row.local.parse::<SocketAddr>() else { continue; };
        let std::net::IpAddr::V4(local_ip) = local_addr.ip() else { continue };
        if local_ip != local && !local_ip.is_unspecified() { continue; }
        let remote_addr = row.remote.parse::<SocketAddr>().ok();
        let remote_ip = remote_addr.as_ref().and_then(|addr| match addr.ip() {
            std::net::IpAddr::V4(ip) => Some(ip),
            std::net::IpAddr::V6(_) => None,
        });
        let remote_port = remote_addr.map(|addr| addr.port());
        owners.push(crate::adapters::windows::raw_capture::FlowOwner {
            protocol: row.protocol.clone(),
            local_port: local_addr.port(),
            remote_ip,
            remote_port,
            pid: row.pid,
            process: names.get(&row.pid).cloned().unwrap_or_else(|| "-".to_owned()),
            services: services.get(&row.pid).cloned().unwrap_or_default(),
        });
    }
    match crate::adapters::windows::raw_capture::capture(local, seconds, limit, payload as usize, &owners) {
        Ok(report) => Ok(CommandOutput::ok(report)),
        Err(error) => Ok(CommandOutput::error(format!(
            "netcheck capture: {error:#}\n\
             No se ha simulado captura alguna. Verifica IPv4 local, permisos y política del adaptador.", 1)),
    }
}

fn speed(args: &[String]) -> Result<CommandOutput> {
    let flags = ["--seconds","--download","--max-mib"];
    if let Some(error) = unknown_arguments(args, &flags, 0) {
        return Ok(CommandOutput::error(format!("netcheck speed: {error}"), 2));
    }
    let seconds = match option_u32(args, "--seconds", 4, 1, 30) {
        Ok(n) => n,
        Err(e) => return Ok(CommandOutput::error(e, 2)),
    };
    let before = interface_counters()?;
    if before.is_empty() {
        return Ok(CommandOutput::error("netcheck speed: no hay interfaces Ethernet/Wi-Fi activas", 1));
    }
    let started = Instant::now();
    let max_mib = match option_u32(args, "--max-mib", 64, 1, 256) {
        Ok(value) => value,
        Err(error) => return Ok(CommandOutput::error(error, 2)),
    };
    let active = option_string(args, "--download").map(|url| {
        crate::adapters::windows::https_download::measure(url, seconds, max_mib)
    });
    if active.is_none() { thread::sleep(Duration::from_secs(seconds as u64)); }
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let after = interface_counters()?;
    let by_index = before.into_iter().map(|row| (row.index, row)).collect::<HashMap<_,_>>();
    let mut report = String::from(
        "VELOCIDAD Y ESTADO DE INTERFACES — IP HELPER NATIVO\n\
         Modo pasivo: mide tráfico real observado durante la ventana, NO capacidad máxima de Internet.\n");
    report.push_str(&format!("Ventana: {elapsed:.2} s\n"));
    for sample in after {
        if let Some(old) = by_index.get(&sample.index) {
            let rx_mbps = rate_mbps(sample.rx_bytes.saturating_sub(old.rx_bytes), elapsed);
            let tx_mbps = rate_mbps(sample.tx_bytes.saturating_sub(old.tx_bytes), elapsed);
            report.push_str(&format!(
                "\nAdaptador: {} ({})\n  Enlace negociado RX/TX: {:.1}/{:.1} Mbps\n\
                 Tráfico real RX/TX: {rx_mbps:.2}/{tx_mbps:.2} Mbps\n\
                 Bytes recibidos/enviados: {}/{}\n\
                 Errores RX/TX: {}/{}\nDescartes RX/TX: {}/{}\n",
                sample.name, sample.medium,
                sample.link_rx_bps as f64 / 1e6, sample.link_tx_bps as f64 / 1e6,
                sample.rx_bytes.saturating_sub(old.rx_bytes),
                sample.tx_bytes.saturating_sub(old.tx_bytes),
                sample.rx_errors.saturating_sub(old.rx_errors),
                sample.tx_errors.saturating_sub(old.tx_errors),
                sample.rx_discards.saturating_sub(old.rx_discards),
                sample.tx_discards.saturating_sub(old.tx_discards),
            ));
        }
    }
    if let Some(active) = active {
        match active {
            Ok(measure) => report.push_str(&format!(
                "\nTRANSFERENCIA HTTPS ACTIVA (destino elegido explícitamente)\n\
                 URL: {}\nEstado HTTP: {}\nBytes descargados: {}\n\
                 Duración de descarga: {:.2} s\n\
                 Velocidad de descarga medida: {:.2} Mbps\n\
                 No es un resultado de subida ni velocidad máxima universal.\n",
                measure.url, measure.http_status, measure.bytes,
                measure.elapsed_seconds, rate_mbps(measure.bytes, measure.elapsed_seconds),
            )),
            Err(error) => report.push_str(&format!(
                "\nTRANSFERENCIA HTTPS ACTIVA: FALLÓ\nMotivo: {error}\n\
                 No se inventa una velocidad de descarga.\n"
            )),
        }
    } else {
        report.push_str("\nPrueba activa de descarga: netcheck speed --download https://SERVIDOR/archivo.bin\n\
                         El servidor y el volumen de descarga deben ser conocidos y autorizados.\n");
    }
    Ok(CommandOutput::ok(report))
}

fn rate_mbps(bytes: u64, seconds: f64) -> f64 {
    8.0 * bytes as f64 / seconds.max(0.001) / 1e6
}

fn traffic_report(
    args: &[String], probe: &dyn NetworkProbe, factory: &dyn TrafficMonitorFactory,
    mode: &str,
) -> Result<CommandOutput> {
    let flags = ["--seconds","--pid","--top"];
    if let Some(error) = unknown_arguments(args, &flags, 0) {
        return Ok(CommandOutput::error(format!("netcheck traffic: {error}"), 2));
    }
    let seconds = match option_u32(args, "--seconds", 3, 1, 30) {
        Ok(n) => n,
        Err(e) => return Ok(CommandOutput::error(e, 2)),
    };
    let top = match option_u32(args, "--top", 12, 1, 100) {
        Ok(n) => n as usize,
        Err(e) => return Ok(CommandOutput::error(e, 2)),
    };
    let pid = if args.iter().any(|arg| arg == "--pid") {
        match option_u32(args, "--pid", 0, 1, u32::MAX) {
            Ok(n) => Some(n),
            Err(e) => return Ok(CommandOutput::error(e, 2)),
        }
    } else { None };
    let monitor = match factory.start() {
        Ok(monitor) => monitor,
        Err(error) => return Ok(CommandOutput::error(format!(
            "netcheck {mode}: ETW no disponible: {error}\n\
             Verifica permisos de administración/servicios de seguimiento de Windows; no se sustituyen cifras por estimaciones."), 1))
    };
    let before = monitor.snapshot();
    let clock = Instant::now();
    thread::sleep(Duration::from_secs(seconds as u64));
    let after = monitor.snapshot();
    let events = if mode == "packets" { monitor.events() } else { Vec::new() };
    let rates = factory.rates(&before, &after, clock.elapsed());
    let sockets = probe.connections()?;
    let mut process_system = System::new_all();
    process_system.refresh_all();
    let names = process_system.processes().iter().map(|(pid, process)| (
        pid.as_u32(), process.name().to_string_lossy().into_owned()
    )).collect::<HashMap<_,_>>();
    let services = crate::adapters::windows::service_map::services_by_pid();
    let mut rows = rates.into_iter().filter(|(id,counters)| {
        (counters.sent > 0 || counters.received > 0)
            && pid.is_none_or(|filter| filter == *id)
    }).collect::<Vec<_>>();
    rows.sort_by(|a,b| (b.1.sent + b.1.received).cmp(&(a.1.sent + a.1.received)));
    let mut report = format!(
        "TRÁFICO POR PROCESO — ETW + WINDOWS IP HELPER\n\
         Ventana de observación: {:.2} s\n\
         Origen: eventos ETW de tamaño y dirección, correlacionados con conexiones TCP/UDP actuales.\n\
         Estos contadores no contienen ni descifran payload, archivos ni mensajes de usuario.\n\n",
        clock.elapsed().as_secs_f64()
    );
    if rows.is_empty() {
        report.push_str("Sin tráfico atribuible durante la muestra, o eventos no accesibles a ETW.\n");
    }
    for (id, counter) in rows.into_iter().take(top) {
        let name = names.get(&id).map(String::as_str).unwrap_or("proceso finalizado/desconocido");
        report.push_str(&format!("PID {id} | {name} | TX {} B/s | RX {} B/s\n",
            counter.sent, counter.received));
        if let Ok(service_map) = &services {
            if let Some(items) = service_map.get(&id) {
                report.push_str(&format!("  Servicios Windows asociados: {}\n", items.join(", ")));
                if items.len() > 1 {
                    report.push_str("  ATENCIÓN: PID compartido; ETW no permite atribuir cada byte a un servicio individual.\n");
                }
            }
        } else {
            report.push_str("  Consulta SCM no disponible: no se confirmó qué servicios comparten este PID.\n");
        }
        let mut shown = HashSet::new();
        for socket in sockets.iter().filter(|socket| socket.pid == id) {
            let endpoint = format!("{} {} -> {} [{}]",
                socket.protocol, socket.local, socket.remote, socket.state);
            if shown.insert(endpoint.clone()) {
                report.push_str(&format!("  Conexión: {endpoint}\n"));
                if shown.len() >= 5 { break; }
            }
        }
        if shown.is_empty() {
            report.push_str("  Sin conexión identificable en la instantánea (posible sesión ya finalizada).\n");
        }
    }
    if mode == "packets" {
        report.push_str("\nEVENTOS DE RED OBSERVADOS (ETW, hasta 1024 recientes)\n");
        let mut seen = 0usize;
        for event in &events {
            if pid.is_some_and(|filter| filter != event.pid) { continue; }
            let process = names.get(&event.pid).map(String::as_str).unwrap_or("desconocido");
            report.push_str(&format!(
                "  {} | {} B | PID {} ({}) | ETW ID {}\n",
                event.direction, event.size, event.pid, process, event.event_id
            ));
            seen += 1;
            if seen >= 40 { break; }
        }
        if seen == 0 {
            report.push_str("  Ningún evento ETW con tamaño y PID válido observado.\n");
        }
        report.push_str("  Los eventos pueden no representar una trama Ethernet individual.\n");
        report.push_str("\nLÍMITE DE CAPTURA\n  ETW de tamaños/PID no es una captura de paquetes.\n\
                         No se identifican cargas útiles, URL HTTPS, cuerpos de solicitudes ni archivos.\n\
                         Para inspección de contenido se requiere una captura autorizada con un controlador NDIS/WFP.\n\
                         El tráfico cifrado TLS permanece cifrado.\n");
    }
    if mode == "inspect" {
        report.push_str("\nLos destinos/puertos son puntos de conexión observados; no prueban qué datos internos transmitió cada servicio.\n");
    }
    Ok(CommandOutput::ok(report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loss_counts_missing_separately_from_local_errors() {
        let mut samples = Samples::default();
        samples.success(10.0);
        samples.missed();
        samples.missed();
        samples.unknown();
        samples.success(30.0);
        assert_eq!(samples.sent, 4);
        assert_eq!(samples.received, 2);
        assert_eq!(samples.rejected, 1);
        assert_eq!(samples.longest_failures, 2);
        let output = samples.report("TEST", "localhost", "ICMP");
        assert!(output.contains("Pérdida: 50.0%"));
        assert!(output.contains("Variación media entre respuestas: 20.0 ms"));
    }

    #[test]
    fn reject_unbounded_duration_and_unknown_arguments() {
        let args = vec!["--seconds".to_owned(), "9999".to_owned()];
        assert!(option_u32(&args, "--seconds", 4, 1, 30).is_err());
        assert!(unknown_arguments(&["--shell".into()], &["--seconds"], 0).is_some());
    }

    #[test]
    fn speed_units_are_measured_bytes_per_second() {
        assert!((rate_mbps(12_500_000, 1.0) - 100.0).abs() < 0.00001);
    }
}
