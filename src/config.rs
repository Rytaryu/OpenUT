//! Settings for the SteamVR bridge and the Hub log watcher.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openut_core::engine::EngineConfig;
use openut_space::{AxisMap, BodyRotation, Handedness};
use serde::Deserialize;
use toml::{Table, Value};

pub const DEFAULT_BRIDGE_PORT: u16 = 27_316;
pub const DEFAULT_HUB_PROCESS: &str = "ViveTrackerServer.exe";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunConfig {
    pub port: u16,
    pub axis_map: AxisMap,
    pub body_rotation: BodyRotation,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_BRIDGE_PORT,
            axis_map: AxisMap::IDENTITY,
            body_rotation: BodyRotation::IDENTITY,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigWarning {
    Read,
    Syntax,
    Port,
    AxisMap,
    BodyRotation,
    WatchOption,
    UnknownEngineOption,
    EngineValue,
    LegacyTrackers,
}

impl ConfigWarning {
    pub fn message(self) -> &'static str {
        match self {
            Self::Read => "設定を読めなかったため、既定値を使います。",
            Self::Syntax => "設定の形式が違うため、既定値を使います。",
            Self::Port => "bridge.port は 1〜65535 にしてください。既定値を使います。",
            Self::AxisMap => {
                "space.axis_map は向きを反転しない軸の組にしてください。既定値を使います。"
            }
            Self::BodyRotation => "space.body_rotation を読み取れません。既定値を使います。",
            Self::WatchOption => "監視の設定値が不正なため、既定値を使います。",
            Self::UnknownEngineOption => "規則・集計・通知の設定名を読み取れません。",
            Self::EngineValue => "規則・集計・通知の設定値を読み取れません。",
            Self::LegacyTrackers => "以前の trackers 設定は読み飛ばしました。",
        }
    }
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ConfigFile {
    bridge: BridgeConfig,
    space: SpaceConfig,
}

#[derive(Default, Deserialize)]
struct BridgeConfig {
    port: Option<u16>,
}

#[derive(Default, Deserialize)]
struct SpaceConfig {
    axis_map: Option<String>,
    body_rotation: Option<String>,
}

impl RunConfig {
    pub fn parse(text: &str) -> Result<Self, ConfigWarning> {
        let config: ConfigFile = toml::from_str(text).map_err(|_| ConfigWarning::Syntax)?;
        let port = config.bridge.port.unwrap_or(DEFAULT_BRIDGE_PORT);
        if port == 0 {
            return Err(ConfigWarning::Port);
        }
        let axis_map = config
            .space
            .axis_map
            .unwrap_or_else(|| "+x+y+z".to_owned())
            .parse::<AxisMap>()
            .map_err(|_| ConfigWarning::AxisMap)?;
        // Until calibration can disambiguate handedness, K2 uses a proper rotation.
        if axis_map.group().handedness != Handedness::Preserving {
            return Err(ConfigWarning::AxisMap);
        }
        let body_rotation = config
            .space
            .body_rotation
            .unwrap_or_else(|| "+x+y+z".to_owned())
            .parse()
            .map_err(|_| ConfigWarning::BodyRotation)?;
        Ok(Self {
            port,
            axis_map,
            body_rotation,
        })
    }

    /// A missing config is ordinary. Malformed input yields a fixed diagnostic.
    pub fn read(path: Option<&Path>) -> (Self, Option<ConfigWarning>) {
        let Some(path) = path else {
            return (Self::default(), None);
        };
        let result = match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err(ConfigWarning::Read),
        };
        match result {
            Ok(config) => (config, None),
            Err(warning) => (Self::default(), Some(warning)),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct NotifyConfig {
    pub steamvr: bool,
    pub desktop: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            steamvr: true,
            desktop: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchConfig {
    pub engine: EngineConfig,
    pub log_dir: Option<PathBuf>,
    pub hub_process_names: Vec<String>,
    pub process_check_s: u64,
    pub start_wait_s: u64,
    pub poll_ms: u64,
    pub lateness_ms: i64,
    pub notify: NotifyConfig,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct WatchFile {
    log_dir: Option<PathBuf>,
    watch: WatchSettings,
    notify: NotifyConfig,
}

#[derive(Deserialize)]
#[serde(default)]
struct WatchSettings {
    hub_process_names: Vec<String>,
    process_check_s: u64,
    start_wait_s: u64,
    poll_ms: u64,
    lateness_ms: i64,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            hub_process_names: Vec::new(),
            process_check_s: 10,
            start_wait_s: 60,
            poll_ms: 500,
            lateness_ms: 10_000,
        }
    }
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self::from_file(WatchFile::default(), EngineConfig::default())
    }
}

impl WatchConfig {
    /// A legacy tracker-name table is ignored without discarding other settings.
    pub fn parse(text: &str) -> Result<(Self, Option<ConfigWarning>), ConfigWarning> {
        let table: Table = toml::from_str(text).map_err(|_| ConfigWarning::Syntax)?;
        let ignored_trackers = table.contains_key("trackers");
        let mut engine = EngineConfig::default();
        apply_engine_config(&mut engine, &table)?;
        let file: WatchFile = table.try_into().map_err(|_| ConfigWarning::Syntax)?;
        let settings = &file.watch;
        let max_whole_seconds = (i64::MAX / 1_000) as u64;
        if file
            .log_dir
            .as_ref()
            .is_some_and(|dir| dir.as_os_str().is_empty())
            || !(1..=max_whole_seconds).contains(&settings.process_check_s)
            || settings.start_wait_s > max_whole_seconds
            || settings.poll_ms == 0
            || settings.lateness_ms < 0
            || settings
                .hub_process_names
                .iter()
                .any(|name| name.trim().is_empty() || name.contains(['/', '\\', ':', '\0']))
        {
            return Err(ConfigWarning::WatchOption);
        }
        Ok((
            Self::from_file(file, engine),
            ignored_trackers.then_some(ConfigWarning::LegacyTrackers),
        ))
    }

    /// Reads watcher settings with defaults when the file is missing or unusable.
    pub fn read(path: Option<&Path>) -> (Self, Option<ConfigWarning>) {
        let Some(path) = path else {
            return (Self::default(), None);
        };
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|error| (Self::default(), Some(error))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Self::default(), None),
            Err(_) => (Self::default(), Some(ConfigWarning::Read)),
        }
    }

    /// Saves the user selections and keeps all other TOML settings in the file.
    pub fn save_selections(
        path: &Path,
        log_dir: Option<&Path>,
        notify: NotifyConfig,
    ) -> io::Result<()> {
        let mut table: Table = match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|_| invalid_config())?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Table::new(),
            Err(error) => return Err(error),
        };
        match log_dir {
            Some(dir) => {
                let dir = dir.to_str().filter(|dir| !dir.is_empty()).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid log directory setting")
                })?;
                table.insert("log_dir".to_owned(), Value::String(dir.to_owned()));
            }
            None => {
                table.remove("log_dir");
            }
        }
        let notifications = table
            .entry("notify".to_owned())
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(invalid_config)?;
        notifications.insert("steamvr".to_owned(), Value::Boolean(notify.steamvr));
        notifications.insert("desktop".to_owned(), Value::Boolean(notify.desktop));

        let settings = toml::to_string_pretty(&table).map_err(|_| invalid_config())?;
        let contents =
            format!("# OpenUT の設定。OpenUT が書き直すと、コメントは消えます。\n\n{settings}");
        crate::paths::replace_file(path, contents.as_bytes())
    }

    fn from_file(file: WatchFile, engine: EngineConfig) -> Self {
        let mut hub_process_names = vec![DEFAULT_HUB_PROCESS.to_owned()];
        for name in file.watch.hub_process_names {
            if !hub_process_names
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&name))
            {
                hub_process_names.push(name);
            }
        }
        Self {
            engine,
            log_dir: file.log_dir,
            hub_process_names,
            process_check_s: file.watch.process_check_s,
            start_wait_s: file.watch.start_wait_s,
            poll_ms: file.watch.poll_ms,
            lateness_ms: file.watch.lateness_ms,
            notify: file.notify,
        }
    }
}

fn invalid_config() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "could not rewrite config settings",
    )
}

/// Applies named engine sections, leaving the engine unchanged on an invalid option.
pub fn apply_engine_config(engine: &mut EngineConfig, table: &Table) -> Result<(), ConfigWarning> {
    let mut updated = engine.clone();
    for name in ["rules", "state", "notice", "summary"] {
        if let Some(value) = table.get(name) {
            apply_engine_section(&mut updated, name, value)?;
        }
    }
    *engine = updated;
    Ok(())
}

fn apply_engine_section(
    engine: &mut EngineConfig,
    name: &str,
    value: &Value,
) -> Result<(), ConfigWarning> {
    if let Some(table) = value.as_table() {
        for (key, value) in table {
            apply_engine_section(engine, &format!("{name}.{key}"), value)?;
        }
        Ok(())
    } else {
        set_engine_value(engine, name, value)
    }
}

/// Applies a single dotted key, as used by a replay `--set` override.
pub fn set_engine_value(
    engine: &mut EngineConfig,
    key: &str,
    value: &Value,
) -> Result<(), ConfigWarning> {
    let integer = || {
        value
            .as_integer()
            .filter(|value| *value >= 0)
            .ok_or(ConfigWarning::EngineValue)
    };
    let number = || integer().map(|value| value as u64);
    let flag = || value.as_bool().ok_or(ConfigWarning::EngineValue);
    match key {
        "rules.r1.count" => {
            let count = number()?;
            if count == 0 {
                return Err(ConfigWarning::EngineValue);
            }
            engine.rules.r1.count = count;
        }
        "rules.r1.span_s" => engine.rules.r1.span_s = number()?,
        "rules.r1.gap_s" => engine.rules.r1.gap_s = number()?,
        "rules.r1.escalate_s" => engine.rules.r1.escalate_s = number()?,
        "rules.r1.quiet_s" => engine.rules.r1.quiet_s = number()?,
        "rules.r2.wait_s" => engine.rules.r2.wait_s = number()?,
        "rules.r3.wait_s" => engine.rules.r3.wait_s = number()?,
        "rules.r3.clients" => engine.rules.r3.clients = flag()?,
        "rules.r4.cmd_window_s" => engine.rules.r4.cmd_window_s = number()?,
        "rules.r4.min_connected_s" => engine.rules.r4.min_connected_s = number()?,
        "rules.r4.burst_s" => engine.rules.r4.burst_s = number()?,
        "rules.r4.server_s" => engine.rules.r4.server_s = number()?,
        "rules.r4.exclude_power_off" => engine.rules.r4.exclude_power_off = flag()?,
        "rules.r4.notice_before_s" => engine.rules.r4.notice_before_s = number()?,
        "rules.r4.notice_after_s" => engine.rules.r4.notice_after_s = number()?,
        "rules.r4.back_s" => engine.rules.r4.back_s = number()?,
        "rules.r5.wait_s" => engine.rules.r5.wait_s = number()?,
        "rules.r5.enabled" => engine.rules.r5.enabled = flag()?,
        "state.dup_ms" => engine.state.dup_ms = integer()?,
        "notice.cooldown_s" => engine.notice.cooldown_s = number()?,
        "notice.min_gap_s" => engine.notice.min_gap_s = number()?,
        "summary.gap_s" => engine.summary.gap_s = number()?,
        "summary.finalize_delay_s" => engine.summary.finalize_delay_s = number()?,
        _ => return Err(ConfigWarning::UnknownEngineOption),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::TestDirectory;

    #[test]
    fn shared_config_has_defaults_and_accepts_proper_rotations() {
        assert_eq!(RunConfig::parse(""), Ok(RunConfig::default()));
        let config = RunConfig::parse(concat!(
            "[bridge]\nport = 32167\n[space]\naxis_map = '+z+y-x'\nbody_rotation = '-x-y+z'\n",
            "[watch]\nignored = true\n",
        ))
        .unwrap();
        assert_eq!(config.port, 32_167);
        assert_eq!(config.axis_map, "+z+y-x".parse().unwrap());
        assert_eq!(config.body_rotation, "-x-y+z".parse().unwrap());
    }

    #[test]
    fn rejects_zero_port_invalid_formats_and_mirrors_without_echoing_values() {
        assert_eq!(
            RunConfig::parse("[bridge]\nport = 0"),
            Err(ConfigWarning::Port)
        );
        assert_eq!(
            RunConfig::parse("[space]\naxis_map = '-x+y+z'"),
            Err(ConfigWarning::AxisMap)
        );
        assert_eq!(
            RunConfig::parse("[space]\nbody_rotation = '-x+y+z'"),
            Err(ConfigWarning::BodyRotation)
        );
        let result = RunConfig::parse("[space]\nbody_rotation = 'private-value'")
            .expect_err("unexpected setting must be rejected");
        assert!(!result.message().contains("private-value"));
    }

    #[test]
    fn watch_reads_documented_options_and_ignores_old_tracker_names() {
        let (config, warning) = WatchConfig::parse(
            "log_dir = 'synthetic-logs'\n\
             [notify]\nsteamvr = false\n\
             [watch]\nhub_process_names = ['SyntheticHub.exe', 'VIVETRACKERSERVER.EXE']\n\
             process_check_s = 11\nstart_wait_s = 61\npoll_ms = 400\nlateness_ms = 9000\n\
             [rules]\nr1.count = 4\nr3.clients = true\n\
             [summary]\ngap_s = 500\nfinalize_delay_s = 45\n\
             [notice]\ncooldown_s = 25\n\
             [trackers.'synthetic-tracker']\nname = 'unused'\n",
        )
        .unwrap();
        assert_eq!(warning, Some(ConfigWarning::LegacyTrackers));
        assert_eq!(config.log_dir.as_deref(), Some(Path::new("synthetic-logs")));
        assert_eq!(
            config.notify,
            NotifyConfig {
                steamvr: false,
                desktop: true
            }
        );
        assert_eq!(
            config.hub_process_names,
            [DEFAULT_HUB_PROCESS, "SyntheticHub.exe"]
        );
        assert_eq!((config.process_check_s, config.start_wait_s), (11, 61));
        assert_eq!((config.poll_ms, config.lateness_ms), (400, 9000));
        assert_eq!(config.engine.rules.r1.count, 4);
        assert!(config.engine.rules.r3.clients);
        assert_eq!(config.engine.notice.cooldown_s, 25);
        assert_eq!(config.engine.summary.gap_s, 500);
        assert_eq!(config.engine.summary.finalize_delay_s, 45);
    }

    #[test]
    fn watch_defaults_on_missing_or_invalid_files() -> io::Result<()> {
        let directory = TestDirectory::new()?;
        let path = directory.0.join("config.toml");
        assert_eq!(
            WatchConfig::read(Some(&path)),
            (WatchConfig::default(), None)
        );
        assert_eq!(WatchConfig::parse(""), Ok((WatchConfig::default(), None)));

        for (text, warning) in [
            ("[watch", ConfigWarning::Syntax),
            ("[watch]\npoll_ms = 0", ConfigWarning::WatchOption),
            ("[watch]\nprocess_check_s = 0", ConfigWarning::WatchOption),
            ("[watch]\nlateness_ms = -1", ConfigWarning::WatchOption),
            (
                "[watch]\nhub_process_names = ['bad/path']",
                ConfigWarning::WatchOption,
            ),
            ("[rules.r1]\ncount = 0", ConfigWarning::EngineValue),
            ("[rules.r3]\nclients = 'bad'", ConfigWarning::EngineValue),
        ] {
            fs::write(&path, text)?;
            assert_eq!(
                WatchConfig::read(Some(&path)),
                (WatchConfig::default(), Some(warning))
            );
        }
        Ok(())
    }

    #[test]
    fn selection_rewrite_keeps_other_settings_and_round_trips() -> io::Result<()> {
        let directory = TestDirectory::new()?;
        let path = directory.0.join("config.toml");
        fs::write(
            &path,
            "[bridge]\nport = 32167\n[rules.r1]\ncount = 4\n[notify]\nother_setting = true\n",
        )?;
        let notify = NotifyConfig {
            steamvr: false,
            desktop: false,
        };
        let selected = Path::new("synthetic-logs");
        WatchConfig::save_selections(&path, Some(selected), notify)?;

        let text = fs::read_to_string(&path)?;
        assert!(text.starts_with('#'));
        let table: Table = toml::from_str(&text).unwrap();
        assert_eq!(table["notify"]["other_setting"].as_bool(), Some(true));
        assert_eq!(RunConfig::parse(&text).unwrap().port, 32_167);
        let (watch, warning) = WatchConfig::read(Some(&path));
        assert_eq!(warning, None);
        assert_eq!(watch.log_dir.as_deref(), Some(selected));
        assert_eq!(watch.notify, notify);
        assert_eq!(watch.engine.rules.r1.count, 4);

        WatchConfig::save_selections(&path, None, NotifyConfig::default())?;
        let (watch, _) = WatchConfig::read(Some(&path));
        assert_eq!(watch.log_dir, None);
        assert_eq!(watch.notify, NotifyConfig::default());
        assert_eq!(fs::read_dir(&directory.0)?.count(), 1);
        Ok(())
    }

    #[test]
    fn malformed_user_settings_are_not_replaced_by_ui_selections() -> io::Result<()> {
        let directory = TestDirectory::new()?;
        let path = directory.0.join("config.toml");
        let old = "[invalid";
        fs::write(&path, old)?;
        let error = WatchConfig::save_selections(&path, None, NotifyConfig::default()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(path)?, old);
        Ok(())
    }

    #[test]
    fn replay_and_file_overrides_validate_the_same_named_settings() {
        let mut engine = EngineConfig::default();
        set_engine_value(&mut engine, "rules.r5.enabled", &Value::Boolean(true)).unwrap();
        assert!(engine.rules.r5.enabled);
        let before = engine.clone();
        let malformed = "[rules.r1]\ncount = 4\n[summary]\ngap_s = -1"
            .parse()
            .unwrap();
        assert_eq!(
            apply_engine_config(&mut engine, &malformed),
            Err(ConfigWarning::EngineValue)
        );
        assert_eq!(engine, before);
        assert_eq!(
            set_engine_value(&mut engine, "rule-name", &Value::Boolean(true)),
            Err(ConfigWarning::UnknownEngineOption)
        );
    }
}
