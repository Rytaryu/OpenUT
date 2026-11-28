use crate::display::{DeviceRow, GuideStepId, WatchHealth};
use crate::event::{Event, Timed};
use crate::state::State;
use crate::time::{LogTime, UtcOffset};
use crate::tracker::TrackerKey;

use super::*;

const A: &str = "SYNTHETICA0001";
const B: &str = "SYNTHETICA0002";
const C: &str = "SYNTHETICA0003";

fn roster(serials: &[&str]) -> Vec<Identity> {
    serials
        .iter()
        .map(|serial| Identity {
            serial: Serial((*serial).into()),
            firmware: None,
            last_seen: LogTime(0),
        })
        .collect()
}

fn device(serial: Option<&str>, number: Option<u16>, connection: Connection) -> DeviceView {
    DeviceView {
        serial: serial.map(|serial| Serial(serial.into())),
        firmware: None,
        hub_number: number.map(HubNumber),
        key: Some(TrackerKey(9)),
        conn: Some(ConnId(1)),
        host: false,
        connection,
        battery: None,
        phase: crate::state::Phase::Connected,
    }
}

#[test]
fn same_shortest_suffix_width_is_used_for_the_whole_roster() {
    for serials in [
        [A, B, C].as_slice(),
        [A, B, C, "SYNTHETICA0004", "SYNTHETICA0005"].as_slice(),
    ] {
        assert_eq!(serial_tail_length(&roster(serials)), 4);
    }

    let width = serial_tail_length(&roster(&[A, B, C, "SYNTHETICB0001"]));
    assert_eq!(width, 5);
    for (serial, suffix) in [(A, "A0001"), (B, "A0002"), (C, "A0003")] {
        let label = device_label(&device(Some(serial), None, Connection::Dongle), width);
        assert_eq!(label.serial_tail.as_deref(), Some(suffix));
    }
    assert_eq!(serial_tail("短い番号", 12), "短い番号");
    assert_eq!(serial_tail("先頭日本語末尾", 4), "本語末尾");
    assert_eq!(serial_tail_length(&roster(&[A, A])), 4);
}

#[test]
fn label_uses_hub_number_only_while_connected_and_never_displays_slot() {
    let mut active = device(Some(A), Some(1), Connection::Dongle);
    assert_eq!(
        device_label(&active, 4).to_string(),
        "トラッカー 1（…0001）"
    );
    active.key = Some(TrackerKey(4));
    active.host = true;
    assert_eq!(
        device_label(&active, 4).to_string(),
        "トラッカー 1（…0001）"
    );
    active.hub_number = Some(HubNumber(3));
    assert_eq!(
        device_label(&active, 4).to_string(),
        "トラッカー 3（…0001）"
    );
    active.connection = Connection::Disconnected;
    assert_eq!(device_label(&active, 4).to_string(), "トラッカー（…0001）");

    let mut unnamed = device(None, Some(12), Connection::Dongle);
    assert_eq!(device_label(&unnamed, 4).to_string(), "トラッカー");
    unnamed.host = true;
    assert_eq!(device_label(&unnamed, 4).to_string(), "ホストのトラッカー");
    assert_eq!(
        device_label(&device(Some("123"), None, Connection::Usb), 4).to_string(),
        "トラッカー（…123）"
    );
}

fn apply(state: &mut State, ms: i64, event: Event) {
    state.apply(&Timed {
        at: LogTime(ms),
        offset: UtcOffset(0),
        event,
    });
}

fn serial_event(tracker: TrackerKey, serial: &str, number: u16) -> Event {
    Event::SerialKnown {
        tracker,
        serial: Serial(serial.into()),
        hub_number: Some(HubNumber(number)),
        firmware: None,
    }
}

#[test]
fn frozen_push_and_current_label_follow_identity_when_the_slot_is_reused() {
    let key = TrackerKey(2);
    let mut state = State::default();
    apply(
        &mut state,
        0,
        Event::TrackerAdded {
            tracker: key,
            usb: false,
        },
    );
    apply(&mut state, 1, Event::HostAssigned { tracker: key });
    let original = state.snapshot(LogTime(1));
    let detection = Detection {
        rule: RuleId::R1,
        tracker: Some(key),
        episode: 1,
        serial: None,
        conn: original.host.as_ref().map(|host| host.conn),
        host_wifi_queried: Some(false),
        at: original.at,
        step: Step::Raised,
    };
    let original_label = target_label(&original, &detection);
    let text = notice_text(RuleId::R1, Step::Raised, 60, &host_label(&original));
    let push = fill_notice(&text, &original_label);
    assert!(push.1.starts_with("ホストのトラッカーの電源だけを"));

    apply(&mut state, 5, serial_event(key, A, 1));
    let known = state.snapshot(LogTime(5));
    assert_eq!(
        target_label(&known, &detection).to_string(),
        "トラッカー 1（…0001）"
    );
    assert_eq!(host_label(&known), target_label(&known, &detection));

    apply(
        &mut state,
        10,
        Event::TrackerRemoved {
            tracker: key,
            usb: false,
        },
    );
    apply(
        &mut state,
        11,
        Event::TrackerAdded {
            tracker: key,
            usb: false,
        },
    );
    apply(&mut state, 12, serial_event(key, "SYNTHETICB0001", 3));
    let reused = state.snapshot(LogTime(12));
    assert_eq!(
        target_label(&reused, &detection).to_string(),
        "ホストのトラッカー"
    );
    let known_target = Detection {
        serial: Some(Serial(A.into())),
        ..detection
    };
    assert_eq!(
        target_label(&reused, &known_target).to_string(),
        "トラッカー（…A0001）"
    );
    assert_eq!(host_label(&reused).to_string(), "トラッカー（…A0001）");
    assert_eq!(fill_notice(&text, &original_label), push);
}

#[test]
fn covers_the_notice_matrix_and_uses_the_configured_wait() {
    let target = TrackerLabel {
        hub_number: Some(HubNumber(2)),
        serial_tail: Some("0002".into()),
        host: false,
    };
    let host = TrackerLabel {
        hub_number: Some(HubNumber(1)),
        serial_tail: Some("0001".into()),
        host: true,
    };
    let cases = [
        (RuleId::R1, Step::Raised, "トラッカーの追跡が始まりません"),
        (RuleId::R1, Step::Escalated, "まだ追跡が始まりません"),
        (RuleId::R1, Step::Resolved, "直りました"),
        (
            RuleId::R2,
            Step::Raised,
            "トラッカー 2（…0002）の準備に時間がかかっています",
        ),
        (
            RuleId::R3,
            Step::Raised,
            "トラッカー 2（…0002）の追跡が始まりません",
        ),
        (
            RuleId::R4,
            Step::Raised,
            "トラッカー 2（…0002）の接続が切れました",
        ),
        (
            RuleId::R4,
            Step::Resolved,
            "トラッカー 2（…0002）がつながりました",
        ),
        (
            RuleId::R5,
            Step::Raised,
            "トラッカー 2（…0002）の追跡が 1 分以上止まっています",
        ),
    ];
    for (rule, step, headline) in cases {
        let label = if rule == RuleId::R1 { &host } else { &target };
        let result = fill_notice(&notice_text(rule, step, 60, &host), label);
        assert_eq!(result.0, headline);
        assert_eq!(result.1.is_empty(), step == Step::Resolved);
        if rule == RuleId::R2 {
            assert_eq!(
                result.1,
                "トラッカー 2（…0002）をトラッカー 1（…0001）の近くに置いてお待ちください。"
            );
        }
        if rule == RuleId::R1 && step != Step::Resolved {
            assert!(result.1.contains("トラッカー 1（…0001）の電源だけを"));
            assert!(result.1.contains("ほかのトラッカーとドングルはそのまま"));
        }
    }
    for (wait_s, duration) in [(15, "15 秒"), (120, "2 分")] {
        let (headline, _) = fill_notice(
            &notice_text(RuleId::R5, Step::Raised, wait_s, &host),
            &target,
        );
        assert!(headline.contains(duration));
    }

    let mut repeated = notice_text(RuleId::R4, Step::Raised, 60, &host);
    add_repetitions(&mut repeated, 2);
    add_other_notices(&mut repeated, 2);
    assert_eq!(
        fill_notice(&repeated, &target).0,
        "トラッカー 2（…0002）の接続が切れました（2 回）（ほか 2 件）"
    );
}

fn model_with_connected_tracker() -> DisplayModel {
    DisplayModel {
        overall: Overall::Waiting,
        line: None,
        faults: Vec::new(),
        guide: Vec::new(),
        dongle: crate::state::DongleState::Unknown,
        devices: vec![DeviceRow {
            label: TrackerLabel::default(),
            serial: None,
            connection: Connection::Dongle,
            battery: None,
            state: Some(UserState::Ok),
            firmware: None,
            host: false,
        }],
        greyed: false,
        pick_folder: false,
    }
}

#[test]
fn health_and_tray_text_do_not_show_stale_counts_or_internal_reasons() {
    let health = [
        (WatchHealth::Unreadable, "OpenUT を起動し直してください。"),
        (WatchHealth::Stalled, "VIVE Hub を起動し直してください。"),
        (
            WatchHealth::FormatMismatch,
            "OpenUT をアップデートしてください。",
        ),
    ];
    let mut model = model_with_connected_tracker();
    assert_eq!(tray_text(&model), "待機中");
    for (condition, action) in health {
        let line = Line::Health(condition);
        assert_eq!(line_text(&line), (String::new(), action.to_owned()));
        model.line = Some(line);
        assert_eq!(tray_text(&model), format!("待機中 {action}"));
    }
    model.line = Some(Line::Guide(GuideStepId::G1));
    assert_eq!(tray_text(&model), "待機中 VIVE Hub を起動してください。");
    model.line = Some(Line::Guide(GuideStepId::G5));
    model.overall = Overall::Ok;
    assert_eq!(tray_text(&model), "正常 1/1");
    model.line = Some(Line::Fault {
        headline: "接続が切れました".into(),
        action: String::new(),
    });
    assert_eq!(tray_text(&model), "接続が切れました");
}

#[test]
fn user_facing_templates_avoid_internal_terms_and_body_roles() {
    let target = TrackerLabel::default();
    let host = TrackerLabel {
        host: true,
        ..TrackerLabel::default()
    };
    let forbidden = [
        "地図",
        "同期",
        "Wi-Fi",
        "位置合わせ",
        "位置取り直し",
        "見張り",
        "監視",
        "ログ",
        "スロット",
        "エピソード",
        "R1",
        "R2",
        "R3",
        "R4",
        "R5",
        "Wi-Fi 待ち",
        "地図同期中",
        "追跡中",
        "腰",
        "足",
        "胸",
        "手首",
    ];
    let verify_words = |line: &str| {
        for word in forbidden {
            assert!(!line.contains(word), "{line:?} contains {word:?}");
        }
    };
    let mut model = model_with_connected_tracker();
    verify_words(&tray_text(&model));
    for rule in [RuleId::R1, RuleId::R2, RuleId::R3, RuleId::R4, RuleId::R5] {
        for step in [Step::Raised, Step::Escalated, Step::Resolved, Step::Ended] {
            let text = notice_text(rule, step, 60, &host);
            for label in [&target, &host] {
                let (headline, action) = fill_notice(&text, label);
                verify_words(&headline);
                verify_words(&action);
                model.line = Some(Line::Fault { headline, action });
                verify_words(&tray_text(&model));
            }
        }
    }
    for guide in [
        GuideStepId::G1,
        GuideStepId::G2,
        GuideStepId::G3,
        GuideStepId::G4,
        GuideStepId::G5,
    ] {
        verify_words(guide_text(guide));
        model.line = Some(Line::Guide(guide));
        verify_words(&tray_text(&model));
    }
    for health in [
        WatchHealth::Ok,
        WatchHealth::NotFound,
        WatchHealth::Unreadable,
        WatchHealth::Stalled,
        WatchHealth::FormatMismatch,
    ] {
        verify_words(health_text(health));
        model.line = Some(Line::Health(health));
        verify_words(&tray_text(&model));
    }
}
