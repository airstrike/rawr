use super::*;
use crate::input::membership::manifest::{
    load::features,
    reference,
    schema::{dependency::Inheritable, manifest},
    Stop,
};
use rawr_toml::document;

fn check(
    source: &str,
    edition: u16,
    kind: Kind,
    cli: bool,
) -> (Result<Option<bool>, Tag>, Vec<String>) {
    let document = document::parse(source, 80, &mut reference::Account).unwrap();
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let features = Features::interpret(
        &original,
        features::Context {
            allow_features: None,
            nightly_features_allowed: true,
            channel: "nightly",
            is_local: true,
        },
        &mut warnings,
        &mut session,
    )
    .unwrap();
    let input = Inheritable::interpret(
        value::Input::raw(document.table().get("dep").unwrap()),
        &mut session,
    )
    .unwrap();
    let inherit::Progress::Direct(view) =
        inherit::dependency("dep", &input, Path::new("/p"), edition, &mut session).unwrap()
    else {
        panic!("direct")
    };
    let name = primitive::Name::parse(
        value::Text::Borrowed("dep"),
        crate::input::membership::manifest::scalar::Kind::Package,
        None,
        &mut session,
    )
    .unwrap();
    let mut dependency = Dependency {
        name: &name,
        inherited: view,
        path: None,
        remove_public: false,
    };
    let context = Context {
        package_root: Path::new("/p"),
        edition,
        features: &features,
        public_dependency: cli,
        kind,
    };
    let result = match post_inherit(&mut dependency, &context, &mut warnings, &mut session) {
        Ok(()) => Ok(match dependency.get(Field::Public) {
            Some(inherit::Value::Original(primitive::Value::Boolean(value))) => Some(*value),
            None => None,
            _ => panic!("public"),
        }),
        Err(Stop::Rejected(diagnostic)) => Err(diagnostic.tag),
        Err(Stop::Refused) => panic!("refused"),
    };
    (result, warnings.as_slice().to_vec())
}

#[test]
fn underscore_rejection_precedes_public_and_preserves_warning_wording() {
    let (result, warnings) = check(
        "dep={default_features=false,public=true}",
        2024,
        Kind::Build,
        true,
    );
    assert_eq!(result, Err(Tag::DependencyDefaultFeaturesUnderscore));
    assert!(warnings.is_empty());
    let (_, warnings) = check(
        "dep={default_features=false,default-features=true,public=false}",
        2021,
        Kind::Normal,
        false,
    );
    assert_eq!(warnings, ["`default_features` is redundant with `default-features`, preferring `default-features` in the `dep` dependency", "ignoring `public` on dependency dep, pass `-Zpublic-dependency` to enable support for it"]);
    let (_, warnings) = check("dep={default_features=false}", 2021, Kind::Normal, false);
    assert_eq!(warnings, ["`default_features` is deprecated in favor of `default-features` and will not work in the 2024 edition\n(in the `dep` dependency)"]);
}

#[test]
fn public_matrix_checks_presence_even_when_false_and_feature_or_cli() {
    for public in ["true", "false"] {
        for activated in [false, true] {
            for cli in [false, true] {
                for kind in [Kind::Normal, Kind::Development, Kind::Build] {
                    let source = format!(
                        "{}dep={{public={public}}}",
                        if activated {
                            "cargo-features=['public-dependency']\n"
                        } else {
                            ""
                        }
                    );
                    let (result, warnings) = check(&source, 2021, kind, cli);
                    match kind {
                        Kind::Normal if activated || cli => {
                            assert_eq!(result, Ok(Some(public == "true")))
                        }
                        Kind::Normal => {
                            assert_eq!(result, Ok(None));
                            assert_eq!(warnings.len(), 1);
                        }
                        _ if activated || cli => assert_eq!(result, Err(Tag::DependencyPublicKind)),
                        _ => {
                            assert_eq!(result, Ok(None));
                            assert_eq!(warnings.len(), 1);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn swallowed_feature_diagnostic_never_swallows_resource_refusal() {
    struct Reject;
    impl document::Admission for Reject {
        type Error = &'static str;
        fn admit(&mut self, _: document::Request) -> Result<(), Self::Error> {
            Err("stop")
        }
    }
    let document = document::parse("dep={public=true}", 80, &mut reference::Account).unwrap();
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let features = Features::interpret(
        &original,
        features::Context {
            allow_features: None,
            nightly_features_allowed: true,
            channel: "nightly",
            is_local: true,
        },
        &mut warnings,
        &mut session,
    )
    .unwrap();
    let input = Inheritable::interpret(
        value::Input::raw(document.table().get("dep").unwrap()),
        &mut session,
    )
    .unwrap();
    let name = primitive::Name::parse(
        value::Text::Borrowed("dep"),
        crate::input::membership::manifest::scalar::Kind::Package,
        None,
        &mut session,
    )
    .unwrap();
    let inherit::Progress::Direct(view) =
        inherit::dependency("dep", &input, Path::new("/p"), 2021, &mut session).unwrap()
    else {
        panic!("direct")
    };
    let mut dependency = Dependency {
        name: &name,
        inherited: view,
        remove_public: false,
        path: None,
    };
    let context = Context {
        package_root: Path::new("/p"),
        edition: 2021,
        features: &features,
        public_dependency: true,
        kind: Kind::Normal,
    };
    let mut reject = Reject;
    let mut session = Session::new(&mut reject);
    assert!(matches!(
        post_inherit(&mut dependency, &context, &mut warnings, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Admission("stop"))
    ));
    assert!(warnings.as_slice().is_empty());
}
