use crate::ci::CheckRunInfo;
use crate::commands::board::BoardTabSelection;
use crate::forge::PrComment;
use crate::github::board::{
    BoardIssueDetail, BoardIssueSummary, BoardPrDetail, BoardPrFile, BoardPrSummary,
};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardTab {
    PullRequests,
    Issues,
}

impl From<BoardTabSelection> for BoardTab {
    fn from(value: BoardTabSelection) -> Self {
        match value {
            BoardTabSelection::PullRequests => BoardTab::PullRequests,
            BoardTabSelection::Issues => BoardTab::Issues,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardMode {
    List,
    Detail,
    Diff,
    Comments,
    LabelPicker,
    ConfirmMerge,
    Filter,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardTarget {
    Pr(u64),
    Issue(u64),
}

pub enum BoardUpdate {
    Prs(Vec<BoardPrSummary>),
    Issues(Vec<BoardIssueSummary>),
    PrDetail {
        number: u64,
        detail: Box<BoardPrDetail>,
        files: Vec<BoardPrFile>,
        checks: Vec<CheckRunInfo>,
    },
    IssueDetail {
        number: u64,
        detail: Box<BoardIssueDetail>,
    },
    Comments {
        target: BoardTarget,
        comments: Vec<PrComment>,
    },
    Diff {
        number: u64,
        diff: String,
    },
    RepoLabels(Vec<String>),
    ViewerLogin(String),
    ActionDone {
        message: String,
        refresh: bool,
    },
    DetailError {
        target: BoardTarget,
        message: String,
    },
    Error(String),
}

pub struct BoardApp {
    pub repo_label: String,
    pub tab: BoardTab,
    pub mode: BoardMode,
    /// Whether the side detail preview panel renders next to the list.
    /// Toggled with `v`; independent of the narrow-terminal auto-hide.
    pub show_detail: bool,
    /// Filter the list to items authored by `viewer_login`. Toggled with
    /// `a` and persisted to config; fails open (shows everything) while
    /// `viewer_login` hasn't resolved yet.
    pub mine_only: bool,
    pub viewer_login: Option<String>,
    pub prs: Vec<BoardPrSummary>,
    pub issues: Vec<BoardIssueSummary>,
    pub pr_selected: usize,
    pub issue_selected: usize,
    pub filter: String,
    /// Per-item caches for the lifetime of the session: once a PR/issue's
    /// detail, files, checks, diff, or comments have been fetched they stay
    /// available without re-fetching when navigating away and back.
    pub pr_details: HashMap<u64, BoardPrDetail>,
    pub pr_files: HashMap<u64, Vec<BoardPrFile>>,
    pub pr_checks: HashMap<u64, Vec<CheckRunInfo>>,
    pub issue_details: HashMap<u64, BoardIssueDetail>,
    pub comments: HashMap<BoardTarget, Vec<PrComment>>,
    pub diff_lines: HashMap<u64, Vec<Line<'static>>>,
    pub detail_scroll: u16,
    pub diff_scroll: u16,
    pub comments_scroll: u16,
    pub repo_labels: Vec<String>,
    pub label_cursor: usize,
    pub active_labels: Vec<String>,
    pub loading_list: bool,
    pub loading_detail: Option<BoardTarget>,
    pub loading_diff: Option<u64>,
    pub loading_comments: Option<BoardTarget>,
    pub detail_error: HashMap<BoardTarget, String>,
    pub status_message: Option<String>,
    pub should_quit: bool,
}

const MAX_DIFF_LINES: usize = 20_000;

impl BoardApp {
    pub fn new(repo_label: String, tab: BoardTabSelection, mine_only: bool) -> Self {
        Self {
            repo_label,
            tab: tab.into(),
            mode: BoardMode::List,
            show_detail: true,
            mine_only,
            viewer_login: None,
            prs: Vec::new(),
            issues: Vec::new(),
            pr_selected: 0,
            issue_selected: 0,
            filter: String::new(),
            pr_details: HashMap::new(),
            pr_files: HashMap::new(),
            pr_checks: HashMap::new(),
            issue_details: HashMap::new(),
            comments: HashMap::new(),
            diff_lines: HashMap::new(),
            detail_scroll: 0,
            diff_scroll: 0,
            comments_scroll: 0,
            repo_labels: Vec::new(),
            label_cursor: 0,
            active_labels: Vec::new(),
            loading_list: true,
            loading_detail: None,
            loading_diff: None,
            loading_comments: None,
            detail_error: HashMap::new(),
            status_message: None,
            should_quit: false,
        }
    }

    pub fn visible_pr_indices(&self) -> Vec<usize> {
        self.prs
            .iter()
            .enumerate()
            .filter(|(_, pr)| {
                matches_filter(&self.filter, &pr_haystack(pr)) && self.passes_mine_only(&pr.author)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn visible_issue_indices(&self) -> Vec<usize> {
        self.issues
            .iter()
            .enumerate()
            .filter(|(_, issue)| {
                matches_filter(&self.filter, &issue_haystack(issue))
                    && self.passes_mine_only(&issue.author)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// `mine_only` fails open (shows everything) until `viewer_login` is
    /// known, so the list never looks empty while the viewer lookup is
    /// still in flight.
    fn passes_mine_only(&self, author: &str) -> bool {
        if !self.mine_only {
            return true;
        }
        match &self.viewer_login {
            Some(login) => author.eq_ignore_ascii_case(login),
            None => true,
        }
    }

    fn clamp_pr_selection(&mut self) {
        let len = self.visible_pr_indices().len();
        if self.pr_selected >= len {
            self.pr_selected = len.saturating_sub(1);
        }
    }

    fn clamp_issue_selection(&mut self) {
        let len = self.visible_issue_indices().len();
        if self.issue_selected >= len {
            self.issue_selected = len.saturating_sub(1);
        }
    }

    pub fn select_next(&mut self) {
        match self.tab {
            BoardTab::PullRequests => {
                let len = self.visible_pr_indices().len();
                if len > 0 && self.pr_selected + 1 < len {
                    self.pr_selected += 1;
                }
            }
            BoardTab::Issues => {
                let len = self.visible_issue_indices().len();
                if len > 0 && self.issue_selected + 1 < len {
                    self.issue_selected += 1;
                }
            }
        }
    }

    pub fn select_previous(&mut self) {
        match self.tab {
            BoardTab::PullRequests => {
                if self.pr_selected > 0 {
                    self.pr_selected -= 1;
                }
            }
            BoardTab::Issues => {
                if self.issue_selected > 0 {
                    self.issue_selected -= 1;
                }
            }
        }
    }

    pub fn select_first(&mut self) {
        match self.tab {
            BoardTab::PullRequests => self.pr_selected = 0,
            BoardTab::Issues => self.issue_selected = 0,
        }
    }

    pub fn select_last(&mut self) {
        match self.tab {
            BoardTab::PullRequests => {
                self.pr_selected = self.visible_pr_indices().len().saturating_sub(1)
            }
            BoardTab::Issues => {
                self.issue_selected = self.visible_issue_indices().len().saturating_sub(1)
            }
        }
    }

    pub fn switch_tab(&mut self, tab: BoardTab) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        self.clear_detail();
    }

    /// Toggles the side detail preview panel. Hiding it also drops out of
    /// `Detail` mode, since there'd be nothing visible left to scroll.
    pub fn toggle_show_detail(&mut self) {
        self.show_detail = !self.show_detail;
        if !self.show_detail && self.mode == BoardMode::Detail {
            self.mode = BoardMode::List;
        }
    }

    /// Toggles the "mine only" filter and re-clamps selection, since
    /// narrowing or widening the visible set can shift the currently
    /// selected row out of range.
    pub fn toggle_mine_only(&mut self) {
        self.mine_only = !self.mine_only;
        self.clamp_pr_selection();
        self.clamp_issue_selection();
    }

    pub fn selected_pr(&self) -> Option<&BoardPrSummary> {
        let index = *self.visible_pr_indices().get(self.pr_selected)?;
        self.prs.get(index)
    }

    pub fn selected_issue(&self) -> Option<&BoardIssueSummary> {
        let index = *self.visible_issue_indices().get(self.issue_selected)?;
        self.issues.get(index)
    }

    pub fn selected_target(&self) -> Option<BoardTarget> {
        match self.tab {
            BoardTab::PullRequests => self.selected_pr().map(|pr| BoardTarget::Pr(pr.number)),
            BoardTab::Issues => self
                .selected_issue()
                .map(|issue| BoardTarget::Issue(issue.number)),
        }
    }

    pub fn selected_url(&self) -> Option<String> {
        match self.tab {
            BoardTab::PullRequests => self.selected_pr().map(|pr| pr.url.clone()),
            BoardTab::Issues => self.selected_issue().map(|issue| issue.url.clone()),
        }
    }

    /// `(number, head_sha)` when the currently selected PR's detail is loaded
    /// and it is not a draft.
    pub fn merge_target(&self) -> Option<(u64, String)> {
        let pr = self.selected_pr()?;
        let detail = self.pr_details.get(&pr.number)?;
        if detail.is_draft {
            return None;
        }
        Some((detail.number, detail.head_sha.clone()))
    }

    /// Resets transient view state when switching tabs. Per-item caches
    /// (`pr_details`, `issue_details`, `comments`, `diff_lines`, ...) are
    /// intentionally left intact — they persist for the whole session so
    /// revisiting an item doesn't re-fetch it.
    pub fn clear_detail(&mut self) {
        self.detail_scroll = 0;
        self.diff_scroll = 0;
        self.comments_scroll = 0;
        self.loading_detail = None;
        self.loading_diff = None;
        self.loading_comments = None;
    }

    pub fn footer_hints(&self) -> Vec<(&'static str, &'static str)> {
        match self.mode {
            BoardMode::List => vec![
                ("Tab", "tab"),
                ("j/k", "move"),
                ("Enter", "detail"),
                ("d", "diff"),
                ("c", "comments"),
                ("l", "labels"),
                ("t", "draft"),
                ("m", "merge"),
                ("o", "open"),
                ("r", "refresh"),
                ("/", "filter"),
                (
                    "v",
                    if self.show_detail {
                        "hide detail"
                    } else {
                        "show detail"
                    },
                ),
                (
                    "a",
                    if self.mine_only {
                        "show all"
                    } else {
                        "show mine"
                    },
                ),
                ("?", "help"),
                ("q", "quit"),
            ],
            BoardMode::Detail => vec![
                ("d", "diff"),
                ("c", "comments"),
                ("l", "labels"),
                ("t", "draft"),
                ("m", "merge"),
                ("o", "open"),
                ("v", "hide detail"),
                ("Esc", "back"),
            ],
            BoardMode::Diff | BoardMode::Comments => vec![("j/k", "scroll"), ("Esc", "back")],
            BoardMode::LabelPicker => vec![("j/k", "move"), ("Space", "toggle"), ("Esc", "close")],
            BoardMode::ConfirmMerge => vec![("y", "confirm"), ("n/Esc", "cancel")],
            BoardMode::Filter => vec![("Enter", "apply"), ("Esc", "cancel")],
            BoardMode::Help => vec![("any key", "close")],
        }
    }

    pub fn apply_update(&mut self, update: BoardUpdate) {
        match update {
            BoardUpdate::Prs(prs) => {
                self.prs = prs;
                self.loading_list = false;
                self.clamp_pr_selection();
            }
            BoardUpdate::Issues(issues) => {
                self.issues = issues;
                self.loading_list = false;
                self.clamp_issue_selection();
            }
            BoardUpdate::PrDetail {
                number,
                detail,
                files,
                checks,
            } => {
                if self.loading_detail == Some(BoardTarget::Pr(number)) {
                    self.loading_detail = None;
                }
                self.pr_details.insert(number, *detail);
                self.pr_files.insert(number, files);
                self.pr_checks.insert(number, checks);
                self.detail_error.remove(&BoardTarget::Pr(number));
            }
            BoardUpdate::IssueDetail { number, detail } => {
                if self.loading_detail == Some(BoardTarget::Issue(number)) {
                    self.loading_detail = None;
                }
                self.issue_details.insert(number, *detail);
                self.detail_error.remove(&BoardTarget::Issue(number));
            }
            BoardUpdate::Comments { target, comments } => {
                if self.loading_comments == Some(target) {
                    self.loading_comments = None;
                }
                if self.selected_target() == Some(target) {
                    self.comments_scroll = 0;
                }
                self.comments.insert(target, comments);
            }
            BoardUpdate::Diff { number, diff } => {
                if self.loading_diff == Some(number) {
                    self.loading_diff = None;
                }
                if self.selected_target() == Some(BoardTarget::Pr(number)) {
                    self.diff_scroll = 0;
                }
                self.diff_lines.insert(number, build_diff_lines(&diff));
            }
            BoardUpdate::RepoLabels(labels) => {
                self.repo_labels = labels;
            }
            BoardUpdate::ViewerLogin(login) => {
                self.viewer_login = Some(login);
                self.clamp_pr_selection();
                self.clamp_issue_selection();
            }
            BoardUpdate::ActionDone { message, .. } => {
                self.status_message = Some(message);
            }
            BoardUpdate::DetailError { target, message } => {
                if self.loading_detail == Some(target) {
                    self.loading_detail = None;
                }
                self.detail_error.insert(target, message.clone());
                self.status_message = Some(message);
            }
            BoardUpdate::Error(message) => {
                self.status_message = Some(message);
                self.loading_list = false;
                self.loading_detail = None;
                self.loading_diff = None;
                self.loading_comments = None;
            }
        }
    }
}

fn matches_filter(filter: &str, haystack: &str) -> bool {
    if filter.trim().is_empty() {
        return true;
    }
    haystack.to_lowercase().contains(&filter.to_lowercase())
}

fn pr_haystack(pr: &BoardPrSummary) -> String {
    format!(
        "#{} {} {} {} {}",
        pr.number,
        pr.title,
        pr.author,
        pr.head_branch,
        pr.labels.join(" ")
    )
}

fn issue_haystack(issue: &BoardIssueSummary) -> String {
    format!(
        "#{} {} {} {}",
        issue.number,
        issue.title,
        issue.author,
        issue.labels.join(" ")
    )
}

/// Parse a unified diff into styled lines, truncating past `MAX_DIFF_LINES`
/// so a huge PR diff cannot blow up memory/render time. Built once here
/// rather than per-frame in `ui.rs`.
fn build_diff_lines(diff: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = diff.lines().take(MAX_DIFF_LINES).map(diff_line).collect();

    if diff.lines().count() > MAX_DIFF_LINES {
        lines.push(Line::from(Span::styled(
            "… diff truncated",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines
}

fn diff_line(line: &str) -> Line<'static> {
    let style = if line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("+++")
        || line.starts_with("---")
    {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        Style::default().fg(Color::Cyan)
    } else if line.starts_with('+') {
        Style::default().fg(Color::Green)
    } else if line.starts_with('-') {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };
    Line::from(Span::styled(line.to_string(), style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn pr(number: u64, title: &str, labels: &[&str]) -> BoardPrSummary {
        BoardPrSummary {
            number,
            title: title.to_string(),
            author: "octocat".to_string(),
            head_branch: format!("feature/{number}"),
            base_branch: "main".to_string(),
            is_draft: false,
            labels: labels.iter().map(|l| l.to_string()).collect(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            url: format!("https://github.com/o/r/pull/{number}"),
        }
    }

    fn pr_detail(number: u64, is_draft: bool) -> BoardPrDetail {
        BoardPrDetail {
            number,
            title: "title".to_string(),
            body: "body".to_string(),
            head_branch: "feature".to_string(),
            base_branch: "main".to_string(),
            head_sha: "sha123".to_string(),
            is_draft,
            mergeable: Some(true),
            mergeable_state: Some("clean".to_string()),
            additions: 1,
            deletions: 1,
            changed_files: 1,
            comment_count: 0,
            review_comment_count: 0,
            labels: Vec::new(),
            url: format!("https://github.com/o/r/pull/{number}"),
        }
    }

    #[test]
    fn filter_matches_title_author_branch_and_labels() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.prs = vec![
            pr(1, "Add board dashboard", &["cli"]),
            pr(2, "Fix flaky test", &["ci", "bug"]),
        ];

        app.filter = "flaky".to_string();
        assert_eq!(app.visible_pr_indices(), vec![1]);

        app.filter = "bug".to_string();
        assert_eq!(app.visible_pr_indices(), vec![1]);

        app.filter = "feature/1".to_string();
        assert_eq!(app.visible_pr_indices(), vec![0]);

        app.filter = String::new();
        assert_eq!(app.visible_pr_indices(), vec![0, 1]);
    }

    #[test]
    fn mine_only_fails_open_until_viewer_login_known_then_filters() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, true);
        let mut mine = pr(1, "Mine", &[]);
        mine.author = "cesarferreira".to_string();
        let mut theirs = pr(2, "Theirs", &[]);
        theirs.author = "octocat".to_string();
        app.prs = vec![mine, theirs];

        // Viewer login hasn't resolved yet: fail open, show everything.
        assert_eq!(app.visible_pr_indices(), vec![0, 1]);

        app.apply_update(BoardUpdate::ViewerLogin("cesarferreira".to_string()));
        assert_eq!(app.visible_pr_indices(), vec![0]);

        app.toggle_mine_only();
        assert_eq!(app.visible_pr_indices(), vec![0, 1]);

        app.toggle_mine_only();
        assert_eq!(app.visible_pr_indices(), vec![0]);
    }

    #[test]
    fn per_tab_selection_preserved_across_switch_tab() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.prs = vec![pr(1, "a", &[]), pr(2, "b", &[])];
        app.pr_selected = 1;

        app.switch_tab(BoardTab::Issues);
        app.issue_selected = 0;
        app.switch_tab(BoardTab::PullRequests);

        assert_eq!(app.pr_selected, 1);
    }

    #[test]
    fn selection_clamps_on_shorter_refresh() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.apply_update(BoardUpdate::Prs(vec![pr(1, "a", &[]), pr(2, "b", &[])]));
        app.pr_selected = 1;

        app.apply_update(BoardUpdate::Prs(vec![pr(1, "a", &[])]));

        assert_eq!(app.pr_selected, 0);
    }

    #[test]
    fn detail_response_is_cached_regardless_of_current_selection() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.prs = vec![pr(1, "a", &[]), pr(2, "b", &[])];
        app.pr_selected = 0;
        app.loading_detail = Some(BoardTarget::Pr(2));

        app.apply_update(BoardUpdate::PrDetail {
            number: 2,
            detail: Box::new(pr_detail(2, false)),
            files: Vec::new(),
            checks: Vec::new(),
        });

        // A response for a PR that isn't currently selected still gets
        // cached — the point of per-item caching is that background/late
        // responses aren't wasted, they're just waiting to be viewed.
        assert!(app.pr_details.contains_key(&2));
        assert!(app.loading_detail.is_none());
    }

    #[test]
    fn detail_error_surfaces_for_current_selection_and_clears_on_success() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.prs = vec![pr(1, "a", &[])];
        app.pr_selected = 0;
        app.loading_detail = Some(BoardTarget::Pr(1));

        app.apply_update(BoardUpdate::DetailError {
            target: BoardTarget::Pr(1),
            message: "Failed to load PR #1: connection reset".to_string(),
        });

        assert!(app.loading_detail.is_none());
        assert_eq!(
            app.detail_error.get(&BoardTarget::Pr(1)),
            Some(&"Failed to load PR #1: connection reset".to_string())
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("Failed to load PR #1: connection reset")
        );

        app.apply_update(BoardUpdate::PrDetail {
            number: 1,
            detail: Box::new(pr_detail(1, false)),
            files: Vec::new(),
            checks: Vec::new(),
        });

        assert!(!app.detail_error.contains_key(&BoardTarget::Pr(1)));
    }

    #[test]
    fn diff_cache_persists_across_selection_change() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.prs = vec![pr(1, "a", &[]), pr(2, "b", &[])];
        app.pr_selected = 0;

        app.apply_update(BoardUpdate::Diff {
            number: 1,
            diff: "diff --git a/f b/f\n".to_string(),
        });
        assert!(app.diff_lines.contains_key(&1));

        // Select PR #2, then back to PR #1 — the cached diff for #1 must
        // still be there, so re-selecting it doesn't require a re-fetch.
        app.pr_selected = 1;
        assert!(
            app.diff_lines.contains_key(&1),
            "diff cache must persist across selection changes for the session"
        );
        app.pr_selected = 0;
        assert!(app.diff_lines.contains_key(&1));
    }

    #[test]
    fn merge_target_is_none_for_drafts_and_unloaded_details() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.prs = vec![pr(1, "a", &[])];
        app.pr_selected = 0;

        assert_eq!(app.merge_target(), None);

        app.pr_details.insert(1, pr_detail(1, true));
        assert_eq!(app.merge_target(), None);

        app.pr_details.insert(1, pr_detail(1, false));
        assert_eq!(app.merge_target(), Some((1, "sha123".to_string())));
    }

    #[test]
    fn toggle_show_detail_falls_back_to_list_mode_when_hidden() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.mode = BoardMode::Detail;
        assert!(app.show_detail);

        app.toggle_show_detail();
        assert!(!app.show_detail);
        assert_eq!(app.mode, BoardMode::List);

        app.toggle_show_detail();
        assert!(app.show_detail);
    }
}
