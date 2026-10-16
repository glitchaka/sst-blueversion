//! Read-only IP Helper APIs; no command-line tools or shell subprocesses.
use std::{collections::HashMap, net::{Ipv4Addr, Ipv6Addr}, time::Duration};
use anyhow::{bail, Result};
use crate::core::{models::network::{ConnectionRow, EchoReply}, ports::NetworkProbe};
use windows_sys::Win32::{Foundation::*, NetworkManagement::IpHelper::*, Networking::WinSock::*};

pub struct WindowsNetworkProbe;

fn check(status: u32) -> Result<()> {
    if status != 0 { return Err(std::io::Error::from_raw_os_error(status as i32).into()); }
    Ok(())
}
fn ipv4(address: u32) -> Ipv4Addr { Ipv4Addr::from(address.to_ne_bytes()) }
fn port(port: u32) -> u16 { u16::from_be(port as u16) }
fn state(value: u32) -> &'static str {
    match value { 1 => "CLOSED", 2 => "LISTENING", 3 => "SYN_SENT", 4 => "SYN_RECEIVED", 5 => "ESTABLISHED",
        6 => "FIN_WAIT_1", 7 => "FIN_WAIT_2", 8 => "CLOSE_WAIT", 9 => "CLOSING", 10 => "LAST_ACK", 11 => "TIME_WAIT", 12 => "DELETE_TCB", _ => "UNKNOWN" }
}
fn address(value: &SOCKADDR_INET) -> String {
    unsafe { if value.si_family == AF_INET { ipv4(value.Ipv4.sin_addr.S_un.S_addr).to_string() }
        else { Ipv6Addr::from(value.Ipv6.sin6_addr.u.Byte).to_string() } }
}
fn wide(value: &[u16]) -> String {
    let end = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}
fn admin_status(value: u32) -> &'static str {
    match value { 1 => "UP", 2 => "DOWN", 3 => "TESTING", _ => "UNKNOWN" }
}
fn oper_status(value: u32) -> &'static str {
    match value {
        1 => "UP",
        2 => "DOWN",
        3 => "TESTING",
        4 => "UNKNOWN",
        5 => "DORMANT",
        6 => "NOT_PRESENT",
        7 => "LOWER_LAYER_DOWN",
        _ => "UNKNOWN",
    }
}
fn media_status(value: u32) -> &'static str {
    match value { 1 => "UP", 2 => "DOWN", _ => "UNKNOWN" }
}
fn interface_type(value: u32) -> &'static str {
    match value {
        6 => "Ethernet",
        23 => "PPP",
        24 => "Loopback",
        71 => "Wi-Fi",
        131 => "Tunnel",
        _ => "Other",
    }
}
fn link_speed(bits_per_second: u64) -> String {
    if bits_per_second == 0 { return "-".to_owned(); }
    if bits_per_second >= 1_000_000_000 {
        let gbps = bits_per_second as f64 / 1_000_000_000.0;
        return if gbps.fract() == 0.0 { format!("{gbps:.0} Gbps") } else { format!("{gbps:.1} Gbps") };
    }
    if bits_per_second >= 1_000_000 {
        let mbps = bits_per_second as f64 / 1_000_000.0;
        return if mbps.fract() == 0.0 { format!("{mbps:.0} Mbps") } else { format!("{mbps:.1} Mbps") };
    }
    format!("{} Kbps", bits_per_second / 1_000)
}

// Both OWNER_PID row layouts contain only 32-bit fields and byte arrays.
fn table<T: Copy>(tcp: bool, family: u16) -> Result<Vec<T>> {
    let mut bytes = 0u32;
    let call = |buffer, size: &mut u32| unsafe {
        if tcp { GetExtendedTcpTable(buffer, size, 1, family as u32, TCP_TABLE_OWNER_PID_ALL, 0) }
        else { GetExtendedUdpTable(buffer, size, 1, family as u32, UDP_TABLE_OWNER_PID, 0) }
    };
    let status = call(std::ptr::null_mut(), &mut bytes);
    if status != ERROR_INSUFFICIENT_BUFFER && status != 0 { check(status)?; }
    for _ in 0..5 {
        let mut buffer = vec![0u32; (bytes as usize).div_ceil(4).max(1)];
        let status = call(buffer.as_mut_ptr().cast(), &mut bytes);
        if status == ERROR_INSUFFICIENT_BUFFER { continue; }
        check(status)?;
        let count = buffer[0] as usize;
        let required = count.checked_mul(size_of::<T>()).and_then(|n| n.checked_add(4));
        if required.is_none_or(|n| n > buffer.len() * 4) { bail!("Tabla de conexiones incompleta"); }
        let start = unsafe { buffer.as_ptr().add(1).cast::<T>() };
        return Ok((0..count).map(|i| unsafe { start.add(i).read_unaligned() }).collect());
    }
    bail!("La tabla de conexiones cambió durante la consulta")
}

impl NetworkProbe for WindowsNetworkProbe {
    fn interfaces(&self) -> Result<String> {
        let networks = sysinfo::Networks::new_with_refreshed_list();
        let ip_by_name = networks.iter().map(|(name, data)| {
            (
                name.to_lowercase(),
                data.ip_networks().iter().map(ToString::to_string).collect::<Vec<_>>(),
            )
        }).collect::<HashMap<_, _>>();

        unsafe {
            let mut table = std::ptr::null_mut();
            check(GetIfTable2(&mut table))?;
            if table.is_null() { return Ok(String::new()); }

            let rows = std::slice::from_raw_parts(
                std::ptr::addr_of!((*table).Table).cast::<MIB_IF_ROW2>(),
                (*table).NumEntries as usize,
            );
            let mut output = String::new();

            for row in rows {
                let alias = wide(&row.Alias);
                let description = wide(&row.Description);
                let physical_len = (row.PhysicalAddressLength as usize).min(row.PhysicalAddress.len());
                let mac = if physical_len == 0 {
                    "-".to_owned()
                } else {
                    row.PhysicalAddress[..physical_len]
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(":")
                };
                let media = media_status(row.MediaConnectState as u32);
                let ips = ip_by_name.get(&alias.to_lowercase());

                output.push_str(&format!("{}\n", if alias.is_empty() { &description } else { &alias }));
                output.push_str(&format!("  Tipo: {}\n", interface_type(row.Type)));
                if !description.is_empty() && description != alias {
                    output.push_str(&format!("  Adaptador: {description}\n"));
                }
                output.push_str(&format!("  Admin: {}\n", admin_status(row.AdminStatus as u32)));
                output.push_str(&format!("  Enlace: {media}\n"));
                output.push_str(&format!("  Operativo: {}\n", oper_status(row.OperStatus as u32)));
                output.push_str(&format!("  Velocidad: {}\n", if media == "UP" { link_speed(row.ReceiveLinkSpeed) } else { "-".to_owned() }));
                output.push_str(&format!("  MAC: {mac}\n"));
                match ips {
                    Some(addresses) if !addresses.is_empty() => {
                        for address in addresses { output.push_str(&format!("  IP: {address}\n")); }
                    }
                    _ => output.push_str("  IP: -\n"),
                }

                if row.Type == 6 {
                    if row.AdminStatus as u32 == 2 {
                        output.push_str("  Diagnostico: adaptador Ethernet deshabilitado.\n");
                    } else if row.MediaConnectState as u32 == 2 {
                        output.push_str("  Diagnostico: sin enlace fisico; revisar cable, punto, patchera o puerto de switch.\n");
                    } else if row.MediaConnectState as u32 == 1 {
                        output.push_str("  Diagnostico: enlace fisico presente.\n");
                    }
                }
                output.push('\n');
            }

            FreeMibTable(table.cast());
            Ok(output)
        }
    }
    fn connections(&self) -> Result<Vec<ConnectionRow>> {
        let mut rows = Vec::new();
        let mut push = |protocol: &str, local: String, remote: String, status: String, pid: u32| {
            rows.push(ConnectionRow { protocol: protocol.to_owned(), local, remote, state: status, pid, process: String::new() });
        };
        for row in table::<MIB_TCPROW_OWNER_PID>(true, AF_INET)? {
            push("TCP", format!("{}:{}", ipv4(row.dwLocalAddr), port(row.dwLocalPort)),
                format!("{}:{}", ipv4(row.dwRemoteAddr), port(row.dwRemotePort)), state(row.dwState).to_owned(), row.dwOwningPid);
        }
        for row in table::<MIB_TCP6ROW_OWNER_PID>(true, AF_INET6)? {
            push("TCP", format!("[{}]:{}", Ipv6Addr::from(row.ucLocalAddr), port(row.dwLocalPort)),
                format!("[{}]:{}", Ipv6Addr::from(row.ucRemoteAddr), port(row.dwRemotePort)), state(row.dwState).to_owned(), row.dwOwningPid);
        }
        for row in table::<MIB_UDPROW_OWNER_PID>(false, AF_INET)? {
            push("UDP", format!("{}:{}", ipv4(row.dwLocalAddr), port(row.dwLocalPort)), "*:*".to_owned(), "-".to_owned(), row.dwOwningPid);
        }
        for row in table::<MIB_UDP6ROW_OWNER_PID>(false, AF_INET6)? {
            push("UDP", format!("[{}]:{}", Ipv6Addr::from(row.ucLocalAddr), port(row.dwLocalPort)), "*:*".to_owned(), "-".to_owned(), row.dwOwningPid);
        }
        Ok(rows)
    }
    fn routes(&self) -> Result<String> {
        unsafe {
            let mut table = std::ptr::null_mut();
            check(GetIpForwardTable2(AF_UNSPEC, &mut table))?;
            if table.is_null() { return Ok(String::new()); }
            let mut output = String::from("DESTINATION                              GATEWAY                                  IFINDEX METRIC\n");
            let rows = std::slice::from_raw_parts(std::ptr::addr_of!((*table).Table).cast::<MIB_IPFORWARD_ROW2>(), (*table).NumEntries as usize);
            for row in rows {
                let destination = format!("{}/{}", address(&row.DestinationPrefix.Prefix), row.DestinationPrefix.PrefixLength);
                output.push_str(&format!("{destination:<40} {:<40} {:<7} {}\n", address(&row.NextHop), row.InterfaceIndex, row.Metric));
            }
            FreeMibTable(table.cast());
            Ok(output)
        }
    }
    fn neighbors(&self) -> Result<HashMap<Ipv4Addr, String>> {
        unsafe {
            let mut table = std::ptr::null_mut();
            check(GetIpNetTable2(AF_INET, &mut table))?;
            let mut output = HashMap::new();
            if table.is_null() { return Ok(output); }
            let rows = std::slice::from_raw_parts(std::ptr::addr_of!((*table).Table).cast::<MIB_IPNET_ROW2>(), (*table).NumEntries as usize);
            for row in rows {
                if row.PhysicalAddressLength != 6 { continue; }
                let mac = row.PhysicalAddress[..6].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
                output.insert(ipv4(row.Address.Ipv4.sin_addr.S_un.S_addr), mac);
            }
            FreeMibTable(table.cast());
            Ok(output)
        }
    }
    fn resolve_neighbor(&self, destination: Ipv4Addr) -> Result<Option<String>> {
        unsafe {
            let mut mac = [0u8; 8];
            let mut length = 6u32;
            let status = SendARP(
                u32::from_ne_bytes(destination.octets()),
                0,
                mac.as_mut_ptr().cast(),
                &mut length,
            );
            if status != 0 || length != 6 {
                return Ok(None);
            }

            Ok(Some(
                mac[..6]
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(":"),
            ))
        }
    }

    fn echo(&self, destination: Ipv4Addr, ttl: u8, timeout: Duration) -> Result<EchoReply> {
        unsafe {
            let handle = IcmpCreateFile();
            if handle == INVALID_HANDLE_VALUE { return Err(std::io::Error::last_os_error().into()); }
            let options = IP_OPTION_INFORMATION { Ttl: ttl, ..Default::default() };
            let data = b"Shell Shock Tool native ICMP";
            let bytes = size_of::<ICMP_ECHO_REPLY>() + data.len() + 8;
            let mut reply = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let count = IcmpSendEcho(handle, u32::from_ne_bytes(destination.octets()), data.as_ptr().cast(), data.len() as u16,
                &options, reply.as_mut_ptr().cast(), (reply.len() * size_of::<usize>()) as u32, timeout.as_millis().min(u32::MAX as u128) as u32);
            let error = GetLastError();
            IcmpCloseHandle(handle);
            if count == 0 {
                return Ok(EchoReply { address: destination, status: error, elapsed_ms: timeout.as_millis() as u32 });
            }
            let reply = &*reply.as_ptr().cast::<ICMP_ECHO_REPLY>();
            Ok(EchoReply { address: ipv4(reply.Address), status: reply.Status, elapsed_ms: reply.RoundTripTime })
        }
    }
}
