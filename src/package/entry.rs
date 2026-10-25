//! A marked root's async `main` and its runtime's entry (D513.4).
//!
//! `#[tokio::main] async fn main() { .. }` in a marked crate root names the
//! runtime that runs it. Rawr blesses none (D512): the shadow crate gets a
//! native entry that carries the program's own attribute, copied as
//! written, around an `async fn` that awaits the Rawr `main`'s future. The
//! runtime the project chose then runs it as `cargo run` would, with the
//! attribute's arguments (`flavor`, `worker_threads`) as given.
//!
//! Rawr's `main` registers the entry when it is called
//! (`crate::__rawr_entry::start()`, which the compiler writes before the
//! body), and the runner starts it on a thread of its own, as it starts a
//! native root's `main` (`island::start`).

/// The module the entry lives in, in the shadow crate.
pub(crate) const MODULE: &str = "__rawr_entry";

/// The attribute above a top-level `async fn main` in `text`, as written,
/// when its path names a `main` macro of a crate (`#[tokio::main]`,
/// `#[async_std::main]`, `#[tokio::main(flavor = "current_thread")]`). An
/// attribute may span lines. When its path starts at a name a top-level
/// `use .. as name;` binds, as `use tokio as rt; #[rt::main]` does, that
/// `use` comes first, so the shadow crate's entry resolves it too.
pub(crate) fn attribute(text: &str) -> Option<String> {
    let mut attribute: Option<(std::ops::Range<usize>, &str)> = None;
    let mut offset = 0;
    // The end of an attribute that spans lines: lines before it are its own.
    let mut within = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if start < within {
            continue;
        }
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && trimmed.starts_with("#[") {
            let end = matching_bracket(&text[start..]).map(|end| start + end + 1);
            if let Some(end) = end {
                within = end;
                if let Some(path) = text[start + 2..end - 1].split(['(', ']', ' ', '\n']).next()
                    && path.contains("::")
                    && path.rsplit("::").next() == Some("main")
                {
                    attribute = Some((start..end, path));
                }
            }
        } else if indent == 0 {
            let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
            if let Some(rest) = rest.strip_prefix("async fn main")
                && rest.trim_start().starts_with('(')
            {
                let (range, path) = attribute?;
                let first = path.split("::").next().unwrap_or_default();
                return Some(match alias(text, first) {
                    Some(binding) => format!("{binding}\n    {}", &text[range]),
                    None => text[range].to_owned(),
                });
            }
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                attribute = None;
            }
        }
    }
    None
}

/// A top-level `use path as name;` in `text` that binds `name`.
fn alias<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let statement = line.trim_end();
        let rest = statement
            .strip_prefix("pub use ")
            .or_else(|| statement.strip_prefix("use "))?;
        let (_, bound) = rest.strip_suffix(';')?.rsplit_once(" as ")?;
        (bound.trim() == name).then_some(statement)
    })
}

/// The offset of the `]` that closes the attribute `text` starts with.
fn matching_bracket(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '[' if !quoted => depth += 1,
            ']' if !quoted => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The shadow crate's entry module, with `attribute` as written.
#[cfg(feature = "rust-import")]
pub(crate) fn source(attribute: &str) -> String {
    format!(
        r#"
/// The program's entry under `rawr run` (D513.4): its own runtime attribute
/// around the Rawr `main`'s future.
#[doc(hidden)]
#[allow(unsafe_code)]
pub mod {MODULE} {{
    unsafe extern "Rust" {{
        fn __rawr_island_start_v1(entry: fn() -> i32);
        fn __rawr_island_main_v1()
        -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = i32> + Send>>;
    }}

    /// Register this entry; the runner starts it once Rawr's `main` gave
    /// its future.
    pub fn start() {{
        // SAFETY: the runner links rawr, which defines the symbol.
        unsafe {{ __rawr_island_start_v1(entry) }}
    }}

    fn entry() -> i32 {{
        main()
    }}

    {attribute}
    async fn main() -> i32 {{
        // SAFETY: as above.
        unsafe {{ __rawr_island_main_v1() }}.await
    }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::attribute;

    /// An attribute spread over lines is found whole (review 11).
    #[test]
    fn an_attribute_over_several_lines_is_found_whole() {
        let text =
            "// rawr\n#[tokio::main(\n    flavor = \"current_thread\",\n)]\nasync fn main() {}\n";
        assert_eq!(
            attribute(text).as_deref(),
            Some("#[tokio::main(\n    flavor = \"current_thread\",\n)]")
        );
    }

    /// A crate renamed by a `use` in the root keeps its name in the entry
    /// (review 12).
    #[cfg(feature = "rust-import")]
    #[test]
    fn a_renamed_runtime_crate_keeps_its_use() {
        let text = "// rawr\nuse tokio as rt;\n\n#[rt::main]\nasync fn main() {}\n";
        assert_eq!(
            attribute(text).as_deref(),
            Some("use tokio as rt;\n    #[rt::main]")
        );
        let source = super::source(&attribute(text).unwrap());
        assert!(
            source.contains("    use tokio as rt;\n    #[rt::main]\n    async fn main() -> i32 {"),
            "{source}"
        );
    }

    #[test]
    fn the_attribute_of_an_async_main_is_found_as_written() {
        let text = "// rawr\nuse tokio::time::sleep;\n\n#[tokio::main(flavor = \"current_thread\")]\nasync fn main() {\n}\n";
        assert_eq!(
            attribute(text).as_deref(),
            Some("#[tokio::main(flavor = \"current_thread\")]")
        );
        assert_eq!(
            attribute("#[tokio::main]\n// a note\npub async fn main() -> Result<(), String> {}\n")
                .as_deref(),
            Some("#[tokio::main]")
        );
        // A plain `main`, an attribute that names no runtime, or one that
        // another item separates from `main` is no entry.
        assert_eq!(attribute("#[tokio::main]\nfn main() {}\n"), None);
        assert_eq!(attribute("#[inline]\nasync fn main() {}\n"), None);
        assert_eq!(
            attribute("#[tokio::main]\nfn other() {}\nasync fn main() {}\n"),
            None
        );
    }
}
