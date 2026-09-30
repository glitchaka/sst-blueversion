use std::sync::Arc;

use anyhow::Result;

use crate::{
    core::{
        CommandOutput,
        models::network::NetworkProvider,
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

    pub fn usage(&self, _args: &[String]) -> Result<CommandOutput> {
        let providers = self.repository.all()?;
        let Some(active) = providers.iter().find(|provider| provider.active) else {
            return Ok(CommandOutput::error(
                "net usage: no hay proveedor activo. Usa 'net provider add' y 'net provider use'.",
                2,
            ));
        };

        Ok(CommandOutput::error(
            format!(
                "net usage: proveedor '{}' ({}) configurado en {}, pero el driver de contadores por cliente todavía no está implementado para este tipo",
                active.name, active.kind, active.host
            ),
            2,
        ))
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
