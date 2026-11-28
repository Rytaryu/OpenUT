use std::ops::Range;

use super::{PAIRING, TRACKER_DATA};

/// The wire address positions are relative to the normalized report type byte.
const ADDRESSES: [(u8, Range<usize>); 2] = [(TRACKER_DATA, 3..9), (PAIRING, 6..12)];

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct TrackerId([u8; 5]);

impl TrackerId {
    pub(super) fn from_report(report: &[u8]) -> Option<Self> {
        let report_type = *report.first()?;
        let (_, positions) = ADDRESSES.iter().find(|(kind, _)| *kind == report_type)?;
        let address = report.get(positions.clone())?;
        let (_, identifier) = address.split_first()?;
        Some(Self(identifier.try_into().ok()?))
    }

    pub(super) fn slot_hint(self) -> u8 {
        self.0[0] & 0x0f
    }
}
