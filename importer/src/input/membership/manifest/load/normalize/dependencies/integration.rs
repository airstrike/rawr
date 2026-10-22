//! Concrete dependency cursor through the same config owner used by patches.
use super::*;
use crate::input::membership::manifest::{
    load::{
        normalize::{
            dependencies::{self, Context, Kind, Progress},
            inherit,
        },
        roots::Root,
    },
    schema::{dependency::Field, manifest, workspace},
    Diagnostic, Tag,
};

fn environment() -> Vec<observed::Record> {
    vec![
        observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_WORKSPACE".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::EnvironmentPrefix(b"CARGO_PATH_BASES_WORKSPACE_".to_vec()),
            value: observed::Value::EnvironmentPrefix(false),
        },
    ]
}
macro_rules! setup {
    ($source:expr, $records:expr, |$original:ident, $features:ident, $warnings:ident, $owner:ident, $session:ident, $limits:ident| $body:block) => {{
        let document = document::parse($source, 80, &mut reference::Account).unwrap();
        fixture!($records, |$owner, $session, $limits| {
            let $original =
                Original::interpret(value::Input::root(&document), &mut $session).unwrap();
            let mut $warnings = Warnings::new();
            let $features = Features::interpret(
                &$original,
                features::Context {
                    allow_features: None,
                    nightly_features_allowed: true,
                    channel: "nightly",
                    is_local: true,
                },
                &mut $warnings,
                &mut $session,
            )
            .unwrap();
            $body
        });
    }};
}
fn map<'a, 'd>(
    original: &'a Original<'d>,
) -> Option<&'a crate::input::membership::manifest::schema::dependency::inheritable::Map<'d>> {
    match original.get(manifest::Field::Dependencies) {
        Some(manifest::Value::Dependencies(map)) => Some(map),
        None => None,
        _ => panic!("dependencies"),
    }
}
fn stage<'a, 'd>(features: &'a Features<'a, 'd>) -> Context<'a, 'd> {
    Context {
        package_root: Path::new("/p"),
        edition: 2021,
        features,
        public_dependency: false,
        kind: Kind::Normal,
    }
}

#[test]
fn absent_empty_and_btree_order_do_not_acquire_config() {
    for (source, count) in [
        ("", None),
        ("[dependencies]", Some(0)),
        ("[dependencies]\nz='2'\na={version='1',extra=true}", Some(2)),
    ] {
        setup!(
            source,
            Vec::new(),
            |original, features, warnings, owner, session, limits| {
                let Progress::Complete(done) = dependencies::normalize(
                    map(&original),
                    stage(&features),
                    None,
                    &mut warnings,
                    &mut owner,
                    &limits,
                    &mut session,
                )
                .unwrap() else {
                    panic!("complete")
                };
                assert_eq!(done.dependencies().map(|deps| deps.len()), count);
                if count == Some(2) {
                    let deps = done.dependencies().unwrap();
                    assert_eq!(deps[0].name(), "a");
                    assert_eq!(deps[0].extras().count(), 1);
                    assert_eq!(deps[1].simple(), Some("2"));
                }
                assert!(owner.ordinary.is_none());
            }
        );
    }
}

#[test]
fn configured_base_shadows_workspace_and_retains_lexical_paths() {
    let mut records = normal("[path-bases]\nworkspace='root/../base'");
    records.extend(environment());
    setup!("cargo-features=['path-bases']\n[dependencies]\na={path='./child',base='workspace'}\nb={path='/absolute',base='workspace'}", records, |original, features, warnings, owner, session, limits| {
        let Progress::Complete(done) = dependencies::normalize(map(&original), stage(&features), None, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("config precedes workspace") };
        let deps = done.dependencies().unwrap();
        assert!(matches!(deps[0].get(Field::Path), Some(inherit::Value::Text("/p/root/../base/./child"))));
        assert!(matches!(deps[1].get(Field::Path), Some(inherit::Value::Text("/absolute"))));
        assert!(deps.iter().all(|dep| dep.get(Field::Base).is_none()));
        assert!(done.workspace().is_none());
    });
}

#[test]
fn inheritance_suspension_resumes_same_entry_then_reuses_root_for_base() {
    let root_document = document::parse(
        "[dependencies]\nb={version='1',path='dep',base='workspace'}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let mut records = normal("");
    records.extend(environment());
    setup!("cargo-features=['path-bases']\n[dependencies]\na='0'\nb={workspace=true,features=['member']}\nc={path='next',base='workspace'}", records, |original, features, warnings, owner, session, limits| {
        let declaration = workspace::Workspace::interpret(value::Input::root(&root_document), &mut session).unwrap();
        let Progress::Workspace(request) = dependencies::normalize(map(&original), stage(&features), None, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("inheritance request") };
        assert!(owner.ordinary.is_none());
        let Progress::Complete(done) = request.resume_declared(Root { directory: Path::new("/root"), declaration: Some(&declaration) }, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("root reused") };
        let deps = done.dependencies().unwrap();
        assert_eq!(deps.iter().map(|dep| dep.name()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(deps[0].simple(), Some("0"));
        assert!(matches!(deps[1].get(Field::Path), Some(inherit::Value::Text("/root/dep"))));
        assert!(matches!(deps[2].get(Field::Path), Some(inherit::Value::Text("/root/next"))));
        assert_eq!(done.workspace().unwrap().directory, Path::new("/root"));
    });
}

#[test]
fn direct_base_suspension_does_not_repeat_alias_or_public_warnings() {
    let mut records = normal("");
    records.extend(environment());
    setup!("cargo-features=['path-bases']\n[dependencies]\na={path='child',base='workspace',default_features=false,public=true}", records, |original, features, warnings, owner, session, limits| {
        let Progress::Workspace(request) = dependencies::normalize(map(&original), stage(&features), None, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("base request") };
        assert_eq!(warnings.as_slice().len(), 2);
        let Progress::Complete(done) = request.resume_declared(Root { directory: Path::new("/root"), declaration: None }, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("complete") };
        assert_eq!(warnings.as_slice().len(), 2);
        assert!(done.dependencies().unwrap()[0].get(Field::Public).is_none());
    });
}

#[test]
fn alias_error_precedes_configuration_and_base_feature_error() {
    setup!(
        "[dependencies]\na={path='child',base='workspace',default_features=false}",
        Vec::new(),
        |original, features, warnings, owner, session, limits| {
            let mut context = stage(&features);
            context.edition = 2024;
            assert!(matches!(
                dependencies::normalize(
                    map(&original),
                    context,
                    None,
                    &mut warnings,
                    &mut owner,
                    &limits,
                    &mut session
                ),
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::DependencyDefaultFeaturesUnderscore,
                    ..
                }))
            ));
            assert!(owner.ordinary.is_none());
        }
    );
}

fn bounded(
    items: usize,
    bytes: usize,
) -> (
    Result<(), Error<crate::input::membership::Error>>,
    input::Usage,
    crate::exceedance::Record,
) {
    let mut records = normal("[path-bases]\nworkspace='root'");
    records.extend(environment());
    let image = image(records);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
    let mut host = match host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    ) {
        Ok(host) => host,
        Err(error) => {
            return (
                Err(Error::Host(error)),
                allocation.usage(),
                allocation.exceedances(),
            )
        }
    };
    let mut paths = slots::Slots::new();
    let mut config_documents = slots::Slots::new();
    let files = Files::from_owners(paths.writer().unwrap(), config_documents.writer().unwrap());
    let mut owner = Owner::from_files(files, context());
    let document = document::parse("cargo-features=['path-bases']\n[dependencies]\na={path='child',base='workspace',default_features=false,public=true}", 80, &mut reference::Account).unwrap();
    let mut session = Session::new(&mut host);
    let result = (|| {
        let original = Original::interpret(value::Input::root(&document), &mut session)?;
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
        )?;
        let Progress::Complete(done) = dependencies::normalize(
            map(&original),
            stage(&features),
            None,
            &mut warnings,
            &mut owner,
            &limits,
            &mut session,
        )?
        else {
            panic!("configured")
        };
        assert!(matches!(
            done.dependencies().unwrap()[0].get(Field::Path),
            Some(inherit::Value::Text("/p/root/child"))
        ));
        assert_eq!(warnings.as_slice().len(), 2);
        Ok(())
    })();
    if result.is_err() {
        assert!(session.check().is_err());
        assert!(session.items(1).is_err());
    }
    let result = session.finish(result);
    drop(owner);
    drop(host);
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (result, allocation.usage(), allocation.exceedances())
}

#[test]
fn shared_configured_base_and_warnings_keep_exact_original_budget() {
    use crate::input::membership::manifest::tests::one_short;
    let (result, used, _) = bounded(usize::MAX, usize::MAX);
    result.unwrap();
    let (result, exact, exceeded) = bounded(used.items, used.bytes);
    result.unwrap();
    assert_eq!(exact, used);
    assert!(!exceeded.is_exceeded());
    // One short of an aggregate target admits and records that axis.
    let (result, exact, exceeded) = bounded(used.items - 1, used.bytes);
    result.unwrap();
    assert_eq!(exact, used);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        used.items
    ));
    let (result, exact, exceeded) = bounded(used.items, used.bytes - 1);
    result.unwrap();
    assert_eq!(exact, used);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        used.bytes
    ));
}
