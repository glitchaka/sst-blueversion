// Edit the literal values in-place. Keep shell syntax that the form does not own.
use anyhow::{bail, ensure, Context, Result};
use regex::Regex;
use std::{fs, io::Write, ops::Range, path::{Path, PathBuf}};

pub(super) struct Field {
    pub label: String,
    pub value: String,
    pub choices: Vec<String>,
    pub secret: bool,
    key: String,
    bytes: Range<usize>,
}

pub(super) struct Document {
    path: PathBuf,
    original: String,
    pub fields: Vec<Field>,
}

fn label(key: &str) -> &str {
    match key {
        "SST_BACKDROP" => "Efecto de ventana",
        "SST_FOCUSED_OPACITY" => "Opacidad con foco (%)",
        "SST_UNFOCUSED_OPACITY" => "Opacidad sin foco (%)",
        "SST_BACKGROUND_COLOR" => "Color de fondo",
        "SST_BACKGROUND_IMAGE" => "Archivo de fondo",
        "SST_BACKGROUND_IMAGE_OPACITY" => "Opacidad de imagen (%)",
        "SST_BACKGROUND_MODE" => "Modo de fondo",
        "SST_BACKGROUND_CAROUSEL_MINUTES" => "Carrusel (minutos)",
        "SST_BACKGROUND_IMAGE_FIT" => "Ajuste de imagen",
        "SST_CORNER_RADIUS" => "Radio de esquinas",
        "SST_CONTENT_TOP_GAP" => "Margen superior",
        "SST_FONT_SIZE" => "Tamaño de letra",
        "SST_FONT_COLOR" => "Color del texto",
        "SST_FONT_PALETTE" => "Paleta",
        "SST_CELL_WIDTH" => "Ancho de celda",
        "SST_CELL_HEIGHT" => "Alto de celda",
        "SST_TERMINAL_PADDING_X" => "Margen horizontal",
        "SST_TERMINAL_PADDING_Y" => "Margen vertical",
        _ => key,
    }
}

fn choices(key: &str) -> Vec<String> {
    let options: &[&str] = match key {
        "SST_BACKDROP" => &["acrylic", "blur", "glass", "solid"],
        "SST_BACKGROUND_MODE" => &["off", "fixed", "carrousel"],
        "SST_BACKGROUND_IMAGE_FIT" => &["cover", "contain", "fill", "preserve"],
        "SST_FONT_PALETTE" => return crate::core::palettes::names().map(str::to_owned).collect(),
        _ => &[],
    };
    options.iter().map(|s| (*s).to_owned()).collect()
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let original = fs::read_to_string(path).with_context(|| format!("Leer {}", path.display()))?;
        // Only complete single-line literal assignments. Spans exclude inline comments.
        let pattern = Regex::new(r#"^[ \t]*(?:(alias|export)[ \t]+)?([A-Za-z_][A-Za-z_0-9]*)=('(?:[^']|'\\'')*'|"[^"\\$`]*"|[a-zA-Z0-9_./:%+@=-]*)(?:[ \t]+#.*|[ \t]*)$"#)?;
        let mut fields = Vec::new();
        let mut offset = 0;
        for line in original.split_inclusive('\n') {
            if let Some(c) = pattern.captures(line.trim_end_matches(['\n', '\r'])) {
                let key = &c[2];
                let raw = c.get(3).expect("value capture");
                let value = raw.as_str();
                let value = if value.starts_with(['\'', '"']) {
                    value[1..value.len()-1].replace("'\\''", "'")
                } else { value.to_owned() };
                let alias = c.get(1).is_some_and(|m| m.as_str()=="alias");
                let mut options = if alias { vec![] } else { choices(key) };
                if !options.is_empty() && !options.contains(&value) { options.push(value.clone()); }
                let upper = key.to_ascii_uppercase();
                fields.push(Field {
                    label: if alias { format!("Alias · {key}") } else { label(key).to_owned() },
                    key: if alias { String::new() } else { key.to_owned() },
                    secret: ["KEY","TOKEN","PASSWORD","SECRET"].iter().any(|part| upper.contains(*part)),
                    value, choices: options, bytes: offset+raw.start()..offset+raw.end(),
                });
            }
            offset += line.len();
        }
        Ok(Self { path: path.to_owned(), original, fields })
    }

    pub fn save(&self, draft: &[String]) -> Result<()> {
        let mut text = self.original.clone();
        for (field,value) in self.fields.iter().zip(draft).rev() {
            if value == &field.value { continue; }
            ensure!(!value.contains(['\r','\n','\0']), "Valor inválido en {}", field.label);
            let range = match field.key.as_str() {
                "SST_FOCUSED_OPACITY" | "SST_UNFOCUSED_OPACITY" | "SST_BACKGROUND_IMAGE_OPACITY" => Some(0..=100),
                "SST_BACKGROUND_CAROUSEL_MINUTES" => Some(1..=60),
                "SST_CORNER_RADIUS" | "SST_TERMINAL_PADDING_X" | "SST_TERMINAL_PADDING_Y" => Some(0..=64),
                "SST_CONTENT_TOP_GAP" => Some(0..=96),
                "SST_FONT_SIZE" => Some(8..=32),
                "SST_CELL_WIDTH" => Some(5..=32),
                "SST_CELL_HEIGHT" => Some(10..=48),
                _ => None,
            };
            if let Some(range) = range {
                ensure!(value.parse::<u16>().is_ok_and(|n|range.contains(&n)), "{} admite {} a {}.",field.label,range.start(),range.end());
            }
            if matches!(field.key.as_str(),"SST_FONT_COLOR"|"SST_BACKGROUND_COLOR") {
                ensure!(value.len()==7 && value.starts_with('#') && value[1..].bytes().all(|b|b.is_ascii_hexdigit()), "{} requiere #RRGGBB.",field.label);
            }
            text.replace_range(field.bytes.clone(), &format!("'{}'",value.replace('\'', "'\\''")));
        }
        if text == self.original { return Ok(()); }
        let mut temp = tempfile::NamedTempFile::new_in(self.path.parent().context("Ruta de sstrc inválida")?)?;
        temp.as_file().set_permissions(fs::metadata(&self.path)?.permissions())?;
        temp.write_all(text.as_bytes())?;
        temp.as_file().sync_all()?;
        if fs::read_to_string(&self.path)? != self.original { bail!("sstrc cambió mientras editabas. Cancela y vuelve a abrir config."); }
        temp.persist(&self.path)?;
        Ok(())
    }
}
