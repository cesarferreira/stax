mod branch_name;
mod checkout;
mod ci;
mod create;
mod delete;
mod history;
pub mod interaction;
mod model;
mod move_subtree;
mod operation;
mod pull_request;
mod rename;
mod reorder;
mod repository;
mod restack;
pub(crate) mod submit;
mod topology;
mod track_plan;

pub(crate) use branch_name::{
    BranchNameContext, BranchNameError, BranchNameResult, format_branch_name,
};
pub use interaction::{
    ActionAvailability, InteractionState, descendants_of, interaction_state,
    interaction_state_from_transaction, linear_stack_order, move_parent_candidates,
};
pub use model::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, DiffLine,
    DiffLineKind, DiffStatLine, RepositorySnapshot,
};
pub use operation::{
    CheckoutOutcome, NativeStackAdvisory, NoopOperationReporter, OperationError,
    OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome, OperationProgress,
    OperationReceipt, OperationReporter, OperationRequest, OperationResult, OperationSideEffects,
    OperationStage, OperationWarning, PullRequestChange, PullRequestMode, PullRequestReceipt,
    RestackScope, TransactionStatus, TransactionSummary,
};
pub use repository::{RepositorySession, execute_repository_operation};
pub(crate) use restack::RestackExecutionOptions;
#[allow(unused_imports)]
pub(crate) use submit::{
    PreparedSubmit, SubmitConfigSources, SubmitOptions, SubmitPreferences, SubmitPromptAnswer,
    SubmitPromptRequest, SubmitScope,
};
pub use topology::{TopologyCell, TopologyNode, TopologyRow, layout as topology_layout};
pub use track_plan::{
    FetchPlan, ParentDecision, ParentSource, RepoFacts, TrackCandidate, branches_needing_upstream,
    newly_created_branches, parse_branches_with_upstream, plan_fetches, resolve_parent,
    topological_order,
};
