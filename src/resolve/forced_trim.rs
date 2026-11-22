//! rustc's forced-trimmed names (`with_forced_trimmed_paths`), which
//! E0308's label and E0271's message write: each type by its name alone,
//! `Range<u32>` where any other message writes `std::ops::Range<u32>`.

use std::borrow::Cow;

/// `text` with each path in it written by its last segment: a module's or
/// a crate's segment, which starts with a lowercase letter, goes, and a
/// type's stays, so `T::Output` keeps its parameter.
pub(crate) fn trimmed(text: &str) -> Cow<'_, str> {
    if !text.contains("::") {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let length = rest
            .find(|character: char| !(character.is_alphanumeric() || character == '_'))
            .unwrap_or(rest.len());
        let (word, after) = rest.split_at(length);
        let module =
            word.starts_with(|character: char| character.is_lowercase() || character == '_');
        match after.strip_prefix("::") {
            Some(next) if module && !word.is_empty() => rest = next,
            _ => {
                output.push_str(word);
                let mut characters = after.chars();
                if let Some(character) = characters.next() {
                    output.push(character);
                }
                rest = characters.as_str();
            }
        }
    }
    Cow::Owned(output)
}

/// `expected` and `found` as E0308's label names them: each `trimmed`,
/// unless that names both alike; then each as any message names it,
/// `PathBuf` beside `std::path::PathBuf`.
pub(crate) fn pair<'a>(expected: &'a str, found: &'a str) -> (Cow<'a, str>, Cow<'a, str>) {
    let (trimmed_expected, trimmed_found) = (trimmed(expected), trimmed(found));
    if trimmed_expected == trimmed_found {
        return (Cow::Borrowed(expected), Cow::Borrowed(found));
    }
    (trimmed_expected, trimmed_found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_trimmed_to_its_type_and_a_projection_keeps_its_base() {
        assert_eq!(
            trimmed("Option<std::ops::Range<{integer}>>"),
            "Option<Range<{integer}>>"
        );
        assert_eq!(
            trimmed("<T as std::iter::Iterator>::Item"),
            "<T as Iterator>::Item"
        );
        assert_eq!(trimmed("T::Output"), "T::Output");
        assert_eq!(
            pair("PathBuf", "std::path::PathBuf"),
            (
                Cow::Borrowed("PathBuf"),
                Cow::Borrowed("std::path::PathBuf")
            )
        );
    }
}
