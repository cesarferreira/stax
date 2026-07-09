use serde::Serialize;

pub(crate) const SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_DIFF_TEXT_BYTES: usize = 448 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopAction {
    Checkout,
    Restack,
    SubmitStack,
    OpenPr,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProgressEvent<'a> {
    pub(crate) schema_version: u32,
    pub(crate) request_id: &'a str,
    #[serde(rename = "type")]
    pub(crate) event_type: &'static str,
    pub(crate) phase: &'a str,
    pub(crate) message: &'a str,
}

#[derive(Debug, Serialize)]
pub(crate) struct TerminalEvent<'a, T: Serialize> {
    pub(crate) schema_version: u32,
    pub(crate) request_id: &'a str,
    #[serde(rename = "type")]
    pub(crate) event_type: &'static str,
    pub(crate) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<DesktopError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DesktopError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) details: String,
    pub(crate) recovery: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryState {
    Normal,
    RebaseInProgress,
    ConflictInProgress,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecommendedAction {
    None,
    Checkout,
    Restack,
    SubmitStack,
    OpenPr,
}

#[derive(Debug, Serialize)]
pub(crate) struct PullRequestSnapshot {
    pub(crate) number: u64,
    pub(crate) state: String,
    pub(crate) is_draft: bool,
    pub(crate) url: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BranchSnapshot {
    pub(crate) name: String,
    pub(crate) parent: Option<String>,
    pub(crate) column: usize,
    pub(crate) is_current: bool,
    pub(crate) is_trunk: bool,
    pub(crate) ahead: usize,
    pub(crate) behind: usize,
    pub(crate) needs_restack: bool,
    pub(crate) has_remote: bool,
    pub(crate) unpushed: usize,
    pub(crate) unpulled: usize,
    pub(crate) pull_request: Option<PullRequestSnapshot>,
    pub(crate) ci_state: Option<String>,
    pub(crate) recommended_action: RecommendedAction,
}

#[derive(Debug, Serialize)]
pub(crate) struct RepositorySnapshot {
    pub(crate) generation: String,
    pub(crate) repository_path: String,
    pub(crate) repository_name: String,
    pub(crate) trunk: String,
    pub(crate) current_branch: String,
    pub(crate) repository_state: RepositoryState,
    pub(crate) dirty: bool,
    pub(crate) branches: Vec<BranchSnapshot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiffLineKind {
    File,
    Hunk,
    Context,
    Addition,
    Deletion,
    Metadata,
}

#[derive(Debug, Serialize)]
pub(crate) struct DiffFileSnapshot {
    pub(crate) path: String,
    pub(crate) additions: usize,
    pub(crate) deletions: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct DiffLineSnapshot {
    pub(crate) kind: DiffLineKind,
    pub(crate) text: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DiffSnapshot {
    pub(crate) generation: String,
    pub(crate) branch: String,
    pub(crate) parent: String,
    pub(crate) additions: usize,
    pub(crate) deletions: usize,
    pub(crate) files: Vec<DiffFileSnapshot>,
    pub(crate) lines: Vec<DiffLineSnapshot>,
    pub(crate) truncated: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ActionResult {
    pub(crate) action: &'static str,
    pub(crate) branch: Option<String>,
    pub(crate) summary: String,
}

impl DesktopError {
    pub(crate) fn unsupported_schema(received: u32) -> Self {
        Self {
            code: "unsupported_schema",
            message: format!("Desktop schema {received} is not supported."),
            details: format!("This engine supports schema {SCHEMA_VERSION}."),
            recovery: "reinstall_app",
        }
    }

    pub(crate) fn operation(
        code: &'static str,
        message: impl Into<String>,
        details: impl Into<String>,
        recovery: &'static str,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: details.into(),
            recovery,
        }
    }
}

impl<'a, T: Serialize> TerminalEvent<'a, T> {
    pub(crate) fn success(request_id: &'a str, data: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id,
            event_type: "result",
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub(crate) fn failure(request_id: &'a str, error: DesktopError) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id,
            event_type: "result",
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}
