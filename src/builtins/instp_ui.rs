use std::{collections::HashSet, path::Path, time::Duration};

use anyhow::Result;

use crate::{
    application::packages::{PackageManifest, PackageService},
    core::{
        CommandOutput,
        ports::{TerminalFactory, TerminalKey, TerminalSession},
    },
};

pub fn run_catalog(
    service: &PackageService,
    terminal_factory: &dyn TerminalFactory,
) -> Result<CommandOutput> {
    let mut terminal = terminal_factory.alternate_screen()?;
    let mut catalog = service.search(None)?;
    let mut installed = installed_ids(service)?;
    let mut query = String::new();
    let mut search_mode = false;
    let mut category_index = 0usize;
    let mut selected = 0usize;
    let mut message = String::from("Selecciona un paquete para ver sus detalles.");

    loop {
        let categories = categories(&catalog);
        if category_index >= categories.len() {
            category_index = 0;
        }
        let active_category = categories
            .get(category_index)
            .map(String::as_str)
            .unwrap_or("todos");
        let visible = visible_packages(&catalog, &query, active_category);
        if visible.is_empty() {
            selected = 0;
        } else if selected >= visible.len() {
            selected = visible.len() - 1;
        }

        render(
            terminal.as_mut(),
            &visible,
            &installed,
            selected,
            &query,
            search_mode,
            active_category,
            &message,
        )?;

        let Some(key) = terminal.poll_key(Duration::from_millis(250))? else {
            continue;
        };

        if search_mode {
            match key {
                TerminalKey::Escape | TerminalKey::Enter => search_mode = false,
                TerminalKey::Backspace => {
                    query.pop();
                    selected = 0;
                }
                TerminalKey::Delete => {
                    query.clear();
                    selected = 0;
                }
                TerminalKey::Char(ch) if !ch.is_control() => {
                    query.push(ch);
                    selected = 0;
                }
                _ => {}
            }
            continue;
        }

        match key {
            TerminalKey::Escape | TerminalKey::Char('q') | TerminalKey::Char('Q') => break,
            TerminalKey::Up => selected = selected.saturating_sub(1),
            TerminalKey::Down => {
                if selected + 1 < visible.len() {
                    selected += 1;
                }
            }
            TerminalKey::PageUp => selected = selected.saturating_sub(10),
            TerminalKey::PageDown => {
                if !visible.is_empty() {
                    selected = (selected + 10).min(visible.len() - 1);
                }
            }
            TerminalKey::Home => selected = 0,
            TerminalKey::End => {
                if !visible.is_empty() {
                    selected = visible.len() - 1;
                }
            }
            TerminalKey::Char('/') => search_mode = true,
            TerminalKey::Char('c') | TerminalKey::Char('C') => {
                category_index = (category_index + 1) % categories.len().max(1);
                selected = 0;
            }
            TerminalKey::Char('i') | TerminalKey::Char('I') => {
                let Some(package) = visible.get(selected).copied() else {
                    message = "No hay un paquete seleccionado.".to_owned();
                    continue;
                };
                if installed.contains(&package.id.to_ascii_lowercase()) {
                    message = format!(
                        "{} ya está instalado. Comandos: {}",
                        package.name,
                        exposed_command_names(package)
                    );
                    continue;
                }
                if matches!(package.portability.as_str(), "system" | "driver")
                    || package.install.kind == "system"
                {
                    message = format!(
                        "{} modifica Windows y no se instala automáticamente desde instp.",
                        package.name
                    );
                    continue;
                }

                message = format!("Instalando {}...", package.name);
                render(
                    terminal.as_mut(),
                    &visible,
                    &installed,
                    selected,
                    &query,
                    search_mode,
                    active_category,
                    &message,
                )?;
                match service.install(&package.id) {
                    Ok(report) => {
                        installed.insert(report.package.id.to_ascii_lowercase());
                        let commands = if report.package.commands.is_empty() {
                            "(ninguno)".to_owned()
                        } else {
                            report
                                .package
                                .commands
                                .iter()
                                .map(|entry| entry.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        message = format!(
                            "Instalado {} {}. Comandos: {}. Reinicia SST para incorporarlos al PATH actual.",
                            report.package.name, report.package.version, commands
                        );
                        if !report.missing_exposed_commands.is_empty() {
                            message.push_str(&format!(
                                " No encontrados: {}.",
                                report.missing_exposed_commands.join(", ")
                            ));
                        }
                    }
                    Err(error) => message = format!("Error al instalar {}: {error:#}", package.id),
                }
            }
            TerminalKey::Char('r') | TerminalKey::Char('R') => {
                let Some(package) = visible.get(selected).copied() else {
                    message = "No hay un paquete seleccionado.".to_owned();
                    continue;
                };
                if !installed.contains(&package.id.to_ascii_lowercase()) {
                    message = format!("{} no está instalado.", package.name);
                    continue;
                }
                match service.remove(&package.id) {
                    Ok(true) => {
                        installed.remove(&package.id.to_ascii_lowercase());
                        message = format!("Eliminado {} de SST.", package.name);
                    }
                    Ok(false) => message = format!("{} no estaba instalado.", package.name),
                    Err(error) => message = format!("Error al eliminar {}: {error:#}", package.id),
                }
            }
            TerminalKey::Char('u') | TerminalKey::Char('U') => {
                message = "Actualizando repositorios...".to_owned();
                render(
                    terminal.as_mut(),
                    &visible,
                    &installed,
                    selected,
                    &query,
                    search_mode,
                    active_category,
                    &message,
                )?;
                match service.update_repositories() {
                    Ok(updated) => {
                        catalog = service.search(None)?;
                        message = if updated.is_empty() {
                            "No hay repositorios remotos habilitados.".to_owned()
                        } else {
                            format!("Repositorios actualizados: {}", updated.join(" · "))
                        };
                    }
                    Err(error) => message = format!("Error actualizando repositorios: {error:#}"),
                }
            }
            _ => {}
        }
    }

    Ok(CommandOutput::ok(""))
}

fn installed_ids(service: &PackageService) -> Result<HashSet<String>> {
    Ok(service
        .installed()?
        .into_iter()
        .map(|package| package.id.to_ascii_lowercase())
        .collect())
}

fn categories(catalog: &[PackageManifest]) -> Vec<String> {
    let mut rows = catalog
        .iter()
        .map(|package| package.category.to_ascii_lowercase())
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    rows.insert(0, "todos".to_owned());
    rows
}

fn visible_packages<'a>(
    catalog: &'a [PackageManifest],
    query: &str,
    category: &str,
) -> Vec<&'a PackageManifest> {
    let query = query.trim().to_ascii_lowercase();
    let mut rows = catalog
        .iter()
        .filter(|package| category == "todos" || package.category.eq_ignore_ascii_case(category))
        .filter(|package| {
            query.is_empty()
                || package.id.to_ascii_lowercase().contains(&query)
                || package.name.to_ascii_lowercase().contains(&query)
                || package.description.to_ascii_lowercase().contains(&query)
                || package.category.to_ascii_lowercase().contains(&query)
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    rows
}

fn exposed_command_names(package: &PackageManifest) -> String {
    if package.install.expose.is_empty() {
        return "(ninguno)".to_owned();
    }

    package
        .install
        .expose
        .iter()
        .map(|exposed| {
            Path::new(exposed)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or(exposed)
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render(
    terminal: &mut dyn TerminalSession,
    packages: &[&PackageManifest],
    installed: &HashSet<String>,
    selected: usize,
    query: &str,
    search_mode: bool,
    category: &str,
    message: &str,
) -> Result<()> {
    let (width, height) = terminal.size()?;
    let width = usize::from(width.max(40));
    let height = usize::from(height.max(14));

    let command_lines = packages
        .get(selected)
        .copied()
        .map(|package| wrap_text(&format!("Comandos: {}", exposed_command_names(package)), width))
        .unwrap_or_default();
    let message_lines = wrap_text(message, width);
    let fixed_rows = 9usize
        .saturating_add(command_lines.len())
        .saturating_add(message_lines.len());
    let list_rows = height.saturating_sub(fixed_rows).max(3);
    let start = if selected >= list_rows {
        selected + 1 - list_rows
    } else {
        0
    };
    let end = (start + list_rows).min(packages.len());

    terminal.clear()?;
    let mut out = String::new();
    out.push_str("INSTP · Catálogo de paquetes SST\n");
    out.push_str(&format!(
        "Categoría: {:<12}  Paquetes: {:<3}  Buscar: {}{}\n",
        category,
        packages.len(),
        if search_mode { "> " } else { "" },
        if query.is_empty() { "-" } else { query }
    ));
    out.push_str(&format!("{}\n", "─".repeat(width.min(120))));

    if packages.is_empty() {
        out.push_str("  No hay paquetes que coincidan con el filtro.\n");
        for _ in 1..list_rows {
            out.push('\n');
        }
    } else {
        for index in start..end {
            let package = packages[index];
            let mark = if index == selected { '>' } else { ' ' };
            let state = if installed.contains(&package.id.to_ascii_lowercase()) {
                "INSTALADO"
            } else if matches!(package.portability.as_str(), "system" | "driver")
                || package.install.kind == "system"
            {
                "SISTEMA"
            } else {
                "DISPONIBLE"
            };
            let line = format!(
                "{} {:<10} {:<18} {:<12} {:<10} {}",
                mark,
                state,
                clip(&package.id, 18),
                clip(&package.category, 12),
                clip(&package.portability.to_ascii_uppercase(), 10),
                package.description
            );
            out.push_str(&clip(&line, width));
            out.push('\n');
        }
        for _ in end - start..list_rows {
            out.push('\n');
        }
    }

    out.push_str(&format!("{}\n", "─".repeat(width.min(120))));
    if let Some(package) = packages.get(selected).copied() {
        out.push_str(&clip(
            &format!(
                "{} {} · {} · {}",
                package.name,
                package.version,
                package.category,
                package.portability.to_ascii_uppercase()
            ),
            width,
        ));
        out.push('\n');
        out.push_str(&clip(&package.description, width));
        out.push('\n');
        for line in &command_lines {
            out.push_str(line);
            out.push('\n');
        }
    } else {
        out.push_str("Sin selección.\n\n");
    }
    for line in &message_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&clip(
        "↑↓ navegar · / buscar · c categoría · i instalar · r quitar · u actualizar · q/Esc salir",
        width,
    ));
    out.push('\n');

    terminal.write(&out)?;
    terminal.flush()
}

fn wrap_text(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    if width == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    for source_line in value.lines() {
        let mut current = String::new();
        for word in source_line.split_whitespace() {
            let separator = usize::from(!current.is_empty());
            if current.chars().count() + separator + word.chars().count() <= width {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
                continue;
            }

            if !current.is_empty() {
                lines.push(current);
                current = String::new();
            }

            if word.chars().count() <= width {
                current.push_str(word);
            } else {
                let mut chunk = String::new();
                for ch in word.chars() {
                    if chunk.chars().count() == width {
                        lines.push(chunk);
                        chunk = String::new();
                    }
                    chunk.push(ch);
                }
                current = chunk;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        } else if source_line.is_empty() {
            lines.push(String::new());
        }
    }
    lines
}

fn clip(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut out = value.chars().take(width - 1).collect::<String>();
    out.push('…');
    out
}
