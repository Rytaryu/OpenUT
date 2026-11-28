//! Display only the documented, non-identifying parts of decoded reports.

use serde_json::{Value, json};

use crate::decode::{DeviceInfoField, DongleEvent, TrackerMessage};
use crate::tracker::{Link, TrackerView, Transition, safe_firmware, safe_role_id};

/// A JSON row suitable for offline diagnostics and replay expectations.
pub fn event_json(mono_us: u64, wall_ms: Option<i64>, event: &DongleEvent) -> Value {
    let mut row = match event {
        DongleEvent::Pair { tracker, unpaired } => {
            json!({"type": "pair", "tracker": tracker, "unpaired": unpaired})
        }
        DongleEvent::Text { tracker, message } => {
            json!({"type": "text", "tracker": tracker, "message": message_json(message)})
        }
        DongleEvent::Pose { tracker, sample } => json!({
            "type": "pose",
            "tracker": tracker,
            "slot": sample.slot,
            "buttons_raw": sample.buttons_raw,
            "position_m": sample.position_m,
            "rotation_wxyz": sample.rotation_wxyz,
            "acceleration": sample.acceleration,
            "angular_velocity": &sample.angular_velocity[..3],
            "tracking_status": sample.tracking_status,
        }),
        DongleEvent::RfStatus { slots } => json!({
            "type": "rf_status",
            "slots": slots.map(|slot| json!({
                "status": slot.status,
                "timeout_ms": slot.timeout_ms,
                "flags": slot.flags,
            })),
        }),
        DongleEvent::OtherTrackerData { tracker, kind, len } => {
            json!({"type": "other_tracker_data", "tracker": tracker, "kind": kind, "len": len})
        }
        DongleEvent::Unknown { report_type, len } => {
            json!({"type": "unknown", "report_type": report_type, "len": len})
        }
        DongleEvent::Malformed { report_type, .. } => {
            json!({"type": "malformed", "report_type": report_type})
        }
    };
    row["mono_us"] = json!(mono_us);
    if let Some(wall_ms) = wall_ms {
        row["wall_ms"] = json!(wall_ms);
    }
    row
}

fn message_json(message: &TrackerMessage) -> Value {
    let kind = match message {
        TrackerMessage::DeviceInfo { field, value } => {
            let (field, value) = match field {
                DeviceInfoField::Nads => ("NADS", None),
                DeviceInfoField::Nass => ("NASS", None),
                DeviceInfoField::Nasi => ("NASI", None),
                DeviceInfoField::Napi => ("NAPI", None),
                DeviceInfoField::Nav1 => ("NAV1", safe_firmware(value)),
                DeviceInfoField::Nari => ("NARI", safe_role_id(value)),
                DeviceInfoField::Nagn => ("NAGN", None),
                DeviceInfoField::Nana => ("NANA", None),
                DeviceInfoField::Nadn => ("NADN", None),
                DeviceInfoField::Nadi => ("NADI", None),
            };
            let mut row = json!({"type": "device_info", "field": field});
            if let Some(value) = value {
                row["value"] = json!(value);
            }
            return row;
        }
        TrackerMessage::MapStatus { a, b } => return json!({"type": "map_status", "a": a, "b": b}),
        TrackerMessage::LambdaStatus(values) => {
            return json!({"type": "lambda_status", "values": values});
        }
        TrackerMessage::SlamStatus { key, value } => {
            return json!({"type": "slam_status", "key": key, "value": value});
        }
        TrackerMessage::SlamError { code } => return json!({"type": "slam_error", "code": code}),
        TrackerMessage::WifiConnected { ok } => return json!({"type": "wifi_connected", "ok": ok}),
        TrackerMessage::Nazz => "nazz",
        TrackerMessage::SlamAttributes(_) => "slam_attributes",
        TrackerMessage::DeviceError(_) => "device_error",
        TrackerMessage::Calibration(_) => "calibration",
        TrackerMessage::WifiHostInfo => "wifi_host_info",
        TrackerMessage::WifiInfoRequest => "wifi_info_request",
        TrackerMessage::WifiSetting(_) => "wifi_setting",
        TrackerMessage::PowerOff => "power_off",
        TrackerMessage::Reset => "reset",
        TrackerMessage::Unknown { .. } => "unknown",
    };
    json!({"type": kind})
}

/// A tracker row for the monitor. The optional slot is the decoder's estimate.
pub fn tracker_line(view: &TrackerView, slot_hint: Option<u8>) -> String {
    let link = if view.link == Link::Receiving && !view.receiving_data {
        "つながった"
    } else {
        link_name(view.link)
    };
    let mut fields = vec![format!("トラッカー {}", view.label.0), link.to_owned()];
    if let Some(slot) = slot_hint {
        fields.push(format!("枠 {slot}（推定）"));
    }
    let tracking = view.tracking_status.map_or_else(
        || "不明".to_owned(),
        |status| tracking_text(i64::from(status)),
    );
    fields.push(format!("追跡 {tracking}（姿勢）"));
    if let Some(status) = view.slam_tracking_status {
        fields.push(format!("追跡 {}（P61:6）", tracking_text(status)));
    }
    if let Some(pose) = &view.last_pose {
        let [x, y, z] = pose.position_m;
        if pose
            .position_m
            .iter()
            .all(|coordinate| coordinate.is_finite())
        {
            fields.push(format!("位置 ({x:.2}, {y:.2}, {z:.2}) m"));
        }
        fields.push(format!("電池・ボタン {:02X}", pose.buttons_raw));
    }
    fields.push(format!("姿勢 {:.0} 件/秒", view.pose_rate_hz));
    fields.push(map_text(view));
    fields.push(format!(
        "ホスト {}",
        match view.is_host {
            Some(true) => "はい",
            Some(false) => "いいえ",
            None => "不明",
        }
    ));
    if let Some(role) = &view.role_id {
        fields.push(format!("役割 {role}"));
    }
    if let Some(error) = view.last_error {
        fields.push(format!("SLAM の誤り {error} ({})", slam_error_name(error)));
    }
    fields.join(" / ")
}

/// A tracker link change in elapsed time.
pub fn transition_line(transition: &Transition) -> String {
    format!(
        "{} 秒 トラッカー {} {} → {}",
        elapsed_text(transition.at_us),
        transition.tracker.0,
        link_name(transition.from),
        link_name(transition.to),
    )
}

pub(crate) fn elapsed_text(mono_us: u64) -> String {
    format!("{}.{:03}", mono_us / 1_000_000, mono_us / 1_000 % 1_000)
}

pub(crate) fn tracking_text(status: i64) -> String {
    let name = match status {
        -1 => "準備中",
        0 => "カメラ画像なし",
        1 => "位置合わせ待ち",
        2 => "追跡中",
        3 => "喪失、向きだけ有効",
        4 => "直前に喪失、位置は停止",
        _ => "不明",
    };
    format!("{status} ({name})")
}

pub(crate) fn map_text(view: &TrackerView) -> String {
    let ms = view
        .map_state
        .map_or_else(|| "不明".to_owned(), |v| v.to_string());
    let slam = view
        .slam_map_state
        .map_or_else(|| "不明".to_owned(), |v| v.to_string());
    format!("地図の状態: MS={ms}、P61:5={slam}")
}

fn link_name(link: Link) -> &'static str {
    match link {
        Link::NotSeen => "未受信",
        Link::Receiving => "受信中",
        Link::Silent => "途切れた",
        Link::Unpaired => "外れた",
    }
}

/// The documented meaning of a received SLAM error code, if known.
pub fn slam_error_name(code: u32) -> &'static str {
    match code {
        1100 => "カメラなし",
        1121 | 1122 => "カメラの異常",
        1200 => "IMU なし",
        1300 => "姿勢なし",
        _ => "不明",
    }
}
