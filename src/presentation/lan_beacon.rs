use std::{
    collections::HashMap,
    env,
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};

pub const SST_BEACON_PORT: u16 = 43838;

const MAGIC: &str = "SST-LAN-BEACON/1";
const HELLO_INTERVAL: Duration = Duration::from_millis(1500);
const PEER_TIMEOUT: Duration = Duration::from_secs(6);
const LOOP_INTERVAL: Duration = Duration::from_millis(80);

#[derive(Debug, Clone)]
pub struct BeaconPeer {
    pub id: String,
    pub host: String,
    pub address: Ipv4Addr,
    pub version: String,
}

#[derive(Debug, Clone)]
pub enum BeaconEvent {
    PeerJoined {
        peer: BeaconPeer,
        peer_count: usize,
    },
    PeerLeft {
        peer: BeaconPeer,
        peer_count: usize,
    },
    Status {
        enabled: bool,
        peer_count: usize,
    },
}

#[derive(Debug)]
enum BeaconCommand {
    SetEnabled(bool),
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
struct BeaconFrame {
    magic: String,
    kind: String,
    instance: String,
    host: String,
    version: String,
}

#[derive(Debug)]
struct PeerState {
    peer: BeaconPeer,
    last_seen: Instant,
}

pub struct SstBeaconHandle {
    command_tx: Sender<BeaconCommand>,
    event_rx: Receiver<BeaconEvent>,
    worker: Option<JoinHandle<()>>,
}

impl SstBeaconHandle {
    pub fn start() -> Result<Self> {
        let socket = shared_udp_socket()?;
        socket
            .set_broadcast(true)
            .context("no se pudo habilitar broadcast para la baliza SST")?;
        socket
            .set_nonblocking(true)
            .context("no se pudo configurar la baliza SST como no bloqueante")?;

        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let host = local_host_name();
        let instance = make_instance_id(&host);
        let version = env!("CARGO_PKG_VERSION").to_owned();

        let worker = thread::Builder::new()
            .name("sst-lan-beacon".to_owned())
            .spawn(move || {
                run_worker(
                    socket,
                    command_rx,
                    event_tx,
                    instance,
                    host,
                    version,
                );
            })
            .context("no se pudo iniciar el worker de presencia LAN de SST")?;

        Ok(Self {
            command_tx,
            event_rx,
            worker: Some(worker),
        })
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.command_tx.send(BeaconCommand::SetEnabled(enabled));
    }

    pub fn try_event(&self) -> Option<BeaconEvent> {
        match self.event_rx.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for SstBeaconHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(BeaconCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn shared_udp_socket() -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("no se pudo crear el socket UDP de la baliza SST")?;
    socket
        .set_reuse_address(true)
        .context("no se pudo habilitar SO_REUSEADDR para la baliza SST")?;
    socket
        .bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, SST_BEACON_PORT).into())
        .with_context(|| {
            format!(
                "no se pudo abrir UDP/{SST_BEACON_PORT} para la baliza LAN de SST"
            )
        })?;
    Ok(socket.into())
}

fn run_worker(
    socket: UdpSocket,
    command_rx: Receiver<BeaconCommand>,
    event_tx: Sender<BeaconEvent>,
    instance: String,
    host: String,
    version: String,
) {
    let broadcast = SocketAddrV4::new(Ipv4Addr::BROADCAST, SST_BEACON_PORT);
    let mut enabled = true;
    let mut peers: HashMap<String, PeerState> = HashMap::new();
    let mut next_hello = Instant::now();
    let mut buffer = [0u8; 1024];

    let _ = event_tx.send(BeaconEvent::Status {
        enabled: true,
        peer_count: 0,
    });

    loop {
        let mut shutdown = false;
        while let Ok(command) = command_rx.try_recv() {
            match command {
                BeaconCommand::SetEnabled(next) => {
                    if enabled == next {
                        continue;
                    }
                    if enabled && !next {
                        let _ = send_frame(
                            &socket,
                            broadcast,
                            "bye",
                            &instance,
                            &host,
                            &version,
                        );
                    }
                    enabled = next;
                    peers.clear();
                    next_hello = Instant::now();
                    let _ = event_tx.send(BeaconEvent::Status {
                        enabled,
                        peer_count: 0,
                    });
                }
                BeaconCommand::Shutdown => {
                    if enabled {
                        let _ = send_frame(
                            &socket,
                            broadcast,
                            "bye",
                            &instance,
                            &host,
                            &version,
                        );
                    }
                    shutdown = true;
                    break;
                }
            }
        }
        if shutdown {
            break;
        }

        let now = Instant::now();
        if enabled && now >= next_hello {
            let _ = send_frame(
                &socket,
                broadcast,
                "hello",
                &instance,
                &host,
                &version,
            );
            next_hello = now + HELLO_INTERVAL;
        }

        loop {
            match socket.recv_from(&mut buffer) {
                Ok((size, source)) => {
                    if !enabled {
                        continue;
                    }
                    let SocketAddr::V4(source) = source else {
                        continue;
                    };
                    let Ok(frame) = serde_json::from_slice::<BeaconFrame>(&buffer[..size]) else {
                        continue;
                    };
                    if frame.magic != MAGIC || frame.instance == instance {
                        continue;
                    }

                    let peer = BeaconPeer {
                        id: frame.instance.clone(),
                        host: sanitize_host(&frame.host),
                        address: *source.ip(),
                        version: frame.version,
                    };

                    match frame.kind.as_str() {
                        "hello" => {
                            let is_new = !peers.contains_key(&frame.instance);
                            peers.insert(
                                frame.instance.clone(),
                                PeerState {
                                    peer: peer.clone(),
                                    last_seen: now,
                                },
                            );

                            if is_new {
                                let count = peers.len();
                                let _ = event_tx.send(BeaconEvent::PeerJoined {
                                    peer,
                                    peer_count: count,
                                });

                                // Reply immediately so a newly opened SST sees us without
                                // waiting for the next periodic broadcast.
                                let _ = send_frame(
                                    &socket,
                                    source,
                                    "hello",
                                    &instance,
                                    &host,
                                    &version,
                                );
                            }
                        }
                        "bye" => {
                            if let Some(state) = peers.remove(&frame.instance) {
                                let count = peers.len();
                                let _ = event_tx.send(BeaconEvent::PeerLeft {
                                    peer: state.peer,
                                    peer_count: count,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        if enabled {
            let expired = peers
                .iter()
                .filter_map(|(id, state)| {
                    (now.duration_since(state.last_seen) > PEER_TIMEOUT).then(|| id.clone())
                })
                .collect::<Vec<_>>();

            for id in expired {
                if let Some(state) = peers.remove(&id) {
                    let count = peers.len();
                    let _ = event_tx.send(BeaconEvent::PeerLeft {
                        peer: state.peer,
                        peer_count: count,
                    });
                }
            }
        }

        thread::sleep(LOOP_INTERVAL);
    }
}

fn send_frame(
    socket: &UdpSocket,
    destination: SocketAddrV4,
    kind: &str,
    instance: &str,
    host: &str,
    version: &str,
) -> Result<()> {
    let frame = BeaconFrame {
        magic: MAGIC.to_owned(),
        kind: kind.to_owned(),
        instance: instance.to_owned(),
        host: host.to_owned(),
        version: version.to_owned(),
    };
    let payload = serde_json::to_vec(&frame)?;
    socket
        .send_to(&payload, destination)
        .with_context(|| format!("no se pudo enviar baliza SST a {destination}"))?;
    Ok(())
}

fn local_host_name() -> String {
    env::var("COMPUTERNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| sanitize_host(&value))
        .unwrap_or_else(|| "SST".to_owned())
}

fn sanitize_host(value: &str) -> String {
    let clean = value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(48)
        .collect::<String>();
    if clean.trim().is_empty() {
        "SST".to_owned()
    } else {
        clean
    }
}

fn make_instance_id(host: &str) -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{host}-{}-{started}", std::process::id())
}
