//! A value's parts as its `{:?}` text writes them, for a tool that shows a
//! value as a tree: the fields of `Name { a: 1 }`, the items of `Name(1,
//! 2)`, `(1, 2)` and `[1, 2]`, and the entries of `{"k": 1}` and `{1, 2}`.
//! Every value prints through `Debug`, the program's own and a crate's, so
//! the tree reads the same for both. A text that isn't in the shapes the
//! standard derives and builders write, as a hand-written `Debug` may be,
//! has no parts: the tool shows it whole.

use serde_json::{json, Value};

/// The parts of the value `text` prints, each `{"name", "value",
/// "children"}` with `children` only on a part that has parts of its own;
/// `None` when it has none.
#[must_use]
pub fn of(text: &str) -> Option<Vec<Value>> {
    let mut reader = Reader { text, at: 0 };
    let node = reader.value()?;
    reader.blank();
    if reader.at != text.len() {
        return None;
    }
    node.parts
        .map(|parts| parts.into_iter().map(Part::into_json).collect())
}

/// One value in the text: where it is, and its parts when it has any.
struct Node {
    start: usize,
    end: usize,
    parts: Option<Vec<Part>>,
}

struct Part {
    name: String,
    value: String,
    node: Node,
}

impl Part {
    fn into_json(self) -> Value {
        match self.node.parts {
            Some(parts) => json!({
                "name": self.name,
                "value": self.value,
                "children": parts.into_iter().map(Self::into_json).collect::<Vec<_>>(),
            }),
            None => json!({ "name": self.name, "value": self.value }),
        }
    }
}

/// How the items of a bracketed list are named.
#[derive(Clone, Copy)]
enum Naming {
    /// `.0`, `.1` and on, as a tuple's fields.
    Field,
    /// `[0]`, `[1]` and on, as a sequence's items.
    Index,
}

struct Reader<'t> {
    text: &'t str,
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn blank(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.blank();
        let found = self.peek() == Some(byte);
        if found {
            self.at += 1;
        }
        found
    }

    fn slice(&self, node: &Node) -> String {
        self.text[node.start..node.end].to_owned()
    }

    /// One value: a quoted string or char, a bracketed list or map, a
    /// name with fields or items, or a word such as a number or a unit
    /// variant.
    fn value(&mut self) -> Option<Node> {
        self.blank();
        let start = self.at;
        match self.peek()? {
            quote @ (b'"' | b'\'') => {
                self.quoted(quote)?;
                Some(self.leaf(start))
            }
            b'[' => {
                self.at += 1;
                let parts = self.items(b']', Naming::Index)?;
                Some(self.node(start, parts))
            }
            b'(' => {
                self.at += 1;
                let parts = self.items(b')', Naming::Field)?;
                Some(self.node(start, parts))
            }
            b'{' => {
                self.at += 1;
                let parts = self.entries()?;
                Some(self.node(start, parts))
            }
            _ => {
                self.word()?;
                let named = self.at;
                self.blank();
                match self.peek() {
                    Some(b'(') => {
                        self.at += 1;
                        let parts = self.items(b')', Naming::Field)?;
                        Some(self.node(start, parts))
                    }
                    Some(b'{') => {
                        self.at += 1;
                        let parts = self.fields()?;
                        Some(self.node(start, parts))
                    }
                    _ => {
                        self.at = named;
                        Some(self.leaf(start))
                    }
                }
            }
        }
    }

    fn leaf(&self, start: usize) -> Node {
        Node {
            start,
            end: self.at,
            parts: None,
        }
    }

    fn node(&self, start: usize, parts: Vec<Part>) -> Node {
        Node {
            start,
            end: self.at,
            parts: Some(parts),
        }
    }

    /// A quoted literal, its escapes skipped.
    fn quoted(&mut self, quote: u8) -> Option<()> {
        self.at += 1;
        loop {
            match self.peek()? {
                b'\\' => self.at += 2,
                byte if byte == quote => {
                    self.at += 1;
                    return Some(());
                }
                _ => self.at += 1,
            }
        }
    }

    /// A run of anything but the brackets, separators and quotes: a path
    /// keeps its `::`.
    fn word(&mut self) -> Option<()> {
        let start = self.at;
        while let Some(byte) = self.peek() {
            match byte {
                b':' if self.text.as_bytes().get(self.at + 1) == Some(&b':') => self.at += 2,
                b',' | b':' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b'\'' => break,
                byte if byte.is_ascii_whitespace() => break,
                _ => self.at += 1,
            }
        }
        (self.at > start).then_some(())
    }

    /// Items up to `close`, separated by commas.
    fn items(&mut self, close: u8, naming: Naming) -> Option<Vec<Part>> {
        let mut parts = Vec::new();
        loop {
            if self.eat(close) {
                return Some(parts);
            }
            let node = self.value()?;
            let name = match naming {
                Naming::Field => parts.len().to_string(),
                Naming::Index => format!("[{}]", parts.len()),
            };
            parts.push(Part {
                name,
                value: self.slice(&node),
                node,
            });
            if !self.eat(b',') {
                return self.eat(close).then_some(parts);
            }
        }
    }

    /// A struct's `name: value` fields up to `}`; `..` stands for fields
    /// it leaves out.
    fn fields(&mut self) -> Option<Vec<Part>> {
        let mut parts = Vec::new();
        loop {
            if self.eat(b'}') {
                return Some(parts);
            }
            self.blank();
            let start = self.at;
            self.word()?;
            let name = self.text[start..self.at].to_owned();
            if name != ".." {
                if !self.eat(b':') {
                    return None;
                }
                let node = self.value()?;
                parts.push(Part {
                    name,
                    value: self.slice(&node),
                    node,
                });
            }
            if !self.eat(b',') {
                return self.eat(b'}').then_some(parts);
            }
        }
    }

    /// A map's `key: value` entries, each named by its key's text, or a
    /// set's items, up to `}`.
    fn entries(&mut self) -> Option<Vec<Part>> {
        let mut parts = Vec::new();
        loop {
            if self.eat(b'}') {
                return Some(parts);
            }
            let key = self.value()?;
            let part = if self.eat(b':') {
                let node = self.value()?;
                Part {
                    name: self.slice(&key),
                    value: self.slice(&node),
                    node,
                }
            } else {
                Part {
                    name: format!("[{}]", parts.len()),
                    value: self.slice(&key),
                    node: key,
                }
            };
            parts.push(part);
            if !self.eat(b',') {
                return self.eat(b'}').then_some(parts);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_debug_text_splits_into_named_parts_down_to_its_leaves() {
        assert_eq!(
            super::of(
                r#"Counter { count: 3, label: "a, b", items: [1, 2], at: Some((1, 'x')), .. }"#
            ),
            Some(vec![
                json!({ "name": "count", "value": "3" }),
                json!({ "name": "label", "value": "\"a, b\"" }),
                json!({ "name": "items", "value": "[1, 2]", "children": [
                    { "name": "[0]", "value": "1" },
                    { "name": "[1]", "value": "2" },
                ] }),
                json!({ "name": "at", "value": "Some((1, 'x'))", "children": [
                    { "name": "0", "value": "(1, 'x')", "children": [
                        { "name": "0", "value": "1" },
                        { "name": "1", "value": "'x'" },
                    ] },
                ] }),
            ])
        );
        assert_eq!(
            super::of(r#"{"k": Point(1, -2), "j": {7}}"#),
            Some(vec![
                json!({ "name": "\"k\"", "value": "Point(1, -2)", "children": [
                    { "name": "0", "value": "1" },
                    { "name": "1", "value": "-2" },
                ] }),
                json!({ "name": "\"j\"", "value": "{7}", "children": [
                    { "name": "[0]", "value": "7" },
                ] }),
            ])
        );
        assert_eq!(super::of("std::cmp::Ordering::Less"), None);
        assert_eq!(super::of("42"), None);
        // A hand-written text in no standard shape stays whole.
        assert_eq!(super::of("<3 items: a; b>"), None);
        assert_eq!(super::of("Empty {}"), Some(Vec::new()));
    }
}
