use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::process::ExitCode;

use openut_core::time::UtcOffset;
use openut_dongle::{replay::Replay, summary, view};

const USAGE: &str =
    "使い方\n  openut-dongle decode RECORD\n  openut-dongle summary RECORD [--utc-offset +9]";

fn main() -> ExitCode {
    if let Err(error) = run() {
        eprintln!("{error}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    if command.as_deref() == Some(OsStr::new("--help")) && args.next().is_none() {
        return writeln!(io::stdout().lock(), "{USAGE}").map_err(output_error);
    }

    let (command, path) = match (command.as_deref().and_then(OsStr::to_str), args.next()) {
        (Some("decode"), Some(path)) if args.next().is_none() => (Command::Decode, path),
        (Some("summary"), Some(path)) => {
            let offset = match args.next().as_deref() {
                None => None,
                Some(option) if option == OsStr::new("--utc-offset") => {
                    let offset = args
                        .next()
                        .as_deref()
                        .and_then(OsStr::to_str)
                        .and_then(summary::parse_utc_offset)
                        .ok_or("時差には +9 や -05:30 のような値を指定してください")?;
                    if args.next().is_some() {
                        return Err(USAGE.into());
                    }
                    Some(offset)
                }
                Some(_) => return Err(USAGE.into()),
            };
            (Command::Summary(offset), path)
        }
        _ => return Err(USAGE.into()),
    };

    let file =
        File::open(path).map_err(|error| format!("記録を開けません（{:?}）", error.kind()))?;
    let input = BufReader::new(file);
    let mut output = BufWriter::new(io::stdout().lock());
    match command {
        Command::Decode => {
            let mut replay = Replay::new(input).map_err(|error| error.to_string())?;
            while let Some(step) = replay
                .read_step()
                .map_err(|error| format!("記録を読み取れません（{:?}）", error.kind()))?
            {
                if let Some(event) = step.event {
                    let row = view::event_json(step.mono_us, step.wall_ms, &event);
                    writeln!(output, "{row}").map_err(output_error)?;
                }
            }
        }
        Command::Summary(offset) => {
            summary::write_summary(input, &mut output, offset)
                .map_err(|error| error.to_string())?;
        }
    }
    output.flush().map_err(output_error)
}

enum Command {
    Decode,
    Summary(Option<UtcOffset>),
}

fn output_error(error: io::Error) -> String {
    format!("結果を出力できません（{:?}）", error.kind())
}
