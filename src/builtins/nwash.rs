use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::Result;
use sysinfo::{Pid, System};

use crate::core::{CommandContext, CommandOutput};

use super::BuiltinCommand;

fn run(program: &str, args: &[String]) -> Result<CommandOutput> {
    let mut command = Command::new(program);
    command.args(args);

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command
        .output()
        .map_err(|e| anyhow::anyhow!("{program}: no se pudo ejecutar: {e}"))?;
    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code().unwrap_or(1),
    })
}

fn help_requested(args: &[String]) -> bool {
    args.iter().any(|a| matches!(a.as_str(), "-h" | "--help" | "help"))
}

macro_rules! simple_builtin {
    ($name:ident, $command:literal, $help:literal, $handler:ident) => {
        pub struct $name;
        impl BuiltinCommand for $name {
            fn name(&self) -> &'static str { $command }
            fn help(&self) -> &'static str { $help }
            fn execute(&self, _: &str, args: &[String], _: CommandContext<'_>) -> Result<CommandOutput> {
                $handler(args)
            }
        }
    };
}

simple_builtin!(EventLogBuiltin, "eventlog", "eventlog — Windows Event Log: list, read, info, export y clear", eventlog);
simple_builtin!(ServiceBuiltin, "service", "service — servicios Windows: list, status, start, stop, pause, resume y restart", service);
simple_builtin!(RegistryBuiltin, "registry", "registry — Registro Windows: get, set, delete, export e import", registry);
simple_builtin!(ProcessBuiltin, "process", "process — procesos Windows: list, info, tree y kill", process);
simple_builtin!(AclBuiltin, "acl", "acl — ACL Windows: show, grant, deny, revoke, inherit y reset", acl);
simple_builtin!(PnpBuiltin, "pnp", "pnp — dispositivos Plug and Play: list, info, enable, disable, restart y scan", pnp);
simple_builtin!(TaskBuiltin, "task", "task — tareas programadas de Windows", task);
simple_builtin!(SessionBuiltin, "session", "session — sesiones locales/RDP de Windows", session);
simple_builtin!(ShareBuiltin, "share", "share — recursos compartidos SMB de Windows", share);
simple_builtin!(FirewallBuiltin, "firewall", "firewall — perfiles y reglas de Windows Firewall", firewall);
simple_builtin!(PowerBuiltin, "power", "power — apagado, reinicio, cierre de sesión e hibernación", power);

fn eventlog(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) {
        return Ok(CommandOutput::ok(
"eventlog — acceso Nwash a Windows Event Log
uso:
  eventlog list
  eventlog publishers
  eventlog info LOG
  eventlog read [LOG] [--count N] [--query XPATH] [--xml]
  eventlog export LOG ARCHIVO [--query XPATH] [--overwrite]
  eventlog clear LOG [--backup ARCHIVO]

Las operaciones que Windows proteja requieren una shell elevada o 'sudo eventlog ...'.
"));
    }
    match args[0].as_str() {
        "list" => run("wevtutil.exe", &["el".into()]),
        "publishers" => run("wevtutil.exe", &["ep".into()]),
        "info" => args.get(1).map(|log| run("wevtutil.exe", &["gl".into(), log.clone()]))
            .unwrap_or_else(|| Ok(CommandOutput::error("eventlog info: falta LOG", 2))),
        "read" => {
            let mut log = "System".to_owned();
            let mut count = "20".to_owned();
            let mut query = None;
            let mut xml = false;
            let mut i=1;
            while i<args.len() {
                match args[i].as_str() {
                    "--count" => { i+=1; if let Some(v)=args.get(i){count=v.clone()} else {return Ok(CommandOutput::error("eventlog read: --count requiere N",2));}},
                    "--query" => { i+=1; if let Some(v)=args.get(i){query=Some(v.clone())} else {return Ok(CommandOutput::error("eventlog read: --query requiere XPATH",2));}},
                    "--xml" => xml=true,
                    x if !x.starts_with('-') => log=x.to_owned(),
                    x => return Ok(CommandOutput::error(format!("eventlog read: opción desconocida: {x}"),2)),
                }
                i+=1;
            }
            let mut native=vec!["qe".into(),log,format!("/c:{count}"),"/rd:true".into(),format!("/f:{}",if xml{"xml"}else{"text"})];
            if let Some(q)=query {native.push(format!("/q:{q}"));}
            run("wevtutil.exe",&native)
        }
        "export" => {
            if args.len()<3 { return Ok(CommandOutput::error("eventlog export: uso: eventlog export LOG ARCHIVO [--query XPATH] [--overwrite]",2)); }
            let mut native=vec!["epl".into(),args[1].clone(),args[2].clone()];
            let mut i=3;
            while i<args.len() {
                match args[i].as_str() {
                    "--query" => {i+=1;if let Some(v)=args.get(i){native.push(format!("/q:{v}"));}else{return Ok(CommandOutput::error("eventlog export: --query requiere XPATH",2));}},
                    "--overwrite" => native.push("/ow:true".into()),
                    x => return Ok(CommandOutput::error(format!("eventlog export: opción desconocida: {x}"),2)),
                } i+=1;
            }
            run("wevtutil.exe",&native)
        }
        "clear" => {
            let Some(log)=args.get(1) else {return Ok(CommandOutput::error("eventlog clear: falta LOG",2));};
            let mut native=vec!["cl".into(),log.clone()];
            if let Some(pos)=args.iter().position(|x|x=="--backup") {
                if let Some(v)=args.get(pos+1){native.push(format!("/bu:{v}"));}else{return Ok(CommandOutput::error("eventlog clear: --backup requiere ARCHIVO",2));}
            }
            run("wevtutil.exe",&native)
        }
        x => Ok(CommandOutput::error(format!("eventlog: subcomando desconocido: {x}"),2)),
    }
}

fn service(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) {
        return Ok(CommandOutput::ok(
"service — control Nwash de servicios Windows
uso:
  service list [--running|--stopped]
  service status NOMBRE
  service start NOMBRE
  service stop NOMBRE
  service pause NOMBRE
  service resume NOMBRE
  service restart NOMBRE

start/stop/pause/resume/restart pueden requerir 'sudo'.
"));
    }
    match args[0].as_str() {
        "list" => {
            let state=if args.iter().any(|x|x=="--running"){"active"}else if args.iter().any(|x|x=="--stopped"){"inactive"}else{"all"};
            run("sc.exe",&["query".into(),"state=".into(),state.into()])
        }
        "status" => args.get(1).map(|n|run("sc.exe",&["query".into(),n.clone()])).unwrap_or_else(||Ok(CommandOutput::error("service status: falta NOMBRE",2))),
        "start"|"stop"|"pause"|"resume" => {
            let op=if args[0]=="resume"{"continue"}else{args[0].as_str()};
            args.get(1).map(|n|run("sc.exe",&[op.into(),n.clone()])).unwrap_or_else(||Ok(CommandOutput::error(format!("service {}: falta NOMBRE",args[0]),2)))
        }
        "restart" => {
            let Some(n)=args.get(1) else{return Ok(CommandOutput::error("service restart: falta NOMBRE",2));};
            let stopped=run("sc.exe",&["stop".into(),n.clone()])?;
            if stopped.status!=0 {return Ok(stopped);}
            let mut stopped_state=false;
            for _ in 0..40 {
                let state=run("sc.exe",&["query".into(),n.clone()])?;
                if state.stdout.contains("STOPPED") { stopped_state=true; break; }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            if !stopped_state {
                return Ok(CommandOutput::error(format!("service restart: {n} no alcanzó STOPPED antes del timeout"),1));
            }
            run("sc.exe",&["start".into(),n.clone()])
        }
        x=>Ok(CommandOutput::error(format!("service: subcomando desconocido: {x}"),2)),
    }
}

fn registry(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) {
        return Ok(CommandOutput::ok(
"registry — acceso Nwash al Registro de Windows
uso:
  registry get CLAVE [--value NOMBRE|--default] [--recursive]
  registry set CLAVE NOMBRE DATO [--type TIPO]
  registry set-default CLAVE DATO [--type TIPO]
  registry delete CLAVE [--value NOMBRE|--default|--key]
  registry export CLAVE ARCHIVO [--overwrite]
  registry import ARCHIVO

TIPO: REG_SZ, REG_EXPAND_SZ, REG_DWORD, REG_QWORD, REG_MULTI_SZ o REG_BINARY.
Las mutaciones pueden requerir 'sudo'.
"));
    }
    match args[0].as_str() {
        "get" => {
            let Some(k)=args.get(1) else{return Ok(CommandOutput::error("registry get: falta CLAVE",2));};
            let mut n=vec!["query".into(),k.clone()];
            if let Some(p)=args.iter().position(|x|x=="--value"){if let Some(v)=args.get(p+1){n.extend(["/v".into(),v.clone()]);}}
            if args.iter().any(|x|x=="--default"){n.push("/ve".into());}
            if args.iter().any(|x|x=="--recursive"){n.push("/s".into());}
            run("reg.exe",&n)
        }
        "set"|"set-default" => {
            let default=args[0]=="set-default";
            let Some(k)=args.get(1) else{return Ok(CommandOutput::error("registry set: falta CLAVE",2));};
            let (name,data)=if default {(None,args.get(2))}else{(args.get(2),args.get(3))};
            let Some(data)=data else{return Ok(CommandOutput::error("registry set: faltan argumentos",2));};
            let mut n=vec!["add".into(),k.clone()];
            if let Some(name)=name{n.extend(["/v".into(),name.clone()]);}else{n.push("/ve".into());}
            n.extend(["/d".into(),data.clone()]);
            if let Some(p)=args.iter().position(|x|x=="--type"){if let Some(t)=args.get(p+1){n.extend(["/t".into(),t.clone()]);}}
            n.push("/f".into()); run("reg.exe",&n)
        }
        "delete" => {
            let Some(k)=args.get(1) else{return Ok(CommandOutput::error("registry delete: falta CLAVE",2));};
            let mut n=vec!["delete".into(),k.clone()];
            if let Some(p)=args.iter().position(|x|x=="--value"){if let Some(v)=args.get(p+1){n.extend(["/v".into(),v.clone()]);}}
            else if args.iter().any(|x|x=="--default"){n.push("/ve".into());}
            else if !args.iter().any(|x|x=="--key"){return Ok(CommandOutput::error("registry delete: indica --value NOMBRE, --default o --key",2));}
            n.push("/f".into()); run("reg.exe",&n)
        }
        "export" => {
            if args.len()<3{return Ok(CommandOutput::error("registry export: uso: registry export CLAVE ARCHIVO [--overwrite]",2));}
            let mut n=vec!["export".into(),args[1].clone(),args[2].clone()];
            if args.iter().any(|x|x=="--overwrite"){n.push("/y".into());}
            run("reg.exe",&n)
        }
        "import" => args.get(1).map(|f|run("reg.exe",&["import".into(),f.clone()])).unwrap_or_else(||Ok(CommandOutput::error("registry import: falta ARCHIVO",2))),
        x=>Ok(CommandOutput::error(format!("registry: subcomando desconocido: {x}"),2)),
    }
}

fn process(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) {
        return Ok(CommandOutput::ok(
"process — procesos Windows para scripts Nwash
uso:
  process list
  process info PID
  process tree PID
  process kill PID [--tree] [--force]

'process kill' usa taskkill; procesos protegidos pueden requerir 'sudo'.
"));
    }
    match args[0].as_str() {
        "list"=>run("tasklist.exe",&[]),
        "info"=>{
            let Some(pid)=args.get(1) else{return Ok(CommandOutput::error("process info: falta PID",2));};
            run("tasklist.exe",&["/FI".into(),format!("PID eq {pid}"),"/V".into()])
        }
        "tree"=>{
            let Some(pid)=args.get(1).and_then(|value| value.parse::<u32>().ok()) else{return Ok(CommandOutput::error("process tree: requiere PID numérico",2));};
            process_tree(pid)
        }
        "kill"=>{
            let Some(pid)=args.get(1) else{return Ok(CommandOutput::error("process kill: falta PID",2));};
            let mut n=vec!["/PID".into(),pid.clone()];
            if args.iter().any(|x|x=="--tree"){n.push("/T".into());}
            if args.iter().any(|x|x=="--force"){n.push("/F".into());}
            run("taskkill.exe",&n)
        }
        x=>Ok(CommandOutput::error(format!("process: subcomando desconocido: {x}"),2)),
    }
}

fn acl(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) {
        return Ok(CommandOutput::ok(
"acl — ACL NTFS para Nwash
uso:
  acl show RUTA
  acl grant RUTA USUARIO PERMISO
  acl deny RUTA USUARIO PERMISO
  acl revoke RUTA USUARIO
  acl inherit RUTA on|off
  acl reset RUTA

PERMISO acepta valores de icacls, por ejemplo F, M, RX, R o W.
Las modificaciones pueden requerir 'sudo'.
"));
    }
    let Some(path)=args.get(1) else{return Ok(CommandOutput::error(format!("acl {}: falta RUTA",args[0]),2));};
    match args[0].as_str() {
        "show"=>run("icacls.exe",&[path.clone()]),
        "grant"|"deny"=>{
            if args.len()<4{return Ok(CommandOutput::error(format!("acl {}: faltan USUARIO y PERMISO",args[0]),2));}
            run("icacls.exe",&[path.clone(),format!("/{}",args[0]),format!("{}:{}",args[2],args[3])])
        }
        "revoke"=>{
            let Some(user)=args.get(2) else{return Ok(CommandOutput::error("acl revoke: falta USUARIO",2));};
            run("icacls.exe",&[path.clone(),"/remove".into(),user.clone()])
        }
        "inherit"=>{
            let Some(mode)=args.get(2) else{return Ok(CommandOutput::error("acl inherit: falta on|off",2));};
            let flag=match mode.as_str(){"on"=>"/inheritance:e","off"=>"/inheritance:d",_=>return Ok(CommandOutput::error("acl inherit: usa on u off",2))};
            run("icacls.exe",&[path.clone(),flag.into()])
        }
        "reset"=>run("icacls.exe",&[path.clone(),"/reset".into()]),
        x=>Ok(CommandOutput::error(format!("acl: subcomando desconocido: {x}"),2)),
    }
}

fn pnp(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) {
        return Ok(CommandOutput::ok(
"pnp — dispositivos Plug and Play para Nwash
uso:
  pnp list [--connected|--disconnected|--problem]
  pnp info INSTANCE_ID
  pnp enable INSTANCE_ID
  pnp disable INSTANCE_ID
  pnp restart INSTANCE_ID
  pnp scan

enable/disable/restart/scan pueden requerir 'sudo'.
"));
    }
    match args[0].as_str() {
        "list"=>{
            let mut n=vec!["/enum-devices".into()];
            if args.iter().any(|x|x=="--connected"){n.push("/connected".into());}
            if args.iter().any(|x|x=="--disconnected"){n.push("/disconnected".into());}
            if args.iter().any(|x|x=="--problem"){n.push("/problem".into());}
            run("pnputil.exe",&n)
        }
        "info"=>{
            let Some(id)=args.get(1) else{return Ok(CommandOutput::error("pnp info: falta INSTANCE_ID",2));};
            run("pnputil.exe",&["/enum-devices".into(),"/instanceid".into(),id.clone(),"/properties".into()])
        }
        "enable"|"disable"|"restart"=>{
            let Some(id)=args.get(1) else{return Ok(CommandOutput::error(format!("pnp {}: falta INSTANCE_ID",args[0]),2));};
            run("pnputil.exe",&[format!("/{}-device",args[0]),id.clone()])
        }
        "scan"=>run("pnputil.exe",&["/scan-devices".into()]),
        x=>Ok(CommandOutput::error(format!("pnp: subcomando desconocido: {x}"),2)),
    }
}

fn process_tree(root_pid: u32) -> Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_all();
    let root = Pid::from_u32(root_pid);
    if system.process(root).is_none() {
        return Ok(CommandOutput::error(format!("process tree: no existe PID {root_pid}"), 1));
    }

    let mut children = std::collections::HashMap::<Pid, Vec<Pid>>::new();
    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent() {
            children.entry(parent).or_default().push(*pid);
        }
    }
    for pids in children.values_mut() {
        pids.sort_by_key(|pid| pid.as_u32());
    }

    fn walk(
        system: &System,
        children: &std::collections::HashMap<Pid, Vec<Pid>>,
        pid: Pid,
        prefix: &str,
        last: bool,
        root: bool,
        out: &mut String,
    ) {
        let name = system.process(pid)
            .map(|p| p.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".to_owned());
        if root {
            out.push_str(&format!("{} {}\n", pid.as_u32(), name));
        } else {
            out.push_str(prefix);
            out.push_str(if last { "└─ " } else { "├─ " });
            out.push_str(&format!("{} {}\n", pid.as_u32(), name));
        }

        let Some(kids) = children.get(&pid) else { return; };
        let next_prefix = if root {
            String::new()
        } else {
            format!("{}{}", prefix, if last { "   " } else { "│  " })
        };
        for (index, child) in kids.iter().enumerate() {
            walk(system, children, *child, &next_prefix, index + 1 == kids.len(), false, out);
        }
    }

    let mut out = String::from("PID PROCESS\n");
    walk(&system, &children, root, "", true, true, &mut out);
    Ok(CommandOutput::ok(out))
}

fn task(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) { return Ok(CommandOutput::ok(
"task — tareas programadas Windows
uso:
  task list [--verbose|--csv]
  task show NOMBRE [--xml]
  task run NOMBRE
  task end NOMBRE
  task enable NOMBRE
  task disable NOMBRE
  task delete NOMBRE
Las mutaciones pueden requerir sudo.
")); }
    match args[0].as_str() {
        "list" => { let mut a=vec!["/Query".into()]; if args.iter().any(|x|x=="--verbose"){a.push("/V".into());} if args.iter().any(|x|x=="--csv"){a.extend(["/FO".into(),"CSV".into()]);} run("schtasks.exe",&a) }
        "show" => { let Some(n)=args.get(1) else{return Ok(CommandOutput::error("task show: falta NOMBRE",2));}; let mut a=vec!["/Query".into(),"/TN".into(),n.clone()]; if args.iter().any(|x|x=="--xml"){a.push("/XML".into());}else{a.push("/V".into());} run("schtasks.exe",&a) }
        "run"|"end"|"delete" => { let Some(n)=args.get(1) else{return Ok(CommandOutput::error(format!("task {}: falta NOMBRE",args[0]),2));}; let op=match args[0].as_str(){"run"=>"/Run","end"=>"/End",_=>"/Delete"}; let mut a=vec![op.into(),"/TN".into(),n.clone()]; if args[0]=="delete"{a.push("/F".into());} run("schtasks.exe",&a) }
        "enable"|"disable" => { let Some(n)=args.get(1) else{return Ok(CommandOutput::error(format!("task {}: falta NOMBRE",args[0]),2));}; run("schtasks.exe",&["/Change".into(),"/TN".into(),n.clone(),if args[0]=="enable"{"/ENABLE".into()}else{"/DISABLE".into()}]) }
        x=>Ok(CommandOutput::error(format!("task: subcomando desconocido: {x}"),2)),
    }
}

fn session(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) { return Ok(CommandOutput::ok(
"session — sesiones Windows
uso:
  session list
  session users
  session logoff ID
  session message ID TEXTO
logoff/message pueden requerir sudo.
")); }
    match args[0].as_str() {
        "list"=>run("query.exe",&["session".into()]),
        "users"=>run("query.exe",&["user".into()]),
        "logoff"=>args.get(1).map(|id|run("logoff.exe",&[id.clone()])).unwrap_or_else(||Ok(CommandOutput::error("session logoff: falta ID",2))),
        "message"=>{if args.len()<3{return Ok(CommandOutput::error("session message: uso: session message ID TEXTO",2));} run("msg.exe",&[args[1].clone(),args[2..].join(" ")])}
        x=>Ok(CommandOutput::error(format!("session: subcomando desconocido: {x}"),2)),
    }
}

fn share(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) { return Ok(CommandOutput::ok(
"share — recursos SMB Windows
uso:
  share list
  share sessions
  share files
  share add NOMBRE RUTA
  share remove NOMBRE
Las mutaciones pueden requerir sudo.
")); }
    match args[0].as_str() {
        "list"=>run("net.exe",&["share".into()]),
        "sessions"=>run("net.exe",&["session".into()]),
        "files"=>run("net.exe",&["file".into()]),
        "add"=>{if args.len()<3{return Ok(CommandOutput::error("share add: uso: share add NOMBRE RUTA",2));} run("net.exe",&["share".into(),format!("{}={}",args[1],args[2])])}
        "remove"=>args.get(1).map(|n|run("net.exe",&["share".into(),n.clone(),"/delete".into(),"/y".into()])).unwrap_or_else(||Ok(CommandOutput::error("share remove: falta NOMBRE",2))),
        x=>Ok(CommandOutput::error(format!("share: subcomando desconocido: {x}"),2)),
    }
}

fn firewall(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) { return Ok(CommandOutput::ok(
"firewall — Windows Defender Firewall
uso:
  firewall status
  firewall rules
  firewall rule NOMBRE
  firewall enable PROFILE
  firewall disable PROFILE
PROFILE: domain | private | public | all
Las mutaciones pueden requerir sudo.
")); }
    match args[0].as_str() {
        "status"=>run("netsh.exe",&["advfirewall".into(),"show".into(),"allprofiles".into()]),
        "rules"=>run("netsh.exe",&["advfirewall".into(),"firewall".into(),"show".into(),"rule".into(),"name=all".into()]),
        "rule"=>args.get(1).map(|n|run("netsh.exe",&["advfirewall".into(),"firewall".into(),"show".into(),"rule".into(),format!("name={n}")])).unwrap_or_else(||Ok(CommandOutput::error("firewall rule: falta NOMBRE",2))),
        "enable"|"disable"=>{let Some(p)=args.get(1)else{return Ok(CommandOutput::error(format!("firewall {}: falta PROFILE",args[0]),2));}; let profile=match p.to_ascii_lowercase().as_str(){"domain"=>"domainprofile","private"=>"privateprofile","public"=>"publicprofile","all"=>"allprofiles",_=>return Ok(CommandOutput::error("firewall: PROFILE debe ser domain, private, public o all",2))}; run("netsh.exe",&["advfirewall".into(),"set".into(),profile.into(),"state".into(),if args[0]=="enable"{"on".into()}else{"off".into()}])}
        x=>Ok(CommandOutput::error(format!("firewall: subcomando desconocido: {x}"),2)),
    }
}

fn power(args: &[String]) -> Result<CommandOutput> {
    if args.is_empty() || help_requested(args) { return Ok(CommandOutput::ok(
"power — control de energía Windows
uso:
  power shutdown [--force]
  power restart [--force]
  power logoff [--force]
  power hibernate
shutdown/restart requieren sudo según la política del equipo.
")); }
    match args[0].as_str() {
        "shutdown"|"restart"|"logoff"=>{let mut a=vec![match args[0].as_str(){"shutdown"=>"/s","restart"=>"/r",_=>"/l"}.into(),"/t".into(),"0".into()];if args.iter().any(|x|x=="--force"){a.push("/f".into());}run("shutdown.exe",&a)}
        "hibernate"=>run("shutdown.exe",&["/h".into()]),
        x=>Ok(CommandOutput::error(format!("power: subcomando desconocido: {x}"),2)),
    }
}
