//! Paths for watcher settings, summaries, and the application log.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub config: Option<PathBuf>,
    pub summary_dir: Option<PathBuf>,
    pub app_log_dir: Option<PathBuf>,
}

impl AppPaths {
    pub fn resolve(config_override: Option<&Path>, data_override: Option<&Path>) -> Self {
        let app_data = env::var_os("APPDATA");
        let local_data = env::var_os("LOCALAPPDATA");
        Self::from_roots(
            config_override,
            data_override,
            app_data.as_deref().map(Path::new),
            local_data.as_deref().map(Path::new),
        )
    }

    fn from_roots(
        config_override: Option<&Path>,
        data_override: Option<&Path>,
        app_data: Option<&Path>,
        local_data: Option<&Path>,
    ) -> Self {
        let config = config_override
            .map(Path::to_owned)
            .or_else(|| app_root(data_override, app_data).map(|root| root.join("config.toml")));
        let data = app_root(data_override, local_data);
        Self {
            config,
            summary_dir: data.as_ref().map(|root| root.join("summary")),
            app_log_dir: data.as_ref().map(|root| root.join("logs")),
        }
    }
}

fn app_root(override_path: Option<&Path>, env_path: Option<&Path>) -> Option<PathBuf> {
    override_path.map(Path::to_owned).or_else(|| {
        env_path
            .filter(|root| !root.as_os_str().is_empty())
            .map(|root| root.join("OpenUT"))
    })
}

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

/// Writes beside the destination so a failed replacement leaves the old file intact.
pub(crate) fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid output path"))?;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory)?;
    let (temporary, mut file) = loop {
        let index = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let mut candidate = name.to_os_string();
        candidate.push(format!(".tmp-{}-{index}", std::process::id()));
        let temporary = directory.join(candidate);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    drop(file);
    let replaced = written.and_then(|()| fs::rename(&temporary, path));
    if replaced.is_err() {
        let _ = fs::remove_file(temporary);
    }
    replaced
}

#[cfg(test)]
pub(crate) struct TestDirectory(pub(crate) PathBuf);

#[cfg(test)]
impl TestDirectory {
    pub(crate) fn new() -> io::Result<Self> {
        loop {
            let index = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
            let name = format!("openut-watch-test-{}-{index}", std::process::id());
            let path = env::temp_dir().join(name);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(test)]
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn honors_each_environment_root_and_explicit_overrides() {
        let roaming = Some(Path::new("roaming"));
        let local = Some(Path::new("local"));
        let defaults = AppPaths::from_roots(None, None, roaming, local);
        assert_eq!(
            defaults.config,
            Some(Path::new("roaming").join("OpenUT/config.toml"))
        );
        assert_eq!(
            defaults.summary_dir,
            Some(Path::new("local").join("OpenUT/summary"))
        );
        assert_eq!(
            defaults.app_log_dir,
            Some(Path::new("local").join("OpenUT/logs"))
        );

        let data = Some(Path::new("data"));
        let config = Some(Path::new("custom.toml"));
        let overrides = AppPaths::from_roots(config, data, roaming, local);
        assert_eq!(overrides.config.as_deref(), config);
        assert_eq!(
            overrides.summary_dir,
            Some(Path::new("data").join("summary"))
        );
        assert_eq!(overrides.app_log_dir, Some(Path::new("data").join("logs")));
        assert_eq!(
            AppPaths::from_roots(None, data, None, None).config,
            Some(Path::new("data").join("config.toml"))
        );
    }

    #[test]
    fn missing_or_empty_environment_roots_do_not_infer_local_paths() {
        assert_eq!(
            AppPaths::from_roots(None, None, Some(Path::new("")), None),
            AppPaths {
                config: None,
                summary_dir: None,
                app_log_dir: None
            }
        );
    }
}
