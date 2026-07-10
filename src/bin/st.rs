use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn stax_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|parent| parent.join(stax_executable_name()))
        })
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(stax_executable_name()))
}

fn stax_executable_name() -> String {
    format!("stax{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(unix)]
fn main() -> ExitCode {
    use std::os::unix::process::CommandExt;

    let error = Command::new(stax_binary())
        .args(std::env::args_os().skip(1))
        .exec();
    eprintln!("Error: failed to launch stax: {error}");
    ExitCode::FAILURE
}

#[cfg(not(unix))]
fn main() -> ExitCode {
    match Command::new(stax_binary())
        .args(std::env::args_os().skip(1))
        .status()
    {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("Error: failed to launch stax: {error}");
            ExitCode::FAILURE
        }
    }
}
