use crate::git::GitRepo;
use anyhow::{Context, Result};
use std::process::Command;

/// One line per commit: short hash, date, refs, subject, committer.
///
/// Every option that shapes the output is passed explicitly, so the view does
/// not depend on the user's `format.pretty`, `log.date`, `log.decorate`,
/// `log.showSignature`, or `color.ui` settings.
pub const HISTORY_FORMAT: &str =
    "format:%C(yellow)%h %ad%C(red)%d %C(reset)%s%C(blue) [%cn]%C(reset)";

pub fn history_args(color: bool, extra: &[String]) -> Vec<String> {
    let mut args: Vec<String> = [
        "-c",
        "log.showSignature=false",
        "log",
        "--date=short",
        "--decorate=short",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("--pretty={HISTORY_FORMAT}"));
    args.push(
        if color {
            "--color=always"
        } else {
            "--color=never"
        }
        .to_string(),
    );
    args.extend(extra.iter().cloned());
    args
}

pub fn run(extra: &[String]) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?;
    let color = colored::control::SHOULD_COLORIZE.should_colorize();

    // Inherit stdio so git pages the output like a normal `git log`.
    let status = Command::new("git")
        .args(history_args(color, extra))
        .current_dir(workdir)
        .status()
        .context("failed to run git log")?;

    if !status.success() {
        anyhow::bail!("git log exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_args_pin_format_and_append_extra_args() {
        let args = history_args(false, &["-n".into(), "5".into()]);
        assert_eq!(&args[..3], ["-c", "log.showSignature=false", "log"]);
        assert!(args.contains(&"--date=short".to_string()));
        assert!(args.contains(&format!("--pretty={HISTORY_FORMAT}")));
        assert!(args.contains(&"--color=never".to_string()));
        assert_eq!(&args[args.len() - 2..], ["-n", "5"]);
    }

    #[test]
    fn history_args_force_color_when_enabled() {
        let args = history_args(true, &[]);
        assert!(args.contains(&"--color=always".to_string()));
    }
}
