//! Incremental decoding of VIVE Hub log files.

use std::borrow::Cow;

const MAX_LINE_BYTES: usize = 64 * 1024;
const ENCODING_PREFIX_BYTES: usize = 4;

#[derive(Clone, Copy)]
enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

/// Counts of malformed input sequences and overlong physical lines.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecodeStats {
    pub replaced_chars: u64,
    pub long_lines: u64,
}

/// Decodes one file, retaining partial character and line data between reads.
#[derive(Default)]
pub struct Decoder {
    encoding: Option<Encoding>,
    pending: Vec<u8>,
    high_surrogate: Option<u16>,
    line: String,
    pending_cr: bool,
    overlong_line: bool,
    stats: DecodeStats,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> DecodeStats {
        self.stats
    }

    /// Emits lines terminated by a newline and fragments of overlong lines.
    pub fn push(&mut self, mut bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        if self.encoding.is_none() {
            let prefix_len = bytes.len().min(ENCODING_PREFIX_BYTES - self.pending.len());
            let (prefix, rest) = bytes.split_at(prefix_len);
            self.pending.extend_from_slice(prefix);
            bytes = rest;
            self.select_encoding(false, &mut lines);
        }
        self.decode(bytes, &mut lines);
        lines
    }

    /// Flushes incomplete data at EOF, including an unterminated final line.
    pub fn finish(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.encoding.is_none() {
            self.select_encoding(true, &mut lines);
        }
        if self.high_surrogate.take().is_some() {
            self.replace(&mut lines);
        }
        if !self.pending.is_empty() {
            self.pending.clear();
            self.replace(&mut lines);
        }
        if std::mem::take(&mut self.pending_cr) {
            self.push_content('\r', &mut lines);
        }
        if !self.line.is_empty() {
            lines.push(std::mem::take(&mut self.line));
        }
        self.overlong_line = false;
        lines
    }

    fn select_encoding(&mut self, eof: bool, lines: &mut Vec<String>) {
        let (encoding, bom_bytes) = match self.pending.as_slice() {
            [0xff, 0xfe, ..] => (Encoding::Utf16Le, 2),
            [0xfe, 0xff, ..] => (Encoding::Utf16Be, 2),
            [0xef, 0xbb, 0xbf, ..] => (Encoding::Utf8, 3),
            [_, 0, _, 0, ..] => (Encoding::Utf16Le, 0),
            // A nonzero second byte rules out BOM-less UTF-16LE.
            [first, second, ..] if *second != 0 && (*first, *second) != (0xef, 0xbb) => {
                (Encoding::Utf8, 0)
            }
            bytes if eof || bytes.len() == ENCODING_PREFIX_BYTES => (Encoding::Utf8, 0),
            _ => return,
        };
        self.encoding = Some(encoding);
        let prefix = std::mem::take(&mut self.pending);
        self.decode(&prefix[bom_bytes..], lines);
    }

    fn decode(&mut self, bytes: &[u8], lines: &mut Vec<String>) {
        if bytes.is_empty() {
            return;
        }
        match self.encoding {
            Some(Encoding::Utf8) => self.decode_utf8(bytes, lines),
            Some(Encoding::Utf16Le) => self.decode_utf16(bytes, u16::from_le_bytes, lines),
            Some(Encoding::Utf16Be) => self.decode_utf16(bytes, u16::from_be_bytes, lines),
            None => {}
        }
    }

    fn decode_utf8(&mut self, bytes: &[u8], lines: &mut Vec<String>) {
        let input = if self.pending.is_empty() {
            Cow::Borrowed(bytes)
        } else {
            let mut joined = std::mem::take(&mut self.pending);
            joined.extend_from_slice(bytes);
            Cow::Owned(joined)
        };
        let mut bytes = input.as_ref();
        while !bytes.is_empty() {
            match std::str::from_utf8(bytes) {
                Ok(text) => {
                    self.push_text(text, lines);
                    break;
                }
                Err(error) => {
                    let (valid, rest) = bytes.split_at(error.valid_up_to());
                    if let Ok(text) = std::str::from_utf8(valid) {
                        self.push_text(text, lines);
                    }
                    let Some(invalid_bytes) = error.error_len() else {
                        self.pending.extend_from_slice(rest);
                        break;
                    };
                    self.replace(lines);
                    bytes = &rest[invalid_bytes..];
                }
            }
        }
    }

    fn decode_utf16(
        &mut self,
        mut bytes: &[u8],
        to_unit: fn([u8; 2]) -> u16,
        lines: &mut Vec<String>,
    ) {
        if let (Some(first), Some((&second, rest))) =
            (self.pending.first().copied(), bytes.split_first())
        {
            self.pending.clear();
            self.push_unit(to_unit([first, second]), lines);
            bytes = rest;
        }

        let (pairs, remainder) = bytes.as_chunks::<2>();
        self.pending.extend_from_slice(remainder);
        for &pair in pairs {
            self.push_unit(to_unit(pair), lines);
        }
    }

    fn push_unit(&mut self, unit: u16, lines: &mut Vec<String>) {
        if let Some(high) = self.high_surrogate.take() {
            if let Some(Ok(character)) = char::decode_utf16([high, unit]).next() {
                self.push_char(character, lines);
                return;
            }
            self.replace(lines);
        }

        if (0xd800..=0xdbff).contains(&unit) {
            self.high_surrogate = Some(unit);
        } else if let Some(character) = char::from_u32(u32::from(unit)) {
            self.push_char(character, lines);
        } else {
            self.replace(lines);
        }
    }

    fn replace(&mut self, lines: &mut Vec<String>) {
        self.stats.replaced_chars = self.stats.replaced_chars.saturating_add(1);
        self.push_char(char::REPLACEMENT_CHARACTER, lines);
    }

    fn push_text(&mut self, text: &str, lines: &mut Vec<String>) {
        for character in text.chars() {
            self.push_char(character, lines);
        }
    }

    fn push_char(&mut self, character: char, lines: &mut Vec<String>) {
        if character == '\n' {
            self.pending_cr = false;
            self.overlong_line = false;
            lines.push(std::mem::take(&mut self.line));
            return;
        }

        // Defer CR so that a CRLF terminator does not count toward the line limit.
        if std::mem::take(&mut self.pending_cr) {
            self.push_content('\r', lines);
        }
        if character == '\r' {
            self.pending_cr = true;
        } else {
            self.push_content(character, lines);
        }
    }

    fn push_content(&mut self, character: char, lines: &mut Vec<String>) {
        // Split the decoded text at a character boundary and count the
        // physical line once, even if it takes multiple fragments to emit it.
        if self.line.len() + character.len_utf8() > MAX_LINE_BYTES {
            lines.push(std::mem::take(&mut self.line));
            if !self.overlong_line {
                self.stats.long_lines = self.stats.long_lines.saturating_add(1);
                self.overlong_line = true;
            }
        }
        self.line.push(character);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn decode_in_chunks(bytes: &[u8], chunk_size: usize) -> (Vec<String>, DecodeStats) {
        let mut decoder = Decoder::new();
        let mut lines = Vec::new();
        for chunk in bytes.chunks(chunk_size) {
            lines.extend(decoder.push(chunk));
        }
        lines.extend(decoder.finish());
        (lines, decoder.stats())
    }

    #[test]
    fn reads_three_utf16le_bom_crlf_lines_with_any_chunk_size() {
        let mut input = vec![0xff, 0xfe];
        input.extend(utf16le("first\r\nsecond\r\nthird\r\n"));
        for chunk_size in 1..=input.len() {
            let (lines, stats) = decode_in_chunks(&input, chunk_size);
            assert_eq!(lines, ["first", "second", "third"]);
            assert_eq!(stats, DecodeStats::default());
        }
    }

    #[test]
    fn carries_a_surrogate_pair_across_reads() {
        let mut decoder = Decoder::new();
        assert!(decoder.push(&[0xff, 0xfe, 0x3d, 0xd8]).is_empty());
        assert!(decoder.push(&[0x00]).is_empty());
        assert_eq!(decoder.push(&[0xde, b'\n', 0]), ["😀"]);
        assert_eq!(decoder.stats(), DecodeStats::default());
    }

    #[test]
    fn replaces_unpaired_surrogates_and_continues_to_next_line() {
        let mut input = vec![0xff, 0xfe, 0x3d, 0xd8, 0x00, 0xd8];
        input.extend(utf16le("x\n"));
        input.extend([0x00, 0xdc]);
        input.extend(utf16le("next\n"));
        let (lines, stats) = decode_in_chunks(&input, 1);
        assert_eq!(lines, ["��x", "�next"]);
        assert_eq!(stats.replaced_chars, 3);
    }

    #[test]
    fn recognizes_bom_and_bom_less_encodings() {
        let expected = "abc日本語";
        let input = format!("{expected}\n");
        let mut utf8_bom = vec![0xef, 0xbb, 0xbf];
        utf8_bom.extend(input.as_bytes());
        let mut utf16be_bom = vec![0xfe, 0xff];
        utf16be_bom.extend(input.encode_utf16().flat_map(u16::to_be_bytes));
        let utf16le = utf16le(&input);

        for bytes in [input.as_bytes(), &utf8_bom, &utf16le, &utf16be_bom] {
            for chunk_size in 1..=bytes.len() {
                let (lines, stats) = decode_in_chunks(bytes, chunk_size);
                assert_eq!(lines, [expected]);
                assert_eq!(stats, DecodeStats::default());
            }
        }
    }

    #[test]
    fn counts_only_replacements_for_malformed_utf8() {
        let mut input = "actual �\n".as_bytes().to_vec();
        input.extend(b"\xff\xe2\x82X\n\xe2\x82");
        for chunk_size in 1..=input.len() {
            let (lines, stats) = decode_in_chunks(&input, chunk_size);
            assert_eq!(lines, ["actual �", "��X", "�"]);
            assert_eq!(stats.replaced_chars, 3);
        }
    }

    #[test]
    fn finish_flushes_only_the_unterminated_line() {
        let mut decoder = Decoder::new();
        assert_eq!(decoder.push(b"first\r\n\nlast\r"), ["first", ""]);
        assert_eq!(decoder.finish(), ["last\r"]);
        assert!(decoder.finish().is_empty());

        let mut short = Decoder::new();
        assert!(short.push(b"x").is_empty());
        assert_eq!(short.finish(), ["x"]);
    }

    #[test]
    fn finish_replaces_an_unpaired_surrogate_and_odd_byte() {
        let mut decoder = Decoder::new();
        assert!(decoder.push(&[0xff, 0xfe, 0x3d, 0xd8, 0x01]).is_empty());
        assert_eq!(decoder.finish(), ["��"]);
        assert_eq!(decoder.stats().replaced_chars, 2);
    }

    #[test]
    fn splits_overlong_lines_without_splitting_characters() {
        let expected = "x".repeat(MAX_LINE_BYTES - 1);
        let remainder = "y".repeat(MAX_LINE_BYTES);
        let input = format!("{expected}é{remainder}\nnext\n");
        let (lines, stats) = decode_in_chunks(input.as_bytes(), 4096);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], expected);
        assert_eq!(lines[1..3].concat(), format!("é{remainder}"));
        assert_eq!(lines[3], "next");
        assert!(lines.iter().all(|line| line.len() <= MAX_LINE_BYTES));
        assert_eq!(stats.long_lines, 1);
    }

    #[test]
    fn allows_a_full_length_line_followed_by_crlf() {
        let expected = "x".repeat(MAX_LINE_BYTES);
        let mut decoder = Decoder::new();
        assert!(decoder.push(expected.as_bytes()).is_empty());
        assert!(decoder.push(b"\r").is_empty());
        assert_eq!(decoder.push(b"\n"), [expected]);
        assert_eq!(decoder.stats().long_lines, 0);
    }

    #[test]
    fn handles_ten_thousand_arbitrary_byte_sequences_across_chunks() {
        let mut state = 0xdead_beef_u32;
        let mut next_byte = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        };
        for sample in 0..10_000 {
            let mut input = Vec::new();
            match sample % 4 {
                1 => input.extend([0xff, 0xfe]),
                2 => input.extend([0xfe, 0xff]),
                3 => input.extend([0xef, 0xbb, 0xbf]),
                _ => {}
            }
            let length = usize::from(next_byte()) % 64;
            input.extend((0..length).map(|_| next_byte()));
            assert_eq!(
                decode_in_chunks(&input, 1),
                decode_in_chunks(&input, input.len().max(1)),
                "sample {sample}"
            );
        }
    }
}
