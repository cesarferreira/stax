//! Maud HTML templates for the `st web` workspace.

use crate::application::{
    BranchDetails, BranchDiff, BranchSummary, DiffLineKind, InteractionState, RepositorySnapshot,
    TopologyCell, TopologyNode, topology_layout,
};
use crate::web::session::{ThemePreference, WebSession};
use maud::{DOCTYPE, Markup, html};

fn csrf_input(csrf: &str) -> Markup {
    html! { input type="hidden" name="csrf" value=(csrf) {} }
}

pub fn workspace_page(
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
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
                    // Toolbar
                    (toolbar(session, snapshot, interaction, &base))

                    // Banner area (operation results)
                    div #banner {}

                    // Three-pane workspace
                    div .pane-area {
                        // Stack pane
                        div
                            class={
                                "pane pane-stack"
                                @if !session.show_stack { " pane-hidden" }
                            }
                            #pane-stack
                            {
                            div .pane-header { "Stack" }
                            div .pane-body #stack-pane {
                                (stack_pane_inner(session, snapshot, interaction, &base))
                            }
                        }

                        // Changes pane
                        div
                            class={
                                "pane pane-changes"
                                @if !session.show_changes { " pane-hidden" }
                            }
                            #pane-changes
                            {
                            div .pane-header { "Changes" }
                            div .pane-body #changes-pane
                                style="display:flex;flex-direction:column;flex:1;"
                                hx-get=(format!("{base}/diff"))
                                hx-trigger="load, every 30s"
                                {
                                (changes_pane_placeholder(selected))
                            }
                        }

                        // Inspector pane
                        div
                            class={
                                "pane pane-inspector"
                                @if !session.show_inspector { " pane-hidden" }
                            }
                            #pane-inspector
                            {
                            div .pane-header { "Inspector" }
                            div .pane-body #inspector-pane
                                hx-get=(format!("{base}/details"))
                                hx-trigger="load"
                                {
                                (inspector_placeholder(selected))
                            }
                        }
                    }
                }
            }
        }
    }
}

fn toolbar(
    session: &WebSession,
    _snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    base: &str,
) -> Markup {
    let csrf = &session.csrf_token;

    html! {
        div .toolbar {
            // Project switcher
            form .project-form
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

            // Search
            input #search-input .search-input
                type="text"
                placeholder="/ to search"
                name="query"
                value=(session.search_query)
                autocomplete="off"
                hx-post=(format!("{base}/search"))
                hx-trigger="input changed delay:200ms"
                hx-target="#stack-pane"
                hx-include="[name='csrf']"
                {}
            // Hidden CSRF for search
            input type="hidden" name="csrf" value=(csrf) {}

            div .spacer {}

            // Toolbar actions
            @if interaction.restack.enabled {
                button .btn.mutating-btn
                    hx-post=(format!("{base}/op/restack"))
                    hx-target="#stack-pane"
                    hx-include="[name='csrf']"
                    title="Restack current branch"
                    { "Restack" }
            } @else {
                button .btn disabled title=(interaction.restack.reason.as_deref().unwrap_or("")) { "Restack" }
            }

            @if interaction.submit.enabled {
                button .btn.btn-primary.mutating-btn
                    hx-post=(format!("{base}/op/submit"))
                    hx-target="#banner"
                    hx-include="[name='csrf']"
                    hx-confirm="Submit the current stack as Draft PRs?"
                    title="Submit stack (draft PRs)"
                    { "Submit" }
            } @else {
                button .btn.btn-primary disabled title=(interaction.submit.reason.as_deref().unwrap_or("")) { "Submit" }
            }

            @if interaction.open_pr.enabled {
                a .btn
                    href=(format!("{base}/op/open-pr"))
                    target="_blank"
                    rel="noopener"
                    title="Open PR for selected branch"
                    { "Open PR" }
            } @else {
                button .btn disabled title=(interaction.open_pr.reason.as_deref().unwrap_or("")) { "Open PR" }
            }

            @if interaction.undo.enabled {
                button .btn.mutating-btn
                    hx-post=(format!("{base}/op/undo"))
                    hx-target="#stack-pane"
                    hx-include="[name='csrf']"
                    title="Undo last local operation"
                    { "Undo" }
            } @else {
                button .btn disabled title=(interaction.undo.reason.as_deref().unwrap_or("")) { "Undo" }
            }

            @if interaction.redo.enabled {
                button .btn.mutating-btn
                    hx-post=(format!("{base}/op/redo"))
                    hx-target="#stack-pane"
                    hx-include="[name='csrf']"
                    title="Redo last local operation"
                    { "Redo" }
            } @else {
                button .btn disabled title=(interaction.redo.reason.as_deref().unwrap_or("")) { "Redo" }
            }

            button .btn
                hx-post=(format!("{base}/refresh"))
                hx-target="#stack-pane"
                hx-include="[name='csrf']"
                title="Refresh repository"
                { "↺" }

            select #theme-select .theme-select
                name="theme"
                title="Appearance"
                hx-post=(format!("{base}/theme"))
                hx-trigger="change"
                hx-include="[name='csrf']"
                hx-swap="none"
                onchange="document.documentElement.setAttribute('data-theme', this.value)"
                {
                option value="system" selected[session.theme == ThemePreference::System] { "System" }
                option value="light" selected[session.theme == ThemePreference::Light] { "Light" }
                option value="dark" selected[session.theme == ThemePreference::Dark] { "Dark" }
            }

            button .btn
                onclick="document.getElementById('help-overlay')?.remove();document.body.insertAdjacentHTML('beforeend', document.getElementById('help-template').innerHTML)"
                title="Keyboard shortcuts"
                { "?" }

            // Hidden CSRF repeated for forms that don't include it from search
            input type="hidden" name="csrf" value=(csrf) {}
            template #help-template {
                (help_fragment())
            }
        }
    }
}

pub fn stack_pane_inner(
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    base: &str,
) -> Markup {
    let branches = &snapshot.branches;
    let rows = topology_layout(branches);
    let query = session.search_query.to_lowercase();

    html! {
        @for row in &rows {
            @let branch_opt = branches.iter().find(|b| b.name == row.branch_name);
            @if let Some(branch) = branch_opt {
                @if query.is_empty() || branch.name.to_lowercase().contains(&query) {
                    (branch_row(session, branch, &row.cells, interaction, base))
                }
            }
        }
        @if branches.is_empty() {
            div .text-muted style="padding:16px;font-size:12px;" {
                "No stacked branches found. Run "
                code { "st init" }
                " to initialize stax."
            }
        }
    }
}

fn branch_row(
    session: &WebSession,
    branch: &BranchSummary,
    cells: &[TopologyCell],
    interaction: &InteractionState,
    base: &str,
) -> Markup {
    let selected = session.selected_branch.as_deref() == Some(&branch.name);
    let csrf = &session.csrf_token;

    let mut row_class = String::from("branch-row");
    if selected {
        row_class.push_str(" selected");
    }
    if branch.is_current {
        row_class.push_str(" is-current");
    }

    html! {
        div
            class=(row_class)
            hx-post=(format!("{base}/select"))
            hx-target="#stack-pane"
            hx-swap="innerHTML"
            hx-vals=(format!(r#"{{"branch":"{}","csrf":"{}"}}"#, branch.name.replace('"', "\\\""), csrf))
            hx-trigger="click"
            hx-on--after-request="htmx.trigger('#inspector-pane','load'); htmx.trigger('#changes-pane','load');"
            {
            // Topology grid
            div .topo-grid {
                @for cell in cells {
                    (topo_cell(cell, branch))
                }
            }

            // Branch name
            span .branch-name title=(branch.name) { (branch.name) }

            // Labels
            @if branch.is_trunk {
                span .branch-label { "trunk" }
            }
            @if branch.is_current && !branch.is_trunk {
                span .branch-label { "HEAD" }
            }
            @if branch.needs_restack {
                span .branch-label.needs-restack title="Needs restack" { "⟳" }
            }
            @if let Some(pr_num) = branch.pr_number {
                span .branch-label.pr-open { "#" (pr_num) }
            }

            // Checkout button (only for non-current, non-trunk branches)
            @if !branch.is_current && !branch.is_trunk && interaction.checkout.enabled {
                button .btn.btn-icon.mutating-btn style="font-size:10px;padding:2px 6px;"
                    hx-post=(format!("{base}/op/checkout"))
                    hx-target="#stack-pane"
                    hx-swap="outerHTML"
                    hx-vals=(format!(r#"{{"branch":"{}","csrf":"{}"}}"#, branch.name.replace('"', "\\\""), csrf))
                    hx-trigger="click[!event.defaultPrevented]"
                    onclick="event.stopPropagation()"
                    title=(format!("Check out {}", branch.name))
                    { "co" }
            }
        }
    }
}

fn topo_cell(cell: &TopologyCell, _branch: &BranchSummary) -> Markup {
    html! {
        div .topo-cell {
            @if cell.top && cell.bottom {
                div .topo-connector-v {}
            } @else if cell.top {
                div .topo-connector-v style="bottom:50%" {}
            } @else if cell.bottom {
                div .topo-connector-v style="top:50%" {}
            }
            @if cell.left {
                div .topo-connector-h-left {}
            }
            @if cell.right {
                div .topo-connector-h-right {}
            }
            @if let Some(node) = cell.node {
                @let is_current = matches!(node, TopologyNode::Current);
                @let lane_class = match cell.lane % 3 {
                    1 => "lane-1",
                    2 => "lane-2",
                    _ => "",
                };
                @if is_current {
                    div class=(format!("topo-node current {lane_class}")) {}
                } @else {
                    div class=(format!("topo-node {lane_class}")) {}
                }
            }
        }
    }
}

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

pub fn diff_view(diff: &BranchDiff) -> Markup {
    html! {
        div .diff-view {
            @for stat in &diff.stat {
                div .diff-line.diff-header {
                    (stat.file)
                    " +"
                    (stat.additions)
                    " -"
                    (stat.deletions)
                }
            }
            @for line in &diff.lines {
                @let cls = match line.kind {
                    DiffLineKind::Addition => "diff-line diff-add",
                    DiffLineKind::Deletion => "diff-line diff-del",
                    DiffLineKind::Hunk     => "diff-line diff-hunk",
                    DiffLineKind::Header   => "diff-line diff-header",
                    DiffLineKind::Context  => "diff-line",
                };
                div class=(cls) { (line.content) }
            }
        }
    }
}

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
        div .inspector-section {
            div .inspector-label { "Branch" }
            div .inspector-row {
                span .inspector-key { "Name" }
                span .inspector-value { (branch.name) }
            }
            @if let Some(parent) = &branch.parent {
                div .inspector-row {
                    span .inspector-key { "Parent" }
                    span .inspector-value { (parent) }
                }
            }
            @if let Some(pr) = branch.pr_number {
                div .inspector-row {
                    span .inspector-key { "PR" }
                    span .inspector-value { "#" (pr) }
                }
            }
        }
        div .inspector-section {
            div .inspector-label { "Divergence" }
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
        @if !details.commits.is_empty() {
            div .inspector-section {
                div .inspector-label { "Commits" }
                @for commit in &details.commits {
                    div .inspector-row {
                        span .inspector-value style="font-family:var(--mono);font-size:11px" { (commit) }
                    }
                }
            }
        }
        div .inspector-section {
            div .inspector-label { "Actions" }
            div style="display:flex;flex-wrap:wrap;gap:6px;padding:4px 0" {
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
                    button .btn.mutating-btn
                        hx-post=(format!("{base}/op/delete"))
                        hx-target="#stack-pane"
                        hx-include="[name='csrf']"
                        hx-vals=(format!(r#"{{"branch":"{}"}}"#, branch.name.replace('"', "\\\"")))
                        hx-confirm=(format!("Delete branch {}?", branch.name))
                        { "Delete" }
                }
                @if interaction.move_subtree.enabled && !move_candidates.is_empty() {
                    form style="display:flex;gap:4px;align-items:center"
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
                        form
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
                @if interaction.restack.enabled {
                    button .btn.mutating-btn
                        hx-post=(format!("{base}/op/restack"))
                        hx-target="#stack-pane"
                        hx-include="[name='csrf']"
                        { "Restack" }
                }
            }
        }
    }
}

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
    html! {
        div class=(cls) {
            (message)
        }
    }
}

pub fn op_result_with_stack(
    message: &str,
    success: bool,
    session: &WebSession,
    snapshot: &RepositorySnapshot,
    interaction: &InteractionState,
    base: &str,
) -> Markup {
    html! {
        (operation_banner(message, success))
        (stack_pane_inner(session, snapshot, interaction, base))
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
    html! {
        div .banner.banner-error { (message) }
    }
}

pub fn help_fragment() -> Markup {
    html! {
        div .overlay-backdrop id="help-overlay" onclick="if(event.target===this)this.remove()" {
            div .overlay-card {
                div .overlay-title { "Keyboard shortcuts" }
                table style="width:100%;border-collapse:collapse;font-size:12px" {
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "/" } } td { "Focus search" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "Esc" } } td { "Dismiss overlay / blur search" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "1" } } td { "Toggle stack pane" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "2" } } td { "Toggle changes pane" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "3" } } td { "Toggle inspector pane" } }
                    tr { td style="padding:3px 8px 3px 0;color:var(--text-muted)" { code { "?" } } td { "Show this help" } }
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
