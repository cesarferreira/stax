//! Consolidated integration-test binary.
//!
//! Every `tests/*_tests.rs` file is included here as a module instead of being
//! compiled into its own separate test binary. Cargo links one binary instead
//! of ~50 (each of which statically links the whole stax lib plus heavy deps
//! like octocrab, git2, and ratatui), which is the dominant cost of building
//! the test suite. See `autotests = false` and the `[[test]]` target in
//! `Cargo.toml`.
//!
//! Shared helpers live in `tests/common/`; it is declared once here and each
//! suite module reaches it via `use crate::common;`.

mod common;

#[path = "abort_tests.rs"]
mod abort_tests;
#[path = "absorb_tests.rs"]
mod absorb_tests;
#[path = "additional_coverage_tests.rs"]
mod additional_coverage_tests;
#[path = "auth_tests.rs"]
mod auth_tests;
#[path = "changelog_tests.rs"]
mod changelog_tests;
#[path = "ci_tests.rs"]
mod ci_tests;
#[path = "cli_tests.rs"]
mod cli_tests;
#[path = "command_coverage_tests.rs"]
mod command_coverage_tests;
#[path = "comments_tests.rs"]
mod comments_tests;
#[path = "comprehensive_coverage_tests.rs"]
mod comprehensive_coverage_tests;
#[path = "conflict_handling_tests.rs"]
mod conflict_handling_tests;
#[path = "continue_tests.rs"]
mod continue_tests;
#[path = "copy_tests.rs"]
mod copy_tests;
#[path = "create_ai_tests.rs"]
mod create_ai_tests;
#[path = "create_below_tests.rs"]
mod create_below_tests;
#[path = "create_insert_tests.rs"]
mod create_insert_tests;
#[path = "create_rollback_tests.rs"]
mod create_rollback_tests;
#[path = "demo_tests.rs"]
mod demo_tests;
#[path = "detach_tests.rs"]
mod detach_tests;
#[path = "doctor_fix_tests.rs"]
mod doctor_fix_tests;
#[path = "downstack_tests.rs"]
mod downstack_tests;
#[path = "edge_cases_tests.rs"]
mod edge_cases_tests;
#[path = "edit_tests.rs"]
mod edit_tests;
#[path = "fix_tests.rs"]
mod fix_tests;
#[path = "fold_tests.rs"]
mod fold_tests;
#[path = "get_tests.rs"]
mod get_tests;
#[path = "github_list_tests.rs"]
mod github_list_tests;
#[path = "integration_tests.rs"]
mod integration_tests;
#[path = "navigation_tests.rs"]
mod navigation_tests;
#[path = "pr_body_tests.rs"]
mod pr_body_tests;
#[path = "pr_template_tests.rs"]
mod pr_template_tests;
#[path = "reorder_tests.rs"]
mod reorder_tests;
#[path = "rerequest_review_tests.rs"]
mod rerequest_review_tests;
#[path = "resolve_tests.rs"]
mod resolve_tests;
#[path = "restack_provenance_tests.rs"]
mod restack_provenance_tests;
#[path = "scoped_submit_tests.rs"]
mod scoped_submit_tests;
#[path = "split_hunk_tests.rs"]
mod split_hunk_tests;
#[path = "split_tests.rs"]
mod split_tests;
#[path = "stack_test_tests.rs"]
mod stack_test_tests;
#[path = "staging_menu_tests.rs"]
mod staging_menu_tests;
#[path = "status_tests.rs"]
mod status_tests;
#[path = "submit_fetch_failure_tests.rs"]
mod submit_fetch_failure_tests;
#[path = "submit_no_verify_tests.rs"]
mod submit_no_verify_tests;
#[path = "sweep_tests.rs"]
mod sweep_tests;
#[path = "track_all_prs_tests.rs"]
mod track_all_prs_tests;
#[path = "track_merge_base_tests.rs"]
mod track_merge_base_tests;
#[path = "tui_commands_tests.rs"]
mod tui_commands_tests;
#[path = "upstack_onto_tests.rs"]
mod upstack_onto_tests;
#[path = "validate_tests.rs"]
mod validate_tests;
#[path = "worktree_cli_tests.rs"]
mod worktree_cli_tests;
#[path = "worktree_seed_tests.rs"]
mod worktree_seed_tests;
#[path = "worktree_tests.rs"]
mod worktree_tests;
