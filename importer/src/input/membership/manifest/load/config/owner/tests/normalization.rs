//! Integration starts at the recorded Reader and reaches the actual config owner.
use super::*;
use crate::input::membership::manifest::{
    load::{
        self,
        normalize::{Field, Progress},
        read::Reader,
        roots::{Root, Roots},
    },
    schema::{dependency, primitive},
    Diagnostic, Source, Tag,
};
use std::os::unix::ffi::OsStrExt;

fn environment(name: &str) -> Vec<observed::Record> {
    vec![
        observed::Record {
            query: observed::Query::Environment(format!("CARGO_PATH_BASES_{name}").into_bytes()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::EnvironmentPrefix(
                format!("CARGO_PATH_BASES_{name}_").into_bytes(),
            ),
            value: observed::Value::EnvironmentPrefix(false),
        },
    ]
}
fn reader_context() -> load::Context<'static> {
    load::Context {
        features: features::Context {
            allow_features: None,
            nightly_features_allowed: true,
            channel: "nightly",
            is_local: true,
        },
        cargo_lints: false,
        profiles: crate::input::membership::manifest::load::ProfileContext::fixture(),
    }
}
macro_rules! read {
    ($text:expr, $records:expr, |$prepared:ident, $owner:ident, $session:ident, $limits:ident| $body:block) => {{
        let mut records = $records;
        records.push(file("/p/Cargo.toml", $text));
        fixture!(records, |$owner, $session, $limits| {
            let mut bodies = slots::Slots::new();
            let mut documents = slots::Slots::new();
            let mut originals = slots::Slots::new();
            let mut reader = Reader {
                bodies: bodies.writer().unwrap(),
                documents: documents.writer().unwrap(),
                originals: originals.writer().unwrap(),
            };
            let mut roots = Roots::new();
            let $prepared = reader
                .prepare(
                    Path::new("/p/Cargo.toml"),
                    false,
                    reader_context(),
                    &mut roots,
                    &$limits,
                    &mut $session,
                )
                .unwrap();
            $body
        });
    }};
}
fn text<'a>(dependency: &'a load::normalize::Dependency<'_, '_>) -> &'a str {
    match dependency.get(dependency::Field::Path).unwrap() {
        Field::Path(text) => text,
        Field::Original(primitive::Value::String(text)) => text.as_str(),
        _ => panic!("path"),
    }
}
#[test]
fn absent_empty_and_original_fields_do_not_reach_config() {
    for (source, expected) in [
        ("[workspace]", None),
        ("[workspace]\n[patch]", Some(0)),
        ("[workspace]\n[patch.empty]", Some(1)),
    ] {
        read!(source, Vec::new(), |prepared, owner, session, limits| {
            let Progress::Patches(done) = prepared
                .normalize_patches(&mut owner, &limits, &mut session)
                .unwrap()
            else {
                panic!("unexpected workspace request")
            };
            assert_eq!(done.patch().map(|patch| patch.len()), expected);
            assert!(owner.ordinary.is_none());
        });
    }
    read!(
        "[workspace]\n[patch.z]\nb='1'\na={path='./a', extra={x=1}}",
        Vec::new(),
        |prepared, owner, session, limits| {
            let Progress::Patches(done) = prepared
                .normalize_patches(&mut owner, &limits, &mut session)
                .unwrap()
            else {
                panic!("request")
            };
            let dependencies = done.patch().unwrap()[0].dependencies();
            assert_eq!(dependencies[0].name.as_str(), "a");
            assert_eq!(text(&dependencies[0]), "./a");
            assert_eq!(dependencies[0].extras().count(), 1);
            assert_eq!(dependencies[1].simple(), Some("1"));
            assert!(owner.ordinary.is_none());
        }
    );
}
#[test]
fn sorted_first_patch_error_precedes_feature_and_configuration() {
    read!(
        "[workspace]\n[patch.z]\na={path='x',base='base'}\n[patch.a]\nz={base='base'}",
        Vec::new(),
        |prepared, owner, session, limits| {
            assert!(matches!(
                prepared.normalize_patches(&mut owner, &limits, &mut session),
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::PatchBaseWithoutPath,
                    ..
                }))
            ));
            assert!(owner.ordinary.is_none());
            session.check().unwrap();
        }
    );
    read!(
        "[workspace]\n[patch.a]\na={path='x',base='base'}",
        Vec::new(),
        |prepared, owner, session, limits| {
            assert!(matches!(
                prepared.normalize_patches(&mut owner, &limits, &mut session),
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::Feature(_),
                    ..
                }))
            ));
            assert!(owner.ordinary.is_none());
        }
    );
}
#[test]
fn configured_workspace_shadows_builtin_and_preserves_dots_absolute_and_extras() {
    let mut records = normal("[path-bases]\nworkspace='root/../base'");
    records.extend(environment("WORKSPACE"));
    read!("cargo-features=['path-bases']\n[package]\nname='p'\n[patch.z]\nb={path='/absolute',base='workspace'}\na={path='./child',base='workspace',optional=true, extra='kept'}", records, |prepared, owner, session, limits| {
        let Progress::Patches(done) = prepared.normalize_patches(&mut owner, &limits, &mut session).unwrap() else { panic!("configured workspace must shadow fallback") };
        let dependencies = done.patch().unwrap()[0].dependencies();
        assert_eq!(text(&dependencies[0]), "/p/root/../base/./child");
        assert_eq!(text(&dependencies[1]), "/absolute");
        assert!(dependencies[0].get(dependency::Field::Base).is_none());
        assert!(dependencies[0].fields().all(|(field, _)| field != dependency::Field::Base));
        assert!(matches!(dependencies[0].get(dependency::Field::Optional), Some(Field::Original(primitive::Value::Boolean(true)))));
        assert_eq!(dependencies[0].extras().next().unwrap().0.text.as_str(), "extra");
        assert!(done.workspace().is_none());
    });
}
#[test]
fn lazy_resume_keeps_cursor_and_memoizes_root_for_later_patches_and_package_stage() {
    let mut records = normal("");
    records.extend(environment("WORKSPACE"));
    read!("cargo-features=['path-bases']\n[package]\nname='p'\nworkspace='../root'\n[patch.a]\na='1'\nb={path='./one',base='workspace'}\nc={path='two',base='workspace'}", records, |prepared, owner, session, limits| {
        let Progress::Workspace(request) = prepared.normalize_patches(&mut owner, &limits, &mut session).unwrap() else { panic!("must request inheritance") };
        assert_eq!(request.manifest(), Path::new("/p/Cargo.toml"));
        assert_eq!(request.explicit_root(), Some("../root"));
        let Progress::Patches(done) = request.resume_declared(Root { directory: Path::new("/resolved"), declaration: None }, &mut owner, &limits, &mut session).unwrap() else { panic!("root must be memoized") };
        let dependencies = done.patch().unwrap()[0].dependencies();
        assert_eq!(dependencies.len(), 3);
        assert_eq!(dependencies[0].simple(), Some("1"));
        assert_eq!(text(&dependencies[1]), "/resolved/./one");
        assert_eq!(text(&dependencies[2]), "/resolved/two");
        assert_eq!(done.workspace(), Some(Path::new("/resolved")));
        assert!(dependencies[1].get(dependency::Field::Base).is_none());
    });
}
#[test]
fn current_root_uses_its_own_declaration_without_discovery() {
    let mut records = normal("");
    records.extend(environment("WORKSPACE"));
    read!(
        "cargo-features=['path-bases']\n[workspace]\n[patch.a]\na={path='one',base='workspace'}",
        records,
        |prepared, owner, session, limits| {
            let Progress::Patches(done) = prepared
                .normalize_patches(&mut owner, &limits, &mut session)
                .unwrap()
            else {
                panic!("current root is already known")
            };
            assert_eq!(text(&done.patch().unwrap()[0].dependencies()[0]), "/p/one");
        }
    );
}
#[test]
fn native_workspace_prefix_is_checked_only_after_actual_join() {
    for (child, fails) in [("child", true), ("/absolute", false)] {
        let mut records = normal("");
        records.extend(environment("WORKSPACE"));
        let source = format!("cargo-features=['path-bases']\n[package]\nname='p'\n[patch.a]\na={{path='{child}',base='workspace'}}");
        read!(&source, records, |prepared, owner, session, limits| {
            let Progress::Workspace(request) = prepared
                .normalize_patches(&mut owner, &limits, &mut session)
                .unwrap()
            else {
                panic!("request")
            };
            let result = request.resume_declared(
                Root {
                    directory: Path::new(std::ffi::OsStr::from_bytes(b"/native/\xff")),
                    declaration: None,
                },
                &mut owner,
                &limits,
                &mut session,
            );
            if fails {
                assert!(matches!(result, Err(Stop::Refused)));
                assert!(matches!(
                    session.finish(Ok(())),
                    Err(Error::Source {
                        cause: Source::JoinedDependencyPathUtf8,
                        span: None
                    })
                ));
            } else {
                let Progress::Patches(done) = result.unwrap() else {
                    panic!("completion")
                };
                assert_eq!(
                    text(&done.patch().unwrap()[0].dependencies()[0]),
                    "/absolute"
                );
                session.finish(Ok(())).unwrap();
            }
        });
    }
}
#[test]
fn undefined_and_malformed_effective_config_are_reached_before_workspace() {
    for (config, tag) in [
        ("", Tag::PatchBaseUndefined),
        ("[path-bases]\nbase=4", Tag::ConfigLookup),
    ] {
        let mut records = normal(config);
        records.extend(environment("BASE"));
        read!("cargo-features=['path-bases']\n[package]\nname='p'\n[patch.a]\na={path='one',base='base'}", records, |prepared, owner, session, limits| {
            assert!(matches!(prepared.normalize_patches(&mut owner, &limits, &mut session), Err(Stop::Rejected(Diagnostic { tag: actual, .. })) if actual == tag));
            session.check().unwrap();
        });
    }
}
#[test]
fn an_original_refusal_wins_over_a_pending_native_conversion_and_cleanup() {
    let mut records = normal("");
    records.extend(environment("WORKSPACE"));
    read!("cargo-features=['path-bases']\n[package]\nname='p'\n[patch.a]\na={path='one',base='workspace'}", records, |prepared, owner, session, limits| {
        let Progress::Workspace(request) = prepared.normalize_patches(&mut owner, &limits, &mut session).unwrap() else { panic!("request") };
        let refused: Step<()> = session.fail(Error::Overflow);
        assert!(matches!(refused, Err(Stop::Refused)));
        assert!(matches!(request.resume_declared(Root { directory: Path::new(std::ffi::OsStr::from_bytes(b"/native/\xff")), declaration: None }, &mut owner, &limits, &mut session), Err(Stop::Refused)));
        drop(owner);
        assert!(matches!(session.finish(Ok(())), Err(Error::Overflow)));
    });
}

fn native_budget(
    items: usize,
    bytes: usize,
) -> (
    Result<(), Error<crate::input::membership::Error>>,
    input::Usage,
    crate::exceedance::Record,
) {
    let mut records = normal("");
    records.extend(environment("WORKSPACE"));
    records.push(file("/p/Cargo.toml", "cargo-features=['path-bases']\n[package]\nname='p'\n[patch.a]\na='1'\nb={path='one',base='workspace'}"));
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
    let mut bodies = slots::Slots::new();
    let mut documents = slots::Slots::new();
    let mut originals = slots::Slots::new();
    let mut reader = Reader {
        bodies: bodies.writer().unwrap(),
        documents: documents.writer().unwrap(),
        originals: originals.writer().unwrap(),
    };
    let mut roots = Roots::new();
    let mut session = Session::new(&mut host);
    let result = (|| {
        let prepared = reader.prepare(
            Path::new("/p/Cargo.toml"),
            false,
            reader_context(),
            &mut roots,
            &limits,
            &mut session,
        )?;
        let Progress::Workspace(request) =
            prepared.normalize_patches(&mut owner, &limits, &mut session)?
        else {
            panic!("request")
        };
        request.resume_declared(
            Root {
                directory: Path::new(std::ffi::OsStr::from_bytes(b"/native/\xff")),
                declaration: None,
            },
            &mut owner,
            &limits,
            &mut session,
        )?;
        Ok(())
    })();
    let result = session.finish(result);
    drop(owner);
    drop(roots);
    drop(host);
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (result, allocation.usage(), allocation.exceedances())
}
#[test]
fn exact_original_account_charges_source_cause_admission() {
    use crate::input::membership::manifest::tests::one_short;
    let (result, usage, _) = native_budget(usize::MAX, usize::MAX);
    assert!(matches!(
        result,
        Err(Error::Source {
            cause: Source::JoinedDependencyPathUtf8,
            span: None
        })
    ));
    let (result, exact, exceeded) = native_budget(usage.items, usage.bytes);
    assert!(matches!(
        result,
        Err(Error::Source {
            cause: Source::JoinedDependencyPathUtf8,
            span: None
        })
    ));
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    // The source cause's retained item is the charge past a one-short
    // item target; it is still admitted and recorded, never refused.
    let (result, exact, exceeded) = native_budget(usage.items - 1, usage.bytes);
    assert!(matches!(
        result,
        Err(Error::Source {
            cause: Source::JoinedDependencyPathUtf8,
            span: None
        })
    ));
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        usage.items
    ));
    let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
    assert_eq!(axis.first_requested, usage.items as u64);
    let (result, exact, exceeded) = native_budget(usage.items, usage.bytes - 1);
    assert!(matches!(
        result,
        Err(Error::Source {
            cause: Source::JoinedDependencyPathUtf8,
            span: None
        })
    ));
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes
    ));
}

#[test]
fn environment_relative_base_uses_native_cwd_before_joined_path_conversion() {
    for (child, fails) in [("child", true), ("/absolute", false)] {
        let mut records = normal("");
        records.push(observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_BASE".to_vec()),
            value: observed::Value::Environment(Some(b"root".to_vec())),
        });
        let source = format!(
            "cargo-features=['path-bases']\n[package]\nname='p'\n[patch.a]\na={{path='{child}',base='base'}}"
        );
        read!(&source, records, |prepared, owner, session, limits| {
            owner.context.cwd = Path::new(std::ffi::OsStr::from_bytes(b"/native/\xff"));
            let result = prepared.normalize_patches(&mut owner, &limits, &mut session);
            if fails {
                assert!(matches!(result, Err(Stop::Refused)));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                let later: Step<()> = session.reject(Diagnostic::at(Tag::State, None));
                assert!(matches!(later, Err(Stop::Refused)));
                assert!(matches!(
                    session.finish(Ok(())),
                    Err(Error::Source {
                        cause: Source::JoinedDependencyPathUtf8,
                        span: None
                    })
                ));
            } else {
                let Progress::Patches(done) = result.unwrap() else {
                    panic!("configured base must not request workspace inheritance")
                };
                let dependency = &done.patch().unwrap()[0].dependencies()[0];
                assert_eq!(text(dependency), "/absolute");
                assert!(dependency.get(dependency::Field::Base).is_none());
                assert!(done.workspace().is_none());
                session.finish(Ok(())).unwrap();
            }
        });
    }
}
