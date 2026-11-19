//! SST-PACKAGE/1 script dispatch, derived from the installed-state journal.
//! Never activate a script interpreter from a catalog entry alone.
use std::collections::HashMap;
use anyhow::Result;

use super::PackageService;

#[derive(Debug, Clone)]
pub enum ScriptRunner {
    Embedded(String),
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
                // Legacy vendor runtimes were external EXE processes. New SST
                // packages may not launch such programs as fake plugins.
                "runtime" => continue,
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
