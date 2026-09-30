use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};

const APPEARANCE_BLOCK: &str = r#"

# Apariencia de la terminal SST.
# backdrop: acrylic | blur | glass | solid
SST_BACKDROP='acrylic'
SST_FOCUSED_OPACITY=80
SST_UNFOCUSED_OPACITY=0
SST_BACKGROUND_COLOR='#111629'

# Imagen de fondo. Vacío = desactivada.
# Ruta absoluta o relativa a la carpeta de sst.exe.
SST_BACKGROUND_IMAGE=''
SST_BACKGROUND_IMAGE_OPACITY=100

# Ajuste de imagen: cover | contain | fill | preserve
SST_BACKGROUND_IMAGE_FIT='cover'

# Esquinas nativas de Windows: 0 = rectas; cualquier valor > 0 = redondeadas.
# Windows decide el radio real y el comportamiento al maximizar/restaurar.
SST_CORNER_RADIUS=16

# Separación entre la isla superior y la primera línea de contenido.
SST_CONTENT_TOP_GAP=12

# Densidad de la terminal.
SST_FONT_SIZE=13
SST_CELL_WIDTH=8
SST_CELL_HEIGHT=17
SST_TERMINAL_PADDING_X=8
SST_TERMINAL_PADDING_Y=6
"#;

const DEFAULT_CONFIG: &str = r#"# Shell Shock Tool portable shell configuration
# Bash-compatible syntax.

alias ll='ls -la'
alias la='ls -a'
alias cls='clear'

# Ejemplos:
# export SST_SITE='laboratorio'
# alias scanlab='net scan 192.168.1.0/24'

# Credenciales opcionales para intel.
# No se escriben en data/security.sources: ese archivo solo referencia estos nombres.
# export SST_MALWAREBAZAAR_AUTH_KEY='...'
# export SST_THREATFOX_AUTH_KEY='...'
# export SST_URLHAUS_AUTH_KEY='...'
"#;

const DEFAULT_SECURITY_SOURCES: &str = r#"# SST security intelligence source registry
# Editable sin recompilar SST. No guardar API keys aquí.

[source malwarebazaar]
enabled=true
adapter=abusech_hash
endpoint=https://mb-api.abuse.ch/api/v1/
auth_env=SST_MALWAREBAZAAR_AUTH_KEY
ttl_hours=24
priority=10

[source threatfox]
enabled=true
adapter=abusech_ioc
endpoint=https://threatfox-api.abuse.ch/api/v1/
auth_env=SST_THREATFOX_AUTH_KEY
ttl_hours=12
priority=20

[source urlhaus]
enabled=true
adapter=abusech_url
endpoint=https://urlhaus-api.abuse.ch/
auth_env=SST_URLHAUS_AUTH_KEY
ttl_hours=12
priority=30

[source lolbas]
enabled=true
adapter=behavior_catalog
endpoint=https://github.com/LOLBAS-Project/LOLBAS
ttl_hours=168
priority=40
"#;

#[derive(Debug, Clone)]
pub struct AppearanceConfig {
    pub backdrop: String,
    pub focused_opacity: u8,
    pub unfocused_opacity: u8,
    pub background_color: String,
    pub background_image: String,
    pub background_image_opacity: u8,
    pub background_image_fit: String,
    pub corner_radius: u16,
    pub content_top_gap: u16,
    pub font_size: u16,
    pub cell_width: u16,
    pub cell_height: u16,
    pub terminal_padding_x: u16,
    pub terminal_padding_y: u16,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            backdrop: "acrylic".to_owned(),
            focused_opacity: 80,
            unfocused_opacity: 0,
            background_color: "#111629".to_owned(),
            background_image: String::new(),
            background_image_opacity: 100,
            background_image_fit: "cover".to_owned(),
            corner_radius: 16,
            content_top_gap: 12,
            font_size: 13,
            cell_width: 8,
            cell_height: 17,
            terminal_padding_x: 8,
            terminal_padding_y: 6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn detect() -> Self {
        let root = env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        Self { root }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.config_dir())?;
        fs::create_dir_all(self.data_dir())?;
        fs::create_dir_all(self.intel_dir())?;

        let config = self.config_file();
        if !config.exists() {
            fs::write(&config, format!("{DEFAULT_CONFIG}{APPEARANCE_BLOCK}"))?;
        } else {
            self.ensure_appearance_block()?;
        }

        let sources = self.security_sources_file();
        if !sources.exists() {
            fs::write(&sources, DEFAULT_SECURITY_SOURCES)?;
        }

        self.migrate_legacy_terminal_toml()?;
        Ok(())
    }

    fn ensure_appearance_block(&self) -> Result<()> {
        let path = self.config_file();
        let mut text = fs::read_to_string(&path)?;
        if !text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("SST_BACKDROP=") || line.starts_with("export SST_BACKDROP=")
        }) {
            text.push_str(APPEARANCE_BLOCK);
        } else {
            if !text.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("SST_CONTENT_TOP_GAP=")
                    || line.starts_with("export SST_CONTENT_TOP_GAP=")
            }) {
                text.push_str("\n# Separación entre la isla superior y la primera línea de contenido.\nSST_CONTENT_TOP_GAP=12\n");
            }
            if !text.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("SST_BACKGROUND_IMAGE_FIT=")
                    || line.starts_with("export SST_BACKGROUND_IMAGE_FIT=")
            }) {
                text.push_str("\n# Ajuste de imagen: cover | contain | fill | preserve\nSST_BACKGROUND_IMAGE_FIT='cover'\n");
            }

            for (key, value, comment) in [
                ("SST_FONT_SIZE", "13", "# Tamaño de fuente de la terminal."),
                ("SST_CELL_WIDTH", "8", "# Ancho de celda de la terminal."),
                ("SST_CELL_HEIGHT", "17", "# Alto de celda de la terminal."),
                ("SST_TERMINAL_PADDING_X", "8", "# Padding horizontal de la terminal."),
                ("SST_TERMINAL_PADDING_Y", "6", "# Padding vertical de la terminal."),
            ] {
                let direct = format!("{key}=");
                let exported = format!("export {key}=");
                if !text.lines().any(|line| {
                    let line = line.trim_start();
                    line.starts_with(direct.as_str()) || line.starts_with(exported.as_str())
                }) {
                    text.push_str(&format!("\n{comment}\n{key}={value}\n"));
                }
            }
        }
        fs::write(path, text)?;
        Ok(())
    }

    fn migrate_legacy_terminal_toml(&self) -> Result<()> {
        let legacy = self.config_dir().join("terminal.toml");
        if !legacy.exists() {
            return Ok(());
        }

        let config_path = self.config_file();
        let mut config = fs::read_to_string(&config_path)?;
        let legacy_text = fs::read_to_string(&legacy).unwrap_or_default();

        // If the unified sstrc still has only defaults, carry across the user's
        // previous terminal.toml values before removing the obsolete file.
        if let Ok(value) = legacy_text.parse::<toml::Value>() {
            if let Some(appearance) = value.get("appearance").and_then(toml::Value::as_table) {
                let mut replace = |key: &str, value: String| {
                    set_assignment(&mut config, key, &value);
                };

                if let Some(value) = appearance.get("backdrop").and_then(toml::Value::as_str) {
                    replace("SST_BACKDROP", value.to_owned());
                }
                if let Some(value) = appearance.get("focused_opacity")
                    .or_else(|| appearance.get("background_opacity"))
                    .and_then(toml::Value::as_integer)
                {
                    replace("SST_FOCUSED_OPACITY", value.clamp(0, 100).to_string());
                }
                if let Some(value) = appearance.get("unfocused_opacity").and_then(toml::Value::as_integer) {
                    replace("SST_UNFOCUSED_OPACITY", value.clamp(0, 100).to_string());
                }
                if let Some(value) = appearance.get("background_color").and_then(toml::Value::as_str) {
                    replace("SST_BACKGROUND_COLOR", value.to_owned());
                }
                if let Some(value) = appearance.get("background_image").and_then(toml::Value::as_str) {
                    replace("SST_BACKGROUND_IMAGE", value.to_owned());
                }
                if let Some(value) = appearance.get("background_image_opacity").and_then(toml::Value::as_integer) {
                    replace("SST_BACKGROUND_IMAGE_OPACITY", value.clamp(0, 100).to_string());
                }
                if let Some(value) = appearance.get("corner_radius").and_then(toml::Value::as_integer) {
                    replace("SST_CORNER_RADIUS", value.clamp(0, 64).to_string());
                }
            }
        }

        fs::write(config_path, config)?;
        fs::remove_file(legacy)?;
        Ok(())
    }

    pub fn load_appearance(&self) -> Result<AppearanceConfig> {
        let text = fs::read_to_string(self.config_file())
            .with_context(|| format!("No se pudo leer {}", self.config_file().display()))?;
        let mut config = AppearanceConfig::default();

        if let Some(value) = assignment_value(&text, "SST_BACKDROP") {
            config.backdrop = value.to_ascii_lowercase();
        }
        if let Some(value) = assignment_value(&text, "SST_FOCUSED_OPACITY")
            .and_then(|value| value.parse::<u8>().ok())
        {
            config.focused_opacity = value.min(100);
        }
        if let Some(value) = assignment_value(&text, "SST_UNFOCUSED_OPACITY")
            .and_then(|value| value.parse::<u8>().ok())
        {
            config.unfocused_opacity = value.min(100);
        }
        if let Some(value) = assignment_value(&text, "SST_BACKGROUND_COLOR") {
            config.background_color = value;
        }
        if let Some(value) = assignment_value(&text, "SST_BACKGROUND_IMAGE") {
            config.background_image = value;
        }
        if let Some(value) = assignment_value(&text, "SST_BACKGROUND_IMAGE_OPACITY")
            .and_then(|value| value.parse::<u8>().ok())
        {
            config.background_image_opacity = value.min(100);
        }
        if let Some(value) = assignment_value(&text, "SST_BACKGROUND_IMAGE_FIT") {
            config.background_image_fit = value.to_ascii_lowercase();
        }
        if let Some(value) = assignment_value(&text, "SST_CORNER_RADIUS")
            .and_then(|value| value.parse::<u16>().ok())
        {
            config.corner_radius = value.min(64);
        }
        if let Some(value) = assignment_value(&text, "SST_CONTENT_TOP_GAP")
            .and_then(|value| value.parse::<u16>().ok())
        {
            config.content_top_gap = value.min(96);
        }
        if let Some(value) = assignment_value(&text, "SST_FONT_SIZE")
            .and_then(|value| value.parse::<u16>().ok())
        {
            config.font_size = value.clamp(8, 32);
        }
        if let Some(value) = assignment_value(&text, "SST_CELL_WIDTH")
            .and_then(|value| value.parse::<u16>().ok())
        {
            config.cell_width = value.clamp(5, 32);
        }
        if let Some(value) = assignment_value(&text, "SST_CELL_HEIGHT")
            .and_then(|value| value.parse::<u16>().ok())
        {
            config.cell_height = value.clamp(10, 48);
        }
        if let Some(value) = assignment_value(&text, "SST_TERMINAL_PADDING_X")
            .and_then(|value| value.parse::<u16>().ok())
        {
            config.terminal_padding_x = value.min(64);
        }
        if let Some(value) = assignment_value(&text, "SST_TERMINAL_PADDING_Y")
            .and_then(|value| value.parse::<u16>().ok())
        {
            config.terminal_padding_y = value.min(64);
        }

        if !matches!(config.backdrop.as_str(), "acrylic" | "blur" | "glass" | "solid") {
            anyhow::bail!("sstrc: SST_BACKDROP debe ser acrylic, blur, glass o solid");
        }
        if !matches!(
            config.background_image_fit.as_str(),
            "cover" | "contain" | "fill" | "preserve"
        ) {
            anyhow::bail!(
                "sstrc: SST_BACKGROUND_IMAGE_FIT debe ser cover, contain, fill o preserve"
            );
        }

        Ok(config)
    }

    pub fn config_value(&self, key: &str) -> Result<Option<String>> {
        let path = self.config_file();
        let text = fs::read_to_string(&path)
            .with_context(|| format!("No se pudo leer {}", path.display()))?;
        Ok(assignment_value(&text, key))
    }

    pub fn root_dir(&self) -> PathBuf { self.root.clone() }
    pub fn data_dir(&self) -> PathBuf { self.root.join("data") }
    pub fn config_dir(&self) -> PathBuf { self.root.join("config") }
    pub fn config_file(&self) -> PathBuf { self.config_dir().join("sstrc") }
    pub fn devices_file(&self) -> PathBuf { self.data_dir().join("devices.json") }
    pub fn presence_file(&self) -> PathBuf { self.data_dir().join("network_presence.json") }
    pub fn providers_file(&self) -> PathBuf { self.data_dir().join("network_providers.json") }
    pub fn switches_file(&self) -> PathBuf { self.data_dir().join("switches.json") }
    pub fn history_file(&self) -> PathBuf { self.data_dir().join("history") }
    pub fn security_db_file(&self) -> PathBuf { self.data_dir().join("security.db") }
    pub fn security_sources_file(&self) -> PathBuf { self.data_dir().join("security.sources") }
    pub fn intel_dir(&self) -> PathBuf { self.data_dir().join("intel") }
}

fn assignment_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        Some(unquote(value.trim()))
    })
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
        {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn set_assignment(text: &mut String, key: &str, value: &str) {
    let replacement = format!("{key}={}", shell_quote(value));
    let mut replaced = false;
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();

    for line in &mut lines {
        let trimmed = line.trim_start();
        let candidate = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        if candidate.split_once('=').is_some_and(|(name, _)| name.trim() == key) {
            *line = replacement.clone();
            replaced = true;
            break;
        }
    }

    if !replaced {
        lines.push(replacement);
    }
    *text = format!("{}\n", lines.join("\n"));
}
