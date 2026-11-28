use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use openut::replay::Expectations;

const R1_CASE: &str = "synthetic-r1-second-host-command";
const R1_LOG: &str = "ViveTrackerServer_20261128.txt";
const MIDNIGHT_CASE: &str = "synthetic-midnight";
const PREVIOUS_LOG: &str = "ViveTrackerServer_20261127.txt";
const NEXT_LOG: &str = "ViveTrackerServer_20261128.txt";

#[test]
fn help_lists_watcher_commands_and_version_is_available() {
    let help = run(&mut command());
    for usage in ["openut watch", "openut replay", "openut summarize"] {
        assert!(help.contains(usage), "{help}");
    }
    assert_eq!(
        run(command().arg("--version")),
        format!("OpenUT {}\n", openut::VERSION)
    );
    assert!(run(command().args(["watch", "--help"])).contains("--log-dir"));
    let output = command().args(["watch", "--config"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn replay_compares_case_expectations_and_reports_unexpected_detections() {
    let case = fixture(R1_CASE);
    let output = run(command().arg("replay").arg(&case));
    assert!(output.contains("2026-11-28 12:00:25.600 R1"));

    let directory = TestDirectory::new();
    let expect = directory.0.join("expect.toml");
    fs::write(&expect, "check_rules = [\"R1\"]\n").unwrap();
    let output = command()
        .arg("replay")
        .arg(&case)
        .arg("--expect")
        .arg(&expect)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostic.contains("余分な検出"), "{diagnostic}");
    assert!(
        diagnostic.contains("2026-11-28 12:00:25.600"),
        "{diagnostic}"
    );
}

#[test]
fn replay_applies_threshold_overrides_and_round_trips_toml() {
    let directory = TestDirectory::new();
    let source = fixture(R1_CASE).join(R1_LOG);
    let expect = directory.0.join("expect.toml");
    let toml = run(command()
        .arg("replay")
        .arg(&source)
        .args(["--format", "toml"]));
    assert!(toml.contains("[[detection]]"), "{toml}");
    assert!(toml.contains("2026-11-28 12:00:25.601"), "{toml}");
    Expectations::parse(&toml).unwrap();
    fs::write(&expect, toml).unwrap();
    run(command()
        .arg("replay")
        .arg(&source)
        .arg("--expect")
        .arg(&expect));

    let quiet = run(command().arg("replay").arg(&source).args([
        "--set",
        "rules.r1.count=1000",
        "--format",
        "toml",
    ]));
    assert!(!quiet.contains("rule = \"R1\""), "{quiet}");
    assert!(quiet.contains("rule = \"R3\""), "{quiet}");
    let parsed = Expectations::parse(&quiet).unwrap();
    assert_eq!(parsed.rules["r1"]["count"].as_integer(), Some(1_000));
    fs::write(&expect, quiet).unwrap();
    run(command()
        .arg("replay")
        .arg(&source)
        .arg("--expect")
        .arg(&expect));
}

#[test]
fn replay_enables_r5_and_uses_the_offset_at_the_detection_deadline() {
    let directory = TestDirectory::new();
    let input = directory.0.join(PREVIOUS_LOG);
    let fixture = fs::read_to_string(fixture(MIDNIGHT_CASE).join(PREVIOUS_LOG)).unwrap();
    let (prefix, _) = fixture.split_once("\n[2026-11-27 23:50:00.000").unwrap();
    let pose = prefix.lines().last().unwrap();
    let lost = pose
        .replace("18:00:01.000", "18:00:02.000")
        .replace("status: 2", "status: 3");
    // The recovery is 60.001 s after the loss, with a different GMT offset.
    let recovered = pose.replace("18:00:01.000 +9", "17:01:02.001 +8");
    fs::write(&input, format!("{prefix}\n{lost}\n{recovered}\n")).unwrap();

    let disabled = run(command().arg("replay").arg(&input));
    assert!(!disabled.contains("R5"), "{disabled}");
    let expect = directory.0.join("expect.toml");
    fs::write(
        &expect,
        concat!(
            "check_rules = [\"R5\"]\n",
            "[replay]\ncatch_up = true\nflush_s = 0\n",
            "[rules]\nr5.enabled = false\n",
            "[[detection]]\nrule = \"R5\"\ntracker = 1\n",
            "after = \"2026-11-27 18:01:02.000\"\n",
            "before = \"2026-11-27 18:01:02.001\"\n",
            "[summary.\"2026-11-27\"]\n\"detections.R5\" = 1\n",
        ),
    )
    .unwrap();
    let enabled = run(command()
        .arg("replay")
        .arg(&input)
        .arg("--expect")
        .arg(&expect)
        .args(["--set", "rules.r5.enabled=true", "--format", "toml"]));
    assert!(enabled.contains("18:01:02.000"), "{enabled}");
    let generated = Expectations::parse(&enabled).unwrap();
    assert!(generated.replay.catch_up);
    assert_eq!(generated.replay.flush_s, 0);
    assert_eq!(generated.rules["r5"]["enabled"].as_bool(), Some(true));
}

#[test]
fn summarize_writes_both_days_and_preserves_final_output() {
    let directory = TestDirectory::new();
    let case = fixture(MIDNIGHT_CASE);
    let earlier = case.join(PREVIOUS_LOG);
    let later = case.join(NEXT_LOG);
    let output = directory.0.join("summaries");
    let mut summarize = command();
    summarize
        .arg("summarize")
        .arg(&later)
        .arg(&earlier)
        .arg("--out")
        .arg(&output);
    run(&mut summarize);

    let previous = output.join("2026-11-27.json");
    let next = output.join("2026-11-28.json");
    let first = read_json(&previous);
    let second = read_json(&next);
    assert_eq!(first["final"], true);
    assert_eq!(first["observed_until"], "2026-11-28 00:00:00.000");
    assert_eq!(second["final"], false);
    assert_eq!(second["observed_until"], "2026-11-28 00:30:00.000");
    assert!(fs::read_dir(output).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        name == "2026-11-27.json" || name == "2026-11-28.json" || name == ".summary.lock"
    }));

    let mut preserved = first;
    preserved["host_starts"] = 42.into();
    fs::write(&previous, serde_json::to_vec(&preserved).unwrap()).unwrap();
    run(&mut summarize);
    assert_eq!(read_json(&previous), preserved);
    assert_eq!(read_json(&next), second);
}

#[test]
fn summarize_keeps_the_input_folder_read_only() {
    let directory = TestDirectory::new();
    let input = directory.0.join(NEXT_LOG);
    let nested = directory.0.join("nested");
    let source = fs::read(fixture(MIDNIGHT_CASE).join(NEXT_LOG)).unwrap();
    fs::write(&input, &source).unwrap();
    let output = command()
        .arg("summarize")
        .arg(&input)
        .arg("--out")
        .arg(&nested)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!nested.exists());
    assert_eq!(fs::read(&input).unwrap(), source);
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_openut"))
}

fn run(command: &mut Command) -> String {
    let Output {
        status,
        stdout,
        stderr,
    } = command.output().unwrap();
    assert!(status.success(), "{}", String::from_utf8_lossy(&stderr));
    String::from_utf8(stdout).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/replay")
        .join(name)
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "openut-cli-watch-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not make a test directory: {error}"),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
