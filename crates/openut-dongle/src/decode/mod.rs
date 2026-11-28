//! Decode received dongle reports without performing device I/O.

use std::collections::BTreeMap;

use crate::record::TrackerLabel;

mod addr;
mod pose;
mod rf_status;
mod text;

pub use pose::PoseSample;
pub use rf_status::SlotStatus;
pub use text::{DeviceInfoField, Redactable, TrackerMessage};

const PAIRING: u8 = 0x18;
const SLOT_COMMAND_REPLY: u8 = 0x1d;
const RF_STATUS: u8 = 0x1e;
const TRACKER_DATA: u8 = 0x28;
const UNCLASSIFIED_REPORT: u8 = 0x29;
const TRACKER_TEXT: u16 = 0x0101;
const TRACKER_POSE: u16 = 0x0110;
const LABELS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

#[derive(Clone, Debug, PartialEq)]
pub enum DongleEvent {
    Pair {
        tracker: TrackerLabel,
        unpaired: bool,
    },
    Text {
        tracker: TrackerLabel,
        message: TrackerMessage,
    },
    Pose {
        tracker: TrackerLabel,
        sample: PoseSample,
    },
    RfStatus {
        slots: [SlotStatus; 5],
    },
    OtherTrackerData {
        tracker: TrackerLabel,
        kind: u16,
        len: usize,
    },
    Unknown {
        report_type: u8,
        len: usize,
    },
    Malformed {
        report_type: u8,
        reason: &'static str,
    },
}

/// Names trackers in first-seen order for one input stream.
#[derive(Default)]
pub struct Decoder {
    labels: BTreeMap<addr::TrackerId, TrackerLabel>,
    anonymized: bool,
}

impl Decoder {
    pub(crate) fn for_recording(anonymized: bool) -> Self {
        Self {
            anonymized,
            ..Self::default()
        }
    }

    pub fn decode(&mut self, bytes: &[u8]) -> DongleEvent {
        let report = normalize(bytes);
        let Some(&report_type) = report.first() else {
            return DongleEvent::Malformed {
                report_type: 0,
                reason: "missing report type",
            };
        };
        let event = match report_type {
            PAIRING => self.pairing(report),
            TRACKER_DATA => self.tracker_data(report),
            RF_STATUS => rf_status::decode(report).map(|slots| DongleEvent::RfStatus { slots }),
            _ => Ok(DongleEvent::Unknown {
                report_type,
                len: report.len(),
            }),
        };
        event.unwrap_or_else(|reason| DongleEvent::Malformed {
            report_type,
            reason,
        })
    }

    /// The slot encoded in the address is an estimate. It is not tracker identity.
    pub fn slot_hint(&self, tracker: TrackerLabel) -> Option<u8> {
        self.labels
            .iter()
            .find_map(|(id, &label)| (label == tracker).then_some(id.slot_hint()))
    }

    fn pairing(&mut self, report: &[u8]) -> Result<DongleEvent, &'static str> {
        let report = declared_report(report)?;
        if !(12..=64).contains(&report.len()) {
            return Err("pairing report length outside 12..=64");
        }
        let unpaired = match report.get(5) {
            Some(0) => false,
            Some(1) => true,
            _ => return Err("invalid unpaired flag"),
        };
        let tracker = self.tracker_label(report)?;
        Ok(DongleEvent::Pair { tracker, unpaired })
    }

    fn tracker_data(&mut self, report: &[u8]) -> Result<DongleEvent, &'static str> {
        let Some((&len, header)) = report.get(..12).and_then(|header| header.split_last()) else {
            return Err("short tracker report header");
        };
        let kind = u16::from_le_bytes([header[9], header[10]]);
        let len = usize::from(len);
        let payload = report.get(12..12 + len).ok_or("short tracker payload")?;
        match kind {
            TRACKER_TEXT => {
                let message = text::decode(payload)?;
                let tracker = self.tracker_label(report)?;
                Ok(DongleEvent::Text { tracker, message })
            }
            TRACKER_POSE => {
                let sample = pose::decode(payload)?;
                let tracker = self.tracker_label(report)?;
                Ok(DongleEvent::Pose { tracker, sample })
            }
            _ => Ok(DongleEvent::OtherTrackerData {
                tracker: self.tracker_label(report)?,
                kind,
                len,
            }),
        }
    }

    fn tracker_label(&mut self, report: &[u8]) -> Result<TrackerLabel, &'static str> {
        let address = addr::report_address(report).ok_or("short tracker address")?;
        let id = addr::TrackerId::from_address(address);
        let exported = self
            .anonymized
            .then(|| addr::anonymized_label(address))
            .flatten();
        if let Some(&label) = self.labels.get(&id) {
            if exported.is_some_and(|exported| exported != label) {
                return Err("conflicting tracker label in export");
            }
            return Ok(label);
        }
        let is_available =
            |candidate: &TrackerLabel| !self.labels.values().any(|label| candidate == label);
        let label = if let Some(label) = exported {
            if !is_available(&label) {
                return Err("conflicting tracker label in export");
            }
            label
        } else {
            LABELS
                .iter()
                .map(|&letter| TrackerLabel(char::from(letter)))
                .find(is_available)
                .ok_or("tracker label capacity exhausted")?
        };
        self.labels.insert(id, label);
        Ok(label)
    }
}

fn normalize(bytes: &[u8]) -> &[u8] {
    if matches!(
        bytes,
        [
            0,
            PAIRING | SLOT_COMMAND_REPLY | RF_STATUS | TRACKER_DATA | UNCLASSIFIED_REPORT,
            ..
        ]
    ) {
        &bytes[1..]
    } else {
        bytes
    }
}

fn declared_report(report: &[u8]) -> Result<&[u8], &'static str> {
    let len = report.get(1).copied().ok_or("missing report length")?;
    report.get(..usize::from(len)).ok_or("short report")
}
