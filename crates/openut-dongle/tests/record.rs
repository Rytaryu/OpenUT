use std::collections::BTreeSet;
use std::io::{BufRead, Cursor};

use openut_dongle::record::{
    Device, EndReason, Header, Mode, Note, OmittedReason, RawReport, ReadError, Record,
    RecordReader, RecordWriter, TrackerLabel,
};
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("../../../tests/fixtures/dongle/m2-1-record.jsonl");

fn header() -> Header {
    Header {
        tool: "openut-dongle synthetic".to_owned(),
        mode: Mode::Record,
        started_wall_ms: 1_764_302_512_123,
        device: Device {
            vid: 1,
            pid: 2,
            interface: 0,
            usage_page: 0xff00,
            usage: 1,
        },
        server_running_at_start: false,
        anonymized: true,
    }
}

fn tick(mono_us: u64, wall_ms: i64) -> Record {
    Record::Tick { mono_us, wall_ms }
}

fn write(header: &Header, records: &[Record]) -> Vec<u8> {
    let mut writer = RecordWriter::new(Vec::new(), header).expect("write header");
    for record in records {
        writer.write_record(record).expect("write record");
    }
    writer.flush().expect("flush capture");
    writer.into_inner()
}

fn read_rows<R: BufRead>(reader: &mut RecordReader<R>) -> Vec<Record> {
    let mut rows = Vec::new();
    while let Some(row) = reader.read_record().expect("read record") {
        rows.push(row);
    }
    rows
}

fn json_rows(bytes: &[u8]) -> Vec<Value> {
    let recorded = std::str::from_utf8(bytes).expect("UTF-8 JSON Lines");
    assert!(recorded.ends_with('\n'), "writer finishes each line");
    recorded
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON value per line"))
        .collect()
}

#[test]
fn reads_synthetic_fixture_and_round_trips_records() {
    let expected = [
        Record::Input(RawReport {
            mono_us: 1_520,
            wall_ms: 1_764_302_512_125,
            bytes: vec![0x28, 0, 0xff, 0x80],
        }),
        tick(1_000_412, 1_764_302_513_123),
        Record::Note {
            mono_us: 1_000_412,
            wall_ms: 1_764_302_513_123,
            text: Note::ReadError(-5),
        },
        Record::Omitted {
            mono_us: 1_000_500,
            wall_ms: 1_764_302_513_124,
            report_type: 0x28,
            len: 16,
            tracker: Some(TrackerLabel('A')),
            reason: OmittedReason::CheckFailed,
        },
        Record::Footer {
            mono_us: 1_000_500,
            wall_ms: None,
            reports: 1,
            malformed: 0,
            estimated_lost: 0,
            reason: EndReason::Duration,
            time_reversed: 0,
        },
    ];
    let mut reader = RecordReader::new(Cursor::new(FIXTURE)).expect("read fixture header");
    assert_eq!(reader.header(), &header());
    assert_eq!(read_rows(&mut reader), expected);
    assert_eq!(reader.stats().malformed, 0);
    assert_eq!(reader.stats().unknown, 0);
    assert_eq!(reader.stats().time_reversed, 0);

    let written = write(&header(), &expected);
    assert_eq!(json_rows(&written), json_rows(FIXTURE.as_bytes()));
    let mut reader = RecordReader::new(Cursor::new(written)).expect("read written header");
    assert_eq!(reader.header(), &header());
    assert_eq!(read_rows(&mut reader), expected);
}

#[test]
fn preserves_footer_wall_time_on_round_trip() {
    let first_line = FIXTURE.lines().next().expect("fixture header");
    let footer = r#"{"type":"footer","mono_us":250,"wall_ms":123456,"reports":1,"malformed":0,"estimated_lost":0,"reason":"duration"}"#;
    let bytes = [first_line, footer].join("\n");
    let mut reader = RecordReader::new(Cursor::new(bytes)).expect("read header");
    let rows = read_rows(&mut reader);
    assert!(matches!(
        rows.as_slice(),
        [Record::Footer {
            mono_us: 250,
            wall_ms: Some(123_456),
            ..
        }]
    ));
    assert_eq!(reader.stats().malformed, 0);
    assert_eq!(
        json_rows(&write(reader.header(), &rows))[1],
        serde_json::from_str::<Value>(footer).expect("footer fixture is valid JSON")
    );
}

#[test]
fn ignores_unknown_fields_and_rows_without_affecting_replay_time() {
    let mut future_header = json_rows(FIXTURE.as_bytes()).remove(0);
    future_header["extra"] = json!("ignored");
    future_header["device"]["extra"] = json!(123);
    let bytes = [
        &future_header.to_string(),
        r#"{"type":"tick","mono_us":5,"wall_ms":1,"future_field":{"any":true}}"#,
        r#"{"type":"future","mono_us":999,"anything":[1,2,3]}"#,
        r#"{"type":"tick","mono_us":6,"wall_ms":2}"#,
    ]
    .join("\r\n");
    let mut reader = RecordReader::new(Cursor::new(bytes)).expect("read extended header");
    assert_eq!(reader.header(), &header());
    assert_eq!(read_rows(&mut reader), [tick(5, 1), tick(6, 2)]);
    assert_eq!(reader.stats().unknown, 1);
    assert_eq!(reader.stats().malformed, 0);
    assert_eq!(reader.stats().time_reversed, 0);
    assert!(reader.read_record().expect("read after EOF").is_none());
}

#[test]
fn skips_broken_rows_and_an_interrupted_final_row() {
    let first_line = FIXTURE.lines().next().expect("fixture header");
    let mut bytes = [
        first_line,
        "{bad json}",
        r#"{"type":"in","mono_us":3,"wall_ms":2,"hex":"000x"}"#,
        "",
    ]
    .join("\n")
    .into_bytes();
    bytes.extend_from_slice(b"{\"type\":\"tick\",\"extra\":\"\xff\"}\n");
    bytes.extend_from_slice(
        b"{\"type\":\"tick\",\"mono_us\":5,\"wall_ms\":1}\n{\"type\":\"tick\",\"mono_us\":",
    );
    let mut reader = RecordReader::new(Cursor::new(bytes)).expect("read header");
    assert_eq!(read_rows(&mut reader), [tick(5, 1)]);
    assert_eq!(reader.stats().malformed, 4);
    assert_eq!(reader.stats().unknown, 0);
    assert!(reader.read_record().expect("read after EOF").is_none());
    assert_eq!(reader.stats().malformed, 4);
}

#[test]
fn counts_array_row_as_malformed_and_reads_the_next_object() {
    let first_line = FIXTURE.lines().next().expect("fixture header");
    let bytes = [
        first_line,
        r#"["tick",5,9]"#,
        r#"{"type":"tick","mono_us":7,"wall_ms":10}"#,
    ]
    .join("\n");
    let mut reader = RecordReader::new(Cursor::new(bytes)).expect("read header");
    assert_eq!(read_rows(&mut reader), [tick(7, 10)]);
    assert_eq!(reader.stats().malformed, 1);
    assert_eq!(reader.stats().unknown, 0);
}

#[test]
fn clamps_decreasing_mono_us_in_file_order() {
    let first_line = FIXTURE.lines().next().expect("fixture header");
    let bytes = [
        first_line,
        r#"{"type":"tick","mono_us":10,"wall_ms":14}"#,
        r#"{"type":"tick","mono_us":4,"wall_ms":9}"#,
        r#"{"type":"tick","mono_us":10,"wall_ms":8}"#,
        r#"{"type":"tick","mono_us":11,"wall_ms":15}"#,
    ]
    .join("\n");
    let mut reader = RecordReader::new(Cursor::new(bytes)).expect("read header");
    assert_eq!(
        read_rows(&mut reader),
        [tick(10, 14), tick(10, 9), tick(10, 8), tick(11, 15)]
    );
    assert_eq!(reader.stats().time_reversed, 1);
    assert_eq!(reader.stats().malformed, 1);
}

#[test]
fn rejects_an_unsupported_version() {
    let mut unknown = json_rows(FIXTURE.as_bytes()).remove(0);
    unknown["version"] = json!(300);
    assert!(matches!(
        RecordReader::new(Cursor::new(unknown.to_string())),
        Err(ReadError::UnsupportedVersion(300))
    ));
}

#[test]
fn header_device_and_omitted_row_serialize_only_the_documented_fields() {
    let mut header = header();
    header.mode = Mode::ProbeSharing;
    let capture = write(
        &header,
        &[Record::Omitted {
            mono_us: 22,
            wall_ms: 33,
            report_type: 0x29,
            len: 15,
            tracker: None,
            reason: OmittedReason::NotAllowed,
        }],
    );
    let rows = json_rows(&capture);
    assert_eq!(rows[0]["mode"], "probe-sharing");
    let device = rows[0]["device"].as_object().expect("device fields");
    let keys: BTreeSet<_> = device.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        BTreeSet::from(["vid", "pid", "interface", "usage_page", "usage"])
    );
    assert_eq!(
        rows[1],
        json!({
            "type": "omitted",
            "mono_us": 22,
            "wall_ms": 33,
            "report_type": 0x29,
            "len": 15,
            "reason": "not_allowed",
        })
    );
    let mut reader = RecordReader::new(Cursor::new(capture)).expect("read capture without footer");
    assert_eq!(reader.header(), &header);
    assert_eq!(read_rows(&mut reader).len(), 1);
    assert_eq!(reader.stats().malformed, 0);
}

#[test]
fn note_text_is_limited_to_documented_words_and_numeric_error_codes() {
    let notes = [
        (Note::ServerAppeared, "server_appeared"),
        (Note::ReadError(i32::MIN), "read_error:-2147483648"),
        (Note::DeviceRemoved, "device_removed"),
        (Note::Duration, "duration"),
        (Note::WindowBaseStart, "window_base_start"),
        (Note::WindowBaseEnd, "window_base_end"),
        (Note::WindowProbeStart, "window_probe_start"),
        (Note::WindowProbeEnd, "window_probe_end"),
        (Note::OpenFailed(5), "open_failed:5"),
    ];
    let expected_text = notes.each_ref().map(|(_, text)| *text);
    let records = notes.map(|(text, _)| Record::Note {
        mono_us: 0,
        wall_ms: 0,
        text,
    });
    let written = write(&header(), &records);
    let mut rows = json_rows(&written);
    for (row, text) in rows.iter().skip(1).zip(expected_text) {
        assert_eq!(row["text"], text);
    }

    for text in ["freeform", "read_error:failure", "open_failed:2147483648"] {
        rows.push(json!({"type":"note","mono_us":0,"wall_ms":0,"text":text}));
    }
    let bytes = rows
        .into_iter()
        .map(|row| row.to_string() + "\n")
        .collect::<String>();
    let mut reader = RecordReader::new(Cursor::new(bytes)).expect("read header");
    assert_eq!(read_rows(&mut reader), records);
    assert_eq!(reader.stats().malformed, 3);
}
