use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, OperationError,
    OperationEvent, OperationOutcome, OperationProgress, OperationReceipt, OperationRequest,
    OperationResult, RepositorySnapshot,
};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
};

type SessionDiffKey = (String, Option<String>);
const SESSION_DIFF_CACHE_CAPACITY: usize = 32;

#[derive(Debug, Clone, Default)]
struct SessionDiffCache {
    entries: HashMap<SessionDiffKey, BranchDiff>,
    recency: VecDeque<SessionDiffKey>,
}

impl SessionDiffCache {
    fn get(&mut self, key: &SessionDiffKey) -> Option<BranchDiff> {
        let diff = self.entries.get(key)?.clone();
        self.touch(key);
        Some(diff)
    }

    fn insert(&mut self, key: SessionDiffKey, diff: BranchDiff) {
        self.entries.insert(key.clone(), diff);
        self.touch(&key);
        while self.entries.len() > SESSION_DIFF_CACHE_CAPACITY {
            let Some(evicted) = self.recency.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.recency.clear();
    }

    fn touch(&mut self, key: &SessionDiffKey) {
        if let Some(index) = self.recency.iter().position(|candidate| candidate == key) {
            self.recency.remove(index);
        }
        self.recency.push_back(key.clone());
    }
}

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
    search_query: String,
    search_origin_selection: Option<String>,
    details: LoadState<BranchDetails>,
    diff: LoadState<BranchDiff>,
    diff_refreshing: bool,
    diff_cache: SessionDiffCache,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAvailability {
    pub enabled: bool,
    pub reason: Option<String>,
}

impl ActionAvailability {
    fn enabled() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    fn disabled(reason: impl Into<String>) -> Self {
        Self {
            enabled: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionState {
    pub checkout: ActionAvailability,
    pub create: ActionAvailability,
    pub rename: ActionAvailability,
    pub delete: ActionAvailability,
    pub move_subtree: ActionAvailability,
    pub reorder: ActionAvailability,
    pub undo: ActionAvailability,
    pub redo: ActionAvailability,
    pub restack: ActionAvailability,
    pub restack_all: ActionAvailability,
    pub submit: ActionAvailability,
    pub open_pr: ActionAvailability,
    pub open_repository: ActionAvailability,
    pub refresh: ActionAvailability,
    pub navigation: ActionAvailability,
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
            search_query: String::new(),
            search_origin_selection: None,
            details: LoadState::Idle,
            diff: LoadState::Idle,
            diff_refreshing: false,
            diff_cache: SessionDiffCache::default(),
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

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn filtered_branches(&self) -> Vec<&BranchSummary> {
        let query = self.search_query.to_lowercase();
        self.snapshot
            .branches
            .iter()
            .filter(|branch| query.is_empty() || branch.name.to_lowercase().contains(&query))
            .collect()
    }

    pub fn set_search_query(&mut self, query: impl Into<String>) {
        let query = query.into();
        if self.search_query.is_empty() && !query.is_empty() {
            self.search_origin_selection = self.selected_branch.clone();
        }
        self.search_query = query;
        if self.search_query.is_empty() {
            self.selected_branch = self.search_origin_selection.take().filter(|selected| {
                self.snapshot
                    .branches
                    .iter()
                    .any(|branch| branch.name == *selected)
            });
            if self.selected_branch.is_none() {
                self.selected_branch = self
                    .snapshot
                    .branches
                    .iter()
                    .find(|branch| branch.name == self.snapshot.current_branch)
                    .or_else(|| self.snapshot.branches.first())
                    .map(|branch| branch.name.clone());
            }
        } else if !self.selected_branch.as_deref().is_some_and(|selected| {
            self.filtered_branches()
                .iter()
                .any(|branch| branch.name == selected)
        }) {
            self.selected_branch = self
                .filtered_branches()
                .first()
                .map(|branch| branch.name.clone());
        }
        self.advance_generation();
        self.details = LoadState::Idle;
        self.diff = LoadState::Idle;
        self.diff_refreshing = false;
        self.ci = LoadState::Idle;
    }

    pub fn clear_search(&mut self) {
        self.set_search_query(String::new());
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

    pub fn interaction_state(&self) -> InteractionState {
        if let Some(active) = &self.active_operation {
            let operation_reason = if active.request.is_mutating() {
                "A repository operation is running."
            } else {
                "A pull request operation is running."
            };
            let operation_disabled = ActionAvailability::disabled(operation_reason);
            return InteractionState {
                checkout: operation_disabled.clone(),
                create: operation_disabled.clone(),
                rename: operation_disabled.clone(),
                delete: operation_disabled.clone(),
                move_subtree: operation_disabled.clone(),
                reorder: operation_disabled.clone(),
                undo: operation_disabled.clone(),
                redo: operation_disabled.clone(),
                restack: operation_disabled.clone(),
                restack_all: operation_disabled.clone(),
                submit: operation_disabled.clone(),
                open_pr: operation_disabled.clone(),
                open_repository: operation_disabled.clone(),
                refresh: operation_disabled.clone(),
                navigation: if active.request.is_mutating() {
                    operation_disabled
                } else {
                    self.navigation_availability()
                },
            };
        }

        let selected = self.selected_branch_summary();
        let selected_name = selected
            .map(|branch| branch.name.as_str())
            .unwrap_or("the selected branch");
        let has_non_trunk = self.snapshot.branches.iter().any(|branch| !branch.is_trunk);
        let local_transaction = self
            .last_receipt
            .as_ref()
            .and_then(|receipt| receipt.transaction.as_ref())
            .filter(|transaction| !transaction.changed_remote_refs);
        let reorder_order = selected.and_then(|branch| self.linear_stack_order(&branch.name));

        InteractionState {
            checkout: match selected {
                Some(branch) if !branch.is_current && !branch.is_trunk => {
                    ActionAvailability::enabled()
                }
                Some(branch) if branch.is_trunk => {
                    ActionAvailability::disabled("Select a tracked branch to check out.")
                }
                Some(_) => {
                    ActionAvailability::disabled(format!("{selected_name} is already current."))
                }
                None => ActionAvailability::disabled("Select a branch to check out."),
            },
            create: if selected.is_some() {
                ActionAvailability::enabled()
            } else {
                ActionAvailability::disabled("Open a repository before creating a branch.")
            },
            rename: match selected {
                Some(branch) if branch.is_current && !branch.is_trunk => {
                    ActionAvailability::enabled()
                }
                Some(branch) if branch.is_trunk => {
                    ActionAvailability::disabled("The trunk branch cannot be renamed here.")
                }
                Some(_) => ActionAvailability::disabled("Check out the branch before renaming it."),
                None => ActionAvailability::disabled("Select a branch to rename."),
            },
            delete: match selected {
                Some(branch) if !branch.is_current && !branch.is_trunk => {
                    ActionAvailability::enabled()
                }
                Some(branch) if branch.is_trunk => {
                    ActionAvailability::disabled("The trunk branch cannot be deleted.")
                }
                Some(_) => ActionAvailability::disabled(
                    "Check out another branch before deleting this one.",
                ),
                None => ActionAvailability::disabled("Select a branch to delete."),
            },
            move_subtree: match selected {
                Some(branch)
                    if !branch.is_trunk
                        && !self.move_parent_candidates(&branch.name).is_empty() =>
                {
                    ActionAvailability::enabled()
                }
                Some(branch) if branch.is_trunk => {
                    ActionAvailability::disabled("The trunk branch cannot be moved.")
                }
                Some(_) => ActionAvailability::disabled("No eligible parent branch is available."),
                None => ActionAvailability::disabled("Select a branch to move."),
            },
            reorder: if reorder_order.is_some_and(|order| order.len() >= 2) {
                ActionAvailability::enabled()
            } else {
                ActionAvailability::disabled("Select a linear stack with at least two branches.")
            },
            undo: match local_transaction {
                Some(transaction) if transaction.can_undo => ActionAvailability::enabled(),
                Some(_) => {
                    ActionAvailability::disabled("The latest local operation cannot be undone.")
                }
                None => {
                    ActionAvailability::disabled("No safe local operation is available to undo.")
                }
            },
            redo: match local_transaction {
                Some(transaction) if transaction.can_redo => ActionAvailability::enabled(),
                Some(_) => {
                    ActionAvailability::disabled("The latest local operation cannot be redone.")
                }
                None => {
                    ActionAvailability::disabled("No safe local operation is available to redo.")
                }
            },
            restack: match selected {
                Some(branch) if !branch.is_trunk => ActionAvailability::enabled(),
                Some(_) => ActionAvailability::disabled("Select a tracked branch to restack."),
                None => ActionAvailability::disabled("Select a branch to restack."),
            },
            restack_all: if has_non_trunk {
                ActionAvailability::enabled()
            } else {
                ActionAvailability::disabled("No tracked branches are available to restack.")
            },
            submit: if has_non_trunk {
                ActionAvailability::enabled()
            } else {
                ActionAvailability::disabled("No stack branches are available to submit.")
            },
            open_pr: match selected {
                Some(branch) if !branch.is_trunk => ActionAvailability::enabled(),
                Some(_) => ActionAvailability::disabled("Select a branch with a pull request."),
                None => ActionAvailability::disabled("Select a branch to open its pull request."),
            },
            open_repository: ActionAvailability::enabled(),
            refresh: ActionAvailability::enabled(),
            navigation: self.navigation_availability(),
        }
    }

    pub fn dismiss_operation_presentation(&mut self) {
        self.operation_error = None;
        self.last_receipt = None;
    }

    pub fn descendants_of(&self, source: &str) -> Vec<String> {
        let mut descendants = Vec::new();
        loop {
            let mut changed = false;
            for branch in &self.snapshot.branches {
                let is_descendant = branch.parent.as_deref().is_some_and(|parent| {
                    parent == source || descendants.iter().any(|name| name == parent)
                });
                if branch.name != source && is_descendant && !descendants.contains(&branch.name) {
                    descendants.push(branch.name.clone());
                    changed = true;
                }
            }
            if !changed {
                return descendants;
            }
        }
    }

    pub fn move_parent_candidates(&self, source: &str) -> Vec<String> {
        let Some(source_branch) = self
            .snapshot
            .branches
            .iter()
            .find(|branch| branch.name == source && !branch.is_trunk)
        else {
            return Vec::new();
        };
        let descendants = self.descendants_of(source);
        self.snapshot
            .branches
            .iter()
            .filter(|branch| {
                branch.name != source
                    && source_branch.parent.as_deref() != Some(branch.name.as_str())
                    && !descendants.contains(&branch.name)
            })
            .map(|branch| branch.name.clone())
            .collect()
    }

    pub fn linear_stack_order(&self, branch: &str) -> Option<Vec<String>> {
        let selected = self
            .snapshot
            .branches
            .iter()
            .find(|candidate| candidate.name == branch && !candidate.is_trunk)?;
        let mut root = selected;
        let mut seen = vec![root.name.clone()];
        loop {
            let parent = root.parent.as_deref()?;
            if parent == self.snapshot.trunk {
                break;
            }
            root = self
                .snapshot
                .branches
                .iter()
                .find(|candidate| candidate.name == parent && !candidate.is_trunk)?;
            if seen.contains(&root.name) {
                return None;
            }
            seen.push(root.name.clone());
        }

        let mut order = vec![root.name.clone()];
        let mut current = root.name.as_str();
        loop {
            let children = self
                .snapshot
                .branches
                .iter()
                .filter(|candidate| candidate.parent.as_deref() == Some(current))
                .collect::<Vec<_>>();
            match children.as_slice() {
                [] => return order.contains(&branch.to_string()).then_some(order),
                [child] if !order.contains(&child.name) => {
                    order.push(child.name.clone());
                    current = order.last().expect("order has a child");
                }
                [_] | [_, ..] => return None,
            }
        }
    }

    pub fn present_operation_error(&mut self, error: OperationError) {
        self.operation_error = Some(error);
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
        let cache_key = self.diff_cache_key(name);
        let cached_diff = cache_key.as_ref().and_then(|key| self.diff_cache.get(key));
        self.selected_branch = Some(name.to_owned());
        self.advance_generation();
        self.details = LoadState::Idle;
        if !same_branch || !matches!(self.diff, LoadState::Ready(_)) {
            self.diff = cached_diff.map_or(LoadState::Idle, LoadState::Ready);
        }
        self.diff_refreshing = false;
        self.ci = LoadState::Idle;
        Some(self.current_token(name))
    }

    pub fn move_selection(&mut self, direction: SelectionDirection) -> bool {
        let Some(selected) = self.selected_branch.as_deref() else {
            return false;
        };
        let filtered = self.filtered_branches();
        let Some(index) = filtered.iter().position(|branch| branch.name == selected) else {
            return false;
        };
        let next_index = match direction {
            SelectionDirection::Previous => index.checked_sub(1),
            SelectionDirection::Next => index.checked_add(1).filter(|next| *next < filtered.len()),
        };
        let Some(next_name) = next_index
            .and_then(|next| filtered.get(next))
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
        self.diff_cache.clear();
        let same_repository = previous_repository == snapshot.repository_root;
        if !same_repository {
            self.operation_error = None;
            self.last_receipt = None;
            self.active_operation = None;
            self.search_query.clear();
            self.search_origin_selection = None;
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
        if !self.search_query.is_empty()
            && !self.selected_branch.as_deref().is_some_and(|selected| {
                self.filtered_branches()
                    .iter()
                    .any(|branch| branch.name == selected)
            })
        {
            self.selected_branch = self
                .filtered_branches()
                .first()
                .map(|branch| branch.name.clone());
        }
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
        if let Some(key) = self.diff_cache_key(&token.branch) {
            self.diff_cache.insert(key, diff.clone());
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
        if let Ok(diff) = &result
            && let Some(key) = self.diff_cache_key(&token.branch)
        {
            self.diff_cache.insert(key, diff.clone());
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

    fn diff_cache_key(&self, branch: &str) -> Option<SessionDiffKey> {
        self.snapshot
            .branches
            .iter()
            .find(|summary| summary.name == branch)
            .map(|summary| (summary.name.clone(), summary.parent.clone()))
    }

    fn selected_branch_summary(&self) -> Option<&BranchSummary> {
        self.selected_branch.as_deref().and_then(|selected| {
            self.snapshot
                .branches
                .iter()
                .find(|branch| branch.name == selected)
        })
    }

    fn navigation_availability(&self) -> ActionAvailability {
        if self.snapshot.branches.len() > 1 {
            ActionAvailability::enabled()
        } else {
            ActionAvailability::disabled("No other branches are available.")
        }
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
        OperationOutcome::BranchRenamed { new_name, .. } => Some(new_name.clone()),
        OperationOutcome::BranchDeleted { .. } => None,
        OperationOutcome::SubtreeMoved { source, .. } => Some(source.clone()),
        OperationOutcome::StackReordered { .. } => None,
        OperationOutcome::TransactionUndone { .. }
        | OperationOutcome::TransactionRedone { .. }
        | OperationOutcome::Restacked { .. }
        | OperationOutcome::Submitted { .. }
        | OperationOutcome::PullRequestResolved { .. } => None,
    }
}

fn resolved_url(receipt: &OperationReceipt) -> Option<String> {
    match &receipt.outcome {
        OperationOutcome::PullRequestResolved { url, .. } => Some(url.clone()),
        OperationOutcome::Checkout(_)
        | OperationOutcome::BranchCreated { .. }
        | OperationOutcome::BranchRenamed { .. }
        | OperationOutcome::BranchDeleted { .. }
        | OperationOutcome::SubtreeMoved { .. }
        | OperationOutcome::StackReordered { .. }
        | OperationOutcome::TransactionUndone { .. }
        | OperationOutcome::TransactionRedone { .. }
        | OperationOutcome::Restacked { .. }
        | OperationOutcome::Submitted { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadState, SelectionDirection, WorkspaceState};
    use stax::application::{
        BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, OperationOutcome,
        OperationReceipt, OperationRequest, OperationSideEffects, RepositorySnapshot,
        TransactionStatus, TransactionSummary,
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

    fn tracked_branch(
        name: &str,
        parent: Option<&str>,
        is_current: bool,
        is_trunk: bool,
    ) -> BranchSummary {
        BranchSummary {
            name: name.into(),
            parent: parent.map(str::to_string),
            column: 0,
            is_current,
            is_trunk,
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            ci_state: None,
        }
    }

    fn structural_snapshot() -> RepositorySnapshot {
        RepositorySnapshot {
            repository_root: PathBuf::from("/repo"),
            current_branch: "parent".into(),
            trunk: "main".into(),
            branches: vec![
                tracked_branch("main", None, false, true),
                tracked_branch("parent", Some("main"), true, false),
                tracked_branch("child", Some("parent"), false, false),
            ],
        }
    }

    fn history_receipt(changed_remote_refs: bool) -> OperationReceipt {
        OperationReceipt {
            request: OperationRequest::UndoTransaction {
                operation_id: Some("op-1".into()),
                update_remote: false,
            },
            summary: "History available".into(),
            affected_branches: vec!["parent".into()],
            outcome: OperationOutcome::TransactionUndone {
                operation_id: "op-1".into(),
                changed_refs: vec!["refs/heads/parent".into()],
            },
            transaction: Some(TransactionSummary {
                id: "op-1".into(),
                kind: "rename".into(),
                status: TransactionStatus::Succeeded,
                branches: vec!["parent".into()],
                can_undo: true,
                can_redo: true,
                changed_remote_refs,
            }),
            warnings: Vec::new(),
            side_effects: OperationSideEffects::RepositoryChanged,
        }
    }

    fn present_receipt(state: &mut WorkspaceState, receipt: OperationReceipt) {
        let request = receipt.request.clone();
        let token = state.begin_operation(request).unwrap();
        state.finish_operation(&token, Ok(receipt)).unwrap();
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
    fn interaction_structural_actions_follow_selection_and_stack_shape() {
        let mut state = WorkspaceState::new(structural_snapshot());

        let current = state.interaction_state();
        assert!(current.rename.enabled);
        assert!(!current.delete.enabled);
        assert!(!current.move_subtree.enabled);
        assert!(current.reorder.enabled);

        state.select_branch("child").unwrap();
        let selected = state.interaction_state();
        assert!(!selected.rename.enabled);
        assert!(selected.delete.enabled);
        assert!(selected.move_subtree.enabled);
        assert!(selected.reorder.enabled);
        assert_eq!(state.descendants_of("parent"), vec!["child"]);
        assert_eq!(state.move_parent_candidates("child"), vec!["main"]);
        assert_eq!(
            state.linear_stack_order("child"),
            Some(vec!["parent".into(), "child".into()])
        );
    }

    #[test]
    fn interaction_history_is_available_only_for_local_receipts() {
        let mut local = WorkspaceState::new(structural_snapshot());
        present_receipt(&mut local, history_receipt(false));
        let interaction = local.interaction_state();
        assert!(interaction.undo.enabled);
        assert!(interaction.redo.enabled);

        let mut remote = WorkspaceState::new(structural_snapshot());
        present_receipt(&mut remote, history_receipt(true));
        let interaction = remote.interaction_state();
        assert!(!interaction.undo.enabled);
        assert!(!interaction.redo.enabled);
    }

    #[test]
    fn interaction_mutation_disables_every_structural_action() {
        let mut state = WorkspaceState::new(structural_snapshot());
        state
            .begin_operation(OperationRequest::DeleteBranch {
                branch: "child".into(),
                force: true,
            })
            .unwrap();

        let interaction = state.interaction_state();
        assert!(!interaction.rename.enabled);
        assert!(!interaction.delete.enabled);
        assert!(!interaction.move_subtree.enabled);
        assert!(!interaction.reorder.enabled);
        assert!(!interaction.undo.enabled);
        assert!(!interaction.redo.enabled);
    }

    #[test]
    fn interaction_reorder_rejects_forked_stacks() {
        let mut snapshot = structural_snapshot();
        snapshot
            .branches
            .push(tracked_branch("sibling", Some("parent"), false, false));
        let state = WorkspaceState::new(snapshot);

        assert!(!state.interaction_state().reorder.enabled);
        assert_eq!(state.linear_stack_order("parent"), None);
    }

    #[test]
    fn search_filters_case_insensitively_and_restores_the_prior_selection() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("bugfix", false), ("Feature-B", false)],
        ));
        state.select_branch("bugfix").unwrap();

        state.set_search_query("FEATURE");
        assert_eq!(
            state
                .filtered_branches()
                .iter()
                .map(|branch| branch.name.as_str())
                .collect::<Vec<_>>(),
            vec!["feature-a", "Feature-B"]
        );
        assert_eq!(state.selected_branch(), Some("feature-a"));

        state.clear_search();
        assert_eq!(state.search_query(), "");
        assert_eq!(state.selected_branch(), Some("bugfix"));
    }

    #[test]
    fn search_navigation_moves_only_through_filtered_rows() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("bugfix", false), ("feature-b", false)],
        ));

        state.set_search_query("feature");
        assert!(state.move_selection(SelectionDirection::Next));
        assert_eq!(state.selected_branch(), Some("feature-b"));
        assert!(!state.move_selection(SelectionDirection::Next));
        assert!(state.move_selection(SelectionDirection::Previous));
        assert_eq!(state.selected_branch(), Some("feature-a"));

        state.set_search_query("no matches");
        assert!(state.filtered_branches().is_empty());
        assert_eq!(state.selected_branch(), None);
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
    fn returning_to_a_branch_restores_its_accepted_cached_patch() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        let (a, _) = state.begin_hydration().unwrap();
        assert_eq!(state.diff(), &LoadState::Loading);
        assert!(state.apply_cached_diff(a, diff("cached a")));

        state.select_branch("feature-b").unwrap();
        let (b, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(b, Ok(diff("b"))));
        state.select_branch("feature-a").unwrap();

        assert_eq!(state.diff().ready(), Some(&diff("cached a")));
        assert!(!state.diff_is_refreshing());
    }

    #[test]
    fn returning_to_a_visited_branch_restores_its_ready_patch() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        let (a, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(a, Ok(diff("a"))));

        state.select_branch("feature-b").unwrap();
        let (b, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(b, Ok(diff("b"))));
        state.select_branch("feature-a").unwrap();

        assert_eq!(state.diff().ready(), Some(&diff("a")));
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
    fn snapshot_refresh_invalidates_other_branch_session_patches() {
        let mut state = WorkspaceState::new(snapshot(
            "/repo",
            "feature-a",
            &[("feature-a", true), ("feature-b", false)],
        ));
        let (a, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(a, Ok(diff("old a"))));
        state.select_branch("feature-b").unwrap();
        let (b, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(b, Ok(diff("old b"))));

        state.replace_snapshot(snapshot(
            "/repo",
            "feature-a",
            &[("feature-b", false), ("feature-a", true)],
        ));
        let (_, _) = state.begin_hydration().unwrap();

        assert_eq!(state.diff().ready(), Some(&diff("old b")));
        assert!(state.diff_is_refreshing());
        state.select_branch("feature-a").unwrap();
        assert_eq!(state.diff(), &LoadState::Idle);
    }

    #[test]
    fn session_diff_cache_evicts_the_least_recently_used_entry_after_32_patches() {
        let branches = (0..33)
            .map(|index| {
                tracked_branch(
                    &format!("branch-{index:02}"),
                    Some("main"),
                    index == 0,
                    false,
                )
            })
            .collect();
        let mut state = WorkspaceState::new(RepositorySnapshot {
            repository_root: PathBuf::from("/repo"),
            current_branch: "branch-00".into(),
            trunk: "main".into(),
            branches,
        });

        for index in 0..32 {
            let branch = format!("branch-{index:02}");
            state.select_branch(&branch).unwrap();
            let (token, _) = state.begin_hydration().unwrap();
            assert!(state.apply_diff(token, Ok(diff(&format!("patch-{index:02}")))));
        }

        state.select_branch("branch-00").unwrap();
        assert_eq!(state.diff().ready(), Some(&diff("patch-00")));
        state.select_branch("branch-32").unwrap();
        let (token, _) = state.begin_hydration().unwrap();
        assert!(state.apply_diff(token, Ok(diff("patch-32"))));

        state.select_branch("branch-01").unwrap();
        assert_eq!(state.diff(), &LoadState::Idle);
        state.select_branch("branch-00").unwrap();
        assert_eq!(state.diff().ready(), Some(&diff("patch-00")));
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
