use anyhow::Result;

pub struct RawModeGuard;

impl RawModeGuard {
    pub fn enter() -> Result<Self> {
        super::io::enter_raw()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        super::io::leave_raw();
    }
}
