use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use openut_dongle::anonymize::anonymize_recording;
use openut_dongle::replay::Replay;
use openut_dongle::{summary, tracker::Transition};
use serde_json::{Value, json};

const RECORD: &str = include_str!("../../../tests/fixtures/dongle/m2-3-cli.jsonl");

fn rows(record: &str) -> Vec<Value> {
    record
        .lines()
        .map(|line| serde_json::from_str(line).expect("synthetic JSON row"))
        .collect()
}

fn jsonl(rows: &[Value]) -> String {
    rows.iter().map(|row| format!("{row}\n")).collect()
}

fn transitions(record: &[u8]) -> Vec<Transition> {
    let mut replay = Replay::new(Cursor::new(record)).expect("recording header");
    let mut changes = Vec::new();
    while let Some(step) = replay.read_step().expect("recording row") {
        changes.extend(step.transitions);
    }
    changes
}

#[test]
fn reconstructs_the_record_and_preserves_activity_across_omissions() {
    let mut input = rows(RECORD);
    input.insert(
        2,
        json!({"type":"in","mono_us":10_000_000,"wall_ms":1764288010000_i64,
               "hex":"2844007443254766a801010a4e41"}),
    ); // Full 0x28 header and a truncated text body for a third synthetic tracker.
    input.insert(
        3,
        json!({"type":"note","mono_us":10_000_000,"wall_ms":1764288010000_i64,
               "text":"unsupported_synthetic_note"}),
    );
    let source = jsonl(&input);
    let mapping = "kind\toriginal\treplacement\nTRACKER\tFAKE_SERIAL_CLI_TEST\tTRACKER-Q\n";
    let export = anonymize_recording(Cursor::new(&source), Some(mapping)).expect("verified export");
    let shared = std::str::from_utf8(export.as_bytes()).expect("UTF-8 export");
    let rebuilt = rows(shared);
    assert_eq!(rebuilt[0]["anonymized"], true);
    assert_eq!(export.skipped_rows(), 1);
    assert!(!shared.contains("unsupported_synthetic_note"));
    assert!(!shared.contains("7213345576a9"));
    assert!(!shared.contains("46414b455f53455249414c"));
    assert!(shared.contains("545241434b45522d51")); // The supplied same-length alias suffix.
    assert!(rebuilt.last().expect("footer")["omitted"].is_array());
    assert_eq!(
        transitions(source.as_bytes()),
        transitions(export.as_bytes())
    );
}

#[test]
fn fixed_width_aliases_follow_the_same_mapping_across_recordings() {
    let mapping = concat!(
        "kind\toriginal\treplacement\n",
        "WIFI\tq\tWIFI-A\n",
        "WIFI-SECRET\tl\tWIFI-SECRET-A\n",
        "WIFI\tl\tWIFI-27\n",
        "WIFI-SECRET\tq\tWIFI-SECRET-27\n",
        "WIFI\tr\tWIFI-37\n",
        "WIFI-SECRET\tz\tWIFI-SECRET-Z\n",
    );
    let notices = [
        ("01", "5748712c6c2c35"),
        ("02", "57486c2c712c35"),
        ("03", "5748722c7a2c35"),
    ];
    let by_sequence = |order: &[usize]| {
        let mut input = rows(RECORD);
        input.truncate(1);
        for &i in order {
            let (sequence, payload) = notices[i];
            input.push(json!({
                "type":"in",
                "mono_us": input.len(),
                "wall_ms": 1764288010000_i64,
                "hex": format!("28{sequence}00031486cafe21010107{payload}"),
            }));
        }
        let export = anonymize_recording(Cursor::new(jsonl(&input)), Some(mapping))
            .expect("verified export of a synthetic recording");
        let shared = std::str::from_utf8(export.as_bytes()).expect("UTF-8 export");
        let mut result = [String::new(), String::new(), String::new()];
        for (row, &i) in rows(shared).into_iter().skip(1).zip(order) {
            let raw = row["hex"].as_str().expect("retained text report");
            assert_eq!(raw.len(), 38);
            result[i] = raw[24..].to_owned();
        }
        result
    };
    let original_order = by_sequence(&[0, 1, 2]);
    assert_eq!(original_order, by_sequence(&[2, 1, 0]));
    assert_eq!(original_order[2], by_sequence(&[2])[2]);
    assert_ne!(original_order[1][4..6], original_order[2][4..6]);
}

#[test]
fn summary_uses_a_known_trackers_malformed_receipt_before_and_after_export() {
    let mut input = rows(RECORD);
    input.truncate(1);
    input.extend([
        json!({"type":"in","mono_us":100_000,"wall_ms":1764288000100_i64,
               "hex":"180c00000000031486cafe21"}),
        json!({"type":"in","mono_us":1_900_000,"wall_ms":1764288001900_i64,
               "hex":"282200031486cafe210101024e00"}),
        json!({"type":"tick","mono_us":4_100_000,"wall_ms":1764288004100_i64}),
    ]);
    let source = jsonl(&input);
    let export = anonymize_recording(Cursor::new(&source), None).expect("verified export");
    let summarize = |bytes: &[u8]| {
        let mut output = Vec::new();
        summary::write_summary(Cursor::new(bytes), &mut output, None)
            .expect("summarize a synthetic recording");
        String::from_utf8(output).expect("UTF-8 summary")
    };
    let raw = summarize(source.as_bytes());
    let shared = summarize(export.as_bytes());
    assert!(raw.contains("受信報告: 2 件、不明 0 件、不正 1 件、省略 0 件"));
    assert!(shared.contains("受信報告: 1 件、不明 0 件、不正 0 件、省略 1 件"));
    for output in [raw, shared] {
        assert!(output.contains("ペアリングの知らせだけに現れた名前: 0"));
        assert!(output.contains("見えた時間: 0.100 秒 (2025-11-28 00:00:00.100 UTC) 〜 1.900 秒"));
        assert!(output.contains("受信中: 0.100 秒 (2025-11-28 00:00:00.100 UTC) 〜 3.900 秒"));
    }
}

#[test]
fn cli_checks_before_creating_the_destination() {
    let dir = TempDir::new();
    let input = dir.0.join("source.jsonl");
    let output = dir.0.join("shared.jsonl");
    let mut source = rows(RECORD);
    source[0]["tool"] = json!("FAKE_SERIAL_CLI_TEST");
    fs::write(&input, jsonl(&source)).expect("create synthetic input");
    let result = export_cli(&input, &output);
    assert_eq!(result.status.code(), Some(5));
    assert!(!output.exists());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("FAKE_SERIAL_CLI_TEST"));

    fs::write(&input, RECORD).expect("restore synthetic input");
    let result = export_cli(&input, &output);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(
        fs::read_to_string(&output)
            .expect("written export")
            .contains("\"anonymized\":true")
    );
    assert_eq!(export_cli(&input, &output).status.code(), Some(4));
}

fn export_cli(input: &Path, output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openut-dongle"))
        .args(["export", "--anonymize"])
        .arg(input)
        .arg(output)
        .output()
        .expect("run the built export command")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "openut-export-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).expect("create test directory");
        Self(directory)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
