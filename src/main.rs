use std::process::ExitCode;

mod cli;

fn main() -> ExitCode {
    match cli::dispatch(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
