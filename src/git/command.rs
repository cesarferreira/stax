use std::path::Path;
use std::process::{Command, ExitStatus, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);
static COMMAND_COUNT: AtomicUsize = AtomicUsize::new(0);
static TOTAL_MICROS: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TraceGuard {
    enabled: bool,
    started_at: Instant,
}

impl TraceGuard {
    pub(crate) fn start(enabled: bool) -> Self {
        if enabled {
            COMMAND_COUNT.store(0, Ordering::Relaxed);
            TOTAL_MICROS.store(0, Ordering::Relaxed);
            TRACE_ENABLED.store(true, Ordering::Release);
        }
        Self {
            enabled,
            started_at: Instant::now(),
        }
    }
}

impl Drop for TraceGuard {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        TRACE_ENABLED.store(false, Ordering::Release);
        let count = COMMAND_COUNT.load(Ordering::Relaxed);
        let git_ms = TOTAL_MICROS.load(Ordering::Relaxed) as f64 / 1_000.0;
        let wall_ms = self.started_at.elapsed().as_secs_f64() * 1_000.0;
        eprintln!(
            "[trace] {count} git commands in {git_ms:.1}ms; command wall time {wall_ms:.1}ms"
        );
    }
}

pub(crate) fn output(cwd: &Path, args: &[&str]) -> std::io::Result<Output> {
    timed(cwd, args, || {
        Command::new("git").args(args).current_dir(cwd).output()
    })
}

pub(crate) fn status(cwd: &Path, args: &[&str]) -> std::io::Result<ExitStatus> {
    timed(cwd, args, || {
        Command::new("git").args(args).current_dir(cwd).status()
    })
}

fn timed<T>(
    cwd: &Path,
    args: &[&str],
    operation: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let started_at = Instant::now();
    let result = operation();
    if TRACE_ENABLED.load(Ordering::Acquire) {
        let elapsed = started_at.elapsed();
        let number = COMMAND_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        TOTAL_MICROS.fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
        let command = args
            .iter()
            .map(|arg| redact_arg(arg))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!(
            "[trace] git #{number} {:.1}ms @ {}: git {command}",
            elapsed.as_secs_f64() * 1_000.0,
            cwd.display()
        );
    }
    result
}

fn redact_arg(arg: &str) -> &str {
    if arg.contains("://") || arg.to_ascii_lowercase().contains("token=") {
        "<redacted>"
    } else {
        arg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_redacts_urls_and_tokens() {
        assert_eq!(redact_arg("https://token@example.com/repo"), "<redacted>");
        assert_eq!(redact_arg("token=secret"), "<redacted>");
        assert_eq!(redact_arg("feature/safe"), "feature/safe");
    }
}
