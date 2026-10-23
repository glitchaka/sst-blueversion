use anyhow::Result;

use super::beacon_overlay::BeaconOverlayGuard;

pub fn run() -> Result<()> {
    let _beacon_overlay = BeaconOverlayGuard::start();
    super::gui_slint_base::run()
}
