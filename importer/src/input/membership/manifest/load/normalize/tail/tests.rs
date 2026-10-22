//! Tail composition under the existing original Host/config-owner fixture.
use super::*;
use crate::input::membership::manifest::{
    load::{
        normalize::{
            dependencies::Kind,
            tail::{self, Context, Progress},
        },
        roots::Root,
    },
    schema::{manifest, workspace},
    Diagnostic, Tag,
};

macro_rules! setup {
    ($source:expr, |$original:ident, $features:ident, $warnings:ident, $owner:ident, $session:ident, $limits:ident| $body:block) => {{
        let document = document::parse($source, 80, &mut reference::Account).unwrap();
        fixture!(Vec::new(), |$owner, $session, $limits| {
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
fn context_for<'a, 'd>(features: &'a Features<'a, 'd>) -> Context<'a, 'd> {
    Context {
        package_name: "package",
        package_root: Path::new("/p"),
        edition: 2021,
        features,
        public_dependency: false,
    }
}

#[test]
fn absent_and_empty_target_collapse_but_present_platform_and_dependency_maps_remain() {
    for (source, platforms, normal) in [
        ("", None, None),
        ("[dependencies]\n[target]", None, Some(0)),
        ("[target.z]", Some(1), None),
    ] {
        setup!(source, |original,
                        features,
                        warnings,
                        owner,
                        session,
                        limits| {
            let Progress::Complete(done) = tail::normalize(
                &original,
                context_for(&features),
                None,
                &mut warnings,
                &mut owner,
                &limits,
                &mut session,
            )
            .unwrap() else {
                panic!("complete")
            };
            assert_eq!(done.platforms().map(|platforms| platforms.len()), platforms);
            assert_eq!(done.maps().get(Kind::Normal).map(|map| map.len()), normal);
            assert!(done.maps().get(Kind::Development).is_none());
            assert!(done.lints().is_none());
            assert!(done.hints().is_none());
            assert!(done.badges().is_none());
            assert!(owner.ordinary.is_none());
        });
    }
}

#[test]
fn selected_empty_kebab_map_suppresses_old_dependency_lookup() {
    setup!("[dev-dependencies]\n[dev_dependencies]\nignored={workspace=true}\n[target.z.build-dependencies]\n[target.z.build_dependencies]\nignored={workspace=true}", |original, features, warnings, owner, session, limits| {
        let Progress::Complete(done) = tail::normalize(&original, context_for(&features), None, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("old maps must not be evaluated") };
        assert_eq!(done.maps().get(Kind::Development).unwrap().len(), 0);
        assert_eq!(done.platforms().unwrap()[0].maps().get(Kind::Build).unwrap().len(), 0);
        assert_eq!(warnings.as_slice(), ["`dev_dependencies` is redundant with `dev-dependencies`, preferring `dev-dependencies` in the `package` package", "`build_dependencies` is redundant with `build-dependencies`, preferring `build-dependencies` in the `z` platform target"]);
    });
}

#[test]
fn warnings_follow_top_maps_then_sorted_platform_maps_then_lints() {
    setup!("[dependencies]\ntop={public=true}\n[dev_dependencies]\ndev={public=false}\n[build_dependencies]\n[target.z.dev_dependencies]\n[target.a.build_dependencies]\n[lints]", |original, features, warnings, owner, session, limits| {
        let Progress::Complete(done) = tail::normalize(&original, context_for(&features), None, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("complete") };
        assert_eq!(done.platforms().unwrap().iter().map(|platform| platform.name()).collect::<Vec<_>>(), ["a", "z"]);
        assert!(done.lints().is_some());
        let warnings = warnings.as_slice();
        assert_eq!(warnings.len(), 6);
        assert!(warnings[0].starts_with("ignoring `public` on dependency top"));
        assert!(warnings[1].starts_with("`dev_dependencies` is deprecated"));
        assert_eq!(warnings[2], "'public' specifier can only be used on regular dependencies, not dev-dependencies");
        assert!(warnings[3].starts_with("`build_dependencies` is deprecated"));
        assert!(warnings[4].ends_with("(in the `a` platform target)"));
        assert!(warnings[5].ends_with("(in the `z` platform target)"));
    });
}

#[test]
fn dependency_resume_memoizes_root_through_platform_and_lints() {
    let root_document = document::parse(
        "[dependencies]\na='1'\nb='2'\n[lints.rust]\nunsafe_code='forbid'",
        80,
        &mut reference::Account,
    )
    .unwrap();
    setup!("[dependencies]\na={workspace=true}\n[dev_dependencies]\n[target.z.dependencies]\nb={workspace=true}\n[lints]\nworkspace=true", |original, features, warnings, owner, session, limits| {
        let declaration = workspace::Workspace::interpret(value::Input::root(&root_document), &mut session).unwrap();
        let Progress::Workspace(request) = tail::normalize(&original, context_for(&features), None, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("request") };
        assert!(warnings.as_slice().is_empty(), "later alias must not run before normal map completes");
        let Progress::Complete(done) = request.resume_declared(Root { directory: Path::new("/root"), declaration: Some(&declaration) }, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("one root serves tail") };
        assert_eq!(warnings.as_slice().len(), 1);
        assert_eq!(done.maps().get(Kind::Normal).unwrap()[0].name(), "a");
        assert_eq!(done.platforms().unwrap()[0].maps().get(Kind::Normal).unwrap()[0].name(), "b");
        let workspace::Value::Lints(expected) = declaration.get(workspace::Field::Lints).unwrap() else { panic!("lints") };
        assert!(std::ptr::eq(done.lints().unwrap(), expected));
        assert_eq!(done.workspace().unwrap().directory, Path::new("/root"));
    });
}

#[test]
fn lint_resume_does_not_repeat_prior_alias_and_borrows_badges() {
    let root_document = document::parse("[lints]", 80, &mut reference::Account).unwrap();
    setup!("[build_dependencies]\n[lints]\nworkspace=true\n[badges.maintenance]\nstatus='actively-developed'", |original, features, warnings, owner, session, limits| {
        let declaration = workspace::Workspace::interpret(value::Input::root(&root_document), &mut session).unwrap();
        let Progress::Workspace(request) = tail::normalize(&original, context_for(&features), None, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("lint request") };
        assert_eq!(warnings.as_slice().len(), 1);
        let Progress::Complete(done) = request.resume_declared(Root { directory: Path::new("/root"), declaration: Some(&declaration) }, &mut warnings, &mut owner, &limits, &mut session).unwrap() else { panic!("complete") };
        assert_eq!(warnings.as_slice().len(), 1);
        assert!(std::ptr::eq(done.badges().unwrap(), original.get(manifest::Field::Badges).unwrap()));
    });
}

#[test]
fn edition_2024_alias_fails_before_inherited_dependency_or_lint() {
    setup!(
        "[dev_dependencies]\na={workspace=true}\n[lints]\nworkspace=true",
        |original, features, warnings, owner, session, limits| {
            let mut context = context_for(&features);
            context.edition = 2024;
            assert!(matches!(
                tail::normalize(
                    &original,
                    context,
                    None,
                    &mut warnings,
                    &mut owner,
                    &limits,
                    &mut session
                ),
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::DependencyTableUnderscore,
                    ..
                }))
            ));
            assert!(warnings.as_slice().is_empty());
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
    let image = image(Vec::new());
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
    let mut documents = slots::Slots::new();
    let files = Files::from_owners(paths.writer().unwrap(), documents.writer().unwrap());
    let mut owner = Owner::from_files(files, context());
    let document = document::parse(
        "[dependencies]\na='1'\n[dev_dependencies]\n[target.z.dependencies]\nz='2'\n[lints]",
        80,
        &mut reference::Account,
    )
    .unwrap();
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
        let Progress::Complete(done) = tail::normalize(
            &original,
            context_for(&features),
            None,
            &mut warnings,
            &mut owner,
            &limits,
            &mut session,
        )?
        else {
            panic!("complete")
        };
        assert_eq!(done.platforms().unwrap().len(), 1);
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
fn exact_and_one_short_original_budget_covers_tail_retention() {
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
