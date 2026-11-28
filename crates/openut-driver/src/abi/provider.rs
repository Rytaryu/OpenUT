//! The exported factory and IServerTrackedDeviceProvider_004.

use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::ptr;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use super::host::Host;
use super::types::{
    DriverContext, INIT_DRIVER_FAILED, INIT_INTERFACE_NOT_FOUND, INIT_OK, PROVIDER_VERSION,
    ProviderVTable, interface_versions,
};
use super::{current_callback, guarded, guarded_cleanup};

use crate::frame::{CallbackKind, Frame};

pub(super) static PROVIDER: OnceLock<Provider> = OnceLock::new();

#[repr(C)]
pub(super) struct Provider {
    vtable: &'static ProviderVTable,
    pub(super) frame: Frame,
}

static PROVIDER_VTABLE: ProviderVTable = ProviderVTable {
    init,
    cleanup,
    get_interface_versions,
    run_frame,
    should_block_standby_mode,
    enter_standby,
    leave_standby,
};

fn provider() -> &'static Provider {
    PROVIDER.get_or_init(|| Provider {
        vtable: &PROVIDER_VTABLE,
        frame: Frame::new(),
    })
}

/// The names and result pointer follow SteamVR's factory contract.
#[cfg_attr(all(windows, target_arch = "x86_64"), unsafe(no_mangle))]
#[allow(non_snake_case)]
pub unsafe extern "C" fn HmdDriverFactory(
    interface_name: *const c_char,
    return_code: *mut i32,
) -> *mut c_void {
    let error = |code| {
        if !return_code.is_null() {
            // SAFETY: SteamVR supplies a writable result pointer or null.
            unsafe { return_code.write(code) };
        }
    };
    guarded(
        CallbackKind::Factory,
        || {
            error(INIT_DRIVER_FAILED);
            ptr::null_mut()
        },
        || {
            // SAFETY: SteamVR supplies a NUL-terminated interface name or null.
            if interface_name.is_null()
                || unsafe { CStr::from_ptr(interface_name) } != PROVIDER_VERSION
            {
                error(INIT_INTERFACE_NOT_FOUND);
                return ptr::null_mut();
            }
            error(INIT_OK);
            ptr::from_ref(provider()).cast_mut().cast()
        },
    )
}

unsafe extern "C" fn init(_this: *mut c_void, context: *mut DriverContext) -> i32 {
    guarded(
        CallbackKind::Init,
        || INIT_DRIVER_FAILED,
        || {
            let frame = &provider().frame;
            if !frame.gate.is_open() {
                return INIT_DRIVER_FAILED;
            }
            // SAFETY: OpenVR supplies its context for the lifetime of this initialization.
            let host = unsafe { Host::from_context(context, &frame.gate) };
            if host.is_some_and(|host| frame.init(Arc::new(host))) {
                INIT_OK
            } else {
                INIT_DRIVER_FAILED
            }
        },
    )
}

unsafe extern "C" fn cleanup(_this: *mut c_void) {
    guarded_cleanup(|| {
        let directory = std::env::var_os("LOCALAPPDATA")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .map(|path| path.join("OpenUT").join("logs"));
        provider()
            .frame
            .cleanup(current_callback(), directory.as_deref());
    });
}

unsafe extern "C" fn get_interface_versions(_this: *mut c_void) -> *const *const c_char {
    guarded(CallbackKind::Other, ptr::null, interface_versions)
}

unsafe extern "C" fn run_frame(_this: *mut c_void) {
    guarded(
        CallbackKind::RunFrame,
        || (),
        || provider().frame.run_frame(Instant::now()),
    );
}

unsafe extern "C" fn should_block_standby_mode(_this: *mut c_void) -> bool {
    guarded(CallbackKind::Other, || false, || false)
}

unsafe extern "C" fn enter_standby(_this: *mut c_void) {
    guarded(CallbackKind::Other, || (), || ());
}

unsafe extern "C" fn leave_standby(_this: *mut c_void) {
    guarded(CallbackKind::Other, || (), || ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_returns_the_stable_provider_only_for_its_interface() {
        let mut code = INIT_OK;
        // SAFETY: Each input is a C string and the result variable is writable.
        unsafe {
            assert!(HmdDriverFactory(c"invalid".as_ptr(), &mut code).is_null());
            assert_eq!(code, INIT_INTERFACE_NOT_FOUND);
            let first = HmdDriverFactory(PROVIDER_VERSION.as_ptr(), &mut code);
            assert!(!first.is_null());
            assert_eq!(code, INIT_OK);
            assert_eq!(
                first,
                HmdDriverFactory(PROVIDER_VERSION.as_ptr(), ptr::null_mut())
            );
        }
    }
}
