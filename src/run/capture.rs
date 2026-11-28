//! Owns the read-only dongle handle independently of the output consumer.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use openut_dongle::record::RawReport;
use openut_dongle::run::{Clock, ReportSource, SourceItem, Stamp, reader_loop};

const BUFFERED_ITEMS: usize = 64;
const SERVER_STARTED: &str = "VIVE Hub が起動したため、ドングルの読み取りを停止しました。";

pub(super) enum CaptureItem {
    Report(RawReport),
    Idle(Stamp),
}

pub(super) fn capture(
    source: impl ReportSource + Send,
    clock: impl Clock + Send,
    stop: &AtomicBool,
    guard_interval: Duration,
    mut server_running: impl FnMut() -> bool + Send,
    consume: impl FnOnce(Receiver<CaptureItem>, &AtomicU64) -> Result<(), String>,
) -> Result<(), String> {
    let dropped = AtomicU64::new(0);
    let reader_dropped = &dropped;
    thread::scope(|scope| {
        let (sender, stream) = mpsc::sync_channel(BUFFERED_ITEMS);
        let reader = thread::Builder::new()
            .spawn_scoped(scope, move || {
                if stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                if server_running() {
                    stop.store(true, Ordering::Relaxed);
                    return Err(SERVER_STARTED.to_owned());
                }

                let mut checked = Instant::now();
                let mut result = Ok(());
                reader_loop(
                    source,
                    clock,
                    |item| {
                        // HID reads time out regularly, even without reports.
                        if checked.elapsed() >= guard_interval {
                            checked = Instant::now();
                            if server_running() {
                                result = Err(SERVER_STARTED.to_owned());
                                stop.store(true, Ordering::Relaxed);
                                return false;
                            }
                        }

                        // A terminal outcome is returned by the worker after
                        // the handle closes, even when the queue is full.
                        let item = match item {
                            SourceItem::Report(report) => CaptureItem::Report(report),
                            SourceItem::Idle(at) => CaptureItem::Idle(at),
                            SourceItem::Removed(_) => {
                                result = Err("ドングルとの接続が切れました。".to_owned());
                                return false;
                            }
                            SourceItem::ReadError { os_code, .. } => {
                                result = Err(format!(
                                    "ドングルの読み取りに失敗しました（OS 番号 {os_code}）。"
                                ));
                                return false;
                            }
                        };
                        match sender.try_send(item) {
                            Ok(()) | Err(TrySendError::Full(CaptureItem::Idle(_))) => true,
                            Err(TrySendError::Full(CaptureItem::Report(_))) => {
                                reader_dropped.fetch_add(1, Ordering::Relaxed);
                                true
                            }
                            Err(TrySendError::Disconnected(_)) => false,
                        }
                    },
                    stop,
                );
                result
            })
            .map_err(|_| "ドングルの読み取りを開始できませんでした。".to_owned())?;
        let request_stop = StopOnDrop(stop);
        let result = consume(stream, &dropped);
        drop(request_stop);
        let read_result = reader
            .join()
            .unwrap_or_else(|_| Err("ドングルの読み取りが中断されました。".to_owned()));
        // Keep the cause of a reader shutdown if the output also fails.
        read_result.and(result)
    })
}

struct StopOnDrop<'a>(&'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use openut_dongle::run::{ReadOutcome, SystemClock};

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(2);

    struct FakeSource {
        fail_after: Option<usize>,
        wait_for_writer: Option<Receiver<()>>,
        reads: Arc<AtomicUsize>,
        closed: mpsc::Sender<()>,
    }

    impl ReportSource for FakeSource {
        fn read_timeout(&mut self, _: Duration) -> ReadOutcome {
            let iteration = self.reads.fetch_add(1, Ordering::Relaxed) + 1;
            if iteration == 2
                && let Some(writer_blocked) = self.wait_for_writer.take()
            {
                writer_blocked
                    .recv_timeout(TIMEOUT)
                    .expect("blocked output");
            }
            if self.fail_after == Some(iteration) {
                return ReadOutcome::ReadError(42);
            }
            if self.fail_after.is_some() {
                ReadOutcome::Report(Vec::new())
            } else {
                thread::sleep(Duration::from_millis(1));
                ReadOutcome::Idle
            }
        }
    }

    impl Drop for FakeSource {
        fn drop(&mut self) {
            let _ = self.closed.send(());
        }
    }

    struct StalledWriter {
        entered: mpsc::Sender<()>,
        release: Receiver<()>,
        reject: bool,
    }

    impl Write for StalledWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.entered.send(()).map_err(io::Error::other)?;
            self.release
                .recv_timeout(TIMEOUT)
                .map_err(io::Error::other)?;
            if self.reject {
                Err(io::Error::other("blocked output closed"))
            } else {
                Ok(bytes.len())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_read_handle_closes_while_output_is_blocked() {
        for fail_after in [None, Some(BUFFERED_ITEMS + 3)] {
            let (entered, writing) = mpsc::channel();
            let (release, released) = mpsc::channel();
            let (burst, writer_blocked) = mpsc::channel();
            let (closed, handle_closed) = mpsc::channel();
            let server_running = Arc::new(AtomicBool::new(false));
            let checks = Arc::new(AtomicUsize::new(0));
            let reads = Arc::new(AtomicUsize::new(0));
            let source = FakeSource {
                fail_after,
                wait_for_writer: fail_after.map(|_| writer_blocked),
                reads: Arc::clone(&reads),
                closed,
            };
            let probe = Arc::clone(&server_running);
            let probe_checks = Arc::clone(&checks);
            let runner = thread::spawn(move || {
                capture(
                    source,
                    SystemClock::new(),
                    &AtomicBool::new(false),
                    Duration::from_millis(20),
                    move || {
                        probe_checks.fetch_add(1, Ordering::Relaxed);
                        probe.load(Ordering::Relaxed)
                    },
                    |stream, _| {
                        let received = stream.recv_timeout(TIMEOUT).expect("first update");
                        let at = match (fail_after, received) {
                            (Some(_), CaptureItem::Report(report)) => report.mono_us,
                            (None, CaptureItem::Idle(at)) => at.mono_us,
                            _ => panic!("unexpected first update"),
                        };
                        StalledWriter {
                            entered,
                            release: released,
                            reject: fail_after.is_none(),
                        }
                        .write_all(format!("progress at {at}").as_bytes())
                        .map_err(|error| error.to_string())
                    },
                )
            });

            writing
                .recv_timeout(TIMEOUT)
                .expect("writer reached the blocked call");
            if fail_after.is_none() {
                server_running.store(true, Ordering::Relaxed);
            } else {
                burst.send(()).unwrap();
            }
            handle_closed
                .recv_timeout(TIMEOUT)
                .expect("read handle closed before output was released");
            assert!(!runner.is_finished());
            if let Some(expected) = fail_after {
                assert_eq!(reads.load(Ordering::Relaxed), expected);
            } else {
                assert!(checks.load(Ordering::Relaxed) >= 2);
            }
            release.send(()).unwrap();
            let error = runner.join().unwrap().unwrap_err();
            match fail_after {
                Some(_) => assert!(error.contains("42")),
                None => assert_eq!(error, SERVER_STARTED),
            }
        }
    }
}
