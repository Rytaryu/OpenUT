//! Read-only tracker pipeline and its Windows runtime.

use std::path::PathBuf;

#[cfg(any(windows, test))]
mod capture;
pub mod pipeline;

pub fn run(config_path: Option<PathBuf>) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::start(config_path)
    }
    #[cfg(not(windows))]
    {
        let _ = config_path;
        Err("openut run は Windows で利用できます。".to_owned())
    }
}

#[cfg(windows)]
mod windows {
    use std::collections::hash_map::RandomState;
    use std::env;
    use std::fs::{self, File, OpenOptions};
    use std::hash::BuildHasher;
    use std::io::{self, IsTerminal, Write};
    use std::num::NonZeroU32;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::RecvTimeoutError;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use openut_bridge::{DeviceEntry, Payload};
    use openut_core::time::{LogTime, UtcOffset};
    use openut_dongle::guard::ServerGuard;
    use openut_dongle::process_windows::ProcessList;
    use openut_dongle::run::{Clock, Stamp};
    use openut_dongle::source_windows::DeviceList;

    use super::capture::{CaptureItem, capture};
    use super::pipeline::Pipeline;
    use crate::bridge_out::BridgeOut;
    use crate::config::RunConfig;

    const SERVER_RUNNING: &str =
        "VIVE Hub のトラッカー用サーバーを終了してから OpenUT を起動してください。";
    const GUARD_INTERVAL: Duration = Duration::from_secs(2);
    const IDLE_POLL: Duration = Duration::from_millis(100);
    const CONSOLE_INTERVAL_US: u64 = 1_000_000;
    const LOG_INTERVAL_US: u64 = 10_000_000;

    pub fn start(config_path: Option<PathBuf>) -> Result<(), String> {
        let config_path = config_path.or_else(|| {
            env::var_os("APPDATA").map(|root| PathBuf::from(root).join("OpenUT/config.toml"))
        });
        let mut log = AppLog::open();
        log.write(format_args!("OpenUT を起動します。"));
        let (config, warning) = RunConfig::read(config_path.as_deref());
        if let Some(warning) = warning {
            log.write(format_args!("{}", warning.message()));
        }
        let result = run_with(config, &mut log);
        match &result {
            Ok(()) => log.write(format_args!("OpenUT を終了しました。")),
            Err(reason) => log.write(format_args!("OpenUT を停止しました。{reason}")),
        }
        result
    }

    fn run_with(config: RunConfig, log: &mut AppLog) -> Result<(), String> {
        let guard = ServerGuard::default();
        let mut processes =
            ProcessList::new().map_err(|_| "VIVE Hub の動作を確認できませんでした。".to_owned())?;
        if processes.server_running(&guard) {
            return Err(SERVER_RUNNING.to_owned());
        }
        let mut bridge = BridgeOut::bind(config.port)
            .map_err(|error| format!("SteamVR への送信を準備できません（{}）。", error.kind()))?;
        let devices = DeviceList::new().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        install_console_stop(&stop)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Check after enumerating the HID devices and before opening the handle.
        if processes.server_running(&guard) {
            return Err(SERVER_RUNNING.to_owned());
        }
        let source = devices.open_dongle().map_err(|error| error.to_string())?;
        let runtime_clock = RuntimeClock(Instant::now());

        capture(
            source,
            runtime_clock,
            &stop,
            GUARD_INTERVAL,
            move || processes.server_running(&guard),
            |stream, dropped| {
                log.write(format_args!(
                    "VIVE Hub のトラッカー用サーバーの停止を確認しました。"
                ));
                let mut pipeline =
                    Pipeline::new(session_id(), 0, config.axis_map, config.body_rotation);
                let mut progress = Progress::default();
                let mut console = io::stdout().lock();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break Ok(());
                    }
                    let item = match stream.recv_timeout(IDLE_POLL) {
                        Ok(item) => item,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break Ok(()),
                    };
                    if stop.load(Ordering::Relaxed) {
                        break Ok(());
                    }
                    let (at, packets) = match item {
                        CaptureItem::Report(report) => {
                            let packets = pipeline.report(&report, runtime_clock.mono_us());
                            (report.mono_us, packets)
                        }
                        CaptureItem::Idle(at) => (at.mono_us, pipeline.advance_to(at.mono_us)),
                    };
                    for mut packet in packets {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        match &mut packet.payload {
                            Payload::DeviceList(devices) => progress.announce_devices(devices, log),
                            Payload::Pose(pose) => {
                                let age = runtime_clock.mono_us().saturating_sub(at);
                                pose.age_us = u32::try_from(age).unwrap_or(u32::MAX);
                            }
                        }
                        if !stop.load(Ordering::Relaxed) {
                            bridge.send(&packet);
                        }
                    }
                    if stop.load(Ordering::Relaxed) {
                        break Ok(());
                    }
                    if progress
                        .update(
                            at,
                            &pipeline,
                            &bridge,
                            dropped.load(Ordering::Relaxed),
                            &mut console,
                            log,
                        )
                        .is_err()
                    {
                        break Err("状態をコンソールに書き込めませんでした。".to_owned());
                    }
                }
            },
        )
    }

    fn install_console_stop(stop: &Arc<AtomicBool>) -> Result<(), String> {
        if io::stdin().is_terminal() {
            println!("終了するには Enter キーを押してください。");
            let stop = Arc::clone(stop);
            thread::Builder::new()
                .spawn(move || {
                    let mut input = String::new();
                    if io::stdin().read_line(&mut input).is_ok_and(|n| n > 0) {
                        stop.store(true, Ordering::Relaxed);
                    }
                })
                .map_err(|_| "コンソールから終了する準備ができませんでした。".to_owned())?;
        }
        Ok(())
    }

    #[derive(Clone, Copy)]
    struct RuntimeClock(Instant);

    impl RuntimeClock {
        fn mono_us(self) -> u64 {
            u64::try_from(self.0.elapsed().as_micros()).unwrap_or(u64::MAX)
        }
    }

    impl Clock for RuntimeClock {
        fn now(&mut self) -> Stamp {
            Stamp {
                mono_us: self.mono_us(),
                wall_ms: wall_time().0,
            }
        }
    }

    fn wall_time() -> LogTime {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        LogTime(i64::try_from(timestamp).unwrap_or(i64::MAX))
    }

    fn session_id() -> NonZeroU32 {
        let seed = (std::process::id(), wall_time().0);
        let hash = RandomState::new().hash_one(seed);
        NonZeroU32::new(hash as u32).unwrap_or(NonZeroU32::MIN)
    }

    struct AppLog(Option<File>);

    impl AppLog {
        fn open() -> Self {
            let file = env::var_os("LOCALAPPDATA").and_then(|root| {
                let folder = PathBuf::from(root).join("OpenUT/logs");
                fs::create_dir_all(&folder).ok()?;
                let date = wall_time().to_local(UtcOffset(0)).0;
                let name = format!(
                    "openut-{:04}{:02}{:02}.log",
                    date.year, date.month, date.day
                );
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(folder.join(name))
                    .ok()
            });
            Self(file)
        }

        fn write(&mut self, args: std::fmt::Arguments<'_>) {
            if let Some(file) = &mut self.0 {
                let _ = writeln!(file, "{} UTC {args}", wall_time());
            }
        }
    }

    #[derive(Default)]
    struct Progress {
        devices: usize,
        console_at: u64,
        log_at: u64,
    }

    impl Progress {
        fn announce_devices(&mut self, devices: &[DeviceEntry], log: &mut AppLog) {
            for device in devices.iter().skip(self.devices) {
                log.write(format_args!(
                    "トラッカーを追加しました（番号 {}）。",
                    device.device
                ));
            }
            self.devices = devices.len();
        }

        fn update(
            &mut self,
            at: u64,
            pipeline: &Pipeline,
            bridge: &BridgeOut,
            reader_dropped: u64,
            console: &mut impl Write,
            log: &mut AppLog,
        ) -> io::Result<()> {
            if at < self.console_at && at < self.log_at {
                return Ok(());
            }
            let stats = bridge.stats();
            let pipeline_stats = pipeline.stats();
            let rejected = pipeline_stats
                .malformed_reports
                .saturating_add(pipeline_stats.unknown_slot_poses)
                .saturating_add(pipeline_stats.conflicting_slot_poses);
            let counts = format!(
                "送信 {} 件 / 送信の破棄 {} 件 / 読み取りの破棄 {} 件 / 変換前に除外 {} 件",
                stats.sent, stats.dropped, reader_dropped, rejected,
            );
            if at >= self.console_at {
                self.console_at = at.saturating_add(CONSOLE_INTERVAL_US);
                for tracker in pipeline.status() {
                    writeln!(console, "{tracker}")?;
                }
                writeln!(console, "{counts}")?;
            }
            if at >= self.log_at {
                self.log_at = at.saturating_add(LOG_INTERVAL_US);
                log.write(format_args!("{counts}"));
            }
            Ok(())
        }
    }
}
