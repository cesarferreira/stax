use stax::application::BranchSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TopologyNode {
    Branch,
    Current,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TopologyCell {
    pub lane: usize,
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
    pub node: Option<TopologyNode>,
}

impl TopologyCell {
    fn empty(lane: usize) -> Self {
        Self {
            lane,
            top: false,
            bottom: false,
            left: false,
            right: false,
            node: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TopologyRow {
    pub branch_name: String,
    pub cells: Vec<TopologyCell>,
}

pub(super) fn layout(branches: &[BranchSummary]) -> Vec<TopologyRow> {
    let Some(max_column) = branches.iter().map(|branch| branch.column).max() else {
        return Vec::new();
    };
    let lane_count = max_column + 1;

    branches
        .iter()
        .enumerate()
        .map(|(index, branch)| {
            if branch.is_trunk {
                trunk_row(branches, branch, lane_count)
            } else {
                branch_row(branches, index, branch, lane_count)
            }
        })
        .collect()
}

fn branch_row(
    branches: &[BranchSummary],
    row_index: usize,
    branch: &BranchSummary,
    lane_count: usize,
) -> TopologyRow {
    let previous_column = row_index
        .checked_sub(1)
        .and_then(|index| branches.get(index))
        .map(|previous| previous.column);
    let mut cells = cells(lane_count);

    for cell in cells.iter_mut().take(branch.column) {
        cell.top = true;
        cell.bottom = true;
    }

    let node = &mut cells[branch.column];
    node.top = previous_column == Some(branch.column);
    node.bottom = true;
    node.node = Some(if branch.is_current {
        TopologyNode::Current
    } else {
        TopologyNode::Branch
    });

    if let Some(previous_column) = previous_column.filter(|column| *column > branch.column) {
        cells[branch.column].right = true;
        for (lane, cell) in cells
            .iter_mut()
            .enumerate()
            .take(previous_column + 1)
            .skip(branch.column + 1)
        {
            cell.left = true;
            cell.right = lane < previous_column;
            cell.top = lane == previous_column;
        }
    }

    TopologyRow {
        branch_name: branch.name.clone(),
        cells,
    }
}

fn trunk_row(branches: &[BranchSummary], trunk: &BranchSummary, lane_count: usize) -> TopologyRow {
    let direct_child_max_column = branches
        .iter()
        .filter(|branch| branch.parent.as_deref() == Some(trunk.name.as_str()))
        .map(|branch| branch.column)
        .max()
        .unwrap_or(0);
    let mut cells = cells(lane_count);
    cells[0].top = branches.iter().any(|branch| !branch.is_trunk);
    cells[0].right = direct_child_max_column > 0;
    cells[0].node = Some(if trunk.is_current {
        TopologyNode::Current
    } else {
        TopologyNode::Branch
    });

    for (lane, cell) in cells
        .iter_mut()
        .enumerate()
        .take(direct_child_max_column + 1)
        .skip(1)
    {
        cell.top = true;
        cell.left = true;
        cell.right = lane < direct_child_max_column;
    }

    TopologyRow {
        branch_name: trunk.name.clone(),
        cells,
    }
}

fn cells(lane_count: usize) -> Vec<TopologyCell> {
    (0..lane_count).map(TopologyCell::empty).collect()
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
    fn linear_stack_keeps_every_node_on_the_same_lane() {
        let rows = layout(&[
            branch("tip", Some("middle"), 0, false, false),
            branch("middle", Some("main"), 0, true, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(
            rows[0].cells[0],
            cell(0, false, true, false, false, Some(TopologyNode::Branch))
        );
        assert_eq!(
            rows[1].cells[0],
            cell(0, true, true, false, false, Some(TopologyNode::Current))
        );
        assert_eq!(
            rows[2].cells[0],
            cell(0, true, false, false, false, Some(TopologyNode::Branch))
        );
    }

    #[test]
    fn dropping_a_lane_draws_the_return_corner() {
        let rows = layout(&[
            branch("nested", Some("side"), 2, false, false),
            branch("side", Some("main"), 1, false, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(
            rows[1].cells,
            vec![
                cell(0, true, true, false, false, None),
                cell(1, false, true, false, true, Some(TopologyNode::Branch)),
                cell(2, true, false, true, false, None),
            ]
        );
    }

    #[test]
    fn trunk_joins_only_direct_child_lanes() {
        let rows = layout(&[
            branch("a", Some("main"), 0, false, false),
            branch("nested", Some("side"), 2, false, false),
            branch("side", Some("main"), 1, false, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(
            rows.last().unwrap().cells,
            vec![
                cell(0, true, false, false, true, Some(TopologyNode::Branch)),
                cell(1, true, false, true, false, None),
                cell(2, false, false, false, false, None),
            ]
        );
    }

    #[test]
    fn trunk_joins_multiple_direct_sibling_lanes() {
        let rows = layout(&[
            branch("left", Some("main"), 0, false, false),
            branch("middle", Some("main"), 1, false, false),
            branch("right", Some("main"), 2, false, false),
            branch("main", None, 0, false, true),
        ]);

        let trunk = rows.last().unwrap();
        assert_eq!(
            trunk.cells,
            vec![
                cell(0, true, false, false, true, Some(TopologyNode::Branch)),
                cell(1, true, false, true, true, None),
                cell(2, true, false, true, false, None),
            ]
        );
    }

    #[test]
    fn empty_topology_has_no_rows() {
        assert!(layout(&[]).is_empty());
    }
}
