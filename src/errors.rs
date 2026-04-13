/// Sentinel error returned when a rebase stops on conflict.
///
/// This is not a "real" error — the conflict information has already been
/// printed. The sentinel propagates through `anyhow::Result` so that
/// `cli::run()` can intercept it and exit with code 1 without printing
/// an additional `Error: …` line.
#[derive(Debug)]
pub struct ConflictStopped;

impl std::fmt::Display for ConflictStopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stopped on rebase conflict")
    }
}

impl std::error::Error for ConflictStopped {}
