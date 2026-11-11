//! Script character position type.

use std::{
    fmt,
    ops::{Add, AddAssign},
};

/// A location (line number + character position) in the input script.
///
/// # Limitations
///
/// In order to keep footprint small, both line number and character position have 16-bit resolution,
/// meaning they go up to a maximum of 65,535 lines and 65,535 characters per line.
///
/// Advancing beyond the maximum line length or maximum number of lines is not an error but has no effect.
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Copy)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub struct Position {
    /// Line number: 0 = none
    line: u16,
    /// Character position: 0 = BOL
    pos: u16,
}

impl Position {
    /// A [`Position`] representing no position.
    pub const NONE: Self = Self { line: 0, pos: 0 };
    /// A [`Position`] representing the first position.
    pub const START: Self = Self { line: 1, pos: 0 };

    /// Create a new [`Position`].
    ///
    /// `line` must not be zero.
    ///
    /// If `position` is zero, then it is at the beginning of a line.
    ///
    /// # Panics
    ///
    /// Panics if `line` is zero.
    #[inline]
    #[must_use]
    pub const fn new(line: u16, position: u16) -> Self {
        assert!(line != 0, "line cannot be zero");

        let _pos = position;

        Self { line, pos: _pos }
    }
    /// Get the line number (1-based), or [`None`] if there is no position.
    #[inline]
    #[must_use]
    pub const fn line(self) -> Option<usize> {
        if self.is_none() {
            None
        } else {
            Some(self.line as usize)
        }
    }
    /// Get the character position (1-based), or [`None`] if at beginning of a line.
    #[inline]
    #[must_use]
    pub const fn position(self) -> Option<usize> {
        if self.is_none() || self.pos == 0 {
            None
        } else {
            Some(self.pos as usize)
        }
    }
    /// Advance by one character position.
    #[inline]
    pub(crate) fn advance(&mut self) {
        assert!(!self.is_none(), "cannot advance Position::NONE");
        self.pos = self.pos.saturating_add(1);
    }
    /// Go backwards by one character position.
    ///
    /// # Panics
    ///
    /// Panics if already at beginning of a line - cannot rewind to a previous line.
    #[inline]
    pub(crate) fn rewind(&mut self) {
        assert!(!self.is_none(), "cannot rewind Position::NONE");
        assert!(self.pos > 0, "cannot rewind at position 0");
        self.pos = self.pos.saturating_sub(1);
    }
    /// Advance to the next line.
    #[inline]
    pub(crate) fn new_line(&mut self) {
        assert!(!self.is_none(), "cannot advance Position::NONE");

        // Advance up to maximum position
        self.line = self.line.saturating_add(1);
        self.pos = 0;
    }
    /// Is this [`Position`] at the beginning of a line?
    #[inline]
    #[must_use]
    pub const fn is_beginning_of_line(self) -> bool {
        self.pos == 0 && !self.is_none()
    }
    /// Is there no [`Position`]?
    #[inline]
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.line == 0 && self.pos == 0
    }
    /// Returns an fallback [`Position`] if it is [`NONE`][Position::NONE]?
    #[inline]
    #[must_use]
    pub const fn or_else(self, pos: Self) -> Self {
        if self.is_none() {
            pos
        } else {
            self
        }
    }
}

impl Default for Position {
    #[inline(always)]
    fn default() -> Self {
        Self::START
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            write!(f, "none")
        } else {
            write!(f, "line {}, position {}", self.line, self.pos)
        }
    }
}

impl fmt::Debug for Position {
    #[cold]
    #[inline(never)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return f.write_str("none");
        }
        if let Some(written) = Places::written(*self) {
            return write!(f, "{written}");
        }
        let line = self.line;
        if self.is_beginning_of_line() {
            write!(f, "{line}")
        } else {
            write!(f, "{line}:{}", self.pos)
        }
    }
}

thread_local! {
    /// The places this thread writes positions by (`Places::writing`).
    static WRITING: std::cell::RefCell<Option<Places>> = const { std::cell::RefCell::new(None) };
}

/// Where a text's tokens start, in order: what a digest writes one
/// source's positions by. A position is written as how many of them start
/// before it, so a save that moves them without changing them (a line
/// added between them, a signature written over more lines, a token left
/// out of them) writes them the same (#1646).
#[derive(Clone, Debug)]
pub(crate) struct Places(std::sync::Arc<[Position]>);

impl Places {
    /// The places of tokens that start at `starts`, in order.
    #[cfg(feature = "rust-import")]
    pub(crate) fn new(starts: Vec<Position>) -> Self {
        Self(starts.into())
    }

    /// Those of these places from `start` through `end`: what a digest writes one
    /// declaration's positions by, so a declaration a save adds or removes
    /// before it in its text writes it the same (#1718).
    #[cfg(feature = "rust-import")]
    pub(crate) fn between(&self, start: Position, end: Position) -> Self {
        let from = self.0.partition_point(|place| *place < start);
        let to = self.0.partition_point(|place| *place <= end).max(from);
        Self(self.0[from..to].into())
    }

    /// Run `write` with each position this thread writes by `Debug`
    /// written as how many of these places come before it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn writing<T>(&self, write: impl FnOnce() -> T) -> T {
        /// Puts back the places written by before, as `write` ends or
        /// unwinds.
        struct Restore(Option<Places>);

        impl Drop for Restore {
            fn drop(&mut self) {
                let previous = self.0.take();
                WRITING.with(|writing| *writing.borrow_mut() = previous);
            }
        }

        let previous = WRITING.with(|writing| writing.borrow_mut().replace(self.clone()));
        let _restore = Restore(previous);
        write()
    }

    /// What `position` is written as on this thread: `None` when it writes
    /// positions as they are.
    fn written(position: Position) -> Option<usize> {
        WRITING.with(|writing| {
            let writing = writing.borrow();
            let places = writing.as_ref()?;
            Some(places.0.partition_point(|start| *start < position))
        })
    }

    /// Whether this thread writes positions by some places.
    fn active() -> bool {
        WRITING.with(|writing| writing.borrow().is_some())
    }
}

impl Add for Position {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        if rhs.is_none() {
            self
        } else {
            Self {
                line: self.line + rhs.line - 1,
                pos: if rhs.is_beginning_of_line() {
                    self.pos
                } else {
                    self.pos + rhs.pos - 1
                },
            }
        }
    }
}

impl AddAssign for Position {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

/// A span consisting of starting and ending [positions][Position].
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Copy)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub struct Span {
    /// Starting [position][Position].
    start: Position,
    /// Ending [position][Position].
    end: Position,
}

impl Default for Span {
    #[inline(always)]
    fn default() -> Self {
        Self::NONE
    }
}

impl Span {
    /// Empty [`Span`].
    pub const NONE: Self = Self::new(Position::NONE, Position::NONE);

    /// Create a new [`Span`].
    #[inline(always)]
    #[must_use]
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
    /// Is this [`Span`] non-existent?
    #[inline(always)]
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.start.is_none() && self.end.is_none()
    }
    /// Get the [`Span`]'s starting [position][Position].
    #[inline(always)]
    #[must_use]
    pub const fn start(self) -> Position {
        self.start
    }
    /// Get the [`Span`]'s ending [position][Position].
    #[inline(always)]
    #[must_use]
    pub const fn end(self) -> Position {
        self.end
    }
    /// The text of `source` this span covers. Positions are lines and
    /// 1-based character columns, the end exclusive.
    #[must_use]
    pub(crate) fn text(self, source: &str) -> &str {
        fn offset(text: &str, position: Position) -> usize {
            let (Some(line), Some(column)) = (position.line(), position.position()) else {
                return text.len();
            };
            let mut start = 0;
            for _ in 1..line {
                match text[start..].find('\n') {
                    Some(at) => start += at + 1,
                    None => return text.len(),
                }
            }
            text[start..]
                .char_indices()
                .nth(column.saturating_sub(1))
                .map_or(text.len(), |(at, _)| start + at)
        }
        let start = offset(source, self.start);
        let end = offset(source, self.end).max(start);
        &source[start..end]
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _f = f;

        match (self.start(), self.end()) {
            (Position::NONE, Position::NONE) => write!(_f, "{:?}", Position::NONE),
            (Position::NONE, end) => write!(_f, "..{end:?}"),
            (start, Position::NONE) => write!(_f, "{start:?}"),
            (start, end) if start.line() != end.line() => {
                write!(_f, "{start:?}-{end:?}")
            }
            (start, end) => write!(
                _f,
                "{}:{}-{}",
                start.line().unwrap(),
                start.position().unwrap_or(0),
                end.position().unwrap_or(0)
            ),
        }
    }
}

impl fmt::Debug for Span {
    #[cold]
    #[inline(never)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if Places::active() {
            return write!(f, "{:?}-{:?}", self.start, self.end);
        }
        fmt::Display::fmt(self, f)
    }
}
