use std::{
    collections::HashMap,
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};

pub const LAN_NOTE_PORT: u16 = 43837;
pub const WHO_IS_ALIVE: &str = "::whoisalive";
pub const ALIVE_REPLY: &str = "atrapado";
const MAGIC: &str = "SST-NET-MONITOR/1";
const MAX_MESSAGE_CHARS: usize = 120;
const MESSAGE_TTL: Duration = Duration::from_secs(20);

#[derive(Debug, Serialize, Deserialize)]
struct WireMessage {
    magic: String,
    message: String,
    #[serde(default)]
    sender: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LanNote {
    pub message: String,
    pub last_seen: Instant,
}

pub struct LanNoteBus {
    socket: UdpSocket,
    broadcast: SocketAddrV4,
    instance_id: String,
}

impl LanNoteBus {
    pub fn bind(network: Ipv4Net) -> Result<Self> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, LAN_NOTE_PORT))
            .with_context(|| format!("no se pudo abrir UDP/{LAN_NOTE_PORT} para mensajes SST"))?;
        socket
            .set_broadcast(true)
            .context("no se pudo habilitar UDP broadcast")?;
        socket
            .set_nonblocking(true)
            .context("no se pudo configurar el canal SST como no bloqueante")?;

        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok(Self {
            socket,
            broadcast: SocketAddrV4::new(network.broadcast(), LAN_NOTE_PORT),
            instance_id: format!("{}-{started}", std::process::id()),
        })
    }

    fn send_message_to(&self, message: &str, destination: SocketAddrV4) -> Result<()> {
        let message = sanitize_message(message);
        if message.is_empty() {
            return Ok(());
        }

        let payload = serde_json::to_vec(&WireMessage {
            magic: MAGIC.to_owned(),
            message,
            sender: Some(self.instance_id.clone()),
        })?;
        self.socket
            .send_to(&payload, destination)
            .with_context(|| format!("no se pudo publicar mensaje SST a {destination}"))?;
        Ok(())
    }

    pub fn publish(&self, message: &str) -> Result<()> {
        self.send_message_to(message, self.broadcast)
    }

    pub fn who_is_alive(&self) -> Result<()> {
        self.send_message_to(WHO_IS_ALIVE, self.broadcast)
    }

    pub fn receive_into(
        &self,
        messages: &mut HashMap<Ipv4Addr, LanNote>,
    ) -> Vec<(Ipv4Addr, String)> {
        let mut buffer = [0u8; 1024];
        let mut received = Vec::new();

        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((size, source)) => {
                    let SocketAddr::V4(source) = source else {
                        continue;
                    };
                    let Ok(frame) = serde_json::from_slice::<WireMessage>(&buffer[..size]) else {
                        continue;
                    };
                    if frame.magic != MAGIC {
                        continue;
                    }
                    if frame.sender.as_deref() == Some(self.instance_id.as_str()) {
                        continue;
                    }

                    let message = sanitize_message(&frame.message);
                    if message.is_empty() {
                        continue;
                    }

                    if message.eq_ignore_ascii_case(WHO_IS_ALIVE) {
                        let _ = self.send_message_to(ALIVE_REPLY, source);
                        continue;
                    }

                    let source_ip = *source.ip();
                    let changed = messages
                        .get(&source_ip)
                        .is_none_or(|note| note.message != message);

                    messages.insert(
                        source_ip,
                        LanNote {
                            message: message.clone(),
                            last_seen: Instant::now(),
                        },
                    );

                    if changed {
                        received.push((source_ip, message));
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        let now = Instant::now();
        messages.retain(|_, note| now.duration_since(note.last_seen) <= MESSAGE_TTL);
        received
    }
}

pub fn sanitize_message(message: &str) -> String {
    let mut out = String::new();
    let mut previous_space = false;

    for ch in message.chars() {
        if out.chars().count() >= MAX_MESSAGE_CHARS {
            break;
        }

        let ch = if ch.is_control() { ' ' } else { ch };
        if ch.is_whitespace() {
            if !previous_space && !out.is_empty() {
                out.push(' ');
            }
            previous_space = true;
        } else {
            out.push(ch);
            previous_space = false;
        }
    }

    out.trim().to_owned()
}
