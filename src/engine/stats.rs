//! Pure aggregation layer for `stax stats`.
//!
//! Everything in this module is repo-free: [`StatsFacts`] carries every input
//! gathered by the command layer (`commands/stats.rs`), and [`compute`] turns
//! those facts into a [`RepoStats`] snapshot with no I/O of its own. This keeps
//! the shape/PR-mix/health/attention math unit-testable without a git repo.

use crate::engine::stack::{Stack, TrackedFacts};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A single worktree fact needed for the worktree counters and the "lanes"
/// attention item. Gathered by the command layer from `GitRepo::list_worktrees`
/// plus a bounded dirty-check pass.
#[derive(Debug, Clone)]
pub struct WorktreeFact {
    pub name: String,
    pub is_main: bool,
    pub is_prunable: bool,
    /// `None` when the dirty check was skipped (too many worktrees to check cheaply).
    pub is_dirty: Option<bool>,
}

/// Local branch-hygiene counts, computed by the command layer using the same
/// merged/upstream-gone/stale classification `stax sweep` uses (excluding
/// trunk and the current branch).
#[derive(Debug, Clone, Default)]
pub struct HygieneFacts {
    pub merged_but_local: usize,
    pub upstream_gone: usize,
    pub stale: usize,
    pub stale_days: u64,
}

/// CI roll-up counts across the scoped branches, gathered only when `--ci` is
/// passed.
#[derive(Debug, Clone, Default)]
pub struct CiFacts {
    pub failing: usize,
    pub pending: usize,
    pub passing: usize,
}

/// All inputs [`compute`] needs. Gathered once by `commands/stats.rs::run`.
#[derive(Debug, Clone)]
pub struct StatsFacts {
    pub stack: Stack,
    pub current: String,
    /// `true` for `--current` (scope to the current stack only).
    pub current_only: bool,
    pub repo_slug: Option<String>,
    /// `(ahead, behind)` vs the trunk's remote-tracking branch; `None` when
    /// there is no remote-tracking branch for trunk.
    pub trunk_divergence: Option<(usize, usize)>,
    pub tracked: TrackedFacts,
    /// branch -> (ahead, behind) vs its recorded parent.
    pub ahead_behind: HashMap<String, (usize, usize)>,
    pub worktrees: Vec<WorktreeFact>,
    /// Count of non-prunable worktrees that are dirty; repo-level, not scoped
    /// by `--current`. `None` when the dirty check was skipped.
    pub dirty_worktrees: Option<usize>,
    pub idle_slots: usize,
    pub hygiene: HygieneFacts,
    pub ci: Option<CiFacts>,
    pub ci_unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackShape {
    pub tracked: usize,
    pub independent: usize,
    pub deepest: usize,
    pub avg_height: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrMix {
    pub open: usize,
    pub draft: usize,
    pub ready: usize,
    pub no_pr: usize,
    pub merged: usize,
    pub closed: usize,
    pub frozen: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthCounts {
    pub need_restack: usize,
    pub missing_parent: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty_worktrees: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeCounts {
    pub linked: usize,
    pub idle_slots: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CiCounts {
    pub failing: usize,
    pub pending: usize,
    pub passing: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HygieneCounts {
    pub merged_but_local: usize,
    pub upstream_gone: usize,
    pub stale: usize,
    pub stale_days: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionItem {
    #[serde(skip)]
    pub glyph: &'static str,
    pub kind: &'static str,
    pub detail: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackSummary {
    pub root: String,
    pub height: usize,
    pub commits_ahead: usize,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_low: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_high: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoStats {
    pub scope: &'static str,
    pub trunk: String,
    pub current: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trunk_ahead: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trunk_behind: Option<usize>,
    pub stack_shape: StackShape,
    pub pr_mix: PrMix,
    pub health: HealthCounts,
    pub worktrees: WorktreeCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci: Option<CiCounts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci_unavailable_reason: Option<String>,
    pub attention: Vec<AttentionItem>,
    pub biggest_stacks: Vec<StackSummary>,
    pub hygiene: HygieneCounts,
    pub next_actions: Vec<String>,
}

/// Branches in `scope` whose parent is trunk, or whose parent is outside
/// `scope` — i.e. the roots of independent stacks within the scope. Sorted
/// ascending for deterministic ordering.
pub fn stack_roots(stack: &Stack, scope: &HashSet<String>) -> Vec<String> {
    let mut roots: Vec<String> = scope
        .iter()
        .filter(|name| {
            let parent = stack.branches.get(*name).and_then(|b| b.parent.as_deref());
            match parent {
                Some(p) => p == stack.trunk || !scope.contains(p),
                None => true,
            }
        })
        .cloned()
        .collect();
    roots.sort();
    roots
}

/// Members of `root`'s stack within `scope`, bottom-up (root first, then its
/// children walked depth-first in name order). Cycle-safe.
pub fn stack_members(stack: &Stack, root: &str, scope: &HashSet<String>) -> Vec<String> {
    let mut result = Vec::new();
    if !scope.contains(root) {
        return result;
    }

    let mut frames = vec![root.to_string()];
    let mut visited = HashSet::new();
    while let Some(current) = frames.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        result.push(current.clone());

        if let Some(info) = stack.branches.get(&current) {
            let mut children: Vec<&String> = info
                .children
                .iter()
                .filter(|c| scope.contains(*c))
                .collect();
            children.sort();
            for child in children.into_iter().rev() {
                frames.push(child.clone());
            }
        }
    }

    result
}

/// `1 + max chain length below root within scope`; a lone branch has height 1.
/// Cycle-safe (a node cannot contribute to its own height twice).
pub fn stack_height(stack: &Stack, root: &str, scope: &HashSet<String>) -> usize {
    fn height_of(
        stack: &Stack,
        node: &str,
        scope: &HashSet<String>,
        visiting: &mut HashSet<String>,
    ) -> usize {
        if !scope.contains(node) || !visiting.insert(node.to_string()) {
            return 0;
        }
        let max_child = stack
            .branches
            .get(node)
            .map(|b| b.children.as_slice())
            .unwrap_or(&[])
            .iter()
            .filter(|c| scope.contains(*c))
            .map(|c| height_of(stack, c, scope, visiting))
            .max()
            .unwrap_or(0);
        visiting.remove(node);
        1 + max_child
    }

    if !scope.contains(root) {
        return 0;
    }
    let mut visiting = HashSet::new();
    height_of(stack, root, scope, &mut visiting)
}

/// Average of `heights`, or `0.0` when empty.
pub fn avg_height(heights: &[usize]) -> f64 {
    if heights.is_empty() {
        return 0.0;
    }
    let sum: usize = heights.iter().sum();
    sum as f64 / heights.len() as f64
}

/// Render a block-character bar of `cells` width. A non-zero `value` always
/// gets at least one filled cell.
pub fn bar(value: usize, max: usize, cells: usize) -> String {
    if max == 0 || cells == 0 {
        return "░".repeat(cells);
    }
    let mut filled = (value * cells) / max;
    if value > 0 && filled == 0 {
        filled = 1;
    }
    filled = filled.min(cells);
    format!("{}{}", "█".repeat(filled), "░".repeat(cells - filled))
}

fn branch_pr_state_is_open(state: Option<&str>) -> bool {
    state.is_some_and(|s| s.eq_ignore_ascii_case("open"))
}

fn branch_pr_state_is(state: Option<&str>, want: &str) -> bool {
    state.is_some_and(|s| s.eq_ignore_ascii_case(want))
}

pub fn compute(facts: &StatsFacts) -> RepoStats {
    let stack = &facts.stack;

    let scope: HashSet<String> = if facts.current_only {
        if stack.branches.contains_key(&facts.current) {
            stack
                .current_stack(&facts.current)
                .into_iter()
                .filter(|b| b != &stack.trunk)
                .collect()
        } else {
            HashSet::new()
        }
    } else {
        stack
            .branches
            .keys()
            .filter(|b| *b != &stack.trunk)
            .cloned()
            .collect()
    };

    let roots = stack_roots(stack, &scope);
    let heights: Vec<usize> = roots
        .iter()
        .map(|r| stack_height(stack, r, &scope))
        .collect();
    let deepest = heights.iter().copied().max().unwrap_or(0);

    let stack_shape = StackShape {
        tracked: scope.len(),
        independent: roots.len(),
        deepest,
        avg_height: avg_height(&heights),
    };

    // --- PR mix ---
    let mut open = 0usize;
    let mut draft = 0usize;
    let mut no_pr = 0usize;
    let mut merged = 0usize;
    let mut closed = 0usize;
    for name in &scope {
        let Some(info) = stack.branches.get(name) else {
            continue;
        };
        let is_draft = info.pr_is_draft == Some(true);
        if branch_pr_state_is_open(info.pr_state.as_deref()) || is_draft {
            open += 1;
        }
        if is_draft {
            draft += 1;
        }
        if info.pr_number.is_none() {
            no_pr += 1;
        }
        if branch_pr_state_is(info.pr_state.as_deref(), "merged") {
            merged += 1;
        } else if branch_pr_state_is(info.pr_state.as_deref(), "closed") {
            closed += 1;
        }
    }
    let frozen = facts.tracked.frozen.intersection(&scope).count();
    let pr_mix = PrMix {
        open,
        draft,
        ready: open.saturating_sub(draft),
        no_pr,
        merged,
        closed,
        frozen,
    };

    // --- Health ---
    let need_restack = scope
        .iter()
        .filter(|b| stack.branches.get(*b).is_some_and(|i| i.needs_restack))
        .count();
    let missing_parent = scope
        .iter()
        .filter(|b| facts.tracked.missing_parent.contains_key(*b))
        .count();
    let health = HealthCounts {
        need_restack,
        missing_parent,
        dirty_worktrees: facts.dirty_worktrees,
    };

    // --- Worktrees ---
    let linked_worktrees: Vec<&WorktreeFact> = facts
        .worktrees
        .iter()
        .filter(|w| !w.is_main && !w.is_prunable)
        .collect();
    let worktree_counts = WorktreeCounts {
        linked: linked_worktrees.len(),
        idle_slots: facts.idle_slots,
    };

    // --- CI ---
    let ci = facts.ci.as_ref().map(|c| CiCounts {
        failing: c.failing,
        pending: c.pending,
        passing: c.passing,
    });

    // --- Biggest stacks ---
    let mut biggest_stacks: Vec<StackSummary> = roots
        .iter()
        .map(|root| {
            let members = stack_members(stack, root, &scope);
            let height = stack_height(stack, root, &scope);
            let commits_ahead: usize = members
                .iter()
                .filter_map(|m| facts.ahead_behind.get(m).map(|(ahead, _)| *ahead))
                .sum();
            let draft_count = members
                .iter()
                .filter(|m| stack.branches.get(*m).and_then(|i| i.pr_is_draft) == Some(true))
                .count();
            let no_pr_count = members
                .iter()
                .filter(|m| stack.branches.get(*m).is_none_or(|i| i.pr_number.is_none()))
                .count();
            let label = if draft_count > 0 {
                format!("{} draft", draft_count)
            } else if no_pr_count > 0 {
                format!("{} no PR", no_pr_count)
            } else {
                "ready".to_string()
            };
            let pr_numbers: Vec<u64> = members
                .iter()
                .filter_map(|m| stack.branches.get(m).and_then(|i| i.pr_number))
                .collect();
            StackSummary {
                root: root.clone(),
                height,
                commits_ahead,
                label,
                pr_low: pr_numbers.iter().min().copied(),
                pr_high: pr_numbers.iter().max().copied(),
            }
        })
        .collect();
    biggest_stacks.sort_by(|a, b| b.height.cmp(&a.height).then_with(|| a.root.cmp(&b.root)));
    biggest_stacks.truncate(3);

    // --- Attention ---
    let mut attention: Vec<AttentionItem> = Vec::new();

    let mut restack_items = 0usize;
    for root in &roots {
        if restack_items >= 3 {
            break;
        }
        let members = stack_members(stack, root, &scope);
        let needing: Vec<String> = members
            .into_iter()
            .filter(|m| stack.branches.get(m).is_some_and(|i| i.needs_restack))
            .collect();
        if needing.is_empty() {
            continue;
        }
        let mut detail = needing
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" → ");
        if needing.len() > 3 {
            detail.push_str(" …");
        }
        attention.push(AttentionItem {
            kind: "restack",
            glyph: "⇅",
            detail,
            count: needing.len(),
        });
        restack_items += 1;
    }

    if no_pr > 0 {
        let mut names: Vec<&String> = scope
            .iter()
            .filter(|b| stack.branches.get(*b).is_none_or(|i| i.pr_number.is_none()))
            .collect();
        names.sort();
        let mut detail = names
            .iter()
            .take(3)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("  ·  ");
        if names.len() > 3 {
            detail.push_str(&format!(" +{} more", names.len() - 3));
        }
        attention.push(AttentionItem {
            kind: "no_pr",
            glyph: "○",
            detail,
            count: no_pr,
        });
    }

    let mut missing_names: Vec<&String> = scope
        .iter()
        .filter(|b| facts.tracked.missing_parent.contains_key(*b))
        .collect();
    missing_names.sort();
    for branch in missing_names.into_iter().take(3) {
        let parent = &facts.tracked.missing_parent[branch];
        attention.push(AttentionItem {
            kind: "missing_parent",
            glyph: "⚑",
            detail: format!("{}  (missing: {})", branch, parent),
            count: 1,
        });
    }

    if !linked_worktrees.is_empty() {
        let detail = linked_worktrees
            .iter()
            .take(4)
            .map(|w| {
                let dirty = w.is_dirty.unwrap_or(false);
                format!("{} ({})", w.name, if dirty { "dirty" } else { "clean" })
            })
            .collect::<Vec<_>>()
            .join("  ·  ");
        attention.push(AttentionItem {
            kind: "lanes",
            glyph: "⎇",
            detail,
            count: linked_worktrees.len(),
        });
    }

    // --- Hygiene ---
    let hygiene = HygieneCounts {
        merged_but_local: facts.hygiene.merged_but_local,
        upstream_gone: facts.hygiene.upstream_gone,
        stale: facts.hygiene.stale,
        stale_days: facts.hygiene.stale_days,
    };

    // --- Next actions ---
    let mut next_actions: Vec<String> = Vec::new();
    if need_restack > 0 {
        next_actions.push("st restack --all".to_string());
        next_actions.push("st ss".to_string());
    }
    if next_actions.len() < 2 && no_pr > 0 && !next_actions.iter().any(|a| a == "st ss") {
        next_actions.push("st ss".to_string());
    }
    if next_actions.len() < 2 && (hygiene.merged_but_local + hygiene.upstream_gone) > 0 {
        next_actions.push("st sweep".to_string());
    }
    next_actions.truncate(2);
    if next_actions.is_empty() {
        next_actions.push("st ls".to_string());
    }

    RepoStats {
        scope: if facts.current_only { "current" } else { "all" },
        trunk: stack.trunk.clone(),
        current: facts.current.clone(),
        repo: facts.repo_slug.clone(),
        trunk_ahead: facts.trunk_divergence.map(|(ahead, _)| ahead),
        trunk_behind: facts.trunk_divergence.map(|(_, behind)| behind),
        stack_shape,
        pr_mix,
        health,
        worktrees: worktree_counts,
        ci,
        ci_unavailable_reason: facts.ci_unavailable_reason.clone(),
        attention,
        biggest_stacks,
        hygiene,
        next_actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use std::collections::HashMap;

    fn branch(parent: Option<&str>, children: Vec<&str>) -> StackBranch {
        StackBranch {
            name: String::new(),
            parent: parent.map(str::to_string),
            parent_revision: None,
            children: children.into_iter().map(str::to_string).collect(),
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            pr_is_draft: None,
        }
    }

    /// main (trunk)
    ///  ├── a
    ///  │    └── a1
    ///  │         └── a2
    ///  └── b
    fn two_stack_test_stack() -> Stack {
        let mut branches = HashMap::new();
        branches.insert("main".to_string(), branch(None, vec!["a", "b"]));
        branches.insert("a".to_string(), branch(Some("main"), vec!["a1"]));
        branches.insert("a1".to_string(), branch(Some("a"), vec!["a2"]));
        branches.insert("a2".to_string(), branch(Some("a1"), vec![]));
        branches.insert("b".to_string(), branch(Some("main"), vec![]));
        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    fn scope_all(stack: &Stack) -> HashSet<String> {
        stack
            .branches
            .keys()
            .filter(|b| *b != &stack.trunk)
            .cloned()
            .collect()
    }

    #[test]
    fn stack_roots_finds_direct_trunk_children() {
        let stack = two_stack_test_stack();
        let scope = scope_all(&stack);
        assert_eq!(stack_roots(&stack, &scope), vec!["a", "b"]);
    }

    #[test]
    fn stack_roots_treats_out_of_scope_parent_as_root() {
        let stack = two_stack_test_stack();
        let mut scope = scope_all(&stack);
        scope.remove("a");
        let roots = stack_roots(&stack, &scope);
        assert_eq!(roots, vec!["a1", "b"]);
    }

    #[test]
    fn stack_members_are_bottom_up() {
        let stack = two_stack_test_stack();
        let scope = scope_all(&stack);
        assert_eq!(stack_members(&stack, "a", &scope), vec!["a", "a1", "a2"]);
    }

    #[test]
    fn stack_members_empty_when_root_out_of_scope() {
        let stack = two_stack_test_stack();
        let scope: HashSet<String> = HashSet::new();
        assert!(stack_members(&stack, "a", &scope).is_empty());
    }

    #[test]
    fn stack_height_lone_branch_is_one() {
        let stack = two_stack_test_stack();
        let scope = scope_all(&stack);
        assert_eq!(stack_height(&stack, "b", &scope), 1);
    }

    #[test]
    fn stack_height_counts_deepest_chain() {
        let stack = two_stack_test_stack();
        let scope = scope_all(&stack);
        assert_eq!(stack_height(&stack, "a", &scope), 3);
    }

    #[test]
    fn stack_height_is_cycle_safe() {
        let mut branches = HashMap::new();
        branches.insert("main".to_string(), branch(None, vec!["a"]));
        branches.insert("a".to_string(), branch(Some("main"), vec!["b"]));
        branches.insert("b".to_string(), branch(Some("a"), vec!["a"]));
        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };
        let scope = scope_all(&stack);
        // Must terminate and return a finite height despite the a<->b cycle.
        assert!(stack_height(&stack, "a", &scope) >= 1);
    }

    #[test]
    fn avg_height_empty_is_zero() {
        assert_eq!(avg_height(&[]), 0.0);
    }

    #[test]
    fn avg_height_averages_values() {
        assert_eq!(avg_height(&[1, 2, 3]), 2.0);
    }

    #[test]
    fn bar_empty_at_zero_value() {
        assert_eq!(bar(0, 10, 10), "░".repeat(10));
    }

    #[test]
    fn bar_full_at_max_value() {
        assert_eq!(bar(10, 10, 10), "█".repeat(10));
    }

    #[test]
    fn bar_nonzero_value_gets_at_least_one_filled_cell() {
        let rendered = bar(1, 100, 10);
        assert_eq!(rendered.chars().filter(|c| *c == '█').count(), 1);
    }

    #[test]
    fn bar_handles_zero_max() {
        assert_eq!(bar(5, 0, 10), "░".repeat(10));
    }

    fn empty_facts(stack: Stack, current: &str) -> StatsFacts {
        StatsFacts {
            stack,
            current: current.to_string(),
            current_only: false,
            repo_slug: None,
            trunk_divergence: None,
            tracked: TrackedFacts::default(),
            ahead_behind: HashMap::new(),
            worktrees: Vec::new(),
            dirty_worktrees: None,
            idle_slots: 0,
            hygiene: HygieneFacts::default(),
            ci: None,
            ci_unavailable_reason: None,
        }
    }

    #[test]
    fn compute_reports_two_independent_stacks_and_deepest_height() {
        let stack = two_stack_test_stack();
        let facts = empty_facts(stack, "a2");
        let stats = compute(&facts);

        assert_eq!(stats.stack_shape.tracked, 4);
        assert_eq!(stats.stack_shape.independent, 2);
        assert_eq!(stats.stack_shape.deepest, 3);
        assert_eq!(
            stats.biggest_stacks.first().map(|s| s.root.as_str()),
            Some("a")
        );
    }

    #[test]
    fn compute_current_scope_limits_to_current_stack() {
        let stack = two_stack_test_stack();
        let mut facts = empty_facts(stack, "a1");
        facts.current_only = true;
        let stats = compute(&facts);

        assert_eq!(stats.scope, "current");
        assert_eq!(stats.stack_shape.tracked, 3); // a, a1, a2
        assert_eq!(stats.stack_shape.independent, 1);
    }

    #[test]
    fn compute_current_scope_empty_when_current_untracked() {
        let stack = two_stack_test_stack();
        let mut facts = empty_facts(stack, "untracked");
        facts.current_only = true;
        let stats = compute(&facts);

        assert_eq!(stats.stack_shape.tracked, 0);
        assert_eq!(stats.stack_shape.independent, 0);
        assert_eq!(stats.stack_shape.deepest, 0);
    }

    #[test]
    fn compute_pr_mix_counts_draft_as_open_and_subtracts_for_ready() {
        let mut branches = HashMap::new();
        branches.insert("main".to_string(), branch(None, vec!["a", "b", "c"]));
        let mut a = branch(Some("main"), vec![]);
        a.pr_number = Some(1);
        a.pr_state = Some("OPEN".to_string());
        a.pr_is_draft = Some(true);
        branches.insert("a".to_string(), a);
        let mut b = branch(Some("main"), vec![]);
        b.pr_number = Some(2);
        b.pr_state = Some("OPEN".to_string());
        b.pr_is_draft = Some(false);
        branches.insert("b".to_string(), b);
        branches.insert("c".to_string(), branch(Some("main"), vec![]));
        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };
        let facts = empty_facts(stack, "a");
        let stats = compute(&facts);

        assert_eq!(stats.pr_mix.open, 2);
        assert_eq!(stats.pr_mix.draft, 1);
        assert_eq!(stats.pr_mix.ready, 1);
        assert_eq!(stats.pr_mix.no_pr, 1);
    }

    #[test]
    fn compute_next_actions_always_non_empty() {
        let stack = two_stack_test_stack();
        let facts = empty_facts(stack, "a");
        let stats = compute(&facts);

        assert!(!stats.next_actions.is_empty());
    }

    #[test]
    fn compute_next_actions_falls_back_to_ls_when_nothing_needs_attention() {
        let mut branches = HashMap::new();
        branches.insert("main".to_string(), branch(None, vec!["a"]));
        let mut a = branch(Some("main"), vec![]);
        a.pr_number = Some(1);
        a.pr_state = Some("OPEN".to_string());
        branches.insert("a".to_string(), a);
        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };
        let facts = empty_facts(stack, "a");
        let stats = compute(&facts);

        assert_eq!(stats.next_actions, vec!["st ls".to_string()]);
    }

    #[test]
    fn compute_next_actions_pairs_restack_and_submit() {
        let mut branches = HashMap::new();
        branches.insert("main".to_string(), branch(None, vec!["a"]));
        let mut a = branch(Some("main"), vec![]);
        a.needs_restack = true;
        branches.insert("a".to_string(), a);
        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };
        let facts = empty_facts(stack, "a");
        let stats = compute(&facts);

        assert_eq!(stats.next_actions, vec!["st restack --all", "st ss"]);
    }
}
