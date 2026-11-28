//! Versioned JSON Lines recordings of the dongle's input stream.

use std::fmt;
use std::io::{self, Write};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

mod hex;
mod reader;

pub use reader::{ReadError, ReadStats, RecordReader};

pub const FORMAT: &str = "openut.dongle-record";
pub const VERSION: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub tool: String,
    pub mode: Mode,
    pub started_wall_ms: i64,
    pub device: Device,
    pub server_running_at_start: bool,
    pub anonymized: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Record,
    ProbeSharing,
}

/// Device class and interface numbers from the recording header.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Device {
    #[serde(with = "hex::word")]
    pub vid: u16,
    #[serde(with = "hex::word")]
    pub pid: u16,
    pub interface: i32,
    #[serde(with = "hex::word")]
    pub usage_page: u16,
    #[serde(with = "hex::word")]
    pub usage: u16,
}

/// One input report as received. Its private bytes are stored only in the raw recording.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RawReport {
    pub mono_us: u64,
    pub wall_ms: i64,
    #[serde(rename = "hex", with = "hex::bytes")]
    pub bytes: Vec<u8>,
}

impl fmt::Debug for RawReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawReport")
            .field("mono_us", &self.mono_us)
            .field("wall_ms", &self.wall_ms)
            .field("len", &self.bytes.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackerLabel(pub char);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Record {
    #[serde(rename = "in")]
    Input(RawReport),
    Tick {
        mono_us: u64,
        wall_ms: i64,
    },
    Note {
        mono_us: u64,
        wall_ms: i64,
        text: Note,
    },
    Omitted {
        mono_us: u64,
        wall_ms: i64,
        report_type: u8,
        len: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tracker: Option<TrackerLabel>,
        reason: OmittedReason,
    },
    Footer {
        mono_us: u64,
        reports: u64,
        malformed: u64,
        estimated_lost: u64,
        reason: EndReason,
        #[serde(default, skip_serializing_if = "is_zero")]
        time_reversed: u64,
    },
}

impl Record {
    pub fn mono_us(&self) -> u64 {
        match self {
            Self::Input(report) => report.mono_us,
            Self::Tick { mono_us, .. }
            | Self::Note { mono_us, .. }
            | Self::Omitted { mono_us, .. }
            | Self::Footer { mono_us, .. } => *mono_us,
        }
    }

    fn set_mono_us(&mut self, at: u64) {
        match self {
            Self::Input(report) => report.mono_us = at,
            Self::Tick { mono_us, .. }
            | Self::Note { mono_us, .. }
            | Self::Omitted { mono_us, .. }
            | Self::Footer { mono_us, .. } => *mono_us = at,
        }
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmittedReason {
    NotAllowed,
    CheckFailed,
    TextForm,
    Malformed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Duration,
    ServerAppeared,
    ReadError,
    DeviceRemoved,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Note {
    ServerAppeared,
    ReadError(i32),
    DeviceRemoved,
    Duration,
    WindowBaseStart,
    WindowBaseEnd,
    WindowProbeStart,
    WindowProbeEnd,
    OpenFailed(i32),
}

impl Serialize for Note {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let word = match self {
            Self::ServerAppeared => "server_appeared",
            Self::ReadError(code) => {
                return serializer.collect_str(&format_args!("read_error:{code}"));
            }
            Self::DeviceRemoved => "device_removed",
            Self::Duration => "duration",
            Self::WindowBaseStart => "window_base_start",
            Self::WindowBaseEnd => "window_base_end",
            Self::WindowProbeStart => "window_probe_start",
            Self::WindowProbeEnd => "window_probe_end",
            Self::OpenFailed(code) => {
                return serializer.collect_str(&format_args!("open_failed:{code}"));
            }
        };
        serializer.serialize_str(word)
    }
}

impl<'de> Deserialize<'de> for Note {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let word = String::deserialize(deserializer)?;
        let note = match word.as_str() {
            "server_appeared" => Self::ServerAppeared,
            "device_removed" => Self::DeviceRemoved,
            "duration" => Self::Duration,
            "window_base_start" => Self::WindowBaseStart,
            "window_base_end" => Self::WindowBaseEnd,
            "window_probe_start" => Self::WindowProbeStart,
            "window_probe_end" => Self::WindowProbeEnd,
            _ => {
                let (kind, code) = word
                    .split_once(':')
                    .ok_or_else(|| de::Error::custom("unknown note"))?;
                let code = code
                    .parse()
                    .map_err(|_| de::Error::custom("invalid OS error number"))?;
                match kind {
                    "read_error" => Self::ReadError(code),
                    "open_failed" => Self::OpenFailed(code),
                    _ => return Err(de::Error::custom("unknown note")),
                }
            }
        };
        Ok(note)
    }
}

#[derive(Serialize)]
struct HeaderLine<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    format: &'static str,
    version: u64,
    #[serde(flatten)]
    header: &'a Header,
}

/// Writes a header followed by records in the supplied stream order.
pub struct RecordWriter<W> {
    output: W,
}

impl<W: Write> RecordWriter<W> {
    pub fn new(mut output: W, header: &Header) -> io::Result<Self> {
        let first = HeaderLine {
            kind: "header",
            format: FORMAT,
            version: VERSION,
            header,
        };
        write_line(&mut output, &first)?;
        Ok(Self { output })
    }

    pub fn write_record(&mut self, record: &Record) -> io::Result<()> {
        write_line(&mut self.output, record)
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }

    pub fn into_inner(self) -> W {
        self.output
    }
}

fn write_line(output: &mut impl Write, line: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *output, line)?;
    output.write_all(b"\n")
}
