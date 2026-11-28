//! Japanese labels, notices, and status text for the watcher.

use std::collections::BTreeSet;
use std::fmt;

use crate::display::{DisplayModel, GuideStepId, Line, Overall, UserState, WatchHealth};
use crate::rules::{Detection, RuleId, Step};
use crate::state::{ConnId, Connection, DeviceView, Identity, Snapshot};
pub use crate::tracker::TrackerLabel;
use crate::tracker::{HubNumber, Serial};

#[cfg(test)]
mod tests;

const MIN_SERIAL_TAIL: usize = 4;

impl fmt::Display for TrackerLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.serial_tail, self.hub_number, self.host) {
            (Some(tail), Some(HubNumber(number)), _) => {
                write!(formatter, "トラッカー {number}（…{tail}）")
            }
            (Some(tail), None, _) => write!(formatter, "トラッカー（…{tail}）"),
            (None, _, true) => formatter.write_str("ホストのトラッカー"),
            (None, _, false) => formatter.write_str("トラッカー"),
        }
    }
}

/// The shortest common suffix width that distinguishes the roster entries.
pub fn serial_tail_length(roster: &[Identity]) -> usize {
    let serials: BTreeSet<_> = roster
        .iter()
        .map(|identity| identity.serial.0.as_str())
        .collect();
    let max_width = serials
        .iter()
        .map(|serial| serial.chars().count())
        .max()
        .unwrap_or_default()
        .max(MIN_SERIAL_TAIL);
    (MIN_SERIAL_TAIL..=max_width)
        .find(|&width| {
            let mut tails = BTreeSet::new();
            serials
                .iter()
                .all(|serial| tails.insert(serial_tail(serial, width)))
        })
        .unwrap_or(max_width)
}

pub fn device_label(device: &DeviceView, tail_length: usize) -> TrackerLabel {
    let connected = device.connection != Connection::Disconnected;
    let hub_number = device
        .hub_number
        .filter(|_| connected && device.serial.is_some());
    TrackerLabel {
        hub_number,
        serial_tail: device
            .serial
            .as_ref()
            .map(|serial| serial_tail(&serial.0, tail_length).to_owned()),
        host: connected && device.host,
    }
}

/// Freezes a label without identifying a tracker by a reusable slot.
pub fn target_label(snapshot: &Snapshot, detection: &Detection) -> TrackerLabel {
    label_for(
        snapshot,
        detection.serial.as_ref(),
        detection.conn,
        detection.rule == RuleId::R1,
    )
}

pub fn host_label(snapshot: &Snapshot) -> TrackerLabel {
    let Some(host) = snapshot.host.as_ref() else {
        return TrackerLabel {
            host: true,
            ..TrackerLabel::default()
        };
    };
    if let Some(device) = snapshot.devices.iter().find(|device| {
        device.conn == Some(host.conn) && device.connection != Connection::Disconnected
    }) {
        let mut label = device_label(device, serial_tail_length(&snapshot.roster));
        label.host = true;
        return label;
    }
    label_for(snapshot, host.serial.as_ref(), None, true)
}

fn label_for(
    snapshot: &Snapshot,
    serial: Option<&Serial>,
    conn: Option<ConnId>,
    host: bool,
) -> TrackerLabel {
    let tail_length = serial_tail_length(&snapshot.roster);
    let matching_device = match serial {
        Some(serial) => snapshot
            .devices
            .iter()
            .find(|device| device.serial.as_ref() == Some(serial)),
        None => conn.and_then(|conn| {
            snapshot
                .devices
                .iter()
                .find(|device| device.conn == Some(conn))
        }),
    };
    if let Some(device) = matching_device {
        let mut label = device_label(device, tail_length);
        label.host |= host;
        return label;
    }
    let tail = serial
        .filter(|serial| {
            snapshot
                .roster
                .iter()
                .any(|identity| &identity.serial == *serial)
        })
        .map(|serial| serial_tail(&serial.0, tail_length).to_owned());
    TrackerLabel {
        hub_number: None,
        serial_tail: tail,
        host,
    }
}

fn serial_tail(serial: &str, length: usize) -> &str {
    serial
        .char_indices()
        .rev()
        .nth(length.max(MIN_SERIAL_TAIL) - 1)
        .map_or(serial, |(start, _)| &serial[start..])
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoticeText {
    pub headline: Vec<TextPart>,
    pub action: Vec<TextPart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextPart {
    Words(String),
    Target,
}

pub fn notice_text(rule: RuleId, step: Step, wait_s: u64, host: &TrackerLabel) -> NoticeText {
    use RuleId::{R1, R2, R3, R4, R5};
    use Step::{Escalated, Raised, Resolved};
    use TextPart::Target;

    let (headline, action) = match (rule, step) {
        (R1, Raised | Escalated) => {
            let headline = if step == Raised {
                "トラッカーの追跡が始まりません"
            } else {
                "まだ追跡が始まりません"
            };
            (
                vec![words(headline)],
                vec![
                    Target,
                    words(
                        "の電源だけを入れ直してください。ほかのトラッカーとドングルはそのままにしてください。",
                    ),
                ],
            )
        }
        (R1, Resolved) => (vec![words("直りました")], Vec::new()),
        (R2, Raised) => (
            with_target("の準備に時間がかかっています"),
            vec![
                Target,
                words(format!("を{host}の近くに置いてお待ちください。")),
            ],
        ),
        (R3, Raised) => (
            with_target("の追跡が始まりません"),
            with_target("を手に持ち、カメラを部屋の中に向けながらゆっくり動かしてください。"),
        ),
        (R4, Raised) => (
            with_target("の接続が切れました"),
            with_target("の電源が入っているか確認してください。"),
        ),
        (R4, Resolved) => (with_target("がつながりました"), Vec::new()),
        (R5, Raised) => {
            let duration = if wait_s > 0 && wait_s.is_multiple_of(60) {
                format!("{} 分", wait_s / 60)
            } else {
                format!("{wait_s} 秒")
            };
            (
                with_target(format!("の追跡が {duration}以上止まっています")),
                with_target("のカメラの前をあけ、明るい方へ向けてください。"),
            )
        }
        _ => (Vec::new(), Vec::new()),
    };
    NoticeText { headline, action }
}

pub fn add_repetitions(text: &mut NoticeText, count: u64) {
    if count > 1 {
        text.headline.push(words(format!("（{count} 回）")));
    }
}

pub fn add_other_notices(text: &mut NoticeText, count: usize) {
    if count > 0 {
        text.headline.push(words(format!("（ほか {count} 件）")));
    }
}

/// Renders the same notice for a frozen push target or the current display row.
pub fn fill_notice(text: &NoticeText, target: &TrackerLabel) -> (String, String) {
    let label = target.to_string();
    (
        fill_parts(&text.headline, &label),
        fill_parts(&text.action, &label),
    )
}

fn words(text: impl Into<String>) -> TextPart {
    TextPart::Words(text.into())
}

fn with_target(text: impl Into<String>) -> Vec<TextPart> {
    vec![TextPart::Target, words(text)]
}

fn fill_parts(parts: &[TextPart], label: &str) -> String {
    parts
        .iter()
        .map(|part| match part {
            TextPart::Words(text) => text.as_str(),
            TextPart::Target => label,
        })
        .collect()
}

pub fn guide_text(step: GuideStepId) -> &'static str {
    match step {
        GuideStepId::G1 => "VIVE Hub を起動してください。",
        GuideStepId::G2 => "ドングルを PC につないでください。",
        GuideStepId::G3 => "トラッカーの電源を入れてください。",
        GuideStepId::G4 => {
            "追跡が始まるまでお待ちください。始まらない場合は、カメラの前をふさがないように持ち、トラッカーを部屋に向けてゆっくり動かしてください。"
        }
        GuideStepId::G5 => "トラッカーを身につけてください。",
    }
}

pub fn health_text(health: WatchHealth) -> &'static str {
    match health {
        WatchHealth::Ok => "",
        WatchHealth::NotFound => guide_text(GuideStepId::G1),
        WatchHealth::Unreadable => "OpenUT を起動し直してください。",
        WatchHealth::Stalled => "VIVE Hub を起動し直してください。",
        WatchHealth::FormatMismatch => "OpenUT をアップデートしてください。",
    }
}

/// Returns a headline and an action; guide and health prompts have only an action.
pub fn line_text(line: &Line) -> (String, String) {
    match line {
        Line::Fault { headline, action } => (headline.clone(), action.clone()),
        Line::Guide(step) => (String::new(), guide_text(*step).to_owned()),
        Line::Health(health) => (String::new(), health_text(*health).to_owned()),
    }
}

pub fn tray_text(model: &DisplayModel) -> String {
    let overall = match model.overall {
        Overall::Ok => "正常",
        Overall::Waiting => "待機中",
        Overall::Fault => "異常",
    };
    match model.line.as_ref() {
        None => overall.to_owned(),
        Some(Line::Fault { headline, .. }) => headline.clone(),
        Some(line @ (Line::Health(_) | Line::Guide(GuideStepId::G1))) => {
            let (_, action) = line_text(line);
            if action.is_empty() {
                overall.to_owned()
            } else {
                format!("{overall} {action}")
            }
        }
        Some(Line::Guide(_)) => {
            let (tracking, connected) = model
                .devices
                .iter()
                .filter(|device| device.connection != Connection::Disconnected)
                .fold((0, 0), |(tracking, connected), device| {
                    (
                        tracking + usize::from(device.state == Some(UserState::Ok)),
                        connected + 1,
                    )
                });
            format!("{overall} {tracking}/{connected}")
        }
    }
}
