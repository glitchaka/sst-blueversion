//! Native Windows process launching paths for the SST Bash host.
use std::{collections::HashMap, path::{Path, PathBuf}};
use anyhow::{Context, Result};

pub(super) fn resolve_program(
    name: &str,
    cwd: &Path,
    environment: &HashMap<String, String>,
) -> PathBuf {
    #[cfg(windows)]
    {
        let direct = PathBuf::from(name);
        if direct.is_absolute() {
            return direct;
        }
        if name.contains('/') || name.contains('\\') {
            return crate::support::path::resolve(cwd, name);
        }
        let extensions = if Path::new(name).extension().is_some() {
            vec![""]
        } else {
            vec!["", ".exe", ".com", ".cmd", ".bat"]
        };
        let mut directories = vec![cwd.to_path_buf()];
        if let Some(path) = environment.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value)
        {
            directories.extend(std::env::split_paths(std::ffi::OsStr::new(path)));
        }
        for directory in directories {
            for extension in &extensions {
                let executable = directory.join(format!("{name}{extension}"));
                if executable.is_file() {
                    return executable;
                }
            }
        }
        // Native Windows tools must work even when the portable PATH omits System32.
        for extension in extensions {
            if let Ok(path) = crate::support::windows::system32_executable(
                &format!("{name}{extension}")
            ) {
                return path;
            }
        }
        direct
    }
    #[cfg(not(windows))]
    {
        let _ = (cwd, environment);
        PathBuf::from(name)
    }
}

/// Avoid Win32 ERROR_PATH_NOT_FOUND (3) when the shell cwd has disappeared.
pub(super) fn working_directory(cwd: &Path) -> Result<PathBuf> {
    if cwd.is_dir() {
        return Ok(cwd.to_path_buf());
    }
    #[cfg(windows)]
    {
        let raw = cwd.to_string_lossy();
        if raw.starts_with('/') {
            let native = crate::support::path::resolve(cwd, &raw);
            if native.is_dir() {
                return Ok(native);
            }
        }
    }
    [
        std::env::current_dir().ok(),
        std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)),
        std::env::var_os("USERPROFILE").map(PathBuf::from),
        std::env::var_os("SystemRoot").map(PathBuf::from),
    ].into_iter().flatten().find(|candidate| candidate.is_dir())
        .with_context(|| format!(
            "el directorio de trabajo no existe: {}", cwd.display()
        ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn resolves_standard_windows_commands_without_path() {
        let environment = HashMap::new();
        for name in ["ping", "ipconfig", "tracert", "net", "whoami"] {
            let path = resolve_program(name, &std::env::temp_dir(), &environment);
            assert!(path.is_file(), "no se resolvió {name}: {}", path.display());
        }
    }

    #[test]
    fn recovers_missing_working_directory() {
        let nonexistent = std::env::temp_dir().join("sst-native-command-missing-cwd");
        assert!(working_directory(&nonexistent).unwrap().is_dir());
    }
}
