use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, RepositorySnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState<T> {
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> LoadState<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceState {
    snapshot: RepositorySnapshot,
    selected_branch: Option<String>,
    details: LoadState<BranchDetails>,
    diff: LoadState<BranchDiff>,
    ci: LoadState<CiSummary>,
    generation: u64,
}

impl WorkspaceState {
    pub fn new(snapshot: RepositorySnapshot) -> Self {
        let selected_branch = snapshot
            .branches
            .iter()
            .find(|branch| branch.name == snapshot.current_branch)
            .or_else(|| snapshot.branches.first())
            .map(|branch| branch.name.clone());

        Self {
            snapshot,
            selected_branch,
            details: LoadState::Idle,
            diff: LoadState::Idle,
            ci: LoadState::Idle,
            generation: 0,
        }
    }

    pub fn snapshot(&self) -> &RepositorySnapshot {
        &self.snapshot
    }

    pub fn selected_branch(&self) -> Option<&str> {
        self.selected_branch.as_deref()
    }

    pub fn details(&self) -> &LoadState<BranchDetails> {
        &self.details
    }

    pub fn diff(&self) -> &LoadState<BranchDiff> {
        &self.diff
    }

    pub fn ci(&self) -> &LoadState<CiSummary> {
        &self.ci
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn select_branch(&mut self, name: &str) -> Option<DetailRequestToken> {
        if !self
            .snapshot
            .branches
            .iter()
            .any(|branch| branch.name == name)
        {
            return None;
        }

        self.selected_branch = Some(name.to_owned());
        self.advance_generation();
        self.details = LoadState::Idle;
        self.diff = LoadState::Idle;
        self.ci = LoadState::Idle;
        Some(self.current_token(name))
    }

    pub fn begin_hydration(&mut self) -> Option<(DetailRequestToken, BranchSummary)> {
        let summary = self
            .selected_branch
            .as_deref()
            .and_then(|selected| {
                self.snapshot
                    .branches
                    .iter()
                    .find(|branch| branch.name == selected)
            })?
            .clone();
        self.advance_generation();
        let token = self.current_token(&summary.name);

        self.details = LoadState::Loading;
        self.diff = LoadState::Loading;
        self.ci = LoadState::Loading;
        Some((token, summary))
    }

    pub fn apply_details(
        &mut self,
        token: DetailRequestToken,
        result: Result<BranchDetails, String>,
    ) -> bool {
        if !self.matches(&token) {
            return false;
        }
        self.details = result.map_or_else(LoadState::Failed, LoadState::Ready);
        true
    }

    pub fn apply_diff(
        &mut self,
        token: DetailRequestToken,
        result: Result<BranchDiff, String>,
    ) -> bool {
        if !self.matches(&token) {
            return false;
        }
        self.diff = result.map_or_else(LoadState::Failed, LoadState::Ready);
        true
    }

    pub fn apply_ci(
        &mut self,
        token: DetailRequestToken,
        result: Result<CiSummary, String>,
    ) -> bool {
        if !self.matches(&token) {
            return false;
        }
        self.ci = result.map_or_else(LoadState::Failed, LoadState::Ready);
        true
    }

    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn current_token(&self, branch: &str) -> DetailRequestToken {
        DetailRequestToken::new(
            self.snapshot.repository_root.clone(),
            branch,
            self.generation,
        )
    }

    fn matches(&self, token: &DetailRequestToken) -> bool {
        self.selected_branch.as_deref().is_some_and(|branch| {
            token.matches(&self.snapshot.repository_root, branch, self.generation)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadState, WorkspaceState};
    use stax::application::{
        BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, RepositorySnapshot,
    };
    use std::path::PathBuf;

    fn branch(name: &str, is_current: bool) -> BranchSummary {
        BranchSummary {
            name: name.into(),
            parent: None,
            column: 0,
            is_current,
            is_trunk: false,
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            ci_state: None,
        }
    }

    fn snapshot(repository: &str, current: &str, branches: &[(&str, bool)]) -> RepositorySnapshot {
        RepositorySnapshot {
            repository_root: PathBuf::from(repository),
            current_branch: current.into(),
            trunk: "main".into(),
            branches: branches
                .iter()
                .map(|(name, is_current)| branch(name, *is_current))
                .collect(),
        }
    }

    fn details(ahead: usize) -> BranchDetails {
        BranchDetails {
            ahead,
            behind: 0,
            has_remote: false,
            unpushed: 0,
            unpulled: 0,
            commits: Vec::new(),
        }
    }

    fn diff(line: &str) -> BranchDiff {
        BranchDiff {
            stat: Vec::new(),
            lines: vec![stax::application::DiffLine {
                content: line.into(),
                kind: stax::application::DiffLineKind::Context,
            }],
        }
    }

    fn ci(status: &str) -> CiSummary {
        CiSummary {
            overall_status: Some(status.into()),
            total: 0,
            passed: 0,
            failed: 0,
            running: 0,
            queued: 0,
            skipped: 0,
            started_at: None,
            completed_at: None,
            average_secs: None,
        }
    }

    #[test]
    fn new_selects_the_current_branch_and_starts_idle() {
        let state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-b",
            &[("feature-a", false), ("feature-b", true)],
        ));

        assert_eq!(state.snapshot().repository_root, PathBuf::from("/repo"));
        assert_eq!(state.selected_branch(), Some("feature-b"));
        assert_eq!(state.generation(), 0);
        assert_eq!(state.details(), &LoadState::Idle);
        assert_eq!(state.diff(), &LoadState::Idle);
        assert_eq!(state.ci(), &LoadState::Idle);
    }

    #[test]
    fn new_falls_back_to_the_first_branch_when_current_is_absent() {
        let state = WorkspaceState::new(snapshot(
            "/repo",
            "detached",
            &[("feature-a", false), ("feature-b", false)],
        ));

        assert_eq!(state.selected_branch(), Some("feature-a"));
    }

    #[test]
    fn empty_snapshot_has_no_selection_or_hydration_request() {
        let mut state = WorkspaceState::new(snapshot("/repo", "main", &[]));

        assert_eq!(state.selected_branch(), None);
        assert_eq!(state.generation(), 0);
        assert_eq!(state.begin_hydration(), None);
        assert_eq!(state.generation(), 0);
        assert_eq!(state.details(), &LoadState::Idle);
        assert_eq!(state.diff(), &LoadState::Idle);
        assert_eq!(state.ci(), &LoadState::Idle);
    }

    #[test]
    fn invalid_selection_does_not_mutate_state() {
        let mut state = WorkspaceState::new(snapshot("/repo", "feature-a", &[("feature-a", true)]));
        let (token, _) = state.begin_hydration().unwrap();
        assert!(state.apply_details(token.clone(), Err("keep details".into())));
        assert!(state.apply_diff(token.clone(), Err("keep diff".into())));
        assert!(state.apply_ci(token, Err("keep ci".into())));
        let generation = state.generation();

        assert_eq!(state.select_branch("missing"), None);
        assert_eq!(state.selected_branch(), Some("feature-a"));
        assert_eq!(state.generation(), generation);
        assert_eq!(state.details().error(), Some("keep details"));
        assert_eq!(state.diff().error(), Some("keep diff"));
        assert_eq!(state.ci().error(), Some("keep ci"));
    }

    #[test]
    fn valid_selection_increments_generation_and_resets_hydration() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        let (initial, _) = state.begin_hydration().unwrap();
        assert!(state.apply_details(initial.clone(), Ok(details(1))));
        assert!(state.apply_diff(initial.clone(), Ok(diff("old"))));
        assert!(state.apply_ci(initial, Ok(ci("success"))));

        let first = state.select_branch("feature-a").unwrap();
        assert_eq!(first, DetailRequestToken::new("/repo", "feature-a", 2));
        assert_eq!(state.generation(), 2);
        assert_eq!(state.details(), &LoadState::Idle);
        assert_eq!(state.diff(), &LoadState::Idle);
        assert_eq!(state.ci(), &LoadState::Idle);

        let second = state.select_branch("feature-b").unwrap();
        assert_eq!(second, DetailRequestToken::new("/repo", "feature-b", 3));
        assert_eq!(state.generation(), 3);
        assert_eq!(state.selected_branch(), Some("feature-b"));
    }

    #[test]
    fn begin_hydration_marks_all_results_loading_and_returns_current_request() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-b",
            &[("feature-a", false), ("feature-b", true)],
        ));

        let (token, summary) = state.begin_hydration().unwrap();

        assert_eq!(token, DetailRequestToken::new("/repo", "feature-b", 1));
        assert_eq!(summary, branch("feature-b", true));
        assert_eq!(state.generation(), 1);
        assert_eq!(state.details(), &LoadState::Loading);
        assert_eq!(state.diff(), &LoadState::Loading);
        assert_eq!(state.ci(), &LoadState::Loading);
    }

    #[test]
    fn matching_results_become_ready_and_failures_preserve_their_messages() {
        let mut state = WorkspaceState::new(snapshot("/repo", "feature-a", &[("feature-a", true)]));
        let (token, _) = state.begin_hydration().unwrap();

        assert!(state.apply_details(token.clone(), Ok(details(2))));
        assert!(state.apply_diff(token.clone(), Ok(diff("ready"))));
        assert!(state.apply_ci(token, Ok(ci("success"))));
        assert_eq!(state.details().ready(), Some(&details(2)));
        assert_eq!(state.diff().ready(), Some(&diff("ready")));
        assert_eq!(state.ci().ready(), Some(&ci("success")));
        assert_eq!(state.details().error(), None);

        state.select_branch("feature-a").unwrap();
        let (retry, _) = state.begin_hydration().unwrap();
        assert!(state.apply_details(retry.clone(), Err("details failed".into())));
        assert!(state.apply_diff(retry.clone(), Err("diff failed".into())));
        assert!(state.apply_ci(retry, Err("ci failed".into())));
        assert_eq!(state.details().error(), Some("details failed"));
        assert_eq!(state.diff().error(), Some("diff failed"));
        assert_eq!(state.ci().error(), Some("ci failed"));
        assert_eq!(state.details().ready(), None);
    }

    #[test]
    fn every_result_type_rejects_repository_branch_and_generation_mismatches() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        state.select_branch("feature-a").unwrap();
        let (current, _) = state.begin_hydration().unwrap();

        let mismatches = [
            DetailRequestToken::new("/other-repo", "feature-a", current.generation),
            DetailRequestToken::new("/repo", "feature-b", current.generation),
            DetailRequestToken::new("/repo", "feature-a", current.generation - 1),
        ];

        for token in mismatches {
            assert!(!state.apply_details(token.clone(), Ok(details(9))));
            assert!(!state.apply_diff(token.clone(), Ok(diff("stale"))));
            assert!(!state.apply_ci(token, Ok(ci("failure"))));
        }

        assert_eq!(state.details(), &LoadState::Loading);
        assert_eq!(state.diff(), &LoadState::Loading);
        assert_eq!(state.ci(), &LoadState::Loading);
    }

    #[test]
    fn rapid_branch_selection_prevents_old_results_from_overwriting_new_state() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        let (old, _) = state.begin_hydration().unwrap();
        state.select_branch("feature-b").unwrap();
        let (current, _) = state.begin_hydration().unwrap();

        assert!(!state.apply_details(old.clone(), Ok(details(99))));
        assert!(!state.apply_diff(old.clone(), Ok(diff("old"))));
        assert!(!state.apply_ci(old, Ok(ci("failure"))));
        assert_eq!(state.details(), &LoadState::Loading);
        assert_eq!(state.diff(), &LoadState::Loading);
        assert_eq!(state.ci(), &LoadState::Loading);
        assert!(state.apply_details(current.clone(), Ok(details(3))));
        assert!(state.apply_diff(current.clone(), Ok(diff("new"))));
        assert!(state.apply_ci(current, Ok(ci("success"))));
        assert_eq!(state.details().ready(), Some(&details(3)));
        assert_eq!(state.diff().ready(), Some(&diff("new")));
        assert_eq!(state.ci().ready(), Some(&ci("success")));
        assert_eq!(state.selected_branch(), Some("feature-b"));
    }

    #[test]
    fn retrying_same_branch_hydration_rejects_the_older_request() {
        let mut state = WorkspaceState::new(snapshot("/repo", "feature-a", &[("feature-a", true)]));
        let (old, _) = state.begin_hydration().unwrap();
        let (retry, _) = state.begin_hydration().unwrap();

        assert_eq!(old.generation, 1);
        assert_eq!(retry.generation, 2);
        assert!(!state.apply_details(old.clone(), Ok(details(99))));
        assert!(!state.apply_diff(old.clone(), Ok(diff("old"))));
        assert!(!state.apply_ci(old, Ok(ci("failure"))));
        assert_eq!(state.details(), &LoadState::Loading);
        assert_eq!(state.diff(), &LoadState::Loading);
        assert_eq!(state.ci(), &LoadState::Loading);

        assert!(state.apply_details(retry.clone(), Ok(details(2))));
        assert!(state.apply_diff(retry.clone(), Ok(diff("retry"))));
        assert!(state.apply_ci(retry, Ok(ci("success"))));
        assert_eq!(state.details().ready(), Some(&details(2)));
        assert_eq!(state.diff().ready(), Some(&diff("retry")));
        assert_eq!(state.ci().ready(), Some(&ci("success")));
    }
}
