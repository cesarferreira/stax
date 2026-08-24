//! Axum route handlers for `st web`.

#![allow(clippy::result_large_err)]

use crate::application::{
    NoopOperationReporter, OperationOutcome, OperationRequest, PullRequestMode, RestackScope,
    execute_repository_operation, interaction_state,
};
use crate::web::session::{SharedSession, ThemePreference};
use crate::web::static_assets::{APP_CSS, APP_JS, HTMX_JS};
use crate::web::templates::{
    self, changes_pane_placeholder, diff_view, error_fragment, inspector_details,
    inspector_placeholder, stack_pane_inner, workspace_page,
};
use axum::Router;
use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, Response, StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use serde::Deserialize;

type AppState = SharedSession;

pub fn build_router(session: SharedSession) -> Router {
    Router::new()
        // Static assets (no token needed)
        .route("/assets/app.css", get(serve_css))
        .route("/assets/htmx.min.js", get(serve_htmx))
        .route("/assets/app.js", get(serve_appjs))
        // Session routes
        .route("/s/{token}/", get(workspace_handler))
        .route("/s/{token}/stack", get(stack_partial))
        .route("/s/{token}/select", post(select_branch))
        .route("/s/{token}/details", get(branch_details))
        .route("/s/{token}/diff", get(branch_diff))
        .route("/s/{token}/ci", get(ci_summary))
        .route("/s/{token}/search", post(search_branches))
        .route("/s/{token}/panes", post(toggle_panes))
        .route("/s/{token}/theme", post(set_theme))
        .route("/s/{token}/refresh", post(refresh_handler))
        .route("/s/{token}/op/checkout", post(op_checkout))
        .route("/s/{token}/op/create", post(op_create))
        .route("/s/{token}/op/rename", post(op_rename))
        .route("/s/{token}/op/delete", post(op_delete))
        .route("/s/{token}/op/restack", post(op_restack))
        .route("/s/{token}/op/submit", post(op_submit))
        .route("/s/{token}/op/undo", post(op_undo))
        .route("/s/{token}/op/redo", post(op_redo))
        .route("/s/{token}/op/move", post(op_move))
        .route("/s/{token}/op/reorder", post(op_reorder))
        .route("/s/{token}/op/open-pr", get(op_open_pr))
        .route("/s/{token}/project", post(switch_project))
        .with_state(session)
}

// ── Guard helpers ────────────────────────────────────────────────────────────

fn check_token(state: &SharedSession, token: &str) -> Option<Response<Body>> {
    let s = state.lock().unwrap();
    if s.session_token != token {
        return Some((StatusCode::NOT_FOUND, Html("<h1>Not Found</h1>")).into_response());
    }
    None
}

fn check_csrf(state: &SharedSession, csrf: &str) -> Option<Response<Body>> {
    let s = state.lock().unwrap();
    if s.csrf_token != csrf {
        return Some(
            (
                StatusCode::FORBIDDEN,
                Html("<h1>Forbidden — invalid CSRF token</h1>"),
            )
                .into_response(),
        );
    }
    None
}

fn check_local_host(headers: &HeaderMap) -> Option<Response<Body>> {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let host_part = host.split(':').next().unwrap_or(host);
    if host_part != "127.0.0.1" && host_part != "localhost" {
        return Some((StatusCode::FORBIDDEN, Html("<h1>Forbidden</h1>")).into_response());
    }
    None
}

// ── Static assets ────────────────────────────────────────────────────────────

async fn serve_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS)
}

async fn serve_htmx() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        HTMX_JS,
    )
}

async fn serve_appjs() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        APP_JS,
    )
}

// ── Main workspace ───────────────────────────────────────────────────────────

async fn workspace_handler(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }

    let repo_root = state.lock().unwrap().repository_root.clone();

    let result = tokio::task::spawn_blocking(move || {
        let repo_session = crate::application::RepositorySession::open(&repo_root)?;
        repo_session.snapshot()
    })
    .await;

    match result {
        Ok(Ok(snapshot)) => {
            let session = state.lock().unwrap();
            let selected = session.selected_branch.as_deref();
            let active_op = session.active_operation;
            let last_receipt = session.last_receipt.clone();
            let interaction =
                interaction_state(&snapshot, selected, active_op, last_receipt.as_ref());
            let html = workspace_page(&session, &snapshot, &interaction);
            Html(html.into_string()).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(format!("<h1>Repository error</h1><pre>{e}</pre>")),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(format!("<h1>Spawn error</h1><pre>{e}</pre>")),
        )
            .into_response(),
    }
}

// ── Stack pane partial ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct BranchQuery {
    branch: Option<String>,
}

async fn stack_partial(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Query(q): Query<BranchQuery>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }

    if let Some(branch) = &q.branch {
        let mut s = state.lock().unwrap();
        s.selected_branch = Some(branch.clone());
    }

    render_stack_pane(&state, &token).await
}

// ── Select branch ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SelectForm {
    branch: String,
    csrf: String,
}

async fn select_branch(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<SelectForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    {
        let mut s = state.lock().unwrap();
        s.selected_branch = Some(form.branch.clone());
    }

    render_stack_pane(&state, &token).await
}

// ── Branch details (inspector) ────────────────────────────────────────────────

async fn branch_details(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }

    let (repo_root, selected, csrf, last_receipt, active) = {
        let s = state.lock().unwrap();
        (
            s.repository_root.clone(),
            s.selected_branch.clone(),
            s.csrf_token.clone(),
            s.last_receipt.clone(),
            s.active_operation,
        )
    };

    let Some(branch_name) = selected else {
        return Html(inspector_placeholder(None).into_string()).into_response();
    };

    let result = tokio::task::spawn_blocking(move || {
        let repo_session = crate::application::RepositorySession::open(&repo_root)?;
        let snapshot = repo_session.snapshot()?;
        let branch_summary = snapshot
            .branches
            .iter()
            .find(|b| b.name == branch_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Branch not found: {branch_name}"))?;
        let details = repo_session.branch_details(&branch_summary)?;
        let interaction = interaction_state(
            &snapshot,
            Some(branch_summary.name.as_str()),
            active,
            last_receipt.as_ref(),
        );
        let move_candidates =
            crate::application::move_parent_candidates(&snapshot, &branch_summary.name);
        let reorder_order = crate::application::linear_stack_order(&snapshot, &branch_summary.name);
        Ok::<_, anyhow::Error>((
            branch_summary,
            details,
            interaction,
            move_candidates,
            reorder_order,
        ))
    })
    .await;

    match result {
        Ok(Ok((branch, details, interaction, move_candidates, reorder_order))) => {
            let base = format!("/s/{token}");
            Html(
                inspector_details(
                    &branch,
                    &details,
                    &interaction,
                    &base,
                    &csrf,
                    &move_candidates,
                    reorder_order.as_deref(),
                )
                .into_string(),
            )
            .into_response()
        }
        Ok(Err(e)) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
        Err(e) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
    }
}

// ── Branch diff ──────────────────────────────────────────────────────────────

async fn branch_diff(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }

    let (repo_root, selected) = {
        let s = state.lock().unwrap();
        (s.repository_root.clone(), s.selected_branch.clone())
    };

    let Some(branch_name) = selected else {
        return Html(changes_pane_placeholder(None).into_string()).into_response();
    };

    let result = tokio::task::spawn_blocking(move || {
        let repo_session = crate::application::RepositorySession::open(&repo_root)?;
        let snapshot = repo_session.snapshot()?;
        let branch_summary = snapshot
            .branches
            .iter()
            .find(|b| b.name == branch_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Branch not found: {branch_name}"))?;
        let parent = branch_summary.parent.as_deref().unwrap_or(&snapshot.trunk);
        repo_session.diff(&branch_summary.name, parent)
    })
    .await;

    match result {
        Ok(Ok(diff)) => Html(diff_view(&diff).into_string()).into_response(),
        Ok(Err(e)) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
        Err(e) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
    }
}

// ── CI summary ───────────────────────────────────────────────────────────────

async fn ci_summary(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    Html("<!-- CI summary not yet hydrated -->").into_response()
}

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchForm {
    query: Option<String>,
    csrf: String,
}

async fn search_branches(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<SearchForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    {
        let mut s = state.lock().unwrap();
        s.search_query = form.query.unwrap_or_default();
    }

    render_stack_pane(&state, &token).await
}

// ── Toggle panes ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PanesForm {
    pane: String,
    csrf: String,
}

async fn toggle_panes(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<PanesForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    {
        let mut s = state.lock().unwrap();
        match form.pane.as_str() {
            "stack" => s.show_stack = !s.show_stack,
            "changes" => s.show_changes = !s.show_changes,
            "inspector" => s.show_inspector = !s.show_inspector,
            _ => {}
        }
        s.save_prefs();
    }

    Html("").into_response()
}

// ── Theme preference ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ThemeForm {
    theme: String,
    csrf: String,
}

async fn set_theme(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ThemeForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let Some(theme) = ThemePreference::parse(&form.theme) else {
        return (StatusCode::BAD_REQUEST, Html("<h1>Invalid theme</h1>")).into_response();
    };

    {
        let mut s = state.lock().unwrap();
        s.theme = theme;
        s.save_prefs();
    }

    StatusCode::NO_CONTENT.into_response()
}

// ── Refresh ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CsrfForm {
    csrf: String,
}

async fn refresh_handler(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }
    render_stack_pane(&state, &token).await
}

// ── Op: checkout ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CheckoutForm {
    branch: String,
    csrf: String,
}

async fn op_checkout(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CheckoutForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let request = OperationRequest::Checkout {
        branch: form.branch.clone(),
    };
    run_mutation(state, token, request).await
}

// ── Op: create ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateForm {
    name: String,
    parent: String,
    csrf: String,
}

async fn op_create(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CreateForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let request = OperationRequest::CreateBranch {
        name: form.name.clone(),
        parent: form.parent.clone(),
    };
    run_mutation(state, token, request).await
}

// ── Op: rename ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RenameForm {
    branch: String,
    new_name: String,
    csrf: String,
}

async fn op_rename(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<RenameForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let request = OperationRequest::RenameBranch {
        branch: form.branch.clone(),
        new_name: form.new_name.clone(),
    };
    run_mutation(state, token, request).await
}

// ── Op: delete ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DeleteForm {
    branch: String,
    force: Option<String>,
    csrf: String,
}

async fn op_delete(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let request = OperationRequest::DeleteBranch {
        branch: form.branch.clone(),
        force: form.force.as_deref() == Some("true") || form.force.as_deref() == Some("1"),
    };
    run_mutation(state, token, request).await
}

// ── Op: restack ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RestackForm {
    scope: Option<String>,
    auto_stash: Option<String>,
    csrf: String,
}

async fn op_restack(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<RestackForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let current = state.lock().unwrap().selected_branch.clone();

    let scope = match form.scope.as_deref() {
        Some("all") => RestackScope::All,
        _ => {
            if let Some(branch) = current {
                RestackScope::Branch(branch)
            } else {
                return Html(error_fragment("No branch selected for restack.").into_string())
                    .into_response();
            }
        }
    };
    let auto_stash = form.auto_stash.as_deref() == Some("true");

    let request = OperationRequest::Restack { scope, auto_stash };
    run_mutation(state, token, request).await
}

// ── Op: submit ───────────────────────────────────────────────────────────────

async fn op_submit(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let request = OperationRequest::SubmitStack {
        new_pull_requests: PullRequestMode::Draft,
    };
    run_mutation(state, token, request).await
}

// ── Op: undo ─────────────────────────────────────────────────────────────────

async fn op_undo(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let request = OperationRequest::UndoTransaction {
        operation_id: None,
        update_remote: false,
    };
    run_mutation(state, token, request).await
}

// ── Op: redo ─────────────────────────────────────────────────────────────────

async fn op_redo(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let request = OperationRequest::RedoTransaction {
        operation_id: None,
        update_remote: false,
    };
    run_mutation(state, token, request).await
}

// ── Op: move subtree ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MoveForm {
    source: String,
    new_parent: String,
    auto_stash: Option<String>,
    csrf: String,
}

async fn op_move(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<MoveForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let auto_stash = form.auto_stash.as_deref() == Some("true");
    let request = OperationRequest::MoveSubtree {
        source: form.source.clone(),
        new_parent: form.new_parent.clone(),
        auto_stash,
    };
    run_mutation(state, token, request).await
}

// ── Op: reorder stack ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ReorderForm {
    /// Comma-separated bottom-to-top proposed order.
    proposed_order: String,
    /// Comma-separated original order used to create the preview.
    original_order: String,
    auto_stash: Option<String>,
    csrf: String,
}

async fn op_reorder(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ReorderForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let proposed_order = form
        .proposed_order
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let original_order = form
        .original_order
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let auto_stash = form.auto_stash.as_deref() == Some("true");
    let request = OperationRequest::ReorderStack {
        original_order,
        proposed_order,
        auto_stash,
    };
    run_mutation(state, token, request).await
}

// ── Switch / add project ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ProjectForm {
    path: String,
    csrf: String,
}

async fn switch_project(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ProjectForm>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }
    if let Some(r) = check_csrf(&state, &form.csrf) {
        return r;
    }

    let path = std::path::PathBuf::from(form.path.trim());
    let opened = tokio::task::spawn_blocking({
        let path = path.clone();
        move || crate::application::RepositorySession::open(&path)
    })
    .await;

    match opened {
        Ok(Ok(session)) => {
            let root = session.repository_root().to_path_buf();
            let snap = tokio::task::spawn_blocking({
                let root = root.clone();
                move || crate::application::RepositorySession::open(&root)?.snapshot()
            })
            .await;
            let selected = snap
                .ok()
                .and_then(|r| r.ok())
                .map(|snap| snap.current_branch);
            {
                let mut s = state.lock().unwrap();
                s.switch_repository(root, selected);
            }
            // Full page reload for the new workspace.
            (
                [(
                    axum::http::HeaderName::from_static("hx-redirect"),
                    axum::http::HeaderValue::from_str(&format!("/s/{token}/"))
                        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("/")),
                )],
                StatusCode::OK,
            )
                .into_response()
        }
        Ok(Err(e)) => {
            Html(error_fragment(&format!("Failed to open repository: {e:#}")).into_string())
                .into_response()
        }
        Err(e) => Html(error_fragment(&format!("Spawn error: {e}")).into_string()).into_response(),
    }
}

// ── Op: open PR ──────────────────────────────────────────────────────────────

async fn op_open_pr(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Query(q): Query<BranchQuery>,
) -> impl IntoResponse {
    if let Some(r) = check_local_host(&headers) {
        return r;
    }
    if let Some(r) = check_token(&state, &token) {
        return r;
    }

    let (repo_root, branch) = {
        let s = state.lock().unwrap();
        let b = q.branch.clone().or_else(|| s.selected_branch.clone());
        (s.repository_root.clone(), b)
    };

    let Some(branch_name) = branch else {
        return Html(error_fragment("No branch selected.").into_string()).into_response();
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut reporter = NoopOperationReporter;
        execute_repository_operation(
            &repo_root,
            OperationRequest::ResolvePullRequestUrl {
                branch: branch_name,
            },
            &mut reporter,
        )
    })
    .await;

    match result {
        Ok(Ok(receipt)) => match receipt.outcome {
            OperationOutcome::PullRequestResolved { url, .. } => {
                axum::response::Redirect::temporary(&url).into_response()
            }
            _ => Html(error_fragment("No PR found for this branch.").into_string()).into_response(),
        },
        Ok(Err(e)) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
        Err(e) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
    }
}

// ── Shared mutation runner ────────────────────────────────────────────────────

async fn run_mutation(
    state: SharedSession,
    token: String,
    request: OperationRequest,
) -> axum::response::Response<Body> {
    let repo_root = state.lock().unwrap().repository_root.clone();

    {
        let mut s = state.lock().unwrap();
        s.active_operation = true;
    }

    let result = tokio::task::spawn_blocking(move || {
        let mut reporter = NoopOperationReporter;
        execute_repository_operation(&repo_root, request, &mut reporter)
    })
    .await;

    let (success, message, receipt) = match result {
        Ok(Ok(receipt)) => {
            let msg = format!("✓ {}", receipt.summary);
            (true, msg, Some(receipt))
        }
        Ok(Err(e)) => (false, format!("✗ {e}"), None),
        Err(e) => (false, format!("✗ Spawn error: {e}"), None),
    };

    {
        let mut s = state.lock().unwrap();
        s.active_operation = false;
        if let Some(r) = receipt {
            s.last_receipt = Some(r);
        }
    }

    render_stack_pane_with_banner(&state, &token, &message, success).await
}

// ── Render helpers ────────────────────────────────────────────────────────────

async fn render_stack_pane(state: &SharedSession, token: &str) -> axum::response::Response<Body> {
    let repo_root = state.lock().unwrap().repository_root.clone();
    let result = tokio::task::spawn_blocking(move || {
        let repo_session = crate::application::RepositorySession::open(&repo_root)?;
        repo_session.snapshot()
    })
    .await;

    match result {
        Ok(Ok(snapshot)) => {
            let session = state.lock().unwrap();
            let selected = session.selected_branch.as_deref();
            let active_op = session.active_operation;
            let last_receipt = session.last_receipt.clone();
            let interaction =
                interaction_state(&snapshot, selected, active_op, last_receipt.as_ref());
            let base = format!("/s/{token}");
            Html(stack_pane_inner(&session, &snapshot, &interaction, &base).into_string())
                .into_response()
        }
        Ok(Err(e)) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
        Err(e) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
    }
}

async fn render_stack_pane_with_banner(
    state: &SharedSession,
    token: &str,
    banner_msg: &str,
    success: bool,
) -> axum::response::Response<Body> {
    let repo_root = state.lock().unwrap().repository_root.clone();
    let result = tokio::task::spawn_blocking(move || {
        let repo_session = crate::application::RepositorySession::open(&repo_root)?;
        repo_session.snapshot()
    })
    .await;

    match result {
        Ok(Ok(snapshot)) => {
            let session = state.lock().unwrap();
            let selected = session.selected_branch.as_deref();
            let active_op = session.active_operation;
            let last_receipt = session.last_receipt.clone();
            let interaction =
                interaction_state(&snapshot, selected, active_op, last_receipt.as_ref());
            let base = format!("/s/{token}");
            let markup = templates::op_result_with_stack(
                banner_msg,
                success,
                &session,
                &snapshot,
                &interaction,
                &base,
            );
            Html(markup.into_string()).into_response()
        }
        Ok(Err(e)) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
        Err(e) => Html(error_fragment(&e.to_string()).into_string()).into_response(),
    }
}
