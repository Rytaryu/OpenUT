use super::*;
use crate::event::{OetKind, PoseStatus};
use crate::state::State;
use crate::tracker::HubNumber;

const A: TrackerKey = TrackerKey(2);
const B: TrackerKey = TrackerKey(4);
const WAIT_S: u64 = 2;
const GAP_S: u64 = 600;

#[derive(Default)]
struct Case {
    state: State,
    summaries: Summaries,
}

impl Case {
    fn apply(&mut self, ms: i64, event: Event) {
        self.apply_at(LogTime(ms), UtcOffset(0), event);
    }

    fn apply_at(&mut self, at: LogTime, offset: UtcOffset, event: Event) {
        self.summaries.advance_to(at, offset, WAIT_S);
        let before = self.state.snapshot(at);
        self.summaries.observe(at, offset, &before, GAP_S);
        let timed = Timed { at, offset, event };
        let update = self.state.apply(&timed);
        let after = self.state.snapshot(at);
        self.summaries
            .event(&timed, &before, &after, &update, WAIT_S);
    }

    fn add(&mut self, ms: i64, tracker: TrackerKey, usb: bool) {
        self.apply(ms, Event::TrackerAdded { tracker, usb });
    }

    fn remove(&mut self, ms: i64, tracker: TrackerKey) {
        self.apply(
            ms,
            Event::TrackerRemoved {
                tracker,
                usb: false,
            },
        );
    }

    fn identify(&mut self, ms: i64, tracker: TrackerKey, name: &str, number: u16) {
        self.apply(
            ms,
            Event::SerialKnown {
                tracker,
                serial: Serial(name.into()),
                hub_number: Some(HubNumber(number)),
                firmware: None,
            },
        );
    }

    fn pose(&mut self, ms: i64, tracker: TrackerKey, status: PoseStatus) {
        self.apply(ms, Event::Pose { tracker, status });
    }

    fn current(&self) -> Summary {
        let [summary] = self
            .summaries
            .open_summaries()
            .try_into()
            .expect("one summary");
        summary
    }
}

#[test]
fn attributes_state_spans_and_counts_to_the_event_day() {
    let mut case = Case::default();
    case.add(1_000, A, false);
    case.identify(1_008, A, "SYNTHETIC-A", 1);
    case.add(1_500, A, false);
    case.apply(2_000, Event::HostAssigned { tracker: A });
    case.pose(3_000, A, PoseStatus::Ok);
    case.pose(4_000, A, PoseStatus::Lost);
    case.pose(6_000, A, PoseStatus::Ok);
    case.pose(7_000, A, PoseStatus::Lost);
    case.summaries
        .advance_to(LogTime(9_001), UtcOffset(0), WAIT_S);
    case.pose(10_000, A, PoseStatus::Ok);
    case.apply(
        11_000,
        Event::Oet {
            tracker: A,
            kind: OetKind::Overspeed,
        },
    );
    case.remove(12_000, A);

    let summary = case.current();
    let [tracker] = summary.trackers.as_slice() else {
        panic!("one tracker")
    };
    assert_eq!(summary.unobserved_s, 1.0);
    assert_eq!(summary.host_starts, 1);
    assert_eq!(tracker.connected_s, 11.0);
    assert_eq!(tracker.in_use_s, 9.0);
    assert_eq!(tracker.tracking_s, 4.0);
    assert_eq!(tracker.tracking_ratio, Some(4.0 / 9.0));
    assert_eq!(tracker.sessions, 1);
    assert_eq!(tracker.first_pose_s, [1.0]);
    assert_eq!(tracker.losses, 2);
    assert_eq!(tracker.long_losses, 1);
    assert_eq!(tracker.overspeed, 1);
    assert_eq!(tracker.host_count, 1);
}

#[test]
fn splits_at_local_midnight_without_creating_unobserved_days() {
    let offset = UtcOffset(540);
    let (first_day, next_day, later_day) = (
        LogDate {
            year: 2026,
            month: 11,
            day: 27,
        },
        LogDate {
            year: 2026,
            month: 11,
            day: 28,
        },
        LogDate {
            year: 2026,
            month: 11,
            day: 30,
        },
    );
    let local = |day, hour, minute| LogTime::from_local(day, hour, minute, 0, 0, offset).unwrap();
    let mut case = Case::default();
    case.apply_at(
        local(first_day, 23, 59),
        offset,
        Event::Pose {
            tracker: A,
            status: PoseStatus::Ok,
        },
    );
    case.summaries.source(Some(next_day)).driver.lines = 3;
    assert_eq!(case.current().source.driver.lines, 0);
    assert!(case.summaries.close_days().is_empty());
    case.apply_at(
        local(next_day, 0, 4),
        offset,
        Event::Pose {
            tracker: A,
            status: PoseStatus::Lost,
        },
    );

    let [closed]: [_; 1] = case.summaries.close_days().try_into().unwrap();
    assert_eq!(closed.date, "2026-11-27");
    assert!(closed.is_final);
    assert_eq!(closed.observed_until, "2026-11-28 00:00:00.000");
    assert_eq!(closed.trackers[0].tracking_s, 60.0);
    let next = case.current();
    assert_eq!(next.date, "2026-11-28");
    assert_eq!(next.observed_until, "2026-11-28 00:04:00.000");
    assert_eq!(next.trackers[0].tracking_s, 240.0);
    assert_eq!(next.trackers[0].losses, 1);
    assert_eq!(next.source.driver.lines, 3);

    case.apply_at(local(later_day, 0, 1), offset, Event::ServerStopped);
    let [closed]: [_; 1] = case.summaries.close_days().try_into().unwrap();
    assert_eq!(closed.date, "2026-11-28");
    assert_eq!(closed.unobserved_s, 86_160.0);
    assert_eq!(case.current().unobserved_s, 60.0);
    assert!(case.summaries.close_days().is_empty());
}

#[test]
fn merges_known_serials_but_keeps_slot_reuse_separate() {
    let mut case = Case::default();
    case.add(0, A, false);
    case.identify(8, A, "SYNTHETIC-A", 1);
    case.remove(1_000, A);
    case.add(1_000, B, true);
    case.identify(1_008, B, "SYNTHETIC-A", 2);
    case.add(3_000, B, false);
    case.identify(3_008, B, "SYNTHETIC-B", 1);
    case.add(3_100, A, false);
    case.apply(4_000, Event::DongleRemoved);

    let summary = case.current();
    let [first, second, unknown] = summary.trackers.as_slice() else {
        panic!("three aggregated trackers")
    };
    assert_eq!(
        (
            first.serial.as_deref(),
            first.slots.as_slice(),
            first.hub_number
        ),
        (Some("SYNTHETIC-A"), [2, 4].as_slice(), Some(2))
    );
    assert_eq!((first.connected_s, first.sessions), (3.0, 2));
    assert_eq!(
        (
            second.serial.as_deref(),
            second.connected_s,
            second.sessions
        ),
        (Some("SYNTHETIC-B"), 1.0, 1)
    );
    assert_eq!(
        (unknown.serial.as_deref(), unknown.connected_s),
        (None, 0.9)
    );
}

#[test]
fn counts_timers_without_extending_observation_and_preserves_usb() {
    let mut case = Case::default();
    case.add(0, A, true);
    case.add(0, B, false);
    case.pose(1_000, A, PoseStatus::Ok);
    case.pose(1_000, B, PoseStatus::Ok);
    case.apply(2_000, Event::DongleRemoved);
    case.apply(3_000, Event::DongleAdded);
    let detection = Detection {
        rule: RuleId::R4,
        tracker: Some(B),
        episode: 1,
        serial: None,
        conn: Some(ConnId(2)),
        host_wifi_queried: None,
        at: LogTime(10_000),
        step: Step::Raised,
    };
    case.summaries.detection(&detection, UtcOffset(0));
    case.summaries.detection(
        &Detection {
            step: Step::Resolved,
            ..detection
        },
        UtcOffset(0),
    );
    case.summaries
        .power_off(LogTime(2_000), UtcOffset(0), B, Some(ConnId(2)), None);

    let summary = case.current();
    let [usb, wireless] = summary.trackers.as_slice() else {
        panic!("two trackers")
    };
    assert_eq!(summary.observed_until, "1970-01-01 00:00:03.000");
    assert_eq!((usb.connected_s, usb.tracking_s), (3.0, 2.0));
    assert_eq!((wireless.connected_s, wireless.tracking_s), (2.0, 1.0));
    assert_eq!(
        (wireless.unexpected_disconnects, wireless.power_offs),
        (1, 1)
    );
    assert_eq!(summary.detections.r4, 1);
    assert_eq!(summary.faults.unexpected_disconnects, 1);
}

#[test]
fn an_interval_only_counts_as_unobserved_when_strictly_over_the_gap() {
    let mut case = Case::default();
    case.add(0, A, false);
    case.apply(600_000, Event::DongleAdded);
    case.apply(1_200_001, Event::ServerStopped);
    let summary = case.current();
    assert_eq!(summary.trackers[0].connected_s, 600.0);
    assert_eq!(summary.unobserved_s, 600.001);
}

#[cfg(feature = "serde")]
#[test]
fn exports_schema_and_null_rate_for_an_untracked_connection() {
    let mut case = Case::default();
    case.add(0, A, false);
    case.remove(1_000, A);
    let value = serde_json::to_value(case.current()).unwrap();
    let expected = serde_json::json!({
        "schema": 1, "date": "1970-01-01", "final": false,
        "observed_until": "1970-01-01 00:00:01.000", "unobserved_s": 0.0,
        "source": { "lines": 0, "meta": 0, "other": 0, "unparsed": 0, "no_header": 0,
            "bad_offset": 0, "replaced_chars": 0, "long_lines": 0, "time_regressions": 0,
            "late_writes": 0, "out_of_order": 0, "rebuilds": 0,
            "driver": { "found": false, "lines": 0, "meta": 0, "no_header": 0, "bad_offset": 0,
                "replaced_chars": 0, "long_lines": 0, "late": 0, "late_writes": 0 } },
        "trackers": [{ "serial": null, "slots": [2], "hub_number": null,
            "connected_s": 1.0, "in_use_s": 0.0, "tracking_s": 0.0, "tracking_ratio": null,
            "sessions": 1, "first_pose_s": [], "losses": 0, "long_losses": 0,
            "unexpected_disconnects": 0, "power_offs": 0, "overspeed": 0, "host_count": 0 }],
        "host_starts": 0, "faults": { "wifi_info_missing": 0, "unexpected_disconnects": 0 },
        "detections": { "R1": 0, "R2": 0, "R3": 0, "R4": 0, "R5": 0 }, "pushes": 0
    });
    assert_eq!(value.to_string(), expected.to_string());
}
