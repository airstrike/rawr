// Adapted from semver 1.0.28 src/error.rs. See ../LICENSE-MIT.
//! Pinned diagnostic rendering, without allocating an intermediate message.

use super::{Position, Rejection};
use std::fmt;

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty string, expected a semver version"),
            Self::UnexpectedEnd(position) => {
                write!(f, "unexpected end of input while parsing {position}")
            }
            Self::UnexpectedChar(position, character) => {
                write!(
                    f,
                    "unexpected character {} while parsing {position}",
                    Quoted(*character)
                )
            }
            Self::UnexpectedCharAfter(position, character) => {
                write!(
                    f,
                    "unexpected character {} after {position}",
                    Quoted(*character)
                )
            }
            Self::ExpectedCommaFound(position, character) => {
                write!(
                    f,
                    "expected comma after {position}, found {}",
                    Quoted(*character)
                )
            }
            Self::LeadingZero(position) => write!(f, "invalid leading zero in {position}"),
            Self::Overflow(position) => write!(f, "value of {position} exceeds u64::MAX"),
            Self::EmptySegment(position) => write!(f, "empty identifier segment in {position}"),
            Self::WildcardNotTheOnlyComparator(character) => write!(
                f,
                "wildcard req ({character}) must be the only comparator in the version req"
            ),
            Self::UnexpectedAfterWildcard => {
                f.write_str("unexpected character after wildcard in version req")
            }
            Self::ExcessiveComparators => f.write_str("excessive number of version comparators"),
        }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Major => "major version number",
            Self::Minor => "minor version number",
            Self::Patch => "patch version number",
            Self::Pre => "pre-release identifier",
            Self::Build => "build metadata",
        })
    }
}

struct Quoted(char);
impl fmt::Display for Quoted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == '\0' {
            f.write_str("'\\0'")
        } else {
            write!(f, "{:?}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::membership::manifest::{
        reference, scalar::requirement::Requirement, session::Session,
    };

    #[test]
    fn rendered_requirement_errors_match_pinned_semver() {
        let mut inputs = vec![
            "".to_owned(),
            " ".to_owned(),
            "01".to_owned(),
            "1.02".to_owned(),
            "1.2.03".to_owned(),
            "18446744073709551616".to_owned(),
            "1.2.3-".to_owned(),
            "1.2.3-a..b".to_owned(),
            "1.2.3+".to_owned(),
            "1.2.3+a..b".to_owned(),
            "*,1".to_owned(),
            "*x".to_owned(),
            "1, *".to_owned(),
            "1.*.2".to_owned(),
            std::iter::repeat_n("1", 33).collect::<Vec<_>>().join(","),
        ];
        for prefix in ["", "1.", "1.2.", "1.2.3", "1.2.3-a", "1.2.3+a"] {
            for suffix in ['\0', '\n', '\t', '\'', '\\', 'é', '💥', '?'] {
                inputs.push(format!("{prefix}{suffix}"));
            }
        }
        for text in inputs {
            let expected = semver::VersionReq::parse(&text).unwrap_err().to_string();
            let mut account = reference::Account;
            let mut session = Session::new(&mut account);
            let actual = Requirement::parse(&text, &mut session)
                .unwrap()
                .unwrap_err();
            assert_eq!(actual.to_string(), expected, "{text:?}");
            session.finish(Ok(())).unwrap();
        }
    }

    #[test]
    fn rendered_complete_version_errors_match_pinned_semver() {
        for text in [
            "",
            "1",
            "1.",
            "1.2",
            "1.2.",
            "1.2.3?",
            "1.2.3-0a..b",
            "1.2.3+..",
            "01.2.3",
            "1.2.3\0",
        ] {
            let expected = semver::Version::parse(text).unwrap_err().to_string();
            let actual = super::super::parse(text).unwrap_err();
            assert_eq!(actual.to_string(), expected, "{text:?}");
        }
    }
}
