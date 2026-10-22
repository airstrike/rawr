use super::super::Context;
use super::{inspect, CONTEXT};

#[test]
fn all_stable_warnings_preserve_pinned_versions_paths_and_exact_text() {
    // Full source-based expected warning strings. Cargo metadata suppresses these
    // on its success path, so its separate executed corpus cannot check them.
    for (name, expected) in [
        ("test-dummy-stable", "the cargo feature `test-dummy-stable` has been stabilized in the 1.0 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/ for more information about using this feature."),
        ("alternative-registries", "the cargo feature `alternative-registries` has been stabilized in the 1.34 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/registries.html for more information about using this feature."),
        ("edition", "the cargo feature `edition` has been stabilized in the 1.31 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field for more information about using this feature."),
        ("rename-dependency", "the cargo feature `rename-dependency` has been stabilized in the 1.31 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#renaming-dependencies-in-cargotoml for more information about using this feature."),
        ("profile-overrides", "the cargo feature `profile-overrides` has been stabilized in the 1.41 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/profiles.html#overrides for more information about using this feature."),
        ("default-run", "the cargo feature `default-run` has been stabilized in the 1.37 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/manifest.html#the-default-run-field for more information about using this feature."),
        ("named-profiles", "the cargo feature `named-profiles` has been stabilized in the 1.57 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/profiles.html#custom-profiles for more information about using this feature."),
        ("resolver", "the cargo feature `resolver` has been stabilized in the 1.51 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/resolver.html#resolver-versions for more information about using this feature."),
        ("strip", "the cargo feature `strip` has been stabilized in the 1.58 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/profiles.html#strip-option for more information about using this feature."),
        ("rust-version", "the cargo feature `rust-version` has been stabilized in the 1.56 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field for more information about using this feature."),
        ("edition2021", "the cargo feature `edition2021` has been stabilized in the 1.56 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field for more information about using this feature."),
        ("workspace-inheritance", "the cargo feature `workspace-inheritance` has been stabilized in the 1.64 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/unstable.html#workspace-inheritance for more information about using this feature."),
        ("edition2024", "the cargo feature `edition2024` has been stabilized in the 1.85 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field for more information about using this feature."),
    ] {
        let allow = [];
        let source = format!("cargo-features=['{name}']");
        let (result, warnings) = inspect(&source, Context {
            allow_features: Some(&allow), nightly_features_allowed: false,
            channel: "stable", ..CONTEXT
        });
        assert_eq!(result, Ok(vec![name.to_owned()]), "{name}");
        assert_eq!(warnings, [expected], "{name}");
    }
}

#[test]
fn diagnostic_channel_is_independent_of_eligibility_and_preserves_warning_order() {
    for (channel, path) in [
        ("nightly", "nightly/"),
        ("dev", "nightly/"),
        ("beta", "beta/"),
        ("stable", ""),
        ("", ""),
        ("Nightly", ""),
        ("custom-雪", ""),
    ] {
        let (result, warnings) = inspect(
            "cargo-features=['strip','edition']",
            Context {
                channel,
                nightly_features_allowed: false,
                ..CONTEXT
            },
        );
        assert_eq!(result, Ok(vec!["strip".into(), "edition".into()]));
        assert_eq!(warnings, [
            format!("the cargo feature `strip` has been stabilized in the 1.58 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/{path}cargo/reference/profiles.html#strip-option for more information about using this feature."),
            format!("the cargo feature `edition` has been stabilized in the 1.31 release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/{path}cargo/reference/manifest.html#the-edition-field for more information about using this feature."),
        ], "{channel:?}");
    }
}
