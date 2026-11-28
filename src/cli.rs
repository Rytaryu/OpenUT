use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{self, Path, PathBuf};

use openut::{
    driver_reg::{self, DriverAction},
    run,
};
use openut_anonymize::{Batch, anonymize};
use openut_hublog::decode::decode_utf16le_text;

mod watcher;

const MAP_NAME: &str = "openut-anonymize-map.tsv";
const USAGE: &str = "openut anonymize --out-dir DIR --map-dir DIR [--pc-name NAME] LOG...";
const RUN_USAGE: &str = "openut run [--config FILE]";
const DRIVER_USAGE: &str = "openut driver register | openut driver unregister";

pub(crate) fn dispatch(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("anonymize") => run_anonymize(args),
        Some("watch") => watcher::watch(args),
        Some("replay") => watcher::replay(args),
        Some("summarize") => watcher::summarize(args),
        Some("run") => match args.next().as_deref() {
            None => run::run(None),
            Some(flag) if (flag == "--help" || flag == "-h") && args.next().is_none() => {
                println!("{RUN_USAGE}");
                Ok(())
            }
            Some(flag) if flag == "--config" => {
                let path = args.next().filter(|value| {
                    value
                        .to_str()
                        .is_some_and(|text| !text.is_empty() && !text.starts_with("--"))
                });
                match path {
                    Some(path) if args.next().is_none() => run::run(Some(path.into())),
                    _ => Err(RUN_USAGE.to_owned()),
                }
            }
            _ => Err(RUN_USAGE.to_owned()),
        },
        Some("driver") => {
            let action = match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
                Some("register") => DriverAction::Register,
                Some("unregister") => DriverAction::Unregister,
                Some("--help" | "-h") if args.next().is_none() => {
                    println!("{DRIVER_USAGE}");
                    return Ok(());
                }
                _ => return Err(DRIVER_USAGE.to_owned()),
            };
            if args.next().is_some() {
                return Err(DRIVER_USAGE.to_owned());
            }
            driver_reg::run(action).map_err(|error| error.to_string())?;
            let result = match action {
                DriverAction::Register => "OpenUT のドライバーを SteamVR に登録しました。",
                DriverAction::Unregister => "OpenUT のドライバーを SteamVR から解除しました。",
            };
            println!("{result}");
            Ok(())
        }
        Some("--help" | "-h") | None => {
            println!(
                "OpenUT {}\n{RUN_USAGE}\n{DRIVER_USAGE}\n{USAGE}\n{}\n{}\n{}\nopenut --version",
                openut::VERSION,
                watcher::WATCH_USAGE,
                watcher::REPLAY_USAGE,
                watcher::SUMMARIZE_USAGE,
            );
            Ok(())
        }
        _ => {
            println!("OpenUT {}", openut::VERSION);
            Ok(())
        }
    }
}

struct Input {
    path: PathBuf,
    name: String,
}

struct Options {
    output_dir: PathBuf,
    map_dir: PathBuf,
    pc_name: Option<String>,
    inputs: Vec<Input>,
}

pub(crate) fn run_anonymize(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let options = parse_options(args)?;
    let pc_name = options
        .pc_name
        .clone()
        .or_else(local_pc_name)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            "有効な PC 名を取得できませんでした。採取元の PC 名を --pc-name に指定してください"
                .to_owned()
        })?;
    let mut decoded = Vec::with_capacity(options.inputs.len());
    for (index, input) in options.inputs.iter().enumerate() {
        let number = index + 1;
        let bytes = fs::read(&input.path).map_err(|error| {
            format!(
                "ファイル {number} を読み込めませんでした（{}）",
                error.kind()
            )
        })?;
        decoded.push(
            decode_utf16le_text(&bytes)
                .map_err(|error| format!("ファイル {number} を解読できませんでした（{error}）"))?,
        );
    }
    let logs: Vec<_> = decoded.iter().map(String::as_str).collect();
    let batch = anonymize(&logs, Some(&pc_name)).map_err(|error| error.to_string())?;
    drop(decoded);

    let (output_dir, map_dir) = prepare_directories(&options)?;
    write_batch(&options.inputs, &output_dir, &map_dir, &batch)?;
    for (index, log) in batch.outputs.iter().enumerate() {
        for warning in &log.warnings {
            eprintln!(
                "警告 ファイル {} の記録 {} に未確認の識別情報の候補があります（{}）",
                index + 1,
                warning.record,
                warning.kind
            );
        }
    }
    Ok(())
}

fn parse_options(mut args: impl Iterator<Item = OsString>) -> Result<Options, String> {
    let mut output_dir = None;
    let mut map_dir = None;
    let mut pc_name = None;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out-dir") if output_dir.is_none() => {
                output_dir = Some(PathBuf::from(option_value(&mut args)?));
            }
            Some("--map-dir") if map_dir.is_none() => {
                map_dir = Some(PathBuf::from(option_value(&mut args)?));
            }
            Some("--pc-name") if pc_name.is_none() => {
                pc_name = Some(
                    option_value(&mut args)?
                        .into_string()
                        .map_err(|_| "PC 名を文字列として読み込めませんでした".to_owned())?,
                );
            }
            Some("--") => {
                paths.extend(args.map(PathBuf::from));
                break;
            }
            Some(option) if option.starts_with('-') => {
                return Err(format!("未知のオプションまたは重複した指定です。{USAGE}"));
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let (Some(output_dir), Some(map_dir)) = (output_dir, map_dir) else {
        return Err(format!(
            "匿名化ログと対応表の保存先を指定してください。{USAGE}"
        ));
    };
    if paths.is_empty() {
        return Err(format!("入力ログを指定してください。{USAGE}"));
    }
    let mut seen = BTreeSet::new();
    let mut inputs = Vec::with_capacity(paths.len());
    for (index, path) in paths.into_iter().enumerate() {
        let number = index + 1;
        let name = canonical_log_name(&path).ok_or_else(|| {
            format!("ファイル {number} の名前が VIVE Hub の対象ログの形式と違います")
        })?;
        if !seen.insert(name.to_owned()) {
            return Err(format!(
                "ファイル {number} の名前が別の入力ログと重複しています"
            ));
        }
        inputs.push(Input {
            name: name.to_owned(),
            path,
        });
    }
    Ok(Options {
        output_dir,
        map_dir,
        pc_name,
        inputs,
    })
}

fn option_value(args: &mut impl Iterator<Item = OsString>) -> Result<OsString, String> {
    args.next()
        .filter(|value| {
            !value.is_empty() && !value.to_str().is_some_and(|value| value.starts_with("--"))
        })
        .ok_or_else(|| format!("オプションの値を指定してください。{USAGE}"))
}

fn canonical_log_name(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let (prefix, suffix) = name.split_once('_')?;
    let day = suffix.strip_suffix(".txt")?;
    (matches!(
        prefix,
        "ViveTrackerServer" | "ViveTrackerDriver" | "ViveTrackerDongle"
    ) && day.len() == 8
        && day.bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(name)
}

fn local_pc_name() -> Option<String> {
    env::var(if cfg!(windows) {
        "COMPUTERNAME"
    } else {
        "HOSTNAME"
    })
    .ok()
}

fn prepare_directories(options: &Options) -> Result<(PathBuf, PathBuf), String> {
    let output_dir = path::absolute(&options.output_dir).map_err(|error| {
        format!(
            "匿名化ログの保存先を解決できませんでした（{}）",
            error.kind()
        )
    })?;
    let map_dir = path::absolute(&options.map_dir)
        .map_err(|error| format!("対応表の保存先を解決できませんでした（{}）", error.kind()))?;
    check_separate(&output_dir, &map_dir)?;

    fs::create_dir_all(&output_dir)
        .map_err(|error| format!("匿名化ログの保存先を作れませんでした（{}）", error.kind()))?;
    fs::create_dir_all(&map_dir)
        .map_err(|error| format!("対応表の保存先を作れませんでした（{}）", error.kind()))?;
    let output_dir = fs::canonicalize(output_dir).map_err(|error| {
        format!(
            "匿名化ログの保存先を解決できませんでした（{}）",
            error.kind()
        )
    })?;
    let map_dir = fs::canonicalize(map_dir)
        .map_err(|error| format!("対応表の保存先を解決できませんでした（{}）", error.kind()))?;
    check_separate(&output_dir, &map_dir)?;
    Ok((output_dir, map_dir))
}

fn check_separate(output_dir: &Path, map_dir: &Path) -> Result<(), String> {
    if is_within(map_dir, output_dir) {
        return Err("対応表の保存先を匿名化ログの保存先の外に指定してください".to_owned());
    }
    Ok(())
}

#[cfg(not(windows))]
fn is_within(path: &Path, parent: &Path) -> bool {
    path.starts_with(parent)
}

#[cfg(windows)]
fn is_within(path: &Path, parent: &Path) -> bool {
    let mut components = path.components();
    parent.components().all(|part| {
        components.next().is_some_and(|component| {
            component.as_os_str().to_string_lossy().to_lowercase()
                == part.as_os_str().to_string_lossy().to_lowercase()
        })
    })
}

fn write_batch(
    inputs: &[Input],
    output_dir: &Path,
    map_dir: &Path,
    batch: &Batch,
) -> Result<(), String> {
    let output_paths: Vec<_> = inputs
        .iter()
        .map(|input| output_dir.join(&input.name))
        .collect();
    let map_path = map_dir.join(MAP_NAME);
    for (index, output_path) in output_paths.iter().enumerate() {
        check_absent(output_path).map_err(|error| output_error(index + 1, &error))?;
    }
    check_absent(&map_path).map_err(|error| map_error(&error))?;

    let mut created = Vec::new();
    let result = (|| {
        for (index, (output_path, log)) in output_paths.iter().zip(&batch.outputs).enumerate() {
            let mut bytes = vec![0xff, 0xfe];
            bytes.extend(log.text.encode_utf16().flat_map(u16::to_le_bytes));
            write_new(output_path, &bytes, false, &mut created)
                .map_err(|error| output_error(index + 1, &error))?;
        }
        write_new(
            &map_path,
            batch.mapping_tsv().as_bytes(),
            true,
            &mut created,
        )
        .map_err(|error| map_error(&error))
    })();
    if let Err(error) = result {
        let mut cleanup_failed = false;
        for path in created.iter().rev() {
            cleanup_failed |= fs::remove_file(path).is_err();
        }
        if cleanup_failed {
            return Err(format!(
                "{error}。作成途中のファイルを一部削除できませんでした"
            ));
        }
        return Err(error);
    }
    Ok(())
}

fn check_absent(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn write_new(
    path: &Path,
    bytes: &[u8],
    private: bool,
    created: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;

    let mut file = options.open(path)?;
    created.push(path.to_path_buf());
    file.write_all(bytes)
}

fn output_error(number: usize, error: &io::Error) -> String {
    format!(
        "ファイル {number} の匿名化ログを書き出せませんでした（{}）",
        error.kind()
    )
}

fn map_error(error: &io::Error) -> String {
    format!("対応表を書き出せませんでした（{}）", error.kind())
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn malformed_subcommands_stop_before_opening_hardware_or_registering() {
        for args in [
            ["run", "--config", "--unknown"],
            ["run", "extra", "argument"],
        ] {
            assert_eq!(
                dispatch(args.into_iter().map(OsString::from)),
                Err(RUN_USAGE.to_owned())
            );
        }
        for args in [
            ["driver", "register", "extra"],
            ["driver", "unregister", "extra"],
        ] {
            assert_eq!(
                dispatch(args.into_iter().map(OsString::from)),
                Err(DRIVER_USAGE.to_owned())
            );
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn run_rejects_non_unicode_arguments_before_entering_the_runtime() {
        #[cfg(unix)]
        let invalid = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0xff])
        };
        #[cfg(windows)]
        let invalid = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xd800])
        };

        for tail in [
            vec![invalid.clone()],
            vec![invalid.clone(), OsString::from("extra")],
            vec![OsString::from("--config"), invalid],
        ] {
            let args = std::iter::once(OsString::from("run")).chain(tail);
            assert_eq!(dispatch(args), Err(RUN_USAGE.to_owned()));
        }
    }
}
