//! Read-only live capture using one ordered stream of timestamps.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::decode::{Decoder, DongleEvent, TrackerMessage, normalize};
use crate::record::{EndReason, Mode, Note, RawReport, Record, RecordWriter, TrackerLabel};
use crate::tracker::{TrackerTable, Transition};

const READ_TIMEOUT: Duration = Duration::from_millis(100);
const STREAM_TIMEOUT: Duration = Duration::from_secs(1);
const TICK_US: u64 = 1_000_000;
const SERVER_CHECK_US: u64 = 2_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stamp {
    pub mono_us: u64,
    pub wall_ms: i64,
}

pub trait Clock {
    fn now(&mut self) -> Stamp;
}

pub struct SystemClock {
    started: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&mut self) -> Stamp {
        let mono_us = micros(self.started.elapsed());
        let wall_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
            Err(error) => {
                let elapsed = error.duration().as_millis();
                i64::try_from(elapsed).map_or(i64::MIN, |elapsed| -elapsed)
            }
        };
        Stamp { mono_us, wall_ms }
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub trait ReportSource {
    fn read_timeout(&mut self, timeout: Duration) -> ReadOutcome;
}

pub enum ReadOutcome {
    Report(Vec<u8>),
    Idle,
    ReadError(i32),
    Removed,
}

#[derive(Debug)]
pub enum SourceItem {
    Report(RawReport),
    Idle(Stamp),
    ReadError { at: Stamp, os_code: i32 },
    Removed(Stamp),
}

impl SourceItem {
    fn stamp(&self) -> Stamp {
        match self {
            Self::Report(report) => Stamp {
                mono_us: report.mono_us,
                wall_ms: report.wall_ms,
            },
            Self::Idle(at) | Self::ReadError { at, .. } | Self::Removed(at) => *at,
        }
    }

    fn clamp_to_deadline(self, deadline_us: u64) -> Self {
        let at = self.stamp();
        if at.mono_us <= deadline_us {
            return self;
        }

        // Estimate UTC at the monotonic cutoff from the reader's clock sample.
        let excess_ms = (at.mono_us / 1_000 - deadline_us / 1_000) as i64;
        Self::Idle(Stamp {
            mono_us: deadline_us,
            wall_ms: at.wall_ms.saturating_sub(excess_ms),
        })
    }
}

/// Samples the clock after each read, then sends the item without decoding or writing it.
pub fn reader_loop(
    source: impl ReportSource,
    clock: impl Clock,
    emit: impl FnMut(SourceItem) -> bool,
    stop: &AtomicBool,
) {
    reader_loop_until(source, clock, emit, stop, None);
}

fn reader_loop_until(
    mut source: impl ReportSource,
    mut clock: impl Clock,
    mut emit: impl FnMut(SourceItem) -> bool,
    stop: &AtomicBool,
    deadline_us: Option<u64>,
) {
    while !stop.load(Ordering::Relaxed) {
        let timeout = if let Some(deadline_us) = deadline_us {
            let now = clock.now();
            if now.mono_us >= deadline_us {
                emit(SourceItem::Idle(now).clamp_to_deadline(deadline_us));
                break;
            }
            // The HID backend accepts only whole milliseconds.
            READ_TIMEOUT.min(Duration::from_millis(
                (deadline_us - now.mono_us).div_ceil(1_000),
            ))
        } else {
            READ_TIMEOUT
        };
        let received = source.read_timeout(timeout);
        let at = clock.now();
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let item = match received {
            ReadOutcome::Report(bytes) => SourceItem::Report(RawReport {
                mono_us: at.mono_us,
                wall_ms: at.wall_ms,
                bytes,
            }),
            ReadOutcome::Idle => SourceItem::Idle(at),
            ReadOutcome::ReadError(os_code) => SourceItem::ReadError { at, os_code },
            ReadOutcome::Removed => SourceItem::Removed(at),
        };
        let terminal = matches!(item, SourceItem::ReadError { .. } | SourceItem::Removed(_))
            || deadline_us.is_some_and(|deadline_us| at.mono_us >= deadline_us);
        let item = match deadline_us {
            Some(deadline_us) => item.clamp_to_deadline(deadline_us),
            None => item,
        };
        if !emit(item) || terminal {
            break;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    pub start: Stamp,
    pub duration: Duration,
    pub mode: Mode,
}

impl RunOptions {
    fn probe_deadline(self) -> Option<u64> {
        (self.mode == Mode::ProbeSharing)
            .then(|| self.start.mono_us.saturating_add(micros(self.duration)))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunStats {
    pub reports: u64,
    pub malformed: u64,
    pub estimated_lost: u64,
    pub report_types: BTreeMap<u8, u64>,
}

pub struct RunResult {
    pub end: Stamp,
    pub reason: EndReason,
    pub stats: RunStats,
    pub time_reversed: u64,
    pub table: TrackerTable,
}

pub enum Notice<'a> {
    Tick {
        at: Stamp,
        reports_per_second: u64,
        estimated_lost: u64,
        table: &'a TrackerTable,
        decoder: &'a Decoder,
    },
    Transition(Transition),
    SlamError {
        at: Stamp,
        tracker: TrackerLabel,
        code: u32,
    },
    Stalled,
    Window {
        at: Stamp,
        note: Note,
    },
    ProbeServerStopped,
    ProbeOpened,
    ProbeOpenFailed(i32),
}

/// Records a stream after the caller has confirmed the VIVE Hub server is absent.
pub fn run_record<S: ReportSource + Send, C: Clock + Send, W: Write>(
    source: S,
    clock: C,
    writer: &mut RecordWriter<W>,
    start: Stamp,
    duration: Duration,
    server_running: impl FnMut() -> bool,
    display: impl FnMut(Notice<'_>) -> io::Result<()>,
) -> io::Result<RunResult> {
    capture(
        source,
        clock,
        writer,
        RunOptions {
            start,
            duration,
            mode: Mode::Record,
        },
        server_running,
        display,
    )
}

/// Waits through the baseline without opening the device, then captures the probe window.
pub fn run_probe<S: ReportSource + Send, C: Clock + Send, W: Write>(
    writer: &mut RecordWriter<W>,
    mut clock: C,
    duration: Duration,
    wait_baseline: impl FnOnce(Duration),
    server_running: impl FnOnce() -> bool,
    open: impl FnOnce() -> Result<S, i32>,
    mut display: impl FnMut(Notice<'_>) -> io::Result<()>,
) -> io::Result<RunResult> {
    write_window(writer, clock.now(), Note::WindowBaseStart, &mut display)?;
    wait_baseline(duration);
    write_window(writer, clock.now(), Note::WindowBaseEnd, &mut display)?;
    let attempt_start = clock.now();
    if !server_running() {
        let end = clock.now();
        write_note(writer, end, Note::ProbeServerStopped)?;
        display(Notice::ProbeServerStopped)?;
        return finish_without_capture(writer, end, EndReason::ServerNotRunning);
    }
    match open() {
        Ok(source) => {
            let read_start = clock.now();
            capture(
                source,
                clock,
                writer,
                RunOptions {
                    start: read_start,
                    duration,
                    mode: Mode::ProbeSharing,
                },
                || false,
                display,
            )
        }
        Err(code) => {
            // No reader was started, so the clock remains with the probe setup.
            let end = clock.now();
            write_window(writer, attempt_start, Note::WindowProbeStart, &mut display)?;
            write_note(writer, end, Note::OpenFailed(code))?;
            display(Notice::ProbeOpenFailed(code))?;
            write_window(writer, end, Note::WindowProbeEnd, &mut display)?;
            finish_without_capture(writer, end, EndReason::ReadError)
        }
    }
}

fn finish_without_capture<W: Write>(
    writer: &mut RecordWriter<W>,
    end: Stamp,
    reason: EndReason,
) -> io::Result<RunResult> {
    let result = RunResult {
        reason,
        end,
        stats: RunStats::default(),
        time_reversed: 0,
        table: TrackerTable::default(),
    };
    write_footer(writer, &result)?;
    Ok(result)
}

fn capture<S: ReportSource + Send, C: Clock + Send, W: Write>(
    source: S,
    clock: C,
    writer: &mut RecordWriter<W>,
    settings: RunOptions,
    server_running: impl FnMut() -> bool,
    mut display: impl FnMut(Notice<'_>) -> io::Result<()>,
) -> io::Result<RunResult> {
    let stop = AtomicBool::new(false);
    thread::scope(|scope| {
        let (send, stream) = mpsc::channel();
        let read_stop = &stop;
        let reader = thread::Builder::new().spawn_scoped(scope, move || {
            reader_loop_until(
                source,
                clock,
                |item| send.send(item).is_ok(),
                read_stop,
                settings.probe_deadline(),
            );
        })?;
        let result = (|| {
            if settings.mode == Mode::ProbeSharing {
                // The reader owns the timed device access while startup output
                // is written. Queued reports are consumed after the marker.
                write_window(writer, settings.start, Note::WindowProbeStart, &mut display)?;
                display(Notice::ProbeOpened)?;
            }
            main_loop(&stream, &stop, writer, settings, server_running, display)
        })();
        stop.store(true, Ordering::Relaxed);
        drop(stream);
        let joined = reader.join();
        result.and_then(|result| {
            joined.map_err(|_| io::Error::other("reader thread stopped"))?;
            Ok(result)
        })
    })
}

/// Processes the reader's timestamps in receive order. It has no clock of its own.
/// The producer should close its sender when `stop` is set.
pub fn main_loop<W: Write>(
    stream: &Receiver<SourceItem>,
    stop: &AtomicBool,
    writer: &mut RecordWriter<W>,
    settings: RunOptions,
    mut server_running: impl FnMut() -> bool,
    mut display: impl FnMut(Notice<'_>) -> io::Result<()>,
) -> io::Result<RunResult> {
    let mut capture = Capture::new(settings.start, writer, &mut display);
    let mut checked_at = settings.start.mono_us;
    let duration_us = micros(settings.duration);
    let guard_enabled = settings.mode == Mode::Record;
    let probe_deadline = settings.probe_deadline();
    let (reason, note) = loop {
        match stream.recv_timeout(STREAM_TIMEOUT) {
            Ok(item) => {
                let item = match probe_deadline {
                    Some(deadline_us) => item.clamp_to_deadline(deadline_us),
                    None => item,
                };
                if let Some(end) = capture.apply(item)? {
                    break end;
                }
                let now = capture.last.mono_us;
                if guard_enabled && now - checked_at >= SERVER_CHECK_US {
                    checked_at = now;
                    if server_running() {
                        break (EndReason::ServerAppeared, Note::ServerAppeared);
                    }
                }
                let reached_duration = match probe_deadline {
                    Some(deadline_us) => now >= deadline_us,
                    None => now - settings.start.mono_us >= duration_us,
                };
                if reached_duration {
                    break (EndReason::Duration, Note::Duration);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                (capture.display)(Notice::Stalled)?;
                if guard_enabled && server_running() {
                    break (EndReason::ServerAppeared, Note::ServerAppeared);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                break (EndReason::ReadError, Note::ReadError(0));
            }
        }
    };

    stop.store(true, Ordering::Relaxed);
    write_note(capture.writer, capture.last, note)?;
    if probe_deadline.is_some() {
        write_window(
            capture.writer,
            capture.last,
            Note::WindowProbeEnd,
            capture.display,
        )?;
    }

    let result = RunResult {
        end: capture.last,
        time_reversed: capture.table.time_reversed(),
        reason,
        stats: capture.stats,
        table: capture.table,
    };
    write_footer(capture.writer, &result)?;
    Ok(result)
}

struct Capture<'a, W, F> {
    writer: &'a mut RecordWriter<W>,
    display: &'a mut F,
    decoder: Decoder,
    table: TrackerTable,
    stats: RunStats,
    last: Stamp,
    last_tick: u64,
    reports_at_tick: u64,
    sequence: Option<u16>,
}

impl<W: Write, F: FnMut(Notice<'_>) -> io::Result<()>> Capture<'_, W, F> {
    fn new<'a>(
        start: Stamp,
        writer: &'a mut RecordWriter<W>,
        display: &'a mut F,
    ) -> Capture<'a, W, F> {
        let mut table = TrackerTable::default();
        table.advance(start.mono_us);
        Capture {
            writer,
            display,
            decoder: Decoder::default(),
            table,
            stats: RunStats::default(),
            last: start,
            last_tick: start.mono_us,
            reports_at_tick: 0,
            sequence: None,
        }
    }

    fn apply(&mut self, item: SourceItem) -> io::Result<Option<(EndReason, Note)>> {
        let at = item.stamp();
        self.last = Stamp {
            mono_us: self.last.mono_us.max(at.mono_us),
            wall_ms: at.wall_ms,
        };
        let (changes, ending) = match item {
            SourceItem::Report(mut report) => {
                let event = self.decoder.decode(&report.bytes);
                let header_label = if matches!(event, DongleEvent::Malformed { .. }) {
                    self.decoder.tracker_data_label(&report.bytes)
                } else {
                    None
                };
                self.count(&report.bytes, &event);
                report.mono_us = self.last.mono_us;
                self.writer.write_record(&Record::Input(report))?;
                let changes = match header_label {
                    Some(label) => self.table.receive_omitted(at.mono_us, label),
                    None => self.table.apply(at.mono_us, &event),
                };
                if let DongleEvent::Text {
                    tracker,
                    message: TrackerMessage::SlamError { code },
                } = event
                {
                    (self.display)(Notice::SlamError {
                        at: self.last,
                        tracker,
                        code,
                    })?;
                }
                (changes, None)
            }
            SourceItem::Idle(_) => (self.table.advance(at.mono_us), None),
            SourceItem::ReadError { os_code, .. } => (
                self.table.advance(at.mono_us),
                Some((EndReason::ReadError, Note::ReadError(os_code))),
            ),
            SourceItem::Removed(_) => (
                self.table.advance(at.mono_us),
                Some((EndReason::DeviceRemoved, Note::DeviceRemoved)),
            ),
        };
        for change in changes {
            (self.display)(Notice::Transition(change))?;
        }
        if ending.is_none() {
            self.write_tick()?;
        }
        Ok(ending)
    }

    fn count(&mut self, bytes: &[u8], event: &DongleEvent) {
        self.stats.reports += 1;
        if matches!(event, DongleEvent::Malformed { .. }) {
            self.stats.malformed += 1;
        }
        let raw = normalize(bytes);
        let Some(&kind) = raw.first() else { return };
        *self.stats.report_types.entry(kind).or_default() += 1;
        if let [0x28, low, high, ..] = raw {
            let sequence = u16::from_le_bytes([*low, *high]);
            if let Some(previous) = self.sequence {
                let distance = sequence.wrapping_sub(previous);
                if distance >= 0x8000 {
                    // Do not advance the baseline on an apparent older report.
                    return;
                }
                self.stats.estimated_lost += u64::from(distance.saturating_sub(1));
            }
            self.sequence = Some(sequence);
        }
    }

    fn write_tick(&mut self) -> io::Result<()> {
        let elapsed = self.last.mono_us - self.last_tick;
        if elapsed < TICK_US {
            return Ok(());
        }
        self.writer.write_record(&Record::Tick {
            mono_us: self.last.mono_us,
            wall_ms: self.last.wall_ms,
        })?;
        self.writer.flush()?;
        let count = self.stats.reports - self.reports_at_tick;
        let rate = (u128::from(count) * u128::from(TICK_US) + u128::from(elapsed / 2))
            / u128::from(elapsed);
        (self.display)(Notice::Tick {
            at: self.last,
            reports_per_second: u64::try_from(rate).unwrap_or(u64::MAX),
            estimated_lost: self.stats.estimated_lost,
            table: &self.table,
            decoder: &self.decoder,
        })?;
        self.last_tick = self.last.mono_us;
        self.reports_at_tick = self.stats.reports;
        Ok(())
    }
}

fn write_note<W: Write>(writer: &mut RecordWriter<W>, at: Stamp, text: Note) -> io::Result<()> {
    writer.write_record(&Record::Note {
        mono_us: at.mono_us,
        wall_ms: at.wall_ms,
        text,
    })
}

fn write_window<W: Write>(
    writer: &mut RecordWriter<W>,
    at: Stamp,
    note: Note,
    display: &mut impl FnMut(Notice<'_>) -> io::Result<()>,
) -> io::Result<()> {
    write_note(writer, at, note)?;
    writer.flush()?;
    display(Notice::Window { at, note })
}

fn write_footer<W: Write>(writer: &mut RecordWriter<W>, result: &RunResult) -> io::Result<()> {
    writer.write_record(&Record::Footer {
        mono_us: result.end.mono_us,
        wall_ms: Some(result.end.wall_ms),
        reports: result.stats.reports,
        malformed: result.stats.malformed,
        estimated_lost: result.stats.estimated_lost,
        reason: result.reason,
        time_reversed: result.time_reversed,
    })?;
    writer.flush()
}
