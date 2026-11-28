use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io;

use openut_core::display::{Line, Overall};
use openut_core::rules::{Detection, RuleId, Step};
use openut_core::state::Connection;
use openut_hublog::replay::{ReplayFile, replay_with_engine};
use openut_hublog::tail::LogFile;

use super::*;

const SERVER: &str = "ViveTrackerServer_20261128.txt";
const DRIVER: &str = "ViveTrackerDriver_20261128.txt";
const PRIOR_SERVER: &str = "ViveTrackerServer_20261127.txt";
const PRIOR_DRIVER: &str = "ViveTrackerDriver_20261127.txt";
const MIDNIGHT: i64 = 1_795_824_000_000;
const CUT: i64 = 60_000;
const DECISION: i64 = CUT + 5_000;
const HELD_ROW: i64 = CUT + 5_100;

#[derive(Default)]
struct Source {
    files: RefCell<BTreeMap<&'static str, String>>,
    unreadable: Cell<bool>,
    fail_next_list: Cell<bool>,
}

impl Source {
    fn append(&self, file: &'static str, lines: &str) {
        self.files
            .borrow_mut()
            .entry(file)
            .or_default()
            .push_str(lines);
    }

    fn replace(&self, file: &'static str, contents: String) {
        self.files.borrow_mut().insert(file, contents);
    }

    fn replay(&self, config: &WatchConfig) -> Vec<Detection> {
        let logs = self.files.borrow();
        let files: Vec<_> = logs
            .iter()
            .map(|(&name, content)| ReplayFile {
                name,
                bytes: content.as_bytes(),
            })
            .collect();
        replay_with_engine(&files, config.engine.clone()).detections
    }
}

impl LogSource for Source {
    type Error = io::Error;

    fn list(&self) -> io::Result<Vec<LogFile>> {
        if self.fail_next_list.replace(false) {
            return Err(io::Error::other("synthetic inventory failure"));
        }
        Ok(self
            .files
            .borrow()
            .iter()
            .map(|(&name, content)| LogFile {
                name: name.to_owned(),
                len: content.len() as u64,
            })
            .collect())
    }

    fn read_at(&self, name: &str, offset: u64, max: usize) -> io::Result<Option<Vec<u8>>> {
        if self.unreadable.get() {
            return Err(io::Error::other("synthetic read failure"));
        }
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        Ok(self.files.borrow().get(name).map(|content| {
            let bytes = content.as_bytes();
            let start = offset.min(bytes.len());
            let end = start.saturating_add(max).min(bytes.len());
            bytes[start..end].to_vec()
        }))
    }
}

fn time(ms: i64) -> LogTime {
    LogTime(MIDNIGHT + ms)
}

fn line(at: i64, body: &str) -> String {
    format!("[{} +0 0x1 INFO Test] [1] {body}\n", time(at))
}

fn add(at: i64, tracker: u32) -> String {
    line(
        at,
        &format!("Add new tracker: ({tracker}), connection type: 1"),
    )
}

fn link(at: i64, tracker: u32) -> String {
    line(
        at,
        &format!("Client tracker (index: {tracker}) is connected to host: 1"),
    )
}

fn identify(at: i64, tracker: u32, letter: char) -> String {
    line(
        at,
        &format!("Response - cmd type: 214, device index: {tracker}, sn: (SYNTH-TRACKER-{letter})"),
    )
}

fn tracking(at: i64, tracker: u32) -> String {
    line(
        at,
        &format!(
            "(SteamVR: 1)({tracker})[1]Pose status is changed: Pose data - status: 2, synthetic"
        ),
    )
}

fn tracked(at: i64, tracker: u32, letter: char) -> String {
    add(at, tracker) + &identify(at + 1, tracker, letter) + &tracking(at + 2, tracker)
}

fn shutdown_notice(at: i64) -> String {
    line(at, "(1) Tracker is going to power-off.")
}

fn source_before_disconnect() -> Source {
    let source = Source::default();
    source.append(
        SERVER,
        &(line(0, "Tracker Server Starts.") + &add(1_000, 1)),
    );
    source.append(DRIVER, &line(CUT - 20_000, "driver checkpoint"));
    source
}

fn append_disconnect(source: &Source, following_at: i64) {
    source.append(
        SERVER,
        &(line(
            CUT,
            "Remove tracker (1), connection type: 1, power state:11",
        ) + &line(following_at, "server checkpoint")),
    );
}

fn detections(round: &WatchRound) -> Vec<Detection> {
    round
        .outputs
        .iter()
        .filter_map(|output| match output {
            Output::Detection(detection) => Some(detection.clone()),
            _ => None,
        })
        .collect()
}

fn pushed(round: &WatchRound) -> Vec<RuleId> {
    round
        .outputs
        .iter()
        .filter_map(|output| match output {
            Output::Notice(notice) if notice.push => Some(notice.rule),
            _ => None,
        })
        .collect()
}

#[test]
fn live_driver_notice_counts_only_before_the_inclusive_wait_can_advance() {
    #[derive(Clone, Copy, PartialEq)]
    enum Arrival {
        Timely,
        None,
        Late,
    }

    for arrival in [Arrival::Timely, Arrival::None, Arrival::Late] {
        let config = WatchConfig::default();
        let mut watcher = Watcher::new(config.clone());
        let source = source_before_disconnect();
        source.append(SERVER, &line(CUT - 1_000, "before disconnect"));
        assert!(!watcher.poll(&source, time(CUT - 1_000), None).catching_up);

        append_disconnect(&source, HELD_ROW);
        let held = watcher.poll(&source, time(HELD_ROW), None);
        assert!(detections(&held).is_empty());
        assert_eq!(watcher.stats().server.lines, 4);
        assert_eq!(watcher.now(), Some(time(DECISION)));

        if arrival == Arrival::Timely {
            source.append(DRIVER, &shutdown_notice(CUT + 900));
            let credit = watcher.poll(&source, time(HELD_ROW + 3_000), None);
            assert!(detections(&credit).is_empty());
            assert_eq!(watcher.stats().driver.lines, 2);
        }

        let inclusive = watcher.poll(&source, time(DECISION + 10_000), None);
        assert!(detections(&inclusive).is_empty());
        assert_eq!(watcher.stats().server.lines, 4);
        assert_eq!(watcher.stats().power_offs, 0);

        let released = watcher.poll(&source, time(DECISION + 10_001), None);
        let detected = detections(&released);
        assert_eq!(watcher.stats().server.lines, 5);
        assert_eq!(watcher.now(), Some(time(HELD_ROW)));
        if arrival == Arrival::Timely {
            assert!(detected.is_empty());
            assert_eq!(watcher.stats().power_offs, 1);
        } else {
            assert_eq!(
                detected
                    .iter()
                    .map(|event| (event.rule, event.step, event.at))
                    .collect::<Vec<_>>(),
                [(RuleId::R4, Step::Raised, time(DECISION))]
            );
        }

        if arrival == Arrival::Late {
            source.append(DRIVER, &shutdown_notice(CUT + 900));
            assert!(detections(&watcher.poll(&source, time(DECISION + 13_000), None)).is_empty());
            assert_eq!(watcher.stats().power_offs, 0);
            assert_eq!(watcher.open_summaries()[0].source.driver.late, 1);
            assert!(source.replay(&config).is_empty());
        } else {
            assert_eq!(detected, source.replay(&config));
        }
    }
}

#[test]
fn startup_drains_held_rows_before_leaving_catch_up_and_keeps_quiet_deadlines() {
    for notice in [false, true] {
        let config = WatchConfig::default();
        let mut watcher = Watcher::new(config.clone());
        let source = source_before_disconnect();
        let last = if notice { HELD_ROW } else { CUT + 2_000 };
        let startup = CUT + 5_300;
        append_disconnect(&source, last);

        let initial = watcher.poll(&source, time(startup), None);
        assert!(detections(&initial).is_empty());
        assert_eq!(initial.catching_up, notice);
        assert_eq!(watcher.stats().server.lines, if notice { 3 } else { 4 });

        if notice {
            source.append(DRIVER, &shutdown_notice(CUT + 900));
            let credited = watcher.poll(&source, time(CUT + 5_900), None);
            assert!(credited.catching_up);
            assert!(detections(&credited).is_empty());
            assert_eq!(watcher.stats().server.lines, 3);
        }

        let same_deadline = startup + 10_000 + DECISION - last;
        let inclusive = watcher.poll(&source, time(same_deadline), None);
        assert!(detections(&inclusive).is_empty());
        let after = watcher.poll(&source, time(same_deadline + 1), None);
        assert!(!after.catching_up);
        assert_eq!(watcher.stats().server.lines, 4);
        let detected = detections(&after);
        if notice {
            assert_eq!(watcher.stats().power_offs, 1);
            assert_eq!(detected, source.replay(&config));
        } else {
            assert_eq!(
                detected
                    .iter()
                    .map(|event| (event.rule, event.step, event.at))
                    .collect::<Vec<_>>(),
                [(RuleId::R4, Step::Raised, time(DECISION))]
            );
        }
    }
}

#[test]
fn a_late_at_midnight_driver_row_reaches_its_day_before_the_summary_closes() {
    let source = Source::default();
    source.append(
        PRIOR_SERVER,
        &(line(-60_000, "Tracker Server Starts.") + &line(-1_000, "last server row")),
    );
    source.append(PRIOR_DRIVER, &line(-20_000, "last driver row"));
    let mut watcher = Watcher::new(WatchConfig::default());
    watcher.poll(&source, time(-1_000), None);

    source.append(SERVER, &line(1_000, "next day server row"));
    watcher.poll(&source, time(1_000), None);
    assert_eq!(watcher.stats().server.lines, 2);
    source.append(PRIOR_DRIVER, &shutdown_notice(-500));
    watcher.poll(&source, time(4_000), None);
    let open = watcher.open_summaries();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].source.driver.lines, 2);
    assert_eq!(open[0].source.driver.late, 0);

    watcher.poll(&source, time(11_000), None);
    assert_eq!(watcher.stats().server.lines, 3);
    source.append(SERVER, &line(600_000, "first row at the daily cutoff"));
    let closed = watcher.poll(&source, time(600_000), None);
    let prior = closed
        .outputs
        .iter()
        .find_map(|output| match output {
            Output::DayClosed(day) => Some(day),
            _ => None,
        })
        .expect("prior day closed after its late driver records");
    assert_eq!(prior.date, "2026-11-27");
    assert_eq!(prior.source.driver.lines, 2);
    assert_eq!(watcher.open_summaries()[0].source.driver.lines, 0);
}

#[test]
fn stopped_startup_suppresses_old_faults_and_accepts_only_a_new_start() {
    let mut config = WatchConfig::default();
    config.engine.rules.r2.wait_s = 1;
    config.start_wait_s = 2;
    let mut watcher = Watcher::new(config);
    let source = Source::default();
    source.append(
        SERVER,
        &(line(0, "Tracker Server Starts.")
            + &add(1_000, 1)
            + &identify(1_001, 1, 'A')
            + &link(2_000, 1)),
    );
    let present = Cell::new(false);
    let mut process = |_: &[String]| Some(present.get());

    let stopped = watcher.poll(&source, time(2_000), Some(&mut process));
    assert_eq!(stopped.hub, HubLive::Stopped);
    assert_eq!(stopped.display.line, Some(Line::HubStopped));
    assert!(pushed(&stopped).is_empty());

    let deadline = watcher.poll(&source, time(14_000), Some(&mut process));
    assert_eq!(deadline.hub, HubLive::Stopped);
    assert!(pushed(&deadline).is_empty());
    assert!(
        detections(&deadline)
            .iter()
            .any(|event| event.rule == RuleId::R2
                && event.step == Step::Raised
                && event.at == time(3_000))
    );

    present.set(true);
    let starting = watcher.poll(&source, time(24_000), Some(&mut process));
    assert_eq!(starting.hub, HubLive::Starting);
    assert_eq!(starting.display.overall, Overall::Waiting);
    assert_eq!(starting.display.line, None);
    assert_eq!(starting.display.devices.len(), 1);
    assert!(
        starting
            .display
            .devices
            .iter()
            .all(|row| row.connection == Connection::Disconnected)
    );
    assert!(starting.display.faults.is_empty());
    assert!(pushed(&starting).is_empty());

    let mismatch = watcher.poll(&source, time(26_001), Some(&mut process));
    assert_eq!(mismatch.health, WatchHealth::FormatMismatch);
    assert_eq!(
        mismatch.display.line,
        Some(Line::Health(WatchHealth::FormatMismatch))
    );
    assert!(pushed(&mismatch).is_empty());

    source.append(
        SERVER,
        &(line(27_000, "Tracker Server Starts.") + &add(27_001, 1) + &link(27_002, 1)),
    );
    let restarted = watcher.poll(&source, time(27_002), Some(&mut process));
    assert_eq!(restarted.hub, HubLive::Running);
    assert_eq!(restarted.health, WatchHealth::Ok);
    assert!(pushed(&restarted).is_empty());

    let new_fault = watcher.poll(&source, time(38_003), Some(&mut process));
    assert_eq!(pushed(&new_fault), [RuleId::R2]);
}

#[test]
fn an_appended_start_rechecks_the_process_before_the_next_periodic_check() {
    let mut watcher = Watcher::new(WatchConfig::default());
    let source = Source::default();
    source.append(SERVER, &line(0, "Tracker Server Starts."));
    let present = Cell::new(false);
    let checks = Cell::new(0);
    let mut process = |_: &[String]| {
        checks.set(checks.get() + 1);
        Some(present.get())
    };

    assert_eq!(
        watcher.poll(&source, time(0), Some(&mut process)).hub,
        HubLive::Stopped
    );
    assert_eq!(checks.get(), 1);
    present.set(true);
    source.append(SERVER, &line(5_000, "Tracker Server Starts."));
    let result = watcher.poll(&source, time(5_000), Some(&mut process));
    assert_eq!(checks.get(), 2);
    assert_eq!(result.hub, HubLive::Running);
}

#[test]
fn a_server_stop_cancels_pushes_produced_earlier_in_the_same_poll() {
    for separate_stop in [false, true] {
        let mut config = WatchConfig::default();
        config.engine.rules.r2.wait_s = 1;
        let mut watcher = Watcher::new(config.clone());
        let source = Source::default();
        source.append(
            SERVER,
            &(line(0, "Tracker Server Starts.") + &add(1_000, 1)),
        );
        let mut process = |_: &[String]| Some(true);
        watcher.poll(&source, time(1_000), Some(&mut process));

        source.append(
            SERVER,
            &(link(2_000, 1) + &line(3_001, "after the tracking deadline")),
        );
        if !separate_stop {
            source.append(SERVER, &line(3_002, "Release begin"));
        }
        let result = watcher.poll(&source, time(3_002), Some(&mut process));
        let mut detected = detections(&result);
        if separate_stop {
            assert_eq!(pushed(&result), [RuleId::R2]);
            source.append(SERVER, &line(3_002, "Release begin"));
            let stopped = watcher.poll(&source, time(3_002), Some(&mut process));
            assert!(pushed(&stopped).is_empty());
            assert_eq!(stopped.hub, HubLive::Starting);
            detected.extend(detections(&stopped));
        } else {
            assert!(pushed(&result).is_empty());
            assert_eq!(result.hub, HubLive::Starting);
        }
        assert!(
            detected
                .iter()
                .any(|event| event.rule == RuleId::R2 && event.step == Step::Raised)
        );
        assert_eq!(detected, source.replay(&config));
    }
}

#[test]
fn health_recovers_after_unreadable_rows_then_stalls_only_with_an_attached_device() {
    let mut watcher = Watcher::new(WatchConfig::default());
    let source = Source::default();
    source.append(
        SERVER,
        &(line(0, "Tracker Server Starts.") + &add(1_000, 1)),
    );
    source.unreadable.set(true);
    watcher.poll(&source, time(1_000), None);
    watcher.poll(&source, time(1_500), None);
    assert_eq!(
        watcher.poll(&source, time(2_000), None).health,
        WatchHealth::Unreadable
    );

    source.unreadable.set(false);
    assert_eq!(
        watcher.poll(&source, time(2_500), None).health,
        WatchHealth::Ok
    );
    assert_eq!(
        watcher.poll(&source, time(601_001), None).health,
        WatchHealth::Stalled
    );

    source.append(
        SERVER,
        &line(
            602_000,
            "Remove tracker (1), connection type: 1, power state:11",
        ),
    );
    assert_eq!(
        watcher.poll(&source, time(602_000), None).health,
        WatchHealth::Ok
    );
    assert_eq!(
        watcher.poll(&source, time(1_203_000), None).health,
        WatchHealth::Ok
    );
}

#[test]
fn a_replaced_server_keeps_the_previous_summary_and_does_not_repeat_r1() {
    let mut config = WatchConfig::default();
    config.engine.rules.r1.span_s = 1;
    let mut watcher = Watcher::new(config.clone());
    let source = Source::default();
    let header = line(0, "Tracker Server Starts.")
        + &tracked(1_000, 1, 'A')
        + &line(1_003, "Tracker (1) - switch to HOST mode.");
    source.append(SERVER, &header);
    watcher.poll(&source, time(1_003), None);

    for at in [2_000, 2_500, 3_000] {
        source.append(SERVER, &line(at, "Wifi info is not yet available."));
    }
    let raised = watcher.poll(&source, time(3_000), None);
    assert_eq!(pushed(&raised), [RuleId::R1]);

    // A replacement that retains the entire known prefix is an append.
    let original = source.files.borrow()[SERVER].clone();
    let appended = original + &line(4_000, "appended checkpoint");
    source.replace(SERVER, appended.clone());
    let append = watcher.poll(&source, time(4_000), None);
    assert!(
        append
            .summaries
            .iter()
            .all(|write| !matches!(write, SummaryWrite::BeforeRebuild(_)))
    );
    assert!(pushed(&append).is_empty());
    let before = watcher.open_summaries().remove(0);

    source.replace(SERVER, line(0, "different prefix") + &appended);
    let rebuilt = watcher.poll(&source, time(4_500), None);
    let previous: Vec<_> = rebuilt
        .summaries
        .iter()
        .filter_map(|write| match write {
            SummaryWrite::BeforeRebuild(summary) => Some(summary),
            SummaryWrite::Daily(_) => None,
        })
        .collect();
    assert_eq!(previous, [&before]);
    assert!(!rebuilt.catching_up);
    assert!(pushed(&rebuilt).is_empty());
    assert_eq!(watcher.stats().pushes, 1);

    let mut fresh = Watcher::new(config);
    let reread = fresh.poll(&source, time(4_500), None);
    assert_eq!(detections(&rebuilt), detections(&reread));
    let fresh = fresh.open_summaries().remove(0);
    let after = watcher.open_summaries().remove(0);
    assert_eq!(after.source.lines, before.source.lines + 1);
    assert_eq!(after.source.lines, fresh.source.lines);
    assert_eq!(after.source.rebuilds, 1);
    assert_eq!(after.detections, fresh.detections);
    assert_eq!(after.trackers, fresh.trackers);
}

#[test]
fn a_driver_rebuild_keeps_the_stop_boundary_and_start_wait_timer() {
    let config = WatchConfig {
        process_check_s: 1,
        start_wait_s: 2,
        ..WatchConfig::default()
    };
    let mut watcher = Watcher::new(config);
    let source = Source::default();
    source.append(
        SERVER,
        &(line(0, "Tracker Server Starts.") + &tracked(1_000, 1, 'A')),
    );
    source.append(DRIVER, &line(0, "original driver file"));
    let present = Cell::new(true);
    let mut process = |_: &[String]| Some(present.get());
    let running = watcher.poll(&source, time(2_000), Some(&mut process));
    assert_eq!(running.hub, HubLive::Running);
    assert_eq!(watcher.snapshot().trackers[0].phase, Phase::Tracking);

    // These bytes already exist at the negative process check, even though the
    // failing read has not advanced the observed stop timestamp to this start.
    source.append(
        SERVER,
        &(line(2_500, "Tracker Server Starts.") + &tracked(2_501, 1, 'A')),
    );
    source.unreadable.set(true);
    present.set(false);
    let stopped = watcher.poll(&source, time(3_000), Some(&mut process));
    assert_eq!(stopped.hub, HubLive::Stopped);
    assert!(pushed(&stopped).is_empty());

    present.set(true);
    let starting = watcher.poll(&source, time(4_000), Some(&mut process));
    assert_eq!(starting.hub, HubLive::Starting);
    source.replace(
        DRIVER,
        line(0, "driver replaced with a different prefix and length"),
    );
    source.unreadable.set(false);
    let rebuilt = watcher.poll(&source, time(5_500), Some(&mut process));
    assert!(
        rebuilt
            .summaries
            .iter()
            .any(|write| matches!(write, SummaryWrite::BeforeRebuild(_)))
    );
    assert!(!rebuilt.catching_up);
    assert_eq!(rebuilt.hub, HubLive::Starting);
    assert_eq!(rebuilt.display.devices.len(), 1);
    assert_eq!(
        rebuilt.display.devices[0].connection,
        Connection::Disconnected
    );
    assert!(pushed(&rebuilt).is_empty());

    let expired = watcher.poll(&source, time(6_001), Some(&mut process));
    assert_eq!(expired.hub, HubLive::Starting);
    assert_eq!(expired.health, WatchHealth::FormatMismatch);
    assert!(pushed(&expired).is_empty());

    source.append(
        SERVER,
        &(line(7_000, "Tracker Server Starts.") + &tracked(7_001, 2, 'B')),
    );
    let restarted = watcher.poll(&source, time(7_100), Some(&mut process));
    assert_eq!(restarted.hub, HubLive::Running);
    assert_eq!(restarted.health, WatchHealth::Ok);
    assert_eq!(
        watcher
            .snapshot()
            .trackers
            .iter()
            .filter(|tracker| tracker.phase == Phase::Tracking)
            .count(),
        1
    );
    assert!(pushed(&restarted).is_empty());
}

#[test]
fn a_replaced_server_accepts_a_start_after_the_stop_mark_in_its_initial_read() {
    let config = WatchConfig {
        process_check_s: 1,
        ..WatchConfig::default()
    };
    let mut watcher = Watcher::new(config);
    let source = Source::default();
    let previous = line(0, "Tracker Server Starts.") + &tracked(1_000, 1, 'A');
    source.append(SERVER, &previous);
    let present = Cell::new(true);
    let mut process = |_: &[String]| Some(present.get());
    watcher.poll(&source, time(2_000), Some(&mut process));
    present.set(false);
    assert_eq!(
        watcher.poll(&source, time(3_000), Some(&mut process)).hub,
        HubLive::Stopped
    );
    present.set(true);
    assert_eq!(
        watcher.poll(&source, time(4_000), Some(&mut process)).hub,
        HubLive::Starting
    );

    source.replace(SERVER, line(0, "first replacement") + &previous);
    let historical = watcher.poll(&source, time(5_000), Some(&mut process));
    assert!(!historical.catching_up);
    assert_eq!(historical.hub, HubLive::Starting);
    assert_eq!(historical.display.devices.len(), 1);
    assert_eq!(
        historical.display.devices[0].connection,
        Connection::Disconnected
    );

    source.replace(
        SERVER,
        line(0, "second replacement")
            + &previous
            + &line(6_000, "Tracker Server Starts.")
            + &tracked(6_001, 2, 'B'),
    );
    let restarted = watcher.poll(&source, time(6_100), Some(&mut process));
    assert!(!restarted.catching_up);
    assert_eq!(restarted.hub, HubLive::Running);
    assert_eq!(restarted.health, WatchHealth::Ok);
    let snapshot = watcher.snapshot();
    assert_eq!(
        snapshot
            .trackers
            .iter()
            .filter(|tracker| tracker.phase == Phase::Tracking)
            .map(|tracker| tracker.key.0)
            .collect::<Vec<_>>(),
        [2]
    );
    assert!(pushed(&restarted).is_empty());
}

#[test]
fn unstable_or_failed_inventory_retries_absence_before_setting_the_boundary() {
    for fail_inventory in [false, true] {
        let config = WatchConfig {
            process_check_s: 1,
            ..WatchConfig::default()
        };
        let mut watcher = Watcher::new(config);
        let source = Source::default();
        source.append(SERVER, &line(0, "Tracker Server Starts."));
        let present = Cell::new(false);
        let checks = Cell::new(0);
        let mut process = |_: &[String]| {
            checks.set(checks.get() + 1);
            if checks.get() == 1 {
                source.unreadable.set(true);
                if fail_inventory {
                    source.fail_next_list.set(true);
                } else {
                    source.append(
                        SERVER,
                        &(line(10, "Tracker Server Starts.") + &tracked(11, 1, 'A')),
                    );
                }
            }
            Some(present.get())
        };

        let first = watcher.poll(&source, time(100), Some(&mut process));
        assert_eq!(first.hub, HubLive::Unknown);
        assert_eq!(checks.get(), 1);
        assert_eq!(
            watcher.poll(&source, time(599), Some(&mut process)).hub,
            HubLive::Unknown
        );
        assert_eq!(checks.get(), 1);
        let stable = watcher.poll(&source, time(600), Some(&mut process));
        assert_eq!(stable.hub, HubLive::Stopped);
        assert_eq!(checks.get(), 2);

        present.set(true);
        source.unreadable.set(false);
        let held = watcher.poll(&source, time(1_100), Some(&mut process));
        assert_eq!(held.hub, HubLive::Stopped);
        assert!(pushed(&held).is_empty());
        assert_eq!(checks.get(), 2);
        let starting = watcher.poll(&source, time(1_600), Some(&mut process));
        assert_eq!(starting.hub, HubLive::Starting);
        source.append(SERVER, &line(1_700, "Tracker Server Starts."));
        let running = watcher.poll(&source, time(1_700), Some(&mut process));
        assert_eq!(running.hub, HubLive::Running);
    }
}

#[test]
fn format_mismatch_requires_overshooting_each_daily_threshold() {
    for (threshold, body) in [
        (100, "Add new tracker: (invalid), connection type: 1"),
        (2_000, "unrecognized synthetic log message"),
    ] {
        let source = Source::default();
        let mut watcher = Watcher::new(WatchConfig::default());
        let row = line(1_000, body);
        source.append(SERVER, &row.repeat(threshold));
        assert_eq!(
            watcher.poll(&source, time(1_000), None).health,
            WatchHealth::Ok
        );
        source.append(SERVER, &row);
        assert_eq!(
            watcher.poll(&source, time(1_500), None).health,
            WatchHealth::FormatMismatch
        );
        assert_eq!(watcher.stats().server.lines, threshold as u64 + 1);
    }
}

#[test]
fn appending_a_row_with_the_same_timestamp_clears_stalled() {
    let source = Source::default();
    source.append(
        SERVER,
        &(line(0, "Tracker Server Starts.") + &add(1_000, 1)),
    );
    let mut watcher = Watcher::new(WatchConfig::default());
    assert_eq!(
        watcher.poll(&source, time(1_000), None).health,
        WatchHealth::Ok
    );
    assert_eq!(
        watcher.poll(&source, time(601_000), None).health,
        WatchHealth::Stalled
    );

    source.append(SERVER, &line(1_000, "same timestamp, new bytes"));
    assert_eq!(
        watcher.poll(&source, time(601_500), None).health,
        WatchHealth::Ok
    );
    assert_eq!(watcher.stats().server.lines, 3);
}

#[test]
fn idle_wall_cutoff_freezes_prior_day_without_inventing_an_observation() {
    let source = Source::default();
    source.append(
        PRIOR_SERVER,
        &line(-60_000, "last row before local midnight"),
    );
    let mut watcher = Watcher::new(WatchConfig::default());
    // A large measured server lag makes the wall and useful log cutoffs distinct.
    watcher.poll(&source, time(480_000), None);
    assert_eq!(
        watcher.open_summaries()[0].observed_until,
        "2026-11-27 23:59:00.000"
    );
    let exact = watcher.poll(&source, time(600_000), None);
    assert!(
        exact
            .outputs
            .iter()
            .all(|output| !matches!(output, Output::DayClosed(_)))
    );
    let past = watcher.poll(&source, time(600_001), None);
    let final_day = past
        .outputs
        .iter()
        .find_map(|output| match output {
            Output::DayClosed(summary) => Some(summary),
            _ => None,
        })
        .expect("the wall crossed the daily cutoff");
    assert_eq!(final_day.observed_until, "2026-11-27 23:59:00.000");
    assert!(final_day.is_final);
    assert!(watcher.open_summaries().is_empty());
    assert!(watcher.now().unwrap() < time(600_000));
}
