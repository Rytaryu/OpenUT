use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{self, Path, PathBuf};

use openut::config::{apply_engine_config, set_engine_value};
use openut::paths::AppPaths;
use openut::replay::{Expectations, ObservedSummaries, RaisedDetection, ReplayOptions};
use openut::summary_io::{SummaryWriter, WriteOutcome};
use openut_core::engine::EngineConfig;
use openut_core::rules::Step;
use openut_core::time::LogTime;
use openut_hublog::header::FileKind;
use openut_hublog::replay::{ReplayFile, ReplayResult, replay_with_options};
use openut_hublog::tail::{LogName, parse_log_name};
use serde::Serialize;
use toml::{Table, Value};

pub(super) const WATCH_USAGE: &str =
    "openut watch [--log-dir DIR] [--config FILE] [--data-dir DIR]";
pub(super) const REPLAY_USAGE: &str =
    "openut replay CASE_OR_LOG... [--expect FILE] [--set NAME=VALUE]... [--format text|toml]";
pub(super) const SUMMARIZE_USAGE: &str = "openut summarize LOG... [--out DIR]";

pub(super) fn watch(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let mut log_dir = None;
    let mut config = None;
    let mut data_dir = None;
    while let Some(arg) = args.next() {
        let destination = match arg.to_str() {
            Some("--log-dir") => &mut log_dir,
            Some("--config") => &mut config,
            Some("--data-dir") => &mut data_dir,
            Some("--help" | "-h")
                if log_dir.is_none() && config.is_none() && data_dir.is_none() =>
            {
                return help(args, WATCH_USAGE);
            }
            _ => return Err(WATCH_USAGE.to_owned()),
        };
        set_once(
            destination,
            next_value(&mut args, WATCH_USAGE)?.into(),
            WATCH_USAGE,
        )?;
    }
    openut::watcher::watch(log_dir, config, data_dir)
}

pub(super) fn replay(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let mut args = args.peekable();
    if help_requested(args.peek()) {
        args.next();
        return help(args, REPLAY_USAGE);
    }
    let mut operands = Vec::new();
    let mut expect = None;
    let mut overrides = Vec::new();
    let mut format = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--expect") => {
                set_once(
                    &mut expect,
                    PathBuf::from(next_value(&mut args, REPLAY_USAGE)?),
                    REPLAY_USAGE,
                )?;
            }
            Some("--set") => overrides.push(parse_override(next_value(&mut args, REPLAY_USAGE)?)?),
            Some("--format") => {
                let value = next_value(&mut args, REPLAY_USAGE)?;
                let output = match value.to_str() {
                    Some("text") => Format::Text,
                    Some("toml") => Format::Toml,
                    _ => return Err(REPLAY_USAGE.to_owned()),
                };
                set_once(&mut format, output, REPLAY_USAGE)?;
            }
            Some("--") => {
                operands.extend(args.map(PathBuf::from));
                break;
            }
            Some(arg) if arg.starts_with('-') => return Err(REPLAY_USAGE.to_owned()),
            _ => operands.push(arg.into()),
        }
    }
    if operands.is_empty() {
        return Err(REPLAY_USAGE.to_owned());
    }
    let logs = SavedLogs::load(&operands, true)?;
    let expectations = read_expectations(expect.as_deref(), logs.case_dir.as_deref())?;
    let mut engine = EngineConfig::default();
    let mut rules = Table::new();
    let default_replay = ReplayOptions::default();
    let options = if let Some(expected) = &expectations {
        rules.clone_from(&expected.rules);
        let config = Table::from_iter([("rules".to_owned(), Value::Table(rules.clone()))]);
        apply_engine_config(&mut engine, &config).map_err(|error| error.to_string())?;
        &expected.replay
    } else {
        &default_replay
    };
    for (key, value) in overrides {
        set_engine_value(&mut engine, &key, &value).map_err(|error| error.to_string())?;
        if let Some((section, name)) = key
            .strip_prefix("rules.")
            .and_then(|path| path.split_once('.'))
        {
            let table = rules.entry(section).or_insert(Value::Table(Table::new()));
            if let Value::Table(table) = table {
                table.insert(name.to_owned(), value);
            }
        }
    }
    let result = logs.replay(engine, options);
    let raised = raised_detections(&result);
    let output = match format.unwrap_or(Format::Text) {
        Format::Text => text_output(&raised),
        Format::Toml => toml_output(&raised, options, &rules)?,
    };
    print_output(&output)?;
    if let Some(expected) = expectations {
        let summaries = observed_summaries(&result)?;
        expected.compare(&raised, &summaries)?;
    }
    Ok(())
}

pub(super) fn summarize(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let mut args = args.peekable();
    if help_requested(args.peek()) {
        args.next();
        return help(args, SUMMARIZE_USAGE);
    }
    let mut operands = Vec::new();
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => {
                set_once(
                    &mut out,
                    PathBuf::from(next_value(&mut args, SUMMARIZE_USAGE)?),
                    SUMMARIZE_USAGE,
                )?;
            }
            Some("--") => {
                operands.extend(args.map(PathBuf::from));
                break;
            }
            Some(arg) if arg.starts_with('-') => return Err(SUMMARIZE_USAGE.to_owned()),
            _ => operands.push(arg.into()),
        }
    }
    if operands.is_empty() {
        return Err(SUMMARIZE_USAGE.to_owned());
    }
    let output = out
        .or_else(|| AppPaths::resolve(None, None).summary_dir)
        .ok_or_else(|| "集計の保存先を --out で指定してください。".to_owned())?;
    let logs = SavedLogs::load(&operands, false)?;
    check_summary_output(&output, &operands)?;
    let writer = SummaryWriter::new(output);
    let result = logs.replay(EngineConfig::default(), &ReplayOptions::default());
    for summary in result.summaries {
        let outcome = writer.write(&summary).map_err(|error| {
            format!(
                "{} の集計を保存できませんでした（{}）",
                summary.date,
                error.kind()
            )
        })?;
        if outcome == WriteOutcome::AlreadyFinal {
            println!(
                "{} の確定済み集計があります。別に保存するには --out で保存先を指定してください。",
                summary.date
            );
        }
    }
    Ok(())
}

fn help_requested(arg: Option<&OsString>) -> bool {
    matches!(arg.and_then(|arg| arg.to_str()), Some("--help" | "-h"))
}

fn help(mut args: impl Iterator<Item = OsString>, usage: &str) -> Result<(), String> {
    if args.next().is_some() {
        return Err(usage.to_owned());
    }
    println!("{usage}");
    Ok(())
}

fn next_value(args: &mut impl Iterator<Item = OsString>, usage: &str) -> Result<OsString, String> {
    args.next()
        .filter(|arg| !arg.is_empty() && !arg.to_str().is_some_and(|arg| arg.starts_with('-')))
        .ok_or_else(|| usage.to_owned())
}

fn set_once<T>(slot: &mut Option<T>, value: T, usage: &str) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err(usage.to_owned());
    }
    Ok(())
}

fn parse_override(arg: OsString) -> Result<(String, Value), String> {
    let invalid = || "設定の上書きは --set 名前=値 の形で指定してください。".to_owned();
    let (key, value) = arg
        .to_str()
        .and_then(|arg| arg.split_once('='))
        .ok_or_else(invalid)?;
    let mut table: Table = format!("value = {value}").parse().map_err(|_| invalid())?;
    if key.is_empty() || table.len() != 1 {
        return Err(invalid());
    }
    let value = table.remove("value").ok_or_else(invalid)?;
    Ok((key.to_owned(), value))
}

fn read_expectations(
    explicit: Option<&Path>,
    case: Option<&Path>,
) -> Result<Option<Expectations>, String> {
    let default = case.map(|dir| dir.join("expect.toml"));
    let Some(path) = explicit.or(default.as_deref()) else {
        return Ok(None);
    };
    match fs::read_to_string(path) {
        Ok(text) => Expectations::parse(&text)
            .map(Some)
            .map_err(|error| format!("期待のファイルを読めませんでした。\n{error}")),
        Err(error) if error.kind() == io::ErrorKind::NotFound && explicit.is_none() => Ok(None),
        Err(error) => Err(format!(
            "期待のファイルを読めませんでした（{}）",
            error.kind()
        )),
    }
}

enum Format {
    Text,
    Toml,
}

struct SavedLogs {
    files: Vec<(String, Vec<u8>)>,
    case_dir: Option<PathBuf>,
}

impl SavedLogs {
    fn load(operands: &[PathBuf], allow_directories: bool) -> Result<Self, String> {
        let mut paths = Vec::new();
        let mut case_dir = None;
        for (index, operand) in operands.iter().enumerate() {
            let number = index + 1;
            let metadata = fs::metadata(operand)
                .map_err(|error| format!("入力 {number} を開けませんでした（{}）", error.kind()))?;
            if metadata.is_file() {
                let (name, log) = log_name(operand).ok_or_else(|| {
                    format!("入力 {number} の名前が VIVE Hub の対象ログの形式と違います")
                })?;
                paths.push((name, log, operand.clone()));
            } else if allow_directories && metadata.is_dir() {
                if operands.len() == 1 {
                    case_dir = Some(operand.clone());
                }
                let entries = fs::read_dir(operand).map_err(|error| {
                    format!(
                        "入力 {number} のフォルダーを読めませんでした（{}）",
                        error.kind()
                    )
                })?;
                for entry in entries {
                    let entry = entry.map_err(|error| {
                        format!(
                            "入力 {number} のフォルダーを読めませんでした（{}）",
                            error.kind()
                        )
                    })?;
                    let path = entry.path();
                    if let Some((name, log)) = log_name(&path).filter(|_| path.is_file()) {
                        paths.push((name, log, path));
                    }
                }
            } else {
                return Err(format!("入力 {number} にログファイルを指定してください"));
            }
        }

        if !paths.iter().any(|(_, log, _)| log.kind == FileKind::Server) {
            return Err("VIVE Hub のサーバーログを 1 つ以上指定してください。".to_owned());
        }
        let mut dates = BTreeSet::new();
        let mut files = Vec::with_capacity(paths.len());
        for (name, log, path) in paths {
            if !dates.insert((log.kind == FileKind::Server, log.date)) {
                return Err(format!("{} の同じ種類のログが複数あります", log.date));
            }
            let bytes = fs::read(path)
                .map_err(|error| format!("{name} を読めませんでした（{}）", error.kind()))?;
            files.push((name, bytes));
        }
        Ok(Self { files, case_dir })
    }

    fn replay(&self, engine: EngineConfig, options: &ReplayOptions) -> ReplayResult {
        let files: Vec<_> = self
            .files
            .iter()
            .map(|(name, bytes)| ReplayFile { name, bytes })
            .collect();
        replay_with_options(&files, engine, u64::from(options.flush_s), options.catch_up)
    }
}

fn log_name(path: &Path) -> Option<(String, LogName)> {
    let name = path.file_name().and_then(OsStr::to_str)?;
    let log = parse_log_name(name)?;
    (!log.numbered).then(|| (name.to_owned(), log))
}

fn check_summary_output(output: &Path, inputs: &[PathBuf]) -> Result<(), String> {
    let path_error =
        |error: io::Error| format!("集計の保存先を確かめられませんでした（{}）", error.kind());
    let output = path::absolute(output).map_err(path_error)?;
    // Resolve the nearest existing ancestor, including symlinks, before creating any directory.
    for ancestor in output.ancestors() {
        match fs::canonicalize(ancestor) {
            Ok(parent) => {
                for input in inputs {
                    let input = path::absolute(input).map_err(path_error)?;
                    let canonical_input = fs::canonicalize(&input).map_err(path_error)?;
                    let input_parent =
                        fs::canonicalize(input.parent().unwrap_or(&input)).map_err(path_error)?;
                    if super::is_within(&parent, &input_parent)
                        || canonical_input
                            .parent()
                            .is_some_and(|log_dir| super::is_within(&parent, log_dir))
                    {
                        return Err(
                            "集計の保存先はログのフォルダーの外に指定してください。".to_owned()
                        );
                    }
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(path_error(error)),
        }
    }
    Err("集計の保存先を確かめられませんでした。".to_owned())
}

fn raised_detections(result: &ReplayResult) -> Vec<RaisedDetection<'_>> {
    let mut raised: Vec<_> = result
        .detections
        .iter()
        .zip(&result.detection_offsets)
        .filter(|(detection, _)| detection.step == Step::Raised)
        .map(|(detection, &offset)| RaisedDetection {
            rule: detection.rule.as_str(),
            tracker: detection.tracker.map(|tracker| tracker.0),
            at: detection.at,
            offset,
        })
        .collect();
    raised.sort_by_key(|detection| (detection.at, detection.rule, detection.tracker));
    raised
}

fn observed_summaries(result: &ReplayResult) -> Result<ObservedSummaries, String> {
    result
        .summaries
        .iter()
        .map(|summary| {
            let values = Table::try_from(summary)
                .map_err(|_| format!("{} の集計を比較できませんでした。", summary.date))?;
            Ok((summary.date.clone(), values))
        })
        .collect()
}

fn local_time(detection: &RaisedDetection<'_>, offset_ms: i64) -> String {
    let (date, clock) =
        LogTime(detection.at.0.saturating_add(offset_ms)).to_local(detection.offset);
    format!("{date} {clock}")
}

fn text_output(raised: &[RaisedDetection<'_>]) -> String {
    use std::fmt::Write as _;
    if raised.is_empty() {
        return "検出なし\n".to_owned();
    }
    let mut text = String::new();
    for detection in raised {
        let at = local_time(detection, 0);
        let rule = detection.rule;
        match detection.tracker {
            Some(number) => {
                let _ = writeln!(text, "{at} {rule} トラッカー {number}");
            }
            None => {
                let _ = writeln!(text, "{at} {rule}");
            }
        }
    }
    text
}

#[derive(Serialize)]
struct ExpectedOutput<'a> {
    check_rules: [&'static str; 5],
    replay: OutputOptions,
    #[serde(skip_serializing_if = "Table::is_empty")]
    rules: &'a Table,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    detection: Vec<ExpectedDetection<'a>>,
}

#[derive(Serialize)]
struct OutputOptions {
    flush_s: u32,
    catch_up: bool,
}

#[derive(Serialize)]
struct ExpectedDetection<'a> {
    rule: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracker: Option<u16>,
    after: String,
    before: String,
}

fn toml_output(
    raised: &[RaisedDetection<'_>],
    options: &ReplayOptions,
    rules: &Table,
) -> Result<String, String> {
    let output = ExpectedOutput {
        check_rules: ["R1", "R2", "R3", "R4", "R5"],
        replay: OutputOptions {
            flush_s: options.flush_s,
            catch_up: options.catch_up,
        },
        rules,
        detection: raised
            .iter()
            .map(|detection| ExpectedDetection {
                rule: detection.rule,
                tracker: detection.tracker,
                after: local_time(detection, 0),
                before: local_time(detection, 1),
            })
            .collect(),
    };
    toml::to_string_pretty(&output)
        .map_err(|_| "検出の一覧を TOML に変換できませんでした。".to_owned())
}

fn print_output(output: &str) -> Result<(), String> {
    io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .map_err(|error| format!("検出の一覧を出力できませんでした（{}）", error.kind()))
}
