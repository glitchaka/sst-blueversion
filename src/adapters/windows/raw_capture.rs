//! Optional native IPv4 capture of traffic to/from a local adapter.
//! SIO_RCVALL needs Administrator, explicit IPv4 binding and NIC support.
//! No external binaries, drivers, decryption or implicit payload collection.
use std::{ffi::c_void, net::Ipv4Addr, time::{Duration, Instant}};
use anyhow::{bail, Result};

#[link(name="ws2_32")]
unsafe extern "system" {
    fn WSAStartup(version: u16, data: *mut c_void) -> i32;
    fn WSACleanup() -> i32;
    fn WSAGetLastError() -> i32;
    fn socket(af: i32, kind: i32, protocol: i32) -> usize;
    fn bind(sock: usize, address: *const SockaddrIpv4, length: i32) -> i32;
    fn setsockopt(sock: usize, level: i32, option: i32, value: *const i8, length: i32) -> i32;
    fn WSAIoctl(sock: usize, code: u32, input: *mut c_void, input_len: u32,
        output: *mut c_void, output_len: u32, returned: *mut u32,
        overlapped: *mut c_void, completion: *mut c_void) -> i32;
    fn recv(sock: usize, buf: *mut i8, length: i32, flags: i32) -> i32;
    fn closesocket(sock: usize) -> i32;
}
#[repr(C)]
struct SockaddrIpv4 {
    family: u16,
    port: u16,
    address: u32,
    zero: [u8; 8],
}
struct WinSock;
impl WinSock {
    fn start() -> Result<Self> {
        let mut data = [0u64; 64]; // aligned storage, larger than WSADATA
        let status = unsafe { WSAStartup(0x0202, data.as_mut_ptr().cast()) };
        if status != 0 { bail!("WSAStartup: error {status}"); }
        Ok(Self)
    }
}
impl Drop for WinSock {
    fn drop(&mut self) { unsafe { WSACleanup(); } }
}
struct RawSocket { id: usize, receiving: bool }
impl Drop for RawSocket {
    fn drop(&mut self) {
        if self.receiving {
            let mut mode = 0u32;
            let mut returned = 0u32;
            unsafe {
                let _ = WSAIoctl(self.id, 0x9800_0001, (&mut mode as *mut u32).cast(),
                    4, std::ptr::null_mut(), 0, &mut returned,
                    std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
        unsafe { closesocket(self.id); }
    }
}
fn last_wsa() -> i32 { unsafe { WSAGetLastError() } }

#[derive(Debug, PartialEq, Eq)]
struct IpPacket<'a> {
    source: Ipv4Addr,
    destination: Ipv4Addr,
    protocol: &'static str,
    src_port: Option<u16>,
    dst_port: Option<u16>,
    bytes: usize,
    flags: &'static str,
    payload: &'a [u8],
}
fn parse_ipv4(raw: &[u8]) -> Option<IpPacket<'_>> {
    if raw.len() < 20 || raw[0] >> 4 != 4 { return None; }
    let ihl = usize::from(raw[0] & 0x0f) * 4;
    if ihl < 20 || ihl > raw.len() { return None; }
    let total = usize::from(u16::from_be_bytes([raw[2],raw[3]])).min(raw.len());
    if total < ihl { return None; }
    let source = Ipv4Addr::new(raw[12],raw[13],raw[14],raw[15]);
    let destination = Ipv4Addr::new(raw[16],raw[17],raw[18],raw[19]);
    let fragment = u16::from_be_bytes([raw[6],raw[7]]) & 0x1fff != 0;
    let proto = raw[9];
    let protocol = match proto { 6 => "TCP",17 => "UDP",1 => "ICMP",2 => "IGMP",_ => "IP" };
    let mut src_port = None;
    let mut dst_port = None;
    let mut flags = "-";
    let mut payload_pos = ihl;
    if !fragment && matches!(proto, 6 | 17) && total >= ihl+8 {
        src_port = Some(u16::from_be_bytes([raw[ihl],raw[ihl+1]]));
        dst_port = Some(u16::from_be_bytes([raw[ihl+2],raw[ihl+3]]));
        if proto == 17 { payload_pos += 8; }
        else if total >= ihl + 20 {
            let tcp_header = usize::from(raw[ihl+12] >> 4) * 4;
            if tcp_header < 20 || ihl + tcp_header > total { return None; }
            payload_pos += tcp_header;
            let value = raw[ihl+13];
            flags = if value & 0x04 != 0 { "RST" }
                else if value & 0x02 != 0 && value & 0x10 != 0 { "SYN+ACK" }
                else if value & 0x02 != 0 { "SYN" }
                else if value & 0x01 != 0 { "FIN" }
                else if value & 0x10 != 0 { "ACK" }
                else { "-" };
        } else { return None; }
    } else if proto == 1 && !fragment && total >= ihl + 8 {
        payload_pos += 8;
    } else if fragment {
        flags = "FRAGMENTO";
    }
    Some(IpPacket {
        source,destination,protocol,src_port,dst_port,bytes:total,
        flags,payload: &raw[payload_pos.min(total)..total],
    })
}
#[derive(Debug, Clone)]
pub struct FlowOwner {
    pub protocol: String,
    pub local_port: u16,
    pub remote_ip: Option<Ipv4Addr>,
    pub remote_port: Option<u16>,
    pub pid: u32,
    pub process: String,
    pub services: Vec<String>,
}

fn flow_owner<'a>(packet: &IpPacket<'_>, local: Ipv4Addr, owners: &'a [FlowOwner]) -> Option<&'a FlowOwner> {
    let (local_port, remote_ip, remote_port) = if packet.source == local {
        (packet.src_port?, packet.destination, packet.dst_port)
    } else {
        (packet.dst_port?, packet.source, packet.src_port)
    };
    let mut matches = owners.iter().filter(|owner| {
        owner.local_port == local_port && owner.protocol == packet.protocol
            && owner.remote_ip.is_none_or(|ip| ip == remote_ip)
            && owner.remote_port.is_none_or(|port| Some(port) == remote_port)
    });
    let first = matches.next()?;
    if matches.any(|other| other.pid != first.pid) { return None; }
    Some(first)
}

fn port_label(port: Option<u16>) -> String {
    port.map(|value| format!(":{value}")).unwrap_or_default()
}
fn describe_protocol(port: Option<u16>, protocol: &str) -> &'static str {
    match (port, protocol) {
        (Some(53), _) => "DNS probable",
        (Some(80), "TCP") => "HTTP probable",
        (Some(443), "TCP") => "HTTPS/TLS probable",
        (Some(443), "UDP") => "QUIC/HTTPS posible",
        (Some(445), "TCP") => "SMB probable",
        (Some(3389), _) => "RDP posible",
        (Some(123), "UDP") => "NTP probable",
        (Some(161), "UDP") => "SNMP probable",
        (Some(25), "TCP") | (Some(587), "TCP") => "SMTP probable",
        (Some(9100), "TCP") => "Impresión RAW probable",
        _ => "desconocido (puerto no acredita servicio)",
    }
}
pub fn capture(local: Ipv4Addr, seconds: u32, limit: u32, payload_bytes: usize, owners: &[FlowOwner]) -> Result<String> {
    if local.is_unspecified() || local.is_broadcast() || local.is_multicast() {
        bail!("se requiere la IPv4 asignada a la NIC local (no 0.0.0.0)");
    }
    let _winsock = WinSock::start()?;
    // AF_INET=2 SOCK_RAW=3 IPPROTO_IP=0
    let raw = unsafe { socket(2, 3, 0) };
    if raw == usize::MAX {
        bail!("socket IPv4 RAW: error WinSock {}. Se requieren privilegios de administrador.", last_wsa());
    }
    let mut socket = RawSocket { id:raw,receiving:false };
    let addr = SockaddrIpv4 {
        family: 2, port: 0, address: u32::from_ne_bytes(local.octets()), zero: [0;8]
    };
    if unsafe { bind(socket.id, &addr, size_of::<SockaddrIpv4>() as i32) } != 0 {
        bail!("bind de interfaz local {local}: error WinSock {}", last_wsa());
    }
    // SOL_SOCKET=0xffff, SO_RCVTIMEO=0x1006
    let timeout = 500u32;
    if unsafe { setsockopt(socket.id, 0xffff, 0x1006,
        (&timeout as *const u32).cast(), size_of::<u32>() as i32) } != 0
    {
        bail!("SO_RCVTIMEO: error WinSock {}", last_wsa());
    }
    let mut mode = 1u32; // RCVALL_ON
    let mut returned = 0u32;
    if unsafe {
        WSAIoctl(socket.id, 0x9800_0001, (&mut mode as *mut u32).cast(),
            4, std::ptr::null_mut(), 0, &mut returned,
            std::ptr::null_mut(), std::ptr::null_mut())
    } != 0 {
        bail!("SIO_RCVALL: WinSock {}. Requiere administrador y NIC compatible.", last_wsa());
    }
    socket.receiving = true;
    let mut out = format!(
        "CAPTURA IPv4 NATIVA — Windows SIO_RCVALL\n\
         Interfaz local: {local}\nDuración máxima: {seconds} s\n\
         Límite: {limit} paquetes\nContenido máximo mostrado: {payload_bytes} bytes por paquete\n\
         Sólo tráfico hacia/desde la IPv4 elegida; no descifra TLS.\n\n"
    );
    let start = Instant::now();
    let mut shown = 0u32;
    let mut tx = 0u64;
    let mut rx = 0u64;
    let mut buffer = vec![0u8; 65536];
    while start.elapsed() < Duration::from_secs(seconds as u64) && shown < limit {
        let received = unsafe { recv(socket.id, buffer.as_mut_ptr().cast(), buffer.len() as i32, 0) };
        if received < 0 {
            let error = last_wsa();
            if matches!(error, 10060 | 10035 | 10004) { continue; }
            return Err(anyhow::anyhow!("recv IPv4: error WinSock {error}"));
        }
        let Some(packet) = parse_ipv4(&buffer[..received as usize]) else { continue };
        let direction = if packet.source == local { "TX" } else if packet.destination == local { "RX" } else {
            continue;
        };
        if direction == "TX" { tx += packet.bytes as u64; }
        else { rx += packet.bytes as u64; }
        shown += 1;
        let remote_port = if direction == "TX" { packet.dst_port } else { packet.src_port };
        out.push_str(&format!(
            "{shown:>4}. {direction} {}{} -> {}{} | {} | {} B | {} | {}\n",
            packet.source, port_label(packet.src_port), packet.destination,
            port_label(packet.dst_port), packet.protocol, packet.bytes,
            packet.flags, describe_protocol(remote_port,packet.protocol),
        ));
        if let Some(owner) = flow_owner(&packet, local, owners) {
            out.push_str(&format!("      PID probable {} | Proceso: {} | Servicios asociados: {}\n",
                owner.pid, owner.process, if owner.services.is_empty() { "-".to_owned() } else { owner.services.join(", ") }));
            if owner.services.len() > 1 {
                out.push_str("      PID compartido: no se puede atribuir este paquete a un servicio individual.\n");
            }
        } else {
            out.push_str("      PID: NO IDENTIFICADO (sin coincidencia de socket inequívoca).\n");
        }
        if payload_bytes > 0 && !packet.payload.is_empty() {
            let preview = packet.payload.iter().take(payload_bytes)
                .map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ");
            out.push_str(&format!("      Payload hexadecimal (limitado): {preview}\n"));
            let ascii = packet.payload.iter().take(payload_bytes)
                .map(|b| if b.is_ascii_graphic() || *b == b' ' { char::from(*b) } else { '.' })
                .collect::<String>();
            out.push_str(&format!("      Vista ASCII imprimible: {ascii}\n"));
        }
    }
    out.push_str(&format!(
        "\nRESUMEN\n  Paquetes observados: {shown}\n  Bytes TX/RX: {tx}/{rx}\n\
         Nota: tramas Ethernet, VLAN, IPv6 y payload cifrado TLS no se descifran.\n\
         PID/proceso son coincidencias de la tabla de sockets consultada al inicio de la captura, no evidencia de atribución por paquete.\n\
         Los puertos permiten inferir servicios probables, pero no los identifican con certeza.\n"
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_ipv4_udp_endpoints_and_payload_without_out_of_bounds() {
        let mut packet = [0u8; 32];
        packet[0]=0x45;
        packet[2]=0; packet[3]=32;
        packet[9]=17;
        packet[12..16].copy_from_slice(&[10,0,0,10]);
        packet[16..20].copy_from_slice(&[10,0,0,11]);
        packet[20..24].copy_from_slice(&[0x14,0xe9,0x00,0x35]);
        packet[28..32].copy_from_slice(&[1,2,3,4]);
        let parsed = parse_ipv4(&packet).unwrap();
        assert_eq!(parsed.source, Ipv4Addr::new(10,0,0,10));
        assert_eq!(parsed.destination, Ipv4Addr::new(10,0,0,11));
        assert_eq!(parsed.src_port,Some(5353));
        assert_eq!(parsed.dst_port,Some(53));
        assert_eq!(parsed.payload,&[1,2,3,4]);
    }
    #[test]
    fn rejects_truncated_or_invalid_ipv4_headers() {
        assert!(parse_ipv4(&[0u8; 5]).is_none());
        assert!(parse_ipv4(&[0xf0u8; 30]).is_none());
    }
}
