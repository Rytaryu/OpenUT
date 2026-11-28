use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use openut_dongle::anonymize::anonymize_recording;
use openut_dongle::replay::Replay;
use openut_dongle::tracker::Transition;
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
