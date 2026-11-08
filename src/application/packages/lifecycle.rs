//! Lifetimes of portable packages managed by instp.
//!
//! "Blue" names the IT edition of SST; it is NOT the package retention
//! boundary. The feature generation must change only for a functional
//! release that intentionally expires temporary packages.
use super::model::InstalledPackage;

/// The edition suffix (e.g. "-blue.27") identifies Blue and its
/// corrections. The functional version is the numeric SST base before it.
/// Derive it from the existing canonical version; never invent a second
/// manually maintained version counter.
pub fn functional_generation() -> &'static str {
    functional_version(env!("CARGO_PKG_VERSION"))
}

fn functional_version(version: &str) -> &str {
    version.split_once('-').map_or(version, |(base, _)| base)
}

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
    fn blue_editions_keep_correct_canonical_version_boundaries() {
        assert_eq!(functional_version("0.1.9-blue.27"), "0.1.9");
        assert_eq!(functional_version("0.1.9-blue.28"), "0.1.9");
        assert_eq!(functional_version("0.1.10-blue.0"), "0.1.10");
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
