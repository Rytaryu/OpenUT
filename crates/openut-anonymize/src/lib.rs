//! Anonymize a batch of Hub logs with aliases shared across its files.

use std::collections::BTreeMap;
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
pub struct Residual {
    /// One-based index in the input list.
    pub file: usize,
    pub kind: Kind,
}

impl fmt::Display for Residual {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "入力 {} の出力に元の値が残っています（{}）",
            self.file, self.kind
        )
    }
}

impl std::error::Error for Residual {}

/// Replace recognized spans and verify that their original values do not occur in any output.
pub fn anonymize(logs: &[&str], pc_name: Option<&str>) -> Result<Batch, Residual> {
    let pc_name = pc_name.filter(|name| !name.trim().is_empty());
    let mut aliases = Aliases::default();
    let outputs: Vec<_> = logs
        .iter()
        .map(|log| {
            let scan = scan::scan(log, pc_name);
            let mut text = String::with_capacity(log.len());
            let mut written = 0;
            for span in scan.spans {
                text.push_str(&log[written..span.range.start]);
                let original = &log[span.range.clone()];
                if span.kind != Kind::PcSerialBody || !original.trim().is_empty() {
                    text.push_str(aliases.replace(span.kind, original));
                }
                written = span.range.end;
            }
            text.push_str(&log[written..]);
            RedactedLog {
                text,
                warnings: scan.warnings,
            }
        })
        .collect();
    aliases.verify(&outputs, pc_name)?;
    Ok(Batch {
        outputs,
        mapping: aliases.into_tsv(),
    })
}

#[derive(Default)]
struct Aliases<'a> {
    next: BTreeMap<Kind, usize>,
    aliases: BTreeMap<(Kind, String), String>,
    originals: BTreeMap<(Kind, &'a str), String>,
}

impl<'a> Aliases<'a> {
    fn replace(&mut self, kind: Kind, original: &'a str) -> &str {
        self.originals.entry((kind, original)).or_insert_with(|| {
            if kind == Kind::PcSerialBody {
                return String::new();
            }
            let key = if kind.ignores_ascii_case() {
                original.to_ascii_lowercase()
            } else {
                original.to_owned()
            };
            self.aliases
                .entry((kind, key))
                .or_insert_with(|| {
                    let index = self.next.entry(kind).or_default();
                    let suffix = match b"ABCDEFGHIJKLMNOPQRSTUVWXYZ".get(*index) {
                        Some(&letter) => char::from(letter).to_string(),
                        None => (*index + 1).to_string(),
                    };
                    *index += 1;
                    format!("{}-{suffix}", kind.alias_prefix())
                })
                .clone()
        })
    }

    fn verify(&self, outputs: &[RedactedLog], pc_name: Option<&str>) -> Result<(), Residual> {
        let pc_name = pc_name.map(str::to_ascii_lowercase);
        for (index, output) in outputs.iter().enumerate() {
            let mut folded = None;
            if let Some(pc_name) = &pc_name
                && folded
                    .get_or_insert_with(|| output.text.to_ascii_lowercase())
                    .contains(pc_name)
            {
                return Err(Residual {
                    file: index + 1,
                    kind: Kind::PcName,
                });
            }
            for &(kind, original) in self.originals.keys() {
                let remains = if kind.ignores_ascii_case() {
                    folded
                        .get_or_insert_with(|| output.text.to_ascii_lowercase())
                        .contains(&original.to_ascii_lowercase())
                } else {
                    output.text.contains(original)
                };
                if remains {
                    return Err(Residual {
                        file: index + 1,
                        kind,
                    });
                }
            }
        }
        Ok(())
    }

    fn into_tsv(self) -> String {
        use fmt::Write;

        let mut map = String::from("kind\toriginal\treplacement\n");
        for ((kind, original), alias) in self.originals {
            if kind != Kind::PcSerialBody {
                let _ = writeln!(
                    map,
                    "{}\t{}\t{alias}",
                    kind.alias_prefix(),
                    original.escape_default()
                );
            }
        }
        map
    }
}
