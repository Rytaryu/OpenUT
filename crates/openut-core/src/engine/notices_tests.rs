//! Integration tests for the notice outlet and the engine's rebuild boundary.

use super::{Engine, EngineConfig, FileKind, LineQuality, Output};
use crate::event::{Event, PoseStatus, Timed};
use crate::notice::Notice;
use crate::rules::{Detection, RuleId, Step};
use crate::time::{LogTime, UtcOffset};
use crate::tracker::{Serial, TrackerKey};

const HOST: TrackerKey = TrackerKey(0);
const A: TrackerKey = TrackerKey(1);
const B: TrackerKey = TrackerKey(2);
const C: TrackerKey = TrackerKey(3);
const REASSIGNED: TrackerKey = TrackerKey(4);

const SERIAL_A: &str = "SYNTHETIC0001";
const SERIAL_B: &str = "SYNTHETIC0002";
const SERIAL_C: &str = "SYNTHETIC0003";

fn engine() -> Engine {
    let mut cfg = EngineConfig::default();
    cfg.rules.r1.span_s = 2;
    cfg.rules.r1.escalate_s = 45;
    Engine::new(cfg)
}

fn at(seconds: i64) -> LogTime {
    LogTime(seconds * 1_000)
}

fn apply(engine: &mut Engine, seconds: i64, event: Event) {
    engine.apply(Timed {
        at: at(seconds),
        offset: UtcOffset(0),
        event,
    });
}

fn connect(engine: &mut Engine, seconds: i64, tracker: TrackerKey, serial: &str) {
    apply(
        engine,
        seconds,
        Event::TrackerAdded {
            tracker,
            usb: false,
        },
    );
    apply(
        engine,
        seconds,
        Event::SerialKnown {
            tracker,
            serial: Serial(serial.to_owned()),
            hub_number: None,
            firmware: None,
        },
    );
}

fn cut(engine: &mut Engine, seconds: i64, tracker: TrackerKey) {
    apply(
        engine,
        seconds,
        Event::TrackerRemoved {
            tracker,
            usb: false,
        },
    );
}

fn link(engine: &mut Engine, seconds: i64, tracker: TrackerKey) {
    apply(
        engine,
        seconds,
        Event::ClientLink {
            tracker,
            connected: true,
        },
    );
}

fn unanswered_requests(engine: &mut Engine, seconds: i64, host: TrackerKey) {
    apply(engine, seconds, Event::HostAssigned { tracker: host });
    apply(
        engine,
        seconds,
        Event::Pose {
            tracker: host,
            status: PoseStatus::Ok,
        },
    );
    for time in seconds..=seconds + 2 {
        apply(engine, time, Event::WifiRequestUnanswered { tracker: None });
    }
}

fn notices(output: &[Output]) -> impl Iterator<Item = &Notice> {
    output.iter().filter_map(|item| match item {
        Output::Notice(notice) => Some(notice),
        _ => None,
    })
}

fn detections(output: &[Output]) -> impl Iterator<Item = &Detection> {
    output.iter().filter_map(|item| match item {
        Output::Detection(detection) => Some(detection),
        _ => None,
    })
}

fn pushes(output: &[Output]) -> Vec<(RuleId, Step, Option<TrackerKey>)> {
    notices(output)
        .filter(|notice| notice.push)
        .map(|notice| (notice.rule, notice.step, notice.tracker))
        .collect()
}

#[test]
fn rebuild_keeps_an_r1_push_and_escalation_delivered_for_the_same_serial() {
    let mut engine = engine();
    connect(&mut engine, 0, HOST, SERIAL_A);
    unanswered_requests(&mut engine, 1, HOST);
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R1, Step::Raised, Some(HOST))]
    );
    engine.advance_to(at(49));
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R1, Step::Escalated, Some(HOST))],
    );
    let before = engine.live_notices();
    let [before] = before.as_slice() else {
        panic!("expected the escalated R1 in the status window");
    };

    engine.rebuild();
    engine.set_catching_up(true);
    connect(&mut engine, 0, B, SERIAL_B);
    connect(&mut engine, 0, REASSIGNED, SERIAL_A);
    unanswered_requests(&mut engine, 1, REASSIGNED);
    engine.advance_to(at(49));
    assert!(pushes(&engine.drain()).is_empty());
    engine.set_catching_up(false);
    engine.advance_to(at(60));
    assert!(pushes(&engine.drain()).is_empty());
    assert_eq!(engine.stats().pushes, 2);
    let after = engine.live_notices();
    let [after] = after.as_slice() else {
        panic!("expected the rebuilt R1 in the status window");
    };
    assert_eq!(after.rule, RuleId::R1);
    assert_eq!(after.step, Step::Escalated);
    assert_eq!(after.tracker, Some(REASSIGNED));
    assert_eq!(after.serial, before.serial);
    assert_ne!(after.conn, before.conn);
}

#[test]
fn rebuild_replaces_an_absent_r1_without_recovery_and_pushes_a_different_episode() {
    for expected in [RuleId::R1, RuleId::R2] {
        let mut engine = engine();
        connect(&mut engine, 0, HOST, SERIAL_A);
        unanswered_requests(&mut engine, 1, HOST);
        assert_eq!(
            pushes(&engine.drain()),
            [(RuleId::R1, Step::Raised, Some(HOST))]
        );

        engine.rebuild();
        engine.set_catching_up(true);
        let tracker = if expected == RuleId::R1 {
            connect(&mut engine, 15, A, SERIAL_B);
            unanswered_requests(&mut engine, 15, A);
            A
        } else {
            connect(&mut engine, 15, HOST, SERIAL_A);
            link(&mut engine, 15, HOST);
            engine.advance_to(at(46));
            HOST
        };
        engine.set_catching_up(false);
        let output = engine.drain();
        assert_eq!(pushes(&output), [(expected, Step::Raised, Some(tracker))]);
        assert!(notices(&output).all(|notice| notice.step != Step::Resolved));
        let live = engine.live_notices();
        let [notice] = live.as_slice() else {
            panic!("expected only the episode that exists after rebuild");
        };
        assert_eq!((notice.rule, notice.tracker), (expected, Some(tracker)));
    }
}

#[test]
fn keeps_r2_and_r4_deadlines_and_r1_escalation_during_a_hub_stop() {
    let mut engine = engine();
    connect(&mut engine, 0, HOST, SERIAL_C);
    connect(&mut engine, 0, A, SERIAL_A);
    connect(&mut engine, 0, B, SERIAL_B);
    unanswered_requests(&mut engine, 2, HOST);
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R1, Step::Raised, Some(HOST))]
    );
    link(&mut engine, 10, A);
    engine.advance_to(at(15));
    engine.set_hub_down(true);
    cut(&mut engine, 40, B);
    engine.advance_to(at(50));

    let output = engine.drain();
    let expected = [
        (RuleId::R2, Step::Raised, at(40)),
        (RuleId::R4, Step::Raised, at(45)),
        (RuleId::R1, Step::Escalated, at(49)),
    ];
    assert_eq!(
        detections(&output)
            .map(|detection| (detection.rule, detection.step, detection.at))
            .collect::<Vec<_>>(),
        expected,
    );
    for (rule, step, time) in expected {
        assert!(notices(&output).any(|notice| {
            (notice.rule, notice.step, notice.at, notice.push) == (rule, step, time, false)
        }));
    }
    assert!(pushes(&output).is_empty());
    assert_eq!(engine.stats().pushes, 1);
}

fn engine_with_waiting_r4() -> Engine {
    let mut engine = engine();
    connect(&mut engine, 0, A, SERIAL_A);
    connect(&mut engine, 0, HOST, SERIAL_B);
    cut(&mut engine, 40, A);
    unanswered_requests(&mut engine, 40, HOST);
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R1, Step::Raised, Some(HOST))]
    );
    engine.advance_to(at(46));
    let output = engine.drain();
    assert!(
        detections(&output)
            .any(|d| { (d.rule, d.step, d.at) == (RuleId::R4, Step::Raised, at(45)) })
    );
    assert!(pushes(&output).is_empty());
    engine
}

#[test]
fn far_and_stepped_advances_agree_across_notice_and_rule_deadlines() {
    let mut far = engine_with_waiting_r4();
    let mut stepped = engine_with_waiting_r4();
    for engine in [&mut far, &mut stepped] {
        connect(engine, 46, B, SERIAL_C);
        link(engine, 46, B);
    }

    far.advance_to(at(88));
    for milliseconds in (46_500..=88_000).step_by(500) {
        stepped.advance_to(LogTime(milliseconds));
    }

    let far_output = far.drain();
    assert_eq!(
        pushes(&far_output),
        [
            (RuleId::R4, Step::Raised, Some(A)),
            (RuleId::R2, Step::Raised, Some(B)),
            (RuleId::R1, Step::Escalated, Some(HOST)),
        ],
    );
    assert_eq!(far_output, stepped.drain());
    assert_eq!(far.stats(), stepped.stats());
    assert_eq!(far.live_notices(), stepped.live_notices());
}

#[test]
fn recovery_at_the_notice_deadline_cancels_the_waiting_push() {
    let mut engine = engine_with_waiting_r4();
    let delivery_at = at(52);
    engine.advance_to(delivery_at);
    assert!(pushes(&engine.drain()).is_empty());
    connect(&mut engine, 52, A, SERIAL_A);
    engine.advance_to(LogTime(delivery_at.0 + 1));

    let output = engine.drain();
    assert!(
        detections(&output)
            .any(|d| { (d.rule, d.step, d.at) == (RuleId::R4, Step::Resolved, delivery_at) })
    );
    assert!(pushes(&output).is_empty());
    assert_eq!(engine.stats().pushes, 1);
}

#[test]
fn discards_a_waiting_r4_when_hub_stops_and_does_not_restore_it() {
    let mut engine = engine_with_waiting_r4();
    engine.advance_to(at(47));
    engine.set_hub_down(true);
    engine.advance_to(at(53));
    assert!(pushes(&engine.drain()).is_empty());
    engine.set_hub_down(false);
    engine.advance_to(at(54));
    assert!(pushes(&engine.drain()).is_empty());
    assert_eq!(engine.stats().pushes, 1);
    assert!(
        engine
            .live_notices()
            .iter()
            .any(|notice| notice.rule == RuleId::R4)
    );
}

#[test]
fn hub_restart_preserves_delivered_cooldowns_and_ignores_suppressed_episodes() {
    let mut engine = engine();
    connect(&mut engine, 0, A, SERIAL_A);
    connect(&mut engine, 0, B, SERIAL_B);
    connect(&mut engine, 0, C, SERIAL_C);
    cut(&mut engine, 40, A);
    engine.advance_to(at(46));
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R4, Step::Raised, Some(A))]
    );
    engine.advance_to(at(50));
    engine.set_hub_down(true);
    link(&mut engine, 51, C);
    cut(&mut engine, 55, B);
    engine.advance_to(at(82));
    let while_down = engine.drain();
    for (rule, tracker) in [(RuleId::R2, C), (RuleId::R4, B)] {
        assert!(notices(&while_down).any(|n| {
            (n.rule, n.step, n.tracker, n.push) == (rule, Step::Raised, Some(tracker), false)
        }));
    }
    assert_eq!(engine.stats().pushes, 1);

    apply(&mut engine, 90, Event::ServerStarted);
    engine.set_hub_down(false);
    assert!(pushes(&engine.drain()).is_empty());
    assert!(
        engine
            .live_notices()
            .iter()
            .any(|n| { (n.rule, n.step, n.tracker) == (RuleId::R4, Step::Raised, Some(B)) })
    );

    connect(&mut engine, 91, REASSIGNED, SERIAL_A);
    connect(&mut engine, 92, C, SERIAL_C);
    link(&mut engine, 93, C);
    engine.advance_to(at(124));
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R2, Step::Raised, Some(C))]
    );
    cut(&mut engine, 133, REASSIGNED);
    engine.advance_to(at(139));
    let after_restart = engine.drain();
    assert!(notices(&after_restart).any(|n| {
        (n.rule, n.step, n.tracker, n.push) == (RuleId::R4, Step::Raised, Some(REASSIGNED), false)
    }));
    assert!(pushes(&after_restart).is_empty());
    assert_eq!(engine.stats().pushes, 2);
}

#[test]
fn rebuild_preserves_hub_down_and_catch_up_completion_does_not_queue_a_push() {
    let mut engine = engine();
    engine.set_hub_down(true);
    engine.rebuild();
    engine.set_catching_up(true);
    connect(&mut engine, 0, HOST, SERIAL_A);
    unanswered_requests(&mut engine, 1, HOST);
    assert!(pushes(&engine.drain()).is_empty());
    engine.set_catching_up(false);
    assert!(pushes(&engine.drain()).is_empty());
    engine.set_hub_down(false);
    engine.advance_to(at(20));
    assert!(pushes(&engine.drain()).is_empty());
    assert_eq!(engine.stats().pushes, 0);
    assert!(
        engine
            .live_notices()
            .iter()
            .any(|notice| notice.rule == RuleId::R1)
    );
}

#[test]
fn cancels_a_pending_batch_without_counting_pushes_or_starting_cooldowns() {
    let mut engine = engine();
    connect(&mut engine, 0, A, SERIAL_A);
    connect(&mut engine, 0, B, SERIAL_B);
    connect(&mut engine, 0, HOST, SERIAL_C);
    link(&mut engine, 16, B);
    cut(&mut engine, 40, A);
    engine.advance_to(at(47));

    engine.set_hub_down(true);
    let cancelled = engine.drain();
    assert!(notices(&cancelled).any(|n| {
        (n.rule, n.step, n.tracker, n.push) == (RuleId::R4, Step::Raised, Some(A), false)
    }));
    assert!(
        detections(&cancelled)
            .any(|d| { (d.rule, d.step, d.tracker) == (RuleId::R2, Step::Raised, Some(B)) })
    );
    assert!(pushes(&cancelled).is_empty());
    assert_eq!(engine.stats().pushes, 0);

    engine.advance_to(at(48));
    engine.set_hub_down(false);
    unanswered_requests(&mut engine, 49, HOST);
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R1, Step::Raised, Some(HOST))]
    );
    connect(&mut engine, 52, REASSIGNED, SERIAL_A);
    cut(&mut engine, 90, REASSIGNED);
    engine.advance_to(at(96));
    assert_eq!(
        pushes(&engine.drain()),
        [(RuleId::R4, Step::Raised, Some(REASSIGNED))]
    );
    assert_eq!(engine.stats().pushes, 2);
}

#[test]
fn updating_outlet_after_server_stop_changes_only_pushes() {
    // When the stop itself passes the deadline, the push must be cancelled in the outbox.
    for stop in [47, 53] {
        let mut gated = engine_with_waiting_r4();
        let mut ungated = engine_with_waiting_r4();
        apply(&mut gated, stop, Event::ServerStopped);
        gated.set_hub_down(true);
        apply(&mut ungated, stop, Event::ServerStopped);
        for engine in [&mut gated, &mut ungated] {
            engine.note_line(FileKind::Server, Some(at(54)), LineQuality::Other);
        }
        let gated_output = gated.drain();
        let ungated_output = ungated.drain();
        assert!(pushes(&gated_output).is_empty());
        assert_eq!(
            pushes(&ungated_output),
            [(RuleId::R4, Step::Raised, Some(A))],
        );
        assert!(detections(&gated_output).eq(detections(&ungated_output)));
        assert_eq!(gated.snapshot(), ungated.snapshot());
        let mut stats = ungated.stats().clone();
        assert_eq!(stats.pushes, 2);
        assert_eq!(gated.stats().pushes, 1);
        stats.pushes = gated.stats().pushes;
        assert_eq!(gated.stats(), &stats);
    }
}
