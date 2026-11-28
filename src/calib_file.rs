//! Versioned calibration records stored separately from user settings.

use std::f64::consts::PI;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use openut_space::fingerprint::SpaceFingerprint;
use openut_space::{Calibration, Pose, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::calib_flow::CalibrationRecord;

const VERSION: u32 = 1;
const QUATERNION_TOLERANCE: f64 = 1e-6;
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize, Serialize)]
struct SavedCalibration {
    version: u32,
    axis_map: String,
    rotation_wxyz: [f64; 4],
    translation_m: [f64; 3],
    scale: f64,
    position_rms_m: f64,
    sample_count: usize,
    offset_ms: i32,
    created_at_unix_s: u64,
    fingerprint: SavedFingerprint,
}

#[derive(Deserialize, Serialize)]
struct SavedFingerprint {
    universe_hash: String,
    standing_origin: SavedPose,
    #[serde(deserialize_with = "deserialize_play_area")]
    play_area_m: Option<[f64; 2]>,
}

#[derive(Deserialize, Serialize)]
struct SavedPose {
    position_m: [f64; 3],
    rotation_wxyz: [f64; 4],
}

// A missing play-area measurement is explicit in the file as null.
fn deserialize_play_area<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<[f64; 2]>, D::Error> {
    Option::deserialize(deserializer)
}

/// Returns no calibration for missing files or records this version cannot apply.
pub fn read(path: &Path) -> io::Result<Option<CalibrationRecord>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(serde_json::from_slice::<SavedCalibration>(&bytes)
        .ok()
        .and_then(SavedCalibration::into_record))
}

/// Replaces the previous record only after the new file has been written and synced.
pub fn write(path: &Path, record: &CalibrationRecord) -> io::Result<()> {
    if !valid_record(record) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid calibration record",
        ));
    }
    let saved = SavedCalibration::from(record);
    let json = serde_json::to_vec_pretty(&saved).map_err(io::Error::other)?;
    let (temporary, mut file) = create_temporary_file(path)?;
    let written = file
        .write_all(&json)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all());
    // Closing before replacement also releases the temporary file on Windows.
    drop(file);
    let replaced = written.and_then(|()| fs::rename(&temporary, path));
    if replaced.is_err() {
        let _ = fs::remove_file(temporary);
    }
    replaced
}

fn create_temporary_file(destination: &Path) -> io::Result<(PathBuf, File)> {
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid calibration path"))?;
    let directory = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory)?;
    loop {
        let index = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(".tmp-{}-{index}", std::process::id()));
        let path = directory.join(temporary_name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

impl From<&CalibrationRecord> for SavedCalibration {
    fn from(record: &CalibrationRecord) -> Self {
        let calibration = record.calibration;
        let fingerprint = record.fingerprint;
        Self {
            version: VERSION,
            axis_map: record.axis_map.to_string(),
            rotation_wxyz: quaternion_components(Quat::yaw(calibration.yaw_rad)),
            translation_m: vector_components(calibration.translation),
            scale: calibration.scale,
            position_rms_m: record.position_rms_m,
            sample_count: record.sample_count,
            offset_ms: record.offset_ms,
            created_at_unix_s: record.created_at_unix_s,
            fingerprint: SavedFingerprint {
                universe_hash: format!("{:016x}", fingerprint.universe_hash),
                standing_origin: SavedPose {
                    position_m: vector_components(fingerprint.standing_origin.position),
                    rotation_wxyz: quaternion_components(fingerprint.standing_origin.rotation),
                },
                play_area_m: fingerprint.play_area_m,
            },
        }
    }
}

impl SavedCalibration {
    fn into_record(self) -> Option<CalibrationRecord> {
        if self.version != VERSION {
            return None;
        }
        let [x, y, z] = self.translation_m;
        let record = CalibrationRecord {
            axis_map: self.axis_map.parse().ok()?,
            calibration: Calibration::new(
                yaw_from_quaternion(self.rotation_wxyz)?,
                Vec3::new(x, y, z),
                self.scale,
            ),
            position_rms_m: self.position_rms_m,
            sample_count: self.sample_count,
            offset_ms: self.offset_ms,
            created_at_unix_s: self.created_at_unix_s,
            fingerprint: self.fingerprint.into_fingerprint()?,
        };
        valid_record(&record).then_some(record)
    }
}

impl SavedFingerprint {
    fn into_fingerprint(self) -> Option<SpaceFingerprint> {
        if self.universe_hash.len() != 16
            || !self
                .universe_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        let [x, y, z] = self.standing_origin.position_m;
        let [w, rx, ry, rz] = self.standing_origin.rotation_wxyz;
        Some(SpaceFingerprint {
            universe_hash: u64::from_str_radix(&self.universe_hash, 16).ok()?,
            standing_origin: Pose {
                position: Vec3::new(x, y, z),
                rotation: Quat::new(w, rx, ry, rz),
            },
            play_area_m: self.play_area_m,
        })
    }
}

fn valid_record(record: &CalibrationRecord) -> bool {
    let calibration = record.calibration;
    let fingerprint = record.fingerprint;
    let origin = fingerprint.standing_origin;
    record.axis_map == record.axis_map.group().representative()
        && calibration.yaw_rad > -PI
        && calibration.yaw_rad <= PI
        && positive_finite(calibration.scale)
        && vector_components(calibration.translation)
            .into_iter()
            .all(f64::is_finite)
        && record.position_rms_m.is_finite()
        && record.position_rms_m >= 0.0
        && record.sample_count > 0
        && vector_components(origin.position)
            .into_iter()
            .all(f64::is_finite)
        && is_unit_quaternion(quaternion_components(origin.rotation))
        && fingerprint
            .play_area_m
            .is_none_or(|area| area.into_iter().all(positive_finite))
}

fn positive_finite(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn vector_components(vector: Vec3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

fn quaternion_components(rotation: Quat) -> [f64; 4] {
    [rotation.w, rotation.x, rotation.y, rotation.z]
}

fn is_unit_quaternion(rotation: [f64; 4]) -> bool {
    let squared_norm: f64 = rotation.into_iter().map(|value| value * value).sum();
    (squared_norm - 1.0).abs() <= QUATERNION_TOLERANCE
}

fn yaw_from_quaternion(rotation: [f64; 4]) -> Option<f64> {
    let [mut w, x, mut y, z] = rotation;
    if !is_unit_quaternion(rotation)
        || x.abs() > QUATERNION_TOLERANCE
        || z.abs() > QUATERNION_TOLERANCE
    {
        return None;
    }
    if w.is_sign_negative() {
        w = -w;
        y = -y;
    }
    let yaw = 2.0 * y.atan2(w);
    Some(if yaw <= -PI { PI } else { yaw })
}

#[cfg(test)]
mod tests {
    use openut_space::AxisMap;
    use serde_json::{Value, json};

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            loop {
                let index = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
                let name = format!("openut-calib-test-{}-{index}", std::process::id());
                let path = std::env::temp_dir().join(name);
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create calibration test directory: {error}"),
                }
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn example_record() -> CalibrationRecord {
        CalibrationRecord {
            axis_map: "+x+z+y"
                .parse::<AxisMap>()
                .unwrap()
                .group()
                .representative(),
            calibration: Calibration::new(PI / 3.0, Vec3::new(1.0, -0.25, 0.5), 1.03125),
            position_rms_m: 0.00390625,
            sample_count: 256,
            offset_ms: -35,
            created_at_unix_s: 1_000,
            fingerprint: SpaceFingerprint::new(
                91,
                Pose {
                    position: Vec3::new(0.5, 0.0, -2.0),
                    rotation: Quat::IDENTITY,
                },
                Some([2.5, 3.0]),
            )
            .unwrap(),
        }
    }

    #[test]
    fn round_trips_and_replaces_a_calibration_record() {
        let directory = TestDirectory::new();
        let path = directory.0.join("OpenUT/calibration.json");
        let mut previous = example_record();
        write(&path, &previous).expect("save calibration");
        let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["version"], 1);
        assert_eq!(stored["rotation_wxyz"].as_array().unwrap().len(), 4);
        assert_eq!(
            stored["fingerprint"]["universe_hash"],
            format!("{:016x}", previous.fingerprint.universe_hash)
        );
        assert!(stored["fingerprint"].get("universe_id").is_none());

        let restored = read(&path).unwrap().expect("load saved calibration");
        assert_eq!(restored.axis_map, previous.axis_map);
        assert!((restored.calibration.yaw_rad - previous.calibration.yaw_rad).abs() < 1e-12);
        assert_eq!(
            restored.calibration.translation,
            previous.calibration.translation
        );
        assert_eq!(restored.calibration.scale, previous.calibration.scale);
        assert_eq!(restored.position_rms_m, previous.position_rms_m);
        assert_eq!(restored.sample_count, previous.sample_count);
        assert_eq!(restored.offset_ms, previous.offset_ms);
        assert_eq!(restored.created_at_unix_s, previous.created_at_unix_s);
        assert_eq!(restored.fingerprint, previous.fingerprint);

        previous.calibration.yaw_rad = PI;
        previous.calibration.scale = 1.0;
        previous.fingerprint.play_area_m = None;
        write(&path, &previous).expect("replace calibration");
        let replacement = read(&path).unwrap().expect("load replacement");
        assert_eq!(replacement.calibration.yaw_rad, PI);
        assert_eq!(replacement.calibration.scale, 1.0);
        assert_eq!(replacement.fingerprint.play_area_m, None);
    }

    #[test]
    fn missing_corrupt_or_invalid_records_are_ignored() {
        let directory = TestDirectory::new();
        let path = directory.0.join("calibration.json");
        assert!(read(&path).unwrap().is_none());
        fs::write(&path, "{ truncated").unwrap();
        assert!(read(&path).unwrap().is_none());
        assert!(read(&directory.0).is_err());

        let valid = serde_json::to_value(SavedCalibration::from(&example_record())).unwrap();
        for (pointer, invalid) in [
            ("/version", json!(999)),
            ("/axis_map", json!("+x+x+z")),
            ("/scale", json!(0)),
            ("/position_rms_m", json!(-0.001)),
            ("/sample_count", json!(0)),
            ("/rotation_wxyz", json!([0, 1, 0, 0])),
            ("/fingerprint/universe_hash", json!("xyz")),
            (
                "/fingerprint/standing_origin/rotation_wxyz",
                json!([0, 0, 0, 0]),
            ),
            ("/fingerprint/play_area_m", json!([1, 0])),
        ] {
            let mut invalid_file = valid.clone();
            *invalid_file.pointer_mut(pointer).unwrap() = invalid;
            fs::write(&path, serde_json::to_vec(&invalid_file).unwrap()).unwrap();
            assert!(read(&path).unwrap().is_none(), "accepted invalid {pointer}");
        }
        let mut incomplete = valid.clone();
        incomplete.as_object_mut().unwrap().remove("offset_ms");
        fs::write(&path, serde_json::to_vec(&incomplete).unwrap()).unwrap();
        assert!(read(&path).unwrap().is_none());

        let mut incomplete = valid;
        incomplete["fingerprint"]
            .as_object_mut()
            .unwrap()
            .remove("play_area_m");
        fs::write(&path, serde_json::to_vec(&incomplete).unwrap()).unwrap();
        assert!(read(&path).unwrap().is_none());
    }

    #[test]
    fn save_failure_preserves_existing_contents_and_cleans_up_temporary_files() {
        let directory = TestDirectory::new();
        let path = directory.0.join("calibration.json");
        let mut record = example_record();
        write(&path, &record).unwrap();
        let original = fs::read(&path).unwrap();
        record.calibration.scale = f64::NAN;
        assert_eq!(
            write(&path, &record).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(fs::read(&path).unwrap(), original);

        let unavailable = directory.0.join("existing-directory");
        fs::create_dir(&unavailable).unwrap();
        let sentinel = unavailable.join("keep");
        fs::write(&sentinel, b"keep").unwrap();
        assert!(write(&unavailable, &example_record()).is_err());
        assert_eq!(fs::read(sentinel).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }
}
