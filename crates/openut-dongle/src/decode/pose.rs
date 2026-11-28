use std::fmt;

use crate::half;

const POSE_SIZE: usize = 37;

#[derive(Clone, PartialEq)]
pub struct PoseSample {
    pub slot: u8,
    pub buttons_raw: u8,
    pub position_m: [f32; 3],
    pub rotation_wxyz: [f32; 4],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 4],
    pub tracking_status: i8,
    /// Undocumented bytes following the documented pose fields.
    pub extra: Vec<u8>,
}

impl fmt::Debug for PoseSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoseSample")
            .field("slot", &self.slot)
            .field("buttons_raw", &self.buttons_raw)
            .field("position_m", &self.position_m)
            .field("rotation_wxyz", &self.rotation_wxyz)
            .field("acceleration", &self.acceleration)
            .field("angular_velocity", &self.angular_velocity)
            .field("tracking_status", &self.tracking_status)
            .field("extra_len", &self.extra.len())
            .finish()
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<PoseSample, &'static str> {
    let (pose, extra) = bytes
        .split_first_chunk::<POSE_SIZE>()
        .ok_or("short pose payload")?;

    let [w, z, y, x] = read_halves(pose, 14);
    Ok(PoseSample {
        slot: pose[0],
        buttons_raw: pose[1],
        position_m: std::array::from_fn(|i| {
            let at = 2 + i * 4;
            f32::from_le_bytes([pose[at], pose[at + 1], pose[at + 2], pose[at + 3]])
        }),
        rotation_wxyz: [w, x, y, z],
        acceleration: read_halves(pose, 22),
        angular_velocity: read_halves(pose, 28),
        tracking_status: i8::from_le_bytes([pose[36]]),
        extra: extra.to_vec(),
    })
}

fn read_halves<const N: usize>(pose: &[u8; POSE_SIZE], start: usize) -> [f32; N] {
    std::array::from_fn(|i| {
        let at = start + i * 2;
        half::to_f32(u16::from_le_bytes([pose[at], pose[at + 1]]))
    })
}
