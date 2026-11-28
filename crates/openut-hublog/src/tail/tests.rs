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

fn bodies(events: &[FollowEvent]) -> Vec<(FileKind, &str, bool)> {
    events
        .iter()
        .filter_map(|event| {
            let FollowEvent::Record { record, late } = event else {
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
            reason: RebuildReason::Truncated
        })
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
