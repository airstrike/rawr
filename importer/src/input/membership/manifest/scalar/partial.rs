//! Cargo PartialVersion's full-version trial and complete requirement fallback.

use std::fmt;

use super::{version, Error, Version};

/// A validated partial version with the pinned presence and comparison rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Partial<'text> {
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Option<version::Pre<'text>>,
    build: Option<version::Build<'text>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    VersionReq,
    Prerelease,
    BuildMetadata,
    Unexpected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Op {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    Tilde,
    Caret,
    Wildcard,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Comparator<'text> {
    pub(super) op: Op,
    pub(super) major: u64,
    pub(super) minor: Option<u64>,
    pub(super) patch: Option<u64>,
    pub(super) pre: &'text str,
}

/// The complete requirement is visited; only PartialVersion's projection remains.
#[derive(Debug)]
struct Requirement<'text> {
    count: usize,
    first: Option<Comparator<'text>>,
}

impl fmt::Display for Partial<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.major)?;
        if let Some(minor) = self.minor {
            write!(formatter, ".{minor}")?;
        }
        if let Some(patch) = self.patch {
            write!(formatter, ".{patch}")?;
        }
        if let Some(pre) = self.pre {
            write!(formatter, "-{}", pre.as_str())?;
        }
        if let Some(build) = self.build {
            write!(formatter, "+{}", build.as_str())?;
        }
        Ok(())
    }
}

impl<'text> Partial<'text> {
    pub(crate) fn parse(text: &'text str) -> Result<Self, Error> {
        parse(text).map_err(Error::Partial)
    }

    pub(crate) fn major(&self) -> u64 {
        self.major
    }

    pub(crate) fn minor(&self) -> Option<u64> {
        self.minor
    }

    pub(crate) fn patch(&self) -> Option<u64> {
        self.patch
    }

    pub(crate) fn pre(&self) -> Option<&'text str> {
        self.pre.map(|pre| pre.as_str())
    }

    pub(crate) fn build(&self) -> Option<&'text str> {
        self.build.map(|build| build.as_str())
    }

    pub(crate) fn to_version(self) -> Option<Version<'text>> {
        Version::from_partial(self)
    }

    /// An absent build field is unconstrained, unlike exact owner equality.
    pub(crate) fn matches(&self, value: &Version<'_>) -> bool {
        if !value.pre().is_empty() && self.pre.is_none() {
            return false;
        }
        self.major == value.major()
            && self.minor.is_none_or(|minor| minor == value.minor())
            && self.patch.is_none_or(|patch| patch == value.patch())
            && self.pre.is_none_or(|pre| pre.as_str() == value.pre())
            && self
                .build
                .is_none_or(|build| build.as_str() == value.build())
    }
}

pub(super) fn parse(text: &str) -> Result<Partial<'_>, Rejection> {
    if let Ok(value) = version::parse(text) {
        return Ok(Partial {
            major: value.major(),
            minor: Some(value.minor()),
            patch: Some(value.patch()),
            pre: version::Pre::from_version(value),
            build: version::Build::from_version(value),
        });
    }
    let requirement = match requirement(text) {
        Ok(requirement) => requirement,
        Err(_) if text.contains('-') => return Err(Rejection::Prerelease),
        Err(_) if text.contains('+') => return Err(Rejection::BuildMetadata),
        Err(_) => return Err(Rejection::Unexpected),
    };
    if requirement.count != 1 {
        return Err(Rejection::VersionReq);
    }
    let Some(comparator) = requirement.first else {
        return Err(Rejection::VersionReq);
    };
    if comparator.op != Op::Caret || text.starts_with('^') {
        return Err(Rejection::VersionReq);
    }
    Ok(Partial {
        major: comparator.major,
        minor: comparator.minor,
        patch: comparator.patch,
        pre: (!comparator.pre.is_empty()).then_some(version::Pre::nonempty(comparator.pre)),
        build: None,
    })
}

pub(super) enum Event<'a> {
    Pre(&'a str),
    Build(&'a str),
    Comparator(Comparator<'a>),
}

pub(super) enum Failure<E> {
    Grammar(version::Rejection),
    Capture(E),
}
impl<E> From<version::Rejection> for Failure<E> {
    fn from(error: version::Rejection) -> Self {
        Self::Grammar(error)
    }
}

fn requirement(text: &str) -> Result<Requirement<'_>, version::Rejection> {
    let mut result = Requirement {
        count: 0,
        first: None,
    };
    match capture(text, |event| {
        if let Event::Comparator(comparator) = event {
            result.count += 1;
            if result.first.is_none() {
                result.first = Some(comparator);
            }
        }
        Ok::<_, std::convert::Infallible>(())
    }) {
        Ok(()) => Ok(result),
        Err(Failure::Grammar(error)) => Err(error),
        Err(Failure::Capture(never)) => match never {},
    }
}

pub(super) fn capture<'a, E>(
    text: &'a str,
    mut emit: impl FnMut(Event<'a>) -> Result<(), E>,
) -> Result<(), Failure<E>> {
    let mut text = text.trim_start_matches(' ');
    if let Some((character, rest)) = wildcard(text) {
        let rest = rest.trim_start_matches(' ');
        return if rest.is_empty() {
            Ok(())
        } else if rest.starts_with(',') {
            Err(Failure::Grammar(
                version::Rejection::WildcardNotTheOnlyComparator(character),
            ))
        } else {
            Err(Failure::Grammar(
                version::Rejection::UnexpectedAfterWildcard,
            ))
        };
    }
    let mut count = 0;
    loop {
        let (comparator, position, rest) = match comparator(text, &mut emit) {
            Ok(value) => value,
            Err(Failure::Grammar(mut error)) => {
                if let Some((character, rest)) = wildcard(text) {
                    let rest = rest.trim_start_matches(' ');
                    if rest.is_empty() || rest.starts_with(',') {
                        error = version::Rejection::WildcardNotTheOnlyComparator(character);
                    }
                }
                return Err(Failure::Grammar(error));
            }
            Err(error) => return Err(error),
        };
        count += 1;
        emit(Event::Comparator(comparator)).map_err(Failure::Capture)?;
        if rest.is_empty() {
            return Ok(());
        }
        text = if let Some(rest) = rest.strip_prefix(',') {
            rest.trim_start_matches(' ')
        } else if let Some(unexpected) = rest.chars().next() {
            return Err(Failure::Grammar(version::Rejection::ExpectedCommaFound(
                position, unexpected,
            )));
        } else {
            return Ok(());
        };
        // This is the pinned parser's grammar guard, not a new resource cap.
        if count == 32 {
            return Err(Failure::Grammar(version::Rejection::ExcessiveComparators));
        }
    }
}

fn wildcard(input: &str) -> Option<(char, &str)> {
    if let Some(rest) = input.strip_prefix('*') {
        Some(('*', rest))
    } else if let Some(rest) = input.strip_prefix('x') {
        Some(('x', rest))
    } else {
        input.strip_prefix('X').map(|rest| ('X', rest))
    }
}

fn op(input: &str) -> (Op, &str) {
    if let Some(rest) = input.strip_prefix('=') {
        (Op::Exact, rest)
    } else if let Some(rest) = input.strip_prefix('>') {
        if let Some(rest) = rest.strip_prefix('=') {
            (Op::GreaterEq, rest)
        } else {
            (Op::Greater, rest)
        }
    } else if let Some(rest) = input.strip_prefix('<') {
        if let Some(rest) = rest.strip_prefix('=') {
            (Op::LessEq, rest)
        } else {
            (Op::Less, rest)
        }
    } else if let Some(rest) = input.strip_prefix('~') {
        (Op::Tilde, rest)
    } else if let Some(rest) = input.strip_prefix('^') {
        (Op::Caret, rest)
    } else {
        (Op::Caret, input)
    }
}

fn comparator<'a, E>(
    input: &'a str,
    emit: &mut impl FnMut(Event<'a>) -> Result<(), E>,
) -> Result<(Comparator<'a>, version::Position, &'a str), Failure<E>> {
    let (mut op, text) = op(input);
    let default_op = input.len() == text.len();
    let text = text.trim_start_matches(' ');
    let mut position = version::Position::Major;
    let (major, text) = version::numeric(text, position)?;
    let mut has_wildcard = false;
    let (minor, text) = if let Some(text) = text.strip_prefix('.') {
        position = version::Position::Minor;
        if let Some((_, text)) = wildcard(text) {
            has_wildcard = true;
            if default_op {
                op = Op::Wildcard;
            }
            (None, text)
        } else {
            let (minor, text) = version::numeric(text, position)?;
            (Some(minor), text)
        }
    } else {
        (None, text)
    };
    let (patch, text) = if let Some(text) = text.strip_prefix('.') {
        position = version::Position::Patch;
        if let Some((_, text)) = wildcard(text) {
            if default_op {
                op = Op::Wildcard;
            }
            (None, text)
        } else if has_wildcard {
            return Err(Failure::Grammar(
                version::Rejection::UnexpectedAfterWildcard,
            ));
        } else {
            let (patch, text) = version::numeric(text, position)?;
            (Some(patch), text)
        }
    } else {
        (None, text)
    };
    let (pre, text) = if patch.is_some() && text.starts_with('-') {
        position = version::Position::Pre;
        let (pre, text) = version::identifier(&text[1..], position)?;
        if pre.is_empty() {
            return Err(Failure::Grammar(version::Rejection::EmptySegment(position)));
        }
        emit(Event::Pre(pre)).map_err(Failure::Capture)?;
        (pre, text)
    } else {
        ("", text)
    };
    let text = if patch.is_some() && text.starts_with('+') {
        position = version::Position::Build;
        let (build, text) = version::identifier(&text[1..], position)?;
        if build.is_empty() {
            return Err(Failure::Grammar(version::Rejection::EmptySegment(position)));
        }
        emit(Event::Build(build)).map_err(Failure::Capture)?;
        text
    } else {
        text
    };
    let text = text.trim_start_matches(' ');
    Ok((
        Comparator {
            op,
            major,
            minor,
            patch,
            pre,
        },
        position,
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::requirement;

    #[test]
    fn requirement_capture_matches_pinned_semver() {
        let mut cases = vec![
            "",
            "*",
            "x",
            "X",
            " * ",
            "*,1",
            "1,*",
            "1,x,2",
            "^1.*",
            "1.*",
            "1.x.2",
            "1.2.*",
            "1",
            "1.2",
            "  ^1.2.3 ",
            "1.2.3+abc",
            "1.2.3-01",
            "1.2.3-a.0",
            "1.2.3+",
            "1.2.3+01",
            "1.2.3+abc, >=2",
            "1.2.3-abc,foo",
            "1.2.3\t",
            ">1,<=2",
            "~1.2",
            "1.2.3 , 2.0.0",
            "1.2.3,",
            "1.2.3|2",
            "1.2.3+é",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        cases.push(vec!["1"; 32].join(","));
        cases.push(vec!["1"; 33].join(","));
        cases.push(format!("{},-", vec!["1"; 32].join(",")));
        for text in cases {
            let actual = requirement(&text);
            let expected = semver::VersionReq::parse(&text);
            assert_eq!(
                actual.is_ok(),
                expected.is_ok(),
                "{text:?}: {actual:?} / {expected:?}"
            );
            if let (Ok(actual), Ok(expected)) = (actual, expected) {
                assert_eq!(actual.count, expected.comparators.len(), "{text}");
                if let (Some(actual), Some(expected)) = (actual.first, expected.comparators.first())
                {
                    assert_eq!(format!("{:?}", actual.op), format!("{:?}", expected.op));
                    assert_eq!(
                        (actual.major, actual.minor, actual.patch, actual.pre),
                        (
                            expected.major,
                            expected.minor,
                            expected.patch,
                            expected.pre.as_str()
                        )
                    );
                }
            }
        }
    }
}
