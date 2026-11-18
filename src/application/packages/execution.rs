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
                    match self.module_directory(&package.id) {
                        Ok(Some(_)) => {}
                        Ok(None) => continue,
                        Err(error) => {
                            eprintln!("SST: intérprete {} omitido: {error:#}", package.id);
                            continue;
                        }
                    }
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
                if routes.contains_key(&key) {
                    // Never crash SST because two installed runtimes claim
                    // the same extension; keep the first sorted package and
                    // report the conflict for explicit operator resolution.
                    eprintln!("SST: extensión {key} duplicada por paquete {}; se mantiene el intérprete anterior", package.id);
                    continue;
                }
                routes.insert(key, runner.clone());
            }
        }
        Ok(routes)
    }
}
