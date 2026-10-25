//! A plain program: a snippet that does not use iced, as a program with a
//! `fn main`.

/// Where a snippet's `async fn main` is, as rawr-play rewrites it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsyncMain {
    /// The byte offset of the name `main`.
    pub name: usize,
    /// The span of a `#[tokio::main]` (or `#[tokio::main(..)]`) on it.
    pub attribute: Option<std::ops::Range<usize>>,
    /// Whether it returns a value (a `Result`), which `?` and an `Err`
    /// end the program with.
    pub returns: bool,
}

/// The `async fn main` at the top level of `source`, with tests blanked.
#[must_use]
pub fn async_main(source: &str) -> Option<AsyncMain> {
    let mut offset = 0;
    let mut attribute = None;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && trimmed.starts_with("#[tokio::main") {
            let end = trimmed
                .find(']')
                .map_or(trimmed.trim_end().len(), |end| end + 1);
            attribute = Some(offset..offset + end);
        }
        let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
        if indent == 0
            && let Some(rest) = rest.strip_prefix("async fn main")
            && rest.trim_start().starts_with('(')
        {
            let name = offset + (line.len() - rest.len()) - "main".len();
            let signature = &source[name..];
            let signature = &signature[..signature.find('{').unwrap_or(signature.len())];
            return Some(AsyncMain {
                name,
                attribute,
                returns: signature.contains("->"),
            });
        }
        if !trimmed.is_empty() && !trimmed.starts_with("#[") && !trimmed.starts_with("//") {
            attribute = None;
        }
        offset += line.len();
    }
    None
}

/// The program an `async fn main` runs as: the snippet with its `main`
/// renamed `rawr_play_main` and its `#[tokio::main]` blanked, so every
/// line keeps its number, and after it an app whose boot Task awaits it.
pub fn async_program(source: &str, main: &AsyncMain) -> String {
    let mut program = source.to_owned();
    program.insert_str(main.name, "rawr_play_");
    if let Some(attribute) = &main.attribute {
        program.replace_range(attribute.clone(), &" ".repeat(attribute.len()));
    }
    let run = if main.returns {
        "match rawr_play_main().await { Ok(_) => 0, Err(error) => { eprintln!(\"Error: {error:?}\"); 1 } }"
    } else {
        "rawr_play_main().await; 0"
    };
    program.push_str(&format!(
        "\n#[derive(Default)]\nstruct RawrPlayAsync {{ status: i64 }}\n\
         #[derive(Debug, Clone)]\nenum RawrPlayDone {{ Done(i64) }}\n\
         impl RawrPlayAsync {{\n\
             fn start() -> (Self, iced::Task<RawrPlayDone>) {{ (Self {{ status: -1 }}, iced::Task::perform(rawr_play_run(), RawrPlayDone::Done)) }}\n\
             fn update(&mut self, message: RawrPlayDone) {{ match message {{ RawrPlayDone::Done(status) => self.status = status }} }}\n\
             fn view(&self) -> impl iced::Widget<RawrPlayDone> {{ iced::widget::text(\"\") }}\n\
             fn title(&self) -> String {{ self.status.to_string() }}\n\
         }}\n\
         async fn rawr_play_run() -> i64 {{ {run} }}\n\
         pub fn main() -> iced::Result {{ iced::application(RawrPlayAsync::start, RawrPlayAsync::update, RawrPlayAsync::view).title(RawrPlayAsync::title).run() }}\n"
    ));
    program
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
    let source = crate::without_tests(source);
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
/// next word does not continue (`else`).
pub fn has_item(source: &str) -> bool {
    const ITEMS: &[&str] = &[
        "fn", "pub", "struct", "enum", "impl", "trait", "use", "mod", "const", "static", "type",
        "#[",
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
            item |= ITEMS.iter().any(|word| {
                rest.starts_with(word)
                    && (*word == "#["
                        || !rest[word.len()..]
                            .starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            });
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
        let commented = "// About it.\n/* More. */ fn main() {}\n";
        assert_eq!(program(commented), commented);
        assert_eq!(super::quiet_program("1 + 2"), "fn main() { 1 + 2\n; }\n");
    }
}
