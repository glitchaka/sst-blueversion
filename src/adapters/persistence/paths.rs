use std::{env, fs, io::Write, path::PathBuf, sync::OnceLock, thread};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{Context, Result};

const APPEARANCE_BLOCK: &str = r#"

# Apariencia de la terminal SST.
# backdrop: acrylic | blur | glass | solid
SST_BACKDROP='acrylic'
SST_FOCUSED_OPACITY=80
SST_UNFOCUSED_OPACITY=0
SST_BACKGROUND_COLOR='#282828'

# Imagen de fondo. Vacío = desactivada.
# Ruta absoluta o relativa a la carpeta de sst.exe.
SST_BACKGROUND_IMAGE=''
SST_BACKGROUND_IMAGE_OPACITY=100

# Gestión de fondos: off | fixed | carrousel
# En carrousel se rota también al cambiar de estado de terminal/ventana.
SST_BACKGROUND_MODE='off'
SST_BACKGROUND_CAROUSEL_MINUTES=3

# Ajuste de imagen: cover | contain | fill | preserve
SST_BACKGROUND_IMAGE_FIT='cover'

# Esquinas nativas de Windows: 0 = rectas; cualquier valor > 0 = redondeadas.
# Windows decide el radio real y el comportamiento al maximizar/restaurar.
SST_CORNER_RADIUS=16

# Separación entre la isla superior y la primera línea de contenido.
SST_CONTENT_TOP_GAP=12

# Densidad y texto de la terminal.
SST_FONT_SIZE=13
SST_FONT_COLOR='#EBDBB2'
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

// Generated at build time from the committed guide/ directory.
include!(concat!(env!("OUT_DIR"), "/guide_assets.rs"));

// La instantánea publicada contiene estas fichas exactas; una descarga distinta
// no sustituye contenido por accidente.
const GUIDE_SNAPSHOT_COMMIT: &str = "38e664f3d14467cdfd080c16991055c859c48b3a";
static GUIDE_SYNC_STARTED: OnceLock<()> = OnceLock::new();

pub(crate) fn bundled_guide_asset(filename: &str) -> Option<&'static [u8]> {
    GUIDE_ASSETS.iter().find_map(|(name, bytes)| {
        (*name == filename).then_some(*bytes)
    })
}

const TOUR_SCRIPT: &str = include_str!("../../../tour.sh");
const TOUR_EXAMPLES: &[(&str, &str)] = &[
    ("README.md", include_str!("../../../examples/README.md")),
    ("01-language-tour.sh", include_str!("../../../examples/01-language-tour.sh")),
    ("02-windows-operator-report.sh", include_str!("../../../examples/02-windows-operator-report.sh")),
    ("03-network-discovery.sh", include_str!("../../../examples/03-network-discovery.sh")),
    ("04-jobs-and-coproc.sh", include_str!("../../../examples/04-jobs-and-coproc.sh")),
    ("05-pipelines-and-text.sh", include_str!("../../../examples/05-pipelines-and-text.sh")),
    ("06-security-triage.sh", include_str!("../../../examples/06-security-triage.sh")),
    ("07-operator-console.sh", include_str!("../../../examples/07-operator-console.sh")),
    ("08-config-and-backgrounds.sh", include_str!("../../../examples/08-config-and-backgrounds.sh")),
    ("09-advanced-bash.sh", include_str!("../../../examples/09-advanced-bash.sh")),
    ("10-full-showcase.sh", include_str!("../../../examples/10-full-showcase.sh")),
    ("11-diagnostico-interactivo.sh", include_str!("../../../examples/11-diagnostico-interactivo.sh")),
];

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
    pub background_mode: String,
    pub background_carousel_minutes: u64,
    pub background_image_fit: String,
    pub corner_radius: u16,
    pub content_top_gap: u16,
    pub font_size: u16,
    pub font_color: String,
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
            background_color: "#282828".to_owned(),
            background_image: String::new(),
            background_image_opacity: 100,
            background_mode: "off".to_owned(),
            background_carousel_minutes: 3,
            background_image_fit: "cover".to_owned(),
            corner_radius: 16,
            content_top_gap: 12,
            font_size: 13,
            font_color: "#EBDBB2".to_owned(),
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
        fs::create_dir_all(self.bg_dir())?;
        self.ensure_guide_assets()?;
        self.ensure_tour_assets()?;

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

    fn ensure_guide_assets(&self) -> Result<()> {
        let directory = self.root.join("guide");
        fs::create_dir_all(&directory)?;

        // No bloquear el arranque, ni esperar a abrir guide para reparar faltantes.
        // Las instancias GUI llaman ensure_layout varias veces: lanzar una sola vez.
        GUIDE_SYNC_STARTED.get_or_init(|| {
            let _ = thread::Builder::new()
                .name("sst-guide-bootstrap".to_owned())
                .spawn(move || sync_missing_guide_assets(&directory));
        });
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
            if !text.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("SST_BACKGROUND_MODE=")
                    || line.starts_with("export SST_BACKGROUND_MODE=")
            }) {
                let legacy_mode = if assignment_value(&text, "SST_BACKGROUND_IMAGE")
                    .is_some_and(|value| !value.trim().is_empty())
                {
                    "fixed"
                } else {
                    "off"
                };
                text.push_str(&format!(
                    "\n# Gestión de fondos: off | fixed | carrousel\nSST_BACKGROUND_MODE='{legacy_mode}'\n"
                ));
            }
            if !text.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("SST_BACKGROUND_CAROUSEL_MINUTES=")
                    || line.starts_with("export SST_BACKGROUND_CAROUSEL_MINUTES=")
            }) {
                text.push_str("SST_BACKGROUND_CAROUSEL_MINUTES=3\n");
            }

            for (key, value, comment) in [
                ("SST_FONT_SIZE", "13", "# Tamaño de fuente de la terminal."),
                ("SST_FONT_COLOR", "'#EBDBB2'", "# Color base del texto de la terminal (#RRGGBB)."),
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
        // Migrate only SST's previous built-in blue defaults; preserve any
        // colors deliberately customized in an existing portable sstrc.
        if assignment_value(&text, "SST_BACKGROUND_COLOR").as_deref() == Some("#111629") {
            set_assignment(&mut text, "SST_BACKGROUND_COLOR", "#282828");
        }
        if assignment_value(&text, "SST_FONT_COLOR").as_deref() == Some("#DFE8EF") {
            set_assignment(&mut text, "SST_FONT_COLOR", "#EBDBB2");
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
                if let Some(value) = appearance.get("font_color").and_then(toml::Value::as_str) {
                    replace("SST_FONT_COLOR", value.to_owned());
                }
                if let Some(value) = appearance.get("background_image").and_then(toml::Value::as_str) {
                    replace("SST_BACKGROUND_IMAGE", value.to_owned());
                    replace(
                        "SST_BACKGROUND_MODE",
                        if value.trim().is_empty() { "off".to_owned() } else { "fixed".to_owned() },
                    );
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
        if let Some(value) = assignment_value(&text, "SST_BACKGROUND_MODE") {
            config.background_mode = value.to_ascii_lowercase();
        } else if !config.background_image.trim().is_empty() {
            // Existing installations with a manually configured image retain it.
            config.background_mode = "fixed".to_owned();
        }
        if let Some(value) = assignment_value(&text, "SST_BACKGROUND_CAROUSEL_MINUTES")
            .and_then(|value| value.parse::<u64>().ok())
        {
            config.background_carousel_minutes = value.clamp(1, 60);
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
        if let Some(value) = assignment_value(&text, "SST_FONT_COLOR") {
            config.font_color = value;
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
        if !matches!(
            config.background_mode.as_str(),
            "off" | "fixed" | "carrousel" | "carousel"
        ) {
            anyhow::bail!(
                "sstrc: SST_BACKGROUND_MODE debe ser off, fixed o carrousel"
            );
        }
        if config.background_mode == "carousel" {
            config.background_mode = "carrousel".to_owned();
        }

        Ok(config)
    }

    pub fn examples_dir(&self) -> PathBuf {
        self.root.join("examples")
    }

    pub fn tour_file(&self) -> PathBuf {
        self.root.join("tour.sh")
    }

    fn ensure_tour_assets(&self) -> Result<()> {
        let examples = self.examples_dir();
        fs::create_dir_all(&examples)?;

        // These files are user-editable scripts once they exist. Never replace
        // them on startup or during an update: an earlier implementation
        // silently destroyed user-written tour.sh scripts on every launch.
        for (name, contents) in TOUR_EXAMPLES {
            write_bundled_script_if_missing(&examples.join(name), contents)?;
        }
        write_bundled_script_if_missing(&self.tour_file(), TOUR_SCRIPT)?;
        Ok(())
    }

    pub fn background_images(&self) -> Result<Vec<PathBuf>> {
        let mut images = fs::read_dir(self.bg_dir())?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_file() && is_background_image(path))
            .collect::<Vec<_>>();
        images.sort_by(|a, b| {
            a.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .cmp(
                    &b.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase(),
                )
        });
        Ok(images)
    }

    pub fn set_config_values(&self, values: &[(&str, &str)]) -> Result<()> {
        let path = self.config_file();
        let mut text = fs::read_to_string(&path)
            .with_context(|| format!("No se pudo leer {}", path.display()))?;
        for (key, value) in values {
            set_assignment(&mut text, key, value);
        }
        fs::write(&path, text)
            .with_context(|| format!("No se pudo escribir {}", path.display()))?;
        Ok(())
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
    pub fn bg_dir(&self) -> PathBuf { self.root.join("bg") }
}

fn sync_missing_guide_assets(directory: &std::path::Path) {
    let mut network_available = true;
    let mut installed = 0usize;

    for &(filename, embedded) in GUIDE_ASSETS {
        let destination = directory.join(filename);
        if fs::metadata(&destination)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        {
            continue;
        }
        // Una descarga anterior pudo dejar un archivo vacío. Repararlo antes
        // de renombrar: en Windows el destino existente bloquea rename().
        if fs::metadata(&destination)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == 0)
        {
            let _ = fs::remove_file(&destination);
        }

        // A pinned snapshot prevents mismatched indexes and manuals. If offline,
        // fall back to the identical file embedded inside sst.exe.
        if network_available {
            if download_missing_guide(filename, embedded, &destination).is_ok() {
                installed += 1;
                continue;
            }
            network_available = false;
        }

        let _ = write_guide_atomically(&destination, embedded);
        installed += 1;
    }

    if installed > 0 {
        let _ = fs::write(directory.join(".sst-guide-snapshot"), GUIDE_SNAPSHOT_ID);
    }
}

fn download_missing_guide(
    filename: &str,
    expected: &[u8],
    destination: &std::path::Path,
) -> Result<()> {
    let url = format!(
        "https://raw.githubusercontent.com/glitchaka/sst-blueversion/{GUIDE_SNAPSHOT_COMMIT}/guide/{filename}"
    );
    let temporary = destination.with_extension(format!("sst-download-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);

    #[cfg(windows)]
    let program = crate::support::windows::system32_executable("curl.exe")?;
    #[cfg(not(windows))]
    let program = PathBuf::from("curl");

    let mut command = std::process::Command::new(program);
    command.args([
        "--location",
        "--fail",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--connect-timeout",
        "3",
        "--max-time",
        "12",
        "--retry",
        "0",
        "--output",
    ]);
    command.arg(&temporary).arg(url);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);

    let result = (|| -> Result<()> {
        let status = command.status()?;
        anyhow::ensure!(status.success(), "No se pudo descargar el tutorial {filename}");
        let downloaded = fs::read(&temporary)?;
        anyhow::ensure!(
            downloaded == expected,
            "El tutorial {filename} no coincide con la instantánea publicada"
        );
        if destination.is_file() && fs::metadata(destination)?.len() > 0 {
            return Ok(());
        }
        fs::rename(&temporary, destination)?;
        Ok(())
    })();
    // No dejar parciales ni archivos de trabajo al fallar o al competir
    // con la instalación portable.
    let _ = fs::remove_file(&temporary);
    result
}

fn write_guide_atomically(destination: &std::path::Path, contents: &[u8]) -> Result<()> {
    if destination.is_file() && fs::metadata(destination)?.len() > 0 {
        return Ok(());
    }
    let temporary = destination.with_extension(format!("sst-bundled-{}", std::process::id()));
    fs::write(&temporary, contents)?;
    let result = fs::rename(&temporary, destination)
        .with_context(|| format!("No se pudo restaurar {}", destination.display()));
    let _ = fs::remove_file(&temporary);
    result
}

fn write_bundled_script_if_missing(path: &std::path::Path, contents: &str) -> Result<()> {
    // create_new avoids a TOCTOU race and never truncates existing user files.
    match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(contents.as_bytes())
            .with_context(|| format!("No se pudo instalar {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("No se pudo crear {}", path.display())),
    }
}

#[cfg(test)]
mod tour_preservation_tests {
    use super::*;

    #[test]
    fn startup_never_overwrites_a_custom_tour_or_example() {
        let root = std::env::temp_dir().join(format!(
            "sst-tour-preservation-{}-{}", std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(root.join("examples")).unwrap();
        let user_tour = root.join("tour.sh");
        let user_example = root.join("examples/01-language-tour.sh");
        fs::write(&user_tour, "echo 'MENU PERSONALIZADO'\n").unwrap();
        fs::write(&user_example, "echo 'EJEMPLO PERSONALIZADO'\n").unwrap();
        let paths = AppPaths { root: root.clone() };
        paths.ensure_tour_assets().unwrap();
        paths.ensure_tour_assets().unwrap();
        assert_eq!(fs::read_to_string(&user_tour).unwrap(), "echo 'MENU PERSONALIZADO'\n");
        assert_eq!(fs::read_to_string(&user_example).unwrap(), "echo 'EJEMPLO PERSONALIZADO'\n");
        assert!(root.join("examples/02-windows-operator-report.sh").is_file());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn bundled_tour_is_created_only_when_absent() {
        let root = std::env::temp_dir().join(format!(
            "sst-tour-install-{}-{}", std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let paths = AppPaths { root: root.clone() };
        paths.ensure_tour_assets().unwrap();
        assert_eq!(fs::read_to_string(paths.tour_file()).unwrap(), TOUR_SCRIPT);
        fs::write(paths.tour_file(), "echo 'USER EDIT'\n").unwrap();
        paths.ensure_tour_assets().unwrap();
        assert_eq!(fs::read_to_string(paths.tour_file()).unwrap(), "echo 'USER EDIT'\n");
        fs::remove_dir_all(&root).unwrap();
    }
}

fn is_background_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| {
            matches!(
                extension.as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico" | "tif" | "tiff"
            )
        })
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
