//! Anonymize a batch of Hub logs with aliases shared across its files.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod scan;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    TrackerSerial,
    RadioAddress,
    WifiName,
    WifiPassword,
    PcName,
    UserFolder,
    PcSerialBody,
}

impl Kind {
    fn alias_prefix(self) -> &'static str {
        match self {
            Self::TrackerSerial => "TRACKER",
            Self::RadioAddress => "RADIO",
            Self::WifiName => "WIFI",
            Self::WifiPassword => "WIFI-SECRET",
            Self::PcName => "PC",
            Self::UserFolder => "USER",
            Self::PcSerialBody => "PC-SERIAL",
        }
    }

    fn ignores_ascii_case(self) -> bool {
        matches!(
            self,
            Self::TrackerSerial | Self::RadioAddress | Self::PcName | Self::UserFolder
        )
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TrackerSerial => "トラッカーのシリアル番号",
            Self::RadioAddress => "無線のアドレス",
            Self::WifiName => "Wi-Fi の名前",
            Self::WifiPassword => "Wi-Fi のパスワード",
            Self::PcName => "PC の名前",
            Self::UserFolder => "利用者のフォルダー名",
            Self::PcSerialBody => "PC のシリアル番号を記録する本文",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Warning {
    /// One-based record number in this input.
    pub record: usize,
    pub kind: Kind,
}

pub struct RedactedLog {
    pub text: String,
    pub warnings: Vec<Warning>,
}

pub struct Batch {
    pub outputs: Vec<RedactedLog>,
    mapping: String,
}

impl Batch {
    /// The private mapping includes original values. Keep it outside shared output.
    pub fn mapping_tsv(&self) -> &str {
        &self.mapping
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnonymizeError {
    // File indices are one-based.
    AliasUnavailable { file: usize, kind: Kind },
    Residual { file: usize, kind: Kind },
}

impl fmt::Display for AnonymizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AliasUnavailable { file, kind } => {
                write!(f, "入力 {file} の別名を生成できませんでした（{kind}）")
            }
            Self::Residual { file, kind } => {
                write!(f, "入力 {file} の出力に元の値が残っています（{kind}）")
            }
        }
    }
}

impl std::error::Error for AnonymizeError {}

/// Replace recognized spans and verify that their original values do not occur in any output.
pub fn anonymize(logs: &[&str], pc_name: Option<&str>) -> Result<Batch, AnonymizeError> {
    let pc_name = pc_name.filter(|name| !name.trim().is_empty());
    let scans: Vec<_> = logs.iter().map(|log| scan::scan(log, pc_name)).collect();
    let originals = OriginalValues::collect(logs, &scans, pc_name);
    let mut aliases = Aliases::default();
    let mut outputs = Vec::with_capacity(logs.len());
    for (index, (&log, scan)) in logs.iter().zip(scans).enumerate() {
        let mut text = String::with_capacity(log.len());
        let mut written = 0;
        for span in scan.spans {
            text.push_str(&log[written..span.range.start]);
            if span.kind != Kind::PcSerialBody {
                let original = &log[span.range.clone()];
                let alias = aliases.replace(span.kind, original, &originals).ok_or(
                    AnonymizeError::AliasUnavailable {
                        file: index + 1,
                        kind: span.kind,
                    },
                )?;
                text.push_str(alias);
            }
            written = span.range.end;
        }
        text.push_str(&log[written..]);
        if let Some(kind) = originals.find_in(&text) {
            return Err(AnonymizeError::Residual {
                file: index + 1,
                kind,
            });
        }
        outputs.push(RedactedLog {
            text,
            warnings: scan.warnings,
        });
    }
    Ok(Batch {
        outputs,
        mapping: aliases.into_tsv(),
    })
}

struct OriginalValues<'a>(BTreeSet<(Kind, Cow<'a, str>)>);

impl<'a> OriginalValues<'a> {
    fn collect(logs: &[&'a str], scans: &[scan::Scan], pc_name: Option<&'a str>) -> Self {
        let mut values = BTreeSet::new();
        if let Some(pc_name) = pc_name {
            values.insert((Kind::PcName, normalized(Kind::PcName, pc_name)));
        }
        for (&log, scan) in logs.iter().zip(scans) {
            for span in &scan.spans {
                let value = &log[span.range.clone()];
                if span.kind != Kind::PcSerialBody || !value.trim().is_empty() {
                    values.insert((span.kind, normalized(span.kind, value)));
                }
            }
        }
        Self(values)
    }

    fn find_in(&self, text: &str) -> Option<Kind> {
        let mut folded = None;
        self.0.iter().find_map(|(kind, original)| {
            let search: &str = if kind.ignores_ascii_case() {
                folded.get_or_insert_with(|| text.to_ascii_lowercase())
            } else {
                text
            };
            search.contains(original.as_ref()).then_some(*kind)
        })
    }

    fn unused_symbols(&self) -> Option<[char; 2]> {
        let mut unused = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789仮置換別名記号印札"
            .chars()
            .filter(|&symbol| {
                self.0.iter().all(|(kind, value)| {
                    let symbol = if kind.ignores_ascii_case() {
                        symbol.to_ascii_lowercase()
                    } else {
                        symbol
                    };
                    !value.contains(symbol)
                })
            });
        Some([unused.next()?, unused.next()?])
    }
}

fn normalized(kind: Kind, original: &str) -> Cow<'_, str> {
    if kind.ignores_ascii_case() {
        Cow::Owned(original.to_ascii_lowercase())
    } else {
        Cow::Borrowed(original)
    }
}

#[derive(Default)]
struct Aliases<'a> {
    next: BTreeMap<&'static str, usize>,
    aliases: BTreeMap<(Kind, Cow<'a, str>), String>,
    originals: BTreeMap<(Kind, &'a str), String>,
    fallback_symbols: Option<[char; 2]>,
    fallback_next: usize,
}

impl<'a> Aliases<'a> {
    fn replace(
        &mut self,
        kind: Kind,
        original: &'a str,
        forbidden: &OriginalValues<'_>,
    ) -> Option<&str> {
        let source = (kind, original);
        if !self.originals.contains_key(&source) {
            let key = (kind, normalized(kind, original));
            let alias = if let Some(alias) = self.aliases.get(&key) {
                alias.clone()
            } else {
                let alias = self.new_alias(kind, forbidden)?;
                self.aliases.insert(key, alias.clone());
                alias
            };
            self.originals.insert(source, alias);
        }
        self.originals.get(&source).map(String::as_str)
    }

    fn new_alias(&mut self, kind: Kind, forbidden: &OriginalValues<'_>) -> Option<String> {
        for prefix in [kind.alias_prefix(), "ID"] {
            if forbidden.find_in(prefix).is_some() {
                continue;
            }
            let next = self.next.entry(prefix).or_default();
            // Bound the search when a short original blocks many suffixes.
            for _ in 0..26 {
                let suffix = match b"ABCDEFGHIJKLMNOPQRSTUVWXYZ".get(*next) {
                    Some(&letter) => char::from(letter).to_string(),
                    None => next.checked_add(1)?.to_string(),
                };
                *next = next.checked_add(1)?;
                let alias = format!("{prefix}-{suffix}");
                if forbidden.find_in(&alias).is_none() {
                    return Some(alias);
                }
            }
        }

        // An alphabet absent from all originals yields a unique alias for every
        // ordinal without searching. These aliases cannot collide with the
        // hyphenated aliases above.
        let symbols = self
            .fallback_symbols
            .or_else(|| forbidden.unused_symbols())?;
        self.fallback_symbols = Some(symbols);
        let index = self.fallback_next;
        self.fallback_next = index.checked_add(1)?;
        Some(
            format!("{index:b}")
                .bytes()
                .map(|digit| symbols[usize::from(digit - b'0')])
                .collect(),
        )
    }

    fn into_tsv(self) -> String {
        use fmt::Write;

        let mut map = String::from("kind\toriginal\treplacement\n");
        for ((kind, original), alias) in self.originals {
            let _ = writeln!(
                map,
                "{}\t{}\t{alias}",
                kind.alias_prefix(),
                original.escape_default()
            );
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_an_ssid_equal_to_its_preferred_alias_across_files() {
        let batch = anonymize(&["ssid: WIFI-A\r\n", "ssid: WIFI-A\r\n"], None)
            .expect("use a different alias for the known value");
        assert_eq!(batch.outputs[0].text, "ssid: WIFI-B\r\n");
        assert_eq!(batch.outputs[1].text, batch.outputs[0].text);
        assert!(batch.mapping_tsv().contains("WIFI\tWIFI-A\tWIFI-B\n"));
    }

    #[test]
    fn screens_candidates_against_other_kinds_and_the_explicit_pc_name() {
        let batch = anonymize(&["ssid: synthetic-only\n", "psk: WIFI-A\n"], Some("wifi-b"))
            .expect("screen aliases against originals from the entire batch");
        assert_eq!(batch.outputs[0].text, "ssid: WIFI-C\n");
        assert_eq!(batch.outputs[1].text, "psk: WIFI-SECRET-A\n");
    }

    #[test]
    fn shares_fallback_prefix_counters_across_kinds() {
        let batch = anonymize(&["ssid: WIFI\npassword: synthetic-only\n"], None)
            .expect("a value can block a whole prefix");
        assert_eq!(batch.outputs[0].text, "ssid: ID-A\npassword: ID-B\n");
    }

    #[test]
    fn uses_distinct_fallback_symbols_if_an_original_blocks_every_hyphenated_alias() {
        let batch = anonymize(&["ssid: WIFI\npassword: ID\npsk: -\n"], None)
            .expect("choose aliases without an unsafe separator");
        assert_eq!(batch.outputs[0].text, "ssid: A\npassword: B\npsk: BA\n");
    }

    #[test]
    fn detects_a_genuine_residual_against_originals_in_later_files() {
        let result = anonymize(
            &[
                "unknown format with synthetic-only-ssid\n",
                "ssid: synthetic-only-ssid\n",
            ],
            None,
        );
        assert!(matches!(
            result,
            Err(AnonymizeError::Residual {
                file: 1,
                kind: Kind::WifiName
            })
        ));
    }
}
