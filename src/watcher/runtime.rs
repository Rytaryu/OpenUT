//! Filesystem, console, and wall-time adapters for the watcher.

use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openut_core::display::{DisplayModel, Line, WatchHealth};
use openut_core::engine::Output;
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
    let writer = paths.summary_dir.clone().map(SummaryWriter::new);
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
                if let Some(writer) = writer.as_ref() {
                    write_summary(writer, &action, now, &mut log);
                }
            }),
        );
        report(
            &round,
            now,
            writer.as_ref(),
            &mut console,
            &mut previous_line,
            &mut log,
        );
        Ok(Some(round.more))
    })?;
    if !watcher.catching_up
        && let Some(writer) = writer.as_ref()
    {
        for summary in watcher.open_summaries() {
            if let Err(error) = writer.write(&summary) {
                log.write(
                    wall_time(),
                    &format!("終了時の日次集計を保存できません（{}）。", error.kind()),
                );
            }
        }
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
    round: &WatchRound,
    now: LogTime,
    writer: Option<&SummaryWriter>,
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
    if let Some(writer) = writer {
        for summary in &round.summaries {
            write_summary(writer, summary, now, log);
        }
    }
}

fn write_summary(writer: &SummaryWriter, action: &SummaryWrite, now: LogTime, log: &mut AppLog) {
    let result = match action {
        SummaryWrite::Daily(summary) => writer.write(summary).map(|_| ()),
        SummaryWrite::BeforeRebuild(summary) => writer.save_before_rebuild(summary).map(|_| ()),
    };
    if let Err(error) = result {
        log.write(
            now,
            &format!("日次集計を保存できません（{}）。", error.kind()),
        );
    }
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
