use anyhow::{Context, Result, ensure};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;
use tempfile::NamedTempFile;

const MAX_RECENT_REPOSITORIES: usize = 10;

#[derive(Debug, Clone)]
pub struct RecentRepositories {
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneVisibility {
    pub stack: bool,
    pub changes: bool,
    pub inspector: bool,
}

impl Default for PaneVisibility {
    fn default() -> Self {
        Self {
            stack: true,
            changes: true,
            inspector: true,
        }
    }
}

impl PaneVisibility {
    pub fn visible_count(self) -> usize {
        usize::from(self.stack) + usize::from(self.changes) + usize::from(self.inspector)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaneWidths {
    pub stack: f32,
    pub changes: f32,
    pub inspector: f32,
}

impl Default for PaneWidths {
    fn default() -> Self {
        Self {
            stack: 0.29,
            changes: 0.43,
            inspector: 0.28,
        }
    }
}

impl PaneWidths {
    fn normalized(self) -> Option<Self> {
        let values = [self.stack, self.changes, self.inspector];
        if values
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return None;
        }
        let mut normalized = Self {
            stack: self.stack.clamp(0.15, 0.70),
            changes: self.changes.clamp(0.15, 0.70),
            inspector: self.inspector.clamp(0.15, 0.70),
        };
        let total = normalized.stack + normalized.changes + normalized.inspector;
        normalized.stack /= total;
        normalized.changes /= total;
        normalized.inspector /= total;
        Some(normalized)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkspacePreferences {
    pub visibility: PaneVisibility,
    pub widths: PaneWidths,
}

impl WorkspacePreferences {
    pub(crate) fn normalized(self) -> Option<Self> {
        if self.visibility.visible_count() == 0 {
            return None;
        }
        Some(Self {
            visibility: self.visibility,
            widths: self.widths.normalized()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct WorkspacePreferencesFile {
    path: PathBuf,
}

impl WorkspacePreferencesFile {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("stax/gui/workspaces.json")
    }

    pub fn load(&self, repository: &Path) -> WorkspacePreferences {
        let Ok(repository) = repository.canonicalize() else {
            return WorkspacePreferences::default();
        };
        let Ok(contents) = fs::read(&self.path) else {
            return WorkspacePreferences::default();
        };
        let Ok(document) =
            serde_json::from_slice::<HashMap<String, WorkspacePreferences>>(&contents)
        else {
            return WorkspacePreferences::default();
        };
        document
            .get(&repository.to_string_lossy().to_string())
            .cloned()
            .and_then(WorkspacePreferences::normalized)
            .unwrap_or_default()
    }

    pub fn save(&self, repository: &Path, preferences: &WorkspacePreferences) -> Result<()> {
        let repository = repository.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize workspace repository {}",
                repository.display()
            )
        })?;
        let preferences = preferences.clone().normalized().unwrap_or_default();
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create workspace preferences directory {}",
                parent.display()
            )
        })?;
        restrict_directory_permissions(parent)?;
        let lock_path = parent.join(".workspaces.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(&lock_path).with_context(|| {
            format!(
                "failed to open workspace preferences lock {}",
                lock_path.display()
            )
        })?;
        restrict_file_permissions(&lock, &lock_path)?;
        <File as FileExt>::lock(&lock).with_context(|| {
            format!(
                "failed to lock workspace preferences {}",
                lock_path.display()
            )
        })?;
        let _lock = ExclusiveLock { file: lock };

        let mut document: HashMap<String, WorkspacePreferences> = fs::read(&self.path)
            .ok()
            .and_then(|contents| serde_json::from_slice(&contents).ok())
            .unwrap_or_default();
        document.insert(repository.to_string_lossy().to_string(), preferences);
        let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
            format!(
                "failed to create temporary workspace preferences in {}",
                parent.display()
            )
        })?;
        restrict_file_permissions(temporary.as_file(), temporary.path())?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), &document)
            .context("failed to serialize workspace preferences")?;
        temporary.as_file_mut().write_all(b"\n")?;
        temporary.as_file_mut().flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.error)
            .with_context(|| {
                format!(
                    "failed to atomically replace workspace preferences {}",
                    self.path.display()
                )
            })?;
        sync_parent_directory(parent)?;
        Ok(())
    }
}

impl Default for WorkspacePreferencesFile {
    fn default() -> Self {
        Self::at(Self::default_path())
    }
}

pub trait WorkspacePreferenceStore: Send + Sync {
    fn load(&self, repository: &Path) -> WorkspacePreferences;
    fn save(
        &self,
        repository: &Path,
        preferences: &WorkspacePreferences,
    ) -> std::result::Result<(), String>;
}

impl WorkspacePreferenceStore for WorkspacePreferencesFile {
    fn load(&self, repository: &Path) -> WorkspacePreferences {
        WorkspacePreferencesFile::load(self, repository)
    }

    fn save(
        &self,
        repository: &Path,
        preferences: &WorkspacePreferences,
    ) -> std::result::Result<(), String> {
        WorkspacePreferencesFile::save(self, repository, preferences)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct TransientWorkspacePreferences {
    values: Mutex<HashMap<PathBuf, WorkspacePreferences>>,
}

#[cfg(test)]
impl WorkspacePreferenceStore for TransientWorkspacePreferences {
    fn load(&self, repository: &Path) -> WorkspacePreferences {
        self.values
            .lock()
            .expect("transient workspace preferences poisoned")
            .get(repository)
            .cloned()
            .unwrap_or_default()
    }

    fn save(
        &self,
        repository: &Path,
        preferences: &WorkspacePreferences,
    ) -> std::result::Result<(), String> {
        self.values
            .lock()
            .map_err(|_| "transient workspace preferences poisoned".to_string())?
            .insert(repository.to_path_buf(), preferences.clone());
        Ok(())
    }
}

struct ExclusiveLock {
    file: File,
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        let _ = <File as FileExt>::unlock(&self.file);
    }
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
        self.load_unlocked()
    }

    fn load_unlocked(&self) -> Result<Vec<PathBuf>> {
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
        let mut repositories = Vec::new();
        for stored_path in stored {
            let canonical = match stored_path.canonicalize() {
                Ok(canonical) => canonical,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to canonicalize recent repository {} listed in {}",
                            stored_path.display(),
                            self.path.display()
                        )
                    });
                }
            };
            let metadata = match fs::metadata(&canonical) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to inspect recent repository {} listed in {}",
                            canonical.display(),
                            self.path.display()
                        )
                    });
                }
            };
            if metadata.is_dir() && seen.insert(canonical.clone()) {
                repositories.push(canonical);
                if repositories.len() == MAX_RECENT_REPOSITORIES {
                    break;
                }
            }
        }
        Ok(repositories)
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

        let _lock = self.acquire_lock()?;
        let mut repositories = self.load_unlocked()?;
        repositories.retain(|existing| existing != &canonical);
        repositories.insert(0, canonical);
        repositories.truncate(MAX_RECENT_REPOSITORIES);
        self.save(&repositories)
    }

    fn preferences_directory(&self) -> &Path {
        self.path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    }

    fn lock_path(&self) -> PathBuf {
        self.preferences_directory()
            .join(".recent-repositories.lock")
    }

    fn ensure_private_directory(&self) -> Result<&Path> {
        let parent = self.preferences_directory();
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create recent repositories directory {}",
                parent.display()
            )
        })?;
        restrict_directory_permissions(parent)?;
        Ok(parent)
    }

    fn acquire_lock(&self) -> Result<ExclusiveLock> {
        self.ensure_private_directory()?;
        let lock_path = self.lock_path();
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&lock_path)
            .with_context(|| format!("failed to open preferences lock {}", lock_path.display()))?;
        restrict_file_permissions(&file, &lock_path)?;
        <File as FileExt>::lock(&file)
            .with_context(|| format!("failed to lock preferences {}", lock_path.display()))?;
        Ok(ExclusiveLock { file })
    }

    fn save(&self, repositories: &[PathBuf]) -> Result<()> {
        let parent = self.ensure_private_directory()?;

        let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
            format!(
                "failed to create temporary recent repositories file in {}",
                parent.display()
            )
        })?;
        restrict_file_permissions(temporary.as_file(), temporary.path())?;
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
        let persisted = temporary
            .persist(&self.path)
            .map_err(|error| error.error)
            .with_context(|| {
                format!(
                    "failed to atomically replace recent repositories file {}",
                    self.path.display()
                )
            })?;
        drop(persisted);
        sync_parent_directory(parent)?;
        Ok(())
    }
}

impl Default for RecentRepositories {
    fn default() -> Self {
        Self::at(Self::default_path())
    }
}

fn restrict_directory_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
            format!(
                "failed to restrict recent repositories directory {}",
                path.display()
            )
        })?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn restrict_file_permissions(file: &File, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to restrict preferences file {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        let _ = path;
    }
    Ok(())
}

fn sync_parent_directory(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let directory = File::open(parent).with_context(|| {
            format!(
                "failed to open recent repositories directory {} for sync",
                parent.display()
            )
        })?;
        match directory.sync_all() {
            Ok(()) => {}
            #[cfg(target_os = "macos")]
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to sync recent repositories directory {}",
                        parent.display()
                    )
                });
            }
        }
    }
    #[cfg(not(unix))]
    let _ = parent;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        PaneVisibility, PaneWidths, RecentRepositories, WorkspacePreferences,
        WorkspacePreferencesFile,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    const WRITER_STORE_ENV: &str = "STAX_TEST_RECENT_WRITER_STORE";
    const WRITER_REPOSITORY_ENV: &str = "STAX_TEST_RECENT_WRITER_REPOSITORY";
    const WRITER_READY_ENV: &str = "STAX_TEST_RECENT_WRITER_READY";

    #[test]
    fn workspace_preferences_round_trip_independently_per_repository() {
        let temp = TempDir::new().unwrap();
        let first = create_repository(temp.path(), "first-workspace");
        let second = create_repository(temp.path(), "second-workspace");
        let store = WorkspacePreferencesFile::at(temp.path().join("preferences/workspaces.json"));
        let first_preferences = WorkspacePreferences {
            visibility: PaneVisibility {
                stack: true,
                changes: false,
                inspector: true,
            },
            widths: PaneWidths {
                stack: 0.2,
                changes: 0.5,
                inspector: 0.3,
            },
        };

        store.save(&first, &first_preferences).unwrap();

        assert_eq!(store.load(&first), first_preferences);
        assert_eq!(store.load(&second), WorkspacePreferences::default());
    }

    #[test]
    fn workspace_preferences_fall_back_for_corrupt_or_invalid_documents() {
        let temp = TempDir::new().unwrap();
        let repository = create_repository(temp.path(), "workspace");
        let path = temp.path().join("preferences/workspaces.json");
        let store = WorkspacePreferencesFile::at(&path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{not json").unwrap();
        assert_eq!(store.load(&repository), WorkspacePreferences::default());

        let canonical = repository.canonicalize().unwrap();
        fs::write(
            &path,
            format!(
                r#"{{"{}":{{"visibility":{{"stack":false,"changes":false,"inspector":false}},"widths":{{"stack":-1.0,"changes":4.0,"inspector":0.0}}}}}}"#,
                canonical.display()
            ),
        )
        .unwrap();

        assert_eq!(store.load(&repository), WorkspacePreferences::default());
    }

    fn store(temp: &TempDir) -> RecentRepositories {
        RecentRepositories::at(temp.path().join("preferences/recent-repositories.json"))
    }

    fn create_repository(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_stored_paths(path: &Path, repositories: &[PathBuf]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(repositories).unwrap()).unwrap();
    }

    fn spawn_writer(store: &Path, repository: &Path, ready: &Path) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .arg("preferences::tests::concurrent_record_worker")
            .arg("--exact")
            .arg("--nocapture")
            .env(WRITER_STORE_ENV, store)
            .env(WRITER_REPOSITORY_ENV, repository)
            .env(WRITER_READY_ENV, ready)
            .spawn()
            .unwrap()
    }

    fn wait_for_path(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
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
        let stored_paths = vec![missing, existing.clone()];
        let stored = serde_json::to_vec(&stored_paths).unwrap();
        write_stored_paths(&preferences_path, &stored_paths);
        let recent = RecentRepositories::at(&preferences_path);

        assert_eq!(
            recent.load().unwrap(),
            vec![existing.canonicalize().unwrap()]
        );
        assert_eq!(fs::read(&preferences_path).unwrap(), stored);
    }

    #[test]
    fn load_filters_persisted_regular_files() {
        let temp = TempDir::new().unwrap();
        let repository = create_repository(temp.path(), "repository");
        let regular_file = temp.path().join("not-a-repository");
        fs::write(&regular_file, b"file").unwrap();
        let preferences_path = temp.path().join("preferences/recent-repositories.json");
        write_stored_paths(&preferences_path, &[regular_file, repository.clone()]);

        assert_eq!(
            RecentRepositories::at(preferences_path).load().unwrap(),
            vec![repository.canonicalize().unwrap()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn load_deduplicates_raw_symlink_aliases_without_rewriting() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let repository = create_repository(temp.path(), "repository");
        let alias = temp.path().join("alias");
        symlink(&repository, &alias).unwrap();
        let preferences_path = temp.path().join("preferences/recent-repositories.json");
        let stored_paths = vec![alias.clone(), repository.clone(), alias];
        write_stored_paths(&preferences_path, &stored_paths);
        let original = fs::read(&preferences_path).unwrap();

        assert_eq!(
            RecentRepositories::at(&preferences_path).load().unwrap(),
            vec![repository.canonicalize().unwrap()]
        );
        assert_eq!(fs::read(&preferences_path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn load_reports_non_not_found_canonicalization_failures() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let first = temp.path().join("loop-a");
        let second = temp.path().join("loop-b");
        symlink(&second, &first).unwrap();
        symlink(&first, &second).unwrap();
        let preferences_path = temp.path().join("preferences/recent-repositories.json");
        write_stored_paths(&preferences_path, std::slice::from_ref(&first));

        let error = RecentRepositories::at(preferences_path)
            .load()
            .unwrap_err()
            .to_string();

        assert!(error.contains("canonicalize recent repository"));
        assert!(error.contains(&first.display().to_string()));
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
        let names: HashSet<_> = entries.into_iter().collect();
        assert_eq!(
            names,
            HashSet::from([
                preferences_path.file_name().unwrap().to_owned(),
                recent.lock_path().file_name().unwrap().to_owned(),
            ])
        );
    }

    #[cfg(unix)]
    #[test]
    fn preferences_directory_and_files_are_user_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let repository = create_repository(temp.path(), "repository");
        let preferences_dir = temp.path().join("preferences");
        fs::create_dir_all(&preferences_dir).unwrap();
        fs::set_permissions(&preferences_dir, fs::Permissions::from_mode(0o777)).unwrap();
        let recent = RecentRepositories::at(preferences_dir.join("recent-repositories.json"));

        recent.record(&repository).unwrap();

        assert_eq!(
            fs::metadata(&preferences_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&recent.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(recent.lock_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn concurrent_record_worker() {
        let Some(store) = std::env::var_os(WRITER_STORE_ENV) else {
            return;
        };
        let repository = std::env::var_os(WRITER_REPOSITORY_ENV).unwrap();
        let ready = std::env::var_os(WRITER_READY_ENV).unwrap();

        fs::write(ready, b"ready").unwrap();
        RecentRepositories::at(store).record(repository).unwrap();
    }

    #[test]
    fn concurrent_process_writers_preserve_both_repositories() {
        let temp = TempDir::new().unwrap();
        let first_repository = create_repository(temp.path(), "repository-a");
        let second_repository = create_repository(temp.path(), "repository-b");
        let recent = store(&temp);
        let lock = recent.acquire_lock().unwrap();
        let first_ready = temp.path().join("first-ready");
        let second_ready = temp.path().join("second-ready");
        let mut first = spawn_writer(&recent.path, &first_repository, &first_ready);
        let mut second = spawn_writer(&recent.path, &second_repository, &second_ready);

        wait_for_path(&first_ready);
        wait_for_path(&second_ready);
        thread::sleep(Duration::from_millis(250));
        let first_was_blocked = first.try_wait().unwrap().is_none();
        let second_was_blocked = second.try_wait().unwrap().is_none();
        drop(lock);
        let first_status = first.wait().unwrap();
        let second_status = second.wait().unwrap();

        assert!(first_was_blocked);
        assert!(second_was_blocked);
        assert!(first_status.success());
        assert!(second_status.success());
        let loaded: HashSet<_> = recent.load().unwrap().into_iter().collect();
        assert_eq!(
            loaded,
            HashSet::from([
                first_repository.canonicalize().unwrap(),
                second_repository.canonicalize().unwrap(),
            ])
        );
    }
}
