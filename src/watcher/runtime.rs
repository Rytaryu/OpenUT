//! Filesystem, console, and wall-time adapters for the watcher.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openut_core::display::{DisplayModel, Line, WatchHealth};
use openut_core::engine::Output;
use openut_core::summary::Summary;
use openut_core::time::{LogDate, LogTime, UtcOffset};

use crate::config::WatchConfig;
use crate::discover;
use crate::hub_process::HubProcess;
use crate::notify::{Console, NoticeSink};
use crate::paths::AppPaths;
use crate::source_fs::FsLogSource;
use crate::summary_io::SummaryWriter;

use super::{ProcessCheck, SummaryWrite, WatchRound, Watcher};

pub fn watch(
    log_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    data_dir: Option<PathBuf>,
) -> Result<(), String> {
    let paths = AppPaths::resolve(config_path.as_deref(), data_dir.as_deref());
    let (config, warning) = WatchConfig::read(paths.config.as_deref());
    let root = discover::log_dir(log_dir.as_deref(), config.log_dir.as_deref());
    if let Some(input) = root
        .as_deref()
        .or(log_dir.as_deref())
        .or(config.log_dir.as_deref())
    {
        check_output_paths(&paths, input)?;
    }
    let mut log = AppLog::new(paths.app_log_dir.clone(), wall_time());
    if let Some(warning) = warning {
        log.write(wall_time(), warning.message());
    }

    let poll = Duration::from_millis(config.poll_ms);
    let mut source = FsLogSource::from_optional(root);
    let mut summaries = paths.summary_dir.clone().map(SummaryPersistence::new);
    let mut process = HubProcess;
    let mut check = |names: &[String]| process.check(names);
    let mut console =
        Console::new(io::stdout()).with_tracking_wait_s(config.engine.rules.r5.wait_s);
    let mut previous_line = None;
    let configured = config.log_dir.clone();
    let mut watcher = Watcher::new(config);
    let stop = console_stop()?;
    drive(poll, wall_time, thread::sleep, |now| {
        if stop.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !source.has_root()
            && let Some(input) = discover::log_dir(log_dir.as_deref(), configured.as_deref())
        {
            check_output_paths(&paths, &input)?;
            source = FsLogSource::new(input);
        }
        let process = if cfg!(windows) {
            Some(&mut check as &mut ProcessCheck<'_>)
        } else {
            None
        };
        let round = watcher.poll_with_rebuild_sink(
            &source,
            now,
            process,
            Some(&mut |action| {
                if let Some(summaries) = &mut summaries {
                    summaries.write(action, now, &mut log);
                }
            }),
        );
        let more = round.more;
        report(
            round,
            now,
            summaries.as_mut(),
            &mut console,
            &mut previous_line,
            &mut log,
        );
        Ok(Some(more))
    })?;
    if let Some(summaries) = &mut summaries {
        summaries.finish(&watcher, wall_time(), &mut log);
    }
    Ok(())
}

fn drive(
    poll: Duration,
    mut clock: impl FnMut() -> LogTime,
    mut sleep: impl FnMut(Duration),
    mut cycle: impl FnMut(LogTime) -> Result<Option<bool>, String>,
) -> Result<(), String> {
    while let Some(more) = cycle(clock())? {
        if !more {
            sleep(poll);
        }
    }
    Ok(())
}

fn check_output_paths(paths: &AppPaths, input: &Path) -> Result<(), String> {
    let input_absolute = std::path::absolute(input)
        .map_err(|_| "VIVE Hub のログフォルダーを確認できませんでした。".to_owned())?;
    let input_resolved = fs::canonicalize(&input_absolute).ok();
    for output in [&paths.summary_dir, &paths.app_log_dir]
        .into_iter()
        .flatten()
    {
        let output = std::path::absolute(output)
            .map_err(|_| "OpenUT の保存先を確認できませんでした。".to_owned())?;
        let aliases_input = input_resolved.as_ref().is_some_and(|root| {
            output
                .ancestors()
                .find_map(|path| fs::canonicalize(path).ok())
                .is_some_and(|parent| parent.starts_with(root))
        });
        if output.starts_with(&input_absolute) || aliases_input {
            return Err(
                "VIVE Hub のログフォルダーの外に OpenUT の保存先を指定してください。".to_owned(),
            );
        }
    }
    Ok(())
}

fn report(
    round: WatchRound,
    now: LogTime,
    summaries: Option<&mut SummaryPersistence>,
    notices: &mut impl NoticeSink,
    previous_line: &mut Option<&'static str>,
    log: &mut AppLog,
) {
    for diagnostic in &round.diagnostics {
        log.write(now, diagnostic);
    }
    for output in &round.outputs {
        if let Output::Notice(notice) = output
            && notice.push
            && let Err(error) = notices.push(notice)
        {
            log.write(now, &format!("通知を出力できません（{}）。", error.kind()));
        }
    }
    let line = status_text(&round.display);
    if line != *previous_line {
        if let Some(line) = line {
            let _ = writeln!(io::stdout().lock(), "{line}");
        }
        *previous_line = line;
    }
    if let Some(summaries) = summaries {
        if round.periodic_summary {
            summaries.retry_finals(now, log);
        }
        for action in round.summaries {
            summaries.write(action, now, log);
        }
    }
}

struct SummaryPersistence {
    writer: SummaryWriter,
    // The engine releases a finalized summary only once. Keep it ahead of any
    // later snapshot for the same day until it is on disk.
    pending_finals: BTreeMap<String, Summary>,
}

impl SummaryPersistence {
    fn new(path: PathBuf) -> Self {
        Self {
            writer: SummaryWriter::new(path),
            pending_finals: BTreeMap::new(),
        }
    }

    fn write(&mut self, action: SummaryWrite, now: LogTime, log: &mut AppLog) {
        let result = match &action {
            SummaryWrite::Daily(summary) => {
                if self.pending_finals.contains_key(&summary.date) {
                    return;
                }
                self.writer.write(summary).map(|_| ())
            }
            SummaryWrite::BeforeRebuild(summary) => {
                self.writer.save_before_rebuild(summary).map(|_| ())
            }
        };
        if let Err(error) = result {
            log_summary_failure(now, log, &error);
            if let SummaryWrite::Daily(summary) = action
                && summary.is_final
            {
                self.pending_finals.insert(summary.date.clone(), summary);
            }
        }
    }

    fn retry_finals(&mut self, now: LogTime, log: &mut AppLog) {
        self.pending_finals
            .retain(|_, summary| match self.writer.write(summary) {
                Ok(_) => false,
                Err(error) => {
                    log_summary_failure(now, log, &error);
                    true
                }
            });
    }

    fn finish(&mut self, watcher: &Watcher, now: LogTime, log: &mut AppLog) {
        self.retry_finals(now, log);
        if watcher.catching_up {
            return;
        }
        for summary in watcher.open_summaries() {
            if !self.pending_finals.contains_key(&summary.date)
                && let Err(error) = self.writer.write(&summary)
            {
                log.write(
                    now,
                    &format!("終了時の日次集計を保存できません（{}）。", error.kind()),
                );
            }
        }
    }
}

fn log_summary_failure(now: LogTime, log: &mut AppLog, error: &io::Error) {
    log.write(
        now,
        &format!("日次集計を保存できません（{}）。", error.kind()),
    );
}

fn status_text(display: &DisplayModel) -> Option<&'static str> {
    if display.pick_folder || matches!(display.line, Some(Line::Health(WatchHealth::NotFound))) {
        return Some("VIVE Hub のログが見つかりません。ログのフォルダーを確認してください。");
    }
    match display.line.as_ref() {
        Some(Line::Health(WatchHealth::Unreadable)) => {
            Some("ログを読み取れません。フォルダーのアクセス権を確認してください。")
        }
        Some(Line::Health(WatchHealth::Stalled)) => Some("VIVE Hub のログの更新が止まっています。"),
        Some(Line::Health(WatchHealth::FormatMismatch)) => {
            Some("VIVE Hub のログ形式を確認できません。OpenUT のログを確認してください。")
        }
        _ => None,
    }
}

fn console_stop() -> Result<Arc<AtomicBool>, String> {
    if io::stdin().is_terminal() {
        println!("Enter キーでログ監視を終了します。");
    }
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&stop);
    thread::Builder::new()
        .spawn(move || {
            if io::stdin()
                .read_line(&mut String::new())
                .is_ok_and(|n| n > 0)
            {
                done.store(true, Ordering::Relaxed);
            }
        })
        .map_err(|_| "ログ監視の終了操作を準備できませんでした。".to_owned())?;
    Ok(stop)
}

fn wall_time() -> LogTime {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    LogTime(i64::try_from(time).unwrap_or(i64::MAX))
}

struct AppLog {
    root: Option<PathBuf>,
    open: Option<(LogDate, File)>,
}

impl AppLog {
    fn new(root: Option<PathBuf>, now: LogTime) -> Self {
        if let Some(folder) = root.as_deref() {
            expire_old_logs(folder, now.to_local(UtcOffset(0)).0);
        }
        Self { root, open: None }
    }

    fn write(&mut self, now: LogTime, text: &str) {
        let date = now.to_local(UtcOffset(0)).0;
        if self.open.as_ref().is_none_or(|(open, _)| *open != date) {
            self.open = self.root.as_deref().and_then(|folder| {
                fs::create_dir_all(folder).ok()?;
                let file = folder.join(format!(
                    "openut-{:04}{:02}{:02}.log",
                    date.year, date.month, date.day
                ));
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file)
                    .ok()?;
                Some((date, file))
            });
        }
        if let Some((_, file)) = &mut self.open {
            let _ = writeln!(file, "{now} UTC {text}");
        }
    }
}

fn expire_old_logs(folder: &Path, today: LogDate) {
    let Some(today) = today.days_since_epoch() else {
        return;
    };
    let Ok(files) = fs::read_dir(folder) else {
        return;
    };
    for entry in files.flatten() {
        let name = entry.file_name();
        if log_day(name.to_str().unwrap_or_default())
            .is_some_and(|day| today.saturating_sub(day) > 14)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn log_day(name: &str) -> Option<i64> {
    let date = name.strip_prefix("openut-")?.strip_suffix(".log")?;
    if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    LogDate {
        year: date[..4].parse().ok()?,
        month: date[4..6].parse().ok()?,
        day: date[6..].parse().ok()?,
    }
    .days_since_epoch()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::TestDirectory;

    #[test]
    fn shutdown_and_catch_up_use_the_injected_clock_and_sleep() {
        let mut now = 0;
        let mut observed = Vec::new();
        let mut sleeps = Vec::new();
        drive(
            Duration::from_millis(500),
            || {
                now += 1;
                LogTime(now)
            },
            |delay| sleeps.push(delay),
            |at| {
                observed.push(at);
                Ok(match at.0 {
                    1 => Some(true),
                    2 => Some(false),
                    _ => None,
                })
            },
        )
        .unwrap();
        assert_eq!(observed, [LogTime(1), LogTime(2), LogTime(3)]);
        assert_eq!(sleeps, [Duration::from_millis(500)]);
    }

    #[test]
    fn watcher_never_places_its_output_in_the_hub_log_folder() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/replay/synthetic-midnight");
        let inside = AppPaths::resolve(None, Some(&input));
        assert!(check_output_paths(&inside, &input).is_err());
        let outside = AppPaths::resolve(None, Some(input.parent().unwrap()));
        assert!(check_output_paths(&outside, &input).is_ok());
    }

    #[test]
    fn retries_failed_final_summary_at_periodic_save_or_shutdown() {
        const PRIOR_DATE: &str = "2026-11-27";
        const MIDNIGHT: i64 = 1_795_824_000_000;
        enum Retry {
            Periodic,
            Shutdown,
            ShutdownDuringCatchUp,
        }

        for retry in [
            Retry::Periodic,
            Retry::Shutdown,
            Retry::ShutdownDuringCatchUp,
        ] {
            let scratch = TestDirectory::new().unwrap();
            let input = scratch.0.join("hub-logs");
            let output = scratch.0.join("summary");
            let final_path = output.join(format!("{PRIOR_DATE}.json"));
            fs::create_dir(&input).unwrap();
            fs::write(
                input.join("ViveTrackerServer_20261127.txt"),
                "[2026-11-27 23:59:59.000 +0 0x1 INFO Test] [1] Tracker Server Starts.\n",
            )
            .unwrap();
            fs::write(
                input.join("ViveTrackerServer_20261128.txt"),
                "[2026-11-28 00:00:01.000 +0 0x1 INFO Test] [1] First row after midnight\n",
            )
            .unwrap();
            fs::write(&output, "file blocking summary directory").unwrap();

            let source = FsLogSource::new(input);
            let mut watcher = Watcher::new(WatchConfig::default());
            let mut summaries = SummaryPersistence::new(output.clone());
            let mut console = Console::new(Vec::new());
            let mut previous_line = None;
            let start = LogTime(MIDNIGHT + 1_000);
            let cutoff = LogTime(MIDNIGHT + 601_000);
            let mut log = AppLog::new(None, start);
            // The tailer reads each startup file in its own round.
            for catching_up in [true, false] {
                let round = watcher.poll(&source, start, None);
                assert_eq!(round.catching_up, catching_up);
                report(
                    round,
                    start,
                    Some(&mut summaries),
                    &mut console,
                    &mut previous_line,
                    &mut log,
                );
            }

            let round = watcher.poll(&source, cutoff, None);
            let expected = round
                .summaries
                .iter()
                .find_map(|write| match write {
                    SummaryWrite::Daily(day) if day.date == PRIOR_DATE && day.is_final => {
                        Some(serde_json::to_value(day).unwrap())
                    }
                    _ => None,
                })
                .expect("prior day is finalized before the failed write");
            report(
                round,
                cutoff,
                Some(&mut summaries),
                &mut console,
                &mut previous_line,
                &mut log,
            );
            assert!(!final_path.is_file());
            assert!(summaries.pending_finals.contains_key(PRIOR_DATE));
            fs::remove_file(&output).unwrap();

            match retry {
                Retry::Periodic => {
                    let now = LogTime(cutoff.0 + 60_000);
                    let round = watcher.poll(&source, now, None);
                    assert!(round.periodic_summary);
                    assert!(round.summaries.iter().all(|write| {
                        !matches!(write, SummaryWrite::Daily(day) if day.date == PRIOR_DATE)
                    }));
                    report(
                        round,
                        now,
                        Some(&mut summaries),
                        &mut console,
                        &mut previous_line,
                        &mut log,
                    );
                }
                Retry::Shutdown | Retry::ShutdownDuringCatchUp => {
                    watcher.catching_up = matches!(retry, Retry::ShutdownDuringCatchUp);
                    summaries.finish(&watcher, LogTime(cutoff.0 + 1_000), &mut log);
                    assert_eq!(
                        output.join("2026-11-28.json").is_file(),
                        !watcher.catching_up
                    );
                }
            }
            assert!(summaries.pending_finals.is_empty());
            let actual: serde_json::Value =
                serde_json::from_slice(&fs::read(final_path).unwrap()).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[cfg(windows)]
    #[test]
    fn measures_read_only_process_probe_with_both_server_inventories() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/replay/synthetic-midnight");
        let source = FsLogSource::new(root);
        let mut process = HubProcess;
        let mut watcher = Watcher::new(WatchConfig::default());
        for _ in 0..5 {
            let mut data = Vec::new();
            watcher.probe(
                &source,
                wall_time(),
                &mut |names| process.check(names),
                &mut data,
            );
            let timing = data.first().expect("process timing diagnostic");
            // Write directly so CI records the measurement on success too.
            writeln!(io::stderr().lock(), "OPENUT_PROCESS_PROBE {timing}").unwrap();
        }
    }
}
