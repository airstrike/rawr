// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Post-construction warnings, before Manifest::feature_gate.
use crate::input::membership::manifest::{
    load::{features, formatted, normalize::package, Warnings},
    schema::{manifest::Original, package::Field},
    session::Session,
    Step,
};
use rawr_toml::document;

pub(in super::super) struct Failure {
    pub(in super::super) context: &'static str,
    pub(in super::super) cause: String,
}
pub(in super::super) fn gates<A: document::Admission>(
    teapot_present: bool,
    target_present: bool,
    features: &features::Features<'_, '_>,
    cargo_version: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<(), Failure>> {
    session.check()?;
    for (present, feature, context) in [
        (teapot_present, features::Feature::TestDummyUnstable, "the `im-a-teapot` manifest key is unstable and may not work properly in England"),
        (target_present, features::Feature::PerPackageTarget, "the `package.default-target` and `package.forced-target` manifest keys are unstable and may not work properly"),
    ] {
        session.items(1)?;
        if present
            && let Err(cause) = features.require_owned(feature, cargo_version, session)? {
                return Ok(Err(Failure { context, cause }));
            }
    }
    Ok(Ok(()))
}

// Pinned Cargo util/context/mod.rs TOP_LEVEL_CONFIG_KEYS, in source order.
const CONFIG: &[&str] = &[
    "paths",
    "alias",
    "build",
    "credential-alias",
    "doc",
    "env",
    "future-incompat-report",
    "cache",
    "cargo-new",
    "http",
    "install",
    "net",
    "patch",
    "profile",
    "resolver",
    "registries",
    "registry",
    "source",
    "target",
    "term",
];
pub(in super::super) fn warnings<A: document::Admission>(
    package: Option<&package::View<'_, '_>>,
    original: &Original<'_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    if let Some(package) = package {
        session.items(1)?;
        if package.get(Field::LicenseFile).is_some() {
            session.items(1)?;
            if package.get(Field::License).is_some() {
                warnings.push(&["only one of `license` or `license-file` is necessary\n`license` should be used if the package license can be expressed with a standard SPDX expression.\n`license-file` should be used if the package uses a non-standard license.\nSee https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields for more information."], session)?;
            }
        }
    }
    for key in original.unused().unique() {
        session.items(1)?;
        let mut message = formatted(format_args!("unused manifest key: {key}"), session)?;
        let mut config = false;
        for candidate in CONFIG {
            session.items(1)?;
            session.items(key.len())?;
            session.items(candidate.len())?;
            if key == *candidate {
                config = true;
                break;
            }
        }
        if config {
            features::append(&mut message, "\nhelp: ", session)?;
            features::append(&mut message, key, session)?;
            features::append(&mut message, " is a valid .cargo/config.toml key", session)?;
        }
        warnings.push_formatted(message, session)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, value};
    #[test]
    fn final_gates_keep_context_and_teapot_precedence() {
        for (declarations, expected) in [
            ("", Some("test-dummy-unstable")),
            (
                "cargo-features=['test-dummy-unstable']",
                Some("per-package-target"),
            ),
            (
                "cargo-features=['test-dummy-unstable','per-package-target']",
                None,
            ),
        ] {
            let parsed = document::parse(declarations, 256, &mut reference::Account).unwrap();
            let mut account = reference::Account;
            let mut session = Session::new(&mut account);
            let original = Original::interpret(value::Input::root(&parsed), &mut session).unwrap();
            let features = features::Features::interpret(
                &original,
                features::Context {
                    allow_features: None,
                    nightly_features_allowed: true,
                    channel: "nightly",
                    is_local: true,
                },
                &mut Warnings::new(),
                &mut session,
            )
            .unwrap();
            let result = gates(true, true, &features, "TEST CARGO", &mut session).unwrap();
            match expected {
                Some(name) => {
                    let Err(error) = result else { panic!("gate") };
                    assert!(error
                        .cause
                        .starts_with(&format!("feature `{name}` is required")));
                    assert!(error.cause.contains("TEST CARGO"));
                    assert_eq!(
                        error.context,
                        if name == "test-dummy-unstable" {
                            "the `im-a-teapot` manifest key is unstable and may not work properly in England"
                        } else {
                            "the `package.default-target` and `package.forced-target` manifest keys are unstable and may not work properly"
                        }
                    );
                }
                None => assert!(result.is_ok()),
            }
            assert!(gates(false, false, &features, "TEST CARGO", &mut session)
                .unwrap()
                .is_ok());
        }
    }
}
