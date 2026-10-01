use std::{
    collections::{HashMap, HashSet},
    fs,
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex, mpsc},
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
    lan_note::{
        LAN_NOTE_PORT, LanNote, LanNoteBus, ReceivedNote, derive_room_key,
        normalize_room_name, sanitize_message,
    },
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

#[derive(Debug, Clone)]
struct ChatHistoryEntry {
    received_at: String,
    source: Ipv4Addr,
    message: String,
    room: Option<String>,
    encrypted: bool,
    private: bool,
}

#[derive(Debug, Clone)]
struct ChatRoom {
    name: String,
    key: Option<[u8; 32]>,
}

impl ChatRoom {
    fn label(&self) -> String {
        if self.key.is_some() {
            format!("{} [cifrada]", self.name)
        } else {
            self.name.clone()
        }
    }
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
        let (mut history, mut monitor_warning, mut persistence_available) =
            match self.presence.all() {
                Ok(records) => (
                    records
                        .into_iter()
                        .map(|record| (record.id.clone(), record))
                        .collect::<HashMap<String, PresenceRecord>>(),
                    None,
                    true,
                ),
                Err(error) => (
                    HashMap::new(),
                    Some(format!("historial de presencia no disponible: {error}")),
                    false,
                ),
            };
        let mut events = Vec::new();
        let mut chat_history: Vec<ChatHistoryEntry> = Vec::new();
        let mut chat_status: Option<String> = None;
        let mut active_room: Option<ChatRoom> = None;
        let mut message_input: Option<String> = None;
        let mut display_rows: Vec<(String, ScanRow)> = Vec::new();
        let mut scan_status = String::from("iniciando");
        let mut scan_rx: Option<mpsc::Receiver<Result<Vec<ScanRow>>>> = None;
        let mut next_scan = Instant::now();
        let mut dirty = true;
        let mut next_publish = Instant::now();

        loop {
            if let Some(bus) = lan_bus.as_ref() {
                if Instant::now() >= next_publish {
                    if let Some(message) = publish_message.as_deref() {
                        let publish_result = if let Some(room) = active_room.as_ref() {
                            bus.publish_room(&room.name, room.key.as_ref(), message)
                        } else {
                            bus.publish(message)
                        };
                        match publish_result {
                            Ok(()) => {
                                lan_error = None;
                            }
                            Err(error) => {
                                lan_error =
                                    Some(format!("no se pudo publicar mensaje SST: {error}"));
                            }
                        }
                    }
                    next_publish = Instant::now() + Duration::from_secs(3);
                }
                record_chat_history(
                    &mut chat_history,
                    bus.receive_into(
                        &mut lan_notes,
                        active_room.as_ref().map(|room| room.name.as_str()),
                        active_room.as_ref().and_then(|room| room.key.as_ref()),
                    ),
                );
            }

            if scan_rx.is_none() && Instant::now() >= next_scan {
                scan_status = "escaneando".to_owned();
                dirty = true;
                scan_rx = Some(self.start_scan(network));
            }

            if let Some(bus) = lan_bus.as_ref() {
                record_chat_history(
                    &mut chat_history,
                    bus.receive_into(
                        &mut lan_notes,
                        active_room.as_ref().map(|room| room.name.as_str()),
                        active_room.as_ref().and_then(|room| room.key.as_ref()),
                    ),
                );
            }

            let scan_result = match scan_rx.as_ref().map(mpsc::Receiver::try_recv) {
                Some(Ok(result)) => Some(result),
                Some(Err(mpsc::TryRecvError::Disconnected)) => Some(Err(anyhow::anyhow!(
                    "el worker de descubrimiento terminó sin entregar resultado"
                ))),
                Some(Err(mpsc::TryRecvError::Empty)) | None => None,
            };

            if let Some(result) = scan_result {
                scan_rx = None;
                next_scan = Instant::now() + Duration::from_secs(3);

                match result {
                    Ok(mut rows) => {
                        scan_status = format!("ok · {} hosts", rows.len());
                        if persistence_available {
                            monitor_warning = None;
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
                        if persistence_available {
                            if let Err(error) = self.presence.replace_all(&records) {
                                monitor_warning =
                                    Some(format!("no se pudo guardar historial de presencia: {error}"));
                                persistence_available = false;
                            }
                        }

                        display_rows = last_rows
                            .iter()
                            .filter(|(id, _)| stable_online.contains_key(id.as_str()))
                            .map(|(id, row)| (id.clone(), row.clone()))
                            .collect::<Vec<_>>();
                        display_rows.sort_by_key(|(_, row)| row.ip);
                        dirty = true;
                    }
                    Err(error) => {
                        scan_status = "error".to_owned();
                        monitor_warning = Some(format!("falló el barrido de red: {error}"));
                        dirty = true;
                    }
                }
            }

            if let Some(key) = terminal.poll_key(Duration::from_millis(100))? {
                let redraw = handle_monitor_key(
                    key,
                    &mut message_input,
                    &mut publish_message,
                    lan_bus.as_ref(),
                    &mut lan_notes,
                    &mut chat_history,
                    &mut chat_status,
                    &mut active_room,
                    &mut lan_error,
                    &mut next_publish,
                )?;
                if redraw == MonitorInput::Exit {
                    return Ok(CommandOutput::ok(""));
                }
                if redraw == MonitorInput::Redraw {
                    dirty = true;
                }
            }

            if dirty {
                let terminal_width = terminal
                    .size()
                    .map(|(cols, _)| cols as usize)
                    .unwrap_or(120);
                let screen = render_monitor_screen(
                    network,
                    only_unknown,
                    publish_message.as_deref(),
                    message_input.as_deref(),
                    lan_error.as_deref(),
                    monitor_warning.as_deref(),
                    &scan_status,
                    &display_rows,
                    &misses,
                    &lan_notes,
                    &events,
                    chat_status.as_deref(),
                    active_room.as_ref(),
                    terminal_width,
                );

                terminal.clear()?;
                terminal.write(&screen)?;
                terminal.flush()?;
                dirty = false;
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
        scan_rows_with(
            Arc::clone(&self.diagnostics),
            Arc::clone(&self.devices),
            network,
        )
    }

    fn start_scan(&self, network: Ipv4Net) -> mpsc::Receiver<Result<Vec<ScanRow>>> {
        let (sender, receiver) = mpsc::channel();
        let diagnostics = Arc::clone(&self.diagnostics);
        let devices = Arc::clone(&self.devices);

        thread::spawn(move || {
            let result = scan_rows_with(diagnostics, devices, network);
            let _ = sender.send(result);
        });

        receiver
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


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MonitorInput {
    None,
    Redraw,
    Exit,
}

fn handle_monitor_key(
    key: TerminalKey,
    message_input: &mut Option<String>,
    publish_message: &mut Option<String>,
    lan_bus: Option<&LanNoteBus>,
    lan_notes: &mut HashMap<Ipv4Addr, LanNote>,
    chat_history: &mut Vec<ChatHistoryEntry>,
    chat_status: &mut Option<String>,
    active_room: &mut Option<ChatRoom>,
    lan_error: &mut Option<String>,
    next_publish: &mut Instant,
) -> Result<MonitorInput> {
    if let Some(input) = message_input.as_mut() {
        match key {
            TerminalKey::Enter => {
                let raw = input.trim().to_owned();
                if raw.starts_with("::") {
                    handle_chat_command(
                        &raw,
                        lan_bus,
                        lan_notes,
                        chat_history,
                        chat_status,
                        active_room,
                        publish_message,
                        lan_error,
                    )?;
                } else {
                    let message = sanitize_message(&raw);
                    if !message.is_empty() {
                        *publish_message = Some(message.clone());
                        if let Some(bus) = lan_bus {
                            let publish_result = if let Some(room) = active_room.as_ref() {
                                bus.publish_room(&room.name, room.key.as_ref(), &message)
                            } else {
                                bus.publish(&message)
                            };
                            if let Err(error) = publish_result {
                                *lan_error = Some(format!(
                                    "no se pudo publicar mensaje SST: {error}"
                                ));
                            } else {
                                *lan_error = None;
                                record_chat_history(
                                    chat_history,
                                    bus.receive_into(
                                        lan_notes,
                                        active_room.as_ref().map(|room| room.name.as_str()),
                                        active_room.as_ref().and_then(|room| room.key.as_ref()),
                                    ),
                                );
                            }
                        }
                        *next_publish = Instant::now() + Duration::from_secs(3);
                    }
                }
                *message_input = None;
                return Ok(MonitorInput::Redraw);
            }
            TerminalKey::Escape => {
                *message_input = None;
                return Ok(MonitorInput::Redraw);
            }
            TerminalKey::Backspace => {
                input.pop();
                return Ok(MonitorInput::Redraw);
            }
            TerminalKey::Space => {
                if input.chars().count() < 120 {
                    input.push(' ');
                    return Ok(MonitorInput::Redraw);
                }
            }
            TerminalKey::Char(ch) if !ch.is_control() => {
                if input.chars().count() < 120 {
                    input.push(ch);
                    return Ok(MonitorInput::Redraw);
                }
            }
            _ => {}
        }
        return Ok(MonitorInput::None);
    }

    match key {
        TerminalKey::Enter => {
            *message_input = Some(String::new());
            Ok(MonitorInput::Redraw)
        }
        TerminalKey::Char('q') | TerminalKey::Escape => Ok(MonitorInput::Exit),
        _ => Ok(MonitorInput::None),
    }
}

fn record_chat_history(
    history: &mut Vec<ChatHistoryEntry>,
    received: Vec<ReceivedNote>,
) {
    for note in received {
        history.push(ChatHistoryEntry {
            received_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            source: note.source,
            message: note.message,
            room: note.room,
            encrypted: note.encrypted,
            private: note.private,
        });
    }
}

fn export_chat_lore(history: &[ChatHistoryEntry]) -> Result<String> {
    let filename = format!(
        "sst-net-monitor-lore-{}.txt",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let path = std::env::current_dir()?.join(filename);
    let mut text = String::from("SST net monitor — historial de mensajes recibidos\n\n");
    if history.is_empty() {
        text.push_str("(sin mensajes recibidos durante esta sesión)\n");
    } else {
        for entry in history {
            let channel = if entry.private {
                "pm".to_owned()
            } else if let Some(room) = entry.room.as_deref() {
                format!("room:{room}")
            } else {
                "general".to_owned()
            };
            let encrypted = if entry.encrypted { " encrypted" } else { "" };
            text.push_str(&format!(
                "[{}] [{}{}] {}  {}\n",
                entry.received_at,
                channel,
                encrypted,
                entry.source,
                entry.message
            ));
        }
    }
    fs::write(&path, text)?;
    Ok(path.display().to_string())
}

fn handle_chat_command(
    command: &str,
    lan_bus: Option<&LanNoteBus>,
    lan_notes: &mut HashMap<Ipv4Addr, LanNote>,
    chat_history: &mut Vec<ChatHistoryEntry>,
    chat_status: &mut Option<String>,
    active_room: &mut Option<ChatRoom>,
    publish_message: &mut Option<String>,
    lan_error: &mut Option<String>,
) -> Result<()> {
    let name = command
        .split_whitespace()
        .next()
        .unwrap_or(command)
        .to_ascii_lowercase();

    let args = command.split_whitespace().collect::<Vec<_>>();

    match name.as_str() {
        "::help" => {
            *chat_status = Some(
                "::whoisalive · ::room NOMBRE [CLAVE] · ::join NOMBRE [CLAVE] · ::leave · ::where · ::rooms · ::pm IP MENSAJE · ::fingerprint · ::lore · ::peers · ::clear · ::help".to_owned()
            );
        }
        "::whoisalive" => {
            if let Some(bus) = lan_bus {
                match bus.who_is_alive() {
                    Ok(()) => {
                        *lan_error = None;
                        *chat_status = Some(
                            "sondeo enviado; cada SST escuchando responderá «atrapado»".to_owned()
                        );
                    }
                    Err(error) => {
                        *lan_error = Some(format!("no se pudo enviar ::whoisalive: {error}"));
                    }
                }
            } else {
                *chat_status = Some("chat LAN no disponible".to_owned());
            }
        }
        "::room" | "::join" => {
            let Some(room_raw) = args.get(1) else {
                *chat_status = Some("uso: ::join NOMBRE [CLAVE]".to_owned());
                return Ok(());
            };
            let room = normalize_room_name(room_raw)?;
            let key = if args.len() >= 3 {
                Some(derive_room_key(&room, &args[2..].join(" "))?)
            } else {
                None
            };
            let encrypted = key.is_some();
            *active_room = Some(ChatRoom {
                name: room.clone(),
                key,
            });
            lan_notes.clear();
            *publish_message = None;
            *chat_status = Some(if encrypted {
                format!("sala {room} activa [cifrada]")
            } else {
                format!("sala {room} activa")
            });
        }
        "::leave" => {
            *active_room = None;
            lan_notes.clear();
            *publish_message = None;
            *chat_status = Some("sala general activa".to_owned());
        }
        "::where" => {
            *chat_status = Some(
                active_room
                    .as_ref()
                    .map(|room| format!("sala actual: {}", room.label()))
                    .unwrap_or_else(|| "sala actual: general".to_owned())
            );
        }
        "::rooms" => {
            if let Some(bus) = lan_bus {
                let rooms = bus.seen_rooms();
                *chat_status = Some(if rooms.is_empty() {
                    "no se han visto salas durante esta sesión".to_owned()
                } else {
                    format!("salas vistas: {}", rooms.join(", "))
                });
            } else {
                *chat_status = Some("chat LAN no disponible".to_owned());
            }
        }
        "::pm" => {
            if args.len() < 3 {
                *chat_status = Some("uso: ::pm IP MENSAJE".to_owned());
                return Ok(());
            }
            let Ok(ip) = args[1].parse::<Ipv4Addr>() else {
                *chat_status = Some(format!("IP inválida: {}", args[1]));
                return Ok(());
            };
            let message = args[2..].join(" ");
            if let Some(bus) = lan_bus {
                match bus.send_private(ip, &message) {
                    Ok(()) => {
                        *lan_error = None;
                        *chat_status = Some(format!("PM cifrado enviado a {ip}"));
                    }
                    Err(error) => {
                        *chat_status = Some(error.to_string());
                    }
                }
            } else {
                *chat_status = Some("chat LAN no disponible".to_owned());
            }
        }
        "::fingerprint" => {
            *chat_status = Some(if let Some(bus) = lan_bus {
                format!("fingerprint: {}", bus.fingerprint())
            } else {
                "chat LAN no disponible".to_owned()
            });
        }
        "::lore" => {
            let path = export_chat_lore(chat_history)?;
            *chat_status = Some(format!("historial guardado en {path}"));
        }
        "::peers" => {
            let peers = lan_bus
                .map(LanNoteBus::peer_addresses)
                .unwrap_or_default();
            *chat_status = Some(if peers.is_empty() {
                "ninguna terminal SST con identidad criptográfica conocida; usa ::whoisalive".to_owned()
            } else {
                format!(
                    "terminales SST: {}",
                    peers.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
                )
            });
        }
        "::clear" => {
            lan_notes.clear();
            *chat_status = Some(
                "mensajes visibles limpiados; ::lore conserva el historial de la sesión".to_owned()
            );
        }
        _ => {
            *chat_status = Some(format!(
                "comando desconocido: {name}; usa ::help"
            ));
        }
    }

    Ok(())
}

fn scan_rows_with(
    diagnostics: Arc<NetworkDiagnosticsService>,
    devices: Arc<dyn DeviceRepository>,
    network: Ipv4Net,
) -> Result<Vec<ScanRow>> {
    let timeout = Duration::from_millis(300);
    let hosts = network.hosts().collect::<Vec<Ipv4Addr>>();
    let results = Arc::new(Mutex::new(Vec::new()));

    for chunk in hosts.chunks(48) {
        let mut workers = Vec::new();

        for ip in chunk {
            let ip = *ip;
            let results = Arc::clone(&results);
            let diagnostics = Arc::clone(&diagnostics);

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

    let arp = diagnostics.arp_map().unwrap_or_default();
    let inventory = devices.all().unwrap_or_default();
    let known_names = inventory
        .iter()
        .map(|device| (device.mac.to_ascii_uppercase(), device.name.clone()))
        .collect::<HashMap<_, _>>();
    let known_macs = inventory
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
    monitor_warning: Option<&str>,
    scan_status: &str,
    display_rows: &[(String, ScanRow)],
    misses: &HashMap<String, u8>,
    lan_notes: &HashMap<Ipv4Addr, LanNote>,
    events: &[String],
    chat_status: Option<&str>,
    active_room: Option<&ChatRoom>,
    terminal_width: usize,
) -> String {
    let mut screen = format!(
        "SST net monitor {}{}   [Enter] mensaje · [q/Esc] salir · scan: {} · offline tras {} fallos · UDP/{}\n",
        network,
        if only_unknown { " --unknown" } else { "" },
        scan_status,
        OFFLINE_MISSES,
        LAN_NOTE_PORT,
    );

    let room_label = active_room
        .map(ChatRoom::label)
        .unwrap_or_else(|| "general".to_owned());
    screen.push_str(&format!("Sala: {room_label}\n"));

    if let Some(message) = publish_message {
        screen.push_str(&format!("Publicando: {}\n", shorten(message, 120)));
    }
    if let Some(error) = lan_error {
        screen.push_str(&format!("LAN message warning: {error}\n"));
    }
    if let Some(warning) = monitor_warning {
        screen.push_str(&format!("MONITOR WARNING: {warning}\n"));
    }

    screen.push('\n');
    screen.push_str(&render_monitor_rows(display_rows, misses));
    screen.push_str("\nMensajes SST:\n");
    if let Some(status) = chat_status {
        screen.push_str(&format!("[chat] {status}\n"));
    }
    screen.push_str(&render_lan_notes(
        lan_notes,
        display_rows,
        terminal_width,
    ));
    screen.push_str("\nEventos recientes:\n");

    for event_line in events {
        screen.push_str(event_line);
        screen.push('\n');
    }

    if let Some(input) = message_input {
        screen.push_str("\n");
        screen.push_str(&render_wrapped_prompt("Mensaje> ", input, terminal_width));
        screen.push_str("_\n[Enter] enviar · [Esc] cancelar\n");
    }

    screen
}

fn render_lan_notes(
    notes: &HashMap<Ipv4Addr, LanNote>,
    rows: &[(String, ScanRow)],
    terminal_width: usize,
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
        let channel = if note.private {
            "[PM] ".to_owned()
        } else if note.encrypted {
            "[ENC] ".to_owned()
        } else {
            String::new()
        };
        let prefix = format!(
            "{:<16} {:<28} {:<3}s  {}",
            ip,
            shorten(name, 28),
            note.last_seen.elapsed().as_secs(),
            channel,
        );
        out.push_str(&wrap_with_prefix(
            &prefix,
            &note.message,
            terminal_width,
        ));
    }
    out
}

fn wrap_with_prefix(prefix: &str, text: &str, terminal_width: usize) -> String {
    let width = terminal_width.max(20);
    let prefix_len = prefix.chars().count();
    let available = width.saturating_sub(prefix_len).max(1);
    let continuation = " ".repeat(prefix_len);
    let chars = text.chars().collect::<Vec<_>>();

    if chars.is_empty() {
        return format!("{prefix}\n");
    }

    let mut out = String::new();
    for (index, chunk) in chars.chunks(available).enumerate() {
        if index == 0 {
            out.push_str(prefix);
        } else {
            out.push_str(&continuation);
        }
        out.extend(chunk);
        out.push('\n');
    }
    out
}

fn render_wrapped_prompt(prefix: &str, text: &str, terminal_width: usize) -> String {
    let width = terminal_width.max(20);
    let prefix_len = prefix.chars().count();
    let available = width.saturating_sub(prefix_len).max(1);
    let continuation = " ".repeat(prefix_len);
    let chars = text.chars().collect::<Vec<_>>();

    if chars.is_empty() {
        return prefix.to_owned();
    }

    let mut out = String::new();
    for (index, chunk) in chars.chunks(available).enumerate() {
        if index == 0 {
            out.push_str(prefix);
        } else {
            out.push('\n');
            out.push_str(&continuation);
        }
        out.extend(chunk);
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
