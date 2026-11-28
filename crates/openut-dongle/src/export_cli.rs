use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::process::ExitCode;

use openut_dongle::anonymize::{ExportError, anonymize_recording};

pub fn run(args: impl Iterator<Item = OsString>) -> ExitCode {
    match export(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("{message}");
            ExitCode::from(code)
        }
    }
}

fn export(mut args: impl Iterator<Item = OsString>) -> Result<(), (u8, String)> {
    let usage = || {
        (
            1,
            "使い方\n  openut-dongle export --anonymize RECORD OUT [--map TSV]".to_owned(),
        )
    };
    if args.next().as_deref() != Some(OsStr::new("--anonymize")) {
        return Err(usage());
    }
    let input = args.next().ok_or_else(usage)?;
    let output = args.next().ok_or_else(usage)?;
    let mapping_path = match args.next().as_deref() {
        None => None,
        Some(option) if option == OsStr::new("--map") => Some(args.next().ok_or_else(usage)?),
        _ => return Err(usage()),
    };
    if args.next().is_some() {
        return Err(usage());
    }

    let input = File::open(input)
        .map_err(|error| (1, format!("記録を開けません（{:?}）", error.kind())))?;
    let mapping = mapping_path
        .map(fs::read_to_string)
        .transpose()
        .map_err(|error| (1, format!("置き換えの表を読めません（{:?}）", error.kind())))?;
    let exported =
        anonymize_recording(BufReader::new(input), mapping.as_deref()).map_err(|error| {
            let code = if matches!(error, ExportError::Residual) {
                5
            } else {
                1
            };
            (code, error.to_string())
        })?;

    // Verification finishes before the destination is opened, and existing files are retained.
    super::write_new_file(output, exported.as_bytes()).map_err(output_error)?;

    let mut console = io::stdout().lock();
    writeln!(console, "匿名化した記録を書き出しました。").map_err(output_error)?;
    for omission in exported.omissions() {
        writeln!(
            console,
            "0x{:02X} を {} 件省略しました（{}）",
            omission.report_type,
            omission.count,
            omission_reason(omission.reason),
        )
        .map_err(output_error)?;
    }
    if exported.skipped_rows() != 0 {
        writeln!(
            console,
            "読めない行や未知の行を {} 件省略しました。",
            exported.skipped_rows()
        )
        .map_err(output_error)?;
    }
    Ok(())
}

fn omission_reason(reason: openut_dongle::record::OmittedReason) -> &'static str {
    use openut_dongle::record::OmittedReason::*;
    match reason {
        NotAllowed => "書き出しの対象外",
        CheckFailed => "値が検査に不合格",
        TextForm => "文字列の形が不明",
        Malformed => "報告を解読できない",
    }
}

fn output_error(error: io::Error) -> (u8, String) {
    (4, format!("書き出せません（{:?}）", error.kind()))
}
