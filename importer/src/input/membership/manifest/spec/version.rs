//! Owned PackageIdSpec PartialVersion components, without reparsing or self-reference.
//!
//! Parsing and identifier comparison delegate to the contained scalar closure.
//! Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
//! `crates/cargo-util-schemas/src/core/partial_version.rs:6–13,60–120` defines
//! field order, Option presence, and the full-Version then VersionReq trial.
//! That source's SHA-256 is
//! `fe796cd12bac41396ca66dd6c093fe0ccd281f186764912057f81d1b112127a0`.
//! Semver 1.0.28 `src/impls.rs:51–159` defines prerelease/build ordering; SHA-256
//! `30bac3940b9bab840dc59280832c9afbb342abf47c51740a1b83b43c1646c56b`.
//! Exact source/license correspondence remains in `../scalar/PROVENANCE.md`
//! and `../scalar/LICENSE-MIT`; no upstream grammar is duplicated here.
//!
//! The enclosing caller admits the Version slot. Each present identifier owns
//! one prospectively admitted exact byte layout. String retains that allocation
//! without a shrink/copy; absence allocates nothing. Parsing and comparison are
//! linear in visited input bytes, and teardown owns at most two flat strings.
//! Full conversion follows the same pinned PartialVersion::to_version: test
//! minor and patch presence before cloning prerelease, then build. The returned
//! Full owns those identifiers independently of the original PackageIdSpec.

use std::alloc::Layout;
use std::cmp::Ordering;
use std::string;

use rawr_toml::document;

use super::super::{scalar, session, Diagnostic, Error, Span, Step, Tag};

#[derive(Debug, Eq, PartialEq)]
pub(in super::super) struct Version {
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Option<string::String>,
    build: Option<string::String>,
}

/// Owned complete version, derived from parsed components or Cargo's default.
pub(in super::super) struct Full {
    major: u64,
    minor: u64,
    patch: u64,
    pre: String,
    build: String,
}
impl Full {
    pub(in super::super) fn equal<A: document::Admission>(
        &self,
        other: &Self,
        session: &mut session::Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        if (self.major, self.minor, self.patch) != (other.major, other.minor, other.patch) {
            return Ok(false);
        }
        session.string(self.pre.len())?;
        session.string(other.pre.len())?;
        session.items(self.pre.len())?;
        session.items(other.pre.len())?;
        if self.pre != other.pre {
            return Ok(false);
        }
        session.string(self.build.len())?;
        session.string(other.build.len())?;
        session.items(self.build.len())?;
        session.items(other.build.len())?;
        Ok(self.build == other.build)
    }
    /// Clone normalized components in semver field order; absent versions use
    /// Cargo's PackageId default 0.0.0 without allocating identifier buffers.
    pub(in super::super) fn from_normalized<A: document::Admission>(
        version: Option<&super::super::schema::primitive::Version<'_>>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let Some(version) = version else {
            return Ok(Self {
                major: 0,
                minor: 0,
                patch: 0,
                pre: String::new(),
                build: String::new(),
            });
        };
        let pre = clone_identifier(Some(version.pre()), session)?;
        let build = clone_identifier(Some(version.build()), session)?;
        Ok(Self {
            major: version.major(),
            minor: version.minor(),
            patch: version.patch(),
            pre,
            build,
        })
    }
    pub(in super::super) fn major(&self) -> u64 {
        self.major
    }
    pub(in super::super) fn minor(&self) -> u64 {
        self.minor
    }
    pub(in super::super) fn patch(&self) -> u64 {
        self.patch
    }
    pub(in super::super) fn pre(&self) -> &str {
        &self.pre
    }
    pub(in super::super) fn build(&self) -> &str {
        &self.build
    }
}

impl Version {
    pub(super) fn full<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<Full>> {
        session.check()?;
        let Some(minor) = self.minor else {
            return Ok(None);
        };
        let Some(patch) = self.patch else {
            return Ok(None);
        };
        session.items(1)?;
        let pre = clone_identifier(self.pre(), session)?;
        let build = clone_identifier(self.build(), session)?;
        Ok(Some(Full {
            major: self.major,
            minor,
            patch,
            pre,
            build,
        }))
    }
    pub(super) fn parse<A: document::Admission>(
        text: &str,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        session.string(text.len())?;
        let parsed = match scalar::Partial::parse(text) {
            Ok(parsed) => parsed,
            Err(error) => return session.reject(Diagnostic::at(Tag::Scalar(error), span)),
        };
        let pre = parsed.pre().map(|value| own(value, session)).transpose()?;
        let build = parsed
            .build()
            .map(|value| own(value, session))
            .transpose()?;
        Ok(Self {
            major: parsed.major(),
            minor: parsed.minor(),
            patch: parsed.patch(),
            pre,
            build,
        })
    }

    pub(in super::super) fn major(&self) -> u64 {
        self.major
    }
    pub(in super::super) fn minor(&self) -> Option<u64> {
        self.minor
    }
    pub(in super::super) fn patch(&self) -> Option<u64> {
        self.patch
    }
    pub(in super::super) fn pre(&self) -> Option<&str> {
        self.pre.as_deref()
    }
    pub(in super::super) fn build(&self) -> Option<&str> {
        self.build.as_deref()
    }
}

fn clone_identifier<A: document::Admission>(
    text: Option<&str>,
    session: &mut session::Session<'_, A>,
) -> Step<String> {
    match text {
        Some(text) if !text.is_empty() => own(text, session),
        _ => Ok(String::new()),
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| optional(self.pre(), other.pre(), scalar::compare_pre))
            .then_with(|| optional(self.build(), other.build(), scalar::compare_build))
    }
}

fn optional(
    left: Option<&str>,
    right: Option<&str>,
    compare: impl FnOnce(&str, &str) -> Ordering,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => compare(left, right),
    }
}

fn own<A: document::Admission>(
    value: &str,
    session: &mut session::Session<'_, A>,
) -> Step<string::String> {
    let Ok(layout) = Layout::array::<u8>(value.len()) else {
        return session.fail(Error::Overflow);
    };
    session.string(value.len())?;
    session.items(1)?;
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut owned = string::String::new();
    if let Err(error) = owned.try_reserve_exact(value.len()) {
        return session.fail(Error::Allocation(error));
    }
    owned.push_str(value);
    Ok(owned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn full_equality_repeated_work_exhausts_original_account() {
        struct Work(usize);
        impl document::Admission for Work {
            type Error = ();
            fn admit(&mut self, request: document::Request) -> Result<(), ()> {
                if let document::Request::Items(n) = request {
                    self.0 = self.0.checked_sub(n).ok_or(())?;
                }
                Ok(())
            }
        }
        let left = super::Full {
            major: 1,
            minor: 2,
            patch: 3,
            pre: "long-prefix".into(),
            build: "metadata".into(),
        };
        let right = super::Full {
            major: 1,
            minor: 2,
            patch: 3,
            pre: "long-prefix".into(),
            build: "metadata".into(),
        };
        let mut work = Work(1 + 2 * (left.pre.len() + left.build.len()));
        let mut session = session::Session::new(&mut work);
        assert!(left.equal(&right, &mut session).unwrap());
        assert!(matches!(
            left.equal(&right, &mut session),
            Err(crate::input::membership::manifest::Stop::Refused)
        ));
        assert!(matches!(
            session.check(),
            Err(crate::input::membership::manifest::Stop::Refused)
        ));
    }
    #[test]
    fn normalized_full_version_owns_identifiers_and_defaults() {
        use crate::input::membership::manifest::{
            reference,
            schema::{manifest, package, primitive},
            value,
        };
        for text in [
            "",
            "version='1.2.3'",
            "version='7.8.9-long.prerelease+long.build.metadata'",
        ] {
            let source = format!("[package]\nname='p'\n{text}");
            let parsed = document::parse(&source, 128, &mut reference::Account).unwrap();
            let mut account = reference::Account;
            let mut session = session::Session::new(&mut account);
            let original =
                manifest::Original::interpret(value::Input::root(&parsed), &mut session).unwrap();
            let Some(manifest::Value::Package(package)) = original.get(manifest::Field::Package)
            else {
                panic!("package")
            };
            let version = match package.get(package::Field::Version) {
                None => None,
                Some(package::Value::Local(primitive::Value::Version(version))) => Some(version),
                _ => panic!("version"),
            };
            let full = super::Full::from_normalized(version, &mut session).unwrap();
            drop(original);
            drop(parsed);
            let expected = if text.is_empty() {
                (0, 0, 0, "", "")
            } else if text.contains("1.2.3") {
                (1, 2, 3, "", "")
            } else {
                (7, 8, 9, "long.prerelease", "long.build.metadata")
            };
            assert_eq!(
                (
                    full.major(),
                    full.minor(),
                    full.patch(),
                    full.pre(),
                    full.build()
                ),
                expected
            );
        }
    }
    use std::cmp::Ordering;
    use std::string;

    use rawr_toml::document;

    use super::Version;
    use crate::input::membership::manifest::{scalar, session, Error, Span, Stop, Tag};
    use crate::{cargo, input, node};

    #[derive(Default)]
    struct Meter {
        requests: Vec<document::Request>,
        refuse: Option<usize>,
    }

    impl document::Admission for Meter {
        type Error = usize;

        fn admit(&mut self, request: document::Request) -> Result<(), usize> {
            let index = self.requests.len();
            self.requests.push(request);
            if self.refuse == Some(index) {
                Err(index)
            } else {
                Ok(())
            }
        }
    }

    fn parse(text: &str) -> Version {
        let mut meter = Meter::default();
        let mut session = session::Session::new(&mut meter);
        let result = Version::parse(text, None, &mut session);
        session.finish(result).expect("valid owned partial version")
    }

    type Reference = (
        u64,
        Option<u64>,
        Option<u64>,
        Option<semver::Prerelease>,
        Option<semver::BuildMetadata>,
    );

    // Cargo partial_version.rs:84–120, using the actual pinned semver parser
    // and its Ord implementations. Only the fixed error wording is omitted.
    fn reference(text: &str) -> Option<Reference> {
        if let Ok(value) = semver::Version::parse(text) {
            return Some((
                value.major,
                Some(value.minor),
                Some(value.patch),
                (!value.pre.is_empty()).then_some(value.pre),
                (!value.build.is_empty()).then_some(value.build),
            ));
        }
        let mut requirement = semver::VersionReq::parse(text).ok()?;
        if requirement.comparators.len() != 1 {
            return None;
        }
        let value = requirement.comparators.pop()?;
        if value.op != semver::Op::Caret || text.starts_with('^') {
            return None;
        }
        Some((
            value.major,
            value.minor,
            value.patch,
            (!value.pre.is_empty()).then_some(value.pre),
            None,
        ))
    }

    #[test]
    fn complete_trials_match_pinned_components_and_rejection_spans() {
        let mut cases = [
            "",
            "1",
            "1.2",
            "1.2.3",
            "0.0.0",
            "1.2.3-a+build",
            " 1.2.3-a+build ",
            "1.2.3+build ",
            " ^1.2",
            " ^1.*",
            "^1.2",
            "1.*",
            "*",
            "=1.2.3",
            ">1",
            "1,2",
            "1.2.3\t",
            "01.2.3",
            "1.2.3-01",
            "1.2.3+0007",
            "1.2.3+é",
            "1.2.3-",
            "1.2.3+",
            "1.2.3-a..b",
            "18446744073709551615",
            "18446744073709551616",
            "1.18446744073709551616.0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        cases.push(format!("1.2.3-{}+{}", "9".repeat(4096), "00".repeat(2048)));
        let span = Some(Span { start: 7, end: 41 });
        for text in cases {
            let mut meter = Meter::default();
            let mut session = session::Session::new(&mut meter);
            let result = Version::parse(&text, span, &mut session);
            let actual = session.finish(result);
            let expected = reference(&text);
            assert_eq!(actual.is_ok(), expected.is_some(), "{text:?}");
            match (actual, expected) {
                (Ok(actual), Some(expected)) => {
                    assert_eq!(
                        (actual.major(), actual.minor(), actual.patch()),
                        (expected.0, expected.1, expected.2),
                        "{text:?}"
                    );
                    assert_eq!(
                        actual.pre(),
                        expected.3.as_ref().map(semver::Prerelease::as_str)
                    );
                    assert_eq!(
                        actual.build(),
                        expected.4.as_ref().map(semver::BuildMetadata::as_str)
                    );
                }
                (Err(Error::Invalid(diagnostic)), None) => {
                    assert_eq!(diagnostic.span, span);
                    assert_eq!(
                        diagnostic.tag,
                        Tag::Scalar(scalar::Partial::parse(&text).expect_err("borrowed rejection"))
                    );
                }
                other => panic!("unexpected trial result: {other:?}"),
            }
        }
    }

    #[test]
    fn ordering_matches_partial_presence_and_pinned_numeric_identifiers() {
        let cases = [
            "1",
            "1.0",
            "1.0.0",
            "1.0.0-alpha",
            "1.0.0-alpha.2",
            "1.0.0-alpha.11",
            "1.0.0-9",
            "1.0.0-100",
            "1.0.0+a",
            "1.0.0+0",
            "1.0.0+00",
            "1.0.0+1",
            "1.0.0+01",
            "1.0.0+001",
            "1.0.0+2",
            "1.0.0+10",
            "1.0.0+a.01",
            "1.0.0+a.1",
            " 1.0.0+ignored ",
            "2",
        ];
        for left in cases {
            let owned_left = parse(left);
            for right in cases {
                let owned_right = parse(right);
                let expected = reference(left).unwrap().cmp(&reference(right).unwrap());
                assert_eq!(owned_left.cmp(&owned_right), expected, "{left} / {right}");
                assert_eq!(owned_left.partial_cmp(&owned_right), Some(expected));
                assert_eq!(owned_left == owned_right, expected == Ordering::Equal);
                assert_eq!(
                    owned_left.cmp(&owned_right),
                    scalar::Partial::parse(left)
                        .unwrap()
                        .cmp(&scalar::Partial::parse(right).unwrap())
                );
            }
        }
        assert!(
            parse("1.0.0") < parse("1.0.0-alpha"),
            "Cargo orders absent prerelease first"
        );
        assert!(
            parse("1.0.0+2") < parse("1.0.0+10"),
            "build ordering is numeric"
        );
    }

    #[test]
    fn owned_identifiers_survive_input_drop_without_storing_its_spelling() {
        let value = {
            let source = string::String::from("1.2.3-alpha.2+0007");
            let value = parse(&source);
            assert_ne!(value.pre().unwrap().as_ptr(), source[6..].as_ptr());
            value
        };
        assert_eq!(value.pre(), Some("alpha.2"));
        assert_eq!(value.build(), Some("0007"));
        assert_eq!(value, parse("1.2.3-alpha.2+0007"));
    }

    #[test]
    fn exact_identifier_layouts_and_every_refusal_preserve_the_first_cause() {
        let mut meter = Meter::default();
        let mut session = session::Session::new(&mut meter);
        let result = Version::parse("1.2.3-alpha+0007", None, &mut session);
        session.finish(result).expect("owned identifiers");
        assert_eq!(meter.requests.len(), 7);
        assert!(matches!(meter.requests[0], document::Request::String(16)));
        for (requests, length) in meter.requests[1..].as_chunks::<3>().0.iter().zip([5, 4]) {
            assert!(matches!(requests[0], document::Request::String(n) if n == length));
            assert!(matches!(requests[1], document::Request::Items(1)));
            assert!(
                matches!(requests[2], document::Request::Allocation { bytes, alignment: 1 } if bytes == length)
            );
        }
        for refuse in 0..7 {
            let mut meter = Meter {
                refuse: Some(refuse),
                ..Meter::default()
            };
            let mut session = session::Session::new(&mut meter);
            assert!(matches!(
                Version::parse("1.2.3-alpha+0007", None, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                Version::parse("1", None, &mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refuse)
            );
            assert_eq!(meter.requests.len(), refuse + 1);
        }
        let mut meter = Meter {
            refuse: Some(1),
            ..Meter::default()
        };
        let mut session = session::Session::new(&mut meter);
        assert!(matches!(
            Version::parse("invalid", None, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(1))));
        assert_eq!(
            meter.requests.len(),
            2,
            "refusal while retaining rejection stays terminal"
        );
    }

    #[test]
    fn original_budget_keeps_prior_usage_and_exact_identifier_floors() {
        use crate::input::membership::manifest::tests::one_short;
        for (item_floor, byte_floor) in [(0, 0), (1, 0), (0, 1)] {
            let limits = cargo::Limits::standard();
            let mut allocation = input::Allocation::new(&limits);
            allocation.items(7).unwrap();
            allocation.bytes(31).unwrap();
            allocation.item_limit = 9 - item_floor;
            allocation.byte_limit = 40 - byte_floor;
            let graph = node::GraphBudget::new();
            graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
            graph
                .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
                .unwrap();
            graph
                .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
                .unwrap();
            let mut budget = input::membership::Budget {
                graph: &graph,
                allocation: &mut allocation,
            };
            let mut session = session::Session::new(&mut budget);
            let result = Version::parse("1.2.3-alpha+0007", None, &mut session);
            let result = session.finish(result);
            // A one-short item or byte target still admits both owned
            // identifiers and records only that axis at the exact total.
            assert_eq!(
                result.expect("exact original remainder").build(),
                Some("0007")
            );
            assert_eq!(
                allocation.usage(),
                input::Usage {
                    items: 9,
                    bytes: 40
                }
            );
            assert!(allocation.check().is_ok(), "accounting targets never latch");
            let exceeded = allocation.exceedances();
            match (item_floor, byte_floor) {
                (0, 0) => assert!(!exceeded.is_exceeded()),
                (1, 0) => assert!(one_short(&exceeded, input::INPUT_ITEMS_RESOURCE, 9)),
                _ => {
                    // The first owned identifier (36 bytes) fits; the second
                    // crosses the one-short target and is still admitted.
                    assert!(one_short(&exceeded, input::INPUT_BYTES_RESOURCE, 40));
                    let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                    assert_eq!(axis.first_requested, 40);
                    assert!(axis.target >= 36);
                }
            }
        }
    }
}
