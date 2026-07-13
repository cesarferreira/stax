use stax::application::BranchSummary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TopologySegment {
    pub lane: Option<usize>,
    pub glyph: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TopologyRow {
    pub branch_name: String,
    pub segments: Vec<TopologySegment>,
}

pub(super) fn layout(branches: &[BranchSummary]) -> Vec<TopologyRow> {
    let Some(max_column) = branches.iter().map(|branch| branch.column).max() else {
        return Vec::new();
    };
    let target_width = (max_column + 1) * 2;

    branches
        .iter()
        .enumerate()
        .map(|(index, branch)| {
            if branch.is_trunk {
                trunk_row(branches, branch, target_width)
            } else {
                branch_row(branches, index, branch, target_width)
            }
        })
        .collect()
}

fn branch_row(
    branches: &[BranchSummary],
    row_index: usize,
    branch: &BranchSummary,
    target_width: usize,
) -> TopologyRow {
    let needs_corner = row_index
        .checked_sub(1)
        .and_then(|index| branches.get(index))
        .is_some_and(|previous| previous.column > branch.column);
    let mut segments = Vec::new();

    for lane in 0..=branch.column {
        if lane == branch.column {
            segments.push(TopologySegment {
                lane: Some(lane),
                glyph: if branch.is_current { "◉" } else { "○" }.into(),
            });
            if needs_corner {
                segments.push(TopologySegment {
                    lane: Some(lane),
                    glyph: "─┘".into(),
                });
            }
        } else {
            segments.push(TopologySegment {
                lane: Some(lane),
                glyph: "│ ".into(),
            });
        }
    }

    pad_row(&mut segments, target_width);
    TopologyRow {
        branch_name: branch.name.clone(),
        segments,
    }
}

fn trunk_row(
    branches: &[BranchSummary],
    trunk: &BranchSummary,
    target_width: usize,
) -> TopologyRow {
    let direct_child_max_column = branches
        .iter()
        .filter(|branch| branch.parent.as_deref() == Some(trunk.name.as_str()))
        .map(|branch| branch.column)
        .max()
        .unwrap_or(0);
    let mut segments = vec![TopologySegment {
        lane: Some(0),
        glyph: if trunk.is_current { "◉" } else { "○" }.into(),
    }];

    for lane in 1..=direct_child_max_column {
        segments.push(TopologySegment {
            lane: Some(lane),
            glyph: if lane < direct_child_max_column {
                "─┴".into()
            } else {
                "─┘".into()
            },
        });
    }

    pad_row(&mut segments, target_width);
    TopologyRow {
        branch_name: trunk.name.clone(),
        segments,
    }
}

fn pad_row(segments: &mut Vec<TopologySegment>, target_width: usize) {
    let visual_width = segments
        .iter()
        .map(|segment| segment.glyph.chars().count())
        .sum::<usize>();
    segments.push(TopologySegment {
        lane: None,
        glyph: " ".repeat(target_width.saturating_sub(visual_width) + 1),
    });
}

#[cfg(test)]
mod tests {
    use super::layout;
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

    fn plain(row: &super::TopologyRow) -> String {
        row.segments
            .iter()
            .map(|segment| segment.glyph.as_str())
            .collect()
    }

    #[test]
    fn nested_fork_matches_st_ls_connectors() {
        let rows = layout(&[
            branch("feature/a", Some("main"), 0, false, false),
            branch("feature/b-child", Some("feature/b"), 1, true, false),
            branch("feature/b", Some("main"), 1, false, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(plain(&rows[0]), "○    ");
        assert_eq!(plain(&rows[1]), "│ ◉  ");
        assert_eq!(plain(&rows[2]), "│ ○  ");
        assert_eq!(plain(&rows[3]), "○─┘  ");
        assert_eq!(rows[1].segments[1].lane, Some(1));
    }

    #[test]
    fn linear_stack_keeps_every_node_on_the_same_lane() {
        let rows = layout(&[
            branch("tip", Some("middle"), 0, false, false),
            branch("middle", Some("main"), 0, true, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(
            rows.iter().map(plain).collect::<Vec<_>>(),
            vec!["○  ", "◉  ", "○  "]
        );
    }

    #[test]
    fn dropping_a_lane_draws_the_return_corner() {
        let rows = layout(&[
            branch("nested", Some("side"), 2, false, false),
            branch("side", Some("main"), 1, false, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(plain(&rows[1]), "│ ○─┘  ");
    }

    #[test]
    fn trunk_joins_only_direct_child_lanes() {
        let rows = layout(&[
            branch("a", Some("main"), 0, false, false),
            branch("nested", Some("side"), 2, false, false),
            branch("side", Some("main"), 1, false, false),
            branch("main", None, 0, false, true),
        ]);

        assert_eq!(plain(rows.last().unwrap()), "○─┘    ");
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
        assert_eq!(plain(trunk), "○─┴─┘  ");
        assert_eq!(trunk.segments[1].lane, Some(1));
        assert_eq!(trunk.segments[2].lane, Some(2));
    }

    #[test]
    fn empty_topology_has_no_rows() {
        assert!(layout(&[]).is_empty());
    }
}
