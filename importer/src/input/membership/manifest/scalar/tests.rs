//! Actual borrowed converters against the pinned semver implementation.

use std::cmp::Ordering;

use super::{name, partial, rust, version, Error, Kind, Name, Partial, Rust, Version};

#[test]
fn complete_versions_match_pinned_semver() {
    let mut cases = vec![
        "",
        "1",
        "1.2",
        "1.2.3",
        "0.0.0",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2.3 ",
        " 1.2.3",
        "1.2.3\n",
        "+1.2.3",
        "-1.2.3",
        "1.2.3-0",
        "1.2.3-00",
        "1.2.3-01a",
        "1.2.3-a..b",
        "1.2.3-.a",
        "1.2.3-a.",
        "1.2.3+00",
        "1.2.3+01.002",
        "1.2.3+",
        "1.2.3-",
        "1.2.3-+a",
        "1.2.3-a+b",
        "1.2.3-ä",
        "1.2.3+α",
        "1.2.3+a/b",
        "1.2.3+a\\b",
        "18446744073709551615.0.0",
        "18446744073709551616.0.0",
        "1.18446744073709551616.0",
        "1.0.18446744073709551616",
        "1.2.3+é.0",
        "1.2.3-🦀",
        "1.2.3-a+b+c",
        "1.2.3\0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    cases.push(format!(
        "1.2.3-{}+{}",
        "9".repeat(16_384),
        "00".repeat(8_192)
    ));
    for text in cases {
        let actual = Version::parse(&text);
        let expected = semver::Version::parse(&text);
        assert_eq!(
            actual.is_ok(),
            expected.is_ok(),
            "{text:?}: {actual:?} / {expected:?}"
        );
        if let (Ok(actual), Ok(expected)) = (actual, expected) {
            assert_eq!(
                (actual.major(), actual.minor(), actual.patch()),
                (expected.major, expected.minor, expected.patch)
            );
            assert_eq!(
                (actual.pre(), actual.build()),
                (expected.pre.as_str(), expected.build.as_str())
            );
            assert_eq!(actual.to_string(), expected.to_string());
            if !actual.pre().is_empty() {
                let start = text.as_ptr() as usize;
                let pointer = actual.pre().as_ptr() as usize;
                assert!(pointer >= start && pointer + actual.pre().len() <= start + text.len());
            }
        }
    }
}

#[test]
fn version_and_identifier_order_match_pinned_semver() {
    let values = [
        "0.9.9",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.0+0",
        "1.0.0+00",
        "1.0.0+1",
        "1.0.0+01",
        "1.0.0+001",
        "1.0.0+2",
        "1.0.0+02",
        "1.0.0+002",
        "1.0.0+10",
        "1.0.0+a",
        "1.0.0+a.1",
        "1.0.0+a.01",
        "2.0.0",
    ];
    for left in values {
        for right in values {
            let actual_left = Version::parse(left).unwrap();
            let actual_right = Version::parse(right).unwrap();
            let expected_left = semver::Version::parse(left).unwrap();
            let expected_right = semver::Version::parse(right).unwrap();
            assert_eq!(
                actual_left.cmp(&actual_right),
                expected_left.cmp(&expected_right),
                "{left} / {right}"
            );
            assert_eq!(
                actual_left.cmp_precedence(&actual_right),
                expected_left.cmp_precedence(&expected_right)
            );
            assert_eq!(actual_left == actual_right, expected_left == expected_right);
        }
    }
}

#[test]
fn partial_versions_preserve_full_version_first_and_raw_caret_check() {
    for (text, expected) in [
        ("1", "1"),
        ("1.2", "1.2"),
        ("1.2.3", "1.2.3"),
        ("1.2.3-a+build", "1.2.3-a+build"),
        (" 1.2.3-a+build ", "1.2.3-a"),
        ("1.2.3+build ", "1.2.3"),
        (" ^1.2", "1.2"),
        (" ^1.*", "1"),
        ("1.43.0-beta.1.x", "1.43.0-beta.1.x"),
    ] {
        assert_eq!(
            Partial::parse(text).unwrap().to_string(),
            expected,
            "{text}"
        );
    }
    for (text, expected) in [
        ("^1.2", partial::Rejection::VersionReq),
        ("1.*", partial::Rejection::VersionReq),
        ("1.1.x", partial::Rejection::VersionReq),
        ("*", partial::Rejection::VersionReq),
        ("x", partial::Rejection::VersionReq),
        ("1,2", partial::Rejection::VersionReq),
        ("1-a", partial::Rejection::Prerelease),
        ("1+a", partial::Rejection::BuildMetadata),
        ("foodaddle", partial::Rejection::Unexpected),
        ("1+bad-thing", partial::Rejection::Prerelease),
        ("1,2-a", partial::Rejection::Prerelease),
        ("1,2+build", partial::Rejection::BuildMetadata),
        ("\t1.2", partial::Rejection::Unexpected),
    ] {
        assert_eq!(
            Partial::parse(text),
            Err(Error::Partial(expected)),
            "{text}"
        );
    }
}

#[test]
fn partial_matching_and_order_retain_presence_and_build_metadata() {
    let one = Partial::parse("1").unwrap();
    assert!(one.matches(&Version::parse("1.2.3+build").unwrap()));
    assert!(!one.matches(&Version::parse("1.2.3-a").unwrap()));
    assert!(!one.matches(&Version::parse("2.0.0").unwrap()));
    let exact = Partial::parse("1.2.3-a+build").unwrap();
    assert!(exact.matches(&Version::parse("1.2.3-a+build").unwrap()));
    assert!(!exact.matches(&Version::parse("1.2.3-a+other").unwrap()));
    assert!(Partial::parse("1.2.3-a")
        .unwrap()
        .matches(&Version::parse("1.2.3-a+other").unwrap()));
    assert!(Partial::parse("1").unwrap() < Partial::parse("1.0").unwrap());
    assert!(Partial::parse("1.0").unwrap() < Partial::parse("1.0.0").unwrap());
    // Cargo derives Option ordering: absent prerelease precedes Some.
    assert!(Partial::parse("1.0.0").unwrap() < Partial::parse("1.0.0-a").unwrap());
    assert!(Partial::parse("1.0.0-a.9").unwrap() < Partial::parse("1.0.0-a.10").unwrap());
    assert!(Partial::parse("1.0.0+01").unwrap() < Partial::parse("1.0.0+2").unwrap());
    assert!(one.to_version().is_none());
    assert_eq!(
        exact.to_version().unwrap(),
        Version::parse("1.2.3-a+build").unwrap()
    );
}

#[test]
fn rust_versions_keep_partial_fallback_and_rejection_precedence() {
    for (text, expected) in [
        ("1", "1"),
        ("1.2", "1.2"),
        ("1.2.3", "1.2.3"),
        (" ^1.2", "1.2"),
        ("1.2.3+build ", "1.2.3"),
    ] {
        assert_eq!(Rust::parse(text).unwrap().to_string(), expected);
    }
    assert_eq!(
        Rust::parse("1.2.3-a+b"),
        Err(Error::Rust(rust::Rejection::Prerelease))
    );
    assert_eq!(
        Rust::parse("1.2.3+b"),
        Err(Error::Rust(rust::Rejection::BuildMetadata))
    );
    assert_eq!(
        Rust::parse("1-a"),
        Err(Error::Rust(rust::Rejection::Partial(
            partial::Rejection::Prerelease
        )))
    );
    let version = Rust::parse("1.2").unwrap();
    assert_eq!(
        (version.major(), version.minor(), version.patch()),
        (1, Some(2), None)
    );
}

#[test]
fn version_error_order_is_structured_before_contextual_rendering() {
    assert_eq!(
        Version::parse("01.bad"),
        Err(Error::Version(version::Rejection::LeadingZero(
            version::Position::Major
        )))
    );
    assert_eq!(
        Version::parse("1.2.3-01+"),
        Err(Error::Version(version::Rejection::LeadingZero(
            version::Position::Pre
        )))
    );
    assert_eq!(
        Version::parse("1.2.3-a.+"),
        Err(Error::Version(version::Rejection::EmptySegment(
            version::Position::Pre
        )))
    );
    assert!(Version::parse(" 1.2.3 ").is_err());
    assert!(Version::parse(" 1.2.3 ".trim()).is_ok());
}

#[test]
fn names_preserve_unicode_namespaces_and_distinct_grammars() {
    for text in ["naïve", "東京", "_rawr", "foo-bar", "foo::東京", "a\u{301}"] {
        let name = Name::parse(text, Kind::Package).unwrap();
        assert_eq!(name.as_str(), text);
        assert_eq!(name.kind(), Kind::Package);
        assert_eq!(name.as_str().as_ptr(), text.as_ptr());
    }
    for text in [
        "",
        "1foo",
        "foo::",
        "::foo",
        "foo:::bar",
        "foo/bar",
        "foo+bar",
        "ⒶⒷⒸ",
    ] {
        assert!(Name::parse(text, Kind::Package).is_err(), "{text}");
    }
    for kind in [Kind::Registry, Kind::PathBase] {
        assert!(Name::parse("a-b_東京", kind).is_ok());
        assert!(Name::parse("foo::bar", kind).is_err());
    }
    for text in ["c++17", "128bit", "_foo", "foo.bar", "東京"] {
        assert!(Name::parse(text, Kind::Feature).is_ok(), "{text}");
    }
    for text in [
        "", "+foo", "-foo", ".foo", "dep:bar", "foo/bar", "foo:bar", "foo?", "a¼",
    ] {
        assert!(Name::parse(text, Kind::Feature).is_err(), "{text}");
    }
    assert_eq!(
        Name::parse("dep:x/y", Kind::Feature),
        Err(Error::Name {
            kind: Kind::Feature,
            reason: name::Rejection::FeatureDependencyPrefix
        })
    );
    assert_eq!(
        Name::parse("?x/y", Kind::Feature),
        Err(Error::Name {
            kind: Kind::Feature,
            reason: name::Rejection::FeatureSlash
        })
    );
}

#[test]
fn profiles_preserve_reserved_unicode_casing_and_empty_acceptance() {
    for text in [
        "", "dev", "release", "東京", "Σ", "a¼", "1profile", "İ", "---",
    ] {
        assert!(Name::parse(text, Kind::Profile).is_ok(), "{text}");
    }
    for text in [
        "DEBUG",
        "Build-Override",
        "PACKAGE",
        "RuStC",
        "cargo",
        "CARGOX",
        "CARGOΣ",
    ] {
        assert!(Name::parse(text, Kind::Profile).is_err(), "{text}");
    }
    assert_eq!(
        Name::parse("DEBUG!", Kind::Profile),
        Err(Error::Name {
            kind: Kind::Profile,
            reason: name::Rejection::ProfileCharacter('!')
        })
    );
    assert_eq!(
        Name::parse("DEBUG", Kind::Profile),
        Err(Error::Name {
            kind: Kind::Profile,
            reason: name::Rejection::ProfileDebug
        })
    );
    assert_eq!(
        Name::parse("Build-Override", Kind::Profile),
        Err(Error::Name {
            kind: Kind::Profile,
            reason: name::Rejection::ProfileBuildOverride
        })
    );
    assert_eq!(
        Name::parse("a", Kind::Package)
            .unwrap()
            .cmp(&Name::parse("b", Kind::Package).unwrap()),
        Ordering::Less
    );
}
