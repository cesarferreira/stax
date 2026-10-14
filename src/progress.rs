use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

const DETAIL_MAX_CHARS: usize = 72;

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
    detail: Arc<Mutex<String>>,
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    output: TimerOutput,
}

/// Shared status line drawn under a [`LiveTimer`] while it is spinning.
#[derive(Clone)]
pub struct LiveDetail {
    text: Arc<Mutex<String>>,
}

impl LiveDetail {
    pub fn set(&self, detail: &str) {
        *lock_detail(&self.text) = sanitize_progress_detail(detail);
    }
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
        let detail = Arc::new(Mutex::new(String::new()));
        let detail_for_thread = Arc::clone(&detail);

        let thread = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                let elapsed = bar_clone.elapsed();
                let time_str = format!("{:.3}s", elapsed.as_secs_f64());
                let detail_text = lock_detail(&detail_for_thread).clone();
                let detail_rendered = if detail_text.is_empty() {
                    String::new()
                } else {
                    detail_text.dimmed().to_string()
                };
                bar_clone.set_message(format_timer_message(
                    &label,
                    &time_str.dimmed().to_string(),
                    &detail_rendered,
                ));
                std::thread::sleep(Duration::from_millis(50));
            }
        });

        bar.enable_steady_tick(Duration::from_millis(100));

        Self {
            bar,
            message: message.to_string(),
            detail,
            stop_flag,
            thread: Some(thread),
            output,
        }
    }

    /// Handle for updating the child line under this timer from another thread.
    pub fn detail_handle(&self) -> LiveDetail {
        LiveDetail {
            text: Arc::clone(&self.detail),
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

impl Drop for LiveTimer {
    fn drop(&mut self) {
        self.stop_thread();
        self.bar.finish_and_clear();
    }
}

/// Spinner row, plus an indented child when `detail` is non-empty.
///
/// The child is part of the live redraw and is cleared when the timer finishes.
pub(crate) fn format_timer_message(label: &str, elapsed: &str, detail: &str) -> String {
    let headline = format!("{label:<35} {elapsed}");
    let detail = detail.trim();
    if detail.is_empty() {
        headline
    } else {
        format!("{headline}\n    └─ {detail}")
    }
}

/// Collapse a live status line onto one terminal row.
pub(crate) fn sanitize_progress_detail(detail: &str) -> String {
    let collapsed = detail
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.chars().count() <= DETAIL_MAX_CHARS {
        return collapsed;
    }
    let mut truncated = collapsed
        .chars()
        .take(DETAIL_MAX_CHARS.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn lock_detail(detail: &Mutex<String>) -> std::sync::MutexGuard<'_, String> {
    detail.lock().unwrap_or_else(|err| err.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{LiveTimer, format_timer_message, lock_detail};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    #[test]
    fn dropping_timer_stops_its_worker_thread() {
        let timer = LiveTimer::new("Testing cancellation...");
        let stop_flag = Arc::clone(&timer.stop_flag);

        drop(timer);

        assert!(
            stop_flag.load(Ordering::Relaxed),
            "dropping a live timer must signal its worker thread to stop"
        );
    }

    #[test]
    fn timer_message_is_one_row_without_detail() {
        assert_eq!(
            format_timer_message("Restack feature", "65.776s", ""),
            "Restack feature                     65.776s"
        );
    }

    #[test]
    fn timer_message_adds_an_indented_child_line() {
        assert_eq!(
            format_timer_message(
                "Restack feature",
                "65.776s",
                "Rebasing commit 12/31 · Fix Kotlin compiler warnings"
            ),
            "Restack feature                     65.776s\n    └─ Rebasing commit 12/31 · Fix Kotlin compiler warnings"
        );
    }

    #[test]
    fn detail_handle_stores_a_single_sanitized_line() {
        let timer = LiveTimer::new("Restack feature");
        timer.detail_handle().set(
            "Rebasing commit 1/2 · hello\nworld\u{0007} and then a very long subject that should not wrap the live progress row",
        );

        let stored = lock_detail(&timer.detail).clone();
        assert_eq!(
            stored,
            "Rebasing commit 1/2 · hello world and then a very long subject that s..."
        );
        assert!(stored.chars().count() <= 72);
        drop(timer);
    }
}
