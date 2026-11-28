//! Registers the packaged OpenUT driver with SteamVR's own tool.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverAction {
    Register,
    Unregister,
}

impl DriverAction {
    fn argument(self) -> &'static str {
        match self {
            Self::Register => "adddriver",
            Self::Unregister => "removedriver",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    UnsupportedPlatform,
    MissingLocalAppData,
    ReadRegistry(io::ErrorKind),
    InvalidRegistry,
    MissingRuntime,
    MissingUtility,
    FindExecutable(io::ErrorKind),
    StartUtility(io::ErrorKind),
    UtilityExit(Option<i32>),
}

#[derive(Debug, Eq, PartialEq)]
pub struct DriverError {
    failure: Failure,
    manual: Option<DriverAction>,
}

impl From<Failure> for DriverError {
    fn from(failure: Failure) -> Self {
        Self {
            failure,
            manual: None,
        }
    }
}

impl DriverError {
    fn with_manual(mut self, action: DriverAction) -> Self {
        self.manual = Some(action);
        self
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.failure {
            Failure::UnsupportedPlatform => {
                write!(f, "ドライバーの登録・解除は Windows で実行してください。")
            }
            Failure::MissingLocalAppData => write!(f, "LOCALAPPDATA を取得できませんでした。"),
            Failure::ReadRegistry(kind) => write!(
                f,
                "SteamVR の openvrpaths.vrpath を読めませんでした（{kind}）。"
            ),
            Failure::InvalidRegistry => write!(
                f,
                "SteamVR の openvrpaths.vrpath を JSON として読めませんでした。"
            ),
            Failure::MissingRuntime => write!(
                f,
                "SteamVR の openvrpaths.vrpath に有効な runtime がありません。"
            ),
            Failure::MissingUtility => {
                write!(f, "SteamVR の vrpathreg.exe が見つかりませんでした。")
            }
            Failure::FindExecutable(kind) => write!(
                f,
                "OpenUT の実行ファイルの場所を取得できませんでした（{kind}）。"
            ),
            Failure::StartUtility(kind) => write!(
                f,
                "SteamVR の vrpathreg.exe を起動できませんでした（{kind}）。"
            ),
            Failure::UtilityExit(Some(code)) => write!(
                f,
                "SteamVR の vrpathreg.exe が終了コード {code} を返しました。"
            ),
            Failure::UtilityExit(None) => {
                write!(f, "SteamVR の vrpathreg.exe が途中で終了しました。")
            }
        }?;
        if let Some(action) = self.manual {
            write!(
                f,
                "\nフォルダーの場所を置き換えて PowerShell で実行してください。\n& \"<SteamVR のフォルダー>\\bin\\win64\\vrpathreg.exe\" {} \"<openut.exe のフォルダー>\\openut\"",
                action.argument(),
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for DriverError {}

/// Resolves the utility path using only the first runtime entry in the registry.
pub fn find_vrpathreg(registry: &Path) -> Result<PathBuf, DriverError> {
    let bytes = fs::read(registry).map_err(|error| Failure::ReadRegistry(error.kind()))?;
    let registry: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| Failure::InvalidRegistry)?;
    let runtime = registry
        .get("runtime")
        .and_then(serde_json::Value::as_array)
        .and_then(|entries| entries.first())
        .and_then(serde_json::Value::as_str)
        .filter(|entry| !entry.contains('\0'))
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .ok_or(Failure::MissingRuntime)?;
    Ok(runtime.join("bin").join("win64").join("vrpathreg.exe"))
}

/// Runs SteamVR's registration or removal command for the driver beside this executable.
pub fn run(action: DriverAction) -> Result<(), DriverError> {
    if !cfg!(windows) {
        return Err(Failure::UnsupportedPlatform.into());
    }
    let registry = std::env::var_os("LOCALAPPDATA")
        .filter(|directory| !directory.is_empty())
        .map(PathBuf::from)
        .map(|directory| directory.join("openvr").join("openvrpaths.vrpath"))
        .ok_or_else(|| DriverError::from(Failure::MissingLocalAppData).with_manual(action))?;
    let utility = find_vrpathreg(&registry).map_err(|error| error.with_manual(action))?;
    if !utility.is_file() {
        return Err(DriverError::from(Failure::MissingUtility).with_manual(action));
    }
    let executable =
        std::env::current_exe().map_err(|error| Failure::FindExecutable(error.kind()))?;
    let directory = executable
        .parent()
        .ok_or(Failure::FindExecutable(io::ErrorKind::NotFound))?
        .join("openut");
    let status = Command::new(utility)
        .arg(action.argument())
        .arg(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            DriverError::from(Failure::StartUtility(error.kind())).with_manual(action)
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Failure::UtilityExit(status.code()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RegistryFixture(PathBuf);

    impl RegistryFixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};

            static NEXT_ID: AtomicU32 = AtomicU32::new(0);
            let directory = std::env::temp_dir().join(format!(
                "openut-vrpath-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory)
        }

        fn write(&self, source: impl AsRef<[u8]>) -> PathBuf {
            let registry = self.0.join("openvrpaths.vrpath");
            fs::write(&registry, source).unwrap();
            registry
        }
    }

    impl Drop for RegistryFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn uses_the_first_runtime_and_keeps_spaces_in_the_path() {
        let fixture = RegistryFixture::new();
        let runtime = fixture.0.join("SteamVR with spaces");
        let other = fixture.0.join("Previous SteamVR installation");
        let registry = fixture.write(
            serde_json::json!({ "runtime": [&runtime, &other], "external_drivers": [] })
                .to_string(),
        );
        assert_eq!(
            find_vrpathreg(&registry).unwrap(),
            runtime.join("bin/win64/vrpathreg.exe")
        );
    }

    #[test]
    fn rejects_missing_or_invalid_runtime_and_malformed_json() {
        let fixture = RegistryFixture::new();
        for source in [
            "{}",
            r#"{"runtime":[]}"#,
            r#"{"runtime":null}"#,
            r#"{"runtime":"wrong shape"}"#,
            r#"{"runtime":[null]}"#,
            r#"{"runtime":[42]}"#,
            r#"{"runtime":[""]}"#,
            r#"{"runtime":["relative SteamVR"]}"#,
        ] {
            let registry = fixture.write(source);
            assert_eq!(
                find_vrpathreg(&registry).unwrap_err().failure,
                Failure::MissingRuntime
            );
        }
        let registry = fixture.write("not json");
        assert_eq!(
            find_vrpathreg(&registry).unwrap_err().failure,
            Failure::InvalidRegistry
        );
        fs::remove_file(&registry).unwrap();
        assert_eq!(
            find_vrpathreg(&registry).unwrap_err().failure,
            Failure::ReadRegistry(io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn manual_fallback_gives_the_matching_command_without_local_paths() {
        for (action, expected) in [
            (DriverAction::Register, "adddriver"),
            (DriverAction::Unregister, "removedriver"),
        ] {
            let message = DriverError::from(Failure::MissingUtility)
                .with_manual(action)
                .to_string();
            assert!(message.contains(&format!(
                "& \"<SteamVR のフォルダー>\\bin\\win64\\vrpathreg.exe\" {expected} \"<openut.exe のフォルダー>\\openut\""
            )));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn does_not_attempt_registration_on_other_platforms() {
        assert_eq!(
            run(DriverAction::Register).unwrap_err().failure,
            Failure::UnsupportedPlatform
        );
    }
}
