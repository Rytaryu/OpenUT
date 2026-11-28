//! Read the executable names needed by the watcher's Hub gate.

use std::ffi::OsStr;

#[derive(Default)]
pub struct HubProcess;

impl HubProcess {
    /// `None` means the process list could not be checked on this platform or in this poll.
    #[cfg(windows)]
    pub fn check(&mut self, names: &[String]) -> Option<bool> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

        // Take a fresh snapshot so a reused PID cannot inherit a cached name.
        let mut system = System::new().ok()?;
        let refreshed = system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        // Windows always has processes. A zero count indicates a failed enumeration.
        (refreshed != 0).then(|| {
            any_process_matches(
                system.processes().values().map(|process| process.name()),
                names,
            )
        })
    }

    #[cfg(not(windows))]
    pub fn check(&mut self, _names: &[String]) -> Option<bool> {
        None
    }
}

/// Compare whole executable basenames without ASCII case sensitivity.
pub fn any_process_matches<'a>(
    process_names: impl IntoIterator<Item = &'a OsStr>,
    configured_names: &[String],
) -> bool {
    process_names.into_iter().any(|process| {
        configured_names
            .iter()
            .any(|name| !name.is_empty() && process.eq_ignore_ascii_case(OsStr::new(name)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_complete_configured_basenames() {
        let names = ["ViveTrackerServer.exe".into(), "TrackerService.exe".into()];
        for name in ["VIVETRACKERSERVER.EXE", "trackerservice.Exe"] {
            assert!(any_process_matches(
                [OsStr::new("other.exe"), OsStr::new(name)],
                &names
            ));
        }
        for name in [
            "ViveTrackerServer.exe.old",
            r"C:\Apps\ViveTrackerServer.exe",
            "",
        ] {
            assert!(!any_process_matches([OsStr::new(name)], &names));
        }
        assert!(!any_process_matches([], &names));
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_platform_does_not_report_absence() {
        assert_eq!(HubProcess.check(&["ViveTrackerServer.exe".into()]), None);
    }
}
