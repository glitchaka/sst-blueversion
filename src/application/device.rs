use std::{
    collections::HashSet,
    net::Ipv4Addr,
    sync::Arc,
};

use anyhow::Result;

use crate::{
    application::network::identity::identify_mac,
    core::{
        CommandOutput,
        models::device::Device,
        ports::{DeviceRepository, NetworkProbe, PresenceRepository},
    },
    support::csv,
};

pub struct DeviceService {
    repository: Arc<dyn DeviceRepository>,
    presence: Arc<dyn PresenceRepository>,
    network: Arc<dyn NetworkProbe>,
}

impl DeviceService {
    pub fn new(
        repository: Arc<dyn DeviceRepository>,
        presence: Arc<dyn PresenceRepository>,
        network: Arc<dyn NetworkProbe>,
    ) -> Self {
        Self {
            repository,
            presence,
            network,
        }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        if args.first().is_some_and(|a| matches!(a.as_str(), "help" | "--help" | "-h")) {
            return Ok(CommandOutput::ok(
                "device — inventario local por MAC\n\
                 uso:\n\
                   device list [--json|--csv]\n\
                   device show MAC|NOMBRE\n\
                   device add MAC|IP NOMBRE [--note TEXTO]\n\
                   device remove MAC\n\
                   device unknown [--json|--csv]\n\
                   device path\n\
                 Al registrar por IP, SST resuelve primero la MAC mediante descubrimiento de vecino/ARP.\n",
            ));
        }

        match args.first().map(String::as_str).unwrap_or("list") {
            "list" => self.list(args.get(1..).unwrap_or_default()),
            "show" => self.show(args.get(1..).unwrap_or_default()),
            "add" => self.add(args.get(1..).unwrap_or_default()),
            "remove" => self.remove(args.get(1..).unwrap_or_default()),
            "unknown" => self.unknown(args.get(1..).unwrap_or_default()),
            "path" => Ok(CommandOutput::ok(format!("{}\n", self.repository.path().display()))),
            other => Ok(CommandOutput::error(
                format!("device: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    fn list(&self, args: &[String]) -> Result<CommandOutput> {
        let devices = self.repository.all()?;

        if args.iter().any(|arg| arg == "--json") {
            let rows = devices
                .iter()
                .map(|device| {
                    let identity = identify_mac(&device.mac);
                    serde_json::json!({
                        "mac": device.mac.clone(),
                        "name": device.name.clone(),
                        "notes": device.notes.clone(),
                        "mac_scope": identity.scope,
                        "vendor": identity.vendor,
                        "ieee_registry": identity.registry,
                    })
                })
                .collect::<Vec<_>>();
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&rows)?
            )));
        }

        if args.iter().any(|arg| arg == "--csv") {
            let mut out = String::from("mac,name,mac_scope,vendor,ieee_registry,notes\n");
            for device in devices {
                let identity = identify_mac(&device.mac);
                out.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    csv::escape(&device.mac),
                    csv::escape(&device.name),
                    csv::escape(&identity.scope),
                    csv::escape(identity.vendor.as_deref().unwrap_or("")),
                    csv::escape(identity.registry.as_deref().unwrap_or("")),
                    csv::escape(device.notes.as_deref().unwrap_or(""))
                ));
            }
            return Ok(CommandOutput::ok(out));
        }

        let mut out = String::from(
            "MAC                 NAME                         TYPE           VENDOR                   NOTES\n",
        );
        for device in devices {
            let identity = identify_mac(&device.mac);
            out.push_str(&format!(
                "{:<19} {:<28} {:<14} {:<24} {}\n",
                device.mac,
                shorten(&device.name, 28),
                shorten(&identity.scope, 14),
                shorten(identity.vendor.as_deref().unwrap_or("-"), 24),
                device.notes.unwrap_or_default()
            ));
        }
        Ok(CommandOutput::ok(out))
    }

    fn show(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(query) = args.first() else {
            return Ok(CommandOutput::error("device show: uso: device show MAC|NOMBRE", 2));
        };

        let devices = self.repository.all()?;
        let normalized = normalize_mac(query).ok();
        let found = devices.iter().find(|device| {
            normalized
                .as_ref()
                .is_some_and(|mac| device.mac.eq_ignore_ascii_case(mac))
                || device.name.eq_ignore_ascii_case(query)
        });

        let Some(device) = found else {
            return Ok(CommandOutput::error(format!("device: no se encontró {query}"), 1));
        };

        let identity = identify_mac(&device.mac);
        let last = self
            .presence
            .all()?
            .into_iter()
            .filter(|record| record.mac.eq_ignore_ascii_case(&device.mac))
            .max_by(|a, b| a.last_seen.cmp(&b.last_seen));

        Ok(CommandOutput::ok(format!(
            "name: {}\nmac: {}\nmac_scope: {}\nvendor: {}\nieee_registry: {}\nlast_ip: {}\nlast_hostname: {}\nlast_seen: {}\nnotes: {}\n",
            device.name,
            device.mac,
            identity.scope,
            identity.vendor.as_deref().unwrap_or("-"),
            identity.registry.as_deref().unwrap_or("-"),
            last.as_ref().map(|record| record.ip.as_str()).unwrap_or("-"),
            last.as_ref().map(|record| record.hostname.as_str()).unwrap_or("-"),
            last.as_ref().map(|record| record.last_seen.as_str()).unwrap_or("-"),
            device.notes.as_deref().unwrap_or("-")
        )))
    }

    fn add(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error(
                "device add: uso: device add MAC|IP NOMBRE [--note TEXTO]",
                2,
            ));
        };

        if args.len() < 2 {
            return Ok(CommandOutput::error("device add: falta nombre", 2));
        }

        let mac = self.resolve_registration_mac(target)?;
        let mut name_parts = Vec::new();
        let mut note = None;
        let mut index = 1;

        while index < args.len() {
            if args[index] == "--note" {
                note = Some(args[index + 1..].join(" "));
                break;
            }
            name_parts.push(args[index].clone());
            index += 1;
        }

        let name = name_parts.join(" ");
        if name.trim().is_empty() {
            return Ok(CommandOutput::error("device add: falta nombre", 2));
        }

        let mut devices = self.repository.all()?;
        if let Some(existing) = devices
            .iter_mut()
            .find(|device| device.mac.eq_ignore_ascii_case(&mac))
        {
            existing.name = name.clone();
            if note.is_some() {
                existing.notes = note.clone();
            }
        } else {
            devices.push(Device {
                mac: mac.clone(),
                name: name.clone(),
                notes: note,
            });
        }

        devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.repository.replace_all(&devices)?;

        let identity = identify_mac(&mac);
        Ok(CommandOutput::ok(format!(
            "device saved: {mac} {name}\nmac_scope: {}\nvendor: {}\n",
            identity.scope,
            identity.vendor.as_deref().unwrap_or("-")
        )))
    }

    fn resolve_registration_mac(&self, target: &str) -> Result<String> {
        if let Ok(mac) = normalize_mac(target) {
            return Ok(mac);
        }

        let ip = target
            .parse::<Ipv4Addr>()
            .map_err(|_| anyhow::anyhow!("device add: {target} no es una MAC ni una IPv4 válida"))?;

        let mac = self
            .network
            .resolve_neighbor(ip)?
            .ok_or_else(|| anyhow::anyhow!(
                "device add: no se pudo resolver la MAC de {ip}; ejecuta 'net scan' o verifica que el equipo esté en el mismo segmento"
            ))?;

        normalize_mac(&mac)
    }

    fn unknown(&self, args: &[String]) -> Result<CommandOutput> {
        let known_macs = self
            .repository
            .all()?
            .into_iter()
            .map(|device| device.mac.to_ascii_uppercase())
            .collect::<HashSet<_>>();

        let mut records = self.presence.all()?;
        records.retain(|record| !known_macs.contains(&record.mac.to_ascii_uppercase()));
        records.sort_by(|a, b| {
            b.last_seen
                .cmp(&a.last_seen)
                .then_with(|| a.ip.cmp(&b.ip))
        });

        if args.iter().any(|arg| arg == "--json") {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&records)?
            )));
        }

        if args.iter().any(|arg| arg == "--csv") {
            let mut out = String::from(
                "ip,hostname,mac,mac_scope,vendor,discovery,first_seen,last_seen\n",
            );
            for record in records {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{},{}\n",
                    csv::escape(&record.ip),
                    csv::escape(&record.hostname),
                    csv::escape(&record.mac),
                    csv::escape(&record.mac_scope),
                    csv::escape(record.vendor.as_deref().unwrap_or("")),
                    csv::escape(&record.discovery),
                    csv::escape(&record.first_seen),
                    csv::escape(&record.last_seen),
                ));
            }
            return Ok(CommandOutput::ok(out));
        }

        let mut out = String::from(
            "IP               HOSTNAME                     MAC                 TYPE           VENDOR                   VIA       LAST SEEN\n",
        );
        for record in records {
            out.push_str(&format!(
                "{:<16} {:<28} {:<19} {:<14} {:<24} {:<9} {}\n",
                record.ip,
                shorten(&record.hostname, 28),
                record.mac,
                shorten(&record.mac_scope, 14),
                shorten(record.vendor.as_deref().unwrap_or("-"), 24),
                shorten(if record.discovery.is_empty() { "-" } else { &record.discovery }, 9),
                record.last_seen,
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    fn remove(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(mac) = args.first() else {
            return Ok(CommandOutput::error("device remove: uso: device remove MAC", 2));
        };

        let mac = normalize_mac(mac)?;
        let mut devices = self.repository.all()?;
        let before = devices.len();
        devices.retain(|device| !device.mac.eq_ignore_ascii_case(&mac));

        if before == devices.len() {
            return Ok(CommandOutput::error(format!("device: no existe {mac}"), 1));
        }

        self.repository.replace_all(&devices)?;
        Ok(CommandOutput::ok(format!("device removed: {mac}\n")))
    }
}

pub fn normalize_mac(value: &str) -> Result<String> {
    let normalized = value.replace('-', ":").to_ascii_uppercase();
    let parts = normalized.split(':').collect::<Vec<_>>();

    if parts.len() != 6
        || parts
            .iter()
            .any(|part| part.len() != 2 || u8::from_str_radix(part, 16).is_err())
    {
        anyhow::bail!("MAC inválida: {value}");
    }

    Ok(normalized)
}

pub fn parse_mac(value: &str) -> Result<[u8; 6]> {
    let normalized = normalize_mac(value)?;
    let parts = normalized.split(':').collect::<Vec<_>>();
    let mut mac = [0_u8; 6];

    for (index, part) in parts.iter().enumerate() {
        mac[index] = u8::from_str_radix(part, 16)?;
    }

    Ok(mac)
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
