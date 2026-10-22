//! Connected Reader -> patches -> package -> library stage under one account.
use super::*;

#[path = "tests/dependency.rs"]
mod dependency_budget;
#[path = "tests/gather.rs"]
mod dependency_gather;

#[test]
fn connected_post_target_checks_preserve_no_target_warning_and_links_order() {
    use crate::input::membership::manifest::load::normalize::{tail, validate::Errors};
    for (ordinary, build) in [
        (false, "false"),
        (false, "'build.rs'"),
        (true, "false"),
        (true, "'build.rs'"),
    ] {
        let declarations = if ordinary {
            "[lib]\nname='l'\npath='shared.rs'\n[[bin]]\nname='b'\npath='shared.rs'"
        } else {
            ""
        };
        let source=format!("[package]\nname='p'\nedition='2021'\nbuild={build}\nlinks='ffi'\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n{declarations}\n[dependencies]\na={{version='1',extra=1}}");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&source, vec![root], |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            let Some(tail::Progress::Complete(tail)) = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap()
            else {
                panic!("tail")
            };
            let Ok(header) = inputs
                .conversion_header(&mut warnings, &mut session)
                .unwrap()
            else {
                panic!("header")
            };
            let Ok(targets) = inputs
                .convert_targets(&header, &tail, &mut warnings, &mut session)
                .unwrap()
            else {
                panic!("targets")
            };
            let result = inputs
                .validate_targets(&targets, &mut warnings, &mut session)
                .unwrap();
            if !ordinary {
                assert_eq!(result.unwrap_err(),"no targets specified in the manifest\neither src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present");
                assert!(warnings.as_slice().is_empty());
            } else {
                assert_eq!(warnings.as_slice(),["file `/p/shared.rs` found to be present in multiple build targets:\n  * `lib` target `l`\n  * `bin` target `b`"]);
                if build == "false" {
                    assert_eq!(result.unwrap_err(),"package specifies that it links to `ffi` but does not have a custom build script");
                } else {
                    result.unwrap();
                    inputs
                        .validate_dependencies(&mut warnings, &mut session)
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        warnings.as_slice().last().unwrap(),
                        "unused manifest key: dependencies.a.extra"
                    );
                    for dependency in tail.maps().get(crate::input::membership::manifest::load::normalize::dependencies::Kind::Normal).unwrap() {
                        crate::input::membership::manifest::load::convert::dependency::check(
                            dependency, &mut warnings, &mut session,
                        ).unwrap().unwrap();
                    }
                }
            }
        });
    }
}

#[test]
fn connected_build_targets_preserve_disabled_empty_custom_and_metabuild() {
    use crate::input::membership::manifest::load::{
        convert::build::Source,
        normalize::{tail, validate::Errors},
    };
    for (build, meta, deps, expected) in [
        ("false", "", "", None),
        ("[]", "", "", None),
        ("'a/../build.rs'", "", "", Some("build-script-build")),
        ("false", "metabuild=[]\n", "", Some("metabuild-p")),
        (
            "false",
            "metabuild=['maker']\n",
            "[build-dependencies]\nmaker='1'",
            Some("metabuild-p"),
        ),
    ] {
        let source = format!("cargo-features=['metabuild','multiple-build-scripts']\n[package]\nname='p'\nedition='2021'\nbuild={build}\n{meta}readme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n{deps}");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&source, vec![root], |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            let Some(tail::Progress::Complete(tail)) = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap()
            else {
                panic!("tail")
            };
            let Ok(header) = inputs
                .conversion_header(&mut warnings, &mut session)
                .unwrap()
            else {
                panic!("header")
            };
            let Ok(targets) = inputs
                .convert_targets(&header, &tail, &mut warnings, &mut session)
                .unwrap()
            else {
                panic!("targets")
            };
            assert!(targets.ordinary.as_slice().is_empty());
            if let Some(name) = expected {
                assert_eq!(targets.builds.len(), 1);
                let target = &targets.builds.as_slice()[0];
                assert_eq!(target.name(), name);
                assert!(target.settings().for_host());
                assert!(!target.settings().tested());
                if name.starts_with("metabuild") {
                    assert!(matches!(target.source(), Source::Metabuild));
                    assert_eq!(target.settings().edition().text(), "2018");
                } else {
                    assert!(
                        matches!(target.source(),Source::Path(path) if path==Path::new("/p/build.rs"))
                    );
                    assert_eq!(target.settings().edition().text(), "2021");
                }
            } else {
                assert!(targets.builds.as_slice().is_empty());
            }
        });
    }
}

#[test]
fn connected_missing_build_stem_is_terminal_but_metabuild_conflict_wins() {
    use crate::input::membership::manifest::{
        load::{
            convert::target::Failure,
            normalize::{tail, validate::Errors},
        },
        Source,
    };
    for meta in ["", "metabuild=[]\n"] {
        let source=format!("cargo-features=['metabuild']\n[package]\nname='p'\nedition='2021'\nbuild='/'\n{meta}readme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&source, vec![root], |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            let Some(tail::Progress::Complete(tail)) = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap()
            else {
                panic!("tail")
            };
            let Ok(header) = inputs
                .conversion_header(&mut warnings, &mut session)
                .unwrap()
            else {
                panic!("header")
            };
            let result = inputs.convert_targets(&header, &tail, &mut warnings, &mut session);
            if meta.is_empty() {
                assert!(matches!(result, Err(Stop::Refused)));
                assert!(matches!(
                    session.finish::<()>(Err(Stop::Refused)),
                    Err(Error::Source {
                        cause: Source::BuildScriptStem,
                        span: None
                    })
                ));
            } else {
                assert!(
                    matches!(result,Ok(Err(Failure::Semantic(text))) if text=="cannot specify both `metabuild` and `build`")
                );
                session.check().unwrap();
            }
        });
    }
}

#[test]
fn connected_library_crate_rejections_preserve_warning_precedence() {
    use crate::input::membership::manifest::load::{
        convert::target::Failure, normalize::validate::Errors,
    };
    for (types, expected, warning_count) in [
        (
            "['dylib','cdylib','proc-macro']",
            Some("library `l` cannot set the crate type of both `dylib` and `cdylib`"),
            0,
        ),
        (
            "['proc-macro','rlib']",
            Some("cannot mix `proc-macro` crate type with others"),
            1,
        ),
        ("[]", None, 0),
    ] {
        let source = format!("[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[lib]\nname='l'\npath='lib.rs'\nproc-macro=true\ncrate-type={types}");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&source, vec![root], |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            let _tail = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap();
            let Ok(header) = inputs
                .conversion_header(&mut warnings, &mut session)
                .unwrap()
            else {
                panic!("header")
            };
            let result = inputs
                .ordinary_targets(&header, &mut warnings, &mut session)
                .unwrap();
            match expected {
                Some(expected) => {
                    assert!(matches!(result, Err(Failure::Semantic(text)) if text == expected))
                }
                None => {
                    let Ok(targets) = result else {
                        panic!("targets")
                    };
                    assert!(targets.as_slice()[0].crates().is_empty());
                    assert!(targets.as_slice()[0].settings().proc_macro());
                }
            }
            assert_eq!(warnings.as_slice().len(), warning_count);
        });
    }
}

#[test]
fn connected_ordinary_targets_preserve_order_types_and_flags() {
    use crate::input::membership::manifest::load::{
        convert::target::{configure::Kind, Crate, Known},
        normalize::{tail, validate::Errors},
    };
    let source = "[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[lib]\nname='l'\npath='lib.rs'\ncrate-type=['proc-macro']\n[[bin]]\nname='b'\npath='bin.rs'\nrequired-features=[]\n[[example]]\nname='e'\npath='ex.rs'\ncrate-type=['procmacro']\n[[test]]\nname='t'\npath='test.rs'\n[[bench]]\nname='v'\npath='bench.rs'";
    let mut root = present("/p");
    if let observed::Value::Metadata(metadata) = &mut root.value {
        metadata.kind = observed::Kind::Directory;
    }
    read!(source, vec![root], |prepared, owner, session, limits| {
        let done = complete!(patches!(prepared, owner, session, limits), session)
            .normalize_library(&mut session)
            .unwrap()
            .normalize_targets(&mut Errors::new(), &mut session)
            .unwrap()
            .unwrap();
        let (inputs, mut warnings) = done.into_tail();
        assert!(matches!(
            inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap(),
            Some(tail::Progress::Complete(_))
        ));
        let Ok(header) = inputs
            .conversion_header(&mut warnings, &mut session)
            .unwrap()
        else {
            panic!("header")
        };
        let Ok(targets) = inputs
            .ordinary_targets(&header, &mut warnings, &mut session)
            .unwrap()
        else {
            panic!("targets")
        };
        let targets = targets.as_slice();
        assert_eq!(
            targets.iter().map(|t| t.name()).collect::<Vec<_>>(),
            ["l", "b", "e", "t", "v"]
        );
        assert_eq!(
            targets.iter().map(|t| t.kind()).collect::<Vec<_>>(),
            [Kind::Lib, Kind::Bin, Kind::Example, Kind::Test, Kind::Bench]
        );
        assert_eq!(targets[0].path(), Path::new("/p/lib.rs"));
        assert!(matches!(
            targets[0].crates(),
            [Crate::Known(Known::ProcMacro)]
        ));
        assert!(!targets[0].settings().proc_macro());
        assert!(!targets[0].name_inferred());
        assert!(targets[1].required().unwrap().is_empty());
        assert!(targets[1].filename().is_none());
        assert!(targets[2].example_library());
        assert_eq!(targets[2].crates()[0].text(), "proc-macro");
        assert!(!targets[2].settings().tested());
        assert!(!targets[3].settings().benched());
        assert!(!targets[4].settings().tested());
        assert_eq!(warnings.as_slice(), ["library `l` should only specify `proc-macro = true` instead of setting `crate-type`"]);
    });
}

#[test]
fn connected_binary_filename_gate_precedes_duplicates_and_configuration() {
    use crate::input::membership::manifest::load::{
        convert::target::Failure, normalize::validate::Errors,
    };
    for filename in ["", "filename='other'\n"] {
        let source = format!("[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[[bin]]\nname='same'\npath='one.rs'\nedition='bad'\n[[bin]]\nname='same'\npath='two.rs'\n{filename}");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&source, vec![root], |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            let _tail = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap();
            let Ok(header) = inputs
                .conversion_header(&mut warnings, &mut session)
                .unwrap()
            else {
                panic!("header")
            };
            let result = inputs.ordinary_targets(&header, &mut warnings, &mut session);
            if filename.is_empty() {
                assert!(
                    matches!(result, Ok(Err(Failure::Semantic(text))) if text == "found duplicate binary name same, but all binary targets must have a unique name")
                );
            } else {
                assert!(matches!(
                    result,
                    Err(Stop::Rejected(Diagnostic {
                        tag: Tag::Feature(_),
                        ..
                    }))
                ));
            }
            assert!(warnings.as_slice().is_empty());
        });
    }
}

#[test]
fn connected_embedded_restrictions_precede_invalid_resolver() {
    use crate::input::membership::manifest::load::{
        convert::{preamble::Failure, resolver::Behavior},
        normalize::{tail, validate::Errors},
    };
    for (fields, resolver, expected) in [
        ("", "2", None),
        (
            "build=false\n",
            "bad",
            Some("`package.build` is not allowed in embedded manifests"),
        ),
    ] {
        let source = format!("---cargo\n[package]\nname='s'\nreadme=false\n{fields}resolver='{resolver}'\n---\nfn main() {{}}");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read_at!(
            "/p/script.rs",
            true,
            &source,
            vec![root],
            |prepared, owner, session, limits| {
                let done = complete!(patches!(prepared, owner, session, limits), session)
                    .normalize_library(&mut session)
                    .unwrap()
                    .normalize_targets(&mut Errors::new(), &mut session)
                    .unwrap()
                    .unwrap();
                let (inputs, mut warnings) = done.into_tail();
                assert!(matches!(
                    inputs
                        .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                        .unwrap()
                        .unwrap(),
                    Some(tail::Progress::Complete(_))
                ));
                let result = inputs
                    .conversion_header(&mut warnings, &mut session)
                    .unwrap();
                match expected {
                    Some(expected) => {
                        assert!(matches!(result, Err(Failure::Semantic(text)) if text == expected))
                    }
                    None => {
                        let Ok(header) = result else { panic!("header") };
                        assert_eq!(header.edition.text(), "2024");
                        assert_eq!(header.resolver, Some(Behavior::V2));
                    }
                }
            }
        );
    }
}

#[test]
fn connected_real_resolver_uses_declared_workspace_and_conflict_precedence() {
    use crate::input::membership::manifest::load::{
        convert::{preamble::Failure, resolver::Behavior},
        normalize::{tail, validate::Errors},
    };
    for package_resolver in ["", "resolver='bad'\n"] {
        let source = format!("[workspace]\nresolver='3'\n[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n{package_resolver}");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&source, vec![root], |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            assert!(matches!(
                inputs
                    .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                    .unwrap()
                    .unwrap(),
                Some(tail::Progress::Complete(_))
            ));
            let result = inputs
                .conversion_header(&mut warnings, &mut session)
                .unwrap();
            if package_resolver.is_empty() {
                let Ok(header) = result else { panic!("header") };
                assert_eq!(header.resolver, Some(Behavior::V3));
            } else {
                assert!(
                    matches!(result, Err(Failure::Semantic(text)) if text == "cannot specify `resolver` field in both `[workspace]` and `[package]`")
                );
            }
        });
    }
}

#[test]
fn connected_real_preamble_checks_directory_before_edition_and_legacy_project() {
    use crate::input::membership::manifest::load::{
        convert::preamble::Failure,
        normalize::{tail, validate::Errors},
    };
    for (directory, edition, project, expected) in [
        (
            false,
            "2024",
            true,
            Some("package root '/p' is not a directory"),
        ),
        (
            true,
            "2024",
            true,
            Some("`[project]` is not supported as of the 2024 Edition, please use `[package]`"),
        ),
        (true, "2021", true, None),
        (true, "2024", false, None),
        (
            true,
            "future",
            true,
            Some("`[project]` is not supported as of the 2024 Edition, please use `[package]`"),
        ),
    ] {
        let section = if project { "project" } else { "package" };
        let features = if edition == "future" {
            "cargo-features=['unstable-editions']\n"
        } else {
            ""
        };
        let source = format!("{features}[{section}]\nname='p'\nedition='{edition}'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false");
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value
            && directory
        {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&source, vec![root], |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            assert!(matches!(
                inputs
                    .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                    .unwrap()
                    .unwrap(),
                Some(tail::Progress::Complete(_))
            ));
            let result = inputs.preamble(&mut warnings, &mut session).unwrap();
            match expected {
                Some(expected) => {
                    assert!(matches!(result, Err(Failure::Semantic(text)) if text == expected))
                }
                None => {
                    let Ok(selected) = result else {
                        panic!("edition")
                    };
                    assert_eq!(selected.text(), edition);
                }
            }
            assert_eq!(
                warnings
                    .as_slice()
                    .iter()
                    .filter(
                        |text| text.as_str() == "`[project]` is deprecated in favor of `[package]`"
                    )
                    .count(),
                usize::from(project && directory && edition == "2021")
            );
        });
    }
}

#[test]
fn bare_embedded_script_reaches_tail_with_synthesized_binary() {
    use crate::input::membership::manifest::load::normalize::{
        resolve::Kind, tail, validate::Errors,
    };
    for source in ["fn main() {}", "---cargo\n---\nfn main() {}"] {
        read_at!(
            "/p/script.rs",
            true,
            source,
            vec![
                absent("/p/README.md"),
                absent("/p/README.txt"),
                absent("/p/README")
            ],
            |prepared, owner, session, limits| {
                let done = complete!(patches!(prepared, owner, session, limits), session)
                    .normalize_library(&mut session)
                    .unwrap()
                    .normalize_targets(&mut Errors::new(), &mut session)
                    .unwrap()
                    .unwrap();
                let bins = done.groups().unwrap().get(Kind::Bin);
                assert_eq!(bins.len(), 1);
                assert_eq!(bins[0].name(), "script");
                assert_eq!(bins[0].path(), Path::new("script.rs"));
                assert!(bins[0].original().is_none());
                let (inputs, mut warnings) = done.into_tail();
                assert!(matches!(
                    inputs
                        .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                        .unwrap()
                        .unwrap(),
                    Some(tail::Progress::Complete(_))
                ));
            }
        );
    }
}

#[test]
fn connected_tail_keeps_targets_live_across_workspace_resume() {
    use crate::input::membership::manifest::load::normalize::{
        dependencies::Kind, tail, validate::Errors,
    };
    let root_document = document::parse(
        "[dependencies]\na='1'\n[lints.rust]\nunsafe_code='deny'",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let source = "[package]\nname='p'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[[bin]]\nname='run'\npath='run.rs'\n[dependencies]\na={workspace=true}\n[dev_dependencies]\n[lints]\nworkspace=true";
    read!(source, Vec::new(), |prepared, owner, session, limits| {
        let declaration = schema::workspace::Workspace::interpret(
            value::Input::root(&root_document),
            &mut session,
        )
        .unwrap();
        let targets = complete!(patches!(prepared, owner, session, limits), session)
            .normalize_library(&mut session)
            .unwrap()
            .normalize_targets(&mut Errors::new(), &mut session)
            .unwrap()
            .unwrap();
        let (inputs, mut warnings) = targets.into_tail();
        let Some(tail::Progress::Workspace(request)) = inputs
            .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
            .unwrap()
            .unwrap()
        else {
            panic!("workspace request")
        };
        assert!(warnings.as_slice().is_empty());
        let tail::Progress::Complete(done) = request
            .resume_declared(
                Root {
                    directory: Path::new("/root"),
                    declaration: Some(&declaration),
                },
                &mut warnings,
                &mut owner,
                &limits,
                &mut session,
            )
            .unwrap()
        else {
            panic!("root is memoized")
        };
        assert_eq!(done.maps().get(Kind::Normal).unwrap()[0].name(), "a");
        assert!(done.lints().is_some());
        assert_eq!(warnings.as_slice().len(), 1);
        assert!(warnings.as_slice()[0].starts_with("`dev_dependencies` is deprecated"));
        assert_eq!(
            inputs
                .targets()
                .groups()
                .unwrap()
                .get(normalize::resolve::Kind::Bin)[0]
                .name(),
            "run"
        );
    });
}

#[test]
fn connected_virtual_tail_rejects_package_fields_without_dependency_discovery() {
    use crate::input::membership::manifest::load::normalize::validate::Errors;
    for (source, expected) in [
        ("[workspace]", None),
        (
            "[workspace]\n[dependencies]\na={workspace=true}",
            Some("dependencies"),
        ),
    ] {
        read!(source, Vec::new(), |prepared, owner, session, limits| {
            let targets = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = targets.into_tail();
            let result = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap();
            match expected {
                None => assert!(matches!(result, Ok(None))),
                Some(field) => assert!(
                    matches!(result, Err(error) if error == format!("this virtual manifest specifies a `{field}` section, which is not allowed"))
                ),
            }
        });
    }
}

#[test]
fn connected_target_groups_preserve_paths_payloads_and_critical_reports() {
    use crate::input::membership::manifest::load::normalize::{resolve::Kind, validate::Errors};
    let source = "[package]\nname='p'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[[bin]]\nname='run'\npath='src/../run.rs'\ncrate-type=['rlib']\n[[example]]\nname='demo'\npath='./demo.rs'\nharness=false\n[[test]]\nname='check'\npath='check.rs'\n[[bench]]\nname='speed'\npath='speed.rs'";
    read!(source, Vec::new(), |prepared, owner, session, limits| {
        let mut errors = Errors::new();
        let done = complete!(patches!(prepared, owner, session, limits), session)
            .normalize_library(&mut session)
            .unwrap()
            .normalize_targets(&mut errors, &mut session)
            .unwrap()
            .unwrap();
        let groups = done.groups().unwrap();
        for (kind, name, path) in [
            (Kind::Bin, "run", "run.rs"),
            (Kind::Example, "demo", "demo.rs"),
            (Kind::Test, "check", "check.rs"),
            (Kind::Bench, "speed", "speed.rs"),
        ] {
            let entries = groups.get(kind);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name(), name);
            assert_eq!(entries[0].path(), Path::new(path));
            assert!(entries[0].original().is_some());
        }
        assert_eq!(errors.as_slice().len(), 1);
        assert!(errors.as_slice()[0].contains("crate"));
        assert!(done
            .library()
            .package()
            .patches()
            .warnings()
            .as_slice()
            .is_empty());
    });
}

#[test]
fn connected_target_groups_stop_before_later_group_validation() {
    use crate::input::membership::manifest::load::normalize::validate::Errors;
    let source = "[package]\nname='p'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\n[[bin]]\nname='deps'\npath='run.rs'\n[[example]]\nname=''\npath='demo.rs'";
    read!(source, Vec::new(), |prepared, owner, session, limits| {
        let result = complete!(patches!(prepared, owner, session, limits), session)
            .normalize_library(&mut session)
            .unwrap()
            .normalize_targets(&mut Errors::new(), &mut session);
        assert!(
            matches!(result, Err(Stop::Rejected(diagnostic)) if diagnostic.tag == Tag::BinaryConflictingName)
        );
    });
}

#[test]
fn normalized_fast_path_skips_discovery_and_preserves_payloads() {
    read!(
        "[package]\nname='p'\nbuild=false\nreadme=false\nautolib=false",
        Vec::new(),
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap();
            assert!(done.library().is_none());
        }
    );
    read!("[package]\nname='p'\nbuild=false\nreadme=false\nautolib=false\n[features]\nf=['g']\n[lib]\nname='named'\npath='src/../lib.rs'\nproc-macro=false\ncrate-type=['rlib']\nharness=false\nunknown='retained'", Vec::new(), |prepared, owner, session, limits| {
        let original = match prepared.original.get(schema::manifest::Field::Lib).unwrap() { schema::manifest::Value::Target(target) => target, _ => panic!("lib") };
        let features = prepared.original.get(schema::manifest::Field::Features).unwrap();
        let done = complete!(patches!(prepared, owner, session, limits), session).normalize_library(&mut session).unwrap();
        let lib = done.library().unwrap();
        assert_eq!(lib.name(), "named");
        assert_eq!(lib.path(), Path::new("lib.rs"));
        assert!(std::ptr::eq(lib.original().unwrap(), original));
        assert_eq!(lib.original().unwrap().unused()[0].text.as_str(), "unknown");
        assert!(std::ptr::eq(done.features().unwrap(), features));
        assert!(done.package().patches().warnings().as_slice().is_empty());
    });
}

#[test]
fn original_autolib_and_exists_drive_inference_including_directories() {
    for kind in [observed::Kind::File, observed::Kind::Directory] {
        let mut record = present("/p/src/lib.rs");
        if let observed::Value::Metadata(metadata) = &mut record.value {
            metadata.kind = kind;
        }
        read!(
            "[package]\nname='with-hyphen'\nbuild=false\nreadme=false\nautolib=true",
            vec![record],
            |prepared, owner, session, limits| {
                let done = complete!(patches!(prepared, owner, session, limits), session)
                    .normalize_library(&mut session)
                    .unwrap();
                let lib = done.library().unwrap();
                assert_eq!(lib.name(), "with_hyphen");
                assert_eq!(lib.path(), Path::new("src/lib.rs"));
                assert!(lib.original().is_none());
            }
        );
    }
    read!(
        "[package]\nname='p'\nbuild=false\nreadme=false",
        vec![absent("/p/src/lib.rs")],
        |prepared, owner, session, limits| {
            assert!(
                complete!(patches!(prepared, owner, session, limits), session)
                    .normalize_library(&mut session)
                    .unwrap()
                    .library()
                    .is_none()
            );
        }
    );
    // Autolib=false does not suppress inference for an incomplete explicit lib.
    read!(
        "[package]\nname='p'\nbuild=false\nreadme=false\nautolib=false\n[lib]",
        vec![present("/p/src/lib.rs")],
        |prepared, owner, session, limits| {
            assert_eq!(
                complete!(patches!(prepared, owner, session, limits), session)
                    .normalize_library(&mut session)
                    .unwrap()
                    .library()
                    .unwrap()
                    .name(),
                "p"
            );
        }
    );
}

#[test]
fn legacy_path_and_warning_are_edition_ordered() {
    read!(
        "[package]\nname='p'\nbuild=false\nreadme=false\n[lib]\nname='old'",
        vec![absent("/p/src/lib.rs"), present("/p/src/old.rs")],
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap();
            assert_eq!(done.library().unwrap().path(), Path::new("src/old.rs"));
            assert_eq!(done.package().patches().warnings().as_slice(), &["path `src/old.rs` was erroneously implicitly accepted for library `old`,\nplease rename the file to `src/lib.rs` or set lib.path in Cargo.toml"]);
        }
    );
    for edition in ["2018", "2021", "2024"] {
        let source = format!("[package]\nname='p'\nedition='{edition}'\nbuild=false\nreadme=false\n[lib]\nname='old'");
        read!(
            &source,
            vec![absent("/p/src/lib.rs")],
            |prepared, owner, session, limits| {
                let result = complete!(patches!(prepared, owner, session, limits), session)
                    .normalize_library(&mut session);
                assert!(
                    matches!(result, Err(Stop::Rejected(diagnostic)) if diagnostic.tag == Tag::LibraryMissingPath)
                );
                session.check().unwrap();
            }
        );
    }
    // No recorded path can contain NUL; native exists fails before the syscall.
    read!(
        "[package]\nname='p'\nbuild=false\nreadme=false\n[lib]\nname=\"bad\\u0000name\"",
        vec![absent("/p/src/lib.rs")],
        |prepared, owner, session, limits| {
            let result = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session);
            assert!(
                matches!(result, Err(Stop::Rejected(diagnostic)) if diagnostic.tag == Tag::LibraryMissingPath)
            );
            session.check().unwrap();
        }
    );
}

#[test]
fn library_validation_preserves_source_error_and_warning_order() {
    for (name, expected) in [
        (" ", Tag::LibraryEmptyName),
        ("has-hyphen", Tag::LibraryHyphen),
        ("ok", Tag::LibraryProcMacroUnderscore),
    ] {
        let source = format!("[package]\nname='p'\nedition='2024'\nbuild=false\nreadme=false\nautolib=false\n[lib]\nname='{name}'\npath='lib.rs'\nproc_macro=false\ncrate_type=[]");
        read!(&source, Vec::new(), |prepared, owner, session, limits| {
            let result = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session);
            assert!(
                matches!(result, Err(Stop::Rejected(diagnostic)) if diagnostic.tag == expected)
            );
        });
    }
    read!("[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\n[lib]\nname='named'\npath='lib.rs'\ncrate_type=[]\nproc_macro=false\nproc-macro=true", Vec::new(), |prepared, owner, session, limits| {
        let done = complete!(patches!(prepared, owner, session, limits), session).normalize_library(&mut session).unwrap();
        assert_eq!(done.package().patches().warnings().as_slice(), &[
            "`proc_macro` is redundant with `proc-macro`, preferring `proc-macro` in the `named` library target",
            "`crate_type` is deprecated in favor of `crate-type` and will not work in the 2024 edition\n(in the `named` library target)",
        ]);
    });
}

#[test]
fn virtual_package_does_not_normalize_targets_or_features() {
    read!(
        "[workspace]\n[features]\nf=[]\n[lib]\nname='bad-name'",
        Vec::new(),
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap();
            assert!(done.library().is_none());
            assert!(done.features().is_none());
        }
    );
}

#[test]
fn original_refusal_precedes_library_work() {
    read!(
        "[package]\nname='p'\nbuild=false\nreadme=false",
        Vec::new(),
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session);
            let refused: Step<()> = session.fail(Error::Overflow);
            assert!(matches!(refused, Err(Stop::Refused)));
            assert!(matches!(
                done.normalize_library(&mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.finish(Ok(())), Err(Error::Overflow)));
        }
    );
}

fn library_budget(
    items: usize,
    bytes: usize,
) -> (
    Result<(), Error<crate::input::membership::Error>>,
    input::Usage,
    crate::exceedance::Record,
) {
    // Complete explicit targets still probe unless autolib=false. Consuming the
    // complete image below proves this probe occurred, not only that it could.
    let image = image(vec![
        file("/p/Cargo.toml", "[package]\nname='with-hyphen'\nbuild=false\nreadme=false\n[lib]\nname='named'\npath='src/../lib.rs'\nproc_macro=false"),
        absent("/p/src/lib.rs"),
    ]);
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
        let normalize::Progress::Patches(patches) =
            prepared.normalize_patches(&mut owner, &limits, &mut session)?
        else {
            panic!("patches")
        };
        let Progress::Complete(done) = patches.normalize_package(&mut session)? else {
            panic!("package")
        };
        let done = done.normalize_library(&mut session)?;
        assert_eq!(done.library().unwrap().path(), Path::new("lib.rs"));
        assert_eq!(done.package().patches().warnings().as_slice().len(), 1);
        Ok(())
    })();
    let result = session.finish(result);
    drop(owner);
    drop(roots);
    let result = result.and_then(|()| host.finish().map(|_| ()).map_err(Error::Host));
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (result, allocation.usage(), allocation.exceedances())
}

#[test]
fn library_pipeline_retains_the_original_exact_account() {
    use crate::input::membership::manifest::tests::one_short;
    let (result, usage, _) = library_budget(usize::MAX, usize::MAX);
    result.unwrap();
    let (result, exact, exceeded) = library_budget(usage.items, usage.bytes);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    // One short of an aggregate target admits and records that axis.
    let (result, exact, exceeded) = library_budget(usage.items - 1, usage.bytes);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        usage.items
    ));
    let (result, exact, exceeded) = library_budget(usage.items, usage.bytes - 1);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes
    ));
}

#[test]
fn normalized_dependencies_reach_sequential_source_selection() {
    use crate::input::membership::manifest::load::{
        convert::{dependency, source},
        normalize::{dependencies, tail, validate::Errors},
    };
    for git_parent in [false, true] {
        let mut history = source::History::new();
        let text = "[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[dependencies]\na={git='https://github.com/Team/Repo.git#ignored',branch='a%2Fb+literal',package='actual',features=['one','two','one'],default-features=false,optional=true}\nb={path='../sibling'}\nc={version='1',registry-index='https://example.com/index'}\nd='1'\ne={version='1',registry='mirror'}\nf={version='1',registry='crates-io'}";
        let mut records = normal("[registries.mirror]\nindex='https://mirror.example/index'");
        for key in [
            b"CARGO_REGISTRY_INDEX".as_slice(),
            b"CARGO_REGISTRIES_MIRROR_INDEX".as_slice(),
        ] {
            records.push(observed::Record {
                query: observed::Query::Environment(key.to_vec()),
                value: observed::Value::Environment(None),
            });
            let mut prefix = key.to_vec();
            prefix.push(b'_');
            records.push(observed::Record {
                query: observed::Query::EnvironmentPrefix(prefix),
                value: observed::Value::EnvironmentPrefix(false),
            });
        }
        read!(text, records, |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            let Some(tail::Progress::Complete(tail)) = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap()
            else {
                panic!("tail")
            };
            let mut sources = history.open(&mut session).unwrap();
            let parent_url = crate::input::membership::manifest::spec::url::Url::parse(
                if git_parent {
                    "https://example.com/parent"
                } else {
                    "file:///p"
                },
                &mut session,
            )
            .unwrap()
            .unwrap();
            let parent = sources
                .intern(
                    parent_url,
                    if git_parent {
                        source::Origin::Git(source::Git::Rev("parent"))
                    } else {
                        source::Origin::Path
                    },
                    if git_parent {
                        Some(source::Precise::GitUrlFragment("full-parent-precise"))
                    } else {
                        None
                    },
                    None,
                    &mut session,
                )
                .unwrap()
                .ok()
                .unwrap();
            let platform = crate::input::membership::manifest::load::convert::platform::parse(
                "cfg(any(unix,windows))",
                &mut session,
            )
            .unwrap()
            .ok()
            .unwrap();
            for dep in tail.maps().get(dependencies::Kind::Normal).unwrap() {
                let retained = dependency::before_artifact(
                    dep,
                    Path::new("/p/Cargo.toml"),
                    parent,
                    &mut sources,
                    &mut owner,
                    dependencies::Kind::Normal,
                    Some(&platform),
                    &limits,
                    &mut warnings,
                    &mut session,
                )
                .unwrap()
                .ok()
                .unwrap();
                let id = retained.source();
                assert_eq!(
                    retained.promoted_version(),
                    if dep.name() == "d" { Some("1") } else { None }
                );
                assert_eq!(
                    retained.platform().unwrap().to_string(),
                    "cfg(any(unix, windows))"
                );
                assert!(matches!(retained.kind(), dependencies::Kind::Normal));
                assert!(!retained.public());
                if dep.name() == "a" {
                    assert_eq!(retained.name(), "actual");
                    assert_eq!(retained.rename(), Some("a"));
                    assert_eq!(retained.features(), ["one", "two", "one"]);
                    assert!(!retained.defaults());
                    assert!(retained.optional());
                } else {
                    assert_eq!(retained.name(), dep.name());
                    assert_eq!(retained.rename(), None);
                    assert!(retained.defaults());
                    assert!(!retained.optional());
                }
                assert_eq!(
                    retained.requirement().is_some(),
                    matches!(dep.name(), "c" | "d" | "e" | "f")
                );
                assert_eq!(
                    retained.registry().is_some(),
                    matches!(dep.name(), "c" | "e" | "f")
                );
                if let Some(registry) = retained.registry() {
                    assert!(registry == id);
                }
                let record = sources.get(id, &mut session).unwrap();
                match dep.name() {
                    "a" => {
                        assert_eq!(
                            record.kind(),
                            source::Kind::Git(source::Git::Branch("a%2Fb+literal"))
                        );
                        assert_eq!(
                            record
                                .url(&mut session)
                                .unwrap()
                                .as_str(&mut session)
                                .unwrap(),
                            "https://github.com/Team/Repo.git#ignored"
                        );
                    }
                    "b" if git_parent => assert!(id == parent),
                    "b" => assert_eq!(
                        record
                            .url(&mut session)
                            .unwrap()
                            .as_str(&mut session)
                            .unwrap(),
                        "file:///sibling"
                    ),
                    "c" => assert_eq!(record.kind(), source::Kind::Registry),
                    "d" | "f" => {
                        assert_eq!(record.kind(), source::Kind::Registry);
                        assert_eq!(record.label(), Some(source::Label::Registry("crates-io")));
                    }
                    "e" => {
                        assert_eq!(record.label(), Some(source::Label::Registry("mirror")));
                        assert_eq!(
                            record
                                .url(&mut session)
                                .unwrap()
                                .as_str(&mut session)
                                .unwrap(),
                            "https://mirror.example/index"
                        );
                    }
                    _ => panic!("dependency"),
                }
            }
            assert_eq!(warnings.as_slice(), ["URL fragment `#ignored` in git URL is ignored for dependency (a). If you were trying to specify a specific git revision, use `rev = \"ignored\"` in the dependency declaration."]);
        });
    }
}

#[test]
fn normalized_dependency_prefix_failure_precedes_source_ambiguity() {
    use crate::input::membership::manifest::load::{
        convert::{dependency, source},
        normalize::{dependencies, tail, validate::Errors},
    };
    for (fields, expected) in [
        (
            "git='not a url',path='local',features=['bad/feature']",
            "feature `bad/feature`",
        ),
        (
            "git='not a url',path='local',registry='named',registry-index='bad'",
            "Only one of `git` or `path`",
        ),
        (
            "git='not a url',branch='b',tag='t'",
            "Only one of `branch`, `tag` or `rev`",
        ),
        (
            "git='not a url',branch='b',tag='t',registry='named',registry-index='bad'",
            "Only one of `registry` or `registry-index`",
        ),
        (
            "git='https://example.com/repo',version='bad+meta',package='actual',registry='unobserved',artifact=['bin']",
            "failed to parse the version requirement `bad+meta` for dependency `actual`: unexpected character 'b' while parsing major version number",
        ),
        (
            "git='not a url',version='bad',package='actual'",
            "invalid url `not a url`",
        ),
        (
            "git='https://example.com/repo',version='1.0.0+meta',registry-index='bad'",
            "invalid url `bad`",
        ),
    ] {
        let mut history = source::History::new();
        let text = format!("[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[dependencies]\na={{{fields}}}");
        read!(&text, Vec::new(), |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session)
                .normalize_library(&mut session)
                .unwrap()
                .normalize_targets(&mut Errors::new(), &mut session)
                .unwrap()
                .unwrap();
            let (inputs, mut warnings) = done.into_tail();
            let Some(tail::Progress::Complete(tail)) = inputs
                .normalize(false, &mut warnings, &mut owner, &limits, &mut session)
                .unwrap()
                .unwrap()
            else {
                panic!("tail")
            };
            let mut sources = history.open(&mut session).unwrap();
            let url = crate::input::membership::manifest::spec::url::Url::parse(
                "file:///p",
                &mut session,
            )
            .unwrap()
            .unwrap();
            let parent = sources
                .intern(url, source::Origin::Path, None, None, &mut session)
                .unwrap()
                .ok()
                .unwrap();
            let dep = &tail.maps().get(dependencies::Kind::Normal).unwrap()[0];
            let error = dependency::convert(
                dep,
                Path::new("/p/Cargo.toml"),
                parent,
                &mut sources,
                &mut owner,
                dependencies::Kind::Normal,
                None,
                false,
                false,
                &limits,
                &mut warnings,
                &mut session,
            )
            .unwrap()
            .err()
            .unwrap();
            assert!(error.contains(expected), "{error}");
            if fields.contains("bad+meta") {
                assert_eq!(error, expected);
                assert_eq!(warnings.as_slice(), ["version requirement `bad+meta` for dependency `a` includes semver metadata which will be ignored, removing the metadata is recommended to avoid confusion"]);
            }
            if fields.contains("1.0.0+meta") {
                assert_eq!(warnings.as_slice().len(), 1);
            }
            let _: Step<()> = session.fail(Error::Overflow);
            assert!(dependency::select(
                dep,
                Path::new("/p/Cargo.toml"),
                parent,
                &mut sources,
                &mut owner,
                &limits,
                &mut warnings,
                &mut session
            )
            .is_err());
        });
    }
}
