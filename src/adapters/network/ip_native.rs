//! NOTSH interfaces IPv4/IPv6: consulta nativa de IP Helper.
use crate::core::CommandOutput;
use anyhow::Result;
#[cfg(not(windows))]
pub fn show_interfaces(_: bool)->Result<CommandOutput>{
    Ok(CommandOutput::error("notsh interface: requiere Windows",2))
}
#[cfg(windows)]
pub fn show_interfaces(ipv6:bool)->Result<CommandOutput>{
    use std::ptr;
    use windows_sys::Win32::{
        NetworkManagement::IpHelper::{GetIpInterfaceTable,FreeMibTable,MIB_IPINTERFACE_TABLE},
        Networking::WinSock::{AF_INET,AF_INET6}
    };
    let mut table:*mut MIB_IPINTERFACE_TABLE=ptr::null_mut();
    let family=if ipv6{AF_INET6}else{AF_INET};
    let rc=unsafe{GetIpInterfaceTable(family,&mut table)};
    if rc!=0{return Ok(CommandOutput::error(
        format!("notsh interface: GetIpInterfaceTable (Win32 {rc})"),1
    ));}
    if table.is_null(){return Ok(CommandOutput::ok("Tabla IP vacía.\n"));}
    let rows=unsafe{
        let list=&*table;
        std::slice::from_raw_parts(list.Table.as_ptr(),(list.NumEntries as usize).min(16384)).to_vec()
    };
    unsafe{FreeMibTable(table.cast());}
    let mut out=format!("INTERFACES {} — {} entradas IP Helper\n\n",
        if ipv6{"IPv6"}else{"IPv4"},rows.len());
    out.push_str("IDX      METRICA    MTU      CONECTADO   FORWARDING\n");
    for item in &rows {
        out.push_str(&format!("{:<8} {:<10} {:<8} {:<11} {}\n",
            item.InterfaceIndex,item.Metric,item.NlMtu,
            if item.Connected{"SÍ"}else{"NO"},
            if item.ForwardingEnabled{"SÍ"}else{"NO"}
        ));
    }
    Ok(CommandOutput::ok(out))
}
