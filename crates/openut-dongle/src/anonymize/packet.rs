//! Rebuilds the documented portions of a report on an empty buffer.

use openut_anonymize::Kind;

use crate::decode::{
    Decoder, DongleEvent, PAIRING, PoseSample, TRACKER_DATA, TRACKER_POSE, TRACKER_TEXT,
    addr::report_address_range, declared_report, normalize,
};
use crate::record::{OmittedReason, TrackerLabel};

const TRACKER_HEADER_LEN: usize = 12;
const SAFE_POSE_PREFIX_LEN: usize = 34;
const TRACKING_STATUS_OFFSET: usize = 36;

pub(super) fn report_type(bytes: &[u8]) -> u8 {
    normalize(bytes).first().copied().unwrap_or(0)
}

pub(super) fn tracker(bytes: &[u8], decoder: &mut Decoder) -> Option<TrackerLabel> {
    match report_type(bytes) {
        TRACKER_DATA => decoder.tracker_data_label(bytes),
        PAIRING => match decoder.decode(bytes) {
            DongleEvent::Pair { tracker, .. } => Some(tracker),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn private_values(bytes: &[u8]) -> Vec<&[u8]> {
    let report = normalize(bytes);
    let Some((TRACKER_TEXT, range)) = tracker_payload(report) else {
        return Vec::new();
    };
    // A truncated notification can still contain a private value we need to look for.
    let payload = &report[range.start..range.end.min(report.len())];
    let (_, body) = separate_slot(payload);
    if let Some(value) = body.strip_prefix(b"WH") {
        return value
            .split(|&byte| byte == b',')
            .take(2)
            .filter(|part| part.len() >= 4)
            .collect();
    }
    private_prefix(body)
        .and_then(|(prefix, _)| body.get(prefix..))
        .filter(|value| value.len() >= 4)
        .into_iter()
        .collect()
}

pub(super) fn rebuild(
    bytes: &[u8],
    tracker: Option<TrackerLabel>,
    event: &DongleEvent,
    replace: &mut dyn FnMut(Option<Kind>, &str) -> String,
) -> Result<Vec<u8>, OmittedReason> {
    let report = normalize(bytes);
    let report_type = report.first().copied().ok_or(OmittedReason::Malformed)?;
    let mut output = vec![0; bytes.len()];
    let rebuilt = &mut output[bytes.len() - report.len()..];

    match report_type {
        PAIRING => {
            let DongleEvent::Pair { .. } = event else {
                return Err(OmittedReason::Malformed);
            };
            if !declared_report(report).is_ok_and(|report| (12..=64).contains(&report.len())) {
                return Err(OmittedReason::Malformed);
            }
            rebuilt[..2].copy_from_slice(&report[..2]);
            rebuilt[5] = report[5];
        }
        TRACKER_DATA => {
            let (kind, range) = tracker_payload(report).ok_or(OmittedReason::Malformed)?;
            let payload = report.get(range.clone()).ok_or(if kind == TRACKER_TEXT {
                OmittedReason::TextForm
            } else {
                OmittedReason::Malformed
            })?;
            match kind {
                TRACKER_POSE => rebuild_pose(payload, &mut rebuilt[range], event)?,
                TRACKER_TEXT => rebuild_text(payload, &mut rebuilt[range], event, replace)?,
                _ => return Err(OmittedReason::NotAllowed),
            }
            rebuilt[..3].copy_from_slice(&report[..3]);
            rebuilt[9..TRACKER_HEADER_LEN].copy_from_slice(&report[9..TRACKER_HEADER_LEN]);
        }
        _ if matches!(event, DongleEvent::Malformed { .. }) => {
            return Err(OmittedReason::Malformed);
        }
        _ => return Err(OmittedReason::NotAllowed),
    }

    let range = report_address_range(report_type).ok_or(OmittedReason::Malformed)?;
    let address = report.get(range.clone()).ok_or(OmittedReason::Malformed)?;
    let label = tracker
        .filter(|label| label.0.is_ascii_uppercase())
        .ok_or(OmittedReason::Malformed)?;
    let index = label.0 as u8 - b'A' + 1;
    rebuilt[range].copy_from_slice(&[0, 0xa0 | (address[1] & 0x0f), 0, 0, 0, index]);

    Ok(output)
}

fn tracker_payload(report: &[u8]) -> Option<(u16, std::ops::Range<usize>)> {
    let header = report.first_chunk::<TRACKER_HEADER_LEN>()?;
    if header[0] != TRACKER_DATA {
        return None;
    }
    let kind = u16::from_le_bytes([header[9], header[10]]);
    let length = usize::from(header[11]);
    Some((kind, TRACKER_HEADER_LEN..TRACKER_HEADER_LEN + length))
}

fn rebuild_pose(
    payload: &[u8],
    output: &mut [u8],
    event: &DongleEvent,
) -> Result<(), OmittedReason> {
    if !matches!(payload.len(), 37 | 39 | 50) {
        return Err(OmittedReason::CheckFailed);
    }
    let DongleEvent::Pose { sample, .. } = event else {
        return Err(OmittedReason::Malformed);
    };
    if !valid_pose(sample) {
        return Err(OmittedReason::CheckFailed);
    }
    output[..SAFE_POSE_PREFIX_LEN].copy_from_slice(&payload[..SAFE_POSE_PREFIX_LEN]);
    output[TRACKING_STATUS_OFFSET] = payload[TRACKING_STATUS_OFFSET];
    Ok(())
}

fn valid_pose(sample: &PoseSample) -> bool {
    let norm_squared = |values: &[f32]| {
        values
            .iter()
            .map(|&value| f64::from(value).powi(2))
            .sum::<f64>()
    };
    let position = norm_squared(&sample.position_m);
    let rotation = norm_squared(&sample.rotation_wxyz);
    sample.slot <= 4
        && (-1..=4).contains(&sample.tracking_status)
        && position <= 100.0_f64.powi(2)
        && ((0.9_f64.powi(2)..=1.1_f64.powi(2)).contains(&rotation)
            || (position == 0.0 && rotation == 0.0))
        && sample
            .acceleration
            .iter()
            .chain(&sample.angular_velocity)
            .all(|value| value.is_finite())
}

fn rebuild_text(
    payload: &[u8],
    output: &mut [u8],
    event: &DongleEvent,
    replace: &mut dyn FnMut(Option<Kind>, &str) -> String,
) -> Result<(), OmittedReason> {
    if !matches!(event, DongleEvent::Text { .. }) || !printable(payload) {
        return Err(OmittedReason::TextForm);
    }
    let (slot, body) = separate_slot(payload);
    let text = std::str::from_utf8(body).map_err(|_| OmittedReason::TextForm)?;
    let anonymous = if let Some(value) = text.strip_prefix("WH") {
        let mut row = String::from("WH");
        for (index, value) in value.split(',').enumerate() {
            if index > 0 {
                row.push(',');
            }
            let kind = match index {
                0 => Some(Kind::WifiName),
                1 => Some(Kind::WifiPassword),
                _ => None,
            };
            let alias = same_length_alias(value, kind, replace)?;
            if alias.contains(',') {
                return Err(OmittedReason::CheckFailed);
            }
            row.push_str(&alias);
        }
        row
    } else if let Some((prefix, kind)) = private_prefix(body) {
        let (prefix, value) = text.split_at(prefix);
        let mut row = prefix.to_owned();
        row.push_str(&same_length_alias(value, kind, replace)?);
        row
    } else if valid_public_text(text) {
        text.to_owned()
    } else {
        return Err(OmittedReason::TextForm);
    };
    output[..slot.len()].copy_from_slice(slot);
    output[slot.len()..].copy_from_slice(anonymous.as_bytes());
    Ok(())
}

fn same_length_alias(
    value: &str,
    kind: Option<Kind>,
    replace: &mut dyn FnMut(Option<Kind>, &str) -> String,
) -> Result<String, OmittedReason> {
    if value.is_empty() {
        return Ok(String::new());
    }
    let alias = replace(kind, value);
    if alias.len() != value.len() || !printable(alias.as_bytes()) {
        return Err(OmittedReason::CheckFailed);
    }
    Ok(alias)
}

fn separate_slot(bytes: &[u8]) -> (&[u8], &[u8]) {
    bytes.split_at(usize::from(bytes.first().is_some_and(u8::is_ascii_digit)))
}

fn printable(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| (b' '..=b'~').contains(byte))
}

fn private_prefix(bytes: &[u8]) -> Option<(usize, Option<Kind>)> {
    [
        (b"NADS".as_slice(), Some(Kind::TrackerSerial)),
        (b"NASS", Some(Kind::TrackerSerial)),
        (b"NASI", None),
        (b"NAPI", None),
        (b"NADN", None),
        (b"NADI", None),
        (b"NANA", None),
        (b"Ws", Some(Kind::WifiName)),
        (b"Wp", Some(Kind::WifiPassword)),
        (b"Wt", None),
        (b"C", None),
        (b"c", None),
    ]
    .into_iter()
    .find_map(|(prefix, kind)| bytes.starts_with(prefix).then_some((prefix.len(), kind)))
}

fn valid_public_text(text: &str) -> bool {
    if matches!(text, "WS" | "APF" | "APR" | "NAZZ") {
        return true;
    }
    if let Some(value) = text.strip_prefix("NAV1") {
        return value.len() <= 32
            && value.bytes().any(|byte| byte.is_ascii_digit())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.');
    }
    for (prefix, count) in [
        ("NARI", 1..=1),
        ("NAGN", 3..=3),
        ("MS", 2..=2),
        ("LS", 1..=8),
        ("P61:", 2..=2),
        ("P82:0:", 1..=1),
        ("WC", 1..=1),
    ] {
        if let Some(value) = text.strip_prefix(prefix) {
            let mut fields = 0;
            for value in value.split(',') {
                if !signed_integer(value) {
                    return false;
                }
                fields += 1;
            }
            return count.contains(&fields);
        }
    }
    false
}

fn signed_integer(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    (1..=10).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACKER: Option<TrackerLabel> = Some(TrackerLabel('A'));
    const ADDRESS: [u8; 6] = [0x41, 0x52, 0x63, 0x74, 0x85, 0x96];
    const SYNTHETIC: [u8; 6] = [0, 0xa2, 0, 0, 0, 1];

    fn report(kind: u16, text_or_pose: &[u8], padding: &[u8]) -> Vec<u8> {
        let mut row = vec![TRACKER_DATA, 8, 9];
        row.extend(ADDRESS);
        row.extend(kind.to_le_bytes());
        row.push(u8::try_from(text_or_pose.len()).expect("small test payload"));
        row.extend(text_or_pose);
        row.extend(padding);
        row
    }

    fn synthetic_pose() -> [u8; 50] {
        let mut pose = [0; 50];
        pose[0] = 3;
        pose[1] = 17;
        pose[2..6].copy_from_slice(&10.0_f32.to_le_bytes());
        pose[14..16].copy_from_slice(&0x3c00_u16.to_le_bytes());
        pose[28..34].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        pose[34..36].copy_from_slice(b"AB");
        pose[36] = 2;
        pose[37..].copy_from_slice(b"FAKE_TAIL_XYZ");
        pose
    }

    fn masked(bytes: &[u8]) -> Result<Vec<u8>, OmittedReason> {
        let event = Decoder::default().decode(bytes);
        rebuild(bytes, TRACKER, &event, &mut |_, value| {
            "X".repeat(value.len())
        })
    }

    #[test]
    fn rebuilds_pose_from_allowed_ranges_at_each_supported_length() {
        let pose = synthetic_pose();
        for length in [37, 39, 50] {
            let mut source = vec![0];
            source.extend(report(TRACKER_POSE, &pose[..length], b"FAKE_PADDING"));
            let rebuilt = masked(&source).expect("allowed pose");
            let report = &rebuilt[1..];
            assert_eq!(rebuilt.len(), source.len());
            assert_eq!(rebuilt[0], 0);
            assert_eq!(&report[..3], &source[1..4]);
            assert_eq!(&report[3..9], &SYNTHETIC);
            assert_eq!(&report[9..12], &source[10..13]);
            let payload = &report[12..];
            assert_eq!(&payload[..34], &pose[..34]);
            assert_eq!(payload[36], pose[36]);
            assert!(
                payload[34..36]
                    .iter()
                    .chain(&payload[37..])
                    .all(|&b| b == 0)
            );
        }
    }

    #[test]
    fn rejects_pose_when_shape_or_values_are_outside_the_allowlist() {
        let source = synthetic_pose();
        assert_eq!(
            masked(&report(TRACKER_POSE, &source[..38], &[])),
            Err(OmittedReason::CheckFailed)
        );
        for (offset, bytes) in [
            (0, &[5_u8][..]),
            (2, &f32::INFINITY.to_le_bytes()[..]),
            (2, &101.0_f32.to_le_bytes()[..]),
            (14, &[0, 0][..]),
            (34, &[0, 0x7c][..]),
            (36, &[5][..]),
        ] {
            let mut pose = source;
            pose[offset..offset + bytes.len()].copy_from_slice(bytes);
            assert_eq!(
                masked(&report(TRACKER_POSE, &pose, &[])),
                Err(OmittedReason::CheckFailed)
            );
        }
        assert!(masked(&report(TRACKER_POSE, &[0; 37], &[])).is_ok());
    }

    #[test]
    fn pair_leaves_only_type_length_flag_and_synthetic_address() {
        let mut source = vec![PAIRING, 17, 0xa1, 0xb2, 0xc3, 1];
        source.extend(ADDRESS);
        source.extend(b"TAIL!");
        let rebuilt = masked(&source).expect("allowed pair");
        assert_eq!(&rebuilt[..6], &[PAIRING, 17, 0, 0, 0, 1]);
        assert_eq!(&rebuilt[6..12], &SYNTHETIC);
        assert_eq!(&rebuilt[12..], &[0; 5]);

        // A source using the wrong address location still copies only the allowed fields.
        source[4..10].copy_from_slice(&[0xcc, 0, 0x33, 0xee, 0x55, 0x66]);
        let rebuilt = masked(&source).expect("pair with unknown fields");
        assert_eq!(&rebuilt[2..5], &[0; 3]);
        assert!(rebuilt[12..].iter().all(|&byte| byte == 0));
        assert!(!rebuilt.windows(6).any(|window| window == &source[4..10]));
    }

    #[test]
    fn accepts_only_the_documented_public_text_grammar() {
        for text in [
            "3NAV11.2.3",
            "NARI-2",
            "NAGN-22,+33,44",
            "MS1,2",
            "LS1,2,3,4,5,6,7,8",
            "P61:6,4",
            "P82:0:1121",
            "WC-1",
            "WS",
            "APF",
            "APR",
            "NAZZ",
        ] {
            let row = masked(&report(TRACKER_TEXT, text.as_bytes(), b"FAKE_PADDING"))
                .expect("allowed text");
            assert_eq!(&row[12..12 + text.len()], text.as_bytes());
            assert_eq!(&row[12 + text.len()..], &[0; 12]);
        }
        for text in [
            "NAV11.AAA",
            "NAGN1,2",
            "NARI12345678901",
            "LS1,2,3,4,5,6,7,8,9",
            "LPFAKE_SECRET",
            "DECFAKE_SECRET",
            "ZZFAKE_SECRET",
            "WS_EXTRA",
            "NAV11.\u{0080}",
        ] {
            assert_eq!(
                masked(&report(TRACKER_TEXT, text.as_bytes(), &[])),
                Err(OmittedReason::TextForm)
            );
        }
    }

    #[test]
    fn keeps_private_message_kind_and_length_and_omits_unknown_reports() {
        for (text, masked_text) in [
            ("2NADSFAKE_SERIAL", "2NADSXXXXXXXXXXX"),
            ("NASSFAKE_SERIAL", "NASSXXXXXXXXXXX"),
            ("WHFAKE_WIFI,FAKE_PASS,123", "WHXXXXXXXXX,XXXXXXXXX,XXX"),
            ("WsFAKE_WIFI", "WsXXXXXXXXX"),
            ("CFakeCalibration", "CXXXXXXXXXXXXXXX"),
        ] {
            let source = report(TRACKER_TEXT, text.as_bytes(), b"PRIVATE");
            let rebuilt = masked(&source).expect("recognized private message");
            assert_eq!(rebuilt.len(), source.len());
            assert_eq!(&rebuilt[12..12 + text.len()], masked_text.as_bytes());
            assert_eq!(&rebuilt[12 + text.len()..], &[0; 7]);
            assert!(matches!(
                Decoder::default().decode(&rebuilt),
                DongleEvent::Text { .. }
            ));
        }
        let host = report(TRACKER_TEXT, b"WHFAKE_WIFI,FAKE_PASS,123", &[]);
        assert_eq!(private_values(&host), [b"FAKE_WIFI", b"FAKE_PASS"]);

        for kind in [0x1e, 0x29, 0xee] {
            let mut source = vec![kind, 29, 0, 0];
            source.resize(29, 0xcc);
            assert_eq!(masked(&source), Err(OmittedReason::NotAllowed));
        }
        assert_eq!(
            masked(&report(0x9999, b"FAKE_BODY", &[])),
            Err(OmittedReason::NotAllowed)
        );
    }
}
