use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const BUNDLE_ID: &str = "dev.stax.Stax";
const DEFAULT_OPEN: &str = "/usr/bin/open";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    MacOs,
    Unsupported(&'static str),
}

impl Platform {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unsupported(std::env::consts::OS)
        }
    }
}

trait CommandRunner {
    fn run(&self, program: &Path, args: &[OsString]) -> Result<()>;
}

struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &Path, args: &[OsString]) -> Result<()> {
        let status = Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("failed to spawn {}", program.display()))?;
        if status.success() {
            Ok(())
        } else {
            anyhow::bail!("{} exited with {}", program.display(), status);
        }
    }
}

pub fn run(path: Option<PathBuf>) -> Result<()> {
    run_with_runner(path, Platform::current(), &RealCommandRunner)
}

fn run_with_runner(
    path: Option<PathBuf>,
    platform: Platform,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let repository = canonical_repository_path(path)?;
    if let Platform::Unsupported(os) = platform {
        anyhow::bail!("st gui is only supported on macOS; current platform is {os}");
    }

    let program = open_executable()?;
    let args = vec![
        OsString::from("-n"),
        OsString::from("-b"),
        OsString::from(BUNDLE_ID),
        OsString::from("--args"),
        repository.into_os_string(),
    ];

    runner.run(&program, &args).with_context(|| {
        format!(
            "failed to launch unsigned developer preview Stax.app with {}; run `make install-gui-app` to install $HOME/Applications/Stax.app",
            program.display()
        )
    })
}

fn canonical_repository_path(path: Option<PathBuf>) -> Result<PathBuf> {
    let path = match path {
        Some(path) => path,
        None => std::env::current_dir().context("failed to resolve current directory")?,
    };
    path.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize GUI repository path {}",
            path.display()
        )
    })
}

fn open_executable() -> Result<PathBuf> {
    match std::env::var_os("STAX_GUI_OPEN_EXECUTABLE") {
        Some(value) => {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                anyhow::bail!(
                    "STAX_GUI_OPEN_EXECUTABLE must be an absolute path; run `make install-gui-app` to install the unsigned developer preview"
                );
            }
            Ok(path)
        }
        None => Ok(PathBuf::from(DEFAULT_OPEN)),
    }
}

#[cfg(test)]
struct RecordingCommandRunner {
    result: Result<(), String>,
    program: std::cell::RefCell<Option<PathBuf>>,
    args: std::cell::RefCell<Vec<OsString>>,
}

#[cfg(test)]
impl RecordingCommandRunner {
    fn succeeding() -> Self {
        Self {
            result: Ok(()),
            program: std::cell::RefCell::new(None),
            args: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn failing() -> Self {
        Self {
            result: Err("missing application".into()),
            program: std::cell::RefCell::new(None),
            args: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn program(&self) -> PathBuf {
        self.program
            .borrow()
            .clone()
            .expect("recorded launcher program")
    }

    fn args(&self) -> Vec<OsString> {
        self.args.borrow().clone()
    }
}

#[cfg(test)]
impl CommandRunner for RecordingCommandRunner {
    fn run(&self, program: &Path, args: &[OsString]) -> Result<()> {
        *self.program.borrow_mut() = Some(program.to_path_buf());
        *self.args.borrow_mut() = args.to_vec();
        match &self.result {
            Ok(()) => Ok(()),
            Err(message) => anyhow::bail!("{message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Platform, RecordingCommandRunner, run_with_runner};
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn launcher_opens_a_fresh_instance_with_one_canonical_path_argument() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("repo with spaces");
        std::fs::create_dir(&path).unwrap();
        let runner = RecordingCommandRunner::succeeding();

        run_with_runner(Some(path.clone()), Platform::MacOs, &runner).unwrap();

        assert_eq!(runner.program(), PathBuf::from("/usr/bin/open"));
        assert_eq!(
            runner.args(),
            vec![
                OsString::from("-n"),
                OsString::from("-b"),
                OsString::from("dev.stax.Stax"),
                OsString::from("--args"),
                path.canonicalize().unwrap().into_os_string(),
            ]
        );
    }

    #[test]
    fn launcher_failure_points_to_the_repository_install_target() {
        let temp = tempfile::tempdir().unwrap();
        let error = run_with_runner(
            Some(temp.path().to_path_buf()),
            Platform::MacOs,
            &RecordingCommandRunner::failing(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("make install-gui-app"));
        assert!(error.to_string().contains("unsigned developer preview"));
    }

    #[test]
    fn unsupported_platform_never_runs_a_command() {
        let temp = tempfile::tempdir().unwrap();
        let runner = RecordingCommandRunner::succeeding();
        let error = run_with_runner(
            Some(temp.path().to_path_buf()),
            Platform::Unsupported("linux"),
            &runner,
        )
        .unwrap_err();
        assert!(error.to_string().contains("only supported on macOS"));
        assert!(runner.args().is_empty());
    }
}
