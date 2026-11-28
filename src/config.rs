//! Settings used by the read-only SteamVR bridge.

use std::path::Path;

use openut_space::{AxisMap, BodyRotation, Handedness};
use serde::Deserialize;

pub const DEFAULT_BRIDGE_PORT: u16 = 27_316;

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
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
