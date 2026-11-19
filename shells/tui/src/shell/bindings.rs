//! The shown frame's bindings as rows: each binding, and the fields of
//! each value the person opened.

use super::Shell;
use crate::value::Node;

/// One row of the bindings pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    /// The binding's name, then the key or index of each field down to
    /// this one: what keeps it open across steps.
    pub(super) path: Vec<String>,
    pub(super) depth: usize,
    pub(super) text: String,
    pub(super) expandable: bool,
    pub(super) expanded: bool,
    pub(super) unreadable: bool,
}

impl Shell {
    /// The rows the bindings pane shows.
    pub(super) fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for binding in &self.bindings {
            let path = vec![binding.name.clone()];
            match &binding.value {
                Ok(node) => self.node_rows(&mut rows, path, 0, &binding.name, node),
                Err(reason) => rows.push(Row {
                    path,
                    depth: 0,
                    text: format!("{}: {reason}", binding.name),
                    expandable: false,
                    expanded: false,
                    unreadable: true,
                }),
            }
        }
        rows
    }

    fn node_rows(
        &self,
        rows: &mut Vec<Row>,
        path: Vec<String>,
        depth: usize,
        name: &str,
        node: &Node,
    ) {
        let expandable = !node.children().is_empty();
        let expanded = expandable && self.expanded.contains(&path);
        let text = if expanded {
            format!("{name} = {}", node.opening())
        } else {
            format!("{name} = {}", node.text())
        };
        rows.push(Row {
            path: path.clone(),
            depth,
            text,
            expandable,
            expanded,
            unreadable: false,
        });
        if !expanded {
            return;
        }
        for (key, child) in node.children() {
            let mut child_path = path.clone();
            child_path.push(key.clone());
            self.node_rows(rows, child_path, depth + 1, key, child);
        }
    }
}
