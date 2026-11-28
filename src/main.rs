use std::ffi::OsStr;
use std::process::ExitCode;

mod cli;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(OsStr::new("anonymize")) {
        if let Err(error) = cli::run_anonymize(args) {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    } else {
        println!("OpenUT {}", openut::VERSION);
    }
    ExitCode::SUCCESS
}
