//! A value's `Debug` text as a tree a shell can open and close: a struct,
//! tuple, list or map is a node whose children are its fields or items,
//! and anything else is a leaf.

/// One value, or one field of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The field's name or the map entry's key, if it has one.
    key: Option<String>,
    /// What comes before the brackets (a type or variant name), or the
    /// whole text of a leaf.
    head: String,
    /// The value's text, as `Debug` printed it.
    text: String,
    /// The opening bracket, for a node with children.
    open: Option<char>,
    children: Vec<Node>,
}

impl Node {
    /// The tree of `text`, a value's `Debug` text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        Self::parse_keyed(None, text.trim())
    }

    fn parse_keyed(key: Option<String>, text: &str) -> Self {
        let leaf = || Self {
            key: key.clone(),
            head: text.to_owned(),
            text: text.to_owned(),
            open: None,
            children: Vec::new(),
        };
        let Some((start, open)) = first_open(text) else {
            return leaf();
        };
        let Some(end) = closing(text, start) else {
            return leaf();
        };
        // A value is its brackets and what names them, nothing after.
        if end + 1 != text.len() {
            return leaf();
        }
        let inner = &text[start + 1..end];
        let children = split(inner)
            .into_iter()
            .filter(|item| !item.trim().is_empty() && item.trim() != "..")
            .map(|item| {
                let item = item.trim();
                match field(item) {
                    Some((name, value)) => Self::parse_keyed(Some(name.to_owned()), value),
                    None => Self::parse_keyed(None, item),
                }
            })
            .collect();
        Self {
            key,
            head: text[..start].trim().to_owned(),
            text: text.to_owned(),
            open: Some(open),
            children,
        }
    }

    /// The field's name or the entry's key.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// The value's whole text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The node's children: its fields or items.
    #[must_use]
    pub fn children(&self) -> &[Node] {
        &self.children
    }

    /// The line an open node shows: its name and opening bracket.
    #[must_use]
    pub fn opening(&self) -> String {
        match self.open {
            Some(open) if self.head.is_empty() => open.to_string(),
            Some(open) => format!("{} {open}", self.head),
            None => self.head.clone(),
        }
    }
}

/// The first bracket at the top of `text`, outside quotes.
fn first_open(text: &str) -> Option<(usize, char)> {
    let mut quote = Quote::default();
    for (index, char) in text.char_indices() {
        if quote.take(char) {
            continue;
        }
        if matches!(char, '{' | '(' | '[') {
            return Some((index, char));
        }
    }
    None
}

/// Where the bracket at `start` closes.
fn closing(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut quote = Quote::default();
    for (index, char) in text[start..].char_indices() {
        if quote.take(char) {
            continue;
        }
        match char {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + index);
                }
            }
            _ => {}
        }
    }
    None
}

/// `text` cut at its top-level commas.
fn split(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0_usize;
    let mut quote = Quote::default();
    let mut from = 0;
    for (index, char) in text.char_indices() {
        if quote.take(char) {
            continue;
        }
        match char {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(&text[from..index]);
                from = index + 1;
            }
            _ => {}
        }
    }
    items.push(&text[from..]);
    items
}

/// A field `name: value`, or a map entry `key: value`, split at its first
/// top-level `": "`.
fn field(item: &str) -> Option<(&str, &str)> {
    let mut depth = 0_usize;
    let mut quote = Quote::default();
    for (index, char) in item.char_indices() {
        if quote.take(char) {
            continue;
        }
        match char {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 && item[index + 1..].starts_with(' ') => {
                return Some((item[..index].trim(), item[index + 1..].trim()));
            }
            _ => {}
        }
    }
    None
}

/// Whether a scan is inside a string or character literal, as `Debug`
/// escapes them.
#[derive(Debug, Default)]
struct Quote {
    inside: Option<char>,
    escaped: bool,
}

impl Quote {
    /// Take `char`: whether it belongs to a literal, its quotes included.
    fn take(&mut self, char: char) -> bool {
        match self.inside {
            Some(quote) => {
                if self.escaped {
                    self.escaped = false;
                } else if char == '\\' {
                    self.escaped = true;
                } else if char == quote {
                    self.inside = None;
                }
                true
            }
            None if char == '"' || char == '\'' => {
                self.inside = Some(char);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Node;

    #[test]
    fn a_struct_opens_into_its_fields_and_a_string_stays_whole() {
        let node = Node::parse(r#"App { count: 3, names: ["a, b", "}"], at: Some((1, 'x')) }"#);
        assert_eq!(node.opening(), "App {");
        let keys: Vec<_> = node.children().iter().map(Node::key).collect();
        assert_eq!(keys, [Some("count"), Some("names"), Some("at")]);
        assert_eq!(node.children()[0].text(), "3");
        assert!(node.children()[0].children().is_empty());
        let names = &node.children()[1];
        assert_eq!(names.opening(), "[");
        assert_eq!(
            names.children().iter().map(Node::text).collect::<Vec<_>>(),
            [r#""a, b""#, r#""}""#]
        );
        let at = &node.children()[2];
        assert_eq!(at.opening(), "Some (");
        assert_eq!(at.children()[0].children()[1].text(), "'x'");
    }

    #[test]
    fn a_map_opens_into_its_entries_and_plain_text_is_a_leaf() {
        let map = Node::parse(r#"{"one": 1, "two": [2]}"#);
        assert_eq!(map.children()[1].key(), Some(r#""two""#));
        assert_eq!(map.children()[1].children()[0].text(), "2");
        let leaf = Node::parse("Ordering::Less");
        assert!(leaf.children().is_empty());
        assert_eq!(leaf.opening(), "Ordering::Less");
        // A closure's text, or text after the brackets, stays whole.
        assert!(Node::parse("[1] and more").children().is_empty());
    }
}
