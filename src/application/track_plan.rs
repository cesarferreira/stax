//! Pure planning logic for `stax branch track --all-prs`.
//!
//! No git or forge I/O happens here — everything is plain data in, plain data
//! out, so the topological ordering, fetch planning, and parent resolution can
//! be unit tested without a repo or network access.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackCandidate {
    pub number: u64,
    pub head: String,
    pub base: String,
}

pub struct RepoFacts<'a> {
    pub trunk: &'a str,
    pub remote: &'a str,
    pub local_branches: &'a HashSet<String>,
    pub remote_branches: &'a HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParentSource {
    Trunk,
    LocalBranch,
    RemoteOnly,
    TrunkFallback { unresolved_base: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentDecision {
    pub parent: String,
    pub parent_rev_ref: String,
    pub source: ParentSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FetchPlan {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}

/// Order candidates so that a PR's base (when it is another candidate's head)
/// is always emitted before it. Deterministic: ties break on ascending input
/// index, so the result does not depend on the order PRs were fetched in.
pub fn topological_order(candidates: &[TrackCandidate]) -> Vec<usize> {
    let mut head_to_index: HashMap<&str, usize> = HashMap::new();
    for (i, c) in candidates.iter().enumerate() {
        head_to_index.entry(c.head.as_str()).or_insert(i);
    }

    let mut indegree = vec![0usize; candidates.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); candidates.len()];

    for (i, c) in candidates.iter().enumerate() {
        if let Some(&p) = head_to_index.get(c.base.as_str())
            && p != i
        {
            indegree[i] += 1;
            children[p].push(i);
        }
    }

    let mut ready: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
    for (i, &d) in indegree.iter().enumerate() {
        if d == 0 {
            ready.push(Reverse(i));
        }
    }

    let mut order = Vec::with_capacity(candidates.len());
    let mut emitted = vec![false; candidates.len()];

    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        emitted[i] = true;
        for &child in &children[i] {
            indegree[child] -= 1;
            if indegree[child] == 0 {
                ready.push(Reverse(child));
            }
        }
    }

    // Base cycles leave some nodes with indegree > 0 forever; still emit them
    // (in ascending index order) rather than silently dropping a PR.
    for (i, done) in emitted.iter().enumerate() {
        if !done {
            order.push(i);
        }
    }

    order
}

/// Decide what to fetch before resolving parents: every candidate head that
/// isn't local yet, plus every non-trunk base that isn't local yet either.
pub fn plan_fetches(
    candidates: &[TrackCandidate],
    trunk: &str,
    local_branches: &HashSet<String>,
) -> FetchPlan {
    let mut required = Vec::new();
    let mut required_set: HashSet<&str> = HashSet::new();
    for c in candidates {
        if !local_branches.contains(&c.head) && required_set.insert(c.head.as_str()) {
            required.push(c.head.clone());
        }
    }

    let mut optional = Vec::new();
    let mut optional_set: HashSet<&str> = HashSet::new();
    for c in candidates {
        if c.base != trunk
            && !local_branches.contains(&c.base)
            && !required_set.contains(c.base.as_str())
            && optional_set.insert(c.base.as_str())
        {
            optional.push(c.base.clone());
        }
    }

    FetchPlan { required, optional }
}

/// Resolve a candidate's parent purely from repo facts — never from other
/// candidates directly — so the result does not depend on iteration or fetch
/// order. By the time this runs, phase 1 has already made in-batch heads
/// visible in `facts.local_branches`, so rule 2 covers the stacked case.
pub fn resolve_parent(candidate: &TrackCandidate, facts: &RepoFacts<'_>) -> ParentDecision {
    let base = &candidate.base;

    if base == facts.trunk {
        return ParentDecision {
            parent: facts.trunk.to_string(),
            parent_rev_ref: facts.trunk.to_string(),
            source: ParentSource::Trunk,
        };
    }

    if facts.local_branches.contains(base) {
        return ParentDecision {
            parent: base.clone(),
            parent_rev_ref: base.clone(),
            source: ParentSource::LocalBranch,
        };
    }

    if facts.remote_branches.contains(base) {
        return ParentDecision {
            parent: base.clone(),
            parent_rev_ref: format!("{}/{}", facts.remote, base),
            source: ParentSource::RemoteOnly,
        };
    }

    ParentDecision {
        parent: facts.trunk.to_string(),
        parent_rev_ref: facts.trunk.to_string(),
        source: ParentSource::TrunkFallback {
            unresolved_base: base.clone(),
        },
    }
}
