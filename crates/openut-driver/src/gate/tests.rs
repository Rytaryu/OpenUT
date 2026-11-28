use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, channel};

use super::*;

const LIMIT: Duration = Duration::from_secs(3);

#[test]
fn a_permit_checks_the_index_and_is_returned_on_unwind() {
    let gate = Gate::default();
    assert!(gate.try_pass(7).is_none());
    assert!(gate.open(7));
    assert!(gate.try_pass(6).is_none());
    let held = gate.try_pass(7).unwrap();
    assert!(gate.try_pass(7).is_none());
    assert_eq!(held.call(CheckedIndex::get), Some(7));

    let panic = catch_unwind(AssertUnwindSafe(|| {
        gate.try_pass(7).unwrap().call(|_| panic!("mock host"));
    }));
    assert!(panic.is_err());
    assert_eq!(gate.try_pass(7).unwrap().call(CheckedIndex::get), Some(7));
}

#[test]
fn a_close_on_the_holder_thread_never_waits_and_old_passes_stay_closed() {
    let gate = Gate::default();
    assert!(gate.open(7));
    assert_eq!(
        gate.try_pass(7)
            .unwrap()
            .call(|_| gate.close(Duration::ZERO, || panic!("waited"))),
        Some(Duration::ZERO)
    );
    assert!(gate.try_pass(7).is_none());

    assert!(gate.open(7));
    let held = gate.try_pass(7).unwrap();
    assert_eq!(
        gate.close(Duration::ZERO, || panic!("waited")),
        Duration::ZERO
    );
    assert!(gate.open(7));
    assert_eq!(held.call(|_| panic!("obsolete permit")), None::<()>);
    assert!(gate.open(9));
    assert!(gate.try_pass(7).is_none());
    assert_eq!(gate.try_pass(9).unwrap().call(CheckedIndex::get), Some(9));
    gate.seal();
    assert!(!gate.open(9));
    assert!(gate.try_pass(9).is_none());
}

#[test]
fn close_warns_once_without_a_lock_and_waits_for_a_host_call() {
    let gate = Arc::new(Gate::default());
    gate.open(7);
    let (entered, in_host) = channel();
    let (release, released) = channel();
    let posing_gate = Arc::clone(&gate);
    let pose = thread::spawn(move || {
        posing_gate.try_pass(7).unwrap().call(|index| {
            assert_eq!(index.get(), 7);
            entered.send(()).unwrap();
            released.recv_timeout(LIMIT).unwrap();
        })
    });
    in_host.recv_timeout(LIMIT).unwrap();
    assert!(gate.try_pass(7).is_none());

    let (warning, warned) = channel();
    let (finished, done) = channel();
    let closing_gate = Arc::clone(&gate);
    let close = thread::spawn(move || {
        let elapsed = closing_gate.close(Duration::from_millis(50), || {
            assert!(closing_gate.inner.try_state().is_some());
            warning.send(()).unwrap();
        });
        finished.send(elapsed).unwrap();
    });
    warned.recv_timeout(LIMIT).unwrap();
    assert_eq!(
        done.recv_timeout(Duration::from_millis(450)),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(gate.try_pass(7).is_none());
    release.send(()).unwrap();
    assert_eq!(pose.join().unwrap(), Some(()));
    assert!(done.recv_timeout(LIMIT).unwrap() >= Duration::from_millis(500));
    close.join().unwrap();
    assert!(warned.try_recv().is_err());
}

#[test]
fn a_permit_checked_after_close_skips_the_host_and_unblocks_the_closer() {
    let gate = Arc::new(Gate::default());
    gate.open(7);
    let (issued, received) = channel();
    let (release, released) = channel();
    let posing_gate = Arc::clone(&gate);
    let pose = thread::spawn(move || {
        let pass = posing_gate.try_pass(7).unwrap();
        issued.send(()).unwrap();
        released.recv_timeout(LIMIT).unwrap();
        pass.call(|_| panic!("host must not run"))
    });
    received.recv_timeout(LIMIT).unwrap();

    let (warning, warned) = channel();
    let (finished, done) = channel();
    let closing_gate = Arc::clone(&gate);
    let close = thread::spawn(move || {
        let elapsed = closing_gate.close(Duration::from_millis(50), || warning.send(()).unwrap());
        finished.send(elapsed).unwrap();
    });
    warned.recv_timeout(LIMIT).unwrap();
    assert_eq!(
        done.recv_timeout(Duration::from_millis(150)),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(gate.try_pass(7).is_none());
    release.send(()).unwrap();
    assert_eq!(pose.join().unwrap(), None::<()>);
    assert!(done.recv_timeout(LIMIT).unwrap() >= Duration::from_millis(200));
    close.join().unwrap();
}

#[test]
fn frame_reentry_shuts_every_later_call_without_waiting() {
    let gate = FrameGate::default();
    let frame = gate.try_pass().unwrap();
    assert!(gate.try_pass().is_none());
    assert_eq!(frame.if_open(|_| gate.close()), Some(Duration::ZERO));
    assert_eq!(frame.if_open(|_| panic!("closed frame")), None::<()>);
    assert_eq!(gate.if_open(|_| panic!("closed provider")), None::<()>);
    assert!(!gate.is_open());
    drop(frame);
    assert!(gate.try_pass().is_none());
}

#[test]
fn poisoned_mutex_state_recovers_and_run_frame_can_decline_contention() {
    let gate = Gate::default();
    gate.open(7);
    let poison = catch_unwind(AssertUnwindSafe(|| {
        let _state = gate.inner.state();
        panic!("poison test");
    }));
    assert!(poison.is_err());
    let state = gate.inner.state();
    assert!(gate.try_pass(7).is_none());
    drop(state);
    assert_eq!(gate.try_pass(7).unwrap().call(CheckedIndex::get), Some(7));
}
