//! Semver 1.0.28 parsing with borrowed identifiers and pinned total ordering.

use std::cmp::Ordering;
use std::fmt;

use super::Error;

mod diagnostic;

/// A complete version, preserving prerelease and build identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Version<'text> {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Pre<'text>,
    build: Build<'text>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Pre<'text>(&'text str);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Build<'text>(&'text str);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Position {
    Major,
    Minor,
    Patch,
    Pre,
    Build,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Empty,
    UnexpectedEnd(Position),
    UnexpectedChar(Position, char),
    UnexpectedCharAfter(Position, char),
    ExpectedCommaFound(Position, char),
    LeadingZero(Position),
    Overflow(Position),
    EmptySegment(Position),
    WildcardNotTheOnlyComparator(char),
    UnexpectedAfterWildcard,
    ExcessiveComparators,
}

impl PartialOrd for Pre<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pre<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_pre(self.0, other.0)
    }
}

impl PartialOrd for Build<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Build<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_build(self.0, other.0)
    }
}

pub(in super::super) fn compare_pre(left: &str, right: &str) -> Ordering {
    if left == right {
        return Ordering::Equal;
    }
    if left.is_empty() {
        return Ordering::Greater;
    }
    if right.is_empty() {
        return Ordering::Less;
    }
    identifiers(left, right, false)
}

pub(in super::super) fn compare_build(left: &str, right: &str) -> Ordering {
    if left == right {
        Ordering::Equal
    } else {
        identifiers(left, right, true)
    }
}

impl fmt::Display for Version<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.0.is_empty() {
            write!(formatter, "-{}", self.pre.0)?;
        }
        if !self.build.0.is_empty() {
            write!(formatter, "+{}", self.build.0)?;
        }
        Ok(())
    }
}

impl<'text> Version<'text> {
    /// Parse without trimming; package-version trimming belongs to its visitor.
    pub(crate) fn parse(text: &'text str) -> Result<Self, Error> {
        parse(text).map_err(Error::Version)
    }

    pub(crate) fn major(&self) -> u64 {
        self.major
    }

    pub(crate) fn minor(&self) -> u64 {
        self.minor
    }

    pub(crate) fn patch(&self) -> u64 {
        self.patch
    }

    pub(crate) fn pre(&self) -> &'text str {
        self.pre.0
    }

    pub(crate) fn build(&self) -> &'text str {
        self.build.0
    }

    /// SemVer precedence excludes build metadata; `Ord` retains its tie-break.
    pub(crate) fn cmp_precedence(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch, self.pre).cmp(&(
            other.major,
            other.minor,
            other.patch,
            other.pre,
        ))
    }

    pub(super) fn from_partial(value: super::Partial<'text>) -> Option<Self> {
        Some(Self {
            major: value.major(),
            minor: value.minor()?,
            patch: value.patch()?,
            pre: Pre(value.pre().unwrap_or_default()),
            build: Build(value.build().unwrap_or_default()),
        })
    }
}

impl<'text> Pre<'text> {
    pub(super) fn from_version(value: Version<'text>) -> Option<Self> {
        (!value.pre.0.is_empty()).then_some(value.pre)
    }

    pub(super) fn as_str(&self) -> &'text str {
        self.0
    }

    pub(super) fn nonempty(text: &'text str) -> Self {
        Self(text)
    }
}

impl<'text> Build<'text> {
    pub(super) fn from_version(value: Version<'text>) -> Option<Self> {
        (!value.build.0.is_empty()).then_some(value.build)
    }

    pub(super) fn as_str(&self) -> &'text str {
        self.0
    }
}

pub(super) fn parse(text: &str) -> Result<Version<'_>, Rejection> {
    if text.is_empty() {
        return Err(Rejection::Empty);
    }
    let mut position = Position::Major;
    let (major, text) = numeric(text, position)?;
    let text = dot(text, position)?;
    position = Position::Minor;
    let (minor, text) = numeric(text, position)?;
    let text = dot(text, position)?;
    position = Position::Patch;
    let (patch, text) = numeric(text, position)?;
    let (pre, text) = if let Some(text) = text.strip_prefix('-') {
        position = Position::Pre;
        let (pre, text) = identifier(text, position)?;
        if pre.is_empty() {
            return Err(Rejection::EmptySegment(position));
        }
        (pre, text)
    } else {
        ("", text)
    };
    let (build, text) = if let Some(text) = text.strip_prefix('+') {
        position = Position::Build;
        let (build, text) = identifier(text, position)?;
        if build.is_empty() {
            return Err(Rejection::EmptySegment(position));
        }
        (build, text)
    } else {
        ("", text)
    };
    if let Some(unexpected) = text.chars().next() {
        return Err(Rejection::UnexpectedCharAfter(position, unexpected));
    }
    Ok(Version {
        major,
        minor,
        patch,
        pre: Pre(pre),
        build: Build(build),
    })
}

pub(super) fn numeric(input: &str, position: Position) -> Result<(u64, &str), Rejection> {
    let mut length = 0;
    let mut value = 0_u64;
    while let Some(&digit) = input.as_bytes().get(length) {
        if !digit.is_ascii_digit() {
            break;
        }
        if value == 0 && length > 0 {
            return Err(Rejection::LeadingZero(position));
        }
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit - b'0')))
            .ok_or(Rejection::Overflow(position))?;
        length += 1;
    }
    if length > 0 {
        Ok((value, &input[length..]))
    } else if let Some(unexpected) = input.chars().next() {
        Err(Rejection::UnexpectedChar(position, unexpected))
    } else {
        Err(Rejection::UnexpectedEnd(position))
    }
}

fn dot(input: &str, position: Position) -> Result<&str, Rejection> {
    if let Some(rest) = input.strip_prefix('.') {
        Ok(rest)
    } else if let Some(unexpected) = input.chars().next() {
        Err(Rejection::UnexpectedCharAfter(position, unexpected))
    } else {
        Err(Rejection::UnexpectedEnd(position))
    }
}

pub(super) fn identifier(input: &str, position: Position) -> Result<(&str, &str), Rejection> {
    let mut accumulated = 0;
    let mut segment = 0;
    let mut nondigit = false;
    loop {
        // Both lengths advance only across a byte returned by this slice.
        match input.as_bytes().get(accumulated + segment) {
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'-') => {
                segment += 1;
                nondigit = true;
            }
            Some(b'0'..=b'9') => segment += 1,
            boundary => {
                if segment == 0 {
                    return if accumulated == 0 && boundary != Some(&b'.') {
                        Ok(("", input))
                    } else {
                        Err(Rejection::EmptySegment(position))
                    };
                }
                if position == Position::Pre
                    && segment > 1
                    && !nondigit
                    && input[accumulated..].starts_with('0')
                {
                    return Err(Rejection::LeadingZero(position));
                }
                accumulated += segment;
                if boundary == Some(&b'.') {
                    accumulated += 1;
                    segment = 0;
                    nondigit = false;
                } else {
                    return Ok(input.split_at(accumulated));
                }
            }
        }
    }
}

fn identifiers(left: &str, right: &str, build: bool) -> Ordering {
    let mut right = right.split('.');
    for left in left.split('.') {
        let Some(right) = right.next() else {
            return Ordering::Greater;
        };
        let order = match (
            left.bytes().all(|byte| byte.is_ascii_digit()),
            right.bytes().all(|byte| byte.is_ascii_digit()),
        ) {
            (true, true) if build => {
                let left_value = left.trim_start_matches('0');
                let right_value = right.trim_start_matches('0');
                left_value
                    .len()
                    .cmp(&right_value.len())
                    .then_with(|| left_value.cmp(right_value))
                    .then_with(|| left.len().cmp(&right.len()))
            }
            (true, true) => left.len().cmp(&right.len()).then_with(|| left.cmp(right)),
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => left.cmp(right),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    if right.next().is_none() {
        Ordering::Equal
    } else {
        Ordering::Less
    }
}
