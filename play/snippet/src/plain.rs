//! A snippet as a program with a `fn main`.

/// `source` with each `#[cfg(test)]` item (such as `mod tests { .. }`)
/// blanked, keeping line numbers: a pasted example often ends with tests.
#[must_use]
pub fn without_tests(source: &str) -> String {
    const ATTRIBUTE: &str = "#[cfg(test)]";
    let mut out = source.to_owned();
    let mut from = 0;
    while let Some(found) = out[from..].find(ATTRIBUTE) {
        let start = from + found;
        let mut depth = 0usize;
        let mut end = out.len();
        for (offset, c) in out[start + ATTRIBUTE.len()..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + ATTRIBUTE.len() + offset + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = start + ATTRIBUTE.len() + offset + 1;
                    break;
                }
                _ => {}
            }
        }
        let blank: String = out[start..end]
            .chars()
            .map(|c| if c == '\n' { c } else { ' ' })
            .collect();
        out.replace_range(start..end, &blank);
        from = start + blank.len();
    }
    out
}

/// The program a snippet runs as. One with items runs as written. One of
/// statements alone becomes a block in `fn main`, as Rust reads a block: a
/// final expression without `;` is its value, which is printed with `{:?}`
/// unless it is `()`. The block opens on the first line and closes after
/// the snippet, so its lines and columns keep their numbers.
#[must_use]
pub fn program(source: &str) -> String {
    wrapped(source, true)
}

/// [`program`], with a final value that is evaluated and never printed:
/// the program for a value whose type has no `Debug`, or never ends.
pub fn quiet_program(source: &str) -> String {
    wrapped(source, false)
}

fn wrapped(source: &str, show: bool) -> String {
    let source = without_tests(source);
    if has_item(&source) {
        return source;
    }
    if show {
        format!(
            "fn main() {{ let __v = {{ {source}\n}};\n\
             let __d = format!(\"{{:?}}\", __v); if __d != \"()\" {{ println!(\"{{}}\", __d); }} }}\n"
        )
    } else {
        // The final value is a statement: an iterator there runs no stage.
        format!("fn main() {{ {source}\n; }}\n")
    }
}

/// Whether a top-level item starts a statement, outside strings, chars and
/// comments: statements end at `;`, and at a `}` closing a block that the
/// next word does not continue (`else`). An inner attribute (`#![..]`) is
/// the crate's, so a snippet that opens with one is a file, run as written.
pub fn has_item(source: &str) -> bool {
    const ITEMS: &[&str] = &[
        "fn", "pub", "struct", "enum", "impl", "trait", "use", "mod", "const", "static", "type",
        "#[", "#![",
    ];
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut item = false;
    let mut start = true;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        // A comment keeps `start`: the item may follow it.
        let comment = byte == b'/' && matches!(bytes.get(index + 1), Some(b'/' | b'*'));
        if start && !byte.is_ascii_whitespace() && !comment {
            start = false;
            let rest = &source[index..];
            let word = |rest: &str, word: &str| {
                rest.starts_with(word)
                    && (word.starts_with('#')
                        || !rest[word.len()..]
                            .starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            };
            // `async fn` is an item, and `async { .. }` a statement.
            item |= ITEMS.iter().any(|item| word(rest, item))
                || (word(rest, "async") && word(rest["async".len()..].trim_start(), "fn"));
        }
        match byte {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = source[index..]
                    .find('\n')
                    .map_or(bytes.len(), |end| index + end);
                continue;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = source[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| index + end + 4);
                continue;
            }
            b'"' => {
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
            }
            b'\''
                if bytes.get(index + 2) == Some(&b'\'')
                    || (bytes.get(index + 1) == Some(&b'\\')
                        && bytes.get(index + 3) == Some(&b'\'')) =>
            {
                index += if bytes[index + 1] == b'\\' { 3 } else { 2 };
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let next = source[index + 1..].trim_start();
                    let word: String = next
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !word.is_empty() && word != "else" && word != "as" {
                        start = true;
                    }
                }
            }
            b';' if depth == 0 => {
                start = true;
            }
            _ => {}
        }
        index += 1;
    }
    item
}

#[cfg(test)]
mod tests {
    use super::program;

    #[test]
    fn plain_snippets_become_programs() {
        let print =
            "let __d = format!(\"{:?}\", __v); if __d != \"()\" { println!(\"{}\", __d); } }\n";
        assert_eq!(
            program("1 + 2"),
            format!("fn main() {{ let __v = {{ 1 + 2\n}};\n{print}")
        );
        assert_eq!(
            program("let a = 1;\na + 1"),
            format!("fn main() {{ let __v = {{ let a = 1;\na + 1\n}};\n{print}")
        );
        let looped = "for i in 0..2 {\n    println!(\"{i}\");\n}";
        assert_eq!(
            program(looped),
            format!("fn main() {{ let __v = {{ {looped}\n}};\n{print}")
        );
        let items = "fn main() {\n    println!(\"x\");\n}\n";
        assert_eq!(program(items), items);
        let item_later = "let s = \"a;b\";\nfn f() {}";
        assert_eq!(program(item_later), item_later);
        let asynchronous = "async fn main() {}\n";
        assert_eq!(program(asynchronous), asynchronous);
        assert!(!super::has_item("async { 1 }.await"));
        let commented = "// About it.\n/* More. */ fn main() {}\n";
        assert_eq!(program(commented), commented);
        assert_eq!(super::quiet_program("1 + 2"), "fn main() { 1 + 2\n; }\n");
        let attributed = "#![allow(dead_code)]\n#![cfg_attr(test, allow(unused))]\nfn main() {}\n";
        assert_eq!(program(attributed), attributed);
        let statements = "#![allow(unused)]\nlet x = 1;\n";
        assert_eq!(program(statements), statements);
    }
}
