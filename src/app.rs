use anyhow::Result;

use crate::{composition, presentation::shell::ShellSession};

pub struct ShellShockTool {
    shell: ShellSession,
}

impl ShellShockTool {
    pub fn new() -> Result<Self> {
        // update.rs still exposes these compatibility entry points while the GUI
        // uses the hardened startup/swap path. Keep them referenced until the
        // legacy API is removed so deny(warnings) does not turn the migration
        // into a release-blocking dead_code error.
        let _legacy_background_check: fn(crate::adapters::persistence::AppPaths) =
            crate::application::update::spawn_background_check;
        let _legacy_check_and_stage: fn(
            &crate::adapters::persistence::AppPaths,
        ) -> Result<bool> = crate::application::update::check_and_stage;

        Ok(Self {
            shell: composition::build_shell()?,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        self.shell.run()
    }
}
