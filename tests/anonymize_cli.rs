use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use openut_hublog::decode::{Decoder, LineEnding};

const TEST_PC: &str = "SYNTHETIC-PC";
const SERIAL: &str = "00ZZ000000111";
const OTHER_SERIAL: &str = "00ZZ000000222";
const UNKNOWN_SERIAL: &str = "FUTURE-HW001";
const SSID: &str = "synthetic-only private wifi";
const PASSWORD: &str = "synthetic-only password";
const PSK: &str = "synthetic-only-psk";
const USER_FOLDER: &str = "Synthetic Local User";
const PC_SERIAL_BODY: &str = "synthetic-only pc serial body";
const PC_SERIAL_TAIL: &str = "synthetic-only pc serial continuation";
const ADDRESS: &str = "02 00 00 FE ED 42";
const UNIVERSE_ID: &str = "1234567890123456789";
const MAP_NAME: &str = "openut-anonymize-map.tsv";

const CURRENT_UNIVERSE: &str = "[2026-11-27 22:53:30.000 +9 0x4D2 INFO OETServer::GetCurrentSteamVRChaperoneInfoAndStore] [1234] current universe id: ";
const UNIVERSE_QUERY: &str =
    "[2026-11-27 22:53:31.000 +9 0x4D2 INFO OETServer::OnPipeQueryData] [1234] ";

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "openut-anonymize-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
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

fn encode_utf16le(text: &str) -> Vec<u8> {
    std::iter::once(0xfeff)
        .chain(text.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn write_input(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, encode_utf16le(text)).expect("write synthetic log");
    path
}

fn read_output(dir: &Path, input: &Path) -> String {
    let name = input.file_name().expect("synthetic input has a filename");
    let bytes = fs::read(dir.join(name)).expect("read anonymized log");
    let payload = bytes.strip_prefix(&[0xff, 0xfe]).expect("UTF-16LE BOM");
    let (words, trailing) = payload.as_chunks::<2>();
    assert!(trailing.is_empty(), "partial UTF-16LE code unit");
    String::from_utf16(
        &words
            .iter()
            .copied()
            .map(u16::from_le_bytes)
            .collect::<Vec<_>>(),
    )
    .expect("decode anonymized log")
}

fn crlf_records(text: &str) -> Vec<String> {
    let mut decoder = Decoder::new();
    let mut records = decoder.push(&encode_utf16le(text));
    records.extend(decoder.finish());
    assert_eq!(decoder.line_ending(), Some(LineEnding::CrLf));
    records
}

fn anonymize_command(output: &Path, map: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_openut"));
    command
        .args(["anonymize", "--out-dir"])
        .arg(output)
        .arg("--map-dir")
        .arg(map);
    command
}

fn anonymize(output: &Path, map: &Path, files: &[&Path]) -> Output {
    anonymize_command(output, map)
        .args(["--pc-name", TEST_PC])
        .args(files)
        .output()
        .expect("run openut anonymize")
}

#[test]
fn stops_before_writing_when_the_pc_name_is_missing_or_blank() {
    let directory = TestDirectory::new();
    let input = write_input(
        &directory.0,
        "ViveTrackerServer_20261127.txt",
        "# WriteTimeInterval=5000\r\n",
    );
    let share = directory.0.join("share");
    let private = directory.0.join("private");
    for manual_name in [None, Some(" \t ")] {
        let mut command = anonymize_command(&share, &private);
        if let Some(name) = manual_name {
            // An invalid explicit value must fail even if the environment has a name.
            command
                .env("COMPUTERNAME", TEST_PC)
                .env("HOSTNAME", TEST_PC)
                .args(["--pc-name", name]);
        } else {
            command.env_remove("COMPUTERNAME").env_remove("HOSTNAME");
        }
        let result = command.arg(&input).output().expect("run openut anonymize");
        assert!(!result.status.success());
        let error = String::from_utf8(result.stderr).expect("UTF-8 diagnostic");
        assert!(error.contains("PC 名"));
        assert!(error.contains("--pc-name"));
        assert!(!share.exists());
        assert!(!private.exists());
    }
}

#[test]
fn uses_a_shared_alias_when_a_wifi_name_matches_the_first_candidate() {
    let directory = TestDirectory::new();
    let input_a = write_input(
        &directory.0,
        "ViveTrackerServer_20261127.txt",
        "ssid: \"WIFI-A\", retries: 1\r\n",
    );
    let input_b = write_input(
        &directory.0,
        "ViveTrackerServer_20261128.txt",
        "ssid: \"WIFI-A\", retries: 2\r\n",
    );
    let share = directory.0.join("share");
    let private = directory.0.join("private");
    let result = anonymize(&share, &private, &[&input_a, &input_b]);
    assert!(result.status.success(), "alias collision must be avoided");

    let first = read_output(&share, &input_a);
    let alias = first
        .strip_prefix("ssid: \"")
        .and_then(|value| value.strip_suffix("\", retries: 1\r\n"))
        .expect("keep the surrounding log fields");
    assert!(!alias.is_empty());
    assert!(!alias.contains("WIFI-A"));
    assert_eq!(
        read_output(&share, &input_b),
        format!("ssid: \"{alias}\", retries: 2\r\n")
    );
}

fn serial_fields(log: &str) -> Vec<&str> {
    log.match_indices("sn: (")
        .map(|(index, marker)| {
            log[index + marker.len()..]
                .split_once(')')
                .expect("serial field has its closing parenthesis")
                .0
        })
        .collect()
}

#[test]
fn refuses_to_put_the_mapping_in_the_share_directory_or_replace_a_file() {
    let directory = TestDirectory::new();
    let source = directory.0.join("input");
    fs::create_dir(&source).expect("create synthetic input folder");
    let input = write_input(
        &source,
        "ViveTrackerServer_20261127.txt",
        "# WriteTimeInterval=5000\r\n",
    );
    let share = directory.0.join("share");
    let nested = share.join("private");
    assert!(!anonymize(&share, &nested, &[&input]).status.success());
    assert!(!nested.join(MAP_NAME).exists());

    fs::create_dir_all(&share).expect("create output folder");
    let prior = share.join(input.file_name().expect("synthetic log name"));
    fs::write(&prior, b"previous output").expect("create existing output");
    let private = directory.0.join("private");
    assert!(!anonymize(&share, &private, &[&input]).status.success());
    assert_eq!(
        fs::read(prior).expect("read existing output"),
        b"previous output"
    );
    assert!(!private.join(MAP_NAME).exists());
}

#[test]
fn anonymizes_documented_fields_across_files_and_preserves_record_format() {
    let directory = TestDirectory::new();
    let output = directory.0.join("share");
    let map = directory.0.join("private");

    let response = "[2026-11-27 22:53:25.385 +9 0x82B0 INFO OETServer::GetDeviceInfoAndResponse] [33456] Response - cmd type: 214, cmd key: (TEST), device index: 2, result: 0, sn: ";
    let not_found = "[2026-11-27 22:53:28.522 +9 0xF88 ERROR OETServer::DeviceInfoCB] [3976] TRACKER NOT FOUND. tracker: ";
    let wifi = "[2026-11-27 22:53:30.000 +9 0xF88 INFO OETServer::ProcessDeviceMsg] [3976] ";
    let pc_serial = "[2026-11-27 22:53:31.000 +9 0xF88 INFO BiCollector::GetPcSerialNumber] [3976]";
    let server_header = "# Log begin at 2026-11-27 22:53:25.000";
    let driver_header = "# Log begin at 2026-11-27 19:07:26.866";
    let driver_timeout = "[2026-11-27 19:07:28.939 +9 0x3410 INFO TrackerDongleHID::DataDispatchThreadProc] [13328] Wait for HID data got timeout.\n";
    let driver_log = format!("{driver_header}\r\n{driver_timeout}\r\n");
    let map_creation = "[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::SaveCalibratedDataToFile] [3976] Save map creation time: Wed Nov 25 21:00:00 2026\n.";
    let ordinary = "[2026-11-27 22:53:33.658 +9 0xF88 WARN OETServer::ProcessDeviceMsg] [3976] Wifi info is not yet available.\r\n";
    let unknown = format!("{not_found}{UNKNOWN_SERIAL}\r\n");
    let ending = ", model name: (VIVE Ultimate Tracker 1), fw version: (1.0.999.178)\r\n";

    let server_log = format!(
        "{server_header}\r\n\
         {response}({SERIAL}){ending}\
         {response}({OTHER_SERIAL}){ending}\
         {wifi}ssid: \"{SSID}\", password: \"{PASSWORD}\", psk={PSK}, retries: 2\r\n\
         {wifi}machine: {TEST_PC}, peer: peer_synthetic-pc, path: (C:\\Users\\{USER_FOLDER}\\Steam\\config\\chaperone_info.vrchap)\r\n\
         {pc_serial} {PC_SERIAL_BODY}\r{PC_SERIAL_TAIL}\n\r\n\
         {ordinary}{map_creation}\r\n{unknown}unrecognized final record: 910",
    );
    let second_server_log = format!("{not_found}{SERIAL}\r\n");
    let dongle_log = format!(
        "[2026-11-27 22:47:49.306 +9 0x3410 INFO dumpPUMRFConnect] [13328]   - Change Status:    Connected, bt mac address: (#3)({ADDRESS})\r\n"
    );
    let server = write_input(&directory.0, "ViveTrackerServer_20261127.txt", &server_log);
    let next_day = write_input(
        &directory.0,
        "ViveTrackerServer_20261128.txt",
        &second_server_log,
    );
    let driver = write_input(&directory.0, "ViveTrackerDriver_20261127.txt", &driver_log);
    let dongle = write_input(&directory.0, "ViveTrackerDongle_20261127.txt", &dongle_log);
    let result = anonymize(&output, &map, &[&server, &next_day, &driver, &dongle]);
    assert!(result.status.success(), "batch anonymization failed");

    let sanitized = read_output(&output, &server);
    let aliases = serial_fields(&sanitized);
    assert_eq!(aliases.len(), 2);
    assert!(!aliases[0].is_empty());
    assert!(!aliases[1].is_empty());
    assert_ne!(aliases[0], aliases[1], "different trackers stay distinct");
    assert_eq!(
        read_output(&output, &next_day),
        format!("{not_found}{}\r\n", aliases[0]),
        "the same serial in a different file receives the same alias"
    );
    assert!(sanitized.contains(&format!("{response}({}){ending}", aliases[0])));
    assert!(sanitized.contains(ordinary));
    assert!(sanitized.contains(&unknown));
    assert!(sanitized.ends_with("unrecognized final record: 910"));
    assert!(sanitized.contains(", retries: 2\r\n"));
    assert!(sanitized.contains("\\Steam\\config\\chaperone_info.vrchap)\r\n"));
    let server_before = crlf_records(&server_log);
    let server_after = crlf_records(&sanitized);
    assert_eq!(server_before.len(), 10);
    assert_eq!(server_after.len(), server_before.len());
    assert_eq!(server_after[0], server_header);
    assert_eq!(server_after[5], format!("{pc_serial} \r \n"));
    assert_eq!(server_after[7], map_creation);
    let sanitized_driver = read_output(&output, &driver);
    assert_eq!(sanitized_driver, driver_log);
    assert_eq!(
        crlf_records(&sanitized_driver),
        [driver_header, driver_timeout]
    );

    let sanitized_dongle = read_output(&output, &dongle);
    assert!(sanitized_dongle.contains("bt mac address: (#3)("));
    assert!(sanitized_dongle.ends_with(")\r\n"));
    assert!(!sanitized_dongle.contains(ADDRESS));

    let stdout = String::from_utf8_lossy(&result.stdout).to_ascii_lowercase();
    let stderr = String::from_utf8_lossy(&result.stderr).to_ascii_lowercase();
    assert!(
        !stderr.is_empty(),
        "unknown serial shape should report a warning"
    );
    assert!(!stderr.contains(&UNKNOWN_SERIAL.to_ascii_lowercase()));
    let sanitized = sanitized.to_ascii_lowercase();
    for original in [
        SERIAL,
        OTHER_SERIAL,
        SSID,
        PASSWORD,
        PSK,
        TEST_PC,
        USER_FOLDER,
        PC_SERIAL_BODY,
        PC_SERIAL_TAIL,
        ADDRESS,
    ] {
        let original = original.to_ascii_lowercase();
        assert!(!sanitized.contains(&original));
        assert!(!stdout.contains(&original));
        assert!(!stderr.contains(&original));
    }

    assert!(!output.join(MAP_NAME).exists());
    let private = fs::read_to_string(map.join(MAP_NAME)).expect("read private mapping");
    assert!(private.contains(SERIAL));
    assert!(private.contains(aliases[0]));
    assert_eq!(fs::read_dir(&output).expect("list output").count(), 4);
}

#[test]
fn rejects_a_known_original_left_in_an_unrecognized_record() {
    let directory = TestDirectory::new();
    let output = directory.0.join("share");
    let map = directory.0.join("private");
    let original = "original-synthetic-residual-ssid";
    let first = format!(
        "[2026-11-27 22:53:30.000 +9 0xF88 INFO OETServer::ProcessDeviceMsg] [3976] ssid: {original}\r\n"
    );
    let second = format!("unrecognized format containing {original}\r\n");
    let server = write_input(&directory.0, "ViveTrackerServer_20261127.txt", &first);
    let driver = write_input(&directory.0, "ViveTrackerDriver_20261127.txt", &second);
    let result = anonymize(&output, &map, &[&server, &driver]);

    assert!(
        !result.status.success(),
        "a known residual should fail the batch"
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains(original));
    assert!(!String::from_utf8_lossy(&result.stderr).contains(original));
    for file in [server, driver] {
        let name = file.file_name().expect("synthetic input has a filename");
        assert!(!output.join(name).exists());
    }
    assert!(!map.join(MAP_NAME).exists());
}

#[test]
fn anonymizes_universe_ids_across_files_and_preserves_zero_and_log_shape() {
    let directory = TestDirectory::new();
    let output = directory.0.join("share");
    let map = directory.0.join("private");
    let header = "# Log begin at 2026-11-27 22:53:29.000";
    let program_path = r"C:\Program Files (x86)\Steam\config\oculus\driver_oculus.vrchap";
    let profile_path = format!(r"C:\Users\{USER_FOLDER}\Steam\config\oculus\driver_oculus.vrchap");
    let query_text = |from: &str, to: &str, local: &str, path: &str| {
        format!(
            "{UNIVERSE_QUERY}Universe id from ({from}) to ({to}), local's: ({local}), chaperone file path: ({path})"
        )
    };
    let no_number = format!("{UNIVERSE_QUERY}Universe ID is the same with local, do nothing.");
    let server_text = format!(
        "{header}\r\n{CURRENT_UNIVERSE}{UNIVERSE_ID}\r\n{}\r\n{}\r\n{no_number}\r\n",
        query_text("0", UNIVERSE_ID, "0", program_path),
        query_text(UNIVERSE_ID, UNIVERSE_ID, UNIVERSE_ID, &profile_path),
    );
    let next_day_prefix = CURRENT_UNIVERSE.replace("2026-11-27", "2026-11-28");
    let server = write_input(&directory.0, "ViveTrackerServer_20261127.txt", &server_text);
    let next_day = write_input(
        &directory.0,
        "ViveTrackerServer_20261128.txt",
        &format!("{next_day_prefix}{UNIVERSE_ID}\r\n{next_day_prefix}0\r\n"),
    );
    let result = anonymize(&output, &map, &[&server, &next_day]);
    assert!(result.status.success(), "universe ID anonymization failed");
    assert!(
        result.stderr.is_empty(),
        "recognized values need no warning"
    );

    let sanitized = read_output(&output, &server);
    let records = crlf_records(&sanitized);
    let alias = records[1]
        .strip_prefix(CURRENT_UNIVERSE)
        .expect("the current universe ID stays in its original field");
    assert!(!alias.is_empty());
    assert_ne!(alias, "0");
    let profile_start = r", chaperone file path: (C:\Users\";
    let profile_end = r"\Steam\config\oculus\driver_oculus.vrchap)";
    let (_, folder) = records[3]
        .split_once(profile_start)
        .expect("the path still starts with the users directory");
    let folder_alias = folder
        .strip_suffix(profile_end)
        .expect("preserve the rest of the path");
    assert!(!folder_alias.is_empty());
    let sanitized_path =
        format!(r"C:\Users\{folder_alias}\Steam\config\oculus\driver_oculus.vrchap");
    assert_eq!(
        sanitized,
        format!(
            "{header}\r\n{CURRENT_UNIVERSE}{alias}\r\n{}\r\n{}\r\n{no_number}\r\n",
            query_text("0", alias, "0", program_path),
            query_text(alias, alias, alias, &sanitized_path),
        )
    );
    assert_eq!(
        read_output(&output, &next_day),
        format!("{next_day_prefix}{alias}\r\n{next_day_prefix}0\r\n"),
    );
    assert!(!sanitized.contains(UNIVERSE_ID));
    assert!(!sanitized.contains(USER_FOLDER));
    assert!(!String::from_utf8_lossy(&result.stdout).contains(UNIVERSE_ID));
    assert!(!output.join(MAP_NAME).exists());
    let private = fs::read_to_string(map.join(MAP_NAME)).expect("read private mapping");
    let expected_mapping = format!("UNIVERSE\t{UNIVERSE_ID}\t{alias}");
    assert!(private.lines().any(|line| line == expected_mapping));
    assert!(
        !private
            .lines()
            .any(|line| line.starts_with("UNIVERSE\t0\t"))
    );
}

#[test]
fn allows_a_known_universe_number_in_a_parsed_record_header() {
    let directory = TestDirectory::new();
    let output = directory.0.join("share");
    let map = directory.0.join("private");
    let prefix = "[2026-11-27 00:00:02.000 +9 0x2 INFO OETServer::GetCurrentSteamVRChaperoneInfoAndStore] [2] current universe id: ";
    let server = write_input(
        &directory.0,
        "ViveTrackerServer_20261127.txt",
        &format!("{prefix}2\r\n"),
    );

    let result = anonymize(&output, &map, &[&server]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        read_output(&output, &server),
        format!("{prefix}UNIVERSE-A\r\n")
    );
}

#[test]
fn rejects_a_short_known_universe_number_outside_a_parsed_header() {
    let directory = TestDirectory::new();
    let output = directory.0.join("share");
    let map = directory.0.join("private");
    let server = write_input(
        &directory.0,
        "ViveTrackerServer_20261127.txt",
        "[2026-11-27 00:00:02.000 +9 0x1 INFO OETServer::GetCurrentSteamVRChaperoneInfoAndStore] [1] current universe id: 2\r\n",
    );
    for line in [
        "[2026-11-27 00:00:02.001 +9 0x2 INFO Driver] [2] details: 00002\r\n",
        "[2026-11-27 00:00:61.000 +9 0x1 INFO Driver] [00002] details\r\n",
    ] {
        let driver = write_input(&directory.0, "ViveTrackerDriver_20261127.txt", line);
        let result = anonymize(&output, &map, &[&server, &driver]);

        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("入力 2 の出力に元の値が残っています"),
            "{error}"
        );
        assert!(!error.contains("00002"));
        assert!(!output.exists());
        assert!(!map.exists());
    }
}

#[test]
fn rejects_an_unredacted_known_universe_id_in_another_file() {
    let directory = TestDirectory::new();
    let output = directory.0.join("share");
    let map = directory.0.join("private");
    let server = write_input(
        &directory.0,
        "ViveTrackerServer_20261127.txt",
        &format!("{CURRENT_UNIVERSE}{UNIVERSE_ID}\r\n"),
    );
    let driver = write_input(
        &directory.0,
        "ViveTrackerDriver_20261127.txt",
        &format!("unrecognized format containing {UNIVERSE_ID}\r\n"),
    );
    let result = anonymize(&output, &map, &[&server, &driver]);
    assert!(
        !result.status.success(),
        "a known residual should fail the batch"
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains(UNIVERSE_ID));
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("出力に元の値が残っています"));
    assert!(!error.contains(UNIVERSE_ID));
    assert!(!output.exists());
    assert!(!map.exists());
}
