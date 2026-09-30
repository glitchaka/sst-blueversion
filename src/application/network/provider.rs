use std::{collections::HashMap, env, process::Command, sync::Arc, time::{Duration, Instant}};

use anyhow::Result;

use crate::{
    core::{
        CommandOutput,
        models::network::{LanUsageRow, NetworkProvider},
        ports::NetworkProviderRepository,
    },
    support::options,
};

pub struct NetworkProviderService {
    repository: Arc<dyn NetworkProviderRepository>,
}

impl NetworkProviderService {
    pub fn new(repository: Arc<dyn NetworkProviderRepository>) -> Self {
        Self { repository }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("list") {
            "list" => self.list(args.get(1..).unwrap_or_default()),
            "add" => self.add(args.get(1..).unwrap_or_default()),
            "use" => self.use_provider(args.get(1..).unwrap_or_default()),
            "remove" => self.remove(args.get(1..).unwrap_or_default()),
            "current" => self.current(),
            "path" => Ok(CommandOutput::ok(format!("{}\n", self.repository.path().display()))),
            "capabilities" => self.capabilities(args.get(1..).unwrap_or_default()),
            other => Ok(CommandOutput::error(
                format!("net provider: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    pub fn usage(&self, args: &[String]) -> Result<CommandOutput> {
        let providers = self.repository.all()?;
        let Some(active) = providers.iter().find(|provider| provider.active) else {
            return Ok(CommandOutput::error("net usage: no hay proveedor activo. Usa 'net provider add' y 'net provider use'.",2));
        };
        if args.iter().any(|a| a=="--watch" || a=="-w") {
            return Ok(CommandOutput::error("net usage --watch: usa snapshots repetidos desde el proveedor; la TUI se incorporará sobre este backend sin estimar tráfico local",2));
        }
        let rows = match active.kind.as_str() {
            "openwrt" => self.openwrt_usage(active)?,
            "generic" => self.generic_usage(active)?,
            "snmp" => return Ok(CommandOutput::error("net usage: SNMP estándar no define contadores por cliente; configura un proveedor/MIB específico en vez de inventar datos",2)),
            "opnsense"|"pfsense"|"unifi" => return Ok(CommandOutput::error(format!("net usage: el perfil {} requiere endpoint/API específico de la instalación; usa --type generic con un endpoint JSON normalizado o un backend dedicado",active.kind),2)),
            other => return Ok(CommandOutput::error(format!("net usage: proveedor no soportado: {other}"),2)),
        };
        self.render_usage(args, rows)
    }

    fn render_usage(&self,args:&[String],mut rows:Vec<LanUsageRow>)->Result<CommandOutput>{
        if let Some(ip)=options::value(args,"--device"){rows.retain(|r|r.ip==ip);}
        if let Some(mac)=options::value(args,"--mac"){rows.retain(|r|r.mac.eq_ignore_ascii_case(mac));}
        rows.sort_by_key(|r|std::cmp::Reverse(r.download_bps.saturating_add(r.upload_bps)));
        if let Some(v)=options::value(args,"--top").and_then(|v|v.parse::<usize>().ok()){rows.truncate(v);}
        if options::has(args,"--json"){return Ok(CommandOutput::ok(format!("{}\n",serde_json::to_string_pretty(&rows)?)));}
        if options::has(args,"--csv"){
            let mut out=String::from("device,ip,mac,download_bps,upload_bps,total_bps\n");
            for r in rows{out.push_str(&format!("{},{},{},{},{},{}\n",r.device,r.ip,r.mac,r.download_bps,r.upload_bps,r.download_bps.saturating_add(r.upload_bps)));}
            return Ok(CommandOutput::ok(out));
        }
        let mut out=String::from("DEVICE                   IP               MAC                 DOWN         UP           TOTAL\n");
        for r in rows{out.push_str(&format!("{:<24} {:<16} {:<19} {:>12} {:>12} {:>12}\n",r.device,r.ip,r.mac,rate(r.download_bps),rate(r.upload_bps),rate(r.download_bps.saturating_add(r.upload_bps))));}
        Ok(CommandOutput::ok(out))
    }

    fn generic_usage(&self,p:&NetworkProvider)->Result<Vec<LanUsageRow>>{
        let text=http_json(p,"")?;
        parse_usage_json(&text)
    }

    fn openwrt_usage(&self,p:&NetworkProvider)->Result<Vec<LanUsageRow>>{
        // OpenWrt deployments differ; SST consumes a ubus/cgi endpoint configured in host
        // and requires normalized JSON counters. This keeps the CLI stable without fabricating data.
        let text=http_json(p,"")?;
        parse_usage_json(&text)
    }

    fn list(&self, args: &[String]) -> Result<CommandOutput> {
        let providers = self.repository.all()?;

        if args.iter().any(|arg| arg == "--json") {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&providers)?
            )));
        }

        let mut out = String::from("ACTIVE  NAME                 TYPE           HOST\n");
        for provider in providers {
            out.push_str(&format!(
                "{:<7} {:<20} {:<14} {}\n",
                if provider.active { "*" } else { "" },
                provider.name,
                provider.kind,
                provider.host
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    fn add(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(name) = args.first() else {
            return Ok(CommandOutput::error(
                "net provider add: uso: net provider add NOMBRE --type TIPO --host HOST",
                2,
            ));
        };

        let kind = options::value(args, "--type").unwrap_or("generic").to_ascii_lowercase();
        if !matches!(kind.as_str(), "openwrt" | "opnsense" | "pfsense" | "unifi" | "snmp" | "generic") {
            return Ok(CommandOutput::error(
                format!("net provider add: tipo no soportado: {kind}"),
                2,
            ));
        }
        let Some(host) = options::value(args, "--host") else {
            return Ok(CommandOutput::error("net provider add: falta --host", 2));
        };

        let user_env = options::value(args, "--user-env").map(str::to_owned);
        let secret_env = options::value(args, "--secret-env").map(str::to_owned);
        let community_env = options::value(args, "--community-env").map(str::to_owned);

        let has_credentials = community_env.is_some() || secret_env.is_some() || user_env.is_some();
        let mut providers = self.repository.all()?;

        if let Some(existing) = providers.iter_mut().find(|provider| provider.name == *name) {
            existing.kind = kind.clone();
            existing.host = host.to_owned();
            existing.user_env = user_env.clone();
            existing.secret_env = secret_env.clone();
            existing.community_env = community_env.clone();
        } else {
            let active = providers.is_empty();
            providers.push(NetworkProvider {
                name: name.clone(),
                kind: kind.clone(),
                host: host.to_owned(),
                active,
                user_env,
                secret_env,
                community_env,
            });
        }

        self.repository.replace_all(&providers)?;

        Ok(CommandOutput::ok(format!(
            "provider saved: {} type={} host={} credentials={}\n",
            name, kind, host,
            if has_credentials { "env" } else { "none" }
        )))
    }

    fn use_provider(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(name) = args.first() else {
            return Ok(CommandOutput::error("net provider use: falta nombre", 2));
        };

        let mut providers = self.repository.all()?;
        if !providers.iter().any(|provider| provider.name == *name) {
            return Ok(CommandOutput::error(format!("net provider: no existe {name}"), 1));
        }

        for provider in &mut providers {
            provider.active = provider.name == *name;
        }

        self.repository.replace_all(&providers)?;
        Ok(CommandOutput::ok(format!("active provider: {name}\n")))
    }

    fn remove(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(name) = args.first() else {
            return Ok(CommandOutput::error("net provider remove: falta nombre", 2));
        };

        let mut providers = self.repository.all()?;
        let was_active = providers
            .iter()
            .find(|provider| provider.name == *name)
            .is_some_and(|provider| provider.active);
        let before = providers.len();
        providers.retain(|provider| provider.name != *name);

        if providers.len() == before {
            return Ok(CommandOutput::error(format!("net provider: no existe {name}"), 1));
        }

        if was_active {
            if let Some(first) = providers.first_mut() {
                first.active = true;
            }
        }

        self.repository.replace_all(&providers)?;
        Ok(CommandOutput::ok(format!("provider removed: {name}\n")))
    }

    fn current(&self) -> Result<CommandOutput> {
        let providers = self.repository.all()?;

        if let Some(provider) = providers.iter().find(|provider| provider.active) {
            Ok(CommandOutput::ok(format!(
                "name: {}\ntype: {}\nhost: {}\nuser-env: {}\nsecret-env: {}\ncommunity-env: {}\n",
                provider.name, provider.kind, provider.host,
                provider.user_env.as_deref().unwrap_or("-"),
                provider.secret_env.as_deref().unwrap_or("-"),
                provider.community_env.as_deref().unwrap_or("-")
            )))
        } else {
            Ok(CommandOutput::error("net provider: no hay proveedor activo", 1))
        }
    }

    fn capabilities(&self, args: &[String]) -> Result<CommandOutput> {
        let providers = self.repository.all()?;
        let selected = if let Some(name) = args.first() {
            providers.iter().find(|provider| provider.name == *name)
        } else {
            providers.iter().find(|provider| provider.active)
        };

        let Some(provider) = selected else {
            return Ok(CommandOutput::ok(
                "provider: none\ndevice-discovery: local\nper-device-traffic: no\nconnection-table: local\n",
            ));
        };

        let (traffic, fdb, api) = match provider.kind.as_str() {
            "openwrt" => ("planned", "possible", "ubus/rpc"),
            "opnsense" | "pfsense" => ("planned", "possible", "api"),
            "unifi" => ("planned", "possible", "controller-api"),
            "snmp" => ("depends-on-mib", "possible", "snmp"),
            _ => ("unknown", "unknown", "generic"),
        };

        Ok(CommandOutput::ok(format!(
            "provider: {}\ntype: {}\nhost: {}\ndevice-discovery: local\nper-device-traffic: {}\nfdb: {}\nintegration: {}\n",
            provider.name, provider.kind, provider.host, traffic, fdb, api
        )))
    }
}

fn http_json(p:&NetworkProvider,suffix:&str)->Result<String>{
    let url=format!("{}{}",p.host.trim_end_matches('/'),suffix);
    let mut cmd=Command::new("curl.exe");
    cmd.args(["-fsS","--connect-timeout","5","--max-time","15","-H","Accept: application/json"]);
    if let Some(user_var)=&p.user_env {
        let user=env::var(user_var).map_err(|_|anyhow::anyhow!("falta variable de entorno {}",user_var))?;
        let secret=p.secret_env.as_ref().map(|v|env::var(v)).transpose()?.unwrap_or_default();
        cmd.args(["-u",&format!("{user}:{secret}")]);
    } else if let Some(secret_var)=&p.secret_env {
        let token=env::var(secret_var).map_err(|_|anyhow::anyhow!("falta variable de entorno {}",secret_var))?;
        cmd.args(["-H",&format!("Authorization: Bearer {token}")]);
    }
    let out=cmd.arg(url).output().map_err(|e|anyhow::anyhow!("curl: {e}"))?;
    if !out.status.success(){anyhow::bail!("proveedor HTTP respondió con error: {}",String::from_utf8_lossy(&out.stderr).trim());}
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_usage_json(text:&str)->Result<Vec<LanUsageRow>>{
    let value:serde_json::Value=serde_json::from_str(text)?;
    let array=value.as_array().or_else(||value.get("clients").and_then(|v|v.as_array())).ok_or_else(||anyhow::anyhow!("el proveedor debe devolver un array JSON o {clients:[...]}"))?;
    array.iter().map(|v|Ok(LanUsageRow{
        device:v.get("device").or_else(||v.get("hostname")).and_then(|x|x.as_str()).unwrap_or("-").to_owned(),
        ip:v.get("ip").and_then(|x|x.as_str()).unwrap_or("-").to_owned(),
        mac:v.get("mac").and_then(|x|x.as_str()).unwrap_or("-").to_owned(),
        download_bps:v.get("download_bps").or_else(||v.get("rx_bps")).and_then(|x|x.as_u64()).unwrap_or(0),
        upload_bps:v.get("upload_bps").or_else(||v.get("tx_bps")).and_then(|x|x.as_u64()).unwrap_or(0),
    })).collect()
}

fn rate(v:u64)->String{
    if v>=1_000_000_000{format!("{:.1} Gbps",v as f64/1_000_000_000.0)}
    else if v>=1_000_000{format!("{:.1} Mbps",v as f64/1_000_000.0)}
    else if v>=1_000{format!("{:.1} Kbps",v as f64/1_000.0)}
    else{format!("{v} bps")}
}
