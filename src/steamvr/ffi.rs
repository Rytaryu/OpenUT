//! Windows OpenVR C API adapter. The SteamVR worker owns the session and its overlays.

#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use openut_space::{Mat3, Pose, Quat, Vec3, fingerprint::SpaceFingerprint};
use openvr_sys as vr;

use super::client::{
    Backend, MouseEvent, MouseEventKind, OverlayCommand, OverlayHandles, SteamVrEvent,
    TrackedDeviceClass, TrackedPose,
};

// These names identify the exact FnTable layouts shipped by openvr_sys 2.1.4.
const SYSTEM_VERSION: &CStr = c"FnTable:IVRSystem_026";
const CHAPERONE_VERSION: &CStr = c"FnTable:IVRChaperone_004";
const OVERLAY_VERSION: &CStr = c"FnTable:IVROverlay_028";
const POSE_COUNT: usize = vr::k_unMaxTrackedDeviceCount as usize;
const EVENT_BYTES: u32 = size_of::<vr::VREvent_t>() as u32;
const RGBA_BYTES_PER_PIXEL: u32 = 4;

#[derive(Default)]
pub(super) struct OpenVrBackend {
    session: Option<Session>,
}

impl Backend for OpenVrBackend {
    fn connect(&mut self) -> Result<(), String> {
        if self.session.is_none() {
            self.session = Some(Session::connect()?);
        }
        Ok(())
    }

    fn disconnect(&mut self) {
        self.session = None;
    }

    fn poll_event(&mut self) -> Option<SteamVrEvent> {
        self.session.as_ref()?.poll_event()
    }

    fn raw_poses(&mut self) -> Vec<TrackedPose> {
        self.session
            .as_ref()
            .map_or_else(Vec::new, Session::raw_poses)
    }

    fn fingerprint(&mut self) -> Option<SpaceFingerprint> {
        self.session.as_ref()?.fingerprint()
    }

    fn overlay(&mut self, command: OverlayCommand) -> Result<Option<OverlayHandles>, String> {
        self.session
            .as_mut()
            .ok_or_else(|| "SteamVR is disconnected".to_owned())?
            .command(command)
    }
}

// The SDK shim holds one client context per process. The guard releases it only after
// the session has finished using its tables.
static IN_USE: AtomicBool = AtomicBool::new(false);

struct Runtime;

impl Runtime {
    fn start() -> Result<Self, String> {
        if IN_USE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err("OpenVR already has a client in this process".to_owned());
        }

        let mut error = vr::EVRInitError_VRInitError_None;
        // SAFETY: This thread exclusively owns the SDK context and error is a writable enum.
        unsafe { vr::VR_InitInternal(&mut error, vr::EVRApplicationType_VRApplication_Overlay) };
        if error != vr::EVRInitError_VRInitError_None {
            IN_USE.store(false, Ordering::Release);
            return Err(format!("OpenVR overlay initialization failed ({error})"));
        }
        Ok(Self)
    }

    // The caller supplies the generated type corresponding to this exact SDK table name.
    unsafe fn table<T>(&self, version: &CStr) -> Result<NonNull<T>, String> {
        let mut error = vr::EVRInitError_VRInitError_None;
        // SAFETY: The initialized context is live, the name is NUL-terminated, and error is writable.
        let address = unsafe { vr::VR_GetGenericInterface(version.as_ptr(), &mut error) };
        if error == vr::EVRInitError_VRInitError_None
            && let Some(table) = NonNull::new(address as *mut T)
        {
            return Ok(table);
        }
        Err(format!("OpenVR table {version:?} is unavailable ({error})"))
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: This guard uniquely owns a successfully initialized SDK context.
        unsafe { vr::VR_ShutdownInternal() };
        IN_USE.store(false, Ordering::Release);
    }
}

struct Session {
    system: NonNull<vr::VR_IVRSystem_FnTable>,
    chaperone: Option<NonNull<vr::VR_IVRChaperone_FnTable>>,
    overlay: NonNull<vr::VR_IVROverlay_FnTable>,
    /// Each dashboard owns its thumbnail in SteamVR.
    dashboards: BTreeMap<u64, u64>,
    _runtime: Runtime,
}

impl Session {
    fn connect() -> Result<Self, String> {
        let runtime = Runtime::start()?;
        // SAFETY: Each requested version names the corresponding type in the pinned SDK.
        let (system, chaperone, overlay) = unsafe {
            (
                runtime.table(SYSTEM_VERSION)?,
                runtime.table(CHAPERONE_VERSION).ok(),
                runtime.table(OVERLAY_VERSION)?,
            )
        };
        Ok(Self {
            system,
            chaperone,
            overlay,
            dashboards: BTreeMap::new(),
            _runtime: runtime,
        })
    }

    fn system(&self) -> &vr::VR_IVRSystem_FnTable {
        // SAFETY: The matching table remains readable until the session's SDK guard shuts down.
        unsafe { self.system.as_ref() }
    }

    fn overlay(&self) -> &vr::VR_IVROverlay_FnTable {
        // SAFETY: The matching table remains readable until the session's SDK guard shuts down.
        unsafe { self.overlay.as_ref() }
    }

    fn poll_event(&self) -> Option<SteamVrEvent> {
        let mut event = vr::VREvent_t::default();
        if let Some(poll) = self.system().PollNextEvent {
            // SAFETY: The session owns a live table, and event is writable for EVENT_BYTES bytes.
            if unsafe { poll(&mut event, EVENT_BYTES) } {
                return Some(Self::system_event(event.eventType));
            }
        }
        let poll = self.overlay().PollNextOverlayEvent?;
        for &handle in self.dashboards.keys() {
            // SAFETY: This dashboard belongs to this live session, and event is writable.
            if unsafe { poll(handle, &mut event, EVENT_BYTES) } {
                return Some(mouse_event(handle, &event));
            }
        }
        None
    }

    fn system_event(event_type: u32) -> SteamVrEvent {
        match event_type as vr::EVREventType {
            vr::EVREventType_VREvent_Quit => SteamVrEvent::Quit,
            vr::EVREventType_VREvent_ChaperoneUniverseHasChanged
            | vr::EVREventType_VREvent_ChaperoneRoomSetupCommitted
            | vr::EVREventType_VREvent_StandingZeroPoseReset => SteamVrEvent::SpaceChanged,
            _ => SteamVrEvent::Other,
        }
    }

    fn raw_poses(&self) -> Vec<TrackedPose> {
        let (Some(read), Some(class)) = (
            self.system().GetDeviceToAbsoluteTrackingPose,
            self.system().GetTrackedDeviceClass,
        ) else {
            return Vec::new();
        };
        let mut samples = [vr::TrackedDevicePose_t::default(); POSE_COUNT];
        // SAFETY: samples contains POSE_COUNT writable entries of the SDK pose type.
        unsafe {
            read(
                vr::ETrackingUniverseOrigin_TrackingUniverseRawAndUncalibrated,
                0.0,
                samples.as_mut_ptr(),
                POSE_COUNT as u32,
            )
        };
        samples
            .into_iter()
            .enumerate()
            .filter(|(_, sample)| sample.bDeviceIsConnected)
            .filter_map(|(index, sample)| {
                let pose = matrix_pose(sample.mDeviceToAbsoluteTracking.m)?;
                let device = index as u32;
                // SAFETY: The index comes from the runtime pose array and is below its device limit.
                let class = match unsafe { class(device) } {
                    vr::ETrackedDeviceClass_TrackedDeviceClass_HMD => TrackedDeviceClass::Hmd,
                    vr::ETrackedDeviceClass_TrackedDeviceClass_Controller => {
                        TrackedDeviceClass::Controller
                    }
                    _ => TrackedDeviceClass::Other,
                };
                Some(TrackedPose {
                    device,
                    class,
                    valid: sample.bPoseIsValid,
                    running_ok: sample.eTrackingResult
                        == vr::ETrackingResult_TrackingResult_Running_OK,
                    pose,
                })
            })
            .collect()
    }

    fn fingerprint(&self) -> Option<SpaceFingerprint> {
        let id = self.system().GetUint64TrackedDeviceProperty?;
        let standing_origin = self.system().GetRawZeroPoseToStandingAbsoluteTrackingPose?;
        let mut error = vr::ETrackedPropertyError_TrackedProp_Success;
        // SAFETY: The table is live and the SDK HMD index and property match this getter.
        let universe = unsafe {
            id(
                vr::k_unTrackedDeviceIndex_Hmd,
                vr::ETrackedDeviceProperty_Prop_CurrentUniverseId_Uint64,
                &mut error,
            )
        };
        if error != vr::ETrackedPropertyError_TrackedProp_Success {
            return None;
        }
        // SAFETY: This argument-free getter belongs to the live system table.
        let origin = standing_pose_in_raw_space(unsafe { standing_origin() }.m)?;
        SpaceFingerprint::new(universe, origin, self.play_area())
    }

    fn play_area(&self) -> Option<[f64; 2]> {
        // SAFETY: When available, the matching table is readable for the session lifetime.
        let chaperone = unsafe { self.chaperone?.as_ref() };
        let size = chaperone.GetPlayAreaSize?;
        let (mut width, mut depth) = (0.0, 0.0);
        // SAFETY: Both SDK scalar output pointers are writable and distinct.
        let reported = unsafe { size(&mut width, &mut depth) };
        reported.then_some([f64::from(width), f64::from(depth)])
    }

    fn command(&mut self, command: OverlayCommand) -> Result<Option<OverlayHandles>, String> {
        match command {
            OverlayCommand::CreateDashboard { key, name } => {
                return self.create_dashboard(key, name).map(Some);
            }
            OverlayCommand::SetImage {
                handle,
                width,
                height,
                rgba,
            } => {
                self.set_image(handle, width, height, rgba)?;
            }
            OverlayCommand::SetWidth { handle, meters } => {
                self.set_width(handle, meters)?;
            }
            OverlayCommand::SetMouseScale {
                handle,
                width,
                height,
            } => {
                self.set_mouse_scale(handle, width, height)?;
            }
            OverlayCommand::Destroy { handle } => self.destroy_dashboard(handle)?,
        }
        Ok(None)
    }

    fn create_dashboard(&mut self, key: String, name: String) -> Result<OverlayHandles, String> {
        let mut key = overlay_string(key)?;
        let mut name = overlay_string(name)?;
        let create = interface_method(self.overlay().CreateDashboardOverlay)?;
        let set_input = interface_method(self.overlay().SetOverlayInputMethod)?;
        let destroy = interface_method(self.overlay().DestroyOverlay)?;
        let (mut dashboard, mut thumbnail) = (0, 0);
        // SAFETY: Both strings have a terminal NUL and both handle outputs are writable.
        let result = unsafe {
            create(
                key.as_mut_ptr().cast(),
                name.as_mut_ptr().cast(),
                &mut dashboard,
                &mut thumbnail,
            )
        };
        overlay_result("create dashboard", result)?;
        // SAFETY: dashboard belongs to this live session following a successful creation.
        let result = unsafe { set_input(dashboard, vr::VROverlayInputMethod_Mouse) };
        if let Err(error) = overlay_result("enable dashboard mouse input", result) {
            // SAFETY: Removing the newly created dashboard also drops its thumbnail.
            unsafe { destroy(dashboard) };
            return Err(error);
        }
        self.dashboards.insert(dashboard, thumbnail);
        Ok(OverlayHandles {
            dashboard,
            thumbnail,
        })
    }

    fn set_image(
        &self,
        handle: u64,
        width: u32,
        height: u32,
        mut rgba: Vec<u8>,
    ) -> Result<(), String> {
        self.require_overlay(handle)?;
        let size = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(width, height)| width.checked_mul(height))
            .and_then(|pixels| pixels.checked_mul(RGBA_BYTES_PER_PIXEL as usize));
        if rgba.is_empty() || size != Some(rgba.len()) {
            return Err("Invalid OpenVR RGBA image dimensions or byte count".to_owned());
        }
        let set = interface_method(self.overlay().SetOverlayRaw)?;
        // SAFETY: The owned buffer contains exactly width × height × 4 writable bytes
        // and stays live throughout the synchronous upload.
        let result = unsafe {
            set(
                handle,
                rgba.as_mut_ptr().cast(),
                width,
                height,
                RGBA_BYTES_PER_PIXEL,
            )
        };
        overlay_result("upload RGBA image", result)
    }

    fn set_width(&self, handle: u64, meters: f32) -> Result<(), String> {
        self.require_overlay(handle)?;
        if !positive_finite(meters) {
            return Err("Invalid OpenVR overlay width".to_owned());
        }
        let set = interface_method(self.overlay().SetOverlayWidthInMeters)?;
        // SAFETY: The live session owns this handle, and meters is a positive finite value.
        overlay_result("set overlay width", unsafe { set(handle, meters) })
    }

    fn set_mouse_scale(&self, handle: u64, width: f32, height: f32) -> Result<(), String> {
        self.require_overlay(handle)?;
        if !positive_finite(width) || !positive_finite(height) {
            return Err("Invalid OpenVR overlay mouse scale".to_owned());
        }
        let set = interface_method(self.overlay().SetOverlayMouseScale)?;
        let mut scale = vr::HmdVector2_t { v: [width, height] };
        // SAFETY: The session owns the handle, and scale lives throughout the synchronous setter.
        overlay_result("set mouse scale", unsafe { set(handle, &mut scale) })
    }

    fn require_overlay(&self, handle: u64) -> Result<(), String> {
        if self.dashboards.contains_key(&handle)
            || self.dashboards.values().any(|&thumb| thumb == handle)
        {
            Ok(())
        } else {
            Err("OpenVR overlay is unavailable in this session".to_owned())
        }
    }

    fn destroy_dashboard(&mut self, handle: u64) -> Result<(), String> {
        if !self.dashboards.contains_key(&handle) {
            return Err("OpenVR dashboard is unavailable in this session".to_owned());
        }
        let destroy = interface_method(self.overlay().DestroyOverlay)?;
        // SAFETY: The session owns the dashboard, including its separately indexed thumbnail.
        overlay_result("destroy dashboard", unsafe { destroy(handle) })?;
        self.dashboards.remove(&handle);
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(destroy) = self.overlay().DestroyOverlay {
            for &handle in self.dashboards.keys() {
                // SAFETY: This session still owns each dashboard and its SDK context is live.
                unsafe { destroy(handle) };
            }
        }
    }
}

fn interface_method<T>(method: Option<T>) -> Result<T, String> {
    method.ok_or_else(|| "OpenVR overlay method is unavailable".to_owned())
}

fn overlay_result(operation: &str, result: vr::EVROverlayError) -> Result<(), String> {
    if result == vr::EVROverlayError_VROverlayError_None {
        Ok(())
    } else {
        Err(format!("OpenVR could not {operation} ({result})"))
    }
}

fn overlay_string(value: String) -> Result<Vec<u8>, String> {
    CString::new(value)
        .map(CString::into_bytes_with_nul)
        .map_err(|_| "OpenVR overlay text contains a NUL byte".to_owned())
}

fn positive_finite(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn matrix_pose(matrix: [[f32; 4]; 3]) -> Option<Pose> {
    let [[xx, xy, xz, x], [yx, yy, yz, y], [zx, zy, zz, z]] = matrix;
    if ![x, y, z].into_iter().all(f32::is_finite) {
        return None;
    }
    let rotation = [[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]].map(|row| row.map(f64::from));
    Some(Pose {
        position: Vec3::new(f64::from(x), f64::from(y), f64::from(z)),
        rotation: Quat::from_matrix(Mat3::new(rotation))?,
    })
}

fn standing_pose_in_raw_space(raw_to_standing: [[f32; 4]; 3]) -> Option<Pose> {
    let transform = matrix_pose(raw_to_standing)?;
    let rotation = transform.rotation.conjugate();
    Some(Pose {
        position: rotation.rotate(transform.position * -1.0),
        rotation,
    })
}

fn mouse_event(handle: u64, event: &vr::VREvent_t) -> SteamVrEvent {
    let kind = match event.eventType as vr::EVREventType {
        vr::EVREventType_VREvent_MouseMove => MouseEventKind::Move,
        vr::EVREventType_VREvent_MouseButtonDown => MouseEventKind::ButtonDown,
        vr::EVREventType_VREvent_MouseButtonUp => MouseEventKind::ButtonUp,
        _ => return SteamVrEvent::Other,
    };
    // SAFETY: Only the three SDK mouse event types reach this read of the mouse payload.
    let mouse = unsafe { event.data.mouse };
    if !mouse.x.is_finite() || !mouse.y.is_finite() {
        return SteamVrEvent::Other;
    }
    SteamVrEvent::Mouse(MouseEvent {
        handle,
        kind,
        x: mouse.x,
        y: mouse.y,
        button: mouse.button,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_origin_is_expressed_in_raw_coordinates() {
        let position = Vec3::new(2.0, 3.0, 5.0);
        let translated = standing_pose_in_raw_space([
            [1.0, 0.0, 0.0, -2.0],
            [0.0, 1.0, 0.0, -3.0],
            [0.0, 0.0, 1.0, -5.0],
        ])
        .unwrap();
        let turned = standing_pose_in_raw_space([
            [0.0, 0.0, 1.0, -5.0],
            [0.0, 1.0, 0.0, -3.0],
            [-1.0, 0.0, 0.0, 2.0],
        ])
        .unwrap();
        assert_eq!(translated.position, position);
        assert!((turned.position - position).norm() < 1e-12);
        assert!(
            (turned.rotation.rotate(Vec3::new(0.0, 0.0, 1.0)) - Vec3::new(-1.0, 0.0, 0.0)).norm()
                < 1e-12
        );
    }

    #[test]
    fn invalid_matrices_are_not_exposed_as_poses() {
        assert!(matrix_pose([[0.0; 4]; 3]).is_none());
        assert!(
            matrix_pose([
                [1.0, 0.0, 0.0, f32::NAN],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .is_none()
        );
    }
}
