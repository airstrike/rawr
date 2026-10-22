//! Admitted semantic TOML data, extracted from toml 1.1.2+spec-1.1.0.
//!
//! This module neither reads a file nor establishes EOF, freshness, package
//! membership, or source authority. The caller supplies its already admitted
//! complete UTF-8 observation and its existing cumulative account. See
//! `document/PROVENANCE.md` for the transformation and physical work bounds.

mod assemble;
mod dispose;
mod storage;
mod syntax;
mod table;

#[cfg(test)]
mod tests;

use core::cell::RefCell;
use core::ops::Range;

use crate::parser::{self, RecursionGuard, ValidateWhitespace};
use crate::{ParseError, Source};

use storage::{Buffer, Session, Text};
pub use table::{Iter, Table};
pub use toml_datetime::Datetime;

/// A prospective request against the caller's original cumulative account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Actual token, event, key, value, container slot, or first diagnostic.
    Items(usize),
    /// Full prospective decoded text length, including borrowed text.
    /// This is a per-string bound, not an allocation or cumulative byte charge.
    String(usize),
    /// Full requested replacement layout, before fallible allocation.
    /// Previously allocated bytes remain charged in the caller's account.
    Allocation { bytes: usize, alignment: usize },
}

/// Closed admission interface: it supplies no files, resolver, or capabilities.
pub trait Admission {
    type Error;

    fn admit(&mut self, request: Request) -> Result<(), Self::Error>;
}

/// Bounded diagnostic category; wording is deliberately not retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Syntax,
    Depth,
    DuplicateKey,
    ExtendWrongType,
    ExtendInlineTable,
    DateTime,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub tag: Tag,
    pub span: Option<Range<usize>>,
    pub context: Option<Range<usize>>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
    #[error("TOML admission refused: {0}")]
    Admission(E),
    #[error("TOML allocation failed")]
    Allocation,
    #[error("TOML storage layout overflowed")]
    Overflow,
    #[error("invalid TOML ({0:?})")]
    Invalid(Diagnostic),
}

/// A parsed document borrowing the exact input supplied to `parse`.
#[derive(Debug)]
pub struct Document<'i> {
    source: &'i str,
    table: Table<'i>,
}

impl<'i> Document<'i> {
    pub fn source(&self) -> &'i str {
        self.source
    }

    pub fn table(&self) -> &Table<'i> {
        &self.table
    }
}

#[derive(Debug)]
pub struct Spanned<T> {
    span: Range<usize>,
    value: T,
}

impl<T> Spanned<T> {
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    pub fn value(&self) -> &T {
        &self.value
    }
}

#[derive(Debug)]
pub struct Key<'i> {
    text: Text<'i>,
    span: Range<usize>,
}

impl Key<'_> {
    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }

    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }
}

#[derive(Debug)]
pub enum Value<'i> {
    String(String<'i>),
    Integer(Integer<'i>),
    Float(Float<'i>),
    Boolean(bool),
    DateTime(toml_datetime::Datetime),
    Table(Table<'i>),
    Array(Array<'i>),
}

#[derive(Debug)]
pub struct String<'i>(Text<'i>);

impl String<'_> {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Debug)]
pub struct Integer<'i> {
    spelling: Text<'i>,
    radix: u32,
}

impl Integer<'_> {
    pub fn as_str(&self) -> &str {
        self.spelling.as_str()
    }

    pub fn radix(&self) -> u32 {
        self.radix
    }

    /// Conversion remains deferred, as in pinned `DeInteger`.
    pub fn to_i64(&self) -> Result<i64, core::num::ParseIntError> {
        i64::from_str_radix(self.as_str(), self.radix)
    }
}

#[derive(Debug)]
pub struct Float<'i>(Text<'i>);

impl Float<'_> {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn to_f64(&self) -> Option<f64> {
        let value: f64 = self.as_str().parse().ok()?;
        if value.is_infinite() && !self.as_str().contains("inf") {
            None
        } else {
            Some(value)
        }
    }
}

#[derive(Debug, Default)]
pub struct Array<'i> {
    values: Buffer<Spanned<Value<'i>>>,
    array_of_tables: bool,
    disposal: usize,
}

impl<'i> Array<'i> {
    fn tables() -> Self {
        let mut array = Self::default();
        array.array_of_tables = true;
        array
    }

    pub fn values(&self) -> &[Spanned<Value<'i>>] {
        self.values.as_slice()
    }

    pub fn is_array_of_tables(&self) -> bool {
        self.array_of_tables
    }
}

impl Drop for Array<'_> {
    fn drop(&mut self) {
        dispose::array(self);
    }
}

/// Transform an admitted complete UTF-8 observation into semantic data.
///
/// All syntax, including ignored fields, is parsed. The pinned depth ceiling
/// of 80 is additionally restricted by `caller_depth`. Refusal is permanent;
/// raw recovery can continue scanning, but cannot turn failure into a document.
pub fn parse<'i, A: Admission>(
    input: &'i str,
    caller_depth: u32,
    admission: &mut A,
) -> Result<Document<'i>, Error<A::Error>> {
    let session = RefCell::new(Session::new(admission));
    let source = Source::new(input);
    let depth = caller_depth.min(80);
    // Lexing and event emission are deterministic, so count first and admit
    // each buffer once at its exact size instead of through repeated growth.
    let mut tokens = Buffer::new();
    if tokens.reserve_exact(source.lex().count(), &session) {
        for token in source.lex() {
            if !tokens.push(token, &session) {
                break;
            }
        }
    }
    let mut events = Buffer::new();
    if storage::live(&session) {
        let mut counter = syntax::Counter::default();
        let mut invalid = false;
        {
            let mut receiver = ValidateWhitespace::new(&mut counter, source);
            let mut receiver = RecursionGuard::new(&mut receiver, depth);
            parser::parse_document(tokens.as_slice(), &mut receiver, &mut |_: ParseError| {
                invalid = true;
            });
        }
        // An invalid document keeps incremental growth, so its diagnostic is
        // reported exactly where it was before rather than as an admission.
        if !invalid {
            events.reserve_exact(counter.0, &session);
        }
    }
    if storage::live(&session) {
        let mut receiver = syntax::Receiver {
            events: &mut events,
            session: &session,
        };
        let mut receiver = ValidateWhitespace::new(&mut receiver, source);
        let mut receiver = RecursionGuard::new(&mut receiver, depth);
        let mut errors = |error: ParseError| storage::raw_error(&session, error);
        parser::parse_document(tokens.as_slice(), &mut receiver, &mut errors);
    }
    let table = if storage::live(&session) {
        assemble::document(events.as_slice(), source, depth, &session)
    } else {
        None
    };
    if let Some(error) = session.into_inner().failure {
        return Err(error);
    }
    match table {
        Some(table) => Ok(Document {
            source: input,
            table,
        }),
        None => Err(Error::Invalid(Diagnostic {
            tag: Tag::Internal,
            span: None,
            context: None,
        })),
    }
}
