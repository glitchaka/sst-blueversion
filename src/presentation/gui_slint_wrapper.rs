use anyhow::Result;

use crate::{adapters::persistence::AppPaths, application::update};

use super::beacon_overlay::BeaconOverlayGuard;

pub fn run() -> Result<()> {
    let paths = AppPaths::detect();
    update::spawn_background_check(paths.clone());

    let _beacon_overlay = BeaconOverlayGuard::start();
    let result = super::gui_slint_base::run();

    // A staged update is only handed off after the GUI has closed. The helper
    // waits for this process to release sst.exe, replaces it and relaunches SST.
    if result.is_ok() {
        let _ = update::schedule_pending_update(&paths);
    }
    result
}
