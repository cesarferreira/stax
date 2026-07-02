/// Build the ordered list of candidate parents for a reparent operation.
///
/// Starting from `all_branches`, drops `source` and its `descendants`
/// (since a branch can't be reparented onto itself or something that
/// depends on it), sorts the remainder alphabetically for predictable
/// keyboard navigation, then pins `trunk` (if present) to the top so it
/// becomes the obvious default target.
///
/// Shared between the CLI picker (`pick_parent_interactively` in
/// `src/commands/upstack/onto.rs`) and the TUI move picker
/// (`init_move_picker` in `src/tui/app.rs`) so both surfaces present the
/// same candidate set in the same order.
pub fn build_parent_candidates(
    all_branches: &[String],
    source: &str,
    descendants: &[String],
    trunk: &str,
) -> Vec<String> {
    let mut candidates: Vec<String> = all_branches
        .iter()
        .filter(|n| n.as_str() != source && !descendants.contains(*n))
        .cloned()
        .collect();
    candidates.sort();
    if let Some(pos) = candidates.iter().position(|n| n == trunk) {
        let t = candidates.remove(pos);
        candidates.insert(0, t);
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::build_parent_candidates;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn excludes_source_and_descendants() {
        let all = names(&["main", "a", "b", "c", "sibling"]);
        let descendants = names(&["b", "c"]);
        let got = build_parent_candidates(&all, "a", &descendants, "main");
        assert_eq!(got, names(&["main", "sibling"]));
    }

    #[test]
    fn pins_trunk_to_top_then_sorts_remainder() {
        // Candidates are sorted alphabetically, then trunk is pinned to
        // the top. `apple` sorts before `main`, so this test also proves
        // the trunk pin moves it from a non-first position.
        let all = names(&["z-branch", "sibling", "main", "apple"]);
        let got = build_parent_candidates(&all, "feat-self", &[], "main");
        assert_eq!(got, names(&["main", "apple", "sibling", "z-branch"]));
    }

    #[test]
    fn sorts_alphabetically_when_trunk_absent() {
        // Detached scenario: trunk isn't among the candidates. The
        // remainder is still sorted so navigation stays predictable.
        let all = names(&["z", "a", "m"]);
        let got = build_parent_candidates(&all, "feat", &[], "unlisted-trunk");
        assert_eq!(got, names(&["a", "m", "z"]));
    }

    #[test]
    fn empty_when_everything_is_source_or_descendant() {
        let all = names(&["a", "b"]);
        let got = build_parent_candidates(&all, "a", &names(&["b"]), "a");
        assert!(got.is_empty());
    }
}
