// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Profile override restrictions from pinned Cargo's validate_profile_override.
use crate::input::membership::manifest::{
    load::{
        features::{Feature, Features},
        normalize::target,
        Warnings,
    },
    schema::{
        primitive,
        profile::tree::{Field, Profile, Profiles, Value},
    },
    session::Session,
    Error, Step,
};
use rawr_toml::document;
mod root;

/// Values supplied by the associated Cargo configuration, never ambient flags.
#[derive(Clone, Copy, Default)]
pub(in super::super) struct Unstable {
    pub(in super::super) codegen_backend: bool,
    pub(in super::super) profile_rustflags: bool,
    pub(in super::super) trim_paths: bool,
    pub(in super::super) panic_immediate_abort: bool,
}

#[derive(Clone, Copy)]
pub(in crate::input::membership::manifest) struct Context<'a> {
    pub(in super::super) unstable: Unstable,
    pub(in crate::input::membership::manifest) cargo_version: &'a str,
}

#[cfg(test)]
impl Context<'static> {
    pub(in crate::input::membership::manifest) fn fixture() -> Self {
        Self {
            unstable: Unstable::default(),
            cargo_version: "TEST CARGO",
        }
    }
}

pub(in super::super) fn validate<A: document::Admission>(
    profiles: &Profiles<'_>,
    features: &Features<'_, '_>,
    unstable: Unstable,
    cargo_version: &str,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    for (name, _, profile) in profiles.entries() {
        session.items(1)?;
        if let Err(error) = layer(profile, features, unstable, cargo_version, session)? {
            return Ok(Err(error));
        }
        session.items(profile.entries().len())?;
        match profile.get(Field::BuildOverride) {
            Some(Value::BuildOverride(index)) => {
                let child = profiles.node(*index);
                if let Err(error) = override_fields(child, "build-override", session)? {
                    return Ok(Err(error));
                }
                if let Err(error) = layer(child, features, unstable, cargo_version, session)? {
                    return Ok(Err(error));
                }
            }
            None => {}
            _ => return session.fail(Error::State),
        }
        session.items(profile.entries().len())?;
        match profile.get(Field::Package) {
            Some(Value::Package(packages)) => {
                for (_, _, index) in packages.entries() {
                    session.items(1)?;
                    let child = profiles.node(index);
                    if let Err(error) = override_fields(child, "package", session)? {
                        return Ok(Err(error));
                    }
                    if let Err(error) = layer(child, features, unstable, cargo_version, session)? {
                        return Ok(Err(error));
                    }
                }
            }
            None => {}
            _ => return session.fail(Error::State),
        }
        if let Err(error) = root::validate(profile, name.as_str(), warnings, session)? {
            return Ok(Err(error));
        }
    }
    Ok(Ok(()))
}

fn layer<A: document::Admission>(
    profile: &Profile<'_>,
    features: &Features<'_, '_>,
    unstable: Unstable,
    cargo_version: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    for (field, feature, cli) in [
        (
            Field::CodegenBackend,
            Feature::CodegenBackend,
            unstable.codegen_backend,
        ),
        (
            Field::Rustflags,
            Feature::ProfileRustflags,
            unstable.profile_rustflags,
        ),
        (Field::TrimPaths, Feature::TrimPaths, unstable.trim_paths),
        (
            Field::Panic,
            Feature::PanicImmediateAbort,
            unstable.panic_immediate_abort,
        ),
    ] {
        session.items(profile.entries().len())?;
        let Some(value) = profile.get(field) else {
            continue;
        };
        if field == Field::Panic {
            let Value::Primitive(primitive::Value::String(text)) = value else {
                return session.fail(Error::State);
            };
            session.string(text.as_str().len())?;
            if text.as_str() != "immediate-abort" {
                continue;
            }
        }
        // Cargo constructs the error before consulting the CLI alternative.
        let result = features.require_owned(feature, cargo_version, session)?;
        if !cli && let Err(error) = result {
            return Ok(Err(error));
        }
    }
    Ok(Ok(()))
}

/// Called before feature validation for each build or package override.
pub(in super::super) fn override_fields<A: document::Admission>(
    profile: &Profile<'_>,
    which: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    // Cargo checks presence, including false and empty values, in this order.
    for field in [
        Field::Package,
        Field::BuildOverride,
        Field::Panic,
        Field::Lto,
        Field::Rpath,
    ] {
        session.items(profile.entries().len())?;
        if profile.get(field).is_none() {
            continue;
        }
        return match field {
            Field::Package => target::formatted(
                format_args!("package-specific profiles cannot be nested"),
                session,
            )
            .map(Err),
            Field::BuildOverride => target::formatted(
                format_args!("build-override profiles cannot be nested"),
                session,
            )
            .map(Err),
            _ => {
                let name = match field {
                    Field::Panic => "panic",
                    Field::Lto => "lto",
                    Field::Rpath => "rpath",
                    _ => unreachable!(),
                };
                session.string(which.len())?;
                target::formatted(
                    format_args!("`{name}` may not be specified in a `{which}` profile"),
                    session,
                )
                .map(Err)
            }
        };
    }
    Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, schema::profile::Profiles, value};

    #[test]
    fn composed_profiles_preserve_feature_and_override_precedence() {
        use crate::input::membership::manifest::{load::features::Context, schema::manifest};
        for (declarations, source, cli, expected) in [
            ("", "[profile.dev]\nrustflags=[]\ndir-name='bad'", false, "feature `profile-rustflags` is required"),
            ("", "[profile.dev]\nrustflags=[]\ndir-name='bad'", true, "dir-name=\"bad\""),
            ("cargo-features=['profile-rustflags']\n", "[profile.dev]\nrustflags=[]", false, ""),
            ("", "[profile.dev]\nrustflags=[]", true, ""),
            ("", "[profile.dev]\ncodegen-backend='llvm'\nrustflags=[]", false, "feature `codegen-backend` is required"),
            ("", "[profile.dev]\npanic='immediate-abort'", false, "feature `panic-immediate-abort` is required"),
            ("", "[profile.dev]\npanic='immediate-abort'", true, ""),
            ("", "[profile.dev]\ntrim-paths=false", false, "feature `trim-paths` is required"),
            ("", "[profile.dev.build-override]\npanic='immediate-abort'\nrustflags=[]", false, "`panic` may not be specified in a `build-override` profile"),
            ("", "[profile.dev]\ndir-name='bad'\n[profile.dev.build-override]\nrustflags=[]\n[profile.dev.package.foo]\nlto=false", false, "feature `profile-rustflags` is required"),
            ("", "[profile.dev]\ndir-name='bad'\n[profile.dev.build-override]\nrustflags=[]\n[profile.dev.package.foo]\nlto=false", true, "`lto` may not be specified in a `package` profile"),
        ] {
            let text = format!("{declarations}{source}");
            let parsed = document::parse(&text, 4096, &mut reference::Account).unwrap();
            let mut account = reference::Account;
            let mut session = Session::new(&mut account);
            let original = manifest::Original::interpret(value::Input::root(&parsed), &mut session).unwrap();
            let mut warnings = Warnings::new();
            let features = Features::interpret(&original, Context { allow_features: None, nightly_features_allowed: true, channel: "nightly", is_local: true }, &mut warnings, &mut session).unwrap();
            let profiles = Profiles::interpret(value::Input::raw(parsed.table().get("profile").unwrap()), &mut session).unwrap();
            let unstable = Unstable { codegen_backend: cli, profile_rustflags: cli, trim_paths: cli, panic_immediate_abort: cli };
            let result = validate(&profiles, &features, unstable, "TEST CARGO", &mut warnings, &mut session).unwrap();
            if expected.is_empty() { assert!(result.is_ok(), "{text}: {result:?}"); }
            else { assert!(result.unwrap_err().starts_with(expected), "{text}"); }
            session.finish(Ok(())).unwrap();
        }
    }

    #[test]
    fn override_presence_and_error_order_match_cargo() {
        for (fields, expected) in [
            ("", None),
            ("opt-level=2", None),
            ("rpath=false", Some("`rpath` may not be specified")),
            ("lto=false\nrpath=false", Some("`lto` may not be specified")),
            (
                "panic='abort'\nlto=false",
                Some("`panic` may not be specified"),
            ),
            (
                "build-override={}\npanic='abort'",
                Some("build-override profiles cannot be nested"),
            ),
            (
                "package={}\nbuild-override={}",
                Some("package-specific profiles cannot be nested"),
            ),
        ] {
            for which in ["package", "build-override"] {
                let source = format!("[profile.dev]\n{fields}");
                let document = document::parse(&source, 4096, &mut reference::Account).unwrap();
                let mut account = reference::Account;
                let mut session = Session::new(&mut account);
                let profiles = Profiles::interpret(
                    value::Input::raw(document.table().get("profile").unwrap()),
                    &mut session,
                )
                .unwrap();
                let (_, _, profile) = profiles.entries().next().unwrap();
                let actual = override_fields(profile, which, &mut session).unwrap();
                let expected = expected.map(|text| {
                    if text.starts_with('`') {
                        format!("{text} in a `{which}` profile")
                    } else {
                        text.to_owned()
                    }
                });
                assert_eq!(actual.err(), expected, "{source}");
            }
        }
    }
}
