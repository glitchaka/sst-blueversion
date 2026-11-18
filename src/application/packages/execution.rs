//! SST-PACKAGE/1 script dispatch, derived from the installed-state journal.
//! Never activate a script interpreter from a catalog entry alone.
use std::{collections::HashMap, path::PathBuf};
use anyhow::Result;

use super::{PackageService, installer::find_file};

#[derive(Debug, Clone)]
pub enum ScriptRunner {
    Embedded(String),
    Process(PathBuf),
}

impl PackageService {
    pub fn script_routes(&self) -> Result<HashMap<String, ScriptRunner>> {
        let mut routes = HashMap::new();
        for package in self.installed()? {
            let Some(execution) = &package.execution else { continue; };
            if execution.extensions.is_empty() { continue; }
            let runner = match execution.kind.as_str() {
                "embedded" => {
                    if self.module_directory(&package.id)?.is_none() { continue; }
                    let Some(command) = execution.commands.first() else { continue; };
                    ScriptRunner::Embedded(command.clone())
                }
                "runtime" => {
                    let root = self.store.runtimes_dir().canonicalize()?;
                    let installed = self.store.resolve_root_relative(&package.install_dir);
                    if !installed.is_dir() { continue; }
                    let installed = installed.canonicalize()?;
                    if !installed.starts_with(&root) { continue; }
                    let Some(entry) = find_file(&installed, &execution.entry)? else { continue; };
                    ScriptRunner::Process(entry)
                }
                _ => continue,
            };
            for extension in &execution.extensions {
                let key = extension.to_ascii_lowercase();
                if routes.insert(key.clone(), runner.clone()).is_some() {
                    anyhow::bail!("instp: más de un intérprete activo para {key}");
                }
            }
        }
        Ok(routes)
    }
}
