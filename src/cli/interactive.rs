use std::io::IsTerminal;
use std::time::Duration;

pub(crate) fn detect_interactive_stdio() -> (bool, bool) {
    #[cfg(debug_assertions)]
    if std::env::var_os("STAX_TEST_FORCE_INTERACTIVE_TERMINAL").is_some() {
        // Integration tests use this to drive the interactive fallback path without a real PTY.
        return (true, true);
    }

    (
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )
}

pub(crate) fn has_interactive_terminal(stdin_is_terminal: bool, stdout_is_terminal: bool) -> bool {
    stdin_is_terminal && stdout_is_terminal
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InteractiveTerminalCheck {
    pub(crate) available: bool,
    pub(crate) reason: Option<String>,
}

pub(crate) fn check_interactive_terminal(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> InteractiveTerminalCheck {
    check_interactive_terminal_with_probe(stdin_is_terminal, stdout_is_terminal, || {
        #[cfg(debug_assertions)]
        if let Ok(reason) = std::env::var("STAX_TEST_FORCE_INPUT_READER_FAILURE") {
            // Integration tests use this to exercise the interactive fallback path deterministically.
            return Err(reason);
        }

        crossterm::event::poll(Duration::from_millis(0))
            .map(|_| ())
            .map_err(|err| err.to_string())
    })
}

pub(crate) fn check_interactive_terminal_with_probe<F>(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    probe_input_reader: F,
) -> InteractiveTerminalCheck
where
    F: FnOnce() -> std::result::Result<(), String>,
{
    if !has_interactive_terminal(stdin_is_terminal, stdout_is_terminal) {
        return InteractiveTerminalCheck {
            available: false,
            reason: None,
        };
    }

    match probe_input_reader() {
        Ok(()) => InteractiveTerminalCheck {
            available: true,
            reason: None,
        },
        Err(reason) => InteractiveTerminalCheck {
            available: false,
            reason: Some(reason),
        },
    }
}

pub(crate) fn print_interactive_fallback(reason: Option<&str>, dashboard: &str, fallback: &str) {
    if let Some(reason) = reason {
        eprintln!(
            "stax: interactive {} unavailable ({}); {}.",
            dashboard, reason, fallback
        );
    }
}
