use super::*;
use crate::event::PoseStatus;
use crate::rules::{RuleId, Step};
use crate::tracker::TrackerKey;

fn timed(ms: i64, event: Event) -> Timed {
    Timed {
        at: LogTime(ms),
        offset: UtcOffset(0),
        event,
    }
}

fn add(key: u16) -> Event {
    Event::TrackerAdded {
        tracker: TrackerKey(key),
        usb: false,
    }
}

fn remove(key: u16) -> Event {
    Event::TrackerRemoved {
        tracker: TrackerKey(key),
        usb: false,
    }
}

fn link(key: u16) -> Event {
    Event::ClientLink {
        tracker: TrackerKey(key),
        connected: true,
    }
}

fn tracking(key: u16) -> Event {
    Event::Pose {
        tracker: TrackerKey(key),
        status: PoseStatus::Ok,
    }
}

fn detections(engine: &mut Engine) -> Vec<Detection> {
    engine
        .drain()
        .into_iter()
        .filter_map(|output| match output {
            Output::Detection(detection) => Some(detection),
            Output::Notice(_) | Output::DayClosed(_) => None,
        })
        .collect()
}

#[test]
fn summaries_count_handed_off_pushes_and_keep_the_run_total_after_rebuild() {
    let mut config = EngineConfig::default();
    config.rules.r2.wait_s = 1;
    let mut engine = Engine::new(config);
    engine.apply(timed(0, add(1)));
    engine.apply(timed(1, link(1)));
    engine.advance_to(LogTime(1_002));
    assert_eq!(engine.open_summaries()[0].pushes, 0);
    assert_eq!(detections(&mut engine).len(), 1);
    assert_eq!(engine.open_summaries()[0].pushes, 1);

    engine.rebuild();
    engine.set_hub_down(true);
    engine.apply(timed(0, add(1)));
    engine.apply(timed(1, link(1)));
    engine.advance_to(LogTime(1_002));
    assert_eq!(detections(&mut engine).len(), 1);
    let summaries = engine.open_summaries();
    assert_eq!(summaries[0].detections.r2, 1);
    assert_eq!(summaries[0].pushes, 1);
}

#[test]
fn orders_equal_deadlines_by_rule_and_tracker() {
    let mut cfg = EngineConfig::default();
    cfg.rules.r2.wait_s = 1;
    cfg.rules.r3.wait_s = 1;
    let mut engine = Engine::new(cfg);
    for key in [1, 2, 3] {
        engine.apply(timed(0, add(key)));
    }
    engine.apply(timed(1_000, link(2)));
    engine.apply(timed(1_000, link(1)));
    engine.apply(timed(
        1_000,
        Event::HostAssigned {
            tracker: TrackerKey(3),
        },
    ));
    assert_eq!(engine.next_deadline(), Some(LogTime(2_000)));
    engine.advance_to(LogTime(2_000));
    assert!(engine.drain().is_empty());
    engine.advance_to(LogTime(2_001));
    let raised: Vec<_> = detections(&mut engine)
        .into_iter()
        .map(|d| (d.at, d.rule, d.tracker, d.step))
        .collect();
    assert_eq!(
        raised,
        [
            (
                LogTime(2_000),
                RuleId::R2,
                Some(TrackerKey(1)),
                Step::Raised
            ),
            (
                LogTime(2_000),
                RuleId::R2,
                Some(TrackerKey(2)),
                Step::Raised
            ),
            (
                LogTime(2_000),
                RuleId::R3,
                Some(TrackerKey(3)),
                Step::Raised
            ),
        ]
    );
}

#[test]
fn watcher_ticks_and_replay_agree_at_timer_boundaries() {
    for late in [0, 1] {
        let inputs = [
            timed(0, add(1)),
            timed(0, add(2)),
            timed(0, link(1)),
            timed(30_000 + late, tracking(1)),
            timed(40_000, remove(2)),
            timed(45_000 + late, Event::ServerStarted),
            timed(46_000, add(3)),
        ];
        let mut replay = Engine::new(EngineConfig::default());
        let mut watcher = Engine::new(EngineConfig::default());
        let mut tick = 0;
        for event in inputs {
            while tick <= event.at.0 {
                watcher.advance_to(LogTime(tick));
                tick += 500;
            }
            replay.apply(event.clone());
            watcher.apply(event);
        }
        let replay_detections = detections(&mut replay);
        assert_eq!(replay_detections, detections(&mut watcher));
        assert_eq!(replay.stats(), watcher.stats());
        let raised: Vec<_> = replay_detections
            .iter()
            .filter(|d| d.step == Step::Raised)
            .map(|d| (d.rule, d.at))
            .collect();
        if late == 0 {
            assert!(raised.is_empty());
        } else {
            assert_eq!(
                raised,
                [(RuleId::R2, LogTime(30_000)), (RuleId::R4, LogTime(45_000))]
            );
        }
    }
}

#[test]
fn driver_lines_count_their_own_day_without_advancing_or_regressing() {
    let mut engine = Engine::new(EngineConfig::default());
    let first_day = LogDate {
        year: 2026,
        month: 11,
        day: 28,
    };
    let second_day = LogDate {
        day: 29,
        ..first_day
    };
    let offset = UtcOffset(540);
    let midnight = LogTime::from_local(second_day, 0, 0, 0, 0, offset).expect("valid date");
    let start = LogTime(midnight.0 - 60_000);
    let cut = LogTime(midnight.0 - 20_000);
    engine.apply(Timed {
        at: start,
        offset,
        event: add(1),
    });
    engine.apply(Timed {
        at: cut,
        offset,
        event: remove(1),
    });
    engine.note_line_at(
        FileKind::Driver,
        Some((midnight, offset)),
        LineQuality::Other,
    );
    engine.apply(Timed {
        at: LogTime(cut.0 - 1),
        offset,
        event: Event::PowerOffNotice {
            tracker: TrackerKey(1),
        },
    });
    assert_eq!(engine.now(), Some(cut));
    assert_eq!(engine.next_deadline(), Some(LogTime(cut.0 + 5_000)));
    assert_eq!(engine.stats().driver_days[&first_day].lines, 1);
    assert_eq!(engine.stats().driver_days[&second_day].lines, 1);
    assert_eq!(engine.stats().driver_days[&second_day].other, 1);
    assert_eq!(engine.stats().time_regressions, 0);
    assert!(engine.drain().is_empty());
    engine.advance_to(LogTime(cut.0 + 5_001));
    assert_eq!(engine.stats().power_offs, 1);
    assert!(engine.drain().is_empty());
}

#[test]
fn driver_watermark_is_earliest_pending_decision_or_end_of_the_local_day() {
    let offset = UtcOffset(540);
    let cutoff = 15 * 60 * 60 * 1_000 - 1;
    for exclude in [true, false] {
        let mut cfg = EngineConfig::default();
        cfg.rules.r4.exclude_power_off = exclude;
        let mut engine = Engine::new(cfg);
        assert_eq!(engine.driver_needed_through(), None);
        for (at, event) in [
            (0, add(1)),
            (0, add(2)),
            (40_000, remove(1)),
            (44_000, remove(2)),
        ] {
            engine.apply(Timed {
                at: LogTime(at),
                offset,
                event,
            });
        }
        assert_eq!(
            engine.driver_needed_through(),
            Some(LogTime(if exclude { 45_000 } else { cutoff }))
        );
        engine.advance_to(LogTime(45_000));
        if exclude {
            assert_eq!(engine.driver_needed_through(), Some(LogTime(45_000)));
        }
        engine.advance_to(LogTime(45_001));
        assert_eq!(
            engine.driver_needed_through(),
            Some(LogTime(if exclude { 49_000 } else { cutoff }))
        );
        engine.advance_to(LogTime(49_001));
        assert_eq!(engine.driver_needed_through(), Some(LogTime(cutoff)));
        engine.apply(Timed {
            at: LogTime(cutoff - 1),
            offset,
            event: add(3),
        });
        engine.apply(Timed {
            at: LogTime(cutoff),
            offset,
            event: remove(3),
        });
        assert_eq!(engine.driver_needed_through(), Some(LogTime(cutoff)));
    }
}

#[test]
fn regressions_use_current_time_and_rebuild_matches_fresh_engine() {
    let mut cfg = EngineConfig::default();
    cfg.rules.r2.wait_s = 1;
    let mut engine = Engine::new(cfg.clone());
    let run = |engine: &mut Engine| {
        engine.apply(timed(5_000, add(1)));
        engine.apply(timed(4_999, link(1)));
        engine.note_line(FileKind::Server, Some(LogTime(4_500)), LineQuality::Other);
        assert_eq!(engine.stats().time_regressions, 2);
        assert_eq!(engine.next_deadline(), Some(LogTime(6_000)));
        engine.advance_to(LogTime(6_001));
    };
    run(&mut engine);
    assert_eq!(engine.detections[0].at, LogTime(6_000));
    engine.rebuild();
    assert_eq!(engine.now(), None);
    assert_eq!(engine.next_deadline(), None);
    assert!(engine.drain().is_empty());
    assert_eq!(engine.stats(), &EngineStats::default());
    let mut fresh = Engine::new(cfg);
    run(&mut engine);
    run(&mut fresh);
    assert_eq!(engine.snapshot(), fresh.snapshot());
    assert_eq!(engine.stats(), fresh.stats());
    assert_eq!(detections(&mut engine), detections(&mut fresh));
}
