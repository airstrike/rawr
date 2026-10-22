//! Bounded diagnostic categories and exact first-error spans from pinned Cargo.

use serde_json::{json, Value};

use crate::input::membership::manifest::{reference, scalar, spec, Diagnostic, Tag};

enum Expected {
    Tag(Tag),
    Name(scalar::Kind),
    Version,
    Partial(&'static str),
}

pub(super) fn compare(row: &Value, diagnostic: Diagnostic) {
    let id = reference::id(row);
    assert_eq!(
        row["error"]["phase"], "typed",
        "{id}: first rejection phase"
    );
    assert_eq!(
        json!(diagnostic.span.map(|span| [span.start, span.end])),
        row["error"]["span"],
        "{id}: applicable first-error span",
    );
    let expected = match id {
        "alias-shadow-invalid"
        | "flatten-positional-rejected"
        | "ordinary-before-missing"
        | "flatten-after-known"
        | "inherit-false-trial-fallback"
        | "trimpaths-staged-error"
        | "input-order-error-a"
        | "input-order-error-b"
        | "opt-u64"
        | "debug-float" => Expected::Tag(Tag::Type),
        "hints-positional-missing" | "target-positional-empty" | "missing-before-flatten" => {
            Expected::Tag(Tag::Missing)
        }
        "profile-unknown-overflow" | "unknown-over-u128" => Expected::Tag(Tag::IntegerOverflow),
        "metadata-u64-over-i64" | "dependency-extra-u64" => Expected::Tag(Tag::Integer64),
        "metadata-float-overflow" | "ignored-float-overflow" => Expected::Tag(Tag::FloatOverflow),
        "dependency-stage-u128" => Expected::Tag(Tag::Integer128),
        "inherit-field-false" | "inherit-dependency-false" | "inherit-lints-false" => {
            Expected::Tag(Tag::WorkspaceFalse)
        }
        "lint-unit-nonempty" | "datetime-staged-extra" | "datetime-flattened" => {
            Expected::Tag(Tag::Length)
        }
        "trimpaths-semantic-error"
        | "u32-over"
        | "u32-negative"
        | "opt-string-number"
        | "debug-three"
        | "recursive-profile-failure" => Expected::Tag(Tag::Variant),
        "workspace-keeps-version-space" => Expected::Version,
        "invalid-cargo-features" => Expected::Tag(Tag::CargoFeatures),
        "profile-name-reserved" => Expected::Name(scalar::Kind::Profile),
        "datetime-marker-invalid" => Expected::Tag(Tag::DateTime),
        "spec-url-unicode-fragment" | "spec-diagnostic-missing" | "spec-key-before-value" => {
            Expected::Name(scalar::Kind::Package)
        }
        // These ambient, diagnostic-only existence probes are omitted. They
        // still reject at the same source key under the complete name grammar.
        "spec-diagnostic-existing" | "spec-diagnostic-backslash" => {
            assert!(row["error"]["message"]
                .as_str()
                .unwrap()
                .contains("looks like a file path"));
            Expected::Name(scalar::Kind::Package)
        }
        "spec-query-rejected" => Expected::Tag(Tag::Spec(spec::Invalid::Query)),
        "spec-bad-protocol" => Expected::Tag(Tag::Spec(spec::Invalid::Protocol)),
        "spec-bad-path-protocol" => Expected::Tag(Tag::Spec(spec::Invalid::PathScheme)),
        "spec-empty-fragment" => Expected::Tag(Tag::Spec(spec::Invalid::EmptyFragment)),
        "spec-empty-build" => Expected::Partial("BuildMetadata"),
        "spec-partial-prerelease" => Expected::Partial("Prerelease"),
        "spec-bad-operator" => Expected::Partial("VersionReq"),
        _ => panic!("{id}: unmapped pinned first-error category"),
    };
    match expected {
        Expected::Tag(tag) => assert_eq!(diagnostic.tag, tag, "{id}: first-error category"),
        Expected::Name(expected) => {
            let Tag::Scalar(scalar::Error::Name { kind, .. }) = diagnostic.tag else {
                panic!("{id}: expected name rejection, got {diagnostic:?}")
            };
            assert_eq!(kind, expected, "{id}: name grammar");
        }
        Expected::Version => assert!(
            matches!(diagnostic.tag, Tag::Scalar(scalar::Error::Version(_))),
            "{id}: version conversion rejection: {diagnostic:?}"
        ),
        Expected::Partial(expected) => {
            let Tag::Scalar(scalar::Error::Partial(reason)) = diagnostic.tag else {
                panic!("{id}: expected partial version rejection, got {diagnostic:?}")
            };
            assert_eq!(
                format!("{reason:?}"),
                expected,
                "{id}: partial-version branch"
            );
        }
    }
}
