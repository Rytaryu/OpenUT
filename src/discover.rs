//! Shallow discovery of the VIVE Hub server log directory.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use openut_hublog::header::FileKind;
use openut_hublog::tail::parse_log_name;

const HUB_LOG_SUBDIR: &str = "HTC/ViveSoftware/ViveUltimateTracker/Log";

/// Checks the requested directory, then the known location under ProgramData.
pub fn log_dir(cli_override: Option<&Path>, configured: Option<&Path>) -> Option<PathBuf> {
    let program_data = env::var_os("ProgramData").or_else(|| env::var_os("PROGRAMDATA"));
    log_dir_under(
        cli_override,
        configured,
        program_data.as_deref().map(Path::new),
    )
}

fn log_dir_under(
    cli_override: Option<&Path>,
    configured: Option<&Path>,
    program_data: Option<&Path>,
) -> Option<PathBuf> {
    let requested = cli_override.or(configured).map(Path::to_owned);
    let known = program_data
        .filter(|root| !root.as_os_str().is_empty())
        .map(|root| root.join(HUB_LOG_SUBDIR));
    requested
        .into_iter()
        .chain(known)
        .find(|path| has_server_log(path))
}

fn has_server_log(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(parse_log_name)
            .is_some_and(|name| {
                name.kind == FileKind::Server && entry.file_type().is_ok_and(|kind| kind.is_file())
            })
    })
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::paths::TestDirectory;

    use super::*;

    #[test]
    fn discovery_prefers_config_then_cli_over_config() -> io::Result<()> {
        let directory = TestDirectory::new()?;
        let configured = directory.0.join("selected");
        let cli = directory.0.join("override");
        let known = directory.0.join(HUB_LOG_SUBDIR);
        for dir in [&configured, &cli, &known] {
            fs::create_dir_all(dir)?;
            fs::write(dir.join("viveTrackerServer_20261128.TXT"), b"")?;
        }
        assert_eq!(
            log_dir_under(None, Some(&configured), Some(&directory.0)),
            Some(configured.clone())
        );
        assert_eq!(
            log_dir_under(Some(&cli), Some(&configured), Some(&directory.0)),
            Some(cli)
        );
        Ok(())
    }

    #[test]
    fn discovery_ignores_other_files_and_uses_known_location() -> io::Result<()> {
        let directory = TestDirectory::new()?;
        let configured = directory.0.join("selected");
        fs::create_dir_all(configured.join("ViveTrackerServer_20261128.txt"))?;
        fs::write(configured.join("ViveTrackerDriver_20261128.txt"), b"")?;
        fs::write(configured.join("ViveTrackerServer_invalid.txt"), b"")?;
        fs::write(
            configured.join("ViveTrackerServer_20261128.txt/ViveTrackerServer_20261127.txt"),
            b"",
        )?;
        assert_eq!(log_dir_under(None, Some(&configured), None), None);
        let known = directory.0.join(HUB_LOG_SUBDIR);
        fs::create_dir_all(&known)?;
        fs::write(known.join("ViveTrackerServer_20261128.txt"), b"")?;
        assert_eq!(
            log_dir_under(None, Some(&configured), Some(&directory.0)),
            Some(known)
        );
        assert_eq!(log_dir_under(None, None, None), None);
        Ok(())
    }
}
