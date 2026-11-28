use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;

use openut_core::time::UtcOffset;
use openut_dongle::{replay::Replay, summary, view};

#[cfg(windows)]
mod cli_windows;

mod export_cli;

const USAGE: &str = concat!(
    "使い方\n",
    "  openut-dongle decode RECORD\n",
    "  openut-dongle summary RECORD [--utc-offset +9] [--out FILE]\n",
    "    ファイルに保存するときは --out FILE を使います（UTF-8、BOM なし、上書きなし）。\n",
    "  openut-dongle export --anonymize RECORD OUT [--map TSV]",
);

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(OsStr::new("export")) {
        return export_cli::run(args);
    }
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8, String> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    if command.as_deref() == Some(OsStr::new("--help")) && args.next().is_none() {
        let mut output = io::stdout().lock();
        writeln!(output, "{USAGE}").map_err(output_error)?;
        #[cfg(windows)]
        writeln!(output, "{}", cli_windows::USAGE).map_err(output_error)?;
        return Ok(0);
    }
    #[cfg(windows)]
    if let Some(command) = command
        .as_deref()
        .and_then(OsStr::to_str)
        .and_then(cli_windows::Command::from_name)
    {
        return Ok(cli_windows::run(command, args));
    }

    let (command, path) = match (command.as_deref().and_then(OsStr::to_str), args.next()) {
        (Some("decode"), Some(path)) if args.next().is_none() => (Command::Decode, path),
        (Some("summary"), Some(path)) => {
            let mut offset = None;
            let mut out = None;
            while let Some(option) = args.next() {
                match option.to_str() {
                    Some("--utc-offset") if offset.is_none() => {
                        offset = Some(
                            args.next()
                                .as_deref()
                                .and_then(OsStr::to_str)
                                .and_then(summary::parse_utc_offset)
                                .ok_or("時差には +9 や -05:30 のような値を指定してください")?,
                        );
                    }
                    Some("--out") if out.is_none() => {
                        out = Some(
                            args.next()
                                .filter(|path| {
                                    !path.is_empty() && !path.as_encoded_bytes().starts_with(b"--")
                                })
                                .ok_or(USAGE)?,
                        );
                    }
                    _ => return Err(USAGE.into()),
                }
            }
            (Command::Summary { offset, out }, path)
        }
        _ => return Err(USAGE.into()),
    };

    let file =
        File::open(path).map_err(|error| format!("記録を開けません（{:?}）", error.kind()))?;
    let input = BufReader::new(file);
    if let Command::Summary {
        offset,
        out: Some(path),
    } = command
    {
        // Validate and render the recording before creating the output file.
        let mut report = Vec::new();
        summary::write_summary(input, &mut report, offset).map_err(|error| error.to_string())?;
        if let Err(error) = write_new_file(path, &report) {
            eprintln!("{}", output_error(error));
            return Ok(4);
        }
        return Ok(0);
    }
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
        Command::Summary { offset, .. } => {
            summary::write_summary(input, &mut output, offset)
                .map_err(|error| error.to_string())?;
        }
    }
    output.flush().map_err(output_error)?;
    Ok(0)
}

enum Command {
    Decode,
    Summary {
        offset: Option<UtcOffset>,
        out: Option<OsString>,
    },
}

fn write_new_file(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()
}

fn output_error(error: io::Error) -> String {
    format!("結果を出力できません（{:?}）", error.kind())
}
