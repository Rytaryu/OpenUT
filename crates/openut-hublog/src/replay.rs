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
    fn replays_utf16_crlf_records_with_metadata_and_embedded_lf() {
        let source = concat!(
            "# Log begin at 2026-11-28 00:00:53.613\r\n",
            "[2026-11-28 00:00:53.613 +9 0x1 INFO ] [1] Log Start\r\n",
            "[2026-11-28 00:00:54.000 0x1 INFO Test] [1] ",
            "Save map creation time: Wed Nov 25 21:00:00 2026\n.\r\n",
            "[2026-11-28 00:00:55.000 +9 0x1 ERROR Test] [1] \r\n",
            "broken record\r\n",
        );
        let bytes: Vec<_> = [0xfeff]
            .into_iter()
            .chain(source.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            replay(&[ReplayFile {
                name: "ViveTrackerServer_20261128.txt",
                bytes: &bytes,
            }]),
            ReplayCounts {
                lines: 5,
                no_header: 1,
            },
        );
    }

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
