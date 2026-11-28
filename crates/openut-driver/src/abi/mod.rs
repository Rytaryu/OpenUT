//! SteamVR's C++ interface boundary.

pub(crate) mod device;
mod host;
mod provider;
mod types;

use std::cell::Cell;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::frame::CallbackKind;

thread_local! {
    static CALLBACK_DEPTH: Cell<u32> = const { Cell::new(0) };
    static CALLBACK: Cell<Option<CallbackKind>> = const { Cell::new(None) };
    static ADDING_TRACKER: Cell<bool> = const { Cell::new(false) };
}

struct CallbackScope(Option<CallbackKind>);

impl CallbackScope {
    fn enter(kind: CallbackKind) -> Self {
        CALLBACK_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self(CALLBACK.with(|current| current.replace(Some(kind))))
    }
}

impl Drop for CallbackScope {
    fn drop(&mut self) {
        CALLBACK.with(|current| current.set(self.0));
        CALLBACK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

fn current_callback() -> Option<CallbackKind> {
    (CALLBACK_DEPTH.with(Cell::get) != 0)
        .then(|| CALLBACK.with(Cell::get))
        .flatten()
}

/// Contain panics before returning through SteamVR's ABI.
fn guarded<T>(kind: CallbackKind, fallback: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
    // Keep the callback scope active if reporting the panic reenters SteamVR.
    let scope = CallbackScope::enter(kind);
    let result = match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            // A user-supplied panic payload may itself panic when dropped.
            mem::forget(payload);
            guarded_cleanup(|| {
                if let Some(provider) = provider::PROVIDER.get() {
                    let line = format!("openut: {} failed", kind.name());
                    if matches!(kind, CallbackKind::RunFrame) {
                        provider.frame.log_frame_panic(&line);
                    } else {
                        provider.frame.log(&line);
                    }
                }
            });
            fallback()
        }
    };
    drop(scope);
    result
}

/// Cleanup cannot call SteamVR, including to report a panic.
fn guarded_cleanup(f: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(f)) {
        mem::forget(payload);
    }
}

pub(crate) fn with_tracker_add<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ADDING_TRACKER.with(|adding| adding.set(self.0));
        }
    }
    let _restore = Restore(ADDING_TRACKER.with(|adding| adding.replace(true)));
    f()
}

pub(crate) fn in_tracker_add() -> bool {
    ADDING_TRACKER.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guards_panics_and_restores_the_nested_callback_scope() {
        assert!(current_callback().is_none());
        let value = guarded(
            CallbackKind::RunFrame,
            || 17,
            || {
                assert!(matches!(current_callback(), Some(CallbackKind::RunFrame)));
                let fallback = guarded(CallbackKind::Activate, || 16, || panic!("fallback"));
                assert!(matches!(current_callback(), Some(CallbackKind::RunFrame)));
                fallback
            },
        );
        assert_eq!(value, 16);
        assert!(current_callback().is_none());

        guarded_cleanup(|| with_tracker_add(|| panic!("restore")));
        assert!(!in_tracker_add());
    }
}
