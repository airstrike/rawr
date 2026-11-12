//! The top-level items of a generated source, as rustc reads them, each
//! with the attributes and comments before it, and the name each declares
//! or implements, so the runner's crates can be split by what their items
//! name (`program`).

/// One top-level item of a generated source.
pub(super) struct Item<'a> {
    /// Its text, from the end of the item before it.
    pub(super) text: &'a str,
    /// The name it declares, or the type its `impl` is for.
    pub(super) name: Option<String>,
    /// Whether it declares its name, where an `impl` names a type declared
    /// elsewhere.
    pub(super) declares: bool,
}

/// The words before an item's keyword: its visibility and qualifiers.
const QUALIFIERS: [&str; 8] = [
    "pub", "unsafe", "async", "extern", "crate", "super", "in", "self",
];

/// The items of `source` in order. Their texts join to `source`.
pub(super) fn split(source: &str) -> Vec<Item<'_>> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;
    let mut nested = 0_usize;
    let bytes = source.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = string_end(bytes, at + 1),
            b'r' if raw_string(bytes, at) => at = raw_string_end(bytes, at),
            b'\'' => at = quote_end(source, at),
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                at = source[at..].find('\n').map_or(bytes.len(), |end| at + end);
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at = source[at + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| at + 2 + end + 2);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && nested == 0 {
                    // `static X: T = T { .. };` ends at its `;`.
                    let rest = source[at + 1..].trim_start();
                    if !rest.starts_with(';') {
                        items.push(item(&source[start..=at]));
                        start = at + 1;
                    }
                }
            }
            b'(' | b'[' => nested += 1,
            b')' | b']' => nested = nested.saturating_sub(1),
            b';' if depth == 0 && nested == 0 => {
                items.push(item(&source[start..=at]));
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    if start < source.len() {
        match items.last_mut() {
            Some(last) if source[start..].trim().is_empty() => {
                let begin = last.text.as_ptr() as usize - source.as_ptr() as usize;
                last.text = &source[begin..];
            }
            _ => items.push(item(&source[start..])),
        }
    }
    items
}

/// The byte after the string whose text starts at `at`.
fn string_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// Whether a raw string `r"`, `r#"`, .. starts at `at`.
fn raw_string(bytes: &[u8], at: usize) -> bool {
    if at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_') {
        return false;
    }
    let hashes = bytes[at + 1..]
        .iter()
        .take_while(|byte| **byte == b'#')
        .count();
    bytes.get(at + 1 + hashes) == Some(&b'"')
}

/// The last byte of the raw string starting at `at`.
fn raw_string_end(bytes: &[u8], at: usize) -> usize {
    let hashes = bytes[at + 1..]
        .iter()
        .take_while(|byte| **byte == b'#')
        .count();
    let open = at + 1 + hashes + 1;
    let mut close = Vec::with_capacity(hashes + 1);
    close.push(b'"');
    close.extend(std::iter::repeat_n(b'#', hashes));
    bytes[open..]
        .windows(close.len())
        .position(|window| window == close.as_slice())
        .map_or(bytes.len(), |end| open + end + close.len() - 1)
}

/// The last byte of a char literal at `at`, or `at` for a lifetime.
fn quote_end(source: &str, at: usize) -> usize {
    let rest = &source[at + 1..];
    if let Some(escaped) = rest.strip_prefix('\\') {
        // `'\n'`, `'\''`, `'\u{..}'`.
        let skip = escaped.chars().next().map_or(0, char::len_utf8);
        return escaped[skip..]
            .find('\'')
            .map_or(at, |end| at + 2 + skip + end);
    }
    let mut chars = rest.char_indices();
    match (chars.next(), chars.next()) {
        (Some((_, first)), Some((end, '\''))) if first != '\'' => at + 1 + end,
        _ => at,
    }
}

fn item(text: &str) -> Item<'_> {
    Item {
        text,
        name: name(text),
        declares: words(text)
            .into_iter()
            .find(|word| !QUALIFIERS.contains(word))
            .is_some_and(|word| word != "impl"),
    }
}

/// The name `text`'s item declares, or the last segment of the type its
/// `impl` is for.
fn name(text: &str) -> Option<String> {
    let mut words = words(text).into_iter().peekable();
    while let Some(word) = words.next() {
        match word {
            word if QUALIFIERS.contains(&word) => {}
            "struct" | "enum" | "union" | "type" | "fn" | "static" | "trait" | "mod" => {
                return words.find(|word| *word != "mut").map(str::to_owned);
            }
            "const" => {
                // `const fn name` or `const NAME`.
                let next = words.next()?;
                let name = if next == "fn" { words.next()? } else { next };
                return Some(name.to_owned());
            }
            "macro_rules" => return words.next().map(str::to_owned),
            "impl" => return implemented(text),
            _ => return None,
        }
    }
    None
}

/// The identifiers of `text` outside attributes, comments and generics up
/// to its body: what its head says.
fn words(text: &str) -> Vec<&str> {
    let head = text.find('{').map_or(text, |end| &text[..end]);
    let mut words = Vec::new();
    let mut rest = head;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return words;
        }
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |end| &after[end..]);
            continue;
        }
        if rest.starts_with("#[") || rest.starts_with("#![") {
            rest = &rest[balanced(rest, b'[', b']').unwrap_or(rest.len())..];
            continue;
        }
        let length = rest
            .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .unwrap_or(rest.len());
        if length == 0 {
            // Punctuation, as `pub(crate)`'s parentheses.
            rest = &rest[rest.chars().next().map_or(1, char::len_utf8)..];
            continue;
        }
        words.push(&rest[..length]);
        rest = &rest[length..];
    }
}

/// The byte after the bracket that closes the first `open` of `text`.
fn balanced(text: &str, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, byte) in text.bytes().enumerate() {
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(at + 1);
            }
        }
    }
    None
}

/// The last path segment of the type `text`'s `impl` is for, before its
/// arguments.
fn implemented(text: &str) -> Option<String> {
    // Its comments and attributes aside, as the generator writes them, one
    // to a line.
    let code = text
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("//") && !line.starts_with('#')
        })
        .collect::<Vec<_>>()
        .join("\n");
    let at = code.match_indices("impl").map(|(at, _)| at).find(|at| {
        code[..*at]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
            && code[at + 4..]
                .chars()
                .next()
                .is_some_and(|next| next == '<' || next.is_whitespace())
    })?;
    let head = &code[at + 4..];
    let head = head.find('{').map_or(head, |end| &head[..end]);
    // Generics, the trait's and the type's arguments, at depth zero only.
    let mut depth = 0_usize;
    let mut flat = String::with_capacity(head.len());
    for character in head.chars() {
        match character {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => flat.push(character),
            _ => {}
        }
    }
    let ty = match flat.find(" for ") {
        Some(at) => &flat[at + 5..],
        None => &flat,
    };
    let segment = ty.trim().rsplit("::").next()?;
    segment
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|word| !word.is_empty() && *word != "mut" && *word != "dyn")
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Items end at a depth-zero `}` or `;`, whatever braces their strings,
    /// chars and comments hold, and each says the name it declares or the
    /// type its `impl` is for.
    #[test]
    fn items_split_at_their_ends_and_name_what_they_declare() {
        let source = "#[allow(dead_code)]\npub struct RawrFamily1;\n\n\
            impl __rawr_internal::scope::Family for RawrFamily1 {\n    type Of<'x> = Vec<&'x str>;\n}\n\n\
            impl<'a, T: Clone> crate::Trait<T> for Wrapper<'a, T> {\n    fn f() { let _ = \"}{\"; let _ = '}'; // }\n    }\n}\n\
            /* } */ pub static TABLE: Table = Table { rows: &[] };\n\
            #[allow(unused_imports)]\npub use ::app::theme as _;\n\
            pub fn register() {\n}\n";
        let items = split(source);
        let names = items
            .iter()
            .map(|item| item.name.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                Some("RawrFamily1"),
                Some("RawrFamily1"),
                Some("Wrapper"),
                Some("TABLE"),
                None,
                Some("register")
            ]
        );
        assert_eq!(
            items.iter().map(|item| item.text).collect::<String>(),
            source
        );
    }
}
