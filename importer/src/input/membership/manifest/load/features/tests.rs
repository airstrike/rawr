mod resources;
mod warnings;

use rawr_toml::document;

use super::{Advice, Context, Feature, Features, Invalid};
use crate::input::membership::manifest::{
    load::Warnings,
    reference,
    schema::{manifest, primitive},
    session, value, Error, Stop, Tag,
};

const CONTEXT: Context<'static> = Context {
    allow_features: None,
    nightly_features_allowed: true,
    channel: "nightly",
    is_local: true,
};

fn inspect(source: &str, context: Context<'_>) -> (Result<Vec<String>, Invalid>, Vec<String>) {
    let document = document::parse(source, 80, &mut reference::Account).unwrap();
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let result = match Features::interpret(&original, context, &mut warnings, &mut session) {
        Ok(features) => Ok(features
            .activated(&mut session)
            .unwrap()
            .iter()
            .map(|text| text.as_str().to_owned())
            .collect()),
        Err(Stop::Rejected(error)) => match error.tag {
            Tag::Feature(invalid) => {
                assert_eq!(error.span, None);
                Err(invalid)
            }
            other => panic!("unexpected tag: {other:?}"),
        },
        Err(Stop::Refused) => panic!("unexpected refusal"),
    };
    session.finish(Ok(())).unwrap();
    (result, warnings.as_slice().to_vec())
}

#[test]
fn implicit_stable_enablement_and_explicit_activation_have_distinct_state() {
    let document = document::parse("", 80, &mut reference::Account).unwrap();
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let features = Features::interpret(&original, CONTEXT, &mut warnings, &mut session).unwrap();
    assert!(features
        .is_enabled(Feature::Edition2024, &mut session)
        .unwrap());
    assert!(!features
        .is_enabled(Feature::OpenNamespaces, &mut session)
        .unwrap());
    assert!(!features
        .is_enabled(Feature::PublishLockfile, &mut session)
        .unwrap());
    assert!(features.activated(&mut session).unwrap().is_empty());
    assert!(warnings.as_slice().is_empty());
    features
        .require(Feature::Edition2024, &mut session)
        .unwrap();
    session.finish(Ok(())).unwrap();

    let (result, warnings) = inspect("cargo-features=['edition2024','edition2024']", CONTEXT);
    assert_eq!(
        result,
        Err(Invalid::Duplicate {
            index: 1,
            feature: Feature::Edition2024
        })
    );
    assert_eq!(warnings.len(), 1);
}

#[test]
fn successful_context_borrows_exact_ordered_original_payload() {
    let document = document::parse(
        "cargo-features=['metabuild','edition2024','open-namespaces']",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let Some(manifest::Value::Primitive(primitive::Value::Strings(strings))) =
        original.get(manifest::Field::CargoFeatures)
    else {
        panic!("feature strings")
    };
    let mut warnings = Warnings::new();
    let features = Features::interpret(&original, CONTEXT, &mut warnings, &mut session).unwrap();
    let activated = features.activated(&mut session).unwrap();
    assert!(std::ptr::eq(
        activated.as_ptr(),
        strings.as_slice().as_ptr()
    ));
    assert_eq!(
        activated
            .iter()
            .map(value::Text::as_str)
            .collect::<Vec<_>>(),
        ["metabuild", "edition2024", "open-namespaces"]
    );
    for feature in [
        Feature::Metabuild,
        Feature::Edition2024,
        Feature::OpenNamespaces,
    ] {
        features.require(feature, &mut session).unwrap();
    }
    assert!(matches!(features.require(Feature::TrimPaths, &mut session),
    Err(Stop::Rejected(error)) if error.tag == Tag::Feature(Invalid::Required {
        feature: Feature::TrimPaths, advice: Advice::AddDeclaration, hint: false,
    })));
    session.finish(Ok(())).unwrap();
}

#[test]
fn constructor_preserves_first_failure_and_prior_warnings() {
    for (source, context, expected, warnings) in [
        (
            "cargo-features=['edition','bad_name','metabuild']",
            CONTEXT,
            Invalid::UnknownUnderscore { index: 1 },
            1,
        ),
        (
            "cargo-features=['edition','script','publish-lockfile']",
            CONTEXT,
            Invalid::UnknownCli { index: 1 },
            1,
        ),
        (
            "cargo-features=['open-namespaces','unknown']",
            Context {
                nightly_features_allowed: false,
                ..CONTEXT
            },
            Invalid::Channel {
                index: 0,
                feature: Feature::OpenNamespaces,
            },
            0,
        ),
        (
            "cargo-features=['publish-lockfile','metabuild']",
            CONTEXT,
            Invalid::Removed {
                index: 0,
                feature: Feature::PublishLockfile,
                local: true,
            },
            0,
        ),
        (
            "cargo-features=['metabuild','metabuild','unknown']",
            CONTEXT,
            Invalid::Duplicate {
                index: 1,
                feature: Feature::Metabuild,
            },
            0,
        ),
    ] {
        let (actual, retained) = inspect(source, context);
        assert_eq!(actual, Err(expected), "{source}");
        assert_eq!(retained.len(), warnings, "{source}");
    }
}

#[test]
fn require_uses_captured_locality_and_eligibility_and_keeps_hint_presence() {
    let document = document::parse("", 80, &mut reference::Account).unwrap();
    for (nightly, local, advice) in [
        (true, true, Advice::AddDeclaration),
        (true, false, Advice::NewerNightly),
        (false, true, Advice::NewerCargo),
        (false, false, Advice::NewerCargo),
    ] {
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let original =
            manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
        let mut warnings = Warnings::new();
        let features = Features::interpret(
            &original,
            Context {
                nightly_features_allowed: nightly,
                is_local: local,
                ..CONTEXT
            },
            &mut warnings,
            &mut session,
        )
        .unwrap();
        for hint in [None, Some(""), Some("help: owner@1.0.0 requires rust 1.99")] {
            assert!(
                matches!(features.require_with_hint(Feature::UnstableEditions, hint, &mut session),
                Err(Stop::Rejected(error)) if error.tag == Tag::Feature(Invalid::Required {
                    feature: Feature::UnstableEditions, advice, hint: hint.is_some(),
                }))
            );
            features
                .require_with_hint(Feature::Edition, hint, &mut session)
                .unwrap();
        }
        // A semantic require failure is reusable by pinned fallback callers.
        assert!(features.is_enabled(Feature::Edition, &mut session).unwrap());
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn complete_typed_failure_precedes_feature_check_even_when_feature_is_textually_first() {
    let document = document::parse(
        "cargo-features=['not-a-feature']\npackage={version=123}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let mut warnings = Warnings::new();
    let result = manifest::Original::interpret(value::Input::root(&document), &mut session)
        .and_then(|original| {
            Features::interpret(&original, CONTEXT, &mut warnings, &mut session).map(|_| ())
        });
    assert!(
        matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag != Tag::Feature(Invalid::Unknown { index: 0 }))
    );
    assert!(warnings.as_slice().is_empty());
}
