//! One-time "What's new" notice shown on the first run after an upgrade.
//!
//! The release's `CHANGELOG.md` is embedded at build time (git-cliff regenerates
//! it inside the release commit, so it always contains the version being built).
//! The last version the user saw is stored in `<config dir>/last_seen_version`.

use crate::config::Config;
use colored::Colorize;
use regex::Regex;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::sync::LazyLock;

const CHANGELOG: &str = include_str!("../CHANGELOG.md");
const STATE_FILE: &str = "last_seen_version";
const CHANGELOG_URL: &str = "https://github.com/cesarferreira/stax/blob/main/CHANGELOG.md";
const MAX_RELEASES: usize = 3;
const MAX_ITEMS: usize = 12;

type Version = (u64, u64, u64);

/// What to do once an upgrade is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Print the notice and record the new version.
    Show,
    /// The user turned the notice off: record the new version silently.
    Suppress,
    /// Nobody would see it (no terminal): keep the old version so a later
    /// interactive run still shows the notice.
    Defer,
}

/// Print the notice to stderr if this is the first run after an upgrade.
pub fn show_if_upgraded() {
    let Ok(dir) = Config::dir() else {
        return;
    };
    let notice = check(
        &dir.join(STATE_FILE),
        env!("CARGO_PKG_VERSION"),
        CHANGELOG,
        || {
            if !Config::load_global()
                .map(|c| c.ui.whats_new)
                .unwrap_or(true)
            {
                Gate::Suppress
            } else if std::io::stderr().is_terminal() {
                Gate::Show
            } else {
                Gate::Defer
            }
        },
    );
    if let Some(notice) = notice {
        eprint!("{notice}");
    }
}

/// Compare the recorded version with `current`, update the record, and return
/// the notice to print, if any. `gate` is only consulted after an upgrade, so
/// the common path costs one small file read.
fn check(
    state_path: &Path,
    current: &str,
    changelog: &str,
    gate: impl FnOnce() -> Gate,
) -> Option<String> {
    let current_version = parse_version(current)?;
    let seen = fs::read_to_string(state_path)
        .ok()
        .and_then(|s| parse_version(s.trim()));

    match seen {
        // Fresh install (or unreadable record): nothing to compare against.
        None => {
            record(state_path, current);
            None
        }
        Some(seen) if seen == current_version => None,
        // Downgrade: re-baseline so the next upgrade shows the right range.
        Some(seen) if seen > current_version => {
            record(state_path, current);
            None
        }
        Some(seen) => match gate() {
            Gate::Defer => None,
            Gate::Suppress => {
                record(state_path, current);
                None
            }
            Gate::Show => {
                record(state_path, current);
                render(changelog, seen, current_version, current)
            }
        },
    }
}

fn record(state_path: &Path, current: &str) {
    if let Some(parent) = state_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(state_path, format!("{current}\n"));
}

/// Parse `X.Y.Z`, ignoring any pre-release or build suffix.
fn parse_version(s: &str) -> Option<Version> {
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

struct Release {
    version: String,
    items: Vec<String>,
}

/// Collect releases in `(seen, current]`, newest first, without release chores.
fn releases_between(changelog: &str, seen: Version, current: Version) -> Vec<Release> {
    static HEADING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^## \[([^\]]+)\]").expect("valid regex"));

    let mut releases = Vec::new();
    let mut in_range = false;
    let mut skip_section = false;

    for line in changelog.lines() {
        let line = line.trim_end();
        if let Some(caps) = HEADING.captures(line) {
            let version = &caps[1];
            in_range = parse_version(version).is_some_and(|v| v > seen && v <= current);
            if in_range {
                releases.push(Release {
                    version: version.to_string(),
                    items: Vec::new(),
                });
            }
            skip_section = false;
        } else if !in_range {
            continue;
        } else if let Some(section) = line.strip_prefix("### ") {
            skip_section = section.contains("Miscellaneous");
        } else if let Some(item) = line.strip_prefix("- ")
            && !skip_section
            && let Some(release) = releases.last_mut()
        {
            let item = clean_item(item);
            if !item.is_empty() && item != "Release" {
                release.items.push(item);
            }
        }
    }

    releases.retain(|r| !r.items.is_empty());
    releases
}

/// Turn markdown links into their text and `*(scope)* Msg` into `scope: Msg`.
fn clean_item(item: &str) -> String {
    static LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").expect("valid regex"));
    static SCOPE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\*\(([^)]+)\)\*\s*").expect("valid regex"));

    let item = LINK.replace_all(item.trim(), "$1");
    SCOPE.replace(&item, "$1: ").into_owned()
}

fn render(changelog: &str, seen: Version, current: Version, current_str: &str) -> Option<String> {
    let releases = releases_between(changelog, seen, current);
    if releases.is_empty() {
        return None;
    }

    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "{} {}\n",
        "What's new in stax".bold(),
        current_str.green().bold()
    ));

    let shown_releases = &releases[..releases.len().min(MAX_RELEASES)];
    let mut shown = 0;
    let mut hidden = 0;
    for release in shown_releases {
        let room = MAX_ITEMS - shown;
        if room == 0 {
            hidden += release.items.len();
            continue;
        }
        if releases.len() > 1 {
            out.push_str(&format!("  {}\n", release.version.cyan()));
        }
        for item in release.items.iter().take(room) {
            out.push_str(&format!("  • {item}\n"));
            shown += 1;
        }
        hidden += release.items.len().saturating_sub(room);
    }

    let older = releases.len() - shown_releases.len();
    let mut more = Vec::new();
    if hidden > 0 {
        more.push(format!("{hidden} more"));
    }
    if older > 0 {
        let plural = if older == 1 { "" } else { "s" };
        more.push(format!("{older} earlier release{plural}"));
    }
    if !more.is_empty() {
        out.push_str(&format!(
            "  {}\n",
            format!("…and {}", more.join(", plus ")).dimmed()
        ));
    }
    out.push_str(&format!(
        "  {} {}\n",
        "Full changelog:".dimmed(),
        CHANGELOG_URL.dimmed()
    ));
    out.push_str(&format!(
        "  {}\n",
        "Hide these notes: set `whats_new = false` under [ui] in your stax config".dimmed()
    ));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
# Changelog

## [0.3.0] - 2026-11-20

### 💼 Other

- Add `stax history` ([#948](https://github.com/cesarferreira/stax/issues/948))

### ⚙️ Miscellaneous Tasks

- Release
## [0.2.0] - 2026-11-14

### 🚀 Features

- *(wt)* Add unlock action
## [0.1.0] - 2026-11-10

### 🐛 Bug Fixes

- Old fix
";

    fn plain(s: Option<String>) -> Option<String> {
        s.map(|s| String::from_utf8(strip_ansi(s.as_bytes())).unwrap())
    }

    fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
        let re = regex::bytes::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(bytes, &b""[..]).into_owned()
    }

    fn state_with(version: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        if let Some(v) = version {
            fs::write(&path, format!("{v}\n")).unwrap();
        }
        (dir, path)
    }

    fn recorded(path: &Path) -> String {
        fs::read_to_string(path).unwrap().trim().to_string()
    }

    #[test]
    fn fresh_install_records_version_silently() {
        let (_dir, path) = state_with(None);
        let out = check(&path, "0.3.0", FIXTURE, || panic!("gate not consulted"));
        assert_eq!(out, None);
        assert_eq!(recorded(&path), "0.3.0");
    }

    #[test]
    fn same_version_shows_nothing() {
        let (_dir, path) = state_with(Some("0.3.0"));
        assert_eq!(check(&path, "0.3.0", FIXTURE, || panic!("no gate")), None);
    }

    #[test]
    fn upgrade_shows_each_release_since_last_seen_once() {
        let (_dir, path) = state_with(Some("0.1.0"));
        let out = plain(check(&path, "0.3.0", FIXTURE, || Gate::Show)).unwrap();

        assert!(out.contains("What's new in stax 0.3.0"), "{out}");
        assert!(out.contains("• Add `stax history` (#948)"), "{out}");
        assert!(out.contains("• wt: Add unlock action"), "{out}");
        assert!(!out.contains("Old fix"), "{out}");
        assert!(!out.contains("Release\n"), "{out}");
        assert_eq!(recorded(&path), "0.3.0");

        assert_eq!(check(&path, "0.3.0", FIXTURE, || Gate::Show), None);
    }

    #[test]
    fn non_interactive_run_defers_the_notice() {
        let (_dir, path) = state_with(Some("0.2.0"));
        assert_eq!(check(&path, "0.3.0", FIXTURE, || Gate::Defer), None);
        assert_eq!(recorded(&path), "0.2.0");
    }

    #[test]
    fn disabled_notice_records_without_printing() {
        let (_dir, path) = state_with(Some("0.2.0"));
        assert_eq!(check(&path, "0.3.0", FIXTURE, || Gate::Suppress), None);
        assert_eq!(recorded(&path), "0.3.0");
    }

    #[test]
    fn downgrade_rebaselines_silently() {
        let (_dir, path) = state_with(Some("0.3.0"));
        assert_eq!(check(&path, "0.2.0", FIXTURE, || panic!("no gate")), None);
        assert_eq!(recorded(&path), "0.2.0");
    }

    #[test]
    fn upgrade_without_changelog_entries_records_and_prints_nothing() {
        let (_dir, path) = state_with(Some("0.3.0"));
        assert_eq!(check(&path, "0.4.0", FIXTURE, || Gate::Show), None);
        assert_eq!(recorded(&path), "0.4.0");
    }

    #[test]
    fn long_changelogs_are_truncated() {
        let mut changelog = String::new();
        for minor in (1..=5).rev() {
            changelog.push_str(&format!("## [0.{minor}.0] - 2026-01-01\n\n### Other\n\n"));
            for i in 0..5 {
                changelog.push_str(&format!("- Change {minor}.{i}\n"));
            }
        }
        let out = plain(render(&changelog, (0, 0, 0), (0, 5, 0), "0.5.0")).unwrap();
        assert_eq!(out.matches("• ").count(), MAX_ITEMS, "{out}");
        assert!(
            out.contains("…and 3 more, plus 2 earlier releases"),
            "{out}"
        );
        assert!(!out.contains("0.2.0"), "{out}");
    }

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("0.121.0"), Some((0, 121, 0)));
        assert_eq!(parse_version("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("unreleased"), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn embedded_changelog_has_current_release_shape() {
        assert!(CHANGELOG.lines().any(|l| l.starts_with("## [")));
    }
}
