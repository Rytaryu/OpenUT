use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/dongle/m2-3-cli.jsonl"
);

fn run(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openut-dongle"))
        .args(args)
        .output()
        .expect("run the built offline tool")
}

fn check_private_values_are_absent(output: &str) {
    for private in [
        "FAKE_SERIAL_CLI_TEST",
        "FAKE_WIFI_NAME_CLI_TEST",
        "FAKE_WIFI_PASS_CLI_TEST",
        "FAKE_FIRMWARE_CLI_TEST",
        "7213345576a9",
        "13345576a9",
        "7404274766a8",
    ] {
        assert!(!output.contains(private), "unexpected raw test value");
    }
}

#[test]
fn decode_writes_one_safe_object_per_report_in_stream_order() {
    let output = run(["decode", RECORD]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("output is UTF-8");
    check_private_values_are_absent(&stdout);
    let events: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("each report produces a JSON line"))
        .collect();
    assert_eq!(events.len(), 12);
    assert!(events.iter().all(Value::is_object));
    assert_eq!(events[0]["type"], "pose");
    assert_eq!(events[0]["tracker"], "A");
    assert!(events[0].get("extra").is_none());
    assert_eq!(events[1]["type"], "pair");
    assert_eq!(events[1]["tracker"], "B");
    assert_eq!(events[2]["message"]["field"], "NADS");
    assert!(events[2]["message"].get("value").is_none());
    assert_eq!(events[0]["mono_us"], 10_000_000);
    assert_eq!(events[11]["mono_us"], 11_100_000);
}

#[test]
fn summary_prints_receiving_intervals_and_pairing_only_count() {
    let output = run(["summary", RECORD, "--utc-offset", "+9"]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("output is UTF-8");
    check_private_values_are_absent(&stdout);
    for expected in [
        "トラッカー A",
        "トラッカー B",
        "受信中: 10.000 秒",
        "13.100 秒",
        "姿勢 3 件、文字列 7 件",
        "2 (追跡中) 2 件、3 (喪失、向きだけ有効) 1 件",
        "地図の状態: MS=4、P61:5=7",
        "取りこぼし（推定）: 6 件",
        "不明 1 件",
        "ペアリングの知らせだけに現れた名前: 1",
        "時刻が戻った行 (time_reversed): 1",
        "09:00:10",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} from {stdout}"
        );
    }
}

#[test]
fn summary_writes_the_console_report_as_utf8_without_a_bom() {
    let stdout = run(["summary", RECORD, "--utc-offset", "+9"]);
    assert!(stdout.status.success(), "{:?}", stdout.stderr);
    let report = std::str::from_utf8(&stdout.stdout).expect("console report is UTF-8");
    assert!(report.contains("トラッカー A"));
    assert!(report.contains("09:00:10"));

    let scratch = TempDir::new();
    for out_first in [true, false] {
        let destination = scratch.0.join(format!("集計-{out_first}.txt"));
        let path = destination.as_os_str();
        let output_option = [OsStr::new("--out"), path];
        let offset_option = [OsStr::new("--utc-offset"), OsStr::new("+9")];
        let (first, second) = if out_first {
            (output_option, offset_option)
        } else {
            (offset_option, output_option)
        };
        let output = run([OsStr::new("summary"), OsStr::new(RECORD)]
            .into_iter()
            .chain(first)
            .chain(second));
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let saved = fs::read(destination).expect("read the saved report");
        assert!(!saved.starts_with(b"\xef\xbb\xbf"));
        assert_eq!(saved, stdout.stdout);
    }
}

#[test]
fn summary_preserves_existing_output_and_the_input_record() {
    let scratch = TempDir::new();
    let input = scratch.0.join("input.jsonl");
    let original = fs::read(RECORD).expect("read the synthetic recording");
    fs::write(&input, &original).expect("write test input");
    let destination = scratch.0.join("summary.txt");
    let existing = "保存済みの集計\n";
    fs::write(&destination, existing).expect("write a preexisting report");

    for output_path in [&destination, &input] {
        let output = run([
            OsStr::new("summary"),
            input.as_os_str(),
            OsStr::new("--out"),
            output_path.as_os_str(),
        ]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(fs::read(&input).expect("read the input"), original);
    assert_eq!(
        fs::read_to_string(destination).expect("read the preexisting report"),
        existing
    );
}

#[test]
fn summary_does_not_create_an_output_for_an_invalid_recording() {
    let scratch = TempDir::new();
    let input = scratch.0.join("invalid.jsonl");
    fs::write(&input, "invalid header\n").expect("write an invalid recording");
    let destination = scratch.0.join("summary.txt");
    let output = run([
        OsStr::new("summary"),
        input.as_os_str(),
        OsStr::new("--out"),
        destination.as_os_str(),
    ]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    assert!(!destination.exists());
}

#[test]
fn summary_rejects_missing_or_duplicate_out_arguments_without_writing() {
    let scratch = TempDir::new();
    for args in [
        &["--out"][..],
        &["--out", "--utc-offset"][..],
        &["--out", "first.txt", "--out", "second.txt"][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_openut-dongle"))
            .current_dir(&scratch.0)
            .args(["summary", RECORD])
            .args(args)
            .output()
            .expect("run the built offline tool");
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(fs::read_dir(&scratch.0).unwrap().next().is_none());
    }
}

#[test]
fn rejects_bad_offsets_and_keeps_file_paths_private_in_errors() {
    assert_eq!(
        run(["summary", RECORD, "--utc-offset", "+24"])
            .status
            .code(),
        Some(1)
    );
    let missing = "missing-private-path-cli-test.jsonl";
    let output = run(["decode", missing]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(missing));
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "openut-summary-test-{}-{}",
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
