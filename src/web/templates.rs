//! Maud HTML templates for the `st web` workspace.

use crate::application::{
    BranchDetails, BranchDiff, BranchSummary, DiffLine, DiffLineKind, InteractionState,
    RepositorySnapshot, TopologyCell, TopologyNode, topology_layout,
};
use crate::web::session::{ThemePreference, WebSession};
use maud::{DOCTYPE, Markup, html};
use std::collections::HashMap;

/// Ahead/behind counts keyed by branch name (non-trunk branches only).
pub type StackRowMeta = HashMap<String, (usize, usize)>;

fn csrf_input(csrf: &str) -> Markup {
    html! { input type="hidden" name="csrf" value=(csrf) {} }
}

// ── Diff gutter helpers ───────────────────────────────────────────────────────

struct GutteredLine<'a> {
    old_num: Option<usize>,
    new_num: Option<usize>,
    line: &'a DiffLine,
    anchor_id: Option<String>,
}

/// Parse "@@ -old_start[,count] +new_start[,count] @@" and return (old, new).
fn parse_hunk_header(s: &str) -> Option<(usize, usize)> {
    // Trim leading '@' and whitespace
    let s = s.trim_start_matches('@').trim();
    let mut parts = s.splitn(4, ' ');
    let old_part = parts.next()?.trim_start_matches('-');
    let new_part = parts.next()?.trim_start_matches('+');
    let old_start: usize = old_part.split(',').next()?.parse().ok()?;
    let new_start: usize = new_part.split(',').next()?.parse().ok()?;
    Some((old_start, new_start))
}

fn add_gutter_numbers(lines: &[DiffLine]) -> Vec<GutteredLine<'_>> {
    let mut result = Vec::with_capacity(lines.len());
    let mut old_line: usize = 0;
    let mut new_line: usize = 0;
    let mut file_ordinal: usize = 0;

    for line in lines {
        // Collision-free ordinal anchor: assigned only to `diff --git` boundary
        // lines. Path content is never parsed, so quoted paths, renames, and
        // Unicode all work without collisions.
        let anchor_id = if is_diff_git_line(&line.content) {
            let id = file_ordinal.to_string();
            file_ordinal += 1;
            Some(id)
        } else {
            None
        };
        let (old_num, new_num) = match line.kind {
            DiffLineKind::Hunk => {
                if let Some((old_start, new_start)) = parse_hunk_header(&line.content) {
                    old_line = old_start.saturating_sub(1);
                    new_line = new_start.saturating_sub(1);
                }
                (None, None)
            }
            DiffLineKind::Header => (None, None),
            DiffLineKind::Addition => {
                new_line += 1;
                (None, Some(new_line))
            }
            DiffLineKind::Deletion => {
                old_line += 1;
                (Some(old_line), None)
            }
            DiffLineKind::Context => {
                old_line += 1;
                new_line += 1;
                (Some(old_line), Some(new_line))
            }
        };
        result.push(GutteredLine {
            old_num,
            new_num,
            line,
            anchor_id,
        });
    }
    result
}

fn render_guttered_line(g: &GutteredLine<'_>) -> Markup {
    let cls = match g.line.kind {
        DiffLineKind::Addition => "diff-line diff-add",
        DiffLineKind::Deletion => "diff-line diff-del",
        DiffLineKind::Hunk => "diff-line diff-hunk",
        DiffLineKind::Header => "diff-line diff-header",
        DiffLineKind::Context => "diff-line",
    };
    let old_str = g.old_num.map(|n| n.to_string()).unwrap_or_default();
    let new_str = g.new_num.map(|n| n.to_string()).unwrap_or_default();

    if let Some(id) = &g.anchor_id {
        html! {
            div class=(cls) id=(format!("diff-file-{id}")) {
                span .diff-gutter-old { (old_str) }
                span .diff-gutter-new { (new_str) }
                span .diff-text { (g.line.content) }
            }
        }
    } else {
        html! {
            div class=(cls) {
                span .diff-gutter-old { (old_str) }
                span .diff-gutter-new { (new_str) }
                span .diff-text { (g.line.content) }
            }
        }
    }
}

// ── Topology strip renderer ───────────────────────────────────────────────────

fn render_topo_cells(cells: &[TopologyCell], is_current_branch: bool) -> Markup {
    html! {
        @for cell in cells {
            div .topo-cell {
                // Vertical top rail segment
                @if cell.top {
                    div class=(format!("tc-rail tc-top lane-{}", cell.lane % 3)) {}
                }
                // Vertical bottom rail segment
                @if cell.bottom {
                    div class=(format!("tc-rail tc-bottom lane-{}", cell.lane % 3)) {}
                }
                // Horizontal connectors
                @if cell.left  { div .tc-h.tc-left  {} }
                @if cell.right { div .tc-h.tc-right {} }
                // Branch node marker
                @if let Some(node) = cell.node {
                    @let is_current = matches!(node, TopologyNode::Current) || is_current_branch;
                    div class=(if is_current { "tc-node current".to_string() } else { format!("tc-node lane-{}", cell.lane % 3) }) {}
                }
            }
        }
    }
}

// ── Page shell ────────────────────────────────────────────────────────────────

pub fn workspace_page(
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    row_meta: &StackRowMeta,
) -> Markup {
    let base = format!("/s/{}", session.session_token);
    let selected = session.selected_branch.as_deref();
    let repo_name = session
        .repository_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repository");

    let theme = session.theme.as_str();

    html! {
        (DOCTYPE)
        html lang="en" data-theme=(theme) {
            head {
                meta charset="utf-8" {}
                meta name="viewport" content="width=device-width, initial-scale=1" {}
                meta name="color-scheme" content="light dark" {}
                title { (repo_name) " — stax web" }
                link rel="stylesheet" href="/assets/app.css" {}
                script src="/assets/htmx.min.js" {}
                script src="/assets/app.js" {}
            }
            body {
                div .workspace {
                    // Banner area (OOB landing zone for operation results)
                    div #banner {}

                    // Top bar
                    (topbar(session, snapshot, interaction, &base))

                    // Three-column stage
                    div .stage {
                        // Stack pane (left)
                        div
                            class={
                                "pane pane-stack"
                                @if !session.show_stack { " pane-hidden" }
                            }
                            #pane-stack
                            {
                            div .pane-body #stack-pane {
                                (stack_pane_inner(session, snapshot, interaction, &base, row_meta))
                            }
                        }

                        // Review workspace (centre)
                        div
                            class={
                                "pane pane-changes"
                                @if !session.show_changes { " pane-hidden" }
                            }
                            #pane-changes
                            {
                            // Review header — pre-populated on initial load, then
                            // kept current via OOB updates from diff responses.
                            div #review-header .review-header {
                                @if let Some(branch) = selected {
                                    div .review-tabs {
                                        span .review-tab.active { "Changes" }
                                    }
                                    span .review-branch-name title=(branch) { (branch) }
                                    @if let Some((ahead, _)) = row_meta.get(branch) {
                                        @if *ahead > 0 {
                                            span .review-stat {
                                                (*ahead) " commit" @if *ahead != 1 { "s" }
                                            }
                                        }
                                    }
                                } @else {
                                    span .text-muted { "Select a branch to review" }
                                }
                            }
                            // Changes body — HTMX target
                            div .pane-body #changes-pane
                                style="display:flex;flex-direction:column;flex:1;min-height:0;padding:0;"
                                hx-get=(format!("{base}/diff"))
                                hx-trigger="load, every 30s"
                                {
                                (changes_pane_placeholder(selected))
                            }
                        }

                        // Branch inspector (right)
                        div
                            class={
                                "pane pane-inspector"
                                @if !session.show_inspector { " pane-hidden" }
                            }
                            #pane-inspector
                            {
                            div .pane-body #inspector-pane
                                hx-get=(format!("{base}/details"))
                                hx-trigger="load"
                                {
                                (inspector_placeholder(selected))
                            }
                        }
                    }

                    // Status line
                    (status_bar(session, snapshot, row_meta))
                }
            }
        }
    }
}

// ── Top bar ───────────────────────────────────────────────────────────────────

fn topbar(
    session: &WebSession,
    _snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    base: &str,
) -> Markup {
    let csrf = &session.csrf_token;

    html! {
        header .topbar {
            // Logo
            div .topbar-logo {
                span .logo-mark { "S" }
                span .logo-name { "stax" }
            }

            div .topbar-sep {}

            // Project switcher
            form .topbar-project
                hx-post=(format!("{base}/project"))
                hx-target="body"
                hx-swap="outerHTML"
                {
                (csrf_input(csrf))
                select .project-select name="path"
                    onchange="this.form.requestSubmit()"
                    title="Switch repository"
                    {
                    @for project in &session.recent_projects {
                        @let name = project.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                        @let path_str = project.display().to_string();
                        @let is_current = *project == session.repository_root;
                        option value=(path_str) selected[is_current] {
                            (name)
                            @if is_current { " (current)" }
                        }
                    }
                }
                input .project-add type="text" name="add_path" placeholder="Add path…"
                    title="Type an absolute path and press Enter to open another repository"
                    onkeydown="if(event.key==='Enter'){event.preventDefault();const v=this.value.trim();if(v){this.form.path.value=v;this.form.requestSubmit();}}"
                    {}
            }

            div .topbar-sep {}

            // Branch search
            input #search-input .topbar-search
                type="text"
                placeholder="/ Search branches, files, commits"
                name="query"
                value=(session.search_query)
                autocomplete="off"
                hx-post=(format!("{base}/search"))
                hx-trigger="input changed delay:200ms"
                hx-target="#stack-pane"
                hx-include="#workspace-csrf"
                {}

            div .spacer {}

            // Utility buttons
            div .topbar-group {
                button .btn.btn-icon.mutating-btn
                    hx-post=(format!("{base}/refresh"))
                    hx-target="#stack-pane"
                    hx-include="#workspace-csrf"
                    title="Refresh repository"
                    { "↺" }

                @if interaction.undo.enabled {
                    button .btn.btn-icon.mutating-btn
                        hx-post=(format!("{base}/op/undo"))
                        hx-target="#stack-pane"
                        hx-include="#workspace-csrf"
                        title="Undo last local operation"
                        { "↶" }
                } @else {
                    button .btn.btn-icon disabled title=(interaction.undo.reason.as_deref().unwrap_or("Nothing to undo")) { "↶" }
                }

                @if interaction.redo.enabled {
                    button .btn.btn-icon.mutating-btn
                        hx-post=(format!("{base}/op/redo"))
                        hx-target="#stack-pane"
                        hx-include="#workspace-csrf"
                        title="Redo last local operation"
                        { "↷" }
                } @else {
                    button .btn.btn-icon disabled title=(interaction.redo.reason.as_deref().unwrap_or("Nothing to redo")) { "↷" }
                }

                select #theme-select .theme-select
                    name="theme"
                    title="Appearance"
                    hx-post=(format!("{base}/theme"))
                    hx-trigger="change"
                    hx-include="#workspace-csrf"
                    hx-swap="none"
                    onchange="document.documentElement.setAttribute('data-theme', this.value)"
                    {
                    option value="system" selected[session.theme == ThemePreference::System] { "System" }
                    option value="light"  selected[session.theme == ThemePreference::Light]  { "Light" }
                    option value="dark"   selected[session.theme == ThemePreference::Dark]   { "Dark" }
                }

                button .btn.btn-icon
                    onclick="document.getElementById('help-overlay')?.remove();document.body.insertAdjacentHTML('beforeend', document.getElementById('help-template').innerHTML)"
                    title="Keyboard shortcuts"
                    { "?" }
            }

            div .topbar-sep {}

            // Primary actions (also updated via OOB after mutations)
            (topbar_actions_inner(interaction, base))

            input #workspace-csrf type="hidden" name="csrf" value=(csrf) {}
            template #help-template {
                (help_fragment())
            }
        }
    }
}

/// Inner content for the topbar action group, extracted so it can be OOB-updated.
fn topbar_actions_inner(interaction: &InteractionState, base: &str) -> Markup {
    html! {
        div #topbar-actions .topbar-actions {
            @if interaction.restack.enabled {
                button .btn.mutating-btn
                    hx-post=(format!("{base}/op/restack"))
                    hx-target="#stack-pane"
                    hx-include="#workspace-csrf"
                    title="Restack current branch"
                    { "⟳ Restack" }
            } @else {
                button .btn disabled title=(interaction.restack.reason.as_deref().unwrap_or("")) { "⟳ Restack" }
            }

            @if interaction.open_pr.enabled {
                a .btn
                    href=(format!("{base}/op/open-pr"))
                    target="_blank"
                    rel="noopener"
                    title="Open PR for selected branch"
                    { "Open PR ↗" }
            } @else {
                button .btn disabled title=(interaction.open_pr.reason.as_deref().unwrap_or("")) { "Open PR ↗" }
            }

            @if interaction.submit.enabled {
                button .btn.btn-primary.mutating-btn
                    hx-post=(format!("{base}/op/submit"))
                    hx-target="#stack-pane"
                    hx-include="#workspace-csrf"
                    hx-confirm="Submit the current stack as Draft PRs?"
                    title="Submit stack (draft PRs)"
                    { "Submit stack" }
            } @else {
                button .btn.btn-primary disabled title=(interaction.submit.reason.as_deref().unwrap_or("")) { "Submit stack" }
            }
        }
    }
}

// ── Stack pane ────────────────────────────────────────────────────────────────

/// Returns the pixel width for the stack pane based on how many topology lanes
/// are rendered. 240px base, 20px per lane beyond the first, capped at 400px.
pub(crate) fn stack_pane_width_px(lane_count: usize) -> u32 {
    const BASE: u32 = 240;
    const CAP: u32 = 400;
    let extra_lanes = lane_count
        .saturating_sub(1)
        .min(((CAP - BASE) / 20) as usize);
    BASE + extra_lanes as u32 * 20
}

pub fn stack_pane_fragment(
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    base: &str,
    row_meta: &StackRowMeta,
) -> Markup {
    html! {
        (status_bar_oob(session, snapshot, row_meta))
        (topbar_actions_oob(interaction, base))
        (stack_pane_inner(session, snapshot, interaction, base, row_meta))
    }
}

fn topbar_actions_oob(interaction: &InteractionState, base: &str) -> Markup {
    html! {
        div #topbar-actions hx-swap-oob="true" class="topbar-actions" {
            @if interaction.restack.enabled {
                button .btn.mutating-btn
                    hx-post=(format!("{base}/op/restack"))
                    hx-target="#stack-pane"
                    hx-include="#workspace-csrf"
                    title="Restack current branch"
                    { "⟳ Restack" }
            } @else {
                button .btn disabled title=(interaction.restack.reason.as_deref().unwrap_or("")) { "⟳ Restack" }
            }

            @if interaction.open_pr.enabled {
                a .btn
                    href=(format!("{base}/op/open-pr"))
                    target="_blank"
                    rel="noopener"
                    title="Open PR for selected branch"
                    { "Open PR ↗" }
            } @else {
                button .btn disabled title=(interaction.open_pr.reason.as_deref().unwrap_or("")) { "Open PR ↗" }
            }

            @if interaction.submit.enabled {
                button .btn.btn-primary.mutating-btn
                    hx-post=(format!("{base}/op/submit"))
                    hx-target="#stack-pane"
                    hx-include="#workspace-csrf"
                    hx-confirm="Submit the current stack as Draft PRs?"
                    title="Submit stack (draft PRs)"
                    { "Submit stack" }
            } @else {
                button .btn.btn-primary disabled title=(interaction.submit.reason.as_deref().unwrap_or("")) { "Submit stack" }
            }
        }
    }
}

pub fn status_bar(
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    row_meta: &StackRowMeta,
) -> Markup {
    let selected_name = session.selected_branch.as_deref();
    let selected_branch =
        selected_name.and_then(|name| snapshot.branches.iter().find(|b| b.name == name));

    html! {
        footer #status-bar.status-bar {
            span .status-item {
                span .status-label { "HEAD" }
                span .status-value { (snapshot.current_branch) }
            }
            @if let Some(name) = selected_name {
                span .status-sep {}
                span .status-item {
                    span .status-label { "Selected" }
                    span .status-value { (name) }
                }
                @if let Some((ahead, behind)) = row_meta.get(name) {
                    span .status-sep {}
                    span .status-item {
                        span .status-label { "Δ parent" }
                        span .status-value { (ahead) "↑ " (behind) "↓" }
                    }
                }
                @if let Some(branch) = selected_branch {
                    @if branch.needs_restack {
                        span .status-chip.chip-warning { "needs restack" }
                    }
                    @if let Some(pr) = branch.pr_number {
                        span .status-chip.chip-pr { "PR #" (pr) }
                    }
                }
            } @else {
                span .status-sep {}
                span .status-item .status-muted { "No branch selected" }
            }
        }
    }
}

fn status_bar_oob(
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    row_meta: &StackRowMeta,
) -> Markup {
    let selected_name = session.selected_branch.as_deref();
    let selected_branch =
        selected_name.and_then(|name| snapshot.branches.iter().find(|b| b.name == name));

    html! {
        footer #status-bar hx-swap-oob="true" class="status-bar" {
            span .status-item {
                span .status-label { "HEAD" }
                span .status-value { (snapshot.current_branch) }
            }
            @if let Some(name) = selected_name {
                span .status-sep {}
                span .status-item {
                    span .status-label { "Selected" }
                    span .status-value { (name) }
                }
                @if let Some((ahead, behind)) = row_meta.get(name) {
                    span .status-sep {}
                    span .status-item {
                        span .status-label { "Δ parent" }
                        span .status-value { (ahead) "↑ " (behind) "↓" }
                    }
                }
                @if let Some(branch) = selected_branch {
                    @if branch.needs_restack {
                        span .status-chip.chip-warning { "needs restack" }
                    }
                    @if let Some(pr) = branch.pr_number {
                        span .status-chip.chip-pr { "PR #" (pr) }
                    }
                }
            } @else {
                span .status-sep {}
                span .status-item .status-muted { "No branch selected" }
            }
        }
    }
}

pub fn stack_pane_inner(
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    base: &str,
    row_meta: &StackRowMeta,
) -> Markup {
    let branches = &snapshot.branches;
    let rows = topology_layout(branches);
    let query = session.search_query.to_lowercase();

    let visible_rows: Vec<(&BranchSummary, &[TopologyCell])> = rows
        .iter()
        .filter_map(|row| {
            let branch = branches.iter().find(|b| b.name == row.branch_name)?;
            if query.is_empty() || branch.name.to_lowercase().contains(&query) {
                Some((branch, row.cells.as_slice()))
            } else {
                None
            }
        })
        .collect();

    // Summary counts for the stack header
    let branch_count = visible_rows.iter().filter(|(b, _)| !b.is_trunk).count();
    let pr_count = visible_rows
        .iter()
        .filter(|(b, _)| b.pr_number.is_some())
        .count();

    let max_lanes = visible_rows
        .iter()
        .map(|(_, cells)| cells.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let rail_width = stack_pane_width_px(max_lanes);

    let csrf = &session.csrf_token;
    let create_parent = session
        .selected_branch
        .as_deref()
        .unwrap_or(snapshot.trunk.as_str());

    html! {
        div .stack-rail data-lane-count=(max_lanes) style=(format!("--stack-rail-w:{rail_width}px")) {
            // Header
            div .stack-header {
                div .stack-header-labels {
                    span .stack-header-label { "STACK" }
                    span .stack-trunk-badge { (snapshot.trunk) }
                }
                div .stack-title-row {
                    h2 .stack-title { "Current stack" }
                    @if branch_count > 0 {
                        span .stack-meta {
                            (branch_count)
                            @if branch_count == 1 { " branch" } @else { " branches" }
                            @if pr_count > 0 {
                                " · " (pr_count) " PR" @if pr_count != 1 { "s" }
                            }
                        }
                    }
                }
            }

            // Branch cards
            div .branch-cards {
                @for (branch, cells) in &visible_rows {
                    (branch_card(session, branch, cells, interaction, base, row_meta))
                }
                @if visible_rows.is_empty() {
                    div .stack-empty {
                        @if branches.is_empty() {
                            "No stacked branches found. Run "
                            code { "st init" }
                            " to initialize stax."
                        } @else {
                            "No branches match your search."
                        }
                    }
                }
            }

            // Quick actions
            div .quick-actions {
                div .quick-actions-label { "QUICK ACTIONS" }

                @if interaction.create.enabled {
                    button .quick-action.qa-new-branch
                        onclick=(format!(
                            "document.body.insertAdjacentHTML('beforeend', `{}`)",
                            create_overlay_escaped(create_parent, base, csrf)
                        ))
                        title=(format!("New branch stacked on {create_parent}"))
                        {
                        span .qa-icon { "□" }
                        span .qa-label { "New branch" }
                        span .qa-key { "N" }
                    }
                } @else {
                    button .quick-action disabled {
                        span .qa-icon { "□" }
                        span .qa-label { "New branch" }
                        span .qa-key { "N" }
                    }
                }

                @if interaction.restack.enabled {
                    button .quick-action.qa-restack.mutating-btn
                        hx-post=(format!("{base}/op/restack"))
                        hx-target="#stack-pane"
                        hx-include="#workspace-csrf"
                        title="Restack current branch onto its parent"
                        {
                        span .qa-icon { "⟳" }
                        span .qa-label { "Restack stack" }
                        span .qa-key { "R" }
                    }
                } @else {
                    button .quick-action disabled title=(interaction.restack.reason.as_deref().unwrap_or("")) {
                        span .qa-icon { "⟳" }
                        span .qa-label { "Restack stack" }
                        span .qa-key { "R" }
                    }
                }

                @if interaction.submit.enabled {
                    button .quick-action.qa-submit.mutating-btn
                        hx-post=(format!("{base}/op/submit"))
                        hx-target="#stack-pane"
                        hx-include="#workspace-csrf"
                        hx-confirm="Submit the current stack as Draft PRs?"
                        {
                        span .qa-icon { "↑" }
                        span .qa-label { "Submit stack" }
                        span .qa-key { "S" }
                    }
                } @else {
                    button .quick-action disabled {
                        span .qa-icon { "↑" }
                        span .qa-label { "Submit stack" }
                        span .qa-key { "S" }
                    }
                }

                @if interaction.undo.enabled {
                    button .quick-action.qa-undo.mutating-btn
                        hx-post=(format!("{base}/op/undo"))
                        hx-target="#stack-pane"
                        hx-include="#workspace-csrf"
                        {
                        span .qa-icon { "↩" }
                        span .qa-label { "Undo" }
                        span .qa-key { "⌘Z" }
                    }
                } @else {
                    button .quick-action disabled {
                        span .qa-icon { "↩" }
                        span .qa-label { "Undo" }
                        span .qa-key { "⌘Z" }
                    }
                }
            }
        }
    }
}

fn branch_card(
    session: &WebSession,
    branch: &BranchSummary,
    cells: &[TopologyCell],
    interaction: &InteractionState,
    base: &str,
    row_meta: &StackRowMeta,
) -> Markup {
    let selected = session.selected_branch.as_deref() == Some(&branch.name);
    let csrf = &session.csrf_token;

    let mut card_class = String::from("branch-card");
    if selected {
        card_class.push_str(" selected");
    }
    if branch.is_current {
        card_class.push_str(" is-current");
    }
    if branch.is_trunk {
        card_class.push_str(" is-trunk");
    }

    html! {
        // Outer wrapper: visual card surface only — not interactive itself.
        // The inner .card-select div carries role=button so that the checkout
        // button is a sibling (not a descendant) of the selection surface,
        // avoiding the interactive-descendant-inside-role-button a11y problem.
        div class=(card_class) {
            // ── Selection surface ───────────────────────────────────────────
            div
                .card-select
                role="button"
                tabindex="0"
                aria-pressed=(if selected { "true" } else { "false" })
                hx-post=(format!("{base}/select"))
                hx-target="#stack-pane"
                hx-swap="innerHTML"
                hx-vals=(format!(r#"{{"branch":"{}","csrf":"{}"}}"#, branch.name.replace('"', "\\\""), csrf))
                hx-trigger="click"
                onkeydown="if((event.key==='Enter'||event.key===' ')&&event.target===this){event.preventDefault();this.click();}"
                {
                // Topology strip
                div .card-topo {
                    (render_topo_cells(cells, branch.is_current))
                }

                // Card content
                div .card-inner {
                    div .card-top {
                        span .card-name title=(branch.name) { (branch.name) }
                        div .card-chips {
                            @if branch.is_trunk {
                                span .meta-chip.chip-trunk { "trunk" }
                            }
                            @if branch.is_current && !branch.is_trunk {
                                span .meta-chip.chip-head { "HEAD" }
                            }
                            @if branch.needs_restack {
                                span .meta-chip.chip-warning { "restack" }
                            }
                            @if let Some(pr_num) = branch.pr_number {
                                span .meta-chip.chip-pr { "#" (pr_num) }
                            }
                        }
                    }

                    @let has_bottom_row = branch.ci_state.is_some()
                        || row_meta.get(&branch.name).map(|(a,b)| *a > 0 || *b > 0).unwrap_or(false);
                    @if has_bottom_row {
                        div .card-bottom {
                            @if let Some(ci) = &branch.ci_state {
                                @let ci_cls = if ci.to_lowercase().contains("pass") || ci.to_lowercase().contains("success") {
                                    "card-ci ci-pass"
                                } else if ci.to_lowercase().contains("fail") || ci.to_lowercase().contains("error") {
                                    "card-ci ci-fail"
                                } else {
                                    "card-ci ci-pending"
                                };
                                span class=(ci_cls) { "● " (ci) }
                            }
                            @if let Some((ahead, behind)) = row_meta.get(&branch.name) {
                                @if *ahead > 0 || *behind > 0 {
                                    span .card-diverge { (ahead) "↑ " (behind) "↓" }
                                }
                            }
                        }
                    }
                }
            }

            // ── Checkout button — sibling of the selection surface ──────────
            // Placing it outside role=button avoids nested interactive content.
            @if !branch.is_current && !branch.is_trunk && interaction.checkout.enabled {
                button .btn.btn-icon.btn-checkout.mutating-btn
                    hx-post=(format!("{base}/op/checkout"))
                    hx-target="#stack-pane"
                    hx-swap="innerHTML"
                    hx-vals=(format!(r#"{{"branch":"{}","csrf":"{}"}}"#, branch.name.replace('"', "\\\""), csrf))
                    hx-trigger="click"
                    title=(format!("Check out {}", branch.name))
                    { "co" }
            }
        }
    }
}

// ── Changes pane ──────────────────────────────────────────────────────────────

pub fn changes_pane_placeholder(selected: Option<&str>) -> Markup {
    match selected {
        None => html! {
            div .text-muted style="padding:16px;font-size:12px;" {
                "Select a branch to view its changes."
            }
        },
        Some(branch) => html! {
            div style="padding:8px;font-size:12px;color:var(--text-muted)" {
                "Loading diff for " strong { (branch) } "…"
            }
            div .skeleton style="margin:8px 12px;width:60%" {}
            div .skeleton style="margin:8px 12px;width:80%" {}
            div .skeleton style="margin:8px 12px;width:40%" {}
        },
    }
}

/// OOB review header update emitted by diff responses.
/// Renders an active "Changes" tab and truthful commit + file stats.
fn review_header_oob(
    branch_name: &str,
    file_count: usize,
    total_add: usize,
    total_del: usize,
    commit_count: usize,
) -> Markup {
    html! {
        div #review-header hx-swap-oob="true" class="review-header" {
            div .review-tabs {
                span .review-tab.active { "Changes" }
            }
            span .review-branch-name title=(branch_name) { (branch_name) }
            @if commit_count > 0 {
                span .review-stat {
                    (commit_count) " commit" @if commit_count != 1 { "s" }
                }
            }
            @if file_count > 0 {
                span .review-stat {
                    (file_count) " file" @if file_count != 1 { "s" }
                }
                span .stat-add { "+" (total_add) }
                span .stat-del { "−" (total_del) }
            }
        }
    }
}

pub fn diff_view(diff: &BranchDiff, branch_name: &str, commit_count: usize) -> Markup {
    let file_count = diff.stat.len();
    let total_add: usize = diff.stat.iter().map(|s| s.additions).sum();
    let total_del: usize = diff.stat.iter().map(|s| s.deletions).sum();

    if diff.stat.is_empty() && diff.lines.is_empty() {
        return html! {
            (review_header_oob(branch_name, 0, 0, 0, commit_count))
            (diff_empty())
        };
    }

    let guttered = add_gutter_numbers(&diff.lines);

    html! {
        // OOB update for the review header
        (review_header_oob(branch_name, file_count, total_add, total_del, commit_count))

        // Side-by-side: file navigator + diff pane
        div .changes-panel {
            // File navigator (narrow left)
            div .file-nav {
                div .file-nav-header {
                    span { "Changed files" }
                    span .file-count { (file_count) }
                }
                // Ordinal data-diff-file matches the ordinal anchor on each
                // diff --git boundary line — works for any path format.
                @for (i, stat) in diff.stat.iter().enumerate() {
                    button type="button" class="file-row"
                        data-diff-file=(i)
                        data-file-name=(stat.file)
                        title=(stat.file)
                        {
                        span .file-name { (shorten_path(&stat.file, 28)) }
                        span .file-stats {
                            span .stat-add { "+" (stat.additions) }
                            span .stat-del { "−" (stat.deletions) }
                        }
                    }
                }
            }

            // Diff pane (wide right)
            div .diff-pane role="region" tabindex="0" aria-label="Unified diff" {
                div #diff-file-header .diff-file-header {
                    span #diff-file-path .diff-file-path { "" }
                }
                div .diff-content {
                    @for gline in &guttered {
                        (render_guttered_line(gline))
                    }
                }
            }
        }
    }
}

pub fn diff_empty() -> Markup {
    html! {
        div .changes-empty {
            div .changes-empty-title { "No changes vs parent" }
            div .changes-empty-body {
                "This branch matches its parent — there is nothing to review in the Changes panel."
            }
        }
    }
}

/// Shortens a file path for display in narrow columns.
/// Returns the path unchanged when it fits within `max_chars` Unicode scalar values.
/// Longer paths are trimmed to a tail of `max_chars - 1` chars prefixed with "…",
/// so the overall visible length is always ≤ `max_chars`.
fn shorten_path(path: &str, max_chars: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_chars {
        return path.to_owned();
    }
    let skip = char_count - (max_chars - 1);
    let tail: String = path.chars().skip(skip).collect();
    format!("…{tail}")
}

/// Returns true when `content` is a `diff --git` boundary line.
/// Does NOT parse the path — handles quoted paths, renames, and Unicode.
fn is_diff_git_line(content: &str) -> bool {
    content.starts_with("diff --git ")
}

// ── Inspector ─────────────────────────────────────────────────────────────────

pub fn inspector_placeholder(selected: Option<&str>) -> Markup {
    match selected {
        None => html! {
            div .text-muted style="padding:16px;font-size:12px;" {
                "Select a branch to view its details."
            }
        },
        Some(_) => html! {
            div .skeleton style="margin:16px 12px;width:60%" {}
            div .skeleton style="margin:8px 12px;width:80%" {}
            div .skeleton style="margin:8px 12px;width:40%" {}
        },
    }
}

pub fn inspector_details(
    branch: &BranchSummary,
    details: &BranchDetails,
    interaction: &InteractionState,
    base: &str,
    csrf: &str,
    move_candidates: &[String],
    reorder_order: Option<&[String]>,
) -> Markup {
    html! {
        div .inspector-pane-inner {
            // ── Branch identity ─────────────────────────────────────────────
            div .inspector-section {
                div .inspector-label { "BRANCH" }
                div .inspector-branch-name { (branch.name) }
                div .inspector-badges {
                    @if branch.is_current {
                        span .meta-chip.chip-head { "HEAD" }
                    }
                    @if branch.needs_restack {
                        span .meta-chip.chip-warning { "needs restack" }
                    }
                }
            }

            // ── Identity / parent ────────────────────────────────────────────
            div .inspector-section {
                @if let Some(parent) = &branch.parent {
                    div .inspector-row {
                        span .inspector-key { "Parent" }
                        span .inspector-value { (parent) }
                    }
                }
                div .inspector-row {
                    span .inspector-key { "Ahead / behind" }
                    span .inspector-value { (details.ahead) " / " (details.behind) }
                }
                div .inspector-row {
                    span .inspector-key { "Unpushed / unpulled" }
                    span .inspector-value { (details.unpushed) " / " (details.unpulled) }
                }
                @if details.has_remote {
                    div .inspector-row {
                        span .inspector-key { "Remote" }
                        span .inspector-value { "tracked" }
                    }
                }
            }

            // ── Pull request ─────────────────────────────────────────────────
            @if let Some(pr_num) = branch.pr_number {
                div .inspector-section {
                    div .inspector-label { "PULL REQUEST" }
                    div .inspector-row {
                        span .inspector-key { "Number" }
                        span .inspector-value { "#" (pr_num) }
                    }
                    @if let Some(ci) = &branch.ci_state {
                        div .inspector-row {
                            span .inspector-key { "CI" }
                            @let ci_cls = if ci.to_lowercase().contains("pass") || ci.to_lowercase().contains("success") {
                                "inspector-value text-success"
                            } else if ci.to_lowercase().contains("fail") {
                                "inspector-value text-danger"
                            } else {
                                "inspector-value text-warning"
                            };
                            span class=(ci_cls) { "● " (ci) }
                        }
                    }
                }
            }

            // ── Commits ──────────────────────────────────────────────────────
            @if !details.commits.is_empty() {
                div .inspector-section {
                    div .inspector-label { "COMMITS" }
                    @for commit in &details.commits {
                        @let (sha, msg) = split_commit(commit);
                        div .inspector-commit {
                            span .commit-sha { (sha) }
                            span .commit-msg title=(commit) { (msg) }
                        }
                    }
                }
            }

            // ── Branch operations ────────────────────────────────────────────
            div .inspector-section {
                div .inspector-label { "Actions" }
                div .inspector-actions {
                    @if interaction.create.enabled {
                        button .btn
                            onclick=(format!(
                                "document.body.insertAdjacentHTML('beforeend', `{}`)",
                                create_overlay_escaped(&branch.name, base, csrf)
                            ))
                            { "Create" }
                    }
                    @if interaction.rename.enabled {
                        button .btn
                            onclick=(format!(
                                "document.body.insertAdjacentHTML('beforeend', `{}`)",
                                rename_overlay_escaped(&branch.name, base, csrf)
                            ))
                            { "Rename" }
                    }
                    @if interaction.delete.enabled {
                        button .btn.btn-danger.mutating-btn
                            hx-post=(format!("{base}/op/delete"))
                            hx-target="#stack-pane"
                            hx-include="#workspace-csrf"
                            hx-vals=(format!(r#"{{"branch":"{}"}}"#, branch.name.replace('"', "\\\"")))
                            hx-confirm=(format!("Delete branch {}?", branch.name))
                            { "Delete" }
                    }
                    @if interaction.move_subtree.enabled && !move_candidates.is_empty() {
                        form .move-form
                            hx-post=(format!("{base}/op/move"))
                            hx-target="#stack-pane"
                            hx-confirm="Move this subtree to the selected parent?"
                        {
                            (csrf_input(csrf))
                            input type="hidden" name="source" value=(branch.name) {}
                            select name="new_parent" style="font-size:11px;max-width:120px" {
                                @for candidate in move_candidates {
                                    option value=(candidate) { (candidate) }
                                }
                            }
                            button .btn.mutating-btn type="submit" { "Move" }
                        }
                    }
                    @if interaction.reorder.enabled {
                        @if let Some(order) = reorder_order {
                            form .reorder-form
                                hx-post=(format!("{base}/op/reorder"))
                                hx-target="#stack-pane"
                                hx-confirm="Apply the current linear stack order (no changes) — use CLI for custom reorder, or reverse below."
                            {
                                (csrf_input(csrf))
                                input type="hidden" name="original_order" value=(order.join(",")) {}
                                input type="hidden" name="proposed_order" value=(order.iter().rev().cloned().collect::<Vec<_>>().join(",")) {}
                                button .btn.mutating-btn type="submit" title="Reverse the linear stack order" { "Reorder ↕" }
                            }
                        }
                    }
                }
            }

            // ── Spacer pushes the CTA to the bottom ─────────────────────────
            div .inspector-spacer {}

            // ── Bottom CTA ───────────────────────────────────────────────────
            div .inspector-cta {
                // Dominant: Submit stack
                @if interaction.submit.enabled {
                    button .btn.btn-primary.btn-full.mutating-btn
                        hx-post=(format!("{base}/op/submit"))
                        hx-target="#stack-pane"
                        hx-include="#workspace-csrf"
                        hx-confirm="Submit the current stack as Draft PRs?"
                        title="Submit stack (draft PRs)"
                        { "Submit stack" }
                } @else {
                    button .btn.btn-primary.btn-full disabled title=(interaction.submit.reason.as_deref().unwrap_or("")) { "Submit stack" }
                }

                // Secondary: Restack + Open PR
                div .inspector-cta-secondary {
                    @if interaction.restack.enabled {
                        button .btn.mutating-btn style="flex:1"
                            hx-post=(format!("{base}/op/restack"))
                            hx-target="#stack-pane"
                            hx-include="#workspace-csrf"
                            { "⟳ Restack" }
                    } @else {
                        button .btn style="flex:1" disabled title=(interaction.restack.reason.as_deref().unwrap_or("")) { "⟳ Restack" }
                    }

                    @if interaction.open_pr.enabled {
                        a .btn style="flex:1;justify-content:center"
                            href=(format!("{base}/op/open-pr"))
                            target="_blank"
                            rel="noopener"
                            title="Open PR for selected branch"
                            { "Open PR ↗" }
                    } @else {
                        button .btn style="flex:1" disabled title=(interaction.open_pr.reason.as_deref().unwrap_or("")) { "Open PR ↗" }
                    }
                }
            }
        }
    }
}

/// Split a commit string "sha message" into (sha_prefix, message).
fn split_commit(commit: &str) -> (&str, &str) {
    let trimmed = commit.trim();
    // Commits may be "sha message" or just message lines
    let (sha, msg) = if trimmed.len() >= 7 && trimmed[..7].chars().all(|c| c.is_ascii_hexdigit()) {
        let space = trimmed.find(' ').unwrap_or(trimmed.len());
        (&trimmed[..space.min(7)], trimmed[space..].trim())
    } else {
        ("", trimmed)
    };
    (sha, if msg.is_empty() { trimmed } else { msg })
}

// ── Overlays ──────────────────────────────────────────────────────────────────

fn create_overlay_escaped(parent: &str, base: &str, csrf: &str) -> String {
    create_overlay(parent, base, csrf)
        .into_string()
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${")
}

fn rename_overlay_escaped(name: &str, base: &str, csrf: &str) -> String {
    rename_overlay(name, base, csrf)
        .into_string()
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${")
}

pub fn operation_banner(message: &str, success: bool) -> Markup {
    let cls = if success {
        "banner banner-success"
    } else {
        "banner banner-error"
    };
    html! { div id="banner" hx-swap-oob="true" class=(cls) { (message) } }
}

pub fn op_result_with_stack(
    message: &str,
    success: bool,
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    base: &str,
    row_meta: &StackRowMeta,
) -> Markup {
    html! {
        (operation_banner(message, success))
        (stack_pane_fragment(session, snapshot, interaction, base, row_meta))
    }
}

pub fn confirm_overlay(
    title: &str,
    body: &str,
    confirm_hx_post: &str,
    csrf: &str,
    extra_fields: &[(&str, &str)],
) -> Markup {
    html! {
        div .overlay-backdrop id="confirm-overlay" onclick="if(event.target===this)this.remove()" {
            div .overlay-card {
                div .overlay-title { (title) }
                p style="font-size:12px;color:var(--text-muted)" { (body) }
                div .overlay-actions {
                    button .btn onclick="document.getElementById('confirm-overlay').remove()" { "Cancel" }
                    form
                        hx-post=(confirm_hx_post)
                        hx-target="#stack-pane"
                        hx-on--after-request="document.getElementById('confirm-overlay')?.remove()"
                    {
                        (csrf_input(csrf))
                        @for (name, value) in extra_fields {
                            input type="hidden" name=(name) value=(value) {}
                        }
                        button .btn.btn-primary.mutating-btn type="submit" { "Confirm" }
                    }
                }
            }
        }
    }
}

pub fn rename_overlay(current_name: &str, base: &str, csrf: &str) -> Markup {
    html! {
        div .overlay-backdrop id="rename-overlay" onclick="if(event.target===this)this.remove()" {
            div .overlay-card {
                div .overlay-title { "Rename Branch" }
                form
                    hx-post=(format!("{base}/op/rename"))
                    hx-target="#stack-pane"
                    hx-on--after-request="document.getElementById('rename-overlay')?.remove()"
                {
                    (csrf_input(csrf))
                    input type="hidden" name="branch" value=(current_name) {}
                    div style="margin-bottom:12px" {
                        label style="font-size:12px;display:block;margin-bottom:4px" { "New name" }
                        input type="text" name="new_name" value=(current_name) autofocus {}
                    }
                    div .overlay-actions {
                        button .btn type="button" onclick="document.getElementById('rename-overlay').remove()" { "Cancel" }
                        button .btn.btn-primary.mutating-btn type="submit" { "Rename" }
                    }
                }
            }
        }
    }
}

pub fn create_overlay(parent_name: &str, base: &str, csrf: &str) -> Markup {
    html! {
        div .overlay-backdrop id="create-overlay" onclick="if(event.target===this)this.remove()" {
            div .overlay-card {
                div .overlay-title { "Create Branch" }
                form
                    hx-post=(format!("{base}/op/create"))
                    hx-target="#stack-pane"
                    hx-on--after-request="document.getElementById('create-overlay')?.remove()"
                {
                    (csrf_input(csrf))
                    input type="hidden" name="parent" value=(parent_name) {}
                    div style="margin-bottom:12px" {
                        label style="font-size:12px;display:block;margin-bottom:4px" { "Branch name" }
                        input type="text" name="name" autofocus {}
                    }
                    div .overlay-actions {
                        button .btn type="button" onclick="document.getElementById('create-overlay')?.remove()" { "Cancel" }
                        button .btn.btn-primary.mutating-btn type="submit" { "Create" }
                    }
                }
            }
        }
    }
}

pub fn error_fragment(message: &str) -> Markup {
    html! { div .banner.banner-error { (message) } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{DiffLine, DiffLineKind};

    #[test]
    fn parse_hunk_header_standard() {
        assert_eq!(parse_hunk_header("@@ -1,4 +1,6 @@ fn foo()"), Some((1, 1)));
        assert_eq!(parse_hunk_header("@@ -42,3 +45,7 @@"), Some((42, 45)));
    }

    #[test]
    fn parse_hunk_header_single_line_no_count() {
        assert_eq!(parse_hunk_header("@@ -1 +1 @@"), Some((1, 1)));
    }

    #[test]
    fn parse_hunk_header_invalid_returns_none() {
        assert_eq!(parse_hunk_header("not a hunk"), None);
        assert_eq!(parse_hunk_header(""), None);
    }

    #[test]
    fn add_gutter_numbers_assigns_correct_line_numbers() {
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Hunk,
                content: "@@ -1,2 +1,3 @@ fn foo".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Context,
                content: " context".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Deletion,
                content: "-old".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Addition,
                content: "+new".to_string(),
            },
        ];
        let guttered = add_gutter_numbers(&lines);
        // Hunk header has no line numbers
        assert_eq!(guttered[0].old_num, None);
        assert_eq!(guttered[0].new_num, None);
        // Context line gets both numbers (starts at 1)
        assert_eq!(guttered[1].old_num, Some(1));
        assert_eq!(guttered[1].new_num, Some(1));
        // Deletion gets old number only
        assert_eq!(guttered[2].old_num, Some(2));
        assert_eq!(guttered[2].new_num, None);
        // Addition gets new number only
        assert_eq!(guttered[3].old_num, None);
        assert_eq!(guttered[3].new_num, Some(2));
    }

    #[test]
    fn diff_anchors_are_unique_ordinals() {
        // Three diff --git boundary lines must get ordinals 0, 1, 2 regardless
        // of path content. All other lines must have no anchor.
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Header,
                content: "diff --git a/foo.rs b/foo.rs".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Header,
                content: "index abc..def 100644".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Addition,
                content: "+line".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Header,
                content: "diff --git a/bar.rs b/bar.rs".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Header,
                content: "diff --git a-b.rs b/a-b.rs".to_string(),
            },
        ];
        let guttered = add_gutter_numbers(&lines);
        assert_eq!(
            guttered[0].anchor_id.as_deref(),
            Some("0"),
            "first diff --git should be ordinal 0"
        );
        assert_eq!(
            guttered[1].anchor_id, None,
            "non-diff header should have no anchor"
        );
        assert_eq!(
            guttered[2].anchor_id, None,
            "addition line should have no anchor"
        );
        assert_eq!(
            guttered[3].anchor_id.as_deref(),
            Some("1"),
            "second diff --git should be ordinal 1"
        );
        assert_eq!(
            guttered[4].anchor_id.as_deref(),
            Some("2"),
            "third diff --git should be ordinal 2"
        );
    }

    #[test]
    fn diff_anchor_handles_quoted_path_with_spaces() {
        // Quoted path: no path parsing should occur; ordinal is assigned purely
        // from the diff --git prefix.
        let lines = vec![DiffLine {
            kind: DiffLineKind::Header,
            content: r#"diff --git "a/path with spaces.rs" "b/path with spaces.rs""#.to_string(),
        }];
        let guttered = add_gutter_numbers(&lines);
        assert_eq!(
            guttered[0].anchor_id.as_deref(),
            Some("0"),
            "quoted-path diff --git must get ordinal 0"
        );
    }

    #[test]
    fn diff_anchor_handles_rename() {
        // Rename: a-path != b-path; ordinal still assigned, no path parsed.
        let lines = vec![DiffLine {
            kind: DiffLineKind::Header,
            content: "diff --git a/old_name.rs b/new_name.rs".to_string(),
        }];
        let guttered = add_gutter_numbers(&lines);
        assert_eq!(
            guttered[0].anchor_id.as_deref(),
            Some("0"),
            "rename diff --git must get ordinal 0"
        );
    }

    #[test]
    fn diff_anchor_no_collision_for_slash_vs_dash_paths() {
        // "a/b.rs" and "a-b.rs" would produce the same path-derived anchor ID
        // under a naive path-extraction scheme (the 'a/' prefix stripped yields
        // "b.rs" and "a-b.rs" → both contain '-').  Ordinal assignment must give
        // them distinct IDs regardless of path content.
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Header,
                content: "diff --git a/a/b.rs b/a/b.rs".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Header,
                content: "diff --git a/a-b.rs b/a-b.rs".to_string(),
            },
        ];
        let guttered = add_gutter_numbers(&lines);
        assert_eq!(
            guttered[0].anchor_id.as_deref(),
            Some("0"),
            "a/b.rs boundary should receive ordinal 0"
        );
        assert_eq!(
            guttered[1].anchor_id.as_deref(),
            Some("1"),
            "a-b.rs boundary should receive ordinal 1 (no collision with a/b.rs)"
        );
    }

    #[test]
    fn diff_file_nav_anchors_align_with_diff_boundaries() {
        // File-nav entries are indexed 0..N-1 and must align 1-to-1 with the
        // ordinal IDs on diff --git boundary lines so that clicking a file-row
        // scrolls to the correct diff block.
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Header,
                content: "diff --git a/foo.rs b/foo.rs".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Addition,
                content: "+fn a() {}".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Header,
                content: "diff --git a/bar.rs b/bar.rs".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Addition,
                content: "+fn b() {}".to_string(),
            },
        ];
        let guttered = add_gutter_numbers(&lines);
        // Boundary lines receive sequential ordinals
        assert_eq!(
            guttered[0].anchor_id.as_deref(),
            Some("0"),
            "first boundary → ordinal 0"
        );
        assert_eq!(
            guttered[2].anchor_id.as_deref(),
            Some("1"),
            "second boundary → ordinal 1"
        );
        // Non-boundary lines must carry no anchor
        assert_eq!(
            guttered[1].anchor_id, None,
            "addition line must have no anchor"
        );
        assert_eq!(
            guttered[3].anchor_id, None,
            "addition line must have no anchor"
        );
    }

    #[test]
    fn is_diff_git_line_matches_only_boundary_lines() {
        assert!(is_diff_git_line("diff --git a/foo b/foo"));
        assert!(!is_diff_git_line("+++ b/src/main.rs"));
        assert!(!is_diff_git_line("--- a/src/main.rs"));
        assert!(!is_diff_git_line("index abc..def 100644"));
        assert!(!is_diff_git_line(""));
    }

    #[test]
    fn shorten_path_unchanged_when_short() {
        let path = "src/lib.rs";
        assert_eq!(shorten_path(path, 28), path);
    }

    #[test]
    fn shorten_path_truncates_long_path() {
        let path = "some/very/deeply/nested/directory/structure/my_file.rs";
        let result = shorten_path(path, 28);
        assert!(
            result.starts_with('…'),
            "long path should start with ellipsis: {result}"
        );
        assert_eq!(
            result.chars().count(),
            28,
            "shortened path should be exactly max_chars wide: {result}"
        );
    }

    #[test]
    fn shorten_path_preserves_tail() {
        let path = "some/very/deeply/nested/directory/my_file.rs";
        let result = shorten_path(path, 28);
        assert!(
            result.ends_with("my_file.rs"),
            "shortened path should end with the file tail: {result}"
        );
    }

    #[test]
    fn stack_pane_width_stays_compact_for_linear_stacks() {
        assert_eq!(stack_pane_width_px(0), 240);
        assert_eq!(stack_pane_width_px(1), 240);
    }

    #[test]
    fn stack_pane_width_grows_with_topology_lanes() {
        assert_eq!(stack_pane_width_px(2), 260);
        assert_eq!(stack_pane_width_px(3), 280);
        assert_eq!(stack_pane_width_px(5), 320);
    }

    #[test]
    fn stack_pane_width_is_capped_to_protect_review_space() {
        assert_eq!(stack_pane_width_px(9), 400);
        assert_eq!(stack_pane_width_px(usize::MAX), 400);
    }
}

pub fn help_fragment() -> Markup {
    html! {
        div .overlay-backdrop id="help-overlay" onclick="if(event.target===this)this.remove()" {
            div .overlay-card {
                div .overlay-title { "Keyboard shortcuts" }
                table style="width:100%;border-collapse:collapse;font-size:12px" {
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "/" } }   td { "Focus search" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "Esc" } } td { "Dismiss overlay / blur search" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "1" } }   td { "Toggle stack pane" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "2" } }   td { "Toggle changes pane" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "3" } }   td { "Toggle inspector pane" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "N" } }   td { "New branch (quick action)" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "R" } }   td { "Restack stack (quick action)" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "S" } }   td { "Submit stack (quick action)" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "⌘Z" } }  td { "Undo (quick action)" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "?" } }   td { "Show this help" } }
                }
                p style="font-size:11px;color:var(--text-muted);margin-top:12px" {
                    "Mutations (checkout, create, rename, delete, move, reorder, restack, submit, undo/redo) use the same "
                    code { "stax::application" }
                    " operations as Stax.app. Rebase conflicts must be resolved in the CLI with "
                    code { "st continue" }
                    " / "
                    code { "st abort" }
                    " / "
                    code { "st resolve" }
                    "."
                }
                div .overlay-actions {
                    button .btn onclick="document.getElementById('help-overlay').remove()" { "Close" }
                }
            }
        }
    }
}
