//! A value as a tree a shell can open and close, as the inspector splits
//! it when a read asks for `"children"`: a struct, tuple, list or map is a
//! node whose children are its fields, items or entries, each named by its
//! field, its index or its key, and anything else is a leaf.

use serde_json::Value;

/// One value, or one part of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The value's text, as `Debug` printed it.
    text: String,
    /// Its parts, each with the name the inspector gave it.
    children: Vec<(String, Node)>,
}

impl Node {
    /// The tree of a value the inspector answered, `{"value", "children"}`,
    /// with `children` only on a value that has parts.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        Self {
            text: value["value"].as_str().unwrap_or_default().to_owned(),
            children: value["children"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|part| {
                    let name = part["name"].as_str().unwrap_or_default().to_owned();
                    (name, Self::from_json(part))
                })
                .collect(),
        }
    }

    /// The value's whole text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The node's parts, each with its name.
    #[must_use]
    pub fn children(&self) -> &[(String, Node)] {
        &self.children
    }

    /// The line an open node shows: what names its brackets, and the
    /// opening one. A value with parts is in a shape the standard derives
    /// write, so its first bracket opens them.
    #[must_use]
    pub fn opening(&self) -> &str {
        match self.text.find(['{', '(', '[']) {
            Some(start) if !self.children.is_empty() => &self.text[..=start],
            _ => &self.text,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Node;

    #[test]
    fn a_value_opens_into_the_parts_the_inspector_named() {
        let node = Node::from_json(&json!({
            "value": "App { count: 3, at: Some(1) }",
            "children": [
                { "name": "count", "value": "3" },
                { "name": "at", "value": "Some(1)", "children": [
                    { "name": "0", "value": "1" },
                ] },
            ],
        }));
        assert_eq!(node.opening(), "App {");
        let names: Vec<_> = node.children().iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["count", "at"]);
        let (_, at) = &node.children()[1];
        assert_eq!(at.opening(), "Some(");
        assert_eq!(at.children()[0].1.text(), "1");

        // A value without parts is a leaf, whatever its text holds.
        let leaf = Node::from_json(&json!({ "value": "[1] and more" }));
        assert!(leaf.children().is_empty());
        assert_eq!(leaf.opening(), "[1] and more");
    }
}
