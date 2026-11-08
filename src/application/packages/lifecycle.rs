//! Lifetimes of portable packages managed by instp.
//!
//! "Blue" names the IT edition of SST; it is NOT the package retention
//! boundary. The feature generation must change only for a functional
//! release that intentionally expires temporary packages.
use super::model::InstalledPackage;

/// Update this ONLY when shipping a new functional SST generation.
/// Keep the same value for rebuilds, security fixes, and blue.N revisions.
pub const FUNCTIONAL_GENERATION: &str = "0.1.9";

pub fn expires_on_generation(package: &InstalledPackage, current: &str) -> bool {
    !package.persistent
        && !package.functional_generation.is_empty()
        && package.functional_generation != current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::packages::model::InstalledCommand;

    fn package(persistent: bool, generation: &str) -> InstalledPackage {
        InstalledPackage {
            id: "example".into(),
            name: "Example".into(),
            version: "1".into(),
            category: "cli".into(),
            portability: "pure".into(),
            install_dir: "packages/tools/example".into(),
            commands: vec![InstalledCommand {
                name: "example".into(),
                target: "packages/tools/example/example.exe".into(),
            }],
            path_entries: vec![],
            persistent,
            functional_generation: generation.into(),
        }
    }

    #[test]
    fn temporary_package_expires_on_functional_release() {
        assert!(expires_on_generation(&package(false, "0.1.9"), "0.1.10"));
        assert!(!expires_on_generation(&package(false, "0.1.9"), "0.1.9"));
    }

    #[test]
    fn persistent_package_survives_any_functional_release() {
        assert!(!expires_on_generation(&package(true, "0.1.9"), "0.2.0"));
    }

    #[test]
    fn preexisting_installation_is_migrated_not_automatically_deleted() {
        assert!(!expires_on_generation(&package(false, ""), "0.1.9"));
    }
}
