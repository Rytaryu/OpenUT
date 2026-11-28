use std::fmt;
use std::io::{self, BufRead};

use serde::Deserialize;

use super::{FORMAT, Header, Record, VERSION};

/// Counters for the entries read so far, excluding the header.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadStats {
    /// Invalid rows and rows whose monotonic timestamps were clamped.
    pub malformed: u64,
    /// Well-formed rows with an unrecognized type.
    pub unknown: u64,
    /// Rows whose timestamps went backwards, also counted as malformed.
    pub time_reversed: u64,
}

pub enum ReadError {
    Io(io::Error),
    InvalidHeader,
    UnsupportedVersion(u64),
}

impl fmt::Debug for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => f.debug_tuple("Io").field(&error.kind()).finish(),
            Self::InvalidHeader => f.write_str("InvalidHeader"),
            Self::UnsupportedVersion(version) => {
                f.debug_tuple("UnsupportedVersion").field(version).finish()
            }
        }
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "記録の読み取りに失敗しました（{:?}）", error.kind()),
            Self::InvalidHeader => f.write_str("記録の見出しを読み取れません"),
            Self::UnsupportedVersion(version) => {
                write!(f, "記録のバージョン {version} には対応していません")
            }
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Reads JSON Lines from a buffered source in file order, reusing one line buffer.
pub struct RecordReader<R: BufRead> {
    input: R,
    header: Header,
    line: Vec<u8>,
    stats: ReadStats,
    last_mono_us: u64,
}

impl<R: BufRead> RecordReader<R> {
    pub fn new(mut input: R) -> Result<Self, ReadError> {
        let mut line = Vec::new();
        input.read_until(b'\n', &mut line).map_err(ReadError::Io)?;
        let header = parse_header(&line)?;
        line.clear();
        Ok(Self {
            input,
            header,
            line,
            stats: ReadStats::default(),
            last_mono_us: 0,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn stats(&self) -> ReadStats {
        self.stats
    }

    /// Skips unrecognized or damaged rows. An absent footer is valid.
    pub fn read_record(&mut self) -> io::Result<Option<Record>> {
        loop {
            if self.input.read_until(b'\n', &mut self.line)? == 0 && self.line.is_empty() {
                return Ok(None);
            }
            let line = object_line(&self.line);
            let record = line.and_then(|line| serde_json::from_str::<Record>(line).ok());
            let unknown = record.is_none() && line.is_some_and(unknown_row);
            // Retain partial input when a fallible read needs to be retried.
            self.line.clear();
            match record {
                Some(mut record) => {
                    let mono_us = record.mono_us();
                    if mono_us < self.last_mono_us {
                        self.stats.malformed = self.stats.malformed.saturating_add(1);
                        self.stats.time_reversed = self.stats.time_reversed.saturating_add(1);
                        record.set_mono_us(self.last_mono_us);
                    } else {
                        self.last_mono_us = mono_us;
                    }
                    return Ok(Some(record));
                }
                None if unknown => {
                    self.stats.unknown = self.stats.unknown.saturating_add(1);
                }
                None => {
                    self.stats.malformed = self.stats.malformed.saturating_add(1);
                }
            }
        }
    }
}

fn object_line(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes)
        .ok()
        .filter(|line| line.trim_start().starts_with('{'))
}

fn parse_header(bytes: &[u8]) -> Result<Header, ReadError> {
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(rename = "type")]
        kind: String,
        format: String,
        version: u64,
    }

    let line = object_line(bytes).ok_or(ReadError::InvalidHeader)?;
    let envelope: Envelope = serde_json::from_str(line).map_err(|_| ReadError::InvalidHeader)?;
    if envelope.kind != "header" || envelope.format != FORMAT {
        return Err(ReadError::InvalidHeader);
    }
    if !matches!(envelope.version, 1 | VERSION) {
        return Err(ReadError::UnsupportedVersion(envelope.version));
    }
    serde_json::from_str(line).map_err(|_| ReadError::InvalidHeader)
}

fn unknown_row(line: &str) -> bool {
    #[derive(Deserialize)]
    struct Tag {
        #[serde(rename = "type")]
        kind: String,
    }

    serde_json::from_str::<Tag>(line).is_ok_and(|tag| {
        !matches!(
            tag.kind.as_str(),
            "header" | "in" | "tick" | "note" | "omitted" | "footer"
        )
    })
}
