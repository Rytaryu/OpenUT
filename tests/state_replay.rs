use openut_core::state::{ConnectionChangeKind, HostBy, HostWifi, Phase, Role, Snapshot, State};
use openut_core::time::LogTime;
use openut_core::tracker::TrackerKey;
use openut_hublog::decode::Decoder;
use openut_hublog::header::{FileKind, OffsetFill, ParsedLine, parse_line};
use openut_hublog::map::{MapCounts, map_entry};

fn replay_state(bytes: &[u8]) -> Snapshot {
    let mut state = State::default();
    let mut decoder = Decoder::new();
    let mut offsets = OffsetFill::new();
    let mut map_counts = MapCounts::default();
    let mut last = None;
    let mut starts = Vec::new();
    let mut records = Vec::new();
    for chunk in bytes.chunks(137) {
        records.extend(decoder.push(chunk));
    }
    records.extend(decoder.finish());
    for record in records {
        let entry = match parse_line(&record) {
            ParsedLine::Entry(entry) => entry,
            ParsedLine::Meta(_) => continue,
            ParsedLine::NoHeader(_) => panic!("expected a valid synthetic record"),
        };
        offsets.push(FileKind::Server, entry, |filled| {
            let Some(timed) = map_entry(&filled.entry, filled.file, &mut map_counts) else {
                return;
            };
            assert!(last.is_none_or(|previous| previous <= timed.at));
            last = Some(timed.at);
            let update = state.apply(&timed);
            for change in update.connections {
                assert_eq!(change.kind, ConnectionChangeKind::Started);
                starts.push(change.tracker.key);
            }
        });
    }
    assert_eq!(starts, [TrackerKey(1), TrackerKey(2)]);
    assert_eq!(state.out_of_order(), 0);
    state.snapshot(last.expect("test log contains mapped events"))
}

#[test]
fn normal_start_reaches_tracking_in_both_saved_encodings() {
    let utf8 = include_bytes!("replay/synthetic-normal-start/ViveTrackerServer_20261128.txt");
    let utf16 = include_bytes!("replay/synthetic-utf16/ViveTrackerServer_20261128.txt");
    let snapshot = replay_state(utf8);
    assert_eq!(replay_state(utf16), snapshot);
    let [host, client] = snapshot.trackers.as_slice() else {
        panic!("expected a host and a client");
    };
    assert_eq!((host.phase, host.role), (Phase::Tracking, Role::Host));
    assert_eq!((client.phase, client.role), (Phase::Tracking, Role::Client));
    assert!(host.tracked_since_connect && client.tracked_since_connect);
    assert!(matches!(snapshot.host_wifi, HostWifi::Held { .. }));
    let evidence = snapshot.host.expect("host command was present");
    assert_eq!(
        (evidence.key, evidence.conn, evidence.by),
        (host.key, host.conn, HostBy::Command)
    );
}

#[test]
fn second_host_command_clears_held_wifi_and_the_new_client_waits() {
    let log =
        include_bytes!("replay/synthetic-r1-second-host-command/ViveTrackerServer_20261128.txt");
    let snapshot = replay_state(log);
    let [host, client] = snapshot.trackers.as_slice() else {
        panic!("expected only the two real connections in the log");
    };
    assert_eq!((host.phase, host.role), (Phase::MapSyncing, Role::Host));
    assert_eq!(
        (client.phase, client.role),
        (Phase::WaitingWifi, Role::Client)
    );
    assert_eq!(
        snapshot.host_wifi,
        HostWifi::Missing {
            since: LogTime(1_795_834_810_000),
            queried: false,
        },
    );
    assert_eq!(client.since.to_string(), "2026-11-28 03:00:10.200");
}
