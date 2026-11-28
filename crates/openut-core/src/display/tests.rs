use super::*;
use crate::event::{Event, Timed};
use crate::state::{ConnId, State};
use crate::time::{LogTime, UtcOffset};
use crate::tracker::{HubNumber, TrackerKey};

const S1: &str = "00XX0000A0001";
const S2: &str = "00XX0000A0002";
const S3: &str = "00XX0000A0003";
const S4: &str = "00XX0000B0001";

fn device(key: u16, serial: Option<&str>, phase: Phase) -> DeviceView {
    let connected = phase != Phase::Disconnected;
    DeviceView {
        serial: serial.map(|serial| Serial(serial.to_owned())),
        firmware: serial.map(|_| "9.0.0".to_owned()),
        hub_number: connected.then_some(HubNumber(key)),
        key: connected.then_some(TrackerKey(key)),
        conn: connected.then_some(ConnId(u64::from(key))),
        connection: if connected {
            Connection::Dongle
        } else {
            Connection::Disconnected
        },
        battery: connected.then_some(Battery {
            percent: 70,
            charging: Some(true),
        }),
        host: false,
        phase,
    }
}

fn snapshot(devices: Vec<DeviceView>) -> Snapshot {
    Snapshot {
        roster: devices
            .iter()
            .filter_map(|row| {
                Some(Identity {
                    serial: row.serial.clone()?,
                    firmware: row.firmware.clone(),
                    last_seen: LogTime(0),
                })
            })
            .collect(),
        devices,
        hub: HubState::Running,
        ..State::default().snapshot(LogTime(0))
    }
}

fn notice(
    rule: RuleId,
    step: Step,
    time: u16,
    serial: Option<&str>,
    conn: Option<ConnId>,
) -> Notice {
    let target = TrackerLabel {
        hub_number: None,
        serial_tail: serial.map(|id| id[id.len() - 4..].to_owned()),
        host: rule == RuleId::R1,
    };
    Notice {
        rule,
        tracker: Some(TrackerKey(2)),
        episode: u64::from(time),
        serial: serial.map(|id| Serial(id.to_owned())),
        conn,
        step,
        at: LogTime(i64::from(time)),
        target,
        host: None,
        repetitions: 1,
        other_count: 0,
        push: true,
        priority: match rule {
            RuleId::R1 => 5,
            RuleId::R2 => 4,
            RuleId::R3 => 3,
            RuleId::R4 => 2,
            RuleId::R5 => 1,
        },
    }
}

fn current(snapshot: &Snapshot, live: &[Notice]) -> DisplayModel {
    view(snapshot, live, WatchHealth::Ok, HubLive::Running, false)
}

fn labels(model: &DisplayModel) -> Vec<(Option<u16>, Option<&str>)> {
    model
        .devices
        .iter()
        .map(|row| {
            (
                row.label.hub_number.map(|number| number.0),
                row.label.serial_tail.as_deref(),
            )
        })
        .collect()
}

#[test]
fn tracking_waiting_and_disconnected_rows_use_only_public_states() {
    let source = snapshot(vec![
        device(1, Some(S1), Phase::Tracking),
        device(2, Some(S2), Phase::WaitingWifi),
        device(3, Some(S3), Phase::Disconnected),
        device(4, None, Phase::Connected),
        device(5, None, Phase::MapSyncing),
        device(6, None, Phase::Relocalizing),
    ]);
    let model = current(&source, &[]);
    let states = |model: &DisplayModel| {
        model
            .devices
            .iter()
            .map(|row| row.state)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        states(&model),
        [
            Some(UserState::Ok),
            Some(UserState::Waiting),
            None,
            Some(UserState::Waiting),
            Some(UserState::Waiting),
            Some(UserState::Waiting)
        ]
    );
    assert!(model.devices[2].battery.is_none());
    assert!(model.devices[2].label.hub_number.is_none());
    assert!(!model.devices[2].host);
    assert!(model.devices[3].serial.is_none());
    assert!(model.devices[3].label.serial_tail.is_none());
    assert!(model.devices[3].firmware.is_none());

    let r1 = notice(RuleId::R1, Step::Raised, 1, Some(S1), None);
    let r4 = notice(RuleId::R4, Step::Raised, 2, Some(S3), None);
    let model = current(&source, &[r1, r4]);
    assert_eq!(model.devices[0].state, Some(UserState::Fault));
    assert_eq!(model.devices[1].state, Some(UserState::Waiting));
    assert_eq!(model.devices[2].state, Some(UserState::Fault));
    assert_eq!(model.devices[2].connection, Connection::Disconnected);

    let r4 = notice(RuleId::R4, Step::Resolved, 3, Some(S3), None);
    let model = current(&source, &[r4]);
    assert_eq!(model.overall, Overall::Waiting);
    assert_eq!(model.devices[2].state, None);
    assert_eq!(model.faults[0].step, Step::Resolved);

    let unassigned = notice(RuleId::R1, Step::Raised, 4, None, None);
    let model = current(&source, &[unassigned]);
    assert_eq!(model.overall, Overall::Fault);
    assert!(!states(&model).contains(&Some(UserState::Fault)));
}

#[test]
fn a_notice_follows_a_provisional_connection_into_the_roster_but_not_to_a_new_connection() {
    let mut state = State::default();
    let tracker = TrackerKey(2);
    let mut at = 0;
    let mut apply = |state: &mut State, event| {
        state.apply(&Timed {
            at: LogTime(at),
            offset: UtcOffset(0),
            event,
        });
        at += 1_000;
    };
    apply(
        &mut state,
        Event::Status {
            tracker,
            connected: true,
            host: true,
            map_ready: false,
            usb: None,
            battery: None,
        },
    );
    let before = state.snapshot(LogTime(0));
    let conn = before.devices[0].conn;
    let r1 = notice(RuleId::R1, Step::Raised, 1, None, conn);
    let model = current(&before, std::slice::from_ref(&r1));
    assert!(model.devices[0].serial.is_none());
    assert_eq!(model.devices[0].state, Some(UserState::Fault));
    assert!(model.faults[0].target.host);
    assert_eq!(model.faults[0].target.serial_tail, None);

    apply(
        &mut state,
        Event::SerialKnown {
            tracker,
            serial: Serial(S1.into()),
            hub_number: Some(HubNumber(7)),
            firmware: None,
        },
    );
    let known = current(&state.snapshot(LogTime(1_000)), std::slice::from_ref(&r1));
    assert_eq!(known.devices.len(), 1);
    assert_eq!(known.devices[0].state, Some(UserState::Fault));
    assert_eq!(labels(&known), [(Some(7), Some("0001"))]);
    assert_eq!(known.faults[0].target, known.devices[0].label);

    apply(
        &mut state,
        Event::TrackerRemoved {
            tracker,
            usb: false,
        },
    );
    apply(&mut state, Event::TrackerAdded { tracker, usb: true });
    apply(
        &mut state,
        Event::SerialKnown {
            tracker,
            serial: Serial(S2.into()),
            hub_number: Some(HubNumber(3)),
            firmware: None,
        },
    );
    let reconnected = state.snapshot(LogTime(4_000));
    let mut anonymous_r4 = notice(RuleId::R4, Step::Raised, 2, None, conn);
    anonymous_r4.target.serial_tail = Some("SYNTH".into());
    let fallback = anonymous_r4.target.clone();
    let model = current(&reconnected, &[r1.clone(), anonymous_r4]);
    assert_eq!(model.overall, Overall::Fault);
    assert_eq!(model.devices[0].state, None);
    assert_eq!(model.devices[1].state, Some(UserState::Waiting));
    assert!(!model.devices[0].host);
    assert_eq!(model.faults[0].target, fallback);

    let mut escalated = notice(
        RuleId::R1,
        Step::Escalated,
        4,
        Some(S1),
        reconnected.devices[1].conn,
    );
    escalated.episode = r1.episode;
    let model = current(&reconnected, &[escalated]);
    assert_eq!(model.devices[0].state, Some(UserState::Fault));
    assert_eq!(model.devices[1].state, Some(UserState::Waiting));
}

fn current_with_faults() -> (Snapshot, [Notice; 2]) {
    let mut source = snapshot(vec![
        device(1, Some(S1), Phase::Tracking),
        device(2, Some(S2), Phase::Tracking),
        device(3, Some(S3), Phase::Tracking),
        device(4, None, Phase::Tracking),
    ]);
    source.devices[0].host = true;
    source.dongle = DongleState::Connected;
    let live = [
        notice(RuleId::R1, Step::Raised, 1, Some(S1), None),
        notice(RuleId::R4, Step::Raised, 2, Some(S2), None),
    ];
    (source, live)
}

fn assert_inactive_devices(model: &DisplayModel) {
    assert_eq!(model.overall, Overall::Waiting);
    assert_eq!(model.dongle, DongleState::Unknown);
    assert!(model.faults.is_empty());
    assert!(!model.greyed);
    assert_eq!(
        labels(model),
        [
            (None, Some("0001")),
            (None, Some("0002")),
            (None, Some("0003"))
        ]
    );
    for row in &model.devices {
        assert_eq!(row.connection, Connection::Disconnected);
        assert!(row.serial.is_some());
        assert_eq!(row.firmware.as_deref(), Some("9.0.0"));
        assert!(!row.host);
        assert_eq!(row.label.hub_number, None);
        assert_eq!(row.battery, None);
        assert_eq!(row.state, None);
    }
}

#[test]
fn stopped_hub_overrides_catch_up_and_health_and_uses_only_the_roster() {
    let (mut source, live) = current_with_faults();
    let stopped = view(&source, &live, WatchHealth::Ok, HubLive::Stopped, false);
    assert_inactive_devices(&stopped);
    assert_eq!(stopped.line, Some(Line::HubStopped));
    assert!(!stopped.pick_folder);
    for health in [WatchHealth::Ok, WatchHealth::Stalled] {
        assert_eq!(
            view(&source, &live, health, HubLive::Stopped, true),
            stopped
        );
    }
    let missing = view(
        &source,
        &live,
        WatchHealth::NotFound,
        HubLive::Stopped,
        true,
    );
    assert_eq!(
        missing,
        DisplayModel {
            pick_folder: true,
            ..stopped.clone()
        }
    );

    source.hub = HubState::Stopped;
    assert_eq!(
        view(
            &source,
            &live,
            WatchHealth::Stalled,
            HubLive::Unknown,
            false
        ),
        stopped
    );
    let caught = view(&source, &live, WatchHealth::Ok, HubLive::Unknown, true);
    assert!(caught.greyed);
    assert_eq!(caught.line, None);
}

#[test]
fn starting_hub_hides_the_previous_session_even_during_catch_up() {
    let (mut source, live) = current_with_faults();
    let starting = view(&source, &live, WatchHealth::Ok, HubLive::Starting, false);
    assert_inactive_devices(&starting);
    assert!(!starting.pick_folder);
    assert_eq!(starting.line, None);
    assert_eq!(
        view(&source, &live, WatchHealth::Ok, HubLive::Starting, true),
        starting
    );
    for health in [WatchHealth::FormatMismatch, WatchHealth::Stalled] {
        let model = view(&source, &live, health, HubLive::Starting, true);
        assert_eq!(
            model,
            DisplayModel {
                line: Some(Line::Health(health)),
                ..starting.clone()
            }
        );
    }
    let running = current(&source, &live);
    assert_eq!(running.overall, Overall::Fault);
    assert_eq!(running.devices.len(), 4);
    assert!(running.devices[0].host);
    assert!(running.devices[0].battery.is_some());
    let catch = view(&source, &live, WatchHealth::Ok, HubLive::Running, true);
    assert_eq!(
        catch,
        DisplayModel {
            overall: Overall::Waiting,
            line: None,
            greyed: true,
            ..running
        }
    );

    source.hub = HubState::Stopped;
    assert_eq!(
        view(&source, &live, WatchHealth::Ok, HubLive::Running, true),
        starting
    );
}

#[test]
fn missing_and_unconfirmed_logs_have_their_own_lines_and_visibility() {
    let (source, live) = current_with_faults();
    let current = current(&source, &live);
    for health in [
        WatchHealth::Unreadable,
        WatchHealth::Stalled,
        WatchHealth::FormatMismatch,
    ] {
        let model = view(&source, &live, health, HubLive::Unknown, false);
        let line = Line::Health(health);
        assert_eq!(
            model,
            DisplayModel {
                overall: Overall::Waiting,
                line: Some(line),
                greyed: true,
                ..current.clone()
            }
        );
    }
    let missing = view(
        &source,
        &live,
        WatchHealth::NotFound,
        HubLive::Running,
        true,
    );
    assert_eq!(missing.overall, Overall::Waiting);
    assert_eq!(missing.line, Some(Line::Health(WatchHealth::NotFound)));
    assert!(missing.pick_folder);
    assert!(!missing.greyed);
    assert!(missing.devices.is_empty());
    assert!(missing.faults.is_empty());
}

#[test]
fn tracking_summary_is_ready_only_when_all_connected_trackers_are_ready() {
    let mut source = snapshot(vec![
        device(1, Some(S1), Phase::Tracking),
        device(2, Some(S2), Phase::Tracking),
        device(3, Some(S3), Phase::Relocalizing),
        device(4, Some(S4), Phase::Disconnected),
    ]);
    let running = current(&source, &[]);
    assert_eq!(running.overall, Overall::Waiting);
    assert_eq!(running.line, None);

    source.dongle = DongleState::Connected;
    source.devices[2].phase = Phase::Tracking;
    let tracking = current(&source, &[]);
    assert_eq!(tracking.overall, Overall::Ok);
    assert_eq!(tracking.line, None);
    let empty = current(&snapshot(vec![]), &[]);
    assert_eq!(empty.overall, Overall::Waiting);
    assert_eq!(empty.line, None);
}

#[test]
fn r2_fault_keeps_the_original_host_and_current_target() {
    let source = snapshot(vec![device(1, Some(S1), Phase::MapSyncing)]);
    let mut r2 = notice(RuleId::R2, Step::Raised, 1, Some(S1), None);
    r2.host = Some(TrackerLabel {
        hub_number: Some(HubNumber(2)),
        serial_tail: Some("0002".into()),
        host: true,
    });
    let model = current(&source, std::slice::from_ref(&r2));
    assert_eq!(model.faults[0].host, r2.host);
    assert_eq!(model.faults[0].target, model.devices[0].label);
}

#[test]
fn top_line_prioritizes_notices_before_time_and_list_orders_newest_first() {
    let (source, mut live) = current_with_faults();
    live[0].at = LogTime(1);
    live[1].at = LogTime(10);
    live[1].repetitions = 2;
    let escalated = notice(RuleId::R1, Step::Escalated, 5, Some(S3), None);
    let recovery = notice(RuleId::R4, Step::Resolved, 11, Some(S1), None);
    let model = current(
        &source,
        &[live[1].clone(), live[0].clone(), escalated, recovery],
    );
    assert_eq!(model.overall, Overall::Fault);
    assert_eq!(
        model
            .faults
            .iter()
            .map(|row| (row.rule, row.step, row.target.hub_number))
            .collect::<Vec<_>>(),
        [
            (RuleId::R4, Step::Resolved, Some(HubNumber(1))),
            (RuleId::R4, Step::Raised, Some(HubNumber(2))),
            (RuleId::R1, Step::Escalated, Some(HubNumber(3))),
            (RuleId::R1, Step::Raised, Some(HubNumber(1))),
        ]
    );
    assert_eq!(model.faults[1].repetitions, 2);
    assert_eq!(model.line, Some(Line::Fault(model.faults[2].clone())));
}

#[test]
fn labels_stay_in_serial_order_across_number_host_and_tail_changes() {
    let mut rows = vec![
        device(3, Some(S3), Phase::Tracking),
        device(1, Some(S1), Phase::Tracking),
        device(2, Some(S2), Phase::Tracking),
    ];
    let model = |rows: &[DeviceView]| current(&snapshot(rows.to_vec()), &[]);
    assert_eq!(
        labels(&model(&rows)),
        [
            (Some(1), Some("0001")),
            (Some(2), Some("0002")),
            (Some(3), Some("0003"))
        ]
    );

    rows[1] = device(1, Some(S1), Phase::Disconnected);
    assert_eq!(
        labels(&model(&rows)),
        [
            (None, Some("0001")),
            (Some(2), Some("0002")),
            (Some(3), Some("0003"))
        ]
    );
    rows[1] = device(3, Some(S1), Phase::Tracking);
    rows[0] = device(1, Some(S3), Phase::Tracking);
    rows[2].host = true;
    let swapped_model = model(&rows);
    let swapped = labels(&swapped_model);
    assert_eq!(
        swapped,
        [
            (Some(3), Some("0001")),
            (Some(2), Some("0002")),
            (Some(1), Some("0003"))
        ]
    );
    rows[2].host = false;
    rows[0].host = true;
    let changed = model(&rows);
    assert_eq!(labels(&changed), swapped);
    assert!(changed.devices[2].host);

    rows.push(device(5, Some("00XX0000A0005"), Phase::Tracking));
    rows.push(device(4, Some(S4), Phase::Tracking));
    let notice = notice(RuleId::R1, Step::Raised, 1, Some(S1), None);
    let extended = current(&snapshot(rows.clone()), &[notice]);
    assert_eq!(
        labels(&extended),
        [
            (Some(3), Some("A0001")),
            (Some(2), Some("A0002")),
            (Some(1), Some("A0003")),
            (Some(5), Some("A0005")),
            (Some(4), Some("B0001")),
        ]
    );
    assert_eq!(extended.faults[0].target, extended.devices[0].label);
    rows.insert(0, device(6, None, Phase::WaitingWifi));
    rows.insert(0, device(7, None, Phase::Tracking));
    let provisional = model(&rows);
    assert_eq!(labels(&provisional)[..5], labels(&extended));
    assert_eq!(provisional.devices[5].state, Some(UserState::Waiting));
    assert_eq!(provisional.devices[6].state, Some(UserState::Ok));
}
