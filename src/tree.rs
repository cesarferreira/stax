/// Tree rendering utilities for displaying branch stacks

/// Render a tree structure as a string
/// Each node has a name, depth, and optional children
pub fn render_tree(nodes: &[TreeNode]) -> String {
    let mut lines = Vec::new();
    for node in nodes {
        render_node(node, 0, &mut lines);
    }
    lines.join("\n")
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub name: String,
    pub is_current: bool,
    pub badge: Option<String>,
    pub children: Vec<TreeNode>,
}

fn render_node(node: &TreeNode, depth: usize, lines: &mut Vec<String>) {
    // Render children first (leaves at top) - sorted for consistency
    let mut sorted_children = node.children.clone();
    sorted_children.sort_by(|a, b| a.name.cmp(&b.name));

    for child in &sorted_children {
        render_node(child, depth + 1, lines);
    }

    // Indentation: 2 spaces per depth level
    let indent = "  ".repeat(depth);

    // Branch indicator
    let indicator = if node.is_current { "*" } else { "o" };

    // Build the line
    let mut line = format!("{}{} {}", indent, indicator, node.name);

    if node.is_current {
        line.push_str(" <");
    }

    if let Some(badge) = &node.badge {
        line.push_str(&format!(" {}", badge));
    }

    lines.push(line);

    // Add connector line to parent (if not root)
    if depth > 0 {
        lines.push(format!("{}|", indent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_stack() {
        let tree = vec![TreeNode {
            name: "main".to_string(),
            is_current: false,
            badge: None,
            children: vec![TreeNode {
                name: "feature1".to_string(),
                is_current: false,
                badge: None,
                children: vec![TreeNode {
                    name: "feature2".to_string(),
                    is_current: true,
                    badge: None,
                    children: vec![],
                }],
            }],
        }];

        let output = render_tree(&tree);
        // feature2 at depth 2 (4 spaces), feature1 at depth 1 (2 spaces), main at depth 0
        let expected = "    * feature2 <\n    |\n  o feature1\n  |\no main";

        assert_eq!(output, expected, "\nGot:\n{}\n\nExpected:\n{}", output, expected);
    }

    #[test]
    fn test_two_stacks_from_trunk() {
        // main has two children: feature-a and feature-b
        let tree = vec![TreeNode {
            name: "main".to_string(),
            is_current: false,
            badge: None,
            children: vec![
                TreeNode {
                    name: "feature-a".to_string(),
                    is_current: false,
                    badge: None,
                    children: vec![TreeNode {
                        name: "feature-a2".to_string(),
                        is_current: false,
                        badge: None,
                        children: vec![],
                    }],
                },
                TreeNode {
                    name: "feature-b".to_string(),
                    is_current: true,
                    badge: None,
                    children: vec![],
                },
            ],
        }];

        let output = render_tree(&tree);

        // Both feature-a and feature-b should be at same indent (depth 1 = 2 spaces)
        // feature-a2 should be at depth 2 = 4 spaces
        let expected = "    o feature-a2\n    |\n  o feature-a\n  |\n  * feature-b <\n  |\no main";

        assert_eq!(output, expected, "\nGot:\n{}\n\nExpected:\n{}", output, expected);
    }

    #[test]
    fn test_alignment_same_depth() {
        // Verify all branches at same depth have same indentation
        let tree = vec![TreeNode {
            name: "main".to_string(),
            is_current: false,
            badge: None,
            children: vec![
                TreeNode {
                    name: "stack-a".to_string(),
                    is_current: false,
                    badge: None,
                    children: vec![
                        TreeNode {
                            name: "stack-a-child".to_string(),
                            is_current: false,
                            badge: None,
                            children: vec![],
                        },
                    ],
                },
                TreeNode {
                    name: "stack-b".to_string(),
                    is_current: true,
                    badge: None,
                    children: vec![
                        TreeNode {
                            name: "stack-b-child".to_string(),
                            is_current: false,
                            badge: None,
                            children: vec![],
                        },
                    ],
                },
            ],
        }];

        let output = render_tree(&tree);
        let lines: Vec<&str> = output.lines().collect();

        // Find lines with branch names and check indentation
        let stack_a_child_line = lines.iter().find(|l| l.contains("stack-a-child")).unwrap();
        let stack_b_child_line = lines.iter().find(|l| l.contains("stack-b-child")).unwrap();

        let stack_a_indent = stack_a_child_line.len() - stack_a_child_line.trim_start().len();
        let stack_b_indent = stack_b_child_line.len() - stack_b_child_line.trim_start().len();

        assert_eq!(
            stack_a_indent, stack_b_indent,
            "Branches at same depth should have same indentation.\nstack-a-child indent: {}\nstack-b-child indent: {}",
            stack_a_indent, stack_b_indent
        );
    }
}
