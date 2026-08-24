//! Thin wrapper around shared `stax::application` topology layout.

use stax::application::{BranchSummary, topology_layout};

pub(super) use stax::application::{TopologyCell, TopologyNode, TopologyRow};

pub(super) fn layout(branches: &[BranchSummary]) -> Vec<TopologyRow> {
    topology_layout(branches)
}

#[cfg(test)]
mod tests {
    use super::{TopologyCell, TopologyNode, layout};
    use stax::application::BranchSummary;

    fn branch(
        name: &str,
        parent: Option<&str>,
        column: usize,
        is_current: bool,
        is_trunk: bool,
    ) -> BranchSummary {
        BranchSummary {
            name: name.into(),
            parent: parent.map(str::to_string),
            column,
            is_current,
            is_trunk,
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            ci_state: None,
        }
    }

    fn cell(
        lane: usize,
        top: bool,
        bottom: bool,
        left: bool,
        right: bool,
        node: Option<TopologyNode>,
    ) -> TopologyCell {
        TopologyCell {
            lane,
            top,
            bottom,
            left,
            right,
            node,
        }
    }

    #[test]
    fn nested_fork_matches_st_ls_connectors() {
        let rows = layout(&[
            branch("feature/a", Some("main"), 0, false, false),
            branch("feature/b-child", Some("feature/b"), 1, true, false),
            branch("feature/b", Some("main"), 1, false, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(
            rows[0].cells,
            vec![
                cell(0, false, true, false, false, Some(TopologyNode::Branch)),
                cell(1, false, false, false, false, None),
            ]
        );
        assert_eq!(
            rows[1].cells,
            vec![
                cell(0, true, true, false, false, None),
                cell(1, false, true, false, false, Some(TopologyNode::Current)),
            ]
        );
        assert_eq!(
            rows[2].cells,
            vec![
                cell(0, true, true, false, false, None),
                cell(1, true, true, false, false, Some(TopologyNode::Branch)),
            ]
        );
        assert_eq!(
            rows[3].cells,
            vec![
                cell(0, true, false, false, true, Some(TopologyNode::Branch)),
                cell(1, true, false, true, false, None),
            ]
        );
    }

    #[test]
    fn empty_topology_has_no_rows() {
        assert!(layout(&[]).is_empty());
    }
}
