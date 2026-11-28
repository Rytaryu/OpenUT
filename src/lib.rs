//! OpenUT: an unofficial runtime and tools for VIVE Ultimate Tracker on
//! Windows + SteamVR.

pub mod bridge_out;
pub mod calib_file;
pub mod calib_flow;
pub mod config;
pub mod driver_reg;
pub mod replay;
pub mod run;
pub mod source_fs;
pub mod steamvr;

/// The version of this build, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use openut_core::time::LogTime;
    use openut_hublog::{
        decode::Decoder,
        header::{ParsedLine, parse_line},
    };

    use super::VERSION;

    #[test]
    fn version_comes_from_the_manifest() {
        assert_eq!(VERSION, "0.0.0");
    }

    #[test]
    fn decodes_incremental_hub_log_bytes_and_parses_the_header() {
        let text = "[2026-11-27 22:53:10.123 +9 0x0 INFO Startup] [1] Hub started\r\n";
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let mut decoder = Decoder::new();
        let lines: Vec<_> = bytes
            .chunks(3)
            .flat_map(|chunk| decoder.push(chunk))
            .collect();
        assert_eq!(lines.len(), 1);
        let ParsedLine::Entry(entry) = parse_line(&lines[0]) else {
            panic!("expected a log entry");
        };
        assert_eq!(entry.at, LogTime(1_795_787_590_123));
        assert_eq!(entry.body, "Hub started");
    }
}
