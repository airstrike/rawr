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
        let line = Lines::written(self.line);
        if self.is_none() {
            f.write_str("none")
        } else if self.is_beginning_of_line() {
            write!(f, "{line}")
        } else {
            write!(f, "{line}:{}", self.pos)
        }
    }
}

thread_local! {
    /// The lines this thread writes positions by (`Lines::writing`).
    static WRITING: std::cell::RefCell<Option<Lines>> = const { std::cell::RefCell::new(None) };
}

/// A text's lines, each by the number it is written as: what a digest
/// writes one source's positions by, so a save that moves them without
/// changing what they mean writes them the same (#1646).
#[derive(Clone, Debug, Default)]
pub(crate) struct Lines(std::sync::Arc<[u16]>);

impl Lines {
    /// Line `n` written as `numbers[n - 1]`; a line past them as itself.
    #[cfg(feature = "rust-import")]
    pub(crate) fn new(numbers: Vec<u16>) -> Self {
        Self(numbers.into())
    }

    /// Run `write` with each position this thread writes by `Debug`
    /// written at its line's number here.
    #[cfg(feature = "rust-import")]
    pub(crate) fn writing<T>(&self, write: impl FnOnce() -> T) -> T {
        /// Puts back the lines written by before, as `write` ends or
        /// unwinds.
        struct Restore(Option<Lines>);

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

    /// The number `line` is written as on this thread.
    fn written(line: u16) -> u16 {
        WRITING.with(|writing| {
            writing
                .borrow()
                .as_ref()
                .and_then(|lines| lines.0.get(usize::from(line).checked_sub(1)?).copied())
                .unwrap_or(line)
        })
    }

    /// Whether this thread writes positions by some lines.
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
        if Lines::active() {
            return write!(f, "{:?}-{:?}", self.start, self.end);
        }
        fmt::Display::fmt(self, f)
    }
}
