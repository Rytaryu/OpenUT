//! The thread that owns the SteamVR connection.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use openut_space::Pose;
use openut_space::calib::TimedSample;
use openut_space::fingerprint::SpaceFingerprint;

const RECONNECT_INTERVAL: Duration = Duration::from_secs(10);
const EVENT_INTERVAL: Duration = Duration::from_millis(500);
const SAMPLE_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 30);
const EVENT_CAPACITY: usize = 64;
const SAMPLE_CAPACITY: usize = 64;
const OVERLAY_CAPACITY: usize = 16;

/// Safe SteamVR operations performed by a single owning thread.
pub trait Backend {
    /// A failed attempt leaves the backend disconnected.
    fn connect(&mut self) -> Result<(), String>;
    fn disconnect(&mut self);
    fn poll_event(&mut self) -> Option<SteamVrEvent>;
    fn raw_poses(&mut self) -> Vec<TrackedPose>;
    fn fingerprint(&mut self) -> Option<SpaceFingerprint>;
    fn overlay(&mut self, command: OverlayCommand) -> OverlayResult;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackedPose {
    pub device: u32,
    pub class: TrackedDeviceClass,
    pub valid: bool,
    pub running_ok: bool,
    pub pose: Pose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackedDeviceClass {
    Hmd,
    Controller,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SteamVrEvent {
    Quit,
    SpaceChanged,
    Mouse(MouseEvent),
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseEvent {
    pub handle: u64,
    pub kind: MouseEventKind,
    pub x: f32,
    pub y: f32,
    pub button: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseEventKind {
    Move,
    ButtonDown,
    ButtonUp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayHandles {
    pub dashboard: u64,
    pub thumbnail: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OverlayCommand {
    CreateDashboard {
        key: String,
        name: String,
    },
    SetImage {
        handle: u64,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    SetWidth {
        handle: u64,
        meters: f32,
    },
    SetMouseScale {
        handle: u64,
        width: f32,
        height: f32,
    },
    Destroy {
        handle: u64,
    },
}

pub type OverlayResult = Result<Option<OverlayHandles>, String>;

#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    Connected,
    Disconnected,
    ConnectionFailed(String),
    /// Read after connecting, when first available, or after a space-change event.
    Fingerprint(Option<SpaceFingerprint>),
    Mouse(MouseEvent),
}

/// Current runtime state, retained even if the notification queue is full.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClientState {
    pub connected: bool,
    /// Changes on every successful connection.
    pub connection_epoch: u64,
    pub fingerprint: Option<SpaceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerSample {
    pub device: u32,
    pub sample: TimedSample,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SampleBatch {
    pub connection_epoch: u64,
    pub hmd: Option<TimedSample>,
    pub controllers: Vec<ControllerSample>,
}

#[derive(Default)]
struct Control {
    stopping: AtomicBool,
    calibrating: AtomicBool,
    state: Mutex<ClientState>,
}

impl Control {
    fn state(&self) -> ClientState {
        *self.state.lock().expect("SteamVR state lock")
    }
}

struct OverlayRequest {
    connection_epoch: u64,
    command: OverlayCommand,
    reply: SyncSender<OverlayResult>,
}

struct Outbox {
    events: SyncSender<ClientEvent>,
    samples: SyncSender<SampleBatch>,
}

pub struct Client {
    control: Arc<Control>,
    commands: SyncSender<OverlayRequest>,
    events: Receiver<ClientEvent>,
    samples: Receiver<SampleBatch>,
    thread: Option<JoinHandle<()>>,
}

impl Client {
    /// Constructs and operates the backend on the worker. Use the same
    /// monotonic origin as the dongle reader when aligning calibration poses.
    pub fn start<B: Backend>(
        origin: Instant,
        make_backend: impl FnOnce() -> B + Send + 'static,
    ) -> io::Result<Self> {
        let control = Arc::new(Control::default());
        let worker_control = Arc::clone(&control);
        let (commands, requests) = mpsc::sync_channel(OVERLAY_CAPACITY);
        let (events_tx, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let (samples_tx, samples) = mpsc::sync_channel(SAMPLE_CAPACITY);
        let thread = thread::Builder::new()
            .name("openut-steamvr".to_owned())
            .spawn(move || {
                let outbox = Outbox {
                    events: events_tx,
                    samples: samples_tx,
                };
                let mut worker = Worker::new(make_backend(), origin, Instant::now());
                while !worker_control.stopping.load(Ordering::Acquire) {
                    worker.poll(Instant::now(), Instant::now, &worker_control, &outbox);
                    for _ in 0..OVERLAY_CAPACITY {
                        if worker_control.stopping.load(Ordering::Acquire) {
                            break;
                        }
                        let Ok(request) = requests.try_recv() else {
                            break;
                        };
                        worker.overlay(request, &worker_control);
                    }
                    if worker_control.stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let calibrating = worker_control.calibrating.load(Ordering::Acquire);
                    thread::park_timeout(worker.wait(Instant::now(), calibrating));
                }
                worker.disconnect(&worker_control);
            })?;
        Ok(Self {
            control,
            commands,
            events,
            samples,
            thread: Some(thread),
        })
    }

    pub fn set_calibration(&self, enabled: bool) {
        self.control.calibrating.store(enabled, Ordering::Release);
        self.wake();
    }

    /// Submits without blocking and returns any unqueued command to the caller.
    /// The worker rejects any queued command whose connection has changed.
    pub fn overlay(
        &self,
        command: OverlayCommand,
    ) -> Result<Receiver<OverlayResult>, OverlayCommand> {
        let (reply, response) = mpsc::sync_channel(1);
        let request = OverlayRequest {
            connection_epoch: self.state().connection_epoch,
            command,
            reply,
        };
        match self.commands.try_send(request) {
            Ok(()) => {
                self.wake();
                Ok(response)
            }
            Err(mpsc::TrySendError::Full(request) | mpsc::TrySendError::Disconnected(request)) => {
                Err(request.command)
            }
        }
    }

    pub fn events(&self) -> impl Iterator<Item = ClientEvent> + '_ {
        self.events.try_iter()
    }

    pub fn state(&self) -> ClientState {
        self.control.state()
    }

    /// Skips queued samples from earlier connections. Restart calibration
    /// collection when the connection epoch changes.
    pub fn samples(&self) -> impl Iterator<Item = SampleBatch> + '_ {
        self.samples.try_iter().filter(|batch| {
            let state = self.state();
            state.connected && batch.connection_epoch == state.connection_epoch
        })
    }

    pub fn shutdown(&mut self) {
        self.control.stopping.store(true, Ordering::Release);
        self.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn wake(&self) {
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Worker<B: Backend> {
    backend: B,
    origin: Instant,
    connected: bool,
    next_connect: Instant,
    next_event: Instant,
    next_sample: Option<Instant>,
}

impl<B: Backend> Worker<B> {
    fn new(backend: B, origin: Instant, now: Instant) -> Self {
        Self {
            backend,
            origin,
            connected: false,
            next_connect: now,
            next_event: now,
            next_sample: None,
        }
    }

    fn poll(
        &mut self,
        now: Instant,
        clock: impl Fn() -> Instant,
        control: &Control,
        outbox: &Outbox,
    ) {
        let connecting = !self.connected;
        if connecting {
            if now < self.next_connect {
                return;
            }
            match self.backend.connect() {
                Ok(()) => {
                    self.connected = true;
                    self.next_event = clock();
                    if control.stopping.load(Ordering::Acquire) {
                        return;
                    }
                    let fingerprint = self.backend.fingerprint();
                    {
                        let mut state = control.state.lock().expect("SteamVR state lock");
                        *state = ClientState {
                            connected: true,
                            connection_epoch: state.connection_epoch.wrapping_add(1),
                            fingerprint,
                        };
                    }
                    let _ = outbox.events.try_send(ClientEvent::Connected);
                    let _ = outbox
                        .events
                        .try_send(ClientEvent::Fingerprint(fingerprint));
                }
                Err(error) => {
                    self.next_connect = clock() + RECONNECT_INTERVAL;
                    let _ = outbox.events.try_send(ClientEvent::ConnectionFailed(error));
                    return;
                }
            }
        }

        let now = clock();
        if now >= self.next_event {
            self.next_event = now + EVENT_INTERVAL;
            for _ in 0..EVENT_CAPACITY {
                if control.stopping.load(Ordering::Acquire) {
                    return;
                }
                match self.backend.poll_event() {
                    None => break,
                    Some(SteamVrEvent::Quit) => {
                        self.disconnect(control);
                        self.next_connect = clock() + RECONNECT_INTERVAL;
                        let _ = outbox.events.try_send(ClientEvent::Disconnected);
                        return;
                    }
                    Some(SteamVrEvent::SpaceChanged) => {
                        let fingerprint = self.backend.fingerprint();
                        Self::publish_fingerprint(fingerprint, control, outbox);
                    }
                    Some(SteamVrEvent::Mouse(event)) => {
                        let _ = outbox.events.try_send(ClientEvent::Mouse(event));
                    }
                    Some(SteamVrEvent::Other) => {}
                }
            }
            // The HMD may report its universe only after the client connects.
            if !connecting
                && !control.stopping.load(Ordering::Acquire)
                && control.state().fingerprint.is_none()
                && let Some(fingerprint) = self.backend.fingerprint()
            {
                Self::publish_fingerprint(Some(fingerprint), control, outbox);
            }
        }
        if !control.calibrating.load(Ordering::Acquire) {
            self.next_sample = None;
            return;
        }
        let now = clock();
        if self.next_sample.is_none_or(|deadline| now >= deadline)
            && !control.stopping.load(Ordering::Acquire)
        {
            self.next_sample = Some(now + SAMPLE_INTERVAL);
            let poses = self.backend.raw_poses();
            let at = clock().saturating_duration_since(self.origin);
            let mut batch = SampleBatch {
                connection_epoch: control.state().connection_epoch,
                ..SampleBatch::default()
            };
            for pose in poses
                .into_iter()
                .filter(|pose| pose.running_ok && pose.valid)
            {
                let sample = TimedSample {
                    at,
                    pose: pose.pose,
                };
                match pose.class {
                    TrackedDeviceClass::Hmd => batch.hmd = Some(sample),
                    TrackedDeviceClass::Controller => batch.controllers.push(ControllerSample {
                        device: pose.device,
                        sample,
                    }),
                    TrackedDeviceClass::Other => {}
                }
            }
            if (batch.hmd.is_some() || !batch.controllers.is_empty())
                && control.calibrating.load(Ordering::Acquire)
                && !control.stopping.load(Ordering::Acquire)
            {
                let _ = outbox.samples.try_send(batch);
            }
        }
    }

    fn wait(&self, now: Instant, calibrating: bool) -> Duration {
        let deadline = match (self.connected, calibrating) {
            (false, _) => self.next_connect,
            (true, false) => self.next_event,
            (true, true) => self.next_event.min(self.next_sample.unwrap_or(now)),
        };
        deadline.saturating_duration_since(now)
    }

    fn publish_fingerprint(
        fingerprint: Option<SpaceFingerprint>,
        control: &Control,
        outbox: &Outbox,
    ) {
        control
            .state
            .lock()
            .expect("SteamVR state lock")
            .fingerprint = fingerprint;
        let _ = outbox
            .events
            .try_send(ClientEvent::Fingerprint(fingerprint));
    }

    fn overlay(&mut self, request: OverlayRequest, control: &Control) {
        let state = control.state();
        let result = if !state.connected {
            Err("SteamVR に接続していません。".to_owned())
        } else if request.connection_epoch != state.connection_epoch {
            Err("SteamVR の接続が変わりました。".to_owned())
        } else {
            self.backend.overlay(request.command)
        };
        let _ = request.reply.try_send(result);
    }

    fn disconnect(&mut self, control: &Control) {
        if std::mem::take(&mut self.connected) {
            self.next_sample = None;
            {
                let mut state = control.state.lock().expect("SteamVR state lock");
                state.connected = false;
                state.fingerprint = None;
            }
            self.backend.disconnect();
        }
    }
}

impl<B: Backend> Drop for Worker<B> {
    fn drop(&mut self) {
        if self.connected {
            self.backend.disconnect();
        }
    }
}

#[cfg(test)]
mod tests;
