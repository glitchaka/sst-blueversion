use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::adapters::persistence::AppPaths;

use super::model::{InstalledState, RepoConfig, RepoSource, sanitize_file_name};

const DEFAULT_REPO_URL: &str =
    "https://raw.githubusercontent.com/glitchaka/sst-blueversion/main/packages/index.json";

#[derive(Clone)]
pub(super) struct PackageStore {
    paths: AppPaths,
}

impl PackageStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for path in [
            self.packages_dir(),
            self.cache_dir(),
            self.tools_dir(),
            self.runtimes_dir(),
            self.repositories_dir(),
            self.ecosystems_dir(),
            self.bin_dir(),
        ] {
            fs::create_dir_all(path)?;
        }

        if !self.repos_file().exists() {
            self.save_repo_config(&RepoConfig {
                repositories: vec![RepoSource {
                    name: "sst-main".to_owned(),
                    url: DEFAULT_REPO_URL.to_owned(),
                    priority: 100,
                    enabled: true,
                }],
            })?;
        }
        if !self.installed_file().exists() {
            self.save_installed_state(&InstalledState::default())?;
        }
        Ok(())
    }

    pub fn root_dir(&self) -> PathBuf {
        self.paths.root_dir()
    }

    pub fn packages_dir(&self) -> PathBuf {
        self.paths.root_dir().join("packages")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.packages_dir().join("cache")
    }

    pub fn tools_dir(&self) -> PathBuf {
        self.packages_dir().join("tools")
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.packages_dir().join("runtimes")
    }

    pub fn repositories_dir(&self) -> PathBuf {
        self.packages_dir().join("repositories")
    }

    pub fn ecosystems_dir(&self) -> PathBuf {
        self.packages_dir().join("ecosystems")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.packages_dir().join("bin")
    }

    pub fn repository_cache_file(&self, name: &str) -> PathBuf {
        self.repositories_dir()
            .join(format!("{}.json", sanitize_file_name(name)))
    }

    pub fn load_repo_config(&self) -> Result<RepoConfig> {
        let bytes = fs::read(self.repos_file())?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save_repo_config(&self, config: &RepoConfig) -> Result<()> {
        fs::write(self.repos_file(), serde_json::to_vec_pretty(config)?)?;
        Ok(())
    }

    pub fn load_installed_state(&self) -> Result<InstalledState> {
        let bytes = fs::read(self.installed_file())?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save_installed_state(&self, state: &InstalledState) -> Result<()> {
        fs::write(self.installed_file(), serde_json::to_vec_pretty(state)?)?;
        Ok(())
    }

    pub fn root_relative(&self, path: &Path) -> String {
        path.strip_prefix(self.paths.root_dir())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    pub fn resolve_root_relative(&self, value: &str) -> PathBuf {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            self.paths.root_dir().join(path)
        }
    }

    /// Purge downloaded archives for a temporary package when its functional
    /// generation expires. Do not touch other packages' cache entries.
    pub fn clean_package_cache(&self, id: &str) -> Result<usize> {
        self.ensure_layout()?;
        // Double underscores delimit the package ID unambiguously.
        // "foo" must never delete cached archives belonging to "foo-bar".
        let prefix = format!("{id}__");
        let mut removed = 0usize;
        for item in fs::read_dir(self.cache_dir())? {
            let item = item?;
            if item.path().is_file()
                && item.file_name().to_string_lossy().starts_with(&prefix)
            {
                fs::remove_file(item.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn clean_cache(&self) -> Result<usize> {
        self.ensure_layout()?;
        let mut removed = 0usize;
        for entry in fs::read_dir(self.cache_dir())? {
            let entry = entry?;
            if entry.path().is_file() {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn installed_file(&self) -> PathBuf {
        self.packages_dir().join("installed.json")
    }

    fn repos_file(&self) -> PathBuf {
        self.packages_dir().join("repos.json")
    }
}
