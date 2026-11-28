use super::declared_report;

const SLOTS_START: usize = 9;
const SLOT_BYTES: usize = 4;
const SLOT_COUNT: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotStatus {
    pub status: u8,
    pub timeout_ms: u16,
    pub flags: u8,
}

pub(super) fn decode(report: &[u8]) -> Result<[SlotStatus; SLOT_COUNT], &'static str> {
    let end = SLOTS_START + SLOT_COUNT * SLOT_BYTES;
    let bytes = declared_report(report)?
        .get(SLOTS_START..end)
        .ok_or("short RF slot status")?;
    let (slots, _) = bytes.as_chunks::<SLOT_BYTES>();
    Ok(std::array::from_fn(|i| {
        let [status, low, high, flags] = slots[i];
        SlotStatus {
            status,
            timeout_ms: u16::from_le_bytes([low, high]),
            flags,
        }
    }))
}
