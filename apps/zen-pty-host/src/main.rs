use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[cfg(unix)]
fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(folder) = args.next().map(PathBuf::from) else { return ExitCode::from(2) };
    let mut idle = zen_terminal::host::DEFAULT_IDLE;
    while let Some(arg) = args.next() {
        match (arg.to_str(), args.next().and_then(|value| value.to_str()?.parse::<u64>().ok())) {
            (Some("--idle"), Some(seconds)) => idle = Duration::from_secs(seconds),
            _ => return ExitCode::from(2),
        }
    }
    match zen_terminal::host::serve(&folder, idle) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(not(unix))]
fn main() -> ExitCode {
    let _ = (PathBuf::new(), Duration::ZERO);
    ExitCode::from(2)
}
