use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::{Local, Utc};
use dns_lookup::lookup_addr;
use ipnet::Ipv4Net;
use serde::Serialize;

use crate::{
    application::device::normalize_mac,
    core::{
        CommandOutput,
        models::network::{PresenceRecord, ScanRow},
        ports::{DeviceRepository, PresenceRepository, TerminalFactory, TerminalKey},
    },
    support::csv,
};

use super::{
    NetworkDiagnosticsService,
    identity::identify_mac,
    lan_note::{LAN_NOTE_PORT, LanNote, LanNoteBus, sanitize_message},
};

const OFFLINE_MISSES: u8 = 3;

pub struct NetworkDiscoveryService {
    diagnostics: Arc<NetworkDiagnosticsService>,
    devices: Arc<dyn DeviceRepository>,
    presence: Arc<dyn PresenceRepository>,
    terminal: Arc<dyn TerminalFactory>,
}

#[derive(Debug, Serialize)]
struct IdentifyResult {
    target: String,
    ip: Option<String>,
    hostname: Option<String>,
    mac: Option<String>,
    mac_scope: String,
    vendor: Option<String>,
    ieee_registry: Option<String>,
    inventory_name: Option<String>,
    registered: bool,
    discovery: Option<String>,
    response_ms: Option<u32>,
    last_seen: Option<String>,
}

impl NetworkDiscoveryService {
    pub fn new(
        diagnostics: Arc<NetworkDiagnosticsService>,
        devices: Arc<dyn DeviceRepository>,
        presence: Arc<dyn PresenceRepository>,
        terminal: Arc<dyn TerminalFactory>,
    ) -> Self {
        Self {
            diagnostics,
            devices,
            presence,
            terminal,
        }
    }

    pub fn scan(&self, args: &[String]) -> Result<CommandOutput> {
        let network = self.network_from_args(args, "net scan")?;
        let only_unknown = args.iter().any(|arg| arg == "--unknown");
        let only_known = args.iter().any(|arg| arg == "--authorized" || arg == "--known");
        let json = args.iter().any(|arg| arg == "--json");
        let csv_output = args.iter().any(|arg| arg == "--csv");
        let names_only = args.iter().any(|arg| arg == "--names");

        let mut rows = self.scan_rows(network)?;

        if only_unknown {
            rows.retain(|row| !row.known);
        } else if only_known {
            rows.retain(|row| row.known);
        }

        if json {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&rows)?
            )));
        }

        if names_only {
            let mut out = String::from("IP               NOMBRE\n");
            for row in &rows {
                out.push_str(&format!("{:<16} {}\n", row.ip, display_name(row)));
            }
            return Ok(CommandOutput::ok(out));
        }

        if csv_output {
            let mut out = String::from(
                "ip,mac,hostname,latency_ms,discovery,mac_scope,vendor,known,inventory_name\n",
            );
            for row in rows {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{},{},{}\n",
                    row.ip,
                    row.mac,
                    csv::escape(&row.hostname),
                    row.latency_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    csv::escape(&row.discovery),
                    csv::escape(&row.mac_scope),
                    csv::escape(row.vendor.as_deref().unwrap_or("")),
                    row.known,
                    csv::escape(row.inventory_name.as_deref().unwrap_or(""))
                ));
            }
            return Ok(CommandOutput::ok(out));
        }

        Ok(CommandOutput::ok(render_scan_rows(&rows)))
    }

    pub fn identify(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.iter().find(|arg| !arg.starts_with('-')) else {
            return Ok(CommandOutput::error(
                "net identify: uso: net identify IP|MAC|NOMBRE [--json]",
                2,
            ));
        };
        let json = args.iter().any(|arg| arg == "--json");
        let devices = self.devices.all()?;
        let history = self.presence.all()?;

        let mut ip = None::<Ipv4Addr>;
        let mut hostname = None::<String>;
        let mut mac = None::<String>;
        let mut discovery = None::<String>;
        let mut response_ms = None::<u32>;

        if let Ok(address) = target.parse::<Ipv4Addr>() {
            ip = Some(address);
            if let Some(evidence) = self
                .diagnostics
                .probe_host(address, Duration::from_millis(300))
            {
                mac = evidence.mac;
                discovery = Some(evidence.method);
                response_ms = evidence.latency_ms;
            }
            if mac.is_none() {
                mac = self
                    .diagnostics
                    .arp_map()
                    .ok()
                    .and_then(|mut table| table.remove(&address));
            }
            hostname = lookup_addr(&IpAddr::V4(address)).ok();
        } else if let Ok(normalized) = normalize_mac(target) {
            mac = Some(normalized);
        } else if let Some(device) = devices
            .iter()
            .find(|device| device.name.eq_ignore_ascii_case(target))
        {
            mac = Some(device.mac.clone());
        } else {
            return Ok(CommandOutput::error(
                format!("net identify: no se reconoce IP, MAC ni nombre inventariado: {target}"),
                1,
            ));
        }

        let mut inventory_name = mac.as_ref().and_then(|wanted| {
            devices
                .iter()
                .find(|device| device.mac.eq_ignore_ascii_case(wanted))
                .map(|device| device.name.clone())
        });

        let latest = mac
            .as_ref()
            .and_then(|wanted| {
                history
                    .iter()
                    .filter(|record| record.mac.eq_ignore_ascii_case(wanted))
                    .max_by(|a, b| a.last_seen.cmp(&b.last_seen))
            })
            .or_else(|| {
                ip.and_then(|wanted| {
                    history
                        .iter()
                        .filter(|record| record.ip == wanted.to_string())
                        .max_by(|a, b| a.last_seen.cmp(&b.last_seen))
                })
            });

        let mut last_seen = None::<String>;
        if let Some(record) = latest {
            if ip.is_none() {
                ip = record.ip.parse::<Ipv4Addr>().ok();
            }
            if hostname
                .as_deref()
                .is_none_or(|value| value == "-" || value.trim().is_empty())
                && record.hostname != "-"
                && !record.hostname.trim().is_empty()
            {
                hostname = Some(record.hostname.clone());
            }
            if discovery.is_none() && !record.discovery.is_empty() {
                discovery = Some(record.discovery.clone());
            }
            if mac.is_none() && !record.mac.starts_with("??") {
                mac = Some(record.mac.clone());
            }
            last_seen = Some(record.last_seen.clone());
        }

        if inventory_name.is_none() {
            inventory_name = mac.as_ref().and_then(|wanted| {
                devices
                    .iter()
                    .find(|device| device.mac.eq_ignore_ascii_case(wanted))
                    .map(|device| device.name.clone())
            });
        }

        let mac_identity = mac
            .as_deref()
            .map(identify_mac)
            .unwrap_or_else(|| identify_mac("??:??:??:??:??:??"));

        let result = IdentifyResult {
            target: target.as_str().to_owned(),
            ip: ip.map(|value| value.to_string()),
            hostname,
            mac,
            mac_scope: mac_identity.scope,
            vendor: mac_identity.vendor,
            ieee_registry: mac_identity.registry,
            registered: inventory_name.is_some(),
            inventory_name,
            discovery,
            response_ms,
            last_seen,
        };

        if json {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&result)?
            )));
        }

        let vendor = result.vendor.as_deref().unwrap_or("-");
        let registry = result.ieee_registry.as_deref().unwrap_or("-");
        let inventory = result.inventory_name.as_deref().unwrap_or("-");
        let hostname = result.hostname.as_deref().unwrap_or("-");
        let mac = result.mac.as_deref().unwrap_or("-");
        let ip = result.ip.as_deref().unwrap_or("-");
        let discovery = result.discovery.as_deref().unwrap_or("-");
        let response = result
            .response_ms
            .map(|value| format!("{value} ms"))
            .unwrap_or_else(|| "-".to_owned());
        let last_seen = result.last_seen.as_deref().unwrap_or("-");

        let mut out = format!(
            "target:       {}\nip:           {ip}\nhostname:     {hostname}\nmac:          {mac}\nmac_scope:    {}\nvendor:       {vendor}\nieee_registry:{registry:>5}\ninventory:    {inventory}\nregistered:   {}\ndiscovery:    {discovery}\nresponse:     {response}\nlast_seen:    {last_seen}\n",
            result.target,
            result.mac_scope,
            if result.registered { "yes" } else { "no" },
        );

        if result.mac_scope == "local/private" {
            out.push_str(
                "note:         MAC administrada localmente; puede ser privada/aleatoria. SST no atribuye fabricante por OUI.\n",
            );
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn monitor(&self, args: &[String]) -> Result<CommandOutput> {
        let (network, mut publish_message) = self.monitor_options(args)?;
        let only_unknown = args.iter().any(|arg| arg == "--unknown");
        let mut terminal = self.terminal.alternate_screen()?;

        let mut lan_notes: HashMap<Ipv4Addr, LanNote> = HashMap::new();
        let (lan_bus, mut lan_error) = match LanNoteBus::bind(network) {
            Ok(bus) => (Some(bus), None),
            Err(error) => (
                None,
                Some(format!("mensajes SST no disponibles: {error}")),
            ),
        };

        let mut stable_online: HashMap<String, String> = HashMap::new();
        let mut misses: HashMap<String, u8> = HashMap::new();
        let mut last_rows: HashMap<String, ScanRow> = HashMap::new();
        let mut history: HashMap<String, PresenceRecord> = self
            .presence
            .all()?
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect();
        let mut events = Vec::new();
        let mut message_input: Option<String> = None;

        loop {
            if let Some(bus) = lan_bus.as_ref() {
                if let Some(message) = publish_message.as_deref()
                    && let Err(error) = bus.publish(message)
                {
                    lan_error = Some(format!("no se pudo publicar mensaje SST: {error}"));
                }
                bus.receive_into(&mut lan_notes);
            }

            let mut rows = self.scan_rows(network)?;

            if let Some(bus) = lan_bus.as_ref() {
                bus.receive_into(&mut lan_notes);
            }
            if only_unknown {
                rows.retain(|row| !row.known);
            }

            let now = Utc::now().to_rfc3339();
            let mut observed_ids = HashSet::new();

            for row in rows {
                let mut id = scan_identity(&row);
                let ip_text = row.ip.to_string();

                if !row.mac.starts_with("??") {
                    let old_ip_id = format!("ip:{}", row.ip);
                    if stable_online.contains_key(&old_ip_id) && !stable_online.contains_key(&id) {
                        if let Some(old_ip) = stable_online.remove(&old_ip_id) {
                            stable_online.insert(id.clone(), old_ip);
                        }
                        if let Some(count) = misses.remove(&old_ip_id) {
                            misses.insert(id.clone(), count);
                        }
                        if let Some(previous_row) = last_rows.remove(&old_ip_id) {
                            last_rows.insert(id.clone(), previous_row);
                        }
                        if let Some(mut record) = history.remove(&old_ip_id) {
                            record.id = id.clone();
                            history.insert(id.clone(), record);
                        }
                    }
                } else if let Some(existing_id) = stable_online
                    .iter()
                    .find(|(candidate, existing_ip)| {
                        !candidate.starts_with("ip:") && existing_ip.as_str() == ip_text.as_str()
                    })
                    .map(|(candidate, _)| candidate.to_string())
                {
                    id = existing_id;
                }

                observed_ids.insert(id.clone());
                misses.remove(&id);

                let mut effective_row = row.clone();
                if effective_row.mac.starts_with("??") {
                    if let Some(previous) = last_rows.get(&id) {
                        effective_row.mac = previous.mac.clone();
                        effective_row.mac_scope = previous.mac_scope.clone();
                        effective_row.vendor = previous.vendor.clone();
                        if effective_row.inventory_name.is_none() {
                            effective_row.inventory_name = previous.inventory_name.clone();
                            effective_row.known = previous.known;
                        }
                    }
                }

                match stable_online.get(&id) {
                    None => push_event(
                        &mut events,
                        format!(
                            "{} + {} {} {} {}",
                            timestamp_short(),
                            effective_row.ip,
                            effective_row.mac,
                            display_name(&effective_row),
                            effective_row
                                .vendor
                                .as_deref()
                                .unwrap_or(effective_row.mac_scope.as_str())
                        ),
                    ),
                    Some(previous_ip) if previous_ip != &ip_text => push_event(
                        &mut events,
                        format!(
                            "{} ~ {} cambió IP {} -> {}",
                            timestamp_short(),
                            effective_row.mac,
                            previous_ip,
                            effective_row.ip
                        ),
                    ),
                    _ => {}
                }

                stable_online.insert(id.clone(), ip_text);
                last_rows.insert(id.clone(), effective_row.clone());

                history
                    .entry(id.clone())
                    .and_modify(|record| {
                        record.mac = effective_row.mac.clone();
                        record.ip = effective_row.ip.to_string();
                        record.hostname = effective_row.hostname.clone();
                        record.mac_scope = effective_row.mac_scope.clone();
                        record.vendor = effective_row.vendor.clone();
                        record.discovery = effective_row.discovery.clone();
                        record.last_seen = now.clone();
                    })
                    .or_insert_with(|| PresenceRecord {
                        id,
                        mac: effective_row.mac.clone(),
                        ip: effective_row.ip.to_string(),
                        hostname: effective_row.hostname.clone(),
                        mac_scope: effective_row.mac_scope.clone(),
                        vendor: effective_row.vendor.clone(),
                        discovery: effective_row.discovery.clone(),
                        first_seen: now.clone(),
                        last_seen: now.clone(),
                    });
            }

            let stable_ids = stable_online.keys().cloned().collect::<Vec<_>>();
            for id in stable_ids {
                if observed_ids.contains(&id) {
                    continue;
                }

                let count = misses.entry(id.clone()).or_insert(0);
                *count = count.saturating_add(1);

                if *count >= OFFLINE_MISSES {
                    let ip = stable_online
                        .remove(&id)
                        .unwrap_or_else(|| "-".to_owned());
                    let mac = last_rows
                        .get(&id)
                        .map(|row| row.mac.as_str())
                        .unwrap_or(id.as_str());
                    push_event(
                        &mut events,
                        format!("{} - {} {}", timestamp_short(), ip, mac),
                    );
                    misses.remove(&id);
                    last_rows.remove(&id);
                }
            }

            let records = history.values().cloned().collect::<Vec<_>>();
            self.presence.replace_all(&records)?;

            let mut display_rows = last_rows
                .iter()
                .filter(|(id, _)| stable_online.contains_key(id.as_str()))
                .map(|(id, row)| (id.clone(), row.clone()))
                .collect::<Vec<_>>();
            display_rows.sort_by_key(|(_, row)| row.ip);

            let mut screen = render_monitor_screen(
                network,
                only_unknown,
                publish_message.as_deref(),
                message_input.as_deref(),
                lan_error.as_deref(),
                &display_rows,
                &misses,
                &lan_notes,
                &events,
            );

            terminal.clear()?;
            terminal.write(&screen)?;
            terminal.flush()?;

            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(3) {
                let Some(key) = terminal.poll_key(Duration::from_millis(150))? else {
                    continue;
                };

                let mut redraw = false;

                if let Some(input) = message_input.as_mut() {
                    match key {
                        TerminalKey::Enter => {
                            let message = sanitize_message(input);
                            if !message.is_empty() {
                                publish_message = Some(message.clone());
                                if let Some(bus) = lan_bus.as_ref() {
                                    if let Err(error) = bus.publish(&message) {
                                        lan_error = Some(format!(
                                            "no se pudo publicar mensaje SST: {error}"
                                        ));
                                    } else {
                                        bus.receive_into(&mut lan_notes);
                                    }
                                }
                            }
                            message_input = None;
                            redraw = true;
                        }
                        TerminalKey::Escape => {
                            message_input = None;
                            redraw = true;
                        }
                        TerminalKey::Backspace => {
                            input.pop();
                            redraw = true;
                        }
                        TerminalKey::Space => {
                            if input.chars().count() < 120 {
                                input.push(' ');
                                redraw = true;
                            }
                        }
                        TerminalKey::Char(ch) if !ch.is_control() => {
                            if input.chars().count() < 120 {
                                input.push(ch);
                                redraw = true;
                            }
                        }
                        _ => {}
                    }
                } else {
                    match key {
                        TerminalKey::Enter => {
                            message_input = Some(String::new());
                            redraw = true;
                        }
                        TerminalKey::Char('q') | TerminalKey::Escape => {
                            return Ok(CommandOutput::ok(""));
                        }
                        _ => {}
                    }
                }

                if redraw {
                    screen = render_monitor_screen(
                        network,
                        only_unknown,
                        publish_message.as_deref(),
                        message_input.as_deref(),
                        lan_error.as_deref(),
                        &display_rows,
                        &misses,
                        &lan_notes,
                        &events,
                    );
                    terminal.clear()?;
                    terminal.write(&screen)?;
                    terminal.flush()?;
                }
            }
        }
    }

    pub fn presence(&self, args: &[String]) -> Result<CommandOutput> {
        let mut records = self.presence.all()?;
        for record in &mut records {
            enrich_presence_record(record);
        }
        records.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

        if args.iter().any(|arg| arg == "--json") {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&records)?
            )));
        }

        if args.iter().any(|arg| arg == "--csv") {
            let mut out = String::from(
                "mac,ip,hostname,mac_scope,vendor,discovery,first_seen,last_seen\n",
            );
            for record in records {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{},{}\n",
                    csv::escape(&record.mac),
                    csv::escape(&record.ip),
                    csv::escape(&record.hostname),
                    csv::escape(&record.mac_scope),
                    csv::escape(record.vendor.as_deref().unwrap_or("")),
                    csv::escape(&record.discovery),
                    csv::escape(&record.first_seen),
                    csv::escape(&record.last_seen)
                ));
            }
            return Ok(CommandOutput::ok(out));
        }

        let mut out = String::from(
            "MAC                 IP               HOSTNAME                     TYPE           VENDOR                   VIA       LAST SEEN\n",
        );

        for record in records {
            out.push_str(&format!(
                "{:<19} {:<16} {:<28} {:<14} {:<24} {:<9} {}\n",
                record.mac,
                record.ip,
                shorten(&record.hostname, 28),
                shorten(&record.mac_scope, 14),
                shorten(record.vendor.as_deref().unwrap_or("-"), 24),
                shorten(if record.discovery.is_empty() { "-" } else { &record.discovery }, 9),
                record.last_seen
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn scan_rows(&self, network: Ipv4Net) -> Result<Vec<ScanRow>> {
        let timeout = Duration::from_millis(300);
        let hosts = network.hosts().collect::<Vec<Ipv4Addr>>();
        let results = Arc::new(Mutex::new(Vec::new()));

        for chunk in hosts.chunks(48) {
            let mut workers = Vec::new();

            for ip in chunk {
                let ip = *ip;
                let results = Arc::clone(&results);
                let diagnostics = Arc::clone(&self.diagnostics);

                workers.push(thread::spawn(move || {
                    let Some(evidence) = diagnostics.probe_host(ip, timeout) else {
                        return;
                    };

                    let hostname = lookup_addr(&IpAddr::V4(ip)).ok();
                    if let Ok(mut results) = results.lock() {
                        results.push((ip, evidence, hostname));
                    }
                }));
            }

            for worker in workers {
                let _ = worker.join();
            }
        }

        let arp = self.diagnostics.arp_map().unwrap_or_default();
        let devices = self.devices.all().unwrap_or_default();
        let known_names = devices
            .iter()
            .map(|device| (device.mac.to_ascii_uppercase(), device.name.clone()))
            .collect::<HashMap<_, _>>();
        let known_macs = devices
            .into_iter()
            .map(|device| device.mac.to_ascii_uppercase())
            .collect::<HashSet<_>>();

        let mut discovered = results
            .lock()
            .map(|rows| rows.clone())
            .unwrap_or_default();
        discovered.sort_by_key(|row| row.0);

        Ok(discovered
            .into_iter()
            .map(|(ip, evidence, hostname)| {
                let mac = evidence
                    .mac
                    .or_else(|| arp.get(&ip).cloned())
                    .unwrap_or_else(|| "??:??:??:??:??:??".to_owned());
                let normalized_mac = mac.to_ascii_uppercase();
                let identity = identify_mac(&mac);

                ScanRow {
                    ip,
                    inventory_name: known_names.get(&normalized_mac).cloned(),
                    known: known_macs.contains(&normalized_mac),
                    mac,
                    hostname: hostname.unwrap_or_else(|| "-".to_owned()),
                    latency_ms: evidence.latency_ms,
                    discovery: evidence.method,
                    mac_scope: identity.scope,
                    vendor: identity.vendor,
                }
            })
            .collect())
    }

    fn monitor_options(&self, args: &[String]) -> Result<(Ipv4Net, Option<String>)> {
        let mut network = None;
        let mut message = None;
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "--unknown" => index += 1,
                "-m" | "--message" | "--say" => {
                    if index + 1 >= args.len() {
                        anyhow::bail!(
                            "net monitor: {} requiere texto; usa comillas si contiene espacios",
                            args[index]
                        );
                    }
                    let text = sanitize_message(&args[index + 1]);
                    if text.is_empty() {
                        anyhow::bail!("net monitor: el mensaje no puede estar vacío");
                    }
                    message = Some(text);
                    index += 2;
                }
                value if value.starts_with('-') => {
                    anyhow::bail!("net monitor: opción desconocida: {value}");
                }
                value => {
                    if network.is_some() {
                        anyhow::bail!("net monitor: argumento inesperado: {value}");
                    }
                    network = Some(value.parse::<Ipv4Net>()?);
                    index += 1;
                }
            }
        }

        let network = match network {
            Some(network) => network,
            None => self.diagnostics.default_ipv4_network()?,
        };

        if network.prefix_len() < 20 {
            anyhow::bail!("net monitor: el escaneo está limitado a /20 o redes más pequeñas");
        }

        Ok((network, message))
    }

    fn network_from_args(&self, args: &[String], command: &str) -> Result<Ipv4Net> {
        let network = if let Some(value) = args.iter().find(|arg| !arg.starts_with('-')) {
            value.parse::<Ipv4Net>()?
        } else {
            self.diagnostics.default_ipv4_network()?
        };

        if network.prefix_len() < 20 {
            anyhow::bail!("{command}: el escaneo está limitado a /20 o redes más pequeñas");
        }

        Ok(network)
    }
}

fn display_name(row: &ScanRow) -> &str {
    if row.hostname != "-" && !row.hostname.trim().is_empty() {
        &row.hostname
    } else if let Some(name) = row.inventory_name.as_deref() {
        name
    } else {
        "-"
    }
}

fn render_scan_rows(rows: &[ScanRow]) -> String {
    let mut out = String::from(
        "IP               NOMBRE                       MAC                 TYPE           VENDOR                   VIA       RESP     INVENTORY\n",
    );

    for row in rows {
        let inventory = row
            .inventory_name
            .as_deref()
            .unwrap_or(if row.known { "known" } else { "unknown" });
        let response = row
            .latency_ms
            .map(|value| format!("{value} ms"))
            .unwrap_or_else(|| "-".to_owned());

        out.push_str(&format!(
            "{:<16} {:<28} {:<19} {:<14} {:<24} {:<9} {:<8} {}\n",
            row.ip,
            shorten(display_name(row), 28),
            row.mac,
            shorten(&row.mac_scope, 14),
            shorten(row.vendor.as_deref().unwrap_or("-"), 24),
            shorten(&row.discovery, 9),
            response,
            inventory
        ));
    }

    out
}

fn render_monitor_screen(
    network: Ipv4Net,
    only_unknown: bool,
    publish_message: Option<&str>,
    message_input: Option<&str>,
    lan_error: Option<&str>,
    display_rows: &[(String, ScanRow)],
    misses: &HashMap<String, u8>,
    lan_notes: &HashMap<Ipv4Addr, LanNote>,
    events: &[String],
) -> String {
    let mut screen = format!(
        "SST net monitor {}{}   [Enter] mensaje · [q/Esc] salir · offline tras {} fallos · UDP/{}\n",
        network,
        if only_unknown { " --unknown" } else { "" },
        OFFLINE_MISSES,
        LAN_NOTE_PORT,
    );

    if let Some(message) = publish_message {
        screen.push_str(&format!("Publicando: {}\n", shorten(message, 120)));
    }
    if let Some(error) = lan_error {
        screen.push_str(&format!("LAN message warning: {error}\n"));
    }

    screen.push('\n');
    screen.push_str(&render_monitor_rows(display_rows, misses));
    screen.push_str("\nMensajes SST:\n");
    screen.push_str(&render_lan_notes(lan_notes, display_rows));
    screen.push_str("\nEventos recientes:\n");

    for event_line in events {
        screen.push_str(event_line);
        screen.push('\n');
    }

    if let Some(input) = message_input {
        screen.push_str("\nMensaje> ");
        screen.push_str(input);
        screen.push('_');
        screen.push_str("\n[Enter] enviar · [Esc] cancelar\n");
    }

    screen
}

fn render_lan_notes(
    notes: &HashMap<Ipv4Addr, LanNote>,
    rows: &[(String, ScanRow)],
) -> String {
    if notes.is_empty() {
        return String::from("(ninguno recibido)\n");
    }

    let names = rows
        .iter()
        .map(|(_, row)| (row.ip, display_name(row).to_owned()))
        .collect::<HashMap<_, _>>();

    let mut ordered = notes.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(ip, _)| **ip);

    let mut out = String::new();
    for (ip, note) in ordered {
        let name = names.get(ip).map(String::as_str).unwrap_or("-");
        out.push_str(&format!(
            "{:<16} {:<28} {:<3}s  {}\n",
            ip,
            shorten(name, 28),
            note.last_seen.elapsed().as_secs(),
            note.message
        ));
    }
    out
}

fn render_monitor_rows(
    rows: &[(String, ScanRow)],
    misses: &HashMap<String, u8>,
) -> String {
    let mut out = String::from(
        "IP               NOMBRE                       MAC                 TYPE           VENDOR                   VIA       RESP     STATE       INVENTORY\n",
    );

    for (id, row) in rows {
        let inventory = row
            .inventory_name
            .as_deref()
            .unwrap_or(if row.known { "known" } else { "unknown" });
        let response = row
            .latency_ms
            .map(|value| format!("{value} ms"))
            .unwrap_or_else(|| "-".to_owned());
        let state = match misses.get(id).copied().unwrap_or(0) {
            0 => "online".to_owned(),
            count => format!("miss {count}/{OFFLINE_MISSES}"),
        };

        out.push_str(&format!(
            "{:<16} {:<28} {:<19} {:<14} {:<24} {:<9} {:<8} {:<11} {}\n",
            row.ip,
            shorten(display_name(row), 28),
            row.mac,
            shorten(&row.mac_scope, 14),
            shorten(row.vendor.as_deref().unwrap_or("-"), 24),
            shorten(&row.discovery, 9),
            response,
            state,
            inventory
        ));
    }

    out
}

fn enrich_presence_record(record: &mut PresenceRecord) {
    if record.mac.starts_with("??") {
        if record.mac_scope.is_empty() {
            record.mac_scope = "unknown".to_owned();
        }
        return;
    }

    let identity = identify_mac(&record.mac);
    if record.mac_scope.is_empty() {
        record.mac_scope = identity.scope;
    }
    if record.vendor.is_none() {
        record.vendor = identity.vendor;
    }
}

fn scan_identity(row: &ScanRow) -> String {
    if row.mac.starts_with("??") {
        format!("ip:{}", row.ip)
    } else {
        row.mac.to_ascii_uppercase()
    }
}

fn shorten(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width <= 3 {
        return value.chars().take(width).collect();
    }
    let mut out = value.chars().take(width - 3).collect::<String>();
    out.push_str("...");
    out
}

fn push_event(events: &mut Vec<String>, event: String) {
    events.push(event);
    if events.len() > 12 {
        events.remove(0);
    }
}

fn timestamp_short() -> String {
    Local::now().format("[%H:%M:%S]").to_string()
}
