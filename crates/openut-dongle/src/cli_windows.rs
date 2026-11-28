//! Windows entry points for local diagnostics and recordings.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use openut_core::time::{LogTime, UtcOffset};
use openut_dongle::guard::ServerGuard;
use openut_dongle::process_windows::ProcessList;
use openut_dongle::record::{Device, EndReason, Header, Mode, Note, RecordWriter};
use openut_dongle::run::{self, Clock, Notice, RunResult, SystemClock};
use openut_dongle::source_windows::{DeviceList, OpenError};
use openut_dongle::{summary, view};

pub const USAGE: &str = "\
  openut-dongle list\n\
  openut-dongle check [--server-name NAME]\n\
  openut-dongle record [--minutes N] [--out PATH] [--server-name NAME]\n\
  openut-dongle probe-sharing [--seconds N] [--utc-offset +9] [--out PATH] [--server-name NAME]";

const SERVER_RUNNING: &str = "VIVE Hub のトラッカー用サーバー（ViveTrackerServer.exe）が動いています。VIVE Hub を終了し、タスク マネージャーでサーバーがいないことを確かめてから、もう一度実行してください。";
const SHARE_GUIDANCE: &str =
    "記録には個人の情報が入りうるので、人に渡すときは export --anonymize を使ってください。";

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Command {
    List,
    Check,
    Record,
    ProbeSharing,
}

impl Command {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "list" => Some(Self::List),
            "check" => Some(Self::Check),
            "record" => Some(Self::Record),
            "probe-sharing" => Some(Self::ProbeSharing),
            _ => None,
        }
    }
}

pub fn run(command: Command, args: impl Iterator<Item = OsString>) -> u8 {
    let result = Options::parse(command, args)
        .and_then(|options| options.execute(command, io::stdout().lock()));
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", error.message);
            error.code
        }
    }
}

struct Options {
    length: Duration,
    path: Option<PathBuf>,
    offset: Option<UtcOffset>,
    additional_servers: Vec<OsString>,
}

impl Options {
    fn parse(command: Command, mut args: impl Iterator<Item = OsString>) -> Result<Self, Failure> {
        let mut options = Self {
            length: Duration::from_secs(if command == Command::ProbeSharing {
                30
            } else {
                10 * 60
            }),
            path: None,
            offset: None,
            additional_servers: Vec::new(),
        };
        while let Some(option) = args.next() {
            match (command, option.to_str()) {
                (Command::Record, Some("--minutes")) => {
                    options.length = Duration::from_secs(60 * length(args.next(), "--minutes")?);
                }
                (Command::ProbeSharing, Some("--seconds")) => {
                    options.length = Duration::from_secs(length(args.next(), "--seconds")?);
                }
                (Command::Record | Command::ProbeSharing, Some("--out")) => {
                    options.path = Some(PathBuf::from(argument(args.next())?));
                }
                (Command::ProbeSharing, Some("--utc-offset")) => {
                    options.offset = Some(
                        argument(args.next())?
                            .to_str()
                            .and_then(summary::parse_utc_offset)
                            .ok_or_else(|| {
                                Failure::new(
                                    1,
                                    "時差には +9 や -05:30 のような値を指定してください",
                                )
                            })?,
                    );
                }
                (
                    Command::Check | Command::Record | Command::ProbeSharing,
                    Some("--server-name"),
                ) => {
                    options.additional_servers.push(argument(args.next())?);
                }
                _ => return Err(Failure::new(1, USAGE)),
            }
        }
        Ok(options)
    }

    fn execute(self, command: Command, mut out: impl Write) -> Result<u8, Failure> {
        if command == Command::List {
            return list(&mut out);
        }
        let guard = ServerGuard::new(self.additional_servers.iter().cloned());
        let mut processes = ProcessList::new().map_err(|error| {
            Failure::new(
                3,
                format!(
                    "トラッカー用サーバーの動作を確認できません（{:?}）",
                    error.kind()
                ),
            )
        })?;
        let running = processes.server_running(&guard);
        if command == Command::Check {
            return check(running, &mut out);
        }

        let mode = if command == Command::ProbeSharing {
            if !running {
                return Err(Failure::new(
                    1,
                    "共有の確認には VIVE Hub のトラッカー用サーバーを起動してください。",
                ));
            }
            Mode::ProbeSharing
        } else {
            if running {
                return Err(Failure::new(2, SERVER_RUNNING));
            }
            Mode::Record
        };
        let devices = DeviceList::new().map_err(Failure::dongle)?;
        let device = devices.dongle().map_err(Failure::dongle)?;
        let mut clock = SystemClock::new();
        let started = clock.now();
        let (path, destination) = capture_destination(self.path.as_deref(), mode, started.wall_ms)?;
        let header = Header {
            tool: format!("openut-dongle {}", env!("CARGO_PKG_VERSION")),
            mode,
            started_wall_ms: started.wall_ms,
            device,
            server_running_at_start: running,
            anonymized: false,
        };
        let mut display = Console::new(out, self.offset);
        let result = match mode {
            Mode::Record => {
                if processes.server_running(&guard) {
                    return Err(Failure::new(2, SERVER_RUNNING));
                }
                let source = devices.open_dongle().map_err(Failure::dongle)?;
                let mut writer = start_capture(&path, &header, &destination, &mut display)?;
                run::run_record(
                    source,
                    clock,
                    &mut writer,
                    started,
                    self.length,
                    || processes.server_running(&guard),
                    |notice| display.notice(notice),
                )
            }
            Mode::ProbeSharing => {
                let mut writer = start_capture(&path, &header, &destination, &mut display)?;
                run::run_probe(
                    &mut writer,
                    clock,
                    self.length,
                    std::thread::sleep,
                    || processes.server_running(&guard),
                    || devices.open_dongle().map_err(|error| error.os_code()),
                    |notice| display.notice(notice),
                )
            }
        }
        .map_err(Failure::recording)?;
        display
            .finish(&result, &destination)
            .map_err(Failure::output)?;
        Ok(match result.reason {
            EndReason::Duration | EndReason::Complete => 0,
            EndReason::ServerNotRunning => 1,
            EndReason::ServerAppeared => 2,
            EndReason::ReadError | EndReason::DeviceRemoved => 3,
        })
    }
}

fn argument(value: Option<OsString>) -> Result<OsString, Failure> {
    value
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Failure::new(1, USAGE))
}

fn length(value: Option<OsString>, option: &str) -> Result<u64, Failure> {
    value
        .as_deref()
        .and_then(OsStr::to_str)
        .and_then(|value| value.parse().ok())
        .filter(|number| (1..=60).contains(number))
        .ok_or_else(|| Failure::new(1, format!("{option} は 1〜60 の整数を指定してください")))
}

fn list(out: &mut impl Write) -> Result<u8, Failure> {
    let devices = DeviceList::new().map_err(Failure::dongle)?;
    let mut devices = devices.devices().peekable();
    if devices.peek().is_none() {
        writeln!(out, "HTC の HID 機器は見つかりませんでした。").map_err(Failure::output)?;
    }
    for summary in devices {
        writeln!(
            out,
            "{} / 製品 {}",
            device_text(summary.device),
            summary.product.as_deref().unwrap_or("不明"),
        )
        .map_err(Failure::output)?;
    }
    out.flush().map_err(Failure::output)?;
    Ok(0)
}

fn check(running: bool, out: &mut impl Write) -> Result<u8, Failure> {
    let device = DeviceList::new().and_then(|devices| devices.dongle());
    writeln!(
        out,
        "トラッカー用サーバー: {}",
        if running { "動作中" } else { "停止" },
    )
    .map_err(Failure::output)?;
    match &device {
        Ok(device) => writeln!(out, "対象ドングル: {}", device_text(*device)),
        Err(_) => writeln!(out, "対象ドングルを確認できません。"),
    }
    .map_err(Failure::output)?;
    if running {
        return Err(Failure::new(2, SERVER_RUNNING));
    }
    device.map_err(Failure::dongle)?;
    writeln!(out, "読み取りを始められます。").map_err(Failure::output)?;
    out.flush().map_err(Failure::output)?;
    Ok(0)
}

fn device_text(device: Device) -> String {
    format!(
        "VID {:04X} / PID {:04X} / インターフェース {} / 用途 {:04X}:{:04X}",
        device.vid, device.pid, device.interface, device.usage_page, device.usage,
    )
}

fn capture_destination(
    path: Option<&Path>,
    mode: Mode,
    wall_ms: i64,
) -> Result<(PathBuf, String), Failure> {
    if let Some(path) = path {
        return Ok((path.to_path_buf(), "--out で指定したファイル".to_owned()));
    }
    let root = std::env::var_os("LOCALAPPDATA")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Failure::new(
                4,
                "記録の保存先を決められません。--out で指定してください。",
            )
        })?;
    let directory = Path::new(&root).join("OpenUT").join("dongle");
    fs::create_dir_all(&directory).map_err(Failure::recording)?;
    let (date, time) = LogTime(wall_ms).to_local(UtcOffset(0));
    let mode = match mode {
        Mode::Record => "record",
        Mode::ProbeSharing => "probe-sharing",
    };
    let name = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{mode}.jsonl",
        date.year, date.month, date.day, time.hour, time.minute, time.second,
    );
    Ok((
        directory.join(&name),
        format!("%LOCALAPPDATA%\\OpenUT\\dongle\\{name}"),
    ))
}

fn start_capture(
    path: &Path,
    header: &Header,
    destination: &str,
    console: &mut Console<impl Write>,
) -> Result<RecordWriter<BufWriter<File>>, Failure> {
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(Failure::recording)?;
    let mut writer =
        RecordWriter::new(BufWriter::new(output), header).map_err(Failure::recording)?;
    writer.flush().map_err(Failure::recording)?;
    writeln!(console.out, "対象ドングル: {}", device_text(header.device))
        .map_err(Failure::output)?;
    writeln!(console.out, "記録先: {destination}").map_err(Failure::output)?;
    writeln!(
        console.out,
        "止めるには Ctrl+C を押すか、指定した時間まで待ってください。",
    )
    .map_err(Failure::output)?;
    console.out.flush().map_err(Failure::output)?;
    Ok(writer)
}

struct Console<W> {
    out: W,
    offset: Option<UtcOffset>,
}

impl<W: Write> Console<W> {
    fn new(out: W, offset: Option<UtcOffset>) -> Self {
        Self { out, offset }
    }

    fn notice(&mut self, notice: Notice<'_>) -> io::Result<()> {
        match notice {
            Notice::Tick {
                at,
                reports_per_second,
                estimated_lost,
                table,
                decoder,
            } => {
                writeln!(
                    self.out,
                    "{} 秒 / 受信 {reports_per_second} 件/秒 / 取りこぼし（推定）{estimated_lost} 件",
                    elapsed(at.mono_us),
                )?;
                for tracker in table.views() {
                    writeln!(
                        self.out,
                        "{}",
                        view::tracker_line(tracker, decoder.slot_hint(tracker.label)),
                    )?;
                }
            }
            Notice::Transition(transition) => {
                writeln!(self.out, "{}", view::transition_line(&transition))?;
            }
            Notice::SlamError { at, tracker, code } => {
                let line = view::slam_error_name(code);
                writeln!(
                    self.out,
                    "{} 秒 トラッカー {} SLAM の誤り {code}（{line}）",
                    elapsed(at.mono_us),
                    tracker.0,
                )?;
            }
            Notice::Stalled => writeln!(self.out, "ドングルの読み取りが止まっています。")?,
            Notice::Window { at, note } => {
                let label = match note {
                    Note::WindowBaseStart => "基準の窓 開始",
                    Note::WindowBaseEnd => "基準の窓 終了",
                    Note::WindowProbeStart => "試しの窓 開始",
                    Note::WindowProbeEnd => "試しの窓 終了",
                    _ => return Ok(()),
                };
                writeln!(
                    self.out,
                    "{label}: {}",
                    window_time(at.wall_ms, self.offset)
                )?;
            }
            Notice::ProbeOpened => {
                writeln!(
                    self.out,
                    "ドングルを共有で開けました。受信報告を確認します。"
                )?;
            }
            Notice::ProbeOpenFailed(code) => {
                writeln!(
                    self.out,
                    "ドングルを共有で開けませんでした（OS の番号 {code}）。"
                )?;
            }
            Notice::ProbeServerStopped => {
                writeln!(self.out, "ドングルを開かずに共有の確認を中止しました。")?;
            }
        }
        self.out.flush()
    }

    fn finish(&mut self, result: &RunResult, destination: &str) -> io::Result<()> {
        let reason = match result.reason {
            EndReason::Duration => "指定した時間に達しました",
            EndReason::ServerAppeared => "VIVE Hub のトラッカー用サーバーが起動しました",
            EndReason::ServerNotRunning => "VIVE Hub のトラッカー用サーバーが停止しました",
            EndReason::ReadError => "ドングルを読み取れませんでした",
            EndReason::DeviceRemoved => "ドングルが外れました",
            EndReason::Complete => "終了しました",
        };
        writeln!(self.out, "{reason}。記録先: {destination}")?;
        writeln!(
            self.out,
            "受信 {} 件 / 不正 {} 件 / 取りこぼし（推定）{} 件",
            result.stats.reports, result.stats.malformed, result.stats.estimated_lost,
        )?;
        for (&kind, &count) in &result.stats.report_types {
            writeln!(self.out, "報告 0x{kind:02X}: {count} 件")?;
        }
        writeln!(self.out, "{SHARE_GUIDANCE}")?;
        self.out.flush()
    }
}

fn elapsed(mono_us: u64) -> String {
    format!("{}.{:03}", mono_us / 1_000_000, mono_us / 1_000 % 1_000)
}

fn window_time(wall_ms: i64, offset: Option<UtcOffset>) -> String {
    let local = |offset| {
        let (date, time) = LogTime(wall_ms).to_local(offset);
        format!(
            "{date} {:02}:{:02}:{:02}",
            time.hour, time.minute, time.second
        )
    };
    let utc = local(UtcOffset(0));
    if let Some(offset) = offset {
        let minutes = offset.0.unsigned_abs();
        let sign = if offset.0 < 0 { '-' } else { '+' };
        format!(
            "{utc} UTC / {} {sign}{:02}:{:02}",
            local(offset),
            minutes / 60,
            minutes % 60,
        )
    } else {
        format!("{utc} UTC")
    }
}

struct Failure {
    code: u8,
    message: String,
}

impl Failure {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn dongle(error: OpenError) -> Self {
        Self::new(3, error.to_string())
    }

    fn recording(error: io::Error) -> Self {
        Self::new(4, format!("記録を書けません（{:?}）", error.kind()))
    }

    fn output(error: io::Error) -> Self {
        Self::new(1, format!("結果を出力できません（{:?}）", error.kind()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_maximum_capture_duration_and_extra_server_names() {
        let args = [
            "--minutes",
            "60",
            "--server-name",
            "Example.exe",
            "--out",
            "capture.jsonl",
        ];
        let options = Options::parse(Command::Record, args.into_iter().map(OsString::from));
        let Ok(options) = options else {
            panic!("valid record arguments were rejected");
        };
        assert_eq!(options.length, Duration::from_secs(3600));
        assert_eq!(options.path.as_deref(), Some(Path::new("capture.jsonl")));
        assert_eq!(options.additional_servers, [OsString::from("Example.exe")]);

        for invalid in ["0", "61", "1.5"] {
            let args = ["--seconds", invalid].into_iter().map(OsString::from);
            let error = Options::parse(Command::ProbeSharing, args).err();
            assert_eq!(error.map(|error| error.code), Some(1));
        }
    }

    #[test]
    fn displays_probe_windows_at_the_local_day_boundary() {
        assert_eq!(
            window_time(23 * 3_600_000, Some(UtcOffset(120))),
            "1970-01-01 23:00:00 UTC / 1970-01-02 01:00:00 +02:00",
        );
    }
}
