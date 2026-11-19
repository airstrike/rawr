//! A value's `{:?}` text as a tree a shell can fold: each struct, tuple,
//! list, map and set the text spells opens into its entries. The text is
//! all the debugger gives, so the tree is read from it as written:
//! brackets nest, commas separate, a single colon starts a field's or an
//! entry's value, and quoted text is opaque.

/// One value in a `{:?}` text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The field's name or the map entry's key in front of the value.
    pub key: Option<String>,
    /// The value's whole text, as printed.
    pub text: String,
    /// What precedes the entries: `Point`, `Some`, or nothing for a bare
    /// list or tuple.
    pub head: String,
    /// The entries, for a value that has a bracket.
    pub group: Option<Group>,
}

/// The bracketed entries of a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// `{`, `[` or `(`.
    pub open: char,
    /// The bracket that closes `open`.
    pub close: char,
    /// The entries in order.
    pub entries: Vec<Node>,
}

impl Node {
    /// The tree `text` spells. Text that doesn't nest is one leaf.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let mut reader = Reader {
            chars: &chars,
            at: 0,
        };
        let node = reader.entry();
        if reader.at == chars.len() {
            node
        } else {
            Self::leaf(None, text.trim().to_owned())
        }
    }

    fn leaf(key: Option<String>, text: String) -> Self {
        Self {
            key,
            head: text.clone(),
            text,
            group: None,
        }
    }

    /// Whether the value opens into entries.
    #[must_use]
    pub fn is_group(&self) -> bool {
        self.group
            .as_ref()
            .is_some_and(|group| !group.entries.is_empty())
    }
}

struct Reader<'a> {
    chars: &'a [char],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn text(&self, from: usize) -> String {
        self.chars[from..self.at]
            .iter()
            .collect::<String>()
            .trim()
            .to_owned()
    }

    /// An entry: a value, or a key, a colon and a value.
    fn entry(&mut self) -> Node {
        self.skip_space();
        let start = self.at;
        let first = self.value();
        if self.peek() != Some(':') {
            return first;
        }
        self.at += 1;
        let key = self.text(start);
        let key = key.trim_end_matches(':').trim().to_owned();
        let mut value = self.value();
        value.key = Some(key);
        value
    }

    /// A value: its head, then a bracket's entries, up to a separator.
    fn value(&mut self) -> Node {
        self.skip_space();
        let start = self.at;
        let mut group = None;
        while let Some(next) = self.peek() {
            match next {
                '"' | '\'' => self.quoted(next),
                '{' | '[' | '(' if group.is_none() => {
                    let head = self.text(start);
                    self.at += 1;
                    let close = match next {
                        '{' => '}',
                        '[' => ']',
                        _ => ')',
                    };
                    let mut entries = Vec::new();
                    loop {
                        self.skip_space();
                        match self.peek() {
                            Some(end) if end == close => {
                                self.at += 1;
                                break;
                            }
                            Some(',') => self.at += 1,
                            Some('}' | ']' | ')') | None => break,
                            Some(_) => {
                                let before = self.at;
                                entries.push(self.entry());
                                if self.at == before {
                                    self.at += 1;
                                }
                            }
                        }
                    }
                    group = Some((
                        head,
                        Group {
                            open: next,
                            close,
                            entries,
                        },
                    ));
                }
                ',' | '}' | ']' | ')' => break,
                ':' if self.chars.get(self.at + 1) == Some(&':') => self.at += 2,
                ':' => break,
                _ => self.at += 1,
            }
        }
        let text = self.text(start);
        match group {
            Some((head, group)) => Node {
                key: None,
                text,
                head,
                group: Some(group),
            },
            None => Node::leaf(None, text),
        }
    }

    /// Past a quoted string or character, escapes and all.
    fn quoted(&mut self, quote: char) {
        self.at += 1;
        while let Some(next) = self.peek() {
            self.at += 1;
            match next {
                '\\' => self.at += 1,
                end if end == quote => return,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Node;

    #[test]
    fn a_struct_opens_into_its_fields_and_quoted_text_stays_whole() {
        let node = Node::parse(r#"Point { x: 1, label: "a, b {", inner: Some([1, 2]) }"#);
        assert_eq!(node.head, "Point");
        let group = node.group.as_ref().unwrap();
        assert_eq!(group.open, '{');
        let keys: Vec<_> = group
            .entries
            .iter()
            .map(|entry| entry.key.as_deref().unwrap())
            .collect();
        assert_eq!(keys, ["x", "label", "inner"]);
        assert_eq!(group.entries[1].text, r#""a, b {""#);
        let inner = &group.entries[2];
        assert_eq!(inner.head, "Some");
        assert_eq!(inner.text, "Some([1, 2])");
        let list = &inner.group.as_ref().unwrap().entries[0];
        assert_eq!(list.group.as_ref().unwrap().entries.len(), 2);
    }

    #[test]
    fn paths_maps_and_plain_values_read_as_written() {
        let node = Node::parse(r#"{"a": Kind::One, "b": 2}"#);
        let entries = &node.group.as_ref().unwrap().entries;
        assert_eq!(entries[0].key.as_deref(), Some(r#""a""#));
        assert_eq!(entries[0].text, "Kind::One");
        assert!(!Node::parse("42").is_group());
        assert!(!Node::parse("Unit").is_group());
        assert!(!Node::parse("[]").is_group());
    }
}
