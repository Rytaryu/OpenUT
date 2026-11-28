//! SteamVR space evidence and the calibration change thresholds.

use std::fmt;

use crate::{Pose, Vec3};

const POSITION_THRESHOLD_M: f64 = 0.05;
const ROTATION_THRESHOLD_RAD: f64 = 2.0_f64.to_radians();
// Compensate only for floating-point arithmetic at an inclusive boundary.
const COMPARISON_ROUNDOFF: f64 = 8.0 * f64::EPSILON;

#[derive(Clone, Copy, PartialEq)]
pub struct SpaceFingerprint {
    /// FNV-1a of the eight little-endian bytes of the reported universe ID.
    pub universe_hash: u64,
    pub standing_origin: Pose,
    /// Play-area width and depth. SteamVR can leave these unavailable.
    pub play_area_m: Option<[f64; 2]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpaceChange {
    Same,
    Changed,
}

impl SpaceFingerprint {
    /// Hashes the reported ID immediately and validates the standing origin.
    pub fn new(
        universe_id: u64,
        mut standing_origin: Pose,
        play_area_m: Option<[f64; 2]>,
    ) -> Option<Self> {
        let Vec3 { x, y, z } = standing_origin.position;
        if ![x, y, z].into_iter().all(f64::is_finite) {
            return None;
        }
        standing_origin.rotation = standing_origin.rotation.normalized()?;
        let play_area_m =
            play_area_m.filter(|area| area.iter().all(|size| size.is_finite() && *size > 0.0));
        Some(Self {
            universe_hash: fnv1a_64(&universe_id.to_le_bytes()),
            standing_origin,
            play_area_m,
        })
    }

    /// Compares the universe and standing origin. The play area is retained as evidence.
    pub fn compare(&self, current: &Self) -> SpaceChange {
        if self.universe_hash != current.universe_hash {
            return SpaceChange::Changed;
        }

        let distance = (current.standing_origin.position - self.standing_origin.position).norm();
        if !distance.is_finite() || distance + COMPARISON_ROUNDOFF >= POSITION_THRESHOLD_M {
            return SpaceChange::Changed;
        }

        let (Some(saved), Some(current)) = (
            self.standing_origin.rotation.normalized(),
            current.standing_origin.rotation.normalized(),
        ) else {
            return SpaceChange::Changed;
        };
        let delta = current * saved.conjugate();
        let angle = 2.0
            * Vec3::new(delta.x, delta.y, delta.z)
                .norm()
                .atan2(delta.w.abs());
        if angle + COMPARISON_ROUNDOFF >= ROTATION_THRESHOLD_RAD {
            SpaceChange::Changed
        } else {
            SpaceChange::Same
        }
    }
}

impl fmt::Debug for SpaceFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpaceFingerprint")
            .field("standing_origin", &self.standing_origin)
            .field("play_area_m", &self.play_area_m)
            .finish_non_exhaustive()
    }
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use crate::Quat;

    use super::*;
    use SpaceChange::{Changed, Same};

    fn fingerprint(id: u64, position: Vec3, rotation: Quat) -> SpaceFingerprint {
        SpaceFingerprint::new(id, Pose { position, rotation }, Some([2.5, 3.0])).unwrap()
    }

    #[test]
    fn published_fnv1a_64_vectors() {
        // RFC 9923, section 8.3: the 64-bit string and block test cases.
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
        assert_eq!(fnv1a_64(b"foobar\0"), 0x3453_1ca7_168b_8f38);
    }

    #[test]
    fn universe_hash_uses_fixed_byte_order_and_is_omitted_from_debug_output() {
        let raw = 0x0123_4567_89ab_cdef;
        let evidence = fingerprint(raw, Vec3::ZERO, Quat::IDENTITY);
        assert_eq!(
            evidence.universe_hash,
            fnv1a_64(&[0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01])
        );
        assert_ne!(evidence.universe_hash, raw);
        assert!(!format!("{evidence:?}").contains(&evidence.universe_hash.to_string()));
        assert_eq!(evidence.compare(&evidence), Same);
        assert_eq!(
            evidence.compare(&fingerprint(raw + 1, Vec3::ZERO, Quat::IDENTITY)),
            Changed
        );
    }

    #[test]
    fn standing_origin_translation_or_rotation_at_the_threshold_counts_as_changed() {
        let baseline = Quat::yaw(0.6);
        let saved = fingerprint(1, Vec3::ZERO, baseline);
        for (distance, degrees, expected) in [
            (0.049, 1.9_f64, Same),
            (0.05 - 1e-12, 2.0 - 1e-10, Same),
            (0.05, 0.0, Changed),
            (0.051, 0.0, Changed),
            (0.0, 2.0, Changed),
            (0.0, 2.1, Changed),
        ] {
            let (sin, cos) = (degrees.to_radians() * 0.5).sin_cos();
            let rotation = Quat::new(cos, 0.0, 0.0, sin) * baseline;
            let current = fingerprint(1, Vec3::new(distance, 0.0, 0.0), rotation);
            assert_eq!(
                saved.compare(&current),
                expected,
                "{distance} m, {degrees}°"
            );
            assert_eq!(
                current.compare(&saved),
                expected,
                "{distance} m, {degrees}°"
            );
        }
        let diagonal = fingerprint(1, Vec3::new(0.03, 0.04, 0.0), baseline);
        assert_eq!(saved.compare(&diagonal), Changed);
        let shifted = fingerprint(1, Vec3::new(0.25, 0.0, 0.0), baseline);
        let moved = fingerprint(1, Vec3::new(0.30, 0.0, 0.0), baseline);
        assert_eq!(shifted.compare(&moved), Changed);
    }

    #[test]
    fn equivalent_quaternions_and_play_area_changes_do_not_change_the_origin() {
        let saved = fingerprint(1, Vec3::ZERO, Quat::IDENTITY);
        let mut current = fingerprint(1, Vec3::ZERO, Quat::new(-2.0, 0.0, 0.0, 0.0));
        current.play_area_m = Some([4.0, 3.0]);
        assert_eq!(saved.play_area_m, Some([2.5, 3.0]));
        assert_eq!(saved.compare(&current), Same);
        current.play_area_m = None;
        assert_eq!(saved.compare(&current), Same);
    }

    #[test]
    fn unusable_origins_are_rejected_and_unavailable_play_areas_are_allowed() {
        let pose = Pose {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        assert!(
            SpaceFingerprint::new(
                1,
                Pose {
                    position: Vec3::new(f64::NAN, 0.0, 0.0),
                    ..pose
                },
                None
            )
            .is_none()
        );
        assert!(
            SpaceFingerprint::new(
                1,
                Pose {
                    rotation: Quat::new(0.0, 0.0, 0.0, 0.0),
                    ..pose
                },
                None
            )
            .is_none()
        );
        assert_eq!(
            SpaceFingerprint::new(1, pose, None).unwrap().play_area_m,
            None
        );
        assert_eq!(
            SpaceFingerprint::new(1, pose, Some([2.0, f64::NAN]))
                .unwrap()
                .play_area_m,
            None
        );
    }
}
