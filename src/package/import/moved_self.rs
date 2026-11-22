//! rustc's note at the `self` of a crate method that moved a value as its
//! receiver, in the crate's source.

use crate::diagnostic;
use crate::{Position, Span};

/// rustc's note for `moved`, a local a crate method took as its owned
/// `self`: "`Type::method` takes ownership of the receiver `self`, which
/// moves `moved`", at `self` in `declaration`, the method's declaration in
/// the crate's file, read from disk. `method` is the method's path. `None`
/// when the file or its `self` can't be read.
pub(super) fn note(
    method: &str,
    moved: &str,
    declaration: &diagnostic::ExternalPosition,
) -> Option<diagnostic::Section> {
    let text = std::fs::read_to_string(declaration.file()).ok()?;
    let start = usize::try_from(declaration.byte_range().start).ok()?;
    let at = start + receiver(text.get(start..)?)?;
    let before = &text[..at];
    let line = u16::try_from(before.matches('\n').count() + 1).ok()?;
    let column = before[before.rfind('\n').map_or(0, |newline| newline + 1)..]
        .chars()
        .count();
    let column = u16::try_from(column + 1).ok()?;
    let span = Span::new(Position::new(line, column), Position::new(line, column + 4));
    let source = diagnostic::Source::new(declaration.file(), text).ok()?;
    let mut segments = method.rsplit("::");
    let name = segments.next()?;
    let named = match segments.next() {
        Some(owner) => format!("{owner}::{name}"),
        None => name.to_owned(),
    };
    Some(
        diagnostic::Section::note(format!(
            "`{named}` takes ownership of the receiver `self`, which moves `{moved}`"
        ))
        .primary(source, span, None),
    )
}

/// Where the `self` of the method declared at the start of `declared` is:
/// the first parameter, after its `mut`.
fn receiver(declared: &str) -> Option<usize> {
    let open = declared.find('(')? + 1;
    let parameters = &declared[open..];
    let first = parameters.len() - parameters.trim_start().len();
    let parameter = &parameters[first..];
    let after_mut = parameter
        .strip_prefix("mut")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map_or(0, |rest| parameter.len() - rest.trim_start().len());
    let named = &parameter[after_mut..];
    let ends = named
        .get(4..)
        .is_some_and(|rest| !rest.starts_with(|next: char| next.is_alphanumeric() || next == '_'));
    (named.starts_with("self") && ends).then_some(open + first + after_mut)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_receiver_is_the_method_s_owned_self() {
        let declared = "pub fn owned(self) -> u8 { 1 }";
        assert_eq!(super::receiver(declared), declared.find("self"));
        let declared = "fn into( mut self, other: u8) {}";
        assert_eq!(super::receiver(declared), declared.find("self"));
        assert_eq!(super::receiver("fn shared(&self) {}"), None);
        assert_eq!(super::receiver("fn other(selfish: u8) {}"), None);
    }
}
