//! An ITrackedDeviceServerDriver_005 object and its callbacks.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use super::guarded;
use super::provider::PROVIDER;
use super::types::{DriverPose, INIT_DRIVER_FAILED, INIT_OK, TrackedDeviceVTable};

use crate::frame::CallbackKind;
use crate::gate::Gate;
use crate::pose::Pose;

const INVALID_INDEX: u32 = u32::MAX;

#[repr(C)]
pub(crate) struct Device {
    vtable: &'static TrackedDeviceVTable,
    pub(crate) gate: Gate,
    pub(crate) slot: usize,
    index: AtomicU32,
}

impl Device {
    pub(crate) fn new(slot: usize) -> Self {
        Self {
            vtable: &DEVICE_VTABLE,
            gate: Gate::default(),
            slot,
            index: AtomicU32::new(INVALID_INDEX),
        }
    }

    pub(crate) fn as_raw(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }

    pub(crate) fn index(&self) -> Option<u32> {
        let index = self.index.load(Ordering::Acquire);
        (index != INVALID_INDEX).then_some(index)
    }

    pub(crate) fn set_index(&self, index: u32) {
        self.index.store(index, Ordering::Release);
    }
}

static DEVICE_VTABLE: TrackedDeviceVTable = TrackedDeviceVTable {
    activate,
    deactivate,
    enter_standby,
    get_component,
    debug_request,
    get_pose,
};

unsafe extern "C" fn activate(this: *mut c_void, index: u32) -> i32 {
    guarded(
        CallbackKind::Activate,
        || INIT_DRIVER_FAILED,
        || {
            if index == INVALID_INDEX {
                return INIT_DRIVER_FAILED;
            }
            let frame = PROVIDER.get().map(|provider| &provider.frame);
            if frame.is_some_and(|frame| {
                frame
                    .device(this.addr())
                    .is_some_and(|device| frame.activate(&device, index))
            }) {
                INIT_OK
            } else {
                INIT_DRIVER_FAILED
            }
        },
    )
}

unsafe extern "C" fn deactivate(this: *mut c_void) {
    guarded(
        CallbackKind::Deactivate,
        || (),
        || {
            if let Some(provider) = PROVIDER.get()
                && let Some(device) = provider.frame.device(this.addr())
            {
                provider.frame.deactivate(&device);
            }
        },
    );
}

unsafe extern "C" fn enter_standby(_this: *mut c_void) {
    guarded(CallbackKind::Other, || (), || ());
}

unsafe extern "C" fn get_component(_this: *mut c_void, _name: *const c_char) -> *mut c_void {
    guarded(CallbackKind::Other, ptr::null_mut, ptr::null_mut)
}

unsafe extern "C" fn debug_request(
    _this: *mut c_void,
    _request: *const c_char,
    response: *mut c_char,
    response_size: u32,
) {
    guarded(
        CallbackKind::Other,
        || (),
        || {
            if !response.is_null() && response_size != 0 {
                // SAFETY: A nonzero response size makes SteamVR's first byte writable.
                unsafe { response.write(0) };
            }
        },
    );
}

// MSVC x64 member calls returning DriverPose supply an output pointer after this.
unsafe extern "C" fn get_pose(_this: *mut c_void, result: *mut DriverPose) -> *mut DriverPose {
    guarded(CallbackKind::Other, ptr::null_mut, || {
        if !result.is_null() {
            // SAFETY: SteamVR supplies writable storage for the aggregate return.
            unsafe { result.write(DriverPose::from(&Pose::invalid(false))) };
        }
        result
    })
}
