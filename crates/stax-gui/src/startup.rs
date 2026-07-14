use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupCommand {
    Run(Option<PathBuf>),
    PrintVersion,
}

pub fn parse_startup_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<StartupCommand, String> {
    let mut args = args.into_iter();
    let command = match args.next() {
        None => StartupCommand::Run(None),
        Some(value) if value == OsStr::new("--version") || value == OsStr::new("-V") => {
            StartupCommand::PrintVersion
        }
        Some(value) => StartupCommand::Run(Some(PathBuf::from(value))),
    };
    if args.next().is_some() {
        return Err("stax-gui accepts at most one repository path".into());
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::{StartupCommand, parse_startup_command};
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn no_argument_opens_the_welcome_window() {
        assert_eq!(
            parse_startup_command(Vec::<OsString>::new()).unwrap(),
            StartupCommand::Run(None)
        );
    }

    #[test]
    fn one_repository_path_is_preserved_verbatim() {
        let path = PathBuf::from("/tmp/repository with spaces");
        assert_eq!(
            parse_startup_command([path.clone().into_os_string()]).unwrap(),
            StartupCommand::Run(Some(path))
        );
    }

    #[test]
    fn version_flags_exit_without_opening_gpui() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                parse_startup_command([OsString::from(flag)]).unwrap(),
                StartupCommand::PrintVersion
            );
        }
    }

    #[test]
    fn more_than_one_repository_path_is_rejected() {
        let error = parse_startup_command([OsString::from("/tmp/one"), OsString::from("/tmp/two")])
            .unwrap_err();

        assert!(error.contains("one repository path"));
    }
}
