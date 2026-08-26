use crate::errors::{ConflictStopped, SilentExit, StaxError, exit_codes};
use std::process::ExitCode;

pub fn run() -> ExitCode {
    ExitCode::from(exit_code(crate::cli::run()) as u8)
}

fn exit_code(result: anyhow::Result<()>) -> i32 {
    let Err(err) = result else {
        return exit_codes::SUCCESS;
    };

    if let Some(status) = err.downcast_ref::<SilentExit>() {
        return status.0;
    }

    if err.downcast_ref::<ConflictStopped>().is_some() {
        return exit_codes::CONFLICT;
    }

    if let Some(stax_err) = err.downcast_ref::<StaxError>() {
        eprintln!("Error: {stax_err}");
        return stax_err.exit_code();
    }

    eprintln!("Error: {err:#}");
    exit_codes::GENERAL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_maps_to_zero() {
        assert_eq!(exit_code(Ok(())), exit_codes::SUCCESS);
    }

    #[test]
    fn silent_exit_preserves_requested_code() {
        assert_eq!(
            exit_code(Err(SilentExit(exit_codes::VALIDATION).into())),
            exit_codes::VALIDATION
        );
    }

    #[test]
    fn conflict_stop_maps_to_conflict_code() {
        assert_eq!(exit_code(Err(ConflictStopped.into())), exit_codes::CONFLICT);
    }
}
