use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

#[derive(Clone, Copy)]
enum TimerOutput {
    Stdout,
    Stderr,
}

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
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    output: TimerOutput,
}

impl LiveTimer {
    pub fn new(message: &str) -> Self {
        Self::new_with_output(message, TimerOutput::Stdout)
    }

    pub fn new_stderr(message: &str) -> Self {
        Self::new_with_output(message, TimerOutput::Stderr)
    }

    fn new_with_output(message: &str, output: TimerOutput) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("  {spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let bar_clone = bar.clone();
        let label = message.to_string();

        let thread = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                let elapsed = bar_clone.elapsed();
                let time_str = format!("{:.3}s", elapsed.as_secs_f64());
                bar_clone.set_message(format!("{:<35} {}", label, time_str.dimmed()));
                std::thread::sleep(Duration::from_millis(50));
            }
        });

        bar.enable_steady_tick(Duration::from_millis(100));

        Self {
            bar,
            message: message.to_string(),
            stop_flag,
            thread: Some(thread),
            output,
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

    /// Create a timer that prints its completion row on stderr.
    pub fn maybe_new_stderr(enabled: bool, message: &str) -> Option<Self> {
        if enabled {
            Some(Self::new_stderr(message))
        } else {
            None
        }
    }

    fn stop_thread(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    fn finish_line(&self, line: String) {
        match self.output {
            TimerOutput::Stdout => println!("{}", line),
            TimerOutput::Stderr => eprintln!("{}", line),
        }
    }

    /// Finish with a green suffix.
    pub fn finish_ok(mut self, suffix: &str) {
        let elapsed = self.bar.elapsed();
        self.stop_thread();
        self.bar.finish_and_clear();
        let time_str = format!("{:.3}s", elapsed.as_secs_f64());
        self.finish_line(format!(
            "  {} {:<35} {} {}",
            "✓".green(),
            self.message,
            suffix.green(),
            time_str.dimmed()
        ));
    }

    /// Finish by printing a check mark, the step label, and its elapsed time as a timed table row:
    /// `  ✓ step label                      1.234s`
    pub fn finish_timed(mut self) {
        let elapsed = self.bar.elapsed();
        self.stop_thread();
        self.bar.finish_and_clear();
        let time_str = format!("{:.3}s", elapsed.as_secs_f64());
        self.finish_line(format!(
            "  {} {:<35} {}",
            "✓".green(),
            self.message,
            time_str.dimmed()
        ));
    }

    /// Finish as skipped/deferred — tabular row with a `○` icon and dimmed reason.
    /// `  ○ Step label                       reason`
    pub fn finish_skipped(mut self, reason: &str) {
        self.stop_thread();
        self.bar.finish_and_clear();
        self.finish_line(format!(
            "  {} {:<35} {}",
            "○".dimmed(),
            self.message,
            reason.dimmed()
        ));
    }

    /// Finish with a yellow suffix (partial success / warning).
    pub fn finish_warn(mut self, suffix: &str) {
        self.stop_thread();
        self.bar.finish_and_clear();
        self.finish_line(format!(
            "  {} {:<35} {}",
            "⚠".yellow(),
            self.message,
            suffix.yellow()
        ));
    }

    /// Finish with a red suffix (failure).
    pub fn finish_err(mut self, suffix: &str) {
        self.stop_thread();
        self.bar.finish_and_clear();
        self.finish_line(format!("  {} {}", self.message, suffix.red()));
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

    pub fn maybe_finish_skipped(timer: Option<Self>, reason: &str) {
        if let Some(t) = timer {
            t.finish_skipped(reason);
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
