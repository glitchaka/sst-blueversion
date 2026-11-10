//! NOTSH Winsock2 catalog (native API, read-only, no netsh subprocess).
use crate::core::CommandOutput;
use anyhow::Result;

pub fn execute(args:&[String])->Result<CommandOutput>{
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice(){
        []|["help"]|["?"]|["--help"]=>Ok(CommandOutput::ok(
            "notsh winsock — protocolos Winsock2 visibles\n\
             notsh winsock show catalog    enumera protocolos de transporte\n\
             El catálogo SPI oculto y la reparación siguen pendientes.\n")),
        ["show","catalog"]=>native::catalog(),
        _=>Ok(CommandOutput::error("notsh winsock: subcomando nativo no implementado",2)),
    }
}
#[cfg(not(windows))]
mod native{
    use super::*;
    pub fn catalog()->Result<CommandOutput>{
        Ok(CommandOutput::error("notsh winsock: requiere Windows",2))
    }
}
#[cfg(windows)]
mod native{
    use super::*;
    use std::{mem,ptr};
    use windows_sys::Win32::Networking::WinSock::{
        WSAStartup,WSACleanup,WSAEnumProtocolsW,WSAGetLastError,WSADATA,WSAPROTOCOL_INFOW
    };
    struct Session;
    impl Session{
        fn new()->std::result::Result<Self,CommandOutput>{
            let mut data:WSADATA=unsafe{mem::zeroed()};
            let error=unsafe{WSAStartup(0x0202,&mut data)};
            if error==0{Ok(Self)}else{Err(CommandOutput::error(
                format!("notsh winsock: WSAStartup error {error}"),1
            ))}
        }
    }
    impl Drop for Session{
        fn drop(&mut self){unsafe{WSACleanup();}}
    }
    fn wide(s:&[u16])->String{
        let end=s.iter().position(|&ch|ch==0).unwrap_or(s.len());
        String::from_utf16_lossy(&s[..end])
    }
    pub fn catalog()->Result<CommandOutput>{
        let _guard=match Session::new(){Ok(s)=>s,Err(e)=>return Ok(e)};
        let mut capacity=64usize;
        for _ in 0..5{
            let mut list:Vec<WSAPROTOCOL_INFOW>=(0..capacity).map(|_|unsafe{mem::zeroed()}).collect();
            let mut bytes=(capacity*mem::size_of::<WSAPROTOCOL_INFOW>()) as u32;
            let count=unsafe{WSAEnumProtocolsW(ptr::null_mut(),list.as_mut_ptr(),&mut bytes)};
            if count>=0{
                let mut output=format!("WINSOCK — {} PROTOCOLOS DE TRANSPORTE VISIBLES\n",count);
                output.push_str("La API omite proveedores SPI ocultos y algunos LSP.\n\n\
                    ID      FAMILIA    PROTOCOLO   SOCKET   DESCRIPCIÓN\n");
                for item in list.iter().take(count as usize){
                    output.push_str(&format!("{:<7} {:<10} {:<11} {:<8} {}\n",
                        item.dwCatalogEntryId,item.iAddressFamily,
                        item.iProtocol,item.iSocketType,wide(&item.szProtocol)));
                }
                return Ok(CommandOutput::ok(output));
            }
            let error=unsafe{WSAGetLastError()};
            let required=(bytes as usize).div_ceil(mem::size_of::<WSAPROTOCOL_INFOW>());
            if matches!(error,10055|10014)&&required>capacity&&required<=4096{
                capacity=required;continue;
            }
            return Ok(CommandOutput::error(format!(
                "notsh winsock: WSAEnumProtocolsW error {error}"),1));
        }
        Ok(CommandOutput::error("notsh winsock: catálogo cambió durante la consulta",1))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn rejects_unsupported_reset(){
        assert!(execute(&[]).unwrap().stdout.contains("show catalog"));
        assert_eq!(execute(&["reset".into()]).unwrap().status,2);
    }
}
