use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteParityState {
    InSync,
    Ahead,
    Behind,
    Diverged,
    NoRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteParity {
    pub state: RemoteParityState,
    pub ahead: usize,
    pub behind: usize,
}

/// Classify a local branch against its own remote ref. `ahead_behind` is `(local-only, remote-only)`.
pub fn classify_remote_parity(
    remote_exists: bool,
    ahead_behind: Option<(usize, usize)>,
) -> RemoteParity {
    let (state, ahead, behind) = match (remote_exists, ahead_behind) {
        (true, Some((0, 0))) => (RemoteParityState::InSync, 0, 0),
        (true, Some((a, 0))) => (RemoteParityState::Ahead, a, 0),
        (true, Some((0, b))) => (RemoteParityState::Behind, 0, b),
        (true, Some((a, b))) => (RemoteParityState::Diverged, a, b),
        _ => (RemoteParityState::NoRemote, 0, 0),
    };
    RemoteParity {
        state,
        ahead,
        behind,
    }
}

/// Branches (input order) that exist on the remote and whose cached tracking OID is missing or stale.
pub fn refs_needing_fetch(
    branches: &[String],
    remote_heads: &HashMap<String, String>,
    cached_tracking_oids: &HashMap<String, String>,
) -> Vec<String> {
    branches
        .iter()
        .filter(|branch| {
            remote_heads
                .get(*branch)
                .is_some_and(|oid| cached_tracking_oids.get(*branch) != Some(oid))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{RemoteParity, RemoteParityState, classify_remote_parity, refs_needing_fetch};
    use std::collections::HashMap;

    #[test]
    fn classify_covers_every_state() {
        assert_eq!(
            classify_remote_parity(true, Some((0, 0))).state,
            RemoteParityState::InSync
        );
        let ahead = classify_remote_parity(true, Some((2, 0)));
        assert_eq!(
            (ahead.state, ahead.ahead, ahead.behind),
            (RemoteParityState::Ahead, 2, 0)
        );
        let behind = classify_remote_parity(true, Some((0, 3)));
        assert_eq!(
            (behind.state, behind.ahead, behind.behind),
            (RemoteParityState::Behind, 0, 3)
        );
        let div = classify_remote_parity(true, Some((1, 4)));
        assert_eq!(
            (div.state, div.ahead, div.behind),
            (RemoteParityState::Diverged, 1, 4)
        );
    }

    #[test]
    fn classify_no_remote_when_missing_or_unknown() {
        assert_eq!(
            classify_remote_parity(false, Some((1, 1))),
            RemoteParity {
                state: RemoteParityState::NoRemote,
                ahead: 0,
                behind: 0
            }
        );
        assert_eq!(
            classify_remote_parity(true, None).state,
            RemoteParityState::NoRemote
        );
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn refs_needing_fetch_filters_and_preserves_order() {
        let branches: Vec<String> = ["d", "a", "b", "c", "gone"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let remote = map(&[("a", "1"), ("b", "2"), ("c", "3"), ("d", "4")]);
        let cached = map(&[("a", "1"), ("b", "old"), ("gone", "9")]);
        assert_eq!(
            refs_needing_fetch(&branches, &remote, &cached),
            vec!["d".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn refs_needing_fetch_empty_when_all_match() {
        let branches = vec!["a".to_string()];
        let m = map(&[("a", "1")]);
        assert!(refs_needing_fetch(&branches, &m, &m).is_empty());
    }
}
