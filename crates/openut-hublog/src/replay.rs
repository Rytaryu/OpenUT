//! Read saved log files through the same decoder and header parser as live input.

use crate::decode::Decoder;
use crate::header::{ParsedLine, parse_line};

/// A saved `ViveTrackerServer_YYYYMMDD.txt` file, already read by the caller.
pub struct ReplayFile<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReplayCounts {
    pub lines: usize,
    pub no_header: usize,
}

/// Process files in filename date order and lines in their original order.
pub fn replay(files: &[ReplayFile<'_>]) -> ReplayCounts {
    let mut files: Vec<_> = files.iter().collect();
    // The fixed-width date suffix sorts in calendar order.
    files.sort_by_key(|file| file.name);

    let mut counts = ReplayCounts::default();
    for file in files {
        let mut decoder = Decoder::new();
        for chunk in file.bytes.chunks(8192) {
            count_lines(decoder.push(chunk), &mut counts);
        }
        count_lines(decoder.finish(), &mut counts);
    }
    counts
}

fn count_lines(lines: Vec<String>, counts: &mut ReplayCounts) {
    for line in lines {
        counts.lines += 1;
        if matches!(parse_line(&line), ParsedLine::NoHeader(_)) {
            counts.no_header += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_unterminated_lines_and_resets_decoder_between_files() {
        let header = "[2026-11-28 12:00:00.000 +9 0x1 INFO Test] hello";
        let later = format!("{header}\nunparsed");
        let earlier: Vec<_> = [0xfeff]
            .into_iter()
            .chain(header.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        let files = [
            ReplayFile {
                name: "ViveTrackerServer_20261128.txt",
                bytes: later.as_bytes(),
            },
            ReplayFile {
                name: "ViveTrackerServer_20261127.txt",
                bytes: &earlier,
            },
        ];
        assert_eq!(
            replay(&files),
            ReplayCounts {
                lines: 3,
                no_header: 1
            }
        );
    }
}
