use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, OperationError,
    OperationEvent, OperationOutcome, OperationProgress, OperationReceipt, OperationRequest,
    OperationResult, RepositorySnapshot,
};
use std::path::PathBuf;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone)]
pub struct WorkspaceState {
    snapshot: RepositorySnapshot,
    selected_branch: Option<String>,
    details: LoadState<BranchDetails>,
    diff: LoadState<BranchDiff>,
    diff_refreshing: bool,
    ci: LoadState<CiSummary>,
    generation: u64,
    operation_sequence: u64,
    active_operation: Option<ActiveOperation>,
    operation_error: Option<OperationError>,
    last_receipt: Option<OperationReceipt>,
    #[cfg(test)]
    completion_transition_count: usize,
    #[cfg(test)]
    operation_progress_log: Vec<usize>,
    #[cfg(test)]
    snapshot_refresh_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationToken {
    pub id: u64,
    pub repository_root: PathBuf,
    pub repository_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveOperation {
    pub token: OperationToken,
    pub request: OperationRequest,
    pub progress: Option<OperationProgress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEffect {
    pub refresh_snapshot: bool,
    pub preferred_selection: Option<String>,
    pub open_url: Option<String>,
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
            diff_refreshing: false,
            ci: LoadState::Idle,
            generation: 0,
            operation_sequence: 0,
            active_operation: None,
            operation_error: None,
            last_receipt: None,
            #[cfg(test)]
            completion_transition_count: 0,
            #[cfg(test)]
            operation_progress_log: Vec::new(),
            #[cfg(test)]
            snapshot_refresh_count: 0,
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

    pub fn diff_is_refreshing(&self) -> bool {
        self.diff_refreshing
    }

    pub fn ci(&self) -> &LoadState<CiSummary> {
        &self.ci
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn active_operation(&self) -> Option<&ActiveOperation> {
        self.active_operation.as_ref()
    }

    pub fn operation_error(&self) -> Option<&OperationError> {
        self.operation_error.as_ref()
    }

    pub fn last_receipt(&self) -> Option<&OperationReceipt> {
        self.last_receipt.as_ref()
    }

    #[cfg(test)]
    pub fn completion_transition_count(&self) -> usize {
        self.completion_transition_count
    }

    #[cfg(test)]
    pub fn operation_progress_log(&self) -> Vec<usize> {
        self.operation_progress_log.clone()
    }

    #[cfg(test)]
    pub fn snapshot_refresh_count(&self) -> usize {
        self.snapshot_refresh_count
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

        let same_branch = self.selected_branch.as_deref() == Some(name);
        self.selected_branch = Some(name.to_owned());
        self.advance_generation();
        self.details = LoadState::Idle;
        if !same_branch || !matches!(self.diff, LoadState::Ready(_)) {
            self.diff = LoadState::Idle;
        }
        self.diff_refreshing = false;
        self.ci = LoadState::Idle;
        Some(self.current_token(name))
    }

    pub fn move_selection(&mut self, direction: SelectionDirection) -> bool {
        let Some(selected) = self.selected_branch.as_deref() else {
            return false;
        };
        let Some(index) = self
            .snapshot
            .branches
            .iter()
            .position(|branch| branch.name == selected)
        else {
            return false;
        };
        let next_index = match direction {
            SelectionDirection::Previous => index.checked_sub(1),
            SelectionDirection::Next => index
                .checked_add(1)
                .filter(|next| *next < self.snapshot.branches.len()),
        };
        let Some(next_name) = next_index
            .and_then(|next| self.snapshot.branches.get(next))
            .map(|branch| branch.name.clone())
        else {
            return false;
        };

        self.select_branch(&next_name).is_some()
    }

    pub fn replace_snapshot(&mut self, snapshot: RepositorySnapshot) {
        let previous_repository = self.snapshot.repository_root.clone();
        let previous_selection = self.selected_branch.clone();
        let previous_diff = match &self.diff {
            LoadState::Ready(diff) => Some(diff.clone()),
            LoadState::Idle | LoadState::Loading | LoadState::Failed(_) => None,
        };
        let same_repository = previous_repository == snapshot.repository_root;
        if !same_repository {
            self.operation_error = None;
            self.last_receipt = None;
            self.active_operation = None;
        }
        self.snapshot = snapshot;
        self.selected_branch = previous_selection
            .clone()
            .filter(|selected| {
                self.snapshot
                    .branches
                    .iter()
                    .any(|branch| branch.name == *selected)
            })
            .or_else(|| {
                self.snapshot
                    .branches
                    .iter()
                    .find(|branch| branch.name == self.snapshot.current_branch)
                    .map(|branch| branch.name.clone())
            })
            .or_else(|| {
                self.snapshot
                    .branches
                    .first()
                    .map(|branch| branch.name.clone())
            });
        self.advance_generation();
        self.details = LoadState::Idle;
        self.diff = if same_repository && previous_selection == self.selected_branch {
            previous_diff.map_or(LoadState::Idle, LoadState::Ready)
        } else {
            LoadState::Idle
        };
        self.diff_refreshing = false;
        self.ci = LoadState::Idle;
        #[cfg(test)]
        {
            if same_repository {
                self.snapshot_refresh_count += 1;
            }
        }
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
        if !matches!(self.diff, LoadState::Ready(_)) {
            self.diff = LoadState::Loading;
        }
        self.diff_refreshing = true;
        self.ci = LoadState::Loading;
        Some((token, summary))
    }

    pub fn begin_details_load(&mut self, branch: &str) -> Option<DetailRequestToken> {
        self.select_branch(branch)?;
        self.begin_hydration().map(|(token, _)| token)
    }

    pub fn begin_operation(&mut self, request: OperationRequest) -> Option<OperationToken> {
        if self.active_operation.is_some() {
            return None;
        }
        self.operation_sequence = self.operation_sequence.wrapping_add(1);
        let token = OperationToken {
            id: self.operation_sequence,
            repository_root: self.snapshot.repository_root.clone(),
            repository_generation: self.generation,
        };
        self.active_operation = Some(ActiveOperation {
            token: token.clone(),
            request,
            progress: None,
        });
        Some(token)
    }

    pub fn apply_operation_event(
        &mut self,
        token: &OperationToken,
        event: OperationEvent,
    ) -> Option<OperationEvent> {
        if !self.operation_matches(token) {
            return None;
        }
        match event {
            OperationEvent::Started(_) => None,
            OperationEvent::Progress(progress) => {
                #[cfg(test)]
                self.operation_progress_log.push(progress.completed);
                if let Some(active) = self.active_operation.as_mut() {
                    active.progress = Some(progress);
                }
                None
            }
            OperationEvent::Completed(receipt) => Some(OperationEvent::Completed(receipt)),
            OperationEvent::Failed(error) => Some(OperationEvent::Failed(error)),
        }
    }

    pub fn finish_operation(
        &mut self,
        token: &OperationToken,
        result: OperationResult,
    ) -> Option<CompletionEffect> {
        if !self.operation_matches(token) {
            return None;
        }
        self.active_operation = None;

        let mut effect = CompletionEffect {
            refresh_snapshot: false,
            preferred_selection: None,
            open_url: None,
        };

        match result {
            Ok(receipt) => {
                effect.refresh_snapshot =
                    receipt.request.is_mutating() || receipt.side_effects.requires_refresh();
                effect.preferred_selection = preferred_selection(&receipt);
                effect.open_url = resolved_url(&receipt);
                self.operation_error = None;
                self.last_receipt = Some(receipt);
                #[cfg(test)]
                {
                    self.completion_transition_count += 1;
                }
            }
            Err(error) => {
                effect.refresh_snapshot = error.side_effects.requires_refresh();
                self.last_receipt = error.receipt.clone();
                self.operation_error = Some(error);
                #[cfg(test)]
                {
                    self.completion_transition_count += 1;
                }
            }
        }

        if effect.refresh_snapshot {
            self.invalidate_repository_generation(effect.preferred_selection.as_deref());
        }
        Some(effect)
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

    pub fn apply_cached_diff(&mut self, token: DetailRequestToken, diff: BranchDiff) -> bool {
        if !self.matches(&token)
            || !self.diff_refreshing
            || !matches!(self.diff, LoadState::Loading)
        {
            return false;
        }
        self.diff = LoadState::Ready(diff);
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
        self.diff_refreshing = false;
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
        self.ci = match result {
            Ok(summary) => {
                if let Some(selected) = self.selected_branch.as_deref()
                    && let Some(branch) = self
                        .snapshot
                        .branches
                        .iter_mut()
                        .find(|branch| branch.name == selected)
                {
                    branch.ci_state = summary.overall_status.clone();
                }
                LoadState::Ready(summary)
            }
            Err(error) => LoadState::Failed(error),
        };
        true
    }

    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn invalidate_repository_generation(&mut self, preferred_selection: Option<&str>) {
        if let Some(preferred) = preferred_selection
            && self
                .snapshot
                .branches
                .iter()
                .any(|branch| branch.name == preferred)
        {
            self.selected_branch = Some(preferred.to_string());
        }
        self.advance_generation();
        self.details = LoadState::Idle;
        self.diff = LoadState::Idle;
        self.diff_refreshing = false;
        self.ci = LoadState::Idle;
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

    fn operation_matches(&self, token: &OperationToken) -> bool {
        self.active_operation
            .as_ref()
            .is_some_and(|active| active.token == *token)
            && token.repository_root == self.snapshot.repository_root
            && token.repository_generation == self.generation
    }
}

fn preferred_selection(receipt: &OperationReceipt) -> Option<String> {
    match &receipt.outcome {
        OperationOutcome::Checkout(checked_out) => match checked_out {
            stax::application::CheckoutOutcome::CheckedOut { branch }
            | stax::application::CheckoutOutcome::AlreadyCurrent { branch } => Some(branch.clone()),
        },
        OperationOutcome::BranchCreated { branch, .. } => Some(branch.clone()),
        OperationOutcome::Restacked { .. }
        | OperationOutcome::Submitted { .. }
        | OperationOutcome::PullRequestResolved { .. } => None,
    }
}

fn resolved_url(receipt: &OperationReceipt) -> Option<String> {
    match &receipt.outcome {
        OperationOutcome::PullRequestResolved { url, .. } => Some(url.clone()),
        OperationOutcome::Checkout(_)
        | OperationOutcome::BranchCreated { .. }
        | OperationOutcome::Restacked { .. }
        | OperationOutcome::Submitted { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadState, SelectionDirection, WorkspaceState};
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
    fn valid_selection_retains_only_a_same_branch_ready_diff() {
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
        assert_eq!(state.diff().ready(), Some(&diff("old")));
        assert!(!state.diff_is_refreshing());
        assert_eq!(state.ci(), &LoadState::Idle);

        let second = state.select_branch("feature-b").unwrap();
        assert_eq!(second, DetailRequestToken::new("/repo", "feature-b", 3));
        assert_eq!(state.generation(), 3);
        assert_eq!(state.selected_branch(), Some("feature-b"));
        assert_eq!(state.diff(), &LoadState::Idle);
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
    fn accepted_live_ci_success_replaces_cached_stack_row_status() {
        let mut snapshot = snapshot("/repo", "feature-a", &[("feature-a", true)]);
        snapshot.branches[0].ci_state = Some("failure".into());
        let mut state = WorkspaceState::new(snapshot);
        let (token, _) = state.begin_hydration().unwrap();

        assert!(state.apply_ci(token, Ok(ci("success"))));

        assert_eq!(state.ci().ready(), Some(&ci("success")));
        assert_eq!(
            state.snapshot().branches[0].ci_state.as_deref(),
            Some("success")
        );
    }

    #[test]
    fn stale_live_ci_result_cannot_replace_cached_stack_row_status() {
        let mut snapshot = snapshot("/repo", "feature-a", &[("feature-a", true)]);
        snapshot.branches[0].ci_state = Some("failure".into());
        let mut state = WorkspaceState::new(snapshot);
        let (stale, _) = state.begin_hydration().unwrap();
        let (_, _) = state.begin_hydration().unwrap();

        assert!(!state.apply_ci(stale, Ok(ci("success"))));

        assert_eq!(state.ci(), &LoadState::Loading);
        assert_eq!(
            state.snapshot().branches[0].ci_state.as_deref(),
            Some("failure")
        );
    }

    #[test]
    fn live_ci_failure_retains_cached_stack_row_status() {
        let mut snapshot = snapshot("/repo", "feature-a", &[("feature-a", true)]);
        snapshot.branches[0].ci_state = Some("failure".into());
        let mut state = WorkspaceState::new(snapshot);
        let (token, _) = state.begin_hydration().unwrap();

        assert!(state.apply_ci(token, Err("provider unavailable".into())));

        assert_eq!(state.ci().error(), Some("provider unavailable"));
        assert_eq!(
            state.snapshot().branches[0].ci_state.as_deref(),
            Some("failure")
        );
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

    #[test]
    fn retrying_same_branch_retains_ready_diff_until_refresh_finishes() {
        let mut state = WorkspaceState::new(snapshot("/repo", "feature-a", &[("feature-a", true)]));
        let (first, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(first, Ok(diff("existing patch"))));

        let (retry, _) = state.begin_hydration().unwrap();

        assert_eq!(state.diff().ready(), Some(&diff("existing patch")));
        assert!(state.diff_is_refreshing());
        assert!(state.apply_diff(retry, Ok(diff("replacement patch"))));
        assert_eq!(state.diff().ready(), Some(&diff("replacement patch")));
        assert!(!state.diff_is_refreshing());
    }

    #[test]
    fn cached_diff_is_visible_during_refresh_and_final_failure_replaces_it() {
        let mut state = WorkspaceState::new(snapshot("/repo", "feature-a", &[("feature-a", true)]));
        let (token, _) = state.begin_hydration().unwrap();

        assert!(state.apply_cached_diff(token.clone(), diff("cached patch")));
        assert_eq!(state.diff().ready(), Some(&diff("cached patch")));
        assert!(state.diff_is_refreshing());

        assert!(state.apply_diff(token, Err("fresh diff failed".into())));
        assert_eq!(state.diff().error(), Some("fresh diff failed"));
        assert!(!state.diff_is_refreshing());
    }

    #[test]
    fn cached_diff_does_not_replace_a_ready_patch_retained_for_retry() {
        let mut state = WorkspaceState::new(snapshot("/repo", "feature-a", &[("feature-a", true)]));
        let (first, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(first, Ok(diff("visible patch"))));
        let (retry, _) = state.begin_hydration().unwrap();

        assert!(!state.apply_cached_diff(retry, diff("older cached patch")));

        assert_eq!(state.diff().ready(), Some(&diff("visible patch")));
        assert!(state.diff_is_refreshing());
    }

    #[test]
    fn selecting_a_different_branch_never_retains_the_prior_patch() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        let (first, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(first, Ok(diff("feature-a patch"))));

        state.select_branch("feature-b").unwrap();
        let (_, _) = state.begin_hydration().unwrap();

        assert_eq!(state.diff(), &LoadState::Loading);
        assert!(state.diff_is_refreshing());
    }

    #[test]
    fn snapshot_refresh_preserves_selected_branch_patch_while_rehydrating() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        state.select_branch("feature-b").unwrap();
        let (first, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(first, Ok(diff("feature-b patch"))));

        state.replace_snapshot(snapshot(
            "/repo",
            "feature-a",
            &[("feature-b", false), ("feature-a", true)],
        ));
        let (_, _) = state.begin_hydration().unwrap();

        assert_eq!(state.selected_branch(), Some("feature-b"));
        assert_eq!(state.diff().ready(), Some(&diff("feature-b patch")));
        assert!(state.diff_is_refreshing());
    }

    #[test]
    fn arrow_navigation_stops_at_boundaries_without_changing_current_branch() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-b",
            &[("feature-a", false), ("feature-b", true), ("main", false)],
        ));

        assert!(state.move_selection(SelectionDirection::Previous));
        assert_eq!(state.selected_branch(), Some("feature-a"));
        assert_eq!(state.snapshot().current_branch, "feature-b");
        let generation = state.generation();

        assert!(!state.move_selection(SelectionDirection::Previous));
        assert_eq!(state.selected_branch(), Some("feature-a"));
        assert_eq!(state.generation(), generation);

        assert!(state.move_selection(SelectionDirection::Next));
        assert!(state.move_selection(SelectionDirection::Next));
        assert_eq!(state.selected_branch(), Some("main"));
        let generation = state.generation();

        assert!(!state.move_selection(SelectionDirection::Next));
        assert_eq!(state.selected_branch(), Some("main"));
        assert_eq!(state.generation(), generation);
        assert_eq!(state.snapshot().current_branch, "feature-b");
    }

    #[test]
    fn refresh_preserves_selection_then_falls_back_to_current_or_first_branch() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        state.select_branch("feature-b").unwrap();

        state.replace_snapshot(snapshot(
            "/repo",
            "feature-a",
            &[("feature-b", false), ("feature-a", true)],
        ));
        assert_eq!(state.selected_branch(), Some("feature-b"));

        state.replace_snapshot(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-c", false)],
        ));
        assert_eq!(state.selected_branch(), Some("feature-a"));

        state.replace_snapshot(snapshot(
            "/repo",
            "detached",
            &[("feature-c", false), ("feature-d", false)],
        ));
        assert_eq!(state.selected_branch(), Some("feature-c"));

        state.replace_snapshot(snapshot("/repo", "detached", &[]));
        assert_eq!(state.selected_branch(), None);
        assert_eq!(state.details(), &LoadState::Idle);
        assert_eq!(state.diff(), &LoadState::Idle);
        assert_eq!(state.ci(), &LoadState::Idle);
    }
}
