//! Calls from gated driver callbacks to the SteamVR host.

use std::ffi::{CStr, CString};
use std::mem::size_of;
use std::ptr::NonNull;

use crate::frame::HostApi;
use crate::gate::{CheckedIndex, FrameGate, FrameOpen};
use crate::pose::Pose;

use super::device::Device;
use super::types::{
    DriverContext, DriverLog, DriverPose, INIT_OK, Interface, LOG_VERSION, PROPERTIES_VERSION,
    Properties, PropertyWrite, SERVER_HOST_VERSION, SETTINGS_VERSION, ServerHost, Settings,
};

const GENERIC_TRACKER: i32 = 3;
const INVALID_PROPERTY_CONTAINER: u64 = 0;
const PROPERTY_OK: i32 = 0;
const PROPERTY_SET: i32 = 0;
const STRING_TAG: u32 = 5;
const DEFAULT_PORT: u16 = 27_316;

pub(super) struct Host {
    server: NonNull<ServerHost>,
    properties: NonNull<Properties>,
    settings: NonNull<Settings>,
    log: NonNull<DriverLog>,
}

// SAFETY: These are borrowed interface objects from SteamVR, not Rust-owned
// memory. The driver invokes them only from SteamVR callbacks admitted by its
// gates, which wait for calls on other threads before releasing driver state.
unsafe impl Send for Host {}
// SAFETY: The shared adapter never mutates its own interface pointers and
// passes only the foreign object addresses back to their OpenVR methods.
unsafe impl Sync for Host {}

impl Host {
    /// Obtains the four host interfaces used by the driver.
    ///
    /// # Safety
    /// `context` must come from OpenVR's current provider Init callback.
    pub(super) unsafe fn from_context(
        context: *mut DriverContext,
        frame: &FrameGate,
    ) -> Option<Self> {
        let context = NonNull::new(context)?;
        // SAFETY: The context was supplied by SteamVR's Init. Check the gate
        // separately for each lookup because a lookup can enter Cleanup.
        unsafe {
            Some(Self {
                server: guarded_interface(context, frame, SERVER_HOST_VERSION)?,
                properties: guarded_interface(context, frame, PROPERTIES_VERSION)?,
                settings: guarded_interface(context, frame, SETTINGS_VERSION)?,
                log: guarded_interface(context, frame, LOG_VERSION)?,
            })
        }
    }
}

impl HostApi for Host {
    fn add_tracker(&self, _open: &FrameOpen, name: &CStr, device: &Device) -> bool {
        // SAFETY: The open frame admits this callback. The provider keeps the
        // boxed device at a stable address until frames and devices quiesce.
        unsafe {
            vtable(&self.server).is_some_and(|table| {
                (table.tracked_device_added)(
                    self.server.as_ptr(),
                    name.as_ptr(),
                    GENERIC_TRACKER,
                    device.as_raw(),
                )
            })
        }
    }

    fn update_pose(&self, index: &CheckedIndex, pose: &Pose) {
        let pose = DriverPose::from(pose);
        // SAFETY: The token proves the activated index was checked for this
        // call. OpenVR reads the supplied C-layout pose during the call only.
        unsafe {
            if let Some(table) = vtable(&self.server) {
                (table.tracked_device_pose_updated)(
                    self.server.as_ptr(),
                    index.get(),
                    &pose,
                    size_of::<DriverPose>() as u32,
                );
            }
        }
    }

    fn property_container(&self, _open: &FrameOpen, index: u32) -> Option<u64> {
        // SAFETY: The open frame admits this call to the Properties interface.
        let id = unsafe {
            let table = vtable(&self.properties)?;
            (table.tracked_device_to_property_container)(self.properties.as_ptr(), index)
        };
        (id != INVALID_PROPERTY_CONTAINER).then_some(id)
    }

    fn write_properties(&self, _open: &FrameOpen, container: u64) -> bool {
        let mut manufacturer = *b"OpenUT\0";
        let mut model = *b"OpenUT Tracker\0";
        let mut tracking_system = *b"openut\0";
        let mut input_profile = *b"{openut}/input/openut_tracker_profile.json\0";
        let mut render_model = *b"{htc}vr_tracker_vive_1_0\0";
        let values: [(i32, &mut [u8]); 5] = [
            (1_005, &mut manufacturer),
            (1_001, &mut model),
            (1_000, &mut tracking_system),
            (1_037, &mut input_profile),
            (1_003, &mut render_model),
        ];
        let mut batch = values.map(|(prop, value)| PropertyWrite {
            prop,
            write_type: PROPERTY_SET,
            set_error: PROPERTY_OK,
            buffer: value.as_mut_ptr().cast(),
            buffer_size: value.len() as u32,
            tag: STRING_TAG,
            error: PROPERTY_OK,
        });
        // SAFETY: The open frame admits the property write. Each batch entry
        // points to a separate live NUL-terminated buffer for this call.
        let result = unsafe {
            let Some(table) = vtable(&self.properties) else {
                return false;
            };
            (table.write_property_batch)(
                self.properties.as_ptr(),
                container,
                batch.as_mut_ptr(),
                batch.len() as u32,
            )
        };
        result == PROPERTY_OK && batch.iter().all(|entry| entry.error == PROPERTY_OK)
    }

    fn log(&self, _open: &FrameOpen, line: &str) {
        let line = line.strip_prefix("openut: ").unwrap_or(line);
        let Ok(line) = CString::new(format!("openut: {line}")) else {
            return;
        };
        // SAFETY: The open frame admits logging, and line remains valid
        // and NUL-terminated for the duration of this call.
        unsafe {
            if let Some(table) = vtable(&self.log) {
                (table.log)(self.log.as_ptr(), line.as_ptr());
            }
        }
    }

    fn bridge_port(&self, _open: &FrameOpen) -> u16 {
        let mut error = 0;
        // SAFETY: The open frame admits the settings read, both keys are
        // static C strings, and the error output points to a writable i32.
        let port = unsafe {
            let Some(table) = vtable(&self.settings) else {
                return DEFAULT_PORT;
            };
            (table.get_int32)(
                self.settings.as_ptr(),
                c"driver_openut".as_ptr(),
                c"bridge_port".as_ptr(),
                &mut error,
            )
        };
        if error != 0 {
            return DEFAULT_PORT;
        }
        match u16::try_from(port) {
            Ok(value) if value != 0 => value,
            _ => DEFAULT_PORT,
        }
    }
}

unsafe fn guarded_interface<T>(
    context: NonNull<DriverContext>,
    frame: &FrameGate,
    version: &CStr,
) -> Option<NonNull<Interface<T>>> {
    frame.if_open(|_| {
        // SAFETY: The caller supplied the live context from Init. The frame
        // is open for this lookup, and version names the expected vtable.
        unsafe {
            let table = vtable(&context)?;
            let mut error = INIT_OK;
            let address =
                (table.get_generic_interface)(context.as_ptr(), version.as_ptr(), &mut error);
            if error != INIT_OK {
                return None;
            }
            NonNull::new(address.cast())
        }
    })?
}

/// The pointer must refer to the expected OpenVR class during an admitted call.
unsafe fn vtable<T>(object: &NonNull<Interface<T>>) -> Option<&T> {
    // SAFETY: The caller proves that SteamVR supplied the correct live object.
    // The C++ vtable is immutable; this never borrows the object's other data.
    unsafe { object.as_ref().vtable.as_ref() }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ffi::{c_char, c_void};
    use std::ptr;

    use super::super::types::DriverContextVTable;
    use super::*;

    #[repr(C)]
    struct ClosingContext<'a> {
        interface: DriverContext,
        frame: &'a FrameGate,
        calls: Cell<usize>,
        server: ServerHost,
    }

    unsafe extern "C" fn close_during_lookup(
        context: *mut DriverContext,
        _version: *const c_char,
        error: *mut i32,
    ) -> *mut c_void {
        // SAFETY: The only caller of this fake vtable constructs a
        // ClosingContext with DriverContext as its first field.
        let context = unsafe { &*context.cast::<ClosingContext<'_>>() };
        context.calls.set(context.calls.get() + 1);
        context.frame.close_now();
        // SAFETY: Host passes a valid mutable error output to the lookup.
        unsafe { error.write(INIT_OK) };
        ptr::from_ref(&context.server).cast_mut().cast()
    }

    #[test]
    fn reentrant_close_prevents_remaining_interface_lookups() {
        let frame = FrameGate::default();
        let table = DriverContextVTable {
            get_generic_interface: close_during_lookup,
            get_driver_handle: ptr::null(),
        };
        let mut context = ClosingContext {
            interface: DriverContext { vtable: &table },
            frame: &frame,
            calls: Cell::new(0),
            server: ServerHost {
                vtable: ptr::null(),
            },
        };
        // SAFETY: The mock context and table remain live for every potential
        // call. The first lookup closes the frame before any host is created.
        let host = unsafe { Host::from_context(ptr::from_mut(&mut context).cast(), &frame) };
        assert!(host.is_none());
        assert_eq!(context.calls.get(), 1);
    }
}
