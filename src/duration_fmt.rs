//! Human-readable duration formatting shared by every command and the TUI.
//!
//! Durations under one minute stay in seconds. Longer durations split into
//! minutes and hours, so users read `2m 45s` instead of `165s`.

use std::time::Duration;

/// Format a whole number of seconds: `45s`, `2m 45s`, `2m`, `1h 5m`, `1h`.
pub(crate) fn format_secs(secs: u64) -> String {
    match secs {
        0..60 => format!("{}s", secs),
        60..3600 => {
            let (mins, rem) = (secs / 60, secs % 60);
            if rem == 0 {
                format!("{}m", mins)
            } else {
                format!("{}m {}s", mins, rem)
            }
        }
        _ => {
            let (hours, mins) = (secs / 3600, (secs % 3600) / 60);
            if mins == 0 {
                format!("{}h", hours)
            } else {
                format!("{}h {}m", hours, mins)
            }
        }
    }
}

/// Format a measured step duration. Under one minute it keeps millisecond
/// precision (`1.234s`); from one minute up it uses [`format_secs`].
pub(crate) fn format_elapsed(duration: Duration) -> String {
    // Round to milliseconds first so 59.9996s does not print as "60.000s".
    let millis = (duration.as_secs_f64() * 1000.0).round() as u64;
    if millis < 60_000 {
        format!("{}.{:03}s", millis / 1000, millis % 1000)
    } else {
        format_secs(duration.as_secs().max(60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secs_under_a_minute() {
        assert_eq!(format_secs(0), "0s");
        assert_eq!(format_secs(59), "59s");
    }

    #[test]
    fn secs_minutes() {
        assert_eq!(format_secs(60), "1m");
        assert_eq!(format_secs(165), "2m 45s");
        assert_eq!(format_secs(170), "2m 50s");
        assert_eq!(format_secs(3599), "59m 59s");
    }

    #[test]
    fn secs_hours() {
        assert_eq!(format_secs(3600), "1h");
        assert_eq!(format_secs(3900), "1h 5m");
        assert_eq!(format_secs(7325), "2h 2m");
    }

    #[test]
    fn elapsed_keeps_millis_under_a_minute() {
        assert_eq!(format_elapsed(Duration::ZERO), "0.000s");
        assert_eq!(format_elapsed(Duration::from_millis(1234)), "1.234s");
        assert_eq!(format_elapsed(Duration::from_millis(59_999)), "59.999s");
        assert_eq!(format_elapsed(Duration::from_micros(59_999_600)), "1m");
    }

    #[test]
    fn elapsed_switches_to_minutes_at_sixty_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(60)), "1m");
        assert_eq!(format_elapsed(Duration::from_millis(165_776)), "2m 45s");
        assert_eq!(format_elapsed(Duration::from_secs(3725)), "1h 2m");
    }
}
