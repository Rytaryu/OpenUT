use std::ops::Range;

use crate::record::TrackerLabel;

use super::{PAIRING, TRACKER_DATA};

/// The wire address positions are relative to the normalized report type byte.
const ADDRESSES: [(u8, Range<usize>); 2] = [(TRACKER_DATA, 3..9), (PAIRING, 6..12)];

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct TrackerId([u8; 5]);

pub(super) fn report_address(report: &[u8]) -> Option<&[u8; 6]> {
    let report_type = *report.first()?;
    let (_, positions) = ADDRESSES.iter().find(|(kind, _)| *kind == report_type)?;
    report.get(positions.clone())?.try_into().ok()
}

/// The export encodes its assigned label in the final byte of a synthetic address.
pub(super) fn anonymized_label(address: &[u8; 6]) -> Option<TrackerLabel> {
    let &[0, 0xa0..=0xaf, 0, 0, 0, index @ 1..=26] = address else {
        return None;
    };
    Some(TrackerLabel(char::from(b'A' + index - 1)))
}

impl TrackerId {
    pub(super) fn from_address(address: &[u8; 6]) -> Self {
        let &[_, identifier @ ..] = address;
        Self(identifier)
    }

    pub(super) fn slot_hint(self) -> u8 {
        self.0[0] & 0x0f
    }
}
