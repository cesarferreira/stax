use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// A live spinning timer that updates in-place while a long-running operation runs.
///
/// Usage:
/// ```ignore
/// let timer = LiveTimer::new("Fetching from origin...");
/// // ... do work ...
/// timer.finish_ok("done");
/// ```
pub struct LiveTimer {
    bar: ProgressBar,
    message: String,
}

impl LiveTimer {
    pub fn new(message: &str) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("  {spinner:.cyan} {msg} {elapsed:.dim}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        bar.set_message(message.to_string());
        bar.enable_steady_tick(Duration::from_millis(100));

        Self {
            bar,
            message: message.to_string(),
        }
    }

    /// Create a timer only when `enabled` is true; returns None otherwise.
    /// Use this to respect the `quiet` flag without conditional code at every call site.
    pub fn maybe_new(enabled: bool, message: &str) -> Option<Self> {
        if enabled {
            Some(Self::new(message))
        } else {
            None
        }
    }

    /// Finish with a green suffix.
    pub fn finish_ok(self, suffix: &str) {
        self.bar.finish_and_clear();
        println!("  {} {}", self.message, suffix.green());
    }

    /// Finish by printing the step label and its elapsed time as a timed table row:
    /// `  step label                        1.234s`
    pub fn finish_timed(self) {
        let elapsed = self.bar.elapsed();
        self.bar.finish_and_clear();
        let time_str = format!("{:.3}s", elapsed.as_secs_f64());
        println!("  {:<35} {}", self.message, time_str.dimmed());
    }

    /// Finish with a yellow suffix (partial success / warning).
    pub fn finish_warn(self, suffix: &str) {
        self.bar.finish_and_clear();
        println!("  {} {}", self.message, suffix.yellow());
    }

    /// Finish with a red suffix (failure).
    pub fn finish_err(self, suffix: &str) {
        self.bar.finish_and_clear();
        println!("  {} {}", self.message, suffix.red());
    }

    // --- Option<LiveTimer> helpers ---

    pub fn maybe_finish_ok(timer: Option<Self>, suffix: &str) {
        if let Some(t) = timer {
            t.finish_ok(suffix);
        }
    }

    pub fn maybe_finish_timed(timer: Option<Self>) {
        if let Some(t) = timer {
            t.finish_timed();
        }
    }

    pub fn maybe_finish_warn(timer: Option<Self>, suffix: &str) {
        if let Some(t) = timer {
            t.finish_warn(suffix);
        }
    }

    pub fn maybe_finish_err(timer: Option<Self>, suffix: &str) {
        if let Some(t) = timer {
            t.finish_err(suffix);
        }
    }
}
