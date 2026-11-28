//! Frame coordination and teardown, independent of the SteamVR vtables.

use std::ffi::{CStr, CString};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::abi::{device::Device, in_tracker_add, with_tracker_add};
use crate::gate::{CheckedIndex, FrameGate, FrameOpen};
use crate::pose::Pose;
use crate::receiver::{Inbox, Worker};
use crate::slots::Slots;
use crate::table::{Action, Callback, Table};

const GATE_WARN: Duration = Duration::from_secs(1);

pub(crate) trait HostApi: Send + Sync {
    fn add_tracker(&self, open: &FrameOpen, name: &CStr, device: &Device) -> bool;
    fn update_pose(&self, index: &CheckedIndex, pose: &Pose);
    fn property_container(&self, open: &FrameOpen, index: u32) -> Option<u64>;
    fn write_properties(&self, open: &FrameOpen, container: u64) -> bool;
    fn log(&self, open: &FrameOpen, line: &str);
    fn bridge_port(&self, open: &FrameOpen) -> u16;
}

#[derive(Clone, Copy)]
pub(crate) enum CallbackKind {
    Factory,
    Init,
    RunFrame,
    Activate,
    Deactivate,
    Other,
}

impl CallbackKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Factory => "HmdDriverFactory",
            Self::Init => "Init",
            Self::RunFrame => "RunFrame",
            Self::Activate => "Activate",
            Self::Deactivate => "Deactivate",
            Self::Other => "callback",
        }
    }
}

enum WorkerSlot {
    Empty,
    Running(Worker),
    Joining,
    Joined,
}

struct Callbacks {
    tx: Sender<Callback>,
    rx: Receiver<Callback>,
}

pub(crate) struct Frame {
    pub(crate) gate: FrameGate,
    host: Mutex<Option<Arc<dyn HostApi>>>,
    inbox: Mutex<Option<Arc<Inbox>>>,
    callbacks: Mutex<Option<Callbacks>>,
    table: Mutex<Table>,
    slots: Mutex<Slots<Device>>,
    worker: Mutex<WorkerSlot>,
    worker_changed: Condvar,
    last_frame_thread: Mutex<Option<ThreadId>>,
    frame_active: AtomicBool,
    preserve_allocations: AtomicBool,
    dropped_actions: AtomicU64,
}

impl Frame {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            gate: FrameGate::default(),
            host: Mutex::new(None),
            inbox: Mutex::new(Some(Arc::new(Inbox::default()))),
            callbacks: Mutex::new(Some(Callbacks { tx, rx })),
            table: Mutex::new(Table::default()),
            slots: Mutex::new(Slots::default()),
            worker: Mutex::new(WorkerSlot::Empty),
            worker_changed: Condvar::new(),
            last_frame_thread: Mutex::new(None),
            frame_active: AtomicBool::new(false),
            preserve_allocations: AtomicBool::new(false),
            dropped_actions: AtomicU64::new(0),
        }
    }

    pub(crate) fn init(&self, host: Arc<dyn HostApi>) -> bool {
        let Some(port) = self.gate.if_open(|open| host.bridge_port(open)) else {
            return false;
        };
        let Some(inbox) = lock(&self.inbox).clone() else {
            return false;
        };
        let mut worker = lock(&self.worker);
        if !matches!(*worker, WorkerSlot::Empty) || !self.gate.is_open() {
            return false;
        }
        let started = match Worker::spawn(port, inbox) {
            Ok(started) => started,
            Err(_) => {
                drop(worker);
                self.gate
                    .if_open(|open| host.log(open, "openut: could not start the receiver"));
                return false;
            }
        };
        *lock(&self.host) = Some(host);
        *worker = WorkerSlot::Running(started);
        true
    }

    pub(crate) fn run_frame(&self, now: Instant) {
        let Some(frame) = self.gate.try_pass() else {
            return;
        };
        *lock(&self.last_frame_thread) = Some(thread::current().id());
        self.frame_active.store(true, Ordering::Relaxed);
        let _active = ActiveFrame(&self.frame_active);
        let Some(host) = lock(&self.host).clone() else {
            return;
        };

        let callbacks: Vec<_> = lock(&self.callbacks)
            .as_ref()
            .map(|queue| queue.rx.try_iter().collect())
            .unwrap_or_default();
        let Some(inbox) = lock(&self.inbox).clone() else {
            return;
        };
        let packets = inbox.take_packets();
        let mut logs = inbox.take_logs();
        let mut actions = Vec::new();
        {
            let mut table = lock(&self.table);
            for callback in callbacks {
                table.on_callback(callback, now, &mut actions);
            }
            for received in packets {
                table.on_packet(received.packet, received.received_at, now, &mut actions);
            }
            table.finish_frame(now, &mut actions);
        }

        for action in actions {
            match action {
                Action::Add { slot, name } => {
                    let object = {
                        let mut slots = lock(&self.slots);
                        // Close precedes the Cleanup snapshot of the slots. Any
                        // object allocated here therefore has its gate sealed.
                        if self.gate.is_open() {
                            slots.insert(slot, Device::new(slot))
                        } else {
                            None
                        }
                    };
                    let result = match (CString::new(name), object) {
                        (Ok(name), Some(device)) => frame.if_open(|open| {
                            with_tracker_add(|| host.add_tracker(open, &name, &device))
                        }),
                        _ => None,
                    };
                    if let Some(accepted) = result {
                        if let Some(line) = lock(&self.table).on_added(slot, accepted) {
                            logs.push_back(line);
                        }
                    } else {
                        self.drop_action();
                    }
                }
                Action::Pose { slot, index, pose } => {
                    let device = lock(&self.slots).get(slot);
                    let sent = device
                        .as_ref()
                        .and_then(|device| device.gate.try_pass(index))
                        .and_then(|pass| {
                            pass.call(|index| frame.if_open(|_| host.update_pose(index, &pose)))
                                .flatten()
                        });
                    if sent.is_none() {
                        self.drop_action();
                    }
                }
                Action::Log { message } => logs.push_back(message),
            }
        }
        for line in logs {
            if frame.if_open(|open| host.log(open, &line)).is_none() {
                self.drop_action();
            }
        }
    }

    #[expect(
        clippy::redundant_allocation,
        reason = "the slot owns a Box and the Arc leases it to callbacks"
    )]
    pub(crate) fn device(&self, address: usize) -> Option<Arc<Box<Device>>> {
        lock(&self.slots)
            .iter()
            .find(|device| device.as_raw().addr() == address)
            .cloned()
    }

    pub(crate) fn activate(&self, device: &Device, index: u32) -> bool {
        let Some(host) = lock(&self.host).clone() else {
            return false;
        };
        let container = self
            .guarded_host_call(|open| host.property_container(open, index))
            .flatten();
        let written = container.is_some_and(|container| {
            self.guarded_host_call(|open| host.write_properties(open, container)) == Some(true)
        });
        if !written || !self.gate.is_open() || !device.gate.open(index) {
            if self.gate.is_open() {
                self.enqueue(Callback::ActivationFailed { slot: device.slot });
            }
            self.log_callback(CallbackKind::Activate, Some(index), Duration::ZERO);
            return false;
        }
        device.set_index(index);
        self.enqueue(Callback::Activated {
            slot: device.slot,
            index,
        });
        self.log_callback(CallbackKind::Activate, Some(index), Duration::ZERO);
        self.gate.is_open()
    }

    pub(crate) fn deactivate(&self, device: &Device) {
        let index = device.index();
        let wait = device.gate.close(GATE_WARN, || {
            self.log("openut: waiting for a device call to return");
        });
        self.enqueue(Callback::Deactivated { slot: device.slot });
        self.log_callback(CallbackKind::Deactivate, index, wait);
    }

    pub(crate) fn log(&self, line: &str) {
        let host = lock(&self.host).clone();
        if let Some(host) = host {
            self.guarded_host_call(|open| host.log(open, line));
        }
    }

    pub(crate) fn log_frame_panic(&self, line: &str) {
        // The original frame permit was released during unwinding. A new one
        // prevents parallel Cleanup from passing this host call.
        let Some(frame) = self.gate.try_pass() else {
            return;
        };
        let host = lock(&self.host).clone();
        if let Some(host) = host {
            frame.if_open(|open| host.log(open, line));
        }
    }

    fn guarded_host_call<T>(&self, f: impl FnOnce(&FrameOpen) -> T) -> Option<T> {
        let result = self.gate.if_open(f);
        if result.is_none() {
            self.drop_action();
        }
        result
    }

    fn log_callback(&self, callback: CallbackKind, index: Option<u32>, wait: Duration) {
        let index = index.map_or_else(|| "none".to_owned(), |index| index.to_string());
        self.log(&format!(
            "openut: {} index={} on_frame_thread={} during_add={} wait_ms={}",
            callback.name(),
            index,
            self.on_frame_thread(),
            in_tracker_add(),
            wait.as_millis(),
        ));
    }

    fn enqueue(&self, callback: Callback) {
        if let Some(queue) = lock(&self.callbacks).as_ref() {
            let _ = queue.tx.send(callback);
        }
    }

    fn drop_action(&self) {
        self.dropped_actions.fetch_add(1, Ordering::Relaxed);
    }

    fn on_frame_thread(&self) -> bool {
        lock(&self.last_frame_thread).as_ref() == Some(&thread::current().id())
    }

    pub(crate) fn cleanup(&self, callback: Option<CallbackKind>, directory: Option<&Path>) {
        if callback.is_some() {
            self.preserve_allocations.store(true, Ordering::Release);
        }
        let source = if let Some(callback) = callback {
            CleanupSource::Reentrant(callback)
        } else if self.on_frame_thread() {
            CleanupSource::FrameThread
        } else {
            CleanupSource::OtherThread {
                frame_active: self.frame_active.load(Ordering::Relaxed),
            }
        };

        self.gate.close_now();
        let devices: Vec<_> = lock(&self.slots).iter().cloned().collect();
        for device in &devices {
            device.gate.seal();
        }

        let wait_started = Instant::now();
        self.gate.wait();
        for device in &devices {
            device.gate.wait(None, || {});
        }
        let waited = wait_started.elapsed();

        let join_started = Instant::now();
        self.join_worker();
        let joined = join_started.elapsed();

        let released = !self.preserve_allocations.load(Ordering::Acquire);
        if released {
            lock(&self.slots).clear();
            let inbox = lock(&self.inbox).take();
            let callbacks = lock(&self.callbacks).take();
            let host = lock(&self.host).take();
            drop((devices, inbox, callbacks, host));
            *lock(&self.table) = Table::default();
        }

        if let Some(directory) = directory {
            let record = cleanup_record(SystemTime::now(), source, waited, joined, released);
            let _ = fs::create_dir_all(directory)
                .and_then(|()| fs::write(directory.join("driver-cleanup.txt"), record));
        }
    }

    fn join_worker(&self) {
        let mut slot = lock(&self.worker);
        while matches!(*slot, WorkerSlot::Joining) {
            slot = self
                .worker_changed
                .wait(slot)
                .unwrap_or_else(|error| error.into_inner());
        }
        if matches!(*slot, WorkerSlot::Joined) {
            return;
        }
        let worker = std::mem::replace(&mut *slot, WorkerSlot::Joining);
        drop(slot);
        if let WorkerSlot::Running(worker) = worker {
            worker.stop_and_join();
        }
        *lock(&self.worker) = WorkerSlot::Joined;
        self.worker_changed.notify_all();
    }
}

struct ActiveFrame<'a>(&'a AtomicBool);

impl Drop for ActiveFrame<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

enum CleanupSource {
    Reentrant(CallbackKind),
    FrameThread,
    OtherThread { frame_active: bool },
}

fn cleanup_record(
    time: SystemTime,
    source: CleanupSource,
    waited: Duration,
    joined: Duration,
    released: bool,
) -> String {
    let time = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let source = match source {
        CleanupSource::Reentrant(callback) => format!("reentrant {}", callback.name()),
        CleanupSource::FrameThread => "frame-thread".to_owned(),
        CleanupSource::OtherThread { frame_active } => {
            format!("other-thread frame_active={frame_active}")
        }
    };
    format!(
        "utc_unix_ms={time} source={source} wait_ms={} join_ms={} released={released}\n",
        waited.as_millis(),
        joined.as_millis(),
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests;
