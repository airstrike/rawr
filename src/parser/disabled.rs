//! The items a `#[cfg(..)]` leaves out of a file still parse as Rust.
//! rustc parses every item before it drops those a `cfg` does not hold, so
//! a syntax error in a `#[cfg(test)]` module stops `cargo run` too. Rawr
//! reads only the headers of the items it builds and passes over the rest,
//! so those items are parsed here, by `syn`, at their places in the file.

use crate::{source, ParseError, Position, Span, PERR};

/// The first syntax error in the items at `disabled` of `source`, at its
/// place in the file. `None` when they parse, or when there are none.
pub(super) fn error(source: &source::Map, disabled: Vec<Span>) -> Option<ParseError> {
    if disabled.is_empty() {
        return None;
    }
    let error = syn::parse_file(&source.only(disabled)).err()?;
    let at = |location: proc_macro2::LineColumn| {
        Position::new(
            u16::try_from(location.line).unwrap_or(u16::MAX),
            u16::try_from(location.column + 1).unwrap_or(u16::MAX),
        )
    };
    let (start, end) = (at(error.span().start()), at(error.span().end()));
    let message = error.to_string();
    Some(
        PERR::Unexpected {
            message: message.clone(),
            label: message,
            help: None,
            span: Span::new(start, end),
        }
        .into_err(start),
    )
}
