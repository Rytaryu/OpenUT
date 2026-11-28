//! Desktop process-name policy for the read-only recorder.

use std::ffi::{OsStr, OsString};

const TRACKER_SERVER: &str = "ViveTrackerServer.exe";

#[derive(Default)]
pub struct ServerGuard {
    extra_names: Vec<OsString>,
}

impl ServerGuard {
    /// Accept additional executable names without replacing the default.
    pub fn new(extra_names: impl IntoIterator<Item = OsString>) -> Self {
        Self {
            extra_names: extra_names.into_iter().collect(),
        }
    }

    pub fn server_running<'a>(&self, process_names: impl IntoIterator<Item = &'a OsStr>) -> bool {
        process_names.into_iter().any(|name| {
            name.eq_ignore_ascii_case(OsStr::new(TRACKER_SERVER))
                || self
                    .extra_names
                    .iter()
                    .any(|extra| !extra.is_empty() && name.eq_ignore_ascii_case(extra.as_os_str()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_whole_executable_names_without_ascii_case_sensitivity() {
        let guard = ServerGuard::default();
        assert!(
            guard.server_running([OsStr::new("other.exe"), OsStr::new("VIVETRACKERSERVER.EXE")])
        );
        assert!(!guard.server_running([OsStr::new("ViveTrackerServer.exe.old")]));
        assert!(!guard.server_running([]));
    }

    #[test]
    fn additional_executables_extend_the_default() {
        let guard = ServerGuard::new([OsString::from("TrackerService.exe"), OsString::new()]);
        for name in ["trackerservice.EXE", "ViveTrackerServer.exe"] {
            assert!(guard.server_running([OsStr::new(name)]));
        }
        assert!(!guard.server_running([OsStr::new("")]));
    }
}
