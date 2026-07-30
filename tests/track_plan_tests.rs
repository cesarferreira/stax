//! Unit tests for the pure `stax branch track --all-prs` planning logic in
//! `stax::application::track_plan`. No repo, no network — these drive
//! `topological_order`, `plan_fetches`, and `resolve_parent` directly.

use stax::application::{
    ParentSource, RepoFacts, TrackCandidate, plan_fetches, resolve_parent, topological_order,
};
use std::collections::HashSet;

fn candidate(number: u64, head: &str, base: &str) -> TrackCandidate {
    TrackCandidate {
        number,
        head: head.to_string(),
        base: base.to_string(),
    }
}

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn topological_order_stack_supplied_newest_first() {
    // c -> b -> a -> main, supplied newest-first: [c, b, a]
    let candidates = vec![
        candidate(3, "c", "b"),
        candidate(2, "b", "a"),
        candidate(1, "a", "main"),
    ];
    let order = topological_order(&candidates);
    assert_eq!(order, vec![2, 1, 0]);
}

#[test]
fn topological_order_stack_every_permutation_is_root_first() {
    let base_candidates = [
        candidate(1, "a", "main"),
        candidate(2, "b", "a"),
        candidate(3, "c", "b"),
    ];

    // All 6 permutations of the 3 candidates by original index.
    let perms: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];

    for perm in perms {
        let candidates: Vec<TrackCandidate> =
            perm.iter().map(|&i| base_candidates[i].clone()).collect();
        let order = topological_order(&candidates);

        let head_position = |head: &str| -> usize {
            order
                .iter()
                .position(|&idx| candidates[idx].head == head)
                .unwrap()
        };

        assert!(
            head_position("a") < head_position("b"),
            "perm {:?}: 'a' must come before 'b'",
            perm
        );
        assert!(
            head_position("b") < head_position("c"),
            "perm {:?}: 'b' must come before 'c'",
            perm
        );
    }
}

#[test]
fn topological_order_base_cycle_emits_both_indices_once() {
    let candidates = vec![candidate(1, "a", "b"), candidate(2, "b", "a")];
    let mut order = topological_order(&candidates);
    order.sort_unstable();
    assert_eq!(order, vec![0, 1]);
}

#[test]
fn resolve_parent_local_branch_for_stacked_base() {
    let candidate = candidate(2, "b", "a");
    let local = set(&["a", "b", "main"]);
    let remote = set(&[]);
    let facts = RepoFacts {
        trunk: "main",
        remote: "origin",
        local_branches: &local,
        remote_branches: &remote,
    };

    let decision = resolve_parent(&candidate, &facts);
    assert_eq!(decision.parent, "a");
    assert_eq!(decision.parent_rev_ref, "a");
    assert_eq!(decision.source, ParentSource::LocalBranch);
}

#[test]
fn resolve_parent_trunk_fallback_when_base_unresolved() {
    let candidate = candidate(3, "c", "missing-base");
    let local = set(&["c", "main"]);
    let remote = set(&[]);
    let facts = RepoFacts {
        trunk: "main",
        remote: "origin",
        local_branches: &local,
        remote_branches: &remote,
    };

    let decision = resolve_parent(&candidate, &facts);
    assert_eq!(decision.parent, "main");
    assert_eq!(decision.parent_rev_ref, "main");
    assert_eq!(
        decision.source,
        ParentSource::TrunkFallback {
            unresolved_base: "missing-base".to_string()
        }
    );
}

#[test]
fn resolve_parent_remote_only_uses_remote_ref() {
    let candidate = candidate(4, "d", "remote-base");
    let local = set(&["d", "main"]);
    let remote = set(&["remote-base"]);
    let facts = RepoFacts {
        trunk: "main",
        remote: "origin",
        local_branches: &local,
        remote_branches: &remote,
    };

    let decision = resolve_parent(&candidate, &facts);
    assert_eq!(decision.parent, "remote-base");
    assert_eq!(decision.parent_rev_ref, "origin/remote-base");
    assert_eq!(decision.source, ParentSource::RemoteOnly);
}

#[test]
fn resolve_parent_trunk_base_is_trunk_source() {
    let candidate = candidate(5, "e", "main");
    let local = set(&["e", "main"]);
    let remote = set(&[]);
    let facts = RepoFacts {
        trunk: "main",
        remote: "origin",
        local_branches: &local,
        remote_branches: &remote,
    };

    let decision = resolve_parent(&candidate, &facts);
    assert_eq!(decision.parent, "main");
    assert_eq!(decision.parent_rev_ref, "main");
    assert_eq!(decision.source, ParentSource::Trunk);
}

#[test]
fn plan_fetches_splits_required_and_optional_without_duplicates_or_trunk() {
    let candidates = vec![
        candidate(1, "a", "main"),  // head missing, base is trunk
        candidate(2, "b", "a"),     // head missing, base is another candidate's head (missing)
        candidate(3, "c", "local"), // head missing, base already local
    ];
    let local = set(&["local", "main"]);

    let plan = plan_fetches(&candidates, "main", &local);

    assert_eq!(plan.required, vec!["a", "b", "c"]);
    assert_eq!(plan.optional, Vec::<String>::new());
    assert!(!plan.required.contains(&"main".to_string()));
}

#[test]
fn plan_fetches_lists_missing_non_trunk_base_as_optional() {
    let candidates = vec![candidate(1, "a", "upstream-base")];
    let local = set(&["main"]); // "a" and "upstream-base" both missing locally

    let plan = plan_fetches(&candidates, "main", &local);

    assert_eq!(plan.required, vec!["a".to_string()]);
    assert_eq!(plan.optional, vec!["upstream-base".to_string()]);
}

#[test]
fn plan_fetches_already_local_branches_appear_in_neither_list() {
    let candidates = vec![candidate(1, "a", "main")];
    let local = set(&["a", "main"]);

    let plan = plan_fetches(&candidates, "main", &local);

    assert!(plan.required.is_empty());
    assert!(plan.optional.is_empty());
}
