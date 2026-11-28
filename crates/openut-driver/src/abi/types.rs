//! OpenVR 2.15.6 driver ABI on Windows x64.
//!
//! Declarations adapted from Valve's `headers/openvr_driver.h` at tag
//! v2.15.6. See the crate's LICENSE-openvr for the original terms.

use std::ffi::{CStr, c_char, c_void};

use crate::pose::{Pose, TrackingResult};

pub(super) const INIT_OK: i32 = 0;
pub(super) const INIT_INTERFACE_NOT_FOUND: i32 = 105;
pub(super) const INIT_DRIVER_FAILED: i32 = 200;

pub(super) const PROVIDER_VERSION: &CStr = c"IServerTrackedDeviceProvider_004";
pub(super) const SERVER_HOST_VERSION: &CStr = c"IVRServerDriverHost_006";
pub(super) const PROPERTIES_VERSION: &CStr = c"IVRProperties_001";
pub(super) const SETTINGS_VERSION: &CStr = c"IVRSettings_003";
pub(super) const LOG_VERSION: &CStr = c"IVRDriverLog_001";

#[repr(transparent)]
struct StaticVersions([*const c_char; 13]);

// SAFETY: Every non-null pointer refers to an immutable static C string.
unsafe impl Sync for StaticVersions {}

static VERSIONS: StaticVersions = StaticVersions([
    SETTINGS_VERSION.as_ptr(),
    c"ITrackedDeviceServerDriver_005".as_ptr(),
    c"IVRDisplayComponent_003".as_ptr(),
    c"IVRDriverDirectModeComponent_009".as_ptr(),
    c"IVRCameraComponent_003".as_ptr(),
    PROVIDER_VERSION.as_ptr(),
    c"IVRWatchdogProvider_001".as_ptr(),
    c"IVRVirtualDisplay_002".as_ptr(),
    c"IVRDriverManager_001".as_ptr(),
    c"IVRResources_001".as_ptr(),
    c"IVRCompositorPluginProvider_001".as_ptr(),
    c"IVRIPCResourceManagerClient_003".as_ptr(),
    std::ptr::null(),
]);

pub(super) fn interface_versions() -> *const *const c_char {
    VERSIONS.0.as_ptr()
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct Quaternion {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl From<[f64; 4]> for Quaternion {
    fn from([w, x, y, z]: [f64; 4]) -> Self {
        Self { w, x, y, z }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct DriverPose {
    pub pose_time_offset: f64,
    pub world_from_driver_rotation: Quaternion,
    pub world_from_driver_translation: [f64; 3],
    pub driver_from_head_rotation: Quaternion,
    pub driver_from_head_translation: [f64; 3],
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub acceleration: [f64; 3],
    pub rotation: Quaternion,
    pub angular_velocity: [f64; 3],
    pub angular_acceleration: [f64; 3],
    pub result: i32,
    pub pose_is_valid: bool,
    pub will_drift_in_yaw: bool,
    pub should_apply_head_model: bool,
    pub device_is_connected: bool,
}

impl From<&Pose> for DriverPose {
    fn from(pose: &Pose) -> Self {
        Self {
            pose_time_offset: pose.time_offset,
            world_from_driver_rotation: Quaternion::from(pose.world_from_driver_rotation),
            world_from_driver_translation: [0.0; 3],
            driver_from_head_rotation: Quaternion::from(pose.driver_from_head_rotation),
            driver_from_head_translation: [0.0; 3],
            position: pose.position,
            velocity: pose.velocity,
            acceleration: [0.0; 3],
            rotation: Quaternion::from(pose.rotation),
            angular_velocity: pose.angular_velocity,
            angular_acceleration: [0.0; 3],
            result: match pose.result {
                TrackingResult::Uninitialized => 1,
                TrackingResult::RunningOk => 200,
                TrackingResult::RotationOnly => 300,
            },
            pose_is_valid: pose.valid,
            will_drift_in_yaw: false,
            should_apply_head_model: false,
            device_is_connected: pose.connected,
        }
    }
}

#[repr(C)]
pub(super) struct PropertyWrite {
    pub prop: i32,
    pub write_type: i32,
    pub set_error: i32,
    pub buffer: *mut c_void,
    pub buffer_size: u32,
    pub tag: u32,
    pub error: i32,
}

/// An incoming pure-virtual OpenVR class begins with its vtable pointer.
#[repr(C)]
pub(super) struct Interface<VTable> {
    pub vtable: *const VTable,
}

pub(super) type DriverContext = Interface<DriverContextVTable>;
pub(super) type ServerHost = Interface<ServerHostVTable>;
pub(super) type Properties = Interface<PropertiesVTable>;
pub(super) type Settings = Interface<SettingsVTable>;
pub(super) type DriverLog = Interface<DriverLogVTable>;

#[repr(C)]
pub(super) struct DriverContextVTable {
    pub get_generic_interface:
        unsafe extern "C" fn(*mut DriverContext, *const c_char, *mut i32) -> *mut c_void,
    pub get_driver_handle: *const c_void,
}

#[repr(C)]
pub(super) struct ProviderVTable {
    pub init: unsafe extern "C" fn(*mut c_void, *mut DriverContext) -> i32,
    pub cleanup: unsafe extern "C" fn(*mut c_void),
    pub get_interface_versions: unsafe extern "C" fn(*mut c_void) -> *const *const c_char,
    pub run_frame: unsafe extern "C" fn(*mut c_void),
    pub should_block_standby_mode: unsafe extern "C" fn(*mut c_void) -> bool,
    pub enter_standby: unsafe extern "C" fn(*mut c_void),
    pub leave_standby: unsafe extern "C" fn(*mut c_void),
}

#[repr(C)]
pub(super) struct TrackedDeviceVTable {
    pub activate: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    pub deactivate: unsafe extern "C" fn(*mut c_void),
    pub enter_standby: unsafe extern "C" fn(*mut c_void),
    pub get_component: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pub debug_request: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_char, u32),
    // MSVC x64 member methods receive the hidden return pointer after this.
    // OpenVR 2.15.6 declares this method but does not call it.
    pub get_pose: unsafe extern "C" fn(*mut c_void, *mut DriverPose) -> *mut DriverPose,
}

#[repr(C)]
pub(super) struct ServerHostVTable {
    pub tracked_device_added:
        unsafe extern "C" fn(*mut ServerHost, *const c_char, i32, *mut c_void) -> bool,
    pub tracked_device_pose_updated:
        unsafe extern "C" fn(*mut ServerHost, u32, *const DriverPose, u32),
    pub vsync_event: *const c_void,
    pub vendor_specific_event: *const c_void,
    pub is_exiting: *const c_void,
    pub poll_next_event: *const c_void,
    pub get_raw_tracked_device_poses: *const c_void,
    pub request_restart: *const c_void,
    pub get_frame_timings: *const c_void,
    pub set_display_eye_to_head: *const c_void,
    pub set_display_projection_raw: *const c_void,
    pub set_recommended_render_target_size: *const c_void,
}

#[repr(C)]
pub(super) struct PropertiesVTable {
    pub read_property_batch: *const c_void,
    pub write_property_batch:
        unsafe extern "C" fn(*mut Properties, u64, *mut PropertyWrite, u32) -> i32,
    pub get_prop_error_name_from_enum: *const c_void,
    pub tracked_device_to_property_container: unsafe extern "C" fn(*mut Properties, u32) -> u64,
}

#[repr(C)]
pub(super) struct SettingsVTable {
    pub get_settings_error_name_from_enum: *const c_void,
    pub set_bool: *const c_void,
    pub set_int32: *const c_void,
    pub set_float: *const c_void,
    pub set_string: *const c_void,
    pub get_bool: *const c_void,
    pub get_int32:
        unsafe extern "C" fn(*mut Settings, *const c_char, *const c_char, *mut i32) -> i32,
    pub get_float: *const c_void,
    pub get_string: *const c_void,
    pub remove_section: *const c_void,
    pub remove_key_in_section: *const c_void,
}

#[repr(C)]
pub(super) struct DriverLogVTable {
    pub log: unsafe extern "C" fn(*mut DriverLog, *const c_char),
}

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn openvr_2_15_6_record_layouts() {
        assert_eq!(size_of::<Quaternion>(), 32);
        assert_eq!(align_of::<Quaternion>(), 8);
        assert_eq!(size_of::<DriverPose>(), 280);
        assert_eq!(align_of::<DriverPose>(), 8);
        assert_eq!(
            [
                offset_of!(DriverPose, pose_time_offset),
                offset_of!(DriverPose, world_from_driver_rotation),
                offset_of!(DriverPose, world_from_driver_translation),
                offset_of!(DriverPose, driver_from_head_rotation),
                offset_of!(DriverPose, driver_from_head_translation),
                offset_of!(DriverPose, position),
                offset_of!(DriverPose, velocity),
                offset_of!(DriverPose, acceleration),
                offset_of!(DriverPose, rotation),
                offset_of!(DriverPose, angular_velocity),
                offset_of!(DriverPose, angular_acceleration),
                offset_of!(DriverPose, result),
                offset_of!(DriverPose, pose_is_valid),
                offset_of!(DriverPose, will_drift_in_yaw),
                offset_of!(DriverPose, should_apply_head_model),
                offset_of!(DriverPose, device_is_connected),
            ],
            [
                0, 8, 40, 64, 96, 120, 144, 168, 192, 224, 248, 272, 276, 277, 278, 279
            ],
        );
        assert_eq!(size_of::<PropertyWrite>(), 40);
        assert_eq!(align_of::<PropertyWrite>(), 8);
        assert_eq!(
            [
                offset_of!(PropertyWrite, prop),
                offset_of!(PropertyWrite, write_type),
                offset_of!(PropertyWrite, set_error),
                offset_of!(PropertyWrite, buffer),
                offset_of!(PropertyWrite, buffer_size),
                offset_of!(PropertyWrite, tag),
                offset_of!(PropertyWrite, error),
            ],
            [0, 4, 8, 16, 24, 28, 32],
        );
    }

    #[test]
    fn vtable_slots_match_openvr_2_15_6() {
        let sizes = [
            size_of::<DriverContextVTable>(),
            size_of::<ProviderVTable>(),
            size_of::<TrackedDeviceVTable>(),
            size_of::<ServerHostVTable>(),
            size_of::<PropertiesVTable>(),
            size_of::<SettingsVTable>(),
            size_of::<DriverLogVTable>(),
        ];
        assert_eq!(sizes, [2, 7, 6, 12, 4, 11, 1].map(|slot| slot * 8));
        assert_eq!(offset_of!(DriverContextVTable, get_generic_interface), 0);
        assert_eq!(offset_of!(ServerHostVTable, tracked_device_added), 0);
        assert_eq!(offset_of!(ServerHostVTable, tracked_device_pose_updated), 8);
        assert_eq!(offset_of!(PropertiesVTable, write_property_batch), 8);
        assert_eq!(
            offset_of!(PropertiesVTable, tracked_device_to_property_container),
            24
        );
        assert_eq!(offset_of!(SettingsVTable, get_int32), 48);
        assert_eq!(offset_of!(DriverLogVTable, log), 0);
    }
}
