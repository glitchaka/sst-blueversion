use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use argon2::Argon2;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ipnet::Ipv4Net;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

pub const LAN_NOTE_PORT: u16 = 43837;
pub const ALIVE_REPLY: &str = "atrapado";

const MAGIC_V1: &str = "SST-NET-MONITOR/1";
const MAGIC_V2: &str = "SST-NET-MONITOR/2";
const MAX_MESSAGE_CHARS: usize = 120;
const MAX_ROOM_CHARS: usize = 32;
const MESSAGE_TTL: Duration = Duration::from_secs(20);
const PM_CONTEXT: &[u8] = b"SST-NET-MONITOR-PM/2";
const ROOM_CONTEXT: &[u8] = b"SST-NET-MONITOR-ROOM/2";

fn default_kind() -> String {
    "message".to_owned()
}

#[derive(Debug, Serialize, Deserialize)]
struct WireMessage {
    magic: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    sender: Option<String>,
    #[serde(default)]
    room: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    ciphertext: Option<String>,
    #[serde(default)]
    public_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LanNote {
    pub message: String,
    pub last_seen: Instant,
    pub encrypted: bool,
    pub private: bool,
}

#[derive(Debug, Clone)]
pub struct ReceivedNote {
    pub source: Ipv4Addr,
    pub message: String,
    pub room: Option<String>,
    pub encrypted: bool,
    pub private: bool,
}

pub struct LanNoteBus {
    socket: UdpSocket,
    broadcast: SocketAddrV4,
    instance_id: String,
    secret_key: StaticSecret,
    public_key: PublicKey,
    peer_keys: Mutex<HashMap<Ipv4Addr, [u8; 32]>>,
    seen_rooms: Mutex<HashSet<String>>,
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
        let secret_key = StaticSecret::random_from_rng(OsRng);
        let public_key = PublicKey::from(&secret_key);

        Ok(Self {
            socket,
            broadcast: SocketAddrV4::new(network.broadcast(), LAN_NOTE_PORT),
            instance_id: format!("{}-{started}", std::process::id()),
            secret_key,
            public_key,
            peer_keys: Mutex::new(HashMap::new()),
            seen_rooms: Mutex::new(HashSet::new()),
        })
    }

    fn wire(
        &self,
        kind: &str,
        message: Option<String>,
        room: Option<String>,
        nonce: Option<String>,
        ciphertext: Option<String>,
    ) -> WireMessage {
        WireMessage {
            magic: MAGIC_V2.to_owned(),
            kind: kind.to_owned(),
            message,
            sender: Some(self.instance_id.clone()),
            room,
            nonce,
            ciphertext,
            public_key: Some(BASE64.encode(self.public_key.as_bytes())),
        }
    }

    fn send_wire(&self, frame: &WireMessage, destination: SocketAddrV4) -> Result<()> {
        let payload = serde_json::to_vec(frame)?;
        self.socket
            .send_to(&payload, destination)
            .with_context(|| format!("no se pudo publicar mensaje SST a {destination}"))?;
        Ok(())
    }

    fn send_plain_to(
        &self,
        kind: &str,
        message: &str,
        room: Option<&str>,
        destination: SocketAddrV4,
    ) -> Result<()> {
        let message = sanitize_message(message);
        if message.is_empty() {
            return Ok(());
        }
        let room = room.map(normalize_room_name).transpose()?;
        let frame = self.wire(kind, Some(message), room, None, None);
        self.send_wire(&frame, destination)
    }

    pub fn publish(&self, message: &str) -> Result<()> {
        self.send_plain_to("message", message, None, self.broadcast)
    }

    pub fn publish_room(
        &self,
        room: &str,
        key: Option<&[u8; 32]>,
        message: &str,
    ) -> Result<()> {
        let room = normalize_room_name(room)?;
        let message = sanitize_message(message);
        if message.is_empty() {
            return Ok(());
        }

        if let Some(key) = key {
            let (nonce, ciphertext) = encrypt_payload(key, room.as_bytes(), message.as_bytes())?;
            let frame = self.wire(
                "room",
                None,
                Some(room),
                Some(BASE64.encode(nonce)),
                Some(BASE64.encode(ciphertext)),
            );
            self.send_wire(&frame, self.broadcast)
        } else {
            self.send_plain_to("room", &message, Some(&room), self.broadcast)
        }
    }

    pub fn who_is_alive(&self) -> Result<()> {
        let frame = self.wire("probe", None, None, None, None);
        self.send_wire(&frame, self.broadcast)
    }

    pub fn send_private(&self, destination: Ipv4Addr, message: &str) -> Result<()> {
        let peer = self
            .peer_keys
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&destination)
            .copied()
            .ok_or_else(|| anyhow::anyhow!(
                "{destination}: clave pública desconocida; ejecuta ::whoisalive primero"
            ))?;

        let message = sanitize_message(message);
        if message.is_empty() {
            bail!("el mensaje privado no puede estar vacío");
        }

        let key = self.private_key(peer);
        let (nonce, ciphertext) = encrypt_payload(&key, PM_CONTEXT, message.as_bytes())?;
        let frame = self.wire(
            "pm",
            None,
            None,
            Some(BASE64.encode(nonce)),
            Some(BASE64.encode(ciphertext)),
        );
        self.send_wire(
            &frame,
            SocketAddrV4::new(destination, LAN_NOTE_PORT),
        )
    }

    pub fn peer_addresses(&self) -> Vec<Ipv4Addr> {
        let mut peers = self
            .peer_keys
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .copied()
            .collect::<Vec<_>>();
        peers.sort();
        peers
    }

    pub fn seen_rooms(&self) -> Vec<String> {
        let mut rooms = self
            .seen_rooms
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        rooms.sort();
        rooms
    }

    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.public_key.as_bytes());
        digest[..10]
            .chunks(2)
            .map(|chunk| chunk.iter().map(|byte| format!("{byte:02X}")).collect::<String>())
            .collect::<Vec<_>>()
            .join("-")
    }

    fn private_key(&self, peer: [u8; 32]) -> [u8; 32] {
        let peer_public = PublicKey::from(peer);
        let shared = self.secret_key.diffie_hellman(&peer_public);

        let own = self.public_key.to_bytes();
        let (first, second) = if own <= peer {
            (own, peer)
        } else {
            (peer, own)
        };

        let mut hasher = Sha256::new();
        hasher.update(PM_CONTEXT);
        hasher.update(shared.as_bytes());
        hasher.update(first);
        hasher.update(second);
        hasher.finalize().into()
    }

    fn remember_peer(&self, source: Ipv4Addr, encoded: Option<&str>) {
        let Some(encoded) = encoded else {
            return;
        };
        let Ok(bytes) = BASE64.decode(encoded) else {
            return;
        };
        let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            return;
        };
        self.peer_keys
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(source, key);
    }

    fn remember_room(&self, room: Option<&str>) {
        if let Some(room) = room
            && let Ok(room) = normalize_room_name(room)
        {
            self.seen_rooms
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(room);
        }
    }

    pub fn receive_into(
        &self,
        messages: &mut HashMap<Ipv4Addr, LanNote>,
        active_room: Option<&str>,
        room_key: Option<&[u8; 32]>,
    ) -> Vec<ReceivedNote> {
        let active_room = active_room.and_then(|room| normalize_room_name(room).ok());
        let mut buffer = [0u8; 2048];
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
                    if frame.magic != MAGIC_V1 && frame.magic != MAGIC_V2 {
                        continue;
                    }
                    if frame.sender.as_deref() == Some(self.instance_id.as_str()) {
                        continue;
                    }

                    let source_ip = *source.ip();
                    self.remember_peer(source_ip, frame.public_key.as_deref());
                    self.remember_room(frame.room.as_deref());

                    match frame.kind.as_str() {
                        "probe" => {
                            let reply = self.wire(
                                "alive",
                                Some(ALIVE_REPLY.to_owned()),
                                None,
                                None,
                                None,
                            );
                            let _ = self.send_wire(&reply, source);
                            continue;
                        }
                        "alive" => {
                            let note = ReceivedNote {
                                source: source_ip,
                                message: ALIVE_REPLY.to_owned(),
                                room: None,
                                encrypted: false,
                                private: false,
                            };
                            store_note(messages, &note);
                            received.push(note);
                        }
                        "pm" => {
                            let Some(peer) = self
                                .peer_keys
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .get(&source_ip)
                                .copied()
                            else {
                                continue;
                            };
                            let Some(nonce) = decode_nonce(frame.nonce.as_deref()) else {
                                continue;
                            };
                            let Some(ciphertext) = frame
                                .ciphertext
                                .as_deref()
                                .and_then(|value| BASE64.decode(value).ok())
                            else {
                                continue;
                            };
                            let key = self.private_key(peer);
                            let Ok(plaintext) = decrypt_payload(
                                &key,
                                PM_CONTEXT,
                                &nonce,
                                &ciphertext,
                            ) else {
                                continue;
                            };
                            let Ok(message) = String::from_utf8(plaintext) else {
                                continue;
                            };
                            let message = sanitize_message(&message);
                            if message.is_empty() {
                                continue;
                            }
                            let note = ReceivedNote {
                                source: source_ip,
                                message,
                                room: None,
                                encrypted: true,
                                private: true,
                            };
                            store_note(messages, &note);
                            received.push(note);
                        }
                        "room" => {
                            let Some(frame_room) = frame.room.as_deref() else {
                                continue;
                            };
                            let Ok(frame_room) = normalize_room_name(frame_room) else {
                                continue;
                            };
                            if active_room.as_deref() != Some(frame_room.as_str()) {
                                continue;
                            }

                            if room_key.is_some() && frame.ciphertext.is_none() {
                                continue;
                            }
                            if room_key.is_none() && frame.ciphertext.is_some() {
                                continue;
                            }

                            let (message, encrypted) = if let Some(ciphertext) = frame.ciphertext.as_deref() {
                                let Some(key) = room_key else {
                                    continue;
                                };
                                let Some(nonce) = decode_nonce(frame.nonce.as_deref()) else {
                                    continue;
                                };
                                let Ok(ciphertext) = BASE64.decode(ciphertext) else {
                                    continue;
                                };
                                let Ok(plaintext) = decrypt_payload(
                                    key,
                                    frame_room.as_bytes(),
                                    &nonce,
                                    &ciphertext,
                                ) else {
                                    continue;
                                };
                                let Ok(message) = String::from_utf8(plaintext) else {
                                    continue;
                                };
                                (sanitize_message(&message), true)
                            } else {
                                (
                                    frame.message
                                        .as_deref()
                                        .map(sanitize_message)
                                        .unwrap_or_default(),
                                    false,
                                )
                            };

                            if message.is_empty() {
                                continue;
                            }
                            let note = ReceivedNote {
                                source: source_ip,
                                message,
                                room: Some(frame_room),
                                encrypted,
                                private: false,
                            };
                            store_note(messages, &note);
                            received.push(note);
                        }
                        _ => {
                            if active_room.is_some() {
                                continue;
                            }
                            let message = frame
                                .message
                                .as_deref()
                                .map(sanitize_message)
                                .unwrap_or_default();
                            if message.is_empty() {
                                continue;
                            }
                            let note = ReceivedNote {
                                source: source_ip,
                                message,
                                room: None,
                                encrypted: false,
                                private: false,
                            };
                            store_note(messages, &note);
                            received.push(note);
                        }
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

fn store_note(messages: &mut HashMap<Ipv4Addr, LanNote>, note: &ReceivedNote) {
    messages.insert(
        note.source,
        LanNote {
            message: note.message.clone(),
            last_seen: Instant::now(),
            encrypted: note.encrypted,
            private: note.private,
        },
    );
}

fn decode_nonce(encoded: Option<&str>) -> Option<[u8; 24]> {
    let bytes = BASE64.decode(encoded?).ok()?;
    <[u8; 24]>::try_from(bytes.as_slice()).ok()
}

fn encrypt_payload(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<([u8; 24], Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload { msg: plaintext, aad },
        )
        .map_err(|_| anyhow::anyhow!("falló el cifrado del mensaje"))?;
    Ok((nonce, ciphertext))
}

fn decrypt_payload(
    key: &[u8; 32],
    aad: &[u8],
    nonce: &[u8; 24],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("mensaje cifrado inválido o clave incorrecta"))
}

pub fn derive_room_key(room: &str, password: &str) -> Result<[u8; 32]> {
    let room = normalize_room_name(room)?;
    if password.is_empty() {
        bail!("la clave de sala no puede estar vacía");
    }

    let mut salt_hasher = Sha256::new();
    salt_hasher.update(ROOM_CONTEXT);
    salt_hasher.update(room.as_bytes());
    let salt_digest = salt_hasher.finalize();

    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt_digest[..16], &mut key)
        .map_err(|error| anyhow::anyhow!("no se pudo derivar la clave de sala: {error}"))?;
    Ok(key)
}

pub fn normalize_room_name(room: &str) -> Result<String> {
    let room = room.trim().to_lowercase();
    if room.is_empty() {
        bail!("el nombre de sala no puede estar vacío");
    }
    if room.chars().count() > MAX_ROOM_CHARS {
        bail!("el nombre de sala admite como máximo {MAX_ROOM_CHARS} caracteres");
    }
    if !room
        .chars()
        .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        bail!("el nombre de sala sólo admite letras, números, '.', '_' y '-'");
    }
    Ok(room)
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
