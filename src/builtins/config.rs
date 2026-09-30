use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};

use crate::{
    adapters::persistence::AppPaths,
    core::{CommandContext, CommandOutput, ports::TextEditor},
};

use super::BuiltinCommand;

pub struct ConfigBuiltin {
    paths: AppPaths,
    config_file: PathBuf,
    editor: Arc<dyn TextEditor>,
}

impl ConfigBuiltin {
    pub fn new(paths: AppPaths, editor: Arc<dyn TextEditor>) -> Self {
        let config_file = paths.config_file();
        Self {
            paths,
            config_file,
            editor,
        }
    }

    fn backgrounds(&self, args: &[String]) -> Result<CommandOutput> {
        self.paths.ensure_layout()?;
        let images = self.paths.background_images()?;
        let action = args.first().map(String::as_str);

        match action {
            None | Some("list") => {
                let appearance = self.paths.load_appearance()?;
                let mut out = format!(
                    "Fondos SST: {}\nModo: {}",
                    self.paths.bg_dir().display(),
                    appearance.background_mode
                );
                if appearance.background_mode == "carrousel" {
                    out.push_str(&format!(
                        " · cada {} min + cambios de estado",
                        appearance.background_carousel_minutes
                    ));
                }
                out.push('\n');

                if images.is_empty() {
                    out.push_str(
                        "\n(no hay imágenes)\n\
                         Copia PNG/JPG/JPEG/WebP/BMP/GIF/ICO/TIFF en la carpeta bg.\n",
                    );
                } else {
                    out.push_str("\n");
                    for (index, path) in images.iter().enumerate() {
                        let name = path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("?");
                        let configured = PathBuf::from(&appearance.background_image);
                        let active = configured
                            .file_name()
                            .and_then(|value| value.to_str())
                            .is_some_and(|value| value.eq_ignore_ascii_case(name));
                        out.push_str(&format!(
                            "{:>2}. {}{}\n",
                            index + 1,
                            name,
                            if active { "  [seleccionado]" } else { "" }
                        ));
                    }
                }

                out.push_str(
                    "\nUso:\n\
                       config bg NOMBRE|NUMERO\n\
                       config bg carrousel [MINUTOS]\n\
                       config bg next\n\
                       config bg off\n",
                );
                Ok(CommandOutput::ok(out))
            }
            Some("off") | Some("none") => {
                self.paths.set_config_values(&[
                    ("SST_BACKGROUND_MODE", "off"),
                    ("SST_BACKGROUND_IMAGE", ""),
                ])?;
                Ok(CommandOutput::ok("Fondo desactivado.\n"))
            }
            Some("carrousel") | Some("carousel") => {
                if images.is_empty() {
                    return Ok(CommandOutput::error(
                        format!(
                            "config bg carrousel: no hay imágenes en {}",
                            self.paths.bg_dir().display()
                        ),
                        1,
                    ));
                }

                let minutes = args
                    .get(1)
                    .map(|value| value.parse::<u64>())
                    .transpose()
                    .context("config bg carrousel: MINUTOS debe ser un entero")?
                    .unwrap_or(3);
                if !(1..=60).contains(&minutes) {
                    return Ok(CommandOutput::error(
                        "config bg carrousel: MINUTOS debe estar entre 1 y 60",
                        2,
                    ));
                }

                let current = self.paths.load_appearance()?.background_image;
                let selected = images
                    .iter()
                    .find(|path| {
                        PathBuf::from(&current)
                            .file_name()
                            .and_then(|value| value.to_str())
                            .zip(path.file_name().and_then(|value| value.to_str()))
                            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
                    })
                    .unwrap_or(&images[0]);
                let relative = format!(
                    "bg/{}",
                    selected
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                );

                self.paths.set_config_values(&[
                    ("SST_BACKGROUND_MODE", "carrousel"),
                    ("SST_BACKGROUND_CAROUSEL_MINUTES", &minutes.to_string()),
                    ("SST_BACKGROUND_IMAGE", &relative),
                ])?;

                Ok(CommandOutput::ok(format!(
                    "Carrusel activado: {} imágenes · {} min · rotación por cambios de estado.\n",
                    images.len(),
                    minutes
                )))
            }
            Some("next") => {
                if images.is_empty() {
                    return Ok(CommandOutput::error(
                        format!("config bg next: no hay imágenes en {}", self.paths.bg_dir().display()),
                        1,
                    ));
                }
                let appearance = self.paths.load_appearance()?;
                let current_name = PathBuf::from(&appearance.background_image)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_owned);
                let current_index = current_name
                    .as_deref()
                    .and_then(|name| {
                        images.iter().position(|path| {
                            path.file_name()
                                .and_then(|value| value.to_str())
                                .is_some_and(|value| value.eq_ignore_ascii_case(name))
                        })
                    })
                    .unwrap_or(images.len().saturating_sub(1));
                let next = &images[(current_index + 1) % images.len()];
                let name = next.file_name().and_then(|value| value.to_str()).unwrap_or_default();
                let relative = format!("bg/{name}");
                self.paths.set_config_values(&[
                    ("SST_BACKGROUND_MODE", "fixed"),
                    ("SST_BACKGROUND_IMAGE", &relative),
                ])?;
                Ok(CommandOutput::ok(format!("Fondo: {name}\n")))
            }
            Some(selector) => {
                if images.is_empty() {
                    return Ok(CommandOutput::error(
                        format!("config bg: no hay imágenes en {}", self.paths.bg_dir().display()),
                        1,
                    ));
                }

                let selected = if let Ok(number) = selector.parse::<usize>() {
                    number
                        .checked_sub(1)
                        .and_then(|index| images.get(index))
                } else {
                    images.iter().find(|path| {
                        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default();
                        let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
                        name.eq_ignore_ascii_case(selector) || stem.eq_ignore_ascii_case(selector)
                    })
                };

                let Some(selected) = selected else {
                    return Ok(CommandOutput::error(
                        format!("config bg: no existe '{selector}'; usa 'config bg' para listar"),
                        1,
                    ));
                };

                let name = selected
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                let relative = format!("bg/{name}");
                self.paths.set_config_values(&[
                    ("SST_BACKGROUND_MODE", "fixed"),
                    ("SST_BACKGROUND_IMAGE", &relative),
                ])?;
                Ok(CommandOutput::ok(format!("Fondo fijo: {name}\n")))
            }
        }
    }
}

impl BuiltinCommand for ConfigBuiltin {
    fn name(&self) -> &'static str {
        "sst-config"
    }

    fn help(&self) -> &'static str {
        "sst-config path|edit|bg — configuración portable de SST"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("path") {
            "path" => Ok(CommandOutput::ok(format!("{}\n", self.config_file.display()))),
            "edit" => {
                let args = vec![self.config_file.to_string_lossy().into_owned()];
                let status = self.editor.edit(&args, context.cwd)?;
                Ok(CommandOutput { status, stdout: String::new(), stderr: String::new() })
            }
            "bg" => self.backgrounds(args.get(1..).unwrap_or_default()),
            "reload" => Ok(CommandOutput::error(
                "config reload debe ejecutarse mediante la función Bash 'config'",
                2,
            )),
            other => Ok(CommandOutput::error(
                format!("config: subcomando desconocido: {other}"),
                2,
            )),
        }
    }
}

pub struct PathBuiltin;

impl BuiltinCommand for PathBuiltin {
    fn name(&self) -> &'static str {
        "sst-path"
    }

    fn help(&self) -> &'static str {
        "sst-path PATH — traduce rutas estilo /c/... a rutas Windows"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        let Some(raw) = args.first() else {
            return Ok(CommandOutput::error("sst-path: falta ruta", 2));
        };

        let path = crate::support::path::resolve(context.cwd, raw);
        Ok(CommandOutput::ok(format!("{}\n", path.display())))
    }
}
