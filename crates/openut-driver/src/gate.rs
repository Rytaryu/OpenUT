//! Permits for host calls during tracked-device and provider lifetimes.

use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

/// A device index confirmed immediately before a host call.
pub struct CheckedIndex {
    index: u32,
    thread: PhantomData<Rc<()>>,
}

impl CheckedIndex {
    pub fn get(&self) -> u32 {
        self.index
    }
}

/// Proof that the provider was open immediately before a host call.
pub struct FrameOpen {
    thread: PhantomData<Rc<()>>,
}

pub struct Gate {
    inner: Permits<u32>,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            inner: Permits::new(None),
        }
    }
}

impl Gate {
    pub fn open(&self, index: u32) -> bool {
        let mut state = self.inner.state();
        if state.sealed {
            return false;
        }
        state.generation = state.generation.wrapping_add(1);
        state.open = Some(index);
        true
    }

    pub fn try_pass(&self, expected: u32) -> Option<GatePass<'_>> {
        self.inner.try_pass(expected).map(GatePass)
    }

    pub fn close_now(&self) {
        self.inner.state().open = None;
    }

    /// Permanently close during provider cleanup. Activation cannot reopen it.
    pub fn seal(&self) {
        let mut state = self.inner.state();
        state.open = None;
        state.sealed = true;
    }

    pub fn wait(&self, warn_after: Option<Duration>, on_warn: impl FnOnce()) -> Duration {
        self.inner.wait(warn_after, on_warn)
    }

    pub fn close(&self, warn_after: Duration, on_warn: impl FnOnce()) -> Duration {
        self.close_now();
        self.wait(Some(warn_after), on_warn)
    }
}

pub struct GatePass<'a>(Pass<'a, u32>);

impl GatePass<'_> {
    /// The permit stays issued until this call returns, including when it unwinds.
    pub fn call<R>(self, f: impl FnOnce(&CheckedIndex) -> R) -> Option<R> {
        let index = self.0.checked()?;
        Some(f(&CheckedIndex {
            index,
            thread: PhantomData,
        }))
    }
}

pub struct FrameGate {
    inner: Permits<()>,
}

impl Default for FrameGate {
    fn default() -> Self {
        Self {
            inner: Permits::new(Some(())),
        }
    }
}

impl FrameGate {
    pub fn try_pass(&self) -> Option<FramePass<'_>> {
        self.inner.try_pass(()).map(FramePass)
    }

    pub fn is_open(&self) -> bool {
        self.inner.state().open.is_some()
    }

    /// Check again for each host operation, without holding the gate in the callback.
    pub fn if_open<R>(&self, f: impl FnOnce(&FrameOpen) -> R) -> Option<R> {
        self.inner.try_state()?.open?;
        Some(f(&FrameOpen {
            thread: PhantomData,
        }))
    }

    pub fn close_now(&self) {
        self.inner.state().open = None;
    }

    pub fn wait(&self) -> Duration {
        self.inner.wait(None, || {})
    }

    pub fn close(&self) -> Duration {
        self.close_now();
        self.wait()
    }
}

pub struct FramePass<'a>(Pass<'a, ()>);

impl FramePass<'_> {
    pub fn if_open<R>(&self, f: impl FnOnce(&FrameOpen) -> R) -> Option<R> {
        self.0.checked()?;
        Some(f(&FrameOpen {
            thread: PhantomData,
        }))
    }
}

struct Permits<T> {
    state: Mutex<State<T>>,
    returned: Condvar,
}

struct State<T> {
    open: Option<T>,
    generation: u64,
    sealed: bool,
    holder: Option<ThreadId>,
}

impl<T: Copy + Eq> Permits<T> {
    fn new(open: Option<T>) -> Self {
        Self {
            state: Mutex::new(State {
                open,
                generation: 0,
                sealed: false,
                holder: None,
            }),
            returned: Condvar::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn try_state(&self) -> Option<MutexGuard<'_, State<T>>> {
        match self.state.try_lock() {
            Ok(state) => Some(state),
            Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    fn try_pass(&self, expected: T) -> Option<Pass<'_, T>> {
        let mut state = self.try_state()?;
        if state.open != Some(expected) || state.holder.is_some() {
            return None;
        }
        state.holder = Some(thread::current().id());
        Some(Pass {
            gate: self,
            expected,
            generation: state.generation,
            thread: PhantomData,
        })
    }

    fn wait(&self, warn_after: Option<Duration>, on_warn: impl FnOnce()) -> Duration {
        let current = thread::current().id();
        let mut state = self.state();
        if state.holder.is_none_or(|holder| holder == current) {
            return Duration::ZERO;
        }
        let start = Instant::now();
        let mut warning = warn_after.map(|delay| (delay, on_warn));
        while state.holder.is_some_and(|holder| holder != current) {
            if let Some((delay, _)) = warning.as_ref() {
                let remaining = delay.saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    if let Some((_, warn)) = warning.take() {
                        drop(state);
                        // A failing diagnostic must not let the caller return
                        // while the other thread still holds the permit.
                        let _ = catch_unwind(AssertUnwindSafe(warn));
                        state = self.state();
                    }
                } else {
                    let (next, _) = self
                        .returned
                        .wait_timeout(state, remaining)
                        .unwrap_or_else(PoisonError::into_inner);
                    state = next;
                }
            } else {
                state = self
                    .returned
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
        start.elapsed()
    }
}

struct Pass<'a, T: Copy + Eq> {
    gate: &'a Permits<T>,
    expected: T,
    generation: u64,
    thread: PhantomData<Rc<()>>,
}

impl<T: Copy + Eq> Pass<'_, T> {
    fn checked(&self) -> Option<T> {
        let state = self.gate.try_state()?;
        (state.open == Some(self.expected) && state.generation == self.generation)
            .then_some(self.expected)
    }
}

impl<T: Copy + Eq> Drop for Pass<'_, T> {
    fn drop(&mut self) {
        self.gate.state().holder = None;
        self.gate.returned.notify_all();
    }
}

#[cfg(test)]
mod tests;
