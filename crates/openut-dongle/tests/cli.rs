use std::process::{Command, Output};

use serde_json::Value;

const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/dongle/m2-3-cli.jsonl"
);

fn run(args: &[&str]) -> Output {
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
    let output = run(&["decode", RECORD]);
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
    let output = run(&["summary", RECORD, "--utc-offset", "+9"]);
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
fn rejects_bad_offsets_and_keeps_file_paths_private_in_errors() {
    assert_eq!(
        run(&["summary", RECORD, "--utc-offset", "+24"])
            .status
            .code(),
        Some(1)
    );
    let missing = "missing-private-path-cli-test.jsonl";
    let output = run(&["decode", missing]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(missing));
}
