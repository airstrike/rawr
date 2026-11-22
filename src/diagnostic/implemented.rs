//! rustc's help on an E0277 for a std type that lacks an impl of a trait
//! for some argument: the impls of the trait the type has, "`String`
//! implements trait `From<T>`:", or, where their self types differ, "the
//! following other types implement trait `From<T>`:".

use crate::std_table;

/// How many impls rustc lists as text: with fewer, it marks each in std's
/// source, which the table doesn't locate; with more, it lists eight and
/// counts the rest, which the table can't count, since it holds only the
/// impls stable Rust can name (`Vec<u8>`'s of unstable types are left
/// out).
const LISTED: std::ops::RangeInclusive<usize> = 5..=9;

/// rustc's help for `owner`, a std type as a message names it (`String`,
/// `Vec<i32>`), which implements the std trait `trait_definition`
/// (`core::convert::From`) for other arguments only: each of its impls,
/// as rustc names their types, sorted. `None` for a type std doesn't
/// define, or one whose impls rustc doesn't list as text (`LISTED`).
pub(super) fn help(owner: &str, trait_definition: &str) -> Option<String> {
    let head = owner.split('<').next().unwrap_or(owner);
    let definition = std_table::definition_of(head).or_else(|| std_table::sole_type(head))?;
    let trait_name = trait_definition
        .rsplit("::")
        .next()
        .unwrap_or(trait_definition);
    let parameters: Vec<&str> = std_table::item(trait_definition)?
        .generics()
        .map(|parameter| parameter.name)
        .collect();
    let generic = format!("{trait_name}<{}>", parameters.join(", "));
    let impls: Vec<(String, String)> = std_table::impls(trait_definition, definition)
        .map(|found| {
            let arguments: Vec<String> = found.arguments().into_iter().map(written).collect();
            (
                written(found.self_ty),
                format!("{trait_name}<{}>", arguments.join(", ")),
            )
        })
        .collect();
    if !LISTED.contains(&impls.len()) {
        return None;
    }
    let first = impls.first()?;
    let alike = impls.iter().all(|(self_ty, _)| *self_ty == first.0);
    let (header, mut lines) = if alike {
        (
            format!("`{}` implements trait `{generic}`:", first.0),
            impls
                .iter()
                .map(|(_, implemented)| implemented.clone())
                .collect::<Vec<_>>(),
        )
    } else {
        (
            format!("the following other types implement trait `{generic}`:"),
            impls
                .iter()
                .map(|(self_ty, implemented)| format!("`{self_ty}` implements `{implemented}`"))
                .collect(),
        )
    };
    lines.sort();
    let mut text = header;
    for line in &lines {
        text.push_str("\n  ");
        text.push_str(line);
    }
    Some(text)
}

/// The type std's table writes as `text`, as rustc names it in the list:
/// each path by its name alone, and each lifetime the table leaves out
/// as `'_`, `Cow<'_, str>` of `std::borrow::Cow<str>`.
fn written(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let length = rest
            .find(|character: char| {
                !(character.is_alphanumeric() || matches!(character, '_' | ':'))
            })
            .unwrap_or(rest.len());
        let (path, after) = rest.split_at(length);
        if path.is_empty() {
            let mut characters = after.chars();
            output.extend(characters.next());
            rest = characters.as_str();
            continue;
        }
        output.push_str(&crate::resolve::forced_trim::trimmed(path));
        let lifetimes = std_table::definition_of(path)
            .and_then(std_table::item)
            .map_or(0, |item| {
                item.generics()
                    .filter(|parameter| parameter.kind == std_table::ParameterKind::Lifetime)
                    .count()
            });
        let elided = vec!["'_"; lifetimes].join(", ");
        rest = match after.strip_prefix('<') {
            Some(arguments) if lifetimes > 0 && !arguments.starts_with('\'') => {
                output.push_str(&format!("<{elided}, "));
                arguments
            }
            None if lifetimes > 0 => {
                output.push_str(&format!("<{elided}>"));
                after
            }
            _ => after,
        };
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_type_is_written_with_its_elided_lifetimes() {
        assert_eq!(written("std::borrow::Cow<str>"), "Cow<'_, str>");
        assert_eq!(written("&[T; N]"), "&[T; N]");
        assert_eq!(
            written("std::collections::BinaryHeap<T, A>"),
            "BinaryHeap<T, A>"
        );
    }
}
