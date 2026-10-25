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
/// `#[async_std::main]`, `#[tokio::main(flavor = "current_thread")]`).
pub(crate) fn attribute(text: &str) -> Option<&str> {
    let mut attribute = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && trimmed.starts_with("#[") {
            let end = matching_bracket(&text[offset..]).map(|end| offset + end + 1);
            if let Some(end) = end
                && let Some(path) = text[offset + 2..end - 1].split(['(', ']', ' ']).next()
                && path.contains("::")
                && path.rsplit("::").next() == Some("main")
            {
                attribute = Some(offset..end);
            }
        } else if indent == 0 {
            let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
            if let Some(rest) = rest.strip_prefix("async fn main")
                && rest.trim_start().starts_with('(')
            {
                return attribute.map(|range| &text[range]);
            }
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                attribute = None;
            }
        }
        offset += line.len();
    }
    None
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

    #[test]
    fn the_attribute_of_an_async_main_is_found_as_written() {
        let text = "// rawr\nuse tokio::time::sleep;\n\n#[tokio::main(flavor = \"current_thread\")]\nasync fn main() {\n}\n";
        assert_eq!(
            attribute(text),
            Some("#[tokio::main(flavor = \"current_thread\")]")
        );
        assert_eq!(
            attribute("#[tokio::main]\n// a note\npub async fn main() -> Result<(), String> {}\n"),
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
