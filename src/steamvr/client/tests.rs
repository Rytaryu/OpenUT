use std::collections::VecDeque;
use std::rc::Rc;

use openut_space::{Quat, Vec3};

use super::*;

const TEST_TIMEOUT: Duration = Duration::from_secs(3);
const POSE: Pose = Pose {
    position: Vec3::ZERO,
    rotation: Quat::IDENTITY,
};

#[derive(Default)]
struct MockBackend {
    failures: usize,
    connects: usize,
    disconnects: usize,
    pose_reads: usize,
    pending: VecDeque<SteamVrEvent>,
    poses: Vec<TrackedPose>,
    fingerprint: Option<SpaceFingerprint>,
}

impl Backend for MockBackend {
    fn connect(&mut self) -> Result<(), String> {
        self.connects += 1;
        if self.failures > 0 {
            self.failures -= 1;
            Err("unavailable".to_owned())
        } else {
            Ok(())
        }
    }

    fn disconnect(&mut self) {
        self.disconnects += 1;
    }

    fn poll_event(&mut self) -> Option<SteamVrEvent> {
        self.pending.pop_front()
    }

    fn raw_poses(&mut self) -> Vec<TrackedPose> {
        self.pose_reads += 1;
        self.poses.clone()
    }

    fn fingerprint(&mut self) -> Option<SpaceFingerprint> {
        self.fingerprint
    }

    fn overlay(&mut self, command: OverlayCommand) -> OverlayResult {
        assert!(matches!(command, OverlayCommand::CreateDashboard { .. }));
        Ok(Some(OverlayHandles {
            dashboard: 1,
            thumbnail: 2,
        }))
    }
}

fn outbox(capacity: usize) -> (Outbox, Receiver<ClientEvent>, Receiver<SampleBatch>) {
    let (events, event_rx) = mpsc::sync_channel(capacity);
    let (samples, sample_rx) = mpsc::sync_channel(capacity);
    (Outbox { events, samples }, event_rx, sample_rx)
}

fn poll(worker: &mut Worker<MockBackend>, now: Instant, control: &Control, outbox: &Outbox) {
    worker.poll(now, || now, control, outbox);
}

#[test]
fn retries_unavailable_runtime_every_ten_seconds() {
    let now = Instant::now();
    let origin = now - Duration::from_secs(1);
    let control = Control::default();
    let (outbox, events, _samples) = outbox(EVENT_CAPACITY);
    let mut worker = Worker::new(
        MockBackend {
            failures: 2,
            ..Default::default()
        },
        origin,
        now,
    );

    for attempt in 0..2 {
        let at = now + RECONNECT_INTERVAL * attempt;
        poll(&mut worker, at, &control, &outbox);
        assert_eq!(
            events.try_recv(),
            Ok(ClientEvent::ConnectionFailed("unavailable".to_owned()))
        );
        assert_eq!(worker.wait(at, false), RECONNECT_INTERVAL);
        poll(
            &mut worker,
            at + Duration::from_millis(9_999),
            &control,
            &outbox,
        );
        assert!(events.try_recv().is_err());
        assert_eq!(worker.backend.connects, attempt as usize + 1);
    }

    let at = now + RECONNECT_INTERVAL * 2;
    poll(&mut worker, at, &control, &outbox);
    assert_eq!(
        events.try_iter().collect::<Vec<_>>(),
        [ClientEvent::Connected, ClientEvent::Fingerprint(None)]
    );
    assert_eq!(worker.wait(at, false), EVENT_INTERVAL);
    assert_eq!(worker.backend.connects, 3);
    assert_eq!(worker.backend.pose_reads, 0);
}

#[test]
fn calibration_reads_only_valid_running_hmd_and_controllers_at_30_hz() {
    let origin = Instant::now();
    let now = origin + Duration::from_secs(1);
    let control = Control::default();
    let (outbox, _events, samples) = outbox(EVENT_CAPACITY);
    let controller = TrackedPose {
        device: 2,
        class: TrackedDeviceClass::Controller,
        valid: true,
        running_ok: true,
        pose: POSE,
    };
    let hmd = Pose {
        position: Vec3::new(1.0, 2.0, 3.0),
        ..POSE
    };
    let backend = MockBackend {
        poses: vec![
            TrackedPose {
                device: 0,
                class: TrackedDeviceClass::Hmd,
                pose: hmd,
                ..controller
            },
            controller,
            TrackedPose {
                device: 3,
                valid: false,
                ..controller
            },
            TrackedPose {
                device: 4,
                running_ok: false,
                ..controller
            },
            TrackedPose {
                device: 5,
                class: TrackedDeviceClass::Other,
                ..controller
            },
        ],
        ..Default::default()
    };
    let mut worker = Worker::new(backend, origin, now);

    poll(&mut worker, now, &control, &outbox);
    assert_eq!(worker.backend.pose_reads, 0);
    control.calibrating.store(true, Ordering::Release);
    poll(&mut worker, now, &control, &outbox);
    let at = now.duration_since(origin);
    let sample = ControllerSample {
        device: controller.device,
        sample: TimedSample { at, pose: POSE },
    };
    assert_eq!(
        samples.try_recv(),
        Ok(SampleBatch {
            connection_epoch: 1,
            hmd: Some(TimedSample { at, pose: hmd }),
            controllers: vec![sample]
        })
    );

    assert_eq!(worker.wait(now, true), SAMPLE_INTERVAL);
    poll(
        &mut worker,
        now + SAMPLE_INTERVAL - Duration::from_nanos(1),
        &control,
        &outbox,
    );
    assert_eq!(worker.backend.pose_reads, 1);
    poll(&mut worker, now + SAMPLE_INTERVAL, &control, &outbox);
    assert_eq!(worker.backend.pose_reads, 2);
    assert_eq!(
        samples.try_recv().unwrap().controllers[0].sample.at,
        at + SAMPLE_INTERVAL
    );
    control.calibrating.store(false, Ordering::Release);
    poll(&mut worker, now + EVENT_INTERVAL, &control, &outbox);
    assert_eq!(worker.backend.pose_reads, 2);
    assert!(samples.try_recv().is_err());
}

#[test]
fn polls_space_changes_and_quit_even_when_notifications_are_full() {
    let now = Instant::now();
    let control = Control::default();
    let (outbox, events, _samples) = outbox(1);
    let initial = SpaceFingerprint::new(1, POSE, None);
    let changed = SpaceFingerprint::new(2, POSE, Some([2.0, 3.0]));
    let backend = MockBackend {
        fingerprint: initial,
        ..Default::default()
    };
    let mut worker = Worker::new(backend, now, now);
    poll(&mut worker, now, &control, &outbox);
    let state = || *control.state.lock().unwrap();
    assert_eq!(
        state(),
        ClientState {
            connected: true,
            connection_epoch: 1,
            fingerprint: initial
        }
    );

    worker.backend.fingerprint = changed;
    worker.backend.pending.push_back(SteamVrEvent::SpaceChanged);
    poll(
        &mut worker,
        now + Duration::from_millis(499),
        &control,
        &outbox,
    );
    assert_eq!(state().fingerprint, initial);
    poll(&mut worker, now + EVENT_INTERVAL, &control, &outbox);
    assert_eq!(state().fingerprint, changed);

    worker.backend.pending.push_back(SteamVrEvent::Quit);
    let quit_at = now + EVENT_INTERVAL * 2;
    poll(&mut worker, quit_at, &control, &outbox);
    assert_eq!(worker.backend.disconnects, 1);
    assert_eq!(
        state(),
        ClientState {
            connected: false,
            connection_epoch: 1,
            fingerprint: None
        }
    );
    poll(
        &mut worker,
        quit_at + RECONNECT_INTERVAL - Duration::from_millis(1),
        &control,
        &outbox,
    );
    assert_eq!(worker.backend.connects, 1);
    poll(&mut worker, quit_at + RECONNECT_INTERVAL, &control, &outbox);
    assert_eq!(
        state(),
        ClientState {
            connected: true,
            connection_epoch: 2,
            fingerprint: changed
        }
    );
    assert_eq!(
        events.try_iter().collect::<Vec<_>>(),
        [ClientEvent::Connected]
    );
}

#[test]
fn retries_missing_fingerprint_without_repeated_notifications() {
    let now = Instant::now();
    let control = Control::default();
    let (outbox, events, _samples) = outbox(EVENT_CAPACITY);
    let mut worker = Worker::new(MockBackend::default(), now, now);
    poll(&mut worker, now, &control, &outbox);
    assert_eq!(events.try_recv(), Ok(ClientEvent::Connected));
    assert_eq!(events.try_recv(), Ok(ClientEvent::Fingerprint(None)));

    poll(&mut worker, now + EVENT_INTERVAL, &control, &outbox);
    assert!(events.try_recv().is_err());
    let fingerprint = SpaceFingerprint::new(1, POSE, None);
    worker.backend.fingerprint = fingerprint;
    poll(&mut worker, now + EVENT_INTERVAL * 2, &control, &outbox);
    assert_eq!(control.state().fingerprint, fingerprint);
    assert_eq!(events.try_recv(), Ok(ClientEvent::Fingerprint(fingerprint)));
    poll(&mut worker, now + EVENT_INTERVAL * 3, &control, &outbox);
    assert!(events.try_recv().is_err());
}

#[test]
fn reconnect_discards_samples_and_commands_from_previous_connection() {
    let now = Instant::now();
    let (outbox, events, samples) = outbox(EVENT_CAPACITY);
    let (commands, requests) = mpsc::sync_channel(OVERLAY_CAPACITY);
    let client = Client {
        control: Arc::new(Control::default()),
        commands,
        events,
        samples,
        thread: None,
    };
    let backend = MockBackend {
        poses: vec![TrackedPose {
            device: 2,
            class: TrackedDeviceClass::Controller,
            valid: true,
            running_ok: true,
            pose: POSE,
        }],
        ..Default::default()
    };
    let mut worker = Worker::new(backend, now, now);
    client.set_calibration(true);
    poll(&mut worker, now, &client.control, &outbox);
    assert_eq!(worker.backend.pose_reads, 1);
    let stale_response = client
        .overlay(OverlayCommand::Destroy { handle: 1 })
        .unwrap();

    worker.backend.pending.push_back(SteamVrEvent::Quit);
    poll(&mut worker, now + EVENT_INTERVAL, &client.control, &outbox);
    let reconnect_at = now + EVENT_INTERVAL + RECONNECT_INTERVAL;
    poll(&mut worker, reconnect_at, &client.control, &outbox);
    assert_eq!(worker.backend.pose_reads, 2);
    assert_eq!(client.state().connection_epoch, 2);

    let mut batches = client.samples();
    let batch = batches.next().unwrap();
    assert_eq!(batch.connection_epoch, 2);
    assert_eq!(
        batch.controllers[0].sample.at,
        reconnect_at.duration_since(now)
    );
    assert!(batches.next().is_none());

    worker.overlay(requests.try_recv().unwrap(), &client.control);
    assert_eq!(
        stale_response.try_recv(),
        Ok(Err("SteamVR の接続が変わりました。".to_owned()))
    );
}

struct ThreadBackend {
    _local_only: Rc<()>,
    owner: thread::ThreadId,
    entered: SyncSender<thread::ThreadId>,
    release: Receiver<()>,
    disconnected: SyncSender<thread::ThreadId>,
}

impl Backend for ThreadBackend {
    fn connect(&mut self) -> Result<(), String> {
        self.entered.send(thread::current().id()).unwrap();
        self.release
            .recv_timeout(TEST_TIMEOUT)
            .map_err(|error| error.to_string())
    }

    fn disconnect(&mut self) {
        assert_eq!(thread::current().id(), self.owner);
        self.disconnected.send(thread::current().id()).unwrap();
    }

    fn poll_event(&mut self) -> Option<SteamVrEvent> {
        panic!("stopping must prevent event calls");
    }

    fn raw_poses(&mut self) -> Vec<TrackedPose> {
        panic!("stopping must prevent pose calls");
    }

    fn fingerprint(&mut self) -> Option<SpaceFingerprint> {
        panic!("stopping must prevent fingerprint calls");
    }

    fn overlay(&mut self, _command: OverlayCommand) -> OverlayResult {
        panic!("stopping must discard pending commands");
    }
}

#[test]
fn shutdown_cancels_queued_commands_and_drops_connection_on_owning_thread() {
    let (entered, is_entered) = mpsc::sync_channel(1);
    let (go, release) = mpsc::sync_channel(1);
    let (disconnected, was_disconnected) = mpsc::sync_channel(1);
    let mut client = Client::start(Instant::now(), move || ThreadBackend {
        _local_only: Rc::new(()),
        owner: thread::current().id(),
        entered,
        release,
        disconnected,
    })
    .unwrap();
    let owner = is_entered.recv_timeout(TEST_TIMEOUT).unwrap();
    assert_ne!(owner, thread::current().id());

    let command = OverlayCommand::Destroy { handle: 1 };
    let responses: Vec<_> = (0..OVERLAY_CAPACITY)
        .map(|_| client.overlay(command.clone()).unwrap())
        .collect();
    assert_eq!(client.overlay(command.clone()).unwrap_err(), command);
    let control = Arc::clone(&client.control);
    let (done, finished) = mpsc::sync_channel(1);
    let shutdown = thread::spawn(move || {
        client.shutdown();
        done.send(()).unwrap();
    });
    let until = Instant::now() + TEST_TIMEOUT;
    while !control.stopping.load(Ordering::Acquire) {
        assert!(Instant::now() < until, "shutdown did not publish stop");
        thread::yield_now();
    }
    go.send(()).unwrap();
    finished.recv_timeout(TEST_TIMEOUT).unwrap();
    shutdown.join().unwrap();
    assert_eq!(was_disconnected.try_recv(), Ok(owner));
    for response in responses {
        assert_eq!(response.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }
}

#[test]
fn overlay_requests_wake_a_connected_worker_and_return_the_result() {
    let mut client = Client::start(Instant::now(), MockBackend::default).unwrap();
    assert_eq!(
        client.events.recv_timeout(TEST_TIMEOUT),
        Ok(ClientEvent::Connected)
    );
    assert!(client.state().connected);
    let response = client
        .overlay(OverlayCommand::CreateDashboard {
            key: "openut.test".to_owned(),
            name: "OpenUT test".to_owned(),
        })
        .unwrap();
    assert_eq!(
        response.recv_timeout(TEST_TIMEOUT),
        Ok(Ok(Some(OverlayHandles {
            dashboard: 1,
            thumbnail: 2
        })))
    );
    client.shutdown();
    assert!(!client.state().connected);
}
