use anyhow::{Context, Result, ensure};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const MAX_RECENT_REPOSITORIES: usize = 10;

#[derive(Debug, Clone)]
pub struct RecentRepositories {
    path: PathBuf,
}

impl RecentRepositories {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("stax/gui/recent-repositories.json")
    }

    pub fn load(&self) -> Result<Vec<PathBuf>> {
        let contents = match fs::read(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to read recent repositories from {}",
                        self.path.display()
                    )
                });
            }
        };
        let stored: Vec<PathBuf> = serde_json::from_slice(&contents).with_context(|| {
            format!(
                "failed to parse recent repositories from {}",
                self.path.display()
            )
        })?;

        let mut seen = HashSet::new();
        Ok(stored
            .into_iter()
            .filter_map(|path| path.canonicalize().ok())
            .filter(|path| seen.insert(path.clone()))
            .take(MAX_RECENT_REPOSITORIES)
            .collect())
    }

    pub fn record(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let metadata = fs::metadata(path)
            .with_context(|| format!("repository path {} is not accessible", path.display()))?;
        ensure!(
            metadata.is_dir(),
            "repository path {} is not a directory",
            path.display()
        );
        let canonical = path.canonicalize().with_context(|| {
            format!("failed to canonicalize repository path {}", path.display())
        })?;

        let mut repositories = self.load()?;
        repositories.retain(|existing| existing != &canonical);
        repositories.insert(0, canonical);
        repositories.truncate(MAX_RECENT_REPOSITORIES);
        self.save(&repositories)
    }

    fn save(&self, repositories: &[PathBuf]) -> Result<()> {
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create recent repositories directory {}",
                parent.display()
            )
        })?;

        let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
            format!(
                "failed to create temporary recent repositories file in {}",
                parent.display()
            )
        })?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), repositories).with_context(|| {
            format!(
                "failed to serialize recent repositories for {}",
                self.path.display()
            )
        })?;
        temporary
            .as_file_mut()
            .write_all(b"\n")
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        temporary
            .as_file_mut()
            .flush()
            .with_context(|| format!("failed to flush {}", self.path.display()))?;
        temporary
            .as_file()
            .sync_all()
            .with_context(|| format!("failed to sync {}", self.path.display()))?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.error)
            .with_context(|| {
                format!(
                    "failed to atomically replace recent repositories file {}",
                    self.path.display()
                )
            })?;
        Ok(())
    }
}

impl Default for RecentRepositories {
    fn default() -> Self {
        Self::at(Self::default_path())
    }
}

#[cfg(test)]
mod tests {
    use super::RecentRepositories;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn store(temp: &TempDir) -> RecentRepositories {
        RecentRepositories::at(temp.path().join("preferences/recent-repositories.json"))
    }

    fn create_repository(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_preferences_file_loads_an_empty_list() {
        let temp = TempDir::new().unwrap();

        assert_eq!(store(&temp).load().unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn default_store_uses_the_platform_data_directory() {
        let expected = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("stax/gui/recent-repositories.json");

        assert_eq!(RecentRepositories::default_path(), expected);
        assert_eq!(RecentRepositories::default().path, expected);
    }

    #[test]
    fn record_round_trips_a_canonical_repository_path() {
        let temp = TempDir::new().unwrap();
        let repository = create_repository(temp.path(), "repository");
        let recent = store(&temp);

        recent.record(&repository).unwrap();

        assert_eq!(
            recent.load().unwrap(),
            vec![repository.canonicalize().unwrap()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonical_paths_deduplicate_symlinked_repositories() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let repository = create_repository(temp.path(), "repository");
        let alias = temp.path().join("repository-alias");
        symlink(&repository, &alias).unwrap();
        let recent = store(&temp);

        recent.record(&repository).unwrap();
        recent.record(&alias).unwrap();

        assert_eq!(
            recent.load().unwrap(),
            vec![repository.canonicalize().unwrap()]
        );
    }

    #[test]
    fn record_keeps_the_ten_newest_repositories() {
        let temp = TempDir::new().unwrap();
        let recent = store(&temp);
        let repositories: Vec<_> = (0..12)
            .map(|index| create_repository(temp.path(), &format!("repository-{index}")))
            .collect();

        for repository in &repositories {
            recent.record(repository).unwrap();
        }

        let expected: Vec<_> = repositories[2..]
            .iter()
            .rev()
            .map(|path| path.canonicalize().unwrap())
            .collect();
        assert_eq!(recent.load().unwrap(), expected);
    }

    #[test]
    fn load_filters_missing_entries_without_rewriting_the_file() {
        let temp = TempDir::new().unwrap();
        let existing = create_repository(temp.path(), "existing");
        let missing = temp.path().join("missing");
        let preferences_path = temp.path().join("preferences/recent-repositories.json");
        fs::create_dir_all(preferences_path.parent().unwrap()).unwrap();
        let stored = serde_json::to_vec(&vec![missing, existing.clone()]).unwrap();
        fs::write(&preferences_path, &stored).unwrap();
        let recent = RecentRepositories::at(&preferences_path);

        assert_eq!(
            recent.load().unwrap(),
            vec![existing.canonicalize().unwrap()]
        );
        assert_eq!(fs::read(&preferences_path).unwrap(), stored);
    }

    #[test]
    fn malformed_json_returns_an_actionable_error() {
        let temp = TempDir::new().unwrap();
        let preferences_path = temp.path().join("preferences/recent-repositories.json");
        fs::create_dir_all(preferences_path.parent().unwrap()).unwrap();
        fs::write(&preferences_path, b"{not valid json").unwrap();

        let error = RecentRepositories::at(&preferences_path)
            .load()
            .unwrap_err()
            .to_string();

        assert!(error.contains("parse recent repositories"));
        assert!(error.contains(&preferences_path.display().to_string()));
    }

    #[test]
    fn record_rejects_missing_and_non_directory_paths_with_context() {
        let temp = TempDir::new().unwrap();
        let recent = store(&temp);
        let missing = temp.path().join("missing");
        let file = temp.path().join("file");
        fs::write(&file, b"not a directory").unwrap();

        for invalid in [&missing, &file] {
            let error = recent.record(invalid).unwrap_err().to_string();
            assert!(error.contains(&invalid.display().to_string()));
            assert!(error.contains("repository"));
        }
    }

    #[test]
    fn successful_save_leaves_no_temporary_file() {
        let temp = TempDir::new().unwrap();
        let repository = create_repository(temp.path(), "repository");
        let preferences_dir = temp.path().join("preferences");
        let preferences_path = preferences_dir.join("recent-repositories.json");
        let recent = RecentRepositories::at(&preferences_path);

        recent.record(&repository).unwrap();

        let entries: Vec<_> = fs::read_dir(&preferences_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![preferences_path.file_name().unwrap()]);
    }
}
