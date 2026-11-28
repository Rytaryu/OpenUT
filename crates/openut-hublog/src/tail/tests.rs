use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;

use super::*;

const SERVER: &str = "ViveTrackerServer_20261128.txt";
const DRIVER: &str = "ViveTrackerDriver_20261128.txt";

#[derive(Default)]
struct MemorySource {
    files: BTreeMap<String, Vec<u8>>,
    reads: RefCell<Vec<(String, u64)>>,
    fail: Option<&'static str>,
    driver_read_limit: Option<usize>,
}

impl MemorySource {
    fn set(&mut self, name: &str, bytes: impl AsRef<[u8]>) {
        self.files.insert(name.to_owned(), bytes.as_ref().to_vec());
    }

    fn append(&mut self, name: &str, bytes: impl AsRef<[u8]>) {
        self.files
            .get_mut(name)
            .expect("test file exists")
            .extend(bytes.as_ref());
    }
}

impl LogSource for MemorySource {
    type Error = io::Error;

    fn list(&self) -> io::Result<Vec<LogFile>> {
        Ok(self
            .files
            .iter()
            .map(|(name, bytes)| LogFile {
                name: name.clone(),
                len: bytes.len() as u64,
            })
            .collect())
    }

    fn read_at(&self, name: &str, offset: u64, max: usize) -> io::Result<Option<Vec<u8>>> {
        self.reads.borrow_mut().push((name.to_owned(), offset));
        if self.fail == Some(name) {
            return Err(io::Error::other("test read interrupted"));
        }
        let max = if name == DRIVER {
            max.min(self.driver_read_limit.unwrap_or(max))
        } else {
            max
        };
        Ok(self.files.get(name).map(|bytes| {
            bytes
                .iter()
                .skip(offset as usize)
                .take(max)
                .copied()
                .collect()
        }))
    }
}

fn time(ms: i64) -> LogTime {
    LogTime(1_795_867_200_000 + ms)
}

fn line(ms: i64, body: &str) -> String {
    format!("[{} +0 0x1 INFO Test] [1] {body}\n", time(ms))
}

fn drain(follower: &mut LogFollower, now: i64, deadline: Option<i64>) -> Vec<FollowEvent> {
    std::iter::from_fn(|| follower.next_event(time(now), deadline.map(time))).collect()
}

fn positions(events: &[FollowEvent]) -> Vec<&RecordPosition> {
    events
        .iter()
        .filter_map(|event| match event {
            FollowEvent::Record { position, .. } => position.as_ref(),
            _ => None,
        })
        .collect()
}

#[test]
fn positions_survive_the_driver_wait_and_untimed_server_bytes_refresh_growth() {
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    let server_prefix = "# server\n";
    source.set(SERVER, server_prefix);
    let driver_entry = line(10, "first driver");
    let driver_tail = b"broken \xff\n";
    source.set(DRIVER, [driver_entry.as_bytes(), driver_tail].concat());

    follower.poll(&source, time(15), None).unwrap();
    assert!(follower.server_found());
    assert!(follower.server_grew());
    assert!(!follower.has_pending_server_records());
    assert_eq!(follower.last_log_time(FileKind::Server), None);
    let events = drain(&mut follower, 15, None);
    assert_eq!(positions(&events)[0].end, server_prefix.len() as u64);
    follower.poll(&source, time(17), None).unwrap();
    assert!(!follower.server_grew());

    let server_entry = line(20, "first server");
    source.append(SERVER, &server_entry);
    follower.poll(&source, time(25), None).unwrap();
    assert_eq!(follower.last_log_time(FileKind::Server), Some(time(20)));
    assert_eq!(follower.last_log_time(FileKind::Driver), Some(time(10)));
    assert!(follower.has_pending_server_records());
    let events = drain(&mut follower, 25, None);
    assert!(!follower.has_pending_server_records());
    let records = positions(&events);
    let [server, driver, invalid] = records.as_slice() else {
        panic!("one server and two driver rows");
    };
    assert_eq!(
        (server.name.as_str(), server.end),
        (SERVER, (server_prefix.len() + server_entry.len()) as u64)
    );
    assert_eq!(
        (driver.name.as_str(), driver.end),
        (DRIVER, driver_entry.len() as u64)
    );
    assert_eq!(
        (invalid.name.as_str(), invalid.end),
        (DRIVER, (driver_entry.len() + driver_tail.len()) as u64)
    );
    assert_eq!(invalid.decode.replaced_chars, 1);
}

#[test]
fn eof_and_new_generations_use_positions_from_their_own_file() {
    const NEXT: &str = "ViveTrackerServer_20261129.txt";
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    let old = b"# first\nwithout a delimiter";
    source.set(SERVER, old);
    follower.poll(&source, time(1), None).unwrap();
    let first = drain(&mut follower, 1, None);
    let first_generation = positions(&first)[0].generation;

    source.set(NEXT, b"# new\r\n");
    follower.poll(&source, time(2), None).unwrap();
    let rotated = drain(&mut follower, 2, None);
    let end = positions(&rotated)[0];
    assert_eq!(
        (end.name.as_str(), end.end, end.generation),
        (SERVER, old.len() as u64, first_generation)
    );
    follower.poll(&source, time(3), None).unwrap();
    let next = drain(&mut follower, 3, None);
    let new = positions(&next)[0];
    assert_eq!((new.name.as_str(), new.end), (NEXT, 7));
    assert!(new.generation > first_generation);
}

#[test]
fn server_rebuilds_identify_the_affected_file() {
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    source.set(SERVER, "# former\n");
    follower.poll(&source, time(1), None).unwrap();
    let first = drain(&mut follower, 1, None);
    let mut last_generation = positions(&first)[0].generation;
    for (reason, bytes) in [
        (RebuildReason::Replaced, Some("# longer replacement\n")),
        (RebuildReason::Truncated, Some("# new\n")),
        (RebuildReason::Disappeared, None),
        (RebuildReason::Restored, Some("# restored\n")),
    ] {
        if let Some(bytes) = bytes {
            source.set(SERVER, bytes);
        } else {
            source.files.remove(SERVER);
        }
        follower.poll(&source, time(2), None).unwrap();
        let events = drain(&mut follower, 2, None);
        assert_eq!(
            events[0],
            FollowEvent::Rebuild {
                file: FileKind::Server,
                reason,
                name: Some(SERVER.to_owned()),
            }
        );
        if let Some(bytes) = bytes {
            let records = positions(&events);
            assert_eq!(records[0].end, bytes.len() as u64);
            assert!(records[0].generation > last_generation);
            last_generation = records[0].generation;
        }
    }
}

fn bodies(events: &[FollowEvent]) -> Vec<(FileKind, &str, bool)> {
    events
        .iter()
        .filter_map(|event| {
            let FollowEvent::Record { record, late, .. } = event else {
                return None;
            };
            let text = match &record.data {
                RecordData::Entry(entry) => entry.body.as_str(),
                RecordData::Meta(text) | RecordData::NoHeader(text) => text,
            };
            Some((record.file, text, *late))
        })
        .collect()
}

#[test]
fn a_driver_append_releases_the_server_in_order_and_rechecks_the_deadline() {
    let mut source = MemorySource::default();
    source.set(
        SERVER,
        line(5_000, "through")
            + &line(6_000, "first held")
            + "# after held\nbroken\n"
            + &line(9_000, "second held"),
    );
    source.set(DRIVER, line(5_000, "driver at deadline"));
    let mut follower = LogFollower::default();
    assert!(
        !follower
            .poll(&source, time(10_000), Some(time(5_000)))
            .unwrap()
    );
    let events = drain(&mut follower, 10_000, Some(5_000));
    assert_eq!(
        bodies(&events),
        [
            (FileKind::Server, "through", false),
            (FileKind::Driver, "driver at deadline", false),
        ]
    );
    assert!(events.contains(&FollowEvent::ReadComplete));
    assert_eq!(
        follower.advance_target(time(10_000), Some(time(5_000))),
        Some(time(5_000))
    );
    follower
        .poll(&source, time(10_001), Some(time(5_000)))
        .unwrap();
    assert!(drain(&mut follower, 10_001, Some(5_000)).is_empty());

    source.append(
        DRIVER,
        line(4_999, "late") + &line(6_000, "driver past deadline"),
    );
    follower
        .poll(&source, time(11_000), Some(time(5_000)))
        .unwrap();
    for (file, text, late) in [
        (FileKind::Driver, "late", true),
        (FileKind::Driver, "driver past deadline", false),
        (FileKind::Server, "first held", false),
    ] {
        let event = follower
            .next_event(time(11_000), Some(time(5_000)))
            .unwrap();
        assert_eq!(bodies(&[event]), [(file, text, late)]);
    }
    assert_eq!(
        bodies(&drain(&mut follower, 11_000, Some(7_000))),
        [
            (FileKind::Server, "# after held", false),
            (FileKind::Server, "broken", false),
        ]
    );
    assert_eq!(
        bodies(&drain(&mut follower, 11_000, None)),
        [(FileKind::Server, "second held", false),]
    );
}

#[test]
fn current_driver_eof_still_waits_for_late_appends_until_d0_moves_past_d() {
    let mut source = MemorySource::default();
    source.set(SERVER, line(0, "remove") + &line(5_100, "later"));
    source.set(DRIVER, line(0, "driver"));
    let mut follower = LogFollower::default();
    follower
        .poll(&source, time(5_300), Some(time(5_000)))
        .unwrap();
    let initial = drain(&mut follower, 5_300, Some(5_000));
    assert_eq!(
        bodies(&initial),
        [
            (FileKind::Server, "remove", false),
            (FileKind::Driver, "driver", false)
        ]
    );
    assert!(initial.contains(&FollowEvent::ReadComplete));
    assert!(follower.read_complete());

    source.append(DRIVER, line(900, "shutdown notice"));
    follower
        .poll(&source, time(5_900), Some(time(5_000)))
        .unwrap();
    assert_eq!(
        bodies(&drain(&mut follower, 5_900, Some(5_000))),
        [(FileKind::Driver, "shutdown notice", false)]
    );
    assert!(drain(&mut follower, 15_200, Some(5_000)).is_empty());
    assert_eq!(
        bodies(&drain(&mut follower, 15_201, Some(5_000))),
        [(FileKind::Server, "later", false)]
    );
}

#[test]
fn unread_driver_bytes_keep_the_deadline_held_past_the_wall_estimate() {
    let mut source = MemorySource::default();
    source.set(SERVER, line(1_000, "remove") + &line(6_000, "held"));
    source.set(DRIVER, line(1_000, "initial"));
    let mut follower = LogFollower::default();
    follower
        .poll(&source, time(10_000), Some(time(5_000)))
        .unwrap();
    let _ = drain(&mut follower, 10_000, Some(5_000));

    let first = line(2_000, "first chunk");
    source.driver_read_limit = Some(first.len());
    source.append(DRIVER, first + &line(4_000, "written notice"));
    assert!(
        follower
            .poll(&source, time(20_000), Some(time(5_000)))
            .unwrap()
    );
    assert_eq!(
        bodies(&drain(&mut follower, 20_000, Some(5_000))),
        [(FileKind::Driver, "first chunk", false)],
    );
    assert_eq!(
        follower.advance_target(time(20_000), Some(time(5_000))),
        Some(time(5_000))
    );

    source.driver_read_limit = None;
    assert!(
        !follower
            .poll(&source, time(20_001), Some(time(5_000)))
            .unwrap()
    );
    assert_eq!(
        bodies(&drain(&mut follower, 20_001, Some(5_000))),
        [
            (FileKind::Driver, "written notice", false),
            (FileKind::Server, "held", false)
        ],
    );
}

#[test]
fn a_new_server_clock_sample_does_not_mark_same_poll_driver_records_late() {
    let mut source = MemorySource::default();
    source.set(SERVER, line(1_000, "server"));
    source.set(DRIVER, line(0, "driver"));
    let mut follower = LogFollower::default();
    follower.poll(&source, time(21_000), None).unwrap();
    let _ = drain(&mut follower, 21_000, None);

    source.append(SERVER, line(29_000, "new clock sample"));
    source.append(DRIVER, line(15_000, "driver before updated estimate"));
    follower.poll(&source, time(30_000), None).unwrap();
    assert_eq!(
        bodies(&drain(&mut follower, 30_000, None)),
        [
            (FileKind::Server, "new clock sample", false),
            (FileKind::Driver, "driver before updated estimate", false)
        ],
    );
}

#[test]
fn unavailable_driver_never_blocks_and_its_partial_record_cannot_reappear() {
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    source.set(SERVER, line(9_000, "continues"));
    source.set(
        DRIVER,
        format!("# driver\n{}", line(9_000, "old").trim_end()),
    );
    follower
        .poll(&source, time(10_000), Some(time(5_000)))
        .unwrap();
    assert!(follower.driver_found());
    assert_eq!(
        bodies(&drain(&mut follower, 10_000, Some(5_000))),
        [(FileKind::Driver, "# driver", false)]
    );

    source.files.remove(DRIVER);
    follower
        .poll(&source, time(10_500), Some(time(5_000)))
        .unwrap();
    let events = drain(&mut follower, 10_500, Some(5_000));
    assert!(!follower.driver_found());
    assert_eq!(bodies(&events), [(FileKind::Server, "continues", false)]);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, FollowEvent::Rebuild { .. }))
    );

    source.set(DRIVER, line(10_000, "new"));
    follower.poll(&source, time(11_000), None).unwrap();
    assert_eq!(
        bodies(&drain(&mut follower, 11_000, None)),
        [(FileKind::Driver, "new", false)]
    );
}

#[test]
fn rebuilding_a_driver_file_rewinds_both_streams_and_drops_partial_decoding() {
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    let partial = line(2_000, "partial");
    source.set(SERVER, line(1_000, "once") + partial.trim_end());
    source.set(DRIVER, line(1_000, "driver before"));
    follower.poll(&source, time(3_000), None).unwrap();
    assert_eq!(
        bodies(&drain(&mut follower, 3_000, None)),
        [
            (FileKind::Server, "once", false),
            (FileKind::Driver, "driver before", false),
        ]
    );

    source.set(DRIVER, line(1_000, "new"));
    follower.poll(&source, time(3_500), None).unwrap();
    let events = drain(&mut follower, 3_500, None);
    assert!(matches!(
        events.first(),
        Some(FollowEvent::Rebuild {
            file: FileKind::Driver,
            reason: RebuildReason::Truncated,
            name: Some(name),
        }) if name == DRIVER
    ));
    assert_eq!(
        bodies(&events),
        [
            (FileKind::Server, "once", false),
            (FileKind::Driver, "new", false)
        ]
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == FollowEvent::ReadComplete)
            .count(),
        1
    );

    source.append(SERVER, b"\n");
    follower.poll(&source, time(4_000), None).unwrap();
    assert_eq!(
        bodies(&drain(&mut follower, 4_000, None)),
        [(FileKind::Server, "partial", false)]
    );
}

#[test]
fn failed_second_stream_read_keeps_the_first_stream_retriable() {
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    source.set(SERVER, line(1_000, "server"));
    source.set(DRIVER, line(1_000, "driver"));
    source.fail = Some(DRIVER);
    assert!(follower.poll(&source, time(2_000), None).is_err());
    assert!(drain(&mut follower, 2_000, None).is_empty());
    source.fail = None;
    follower.poll(&source, time(2_100), None).unwrap();
    assert_eq!(
        bodies(&drain(&mut follower, 2_100, None)),
        [
            (FileKind::Server, "server", false),
            (FileKind::Driver, "driver", false)
        ]
    );
}

#[test]
fn a_shorter_replacement_during_catch_up_rewinds_both_streams() {
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    source.set(SERVER, line(1_000, "server from the beginning"));
    let mut old_driver = line(1_000, "former file").into_bytes();
    old_driver.resize(2 * file::MAX_READ_BYTES, b'x');
    source.set(DRIVER, old_driver);

    assert!(follower.poll(&source, time(2_000), None).unwrap());
    let initial = drain(&mut follower, 2_000, None);
    assert!(bodies(&initial).contains(&(FileKind::Driver, "former file", false)));
    assert!(!follower.read_complete());

    let mut new_driver = line(1_500, "replacement from the beginning").into_bytes();
    new_driver.resize(file::MAX_READ_BYTES + file::MAX_READ_BYTES / 2, b'y');
    source.set(DRIVER, new_driver);
    source.reads.borrow_mut().clear();
    assert!(follower.poll(&source, time(2_500), None).unwrap());
    let events = drain(&mut follower, 2_500, None);
    assert_eq!(
        events.first(),
        Some(&FollowEvent::Rebuild {
            file: FileKind::Driver,
            reason: RebuildReason::Replaced,
            name: Some(DRIVER.to_owned()),
        }),
    );
    assert!(bodies(&events).contains(&(FileKind::Server, "server from the beginning", false)));
    assert!(bodies(&events).contains(&(FileKind::Driver, "replacement from the beginning", false)));
    assert!(source.reads.borrow().iter().all(|(_, offset)| *offset == 0));
    assert!(!follower.read_complete());
}

#[test]
fn incomplete_driver_catch_up_reads_first_while_the_server_head_is_held() {
    let mut source = MemorySource::default();
    let mut follower = LogFollower::default();
    let mut server = line(6_000, "held").into_bytes();
    server.resize(file::MAX_READ_BYTES + 10, b'x');
    source.set(SERVER, server);
    let mut driver = line(1_000, "early").into_bytes();
    driver.resize(2 * file::MAX_READ_BYTES + 10, b'x');
    source.set(DRIVER, driver);

    assert!(
        follower
            .poll(&source, time(10_000), Some(time(5_000)))
            .unwrap()
    );
    let _ = drain(&mut follower, 10_000, Some(5_000));
    assert!(!follower.read_complete());

    source.reads.borrow_mut().clear();
    assert!(
        follower
            .poll(&source, time(50_000), Some(time(5_000)))
            .unwrap()
    );
    assert_eq!(
        *source.reads.borrow(),
        [(DRIVER.to_owned(), file::MAX_READ_BYTES as u64)]
    );
    assert!(!follower.read_complete());
    assert!(drain(&mut follower, 50_000, Some(5_000)).is_empty());

    assert!(
        !follower
            .poll(&source, time(50_001), Some(time(5_000)))
            .unwrap()
    );
    let events = drain(&mut follower, 50_001, Some(5_000));
    assert_eq!(events, [FollowEvent::ReadComplete]);
    assert!(follower.read_complete());
}
