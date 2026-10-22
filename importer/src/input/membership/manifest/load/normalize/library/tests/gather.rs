use super::*;
use crate::input::membership::manifest::load::{
    convert::{dependencies::gather, source},
    normalize::{tail, validate::Errors},
};

#[test]
fn manifest_source_identity_checks_files_only_for_embedded_paths() {
    let mut history = source::History::new();
    read!(
        "[workspace]",
        vec![present("/p/script.rs"), absent("/p/missing")],
        |_prepared, _owner, session, _limits| {
            let mut sources = history.open(&mut session).unwrap();
            for (path, expected) in [
                ("/p/Cargo.toml", "file:///p"),
                ("/p/script.rs", "file:///p/script.rs"),
                ("/p/missing", "file:///p"),
            ] {
                let id = sources
                    .manifest(std::path::Path::new(path), &mut session)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    sources
                        .get(id, &mut session)
                        .unwrap()
                        .url(&mut session)
                        .unwrap()
                        .as_str(&mut session)
                        .unwrap(),
                    expected
                );
            }
        }
    );
}

#[test]
fn iterative_loader_preserves_parent_across_child_success_and_failure() {
    for (root_text, state) in [
        ("[workspace]", "complete"),
        ("[workspace]\n[workspace.package]\nlicense-file='LICENSE.txt'", "complete-build"),
        ("[workspace]\nmembers=1", "entry-malformed"),
        ("[workspace]\n[package]\nname='p'\nworkspace='elsewhere'", "entry-configuration"),
        ("[workspace]\n[package]\nname='p'\nversion.workspace=true", "entry-normalization"),
        ("[workspace", "entry-syntax"),
        ("[workspace]\nresolver='bad'", "invalid"),
        ("[package]\nname='middle'\nworkspace='inner'\nbuild=false\nreadme.workspace=true", "nested-invalid"),
        ("[workspace]\nmembers=1", "malformed"),
        ("[workspace]\n[package]\nname='child'\nversion.workspace=true", "normalization"),
        ("[workspace]\nmembers=1", "discovered"),
        ("[workspace]", "no-root"),
        ("[workspace]", "cached-invalid"),
        ("[package]\nname='child'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[[bin]]\nname='child'\npath='main.rs'", "wrong-root"),
        ("[workspace]\n[package]\nname='child'\nworkspace='elsewhere'", "configuration"),
        ("[workspace]\n[replace]\n'x@1.2.3'={git='https://example.com/r',artifact=['bin'],target='relative.json'}", "pending"),
    ] {
        use crate::input::membership::manifest::{load::driver, storage::slots};
        let text = "[package]\nname='p'\nworkspace='../root'\nbuild=false\nreadme.workspace=true\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false";
        let text = if state == "complete-build" { format!("{text}\nlicense-file.workspace=true\ninclude=['src/**']\nexclude=['ignored/**']").replace("build=false", "build='build.rs'") } else { text.to_owned() };
        let text = format!("{text}\nedition='2021'\n[[bin]]\nname='app'\npath='app.rs'");
        let discovered = matches!(state, "discovered" | "no-root");
        let text = if discovered { text.replace("workspace='../root'\n", "") } else { text };
        let text = if state == "cached-invalid" { text.replace("readme.workspace=true", "version.workspace=true") } else { text };
        let text = if state.starts_with("entry-") { root_text.to_owned() } else { text };
        let child_path = if discovered { "/Cargo.toml" } else { "/root/Cargo.toml" };
        let mut project = present("/p");
        if let observed::Value::Metadata(metadata) = &mut project.value {
            metadata.kind = observed::Kind::Directory;
        }
        let mut child_directory = present("/root");
        if let observed::Value::Metadata(metadata) = &mut child_directory.value {
            metadata.kind = observed::Kind::Directory;
        }
        let mut paths = slots::Slots::new();
        let mut paths = paths.writer().unwrap();
        let mut history = source::History::new();
        fixture!(
            vec![
                file("/p/Cargo.toml", &text),
                project,
                child_directory,
                present("/root/LICENSE.txt"),
                present("/p/build.rs"),
                absent("/root/README.md"),
                present("/root/README.txt"),
                file(child_path, root_text),
                file("/root/inner/Cargo.toml", "[workspace]\nresolver='bad'"),
                if state == "no-root" { absent("/Cargo.toml") } else { present("/Cargo.toml") },
                observed::Record {
                    query: observed::Query::Environment(b"__CARGO_TEST_ROOT".to_vec()),
                    value: observed::Value::Environment(None),
                },
            ],
            |owner, session, limits| {
                let mut child_bodies = slots::Slots::new();
                let mut child_documents = slots::Slots::new();
                let mut child_originals = slots::Slots::new();
                let mut child_reader = crate::input::membership::manifest::load::read::Reader {
                    bodies: child_bodies.writer().unwrap(),
                    documents: child_documents.writer().unwrap(),
                    originals: child_originals.writer().unwrap(),
                };
                let mut normalized = slots::Slots::new();
                let mut normalized = normalized.writer().unwrap();
                let mut sources = history.open(&mut session).unwrap();
                let source = sources
                    .manifest(std::path::Path::new("/p/Cargo.toml"), &mut session)
                    .unwrap()
                    .unwrap();
                let mut roots = crate::input::membership::manifest::load::roots::Roots::new();
                if state == "cached-invalid" {
                    roots.insert(std::path::Path::new("/root"), crate::input::membership::manifest::load::roots::Root {
                        directory: std::path::Path::new("/root"), declaration: None,
                    }, &mut session).unwrap();
                }
                let mut stack = driver::Stack::new();
                let mut tails = slots::Slots::new();
                let mut tails = tails.writer().unwrap();
                let mut specs = slots::Slots::new();
                let mut specs = specs.writer().unwrap();
                let mut package_records = slots::Slots::new();
                let mut packages =
                    crate::input::membership::manifest::load::convert::package::Owner::from_slots(
                        package_records.writer().unwrap(),
                    );
                let outcome = driver::run(
                    driver::Work::Read(std::path::Path::new("/p/Cargo.toml")),
                    &mut stack,
                    &mut child_reader,
                    &mut paths,
                    &mut normalized,
                    &mut tails,
                    &mut specs,
                    &mut packages,
                    &mut sources,
                    &mut owner,
                    &mut roots,
                    std::path::Path::new("/cargo"),
                    &reader_context(),
                    false,
                    false,
                    true,
                    true,
                    &limits,
                    &mut session,
                );
                if state == "entry-syntax" {
                    assert!(matches!(outcome, Err(Stop::Refused)));
                    assert!(matches!(session.check(), Err(Stop::Refused)));
                    continue;
                }
                if state.starts_with("entry-") {
                    let driver::Outcome::Rejected { cause, contexts } = outcome.unwrap().unwrap() else { panic!("entry rejection") };
                    assert!(matches!(cause, driver::Cause::Rejected(_)));
                    let expected: &[&str] = if state == "entry-malformed" { &[] } else { &["failed to parse manifest at `/p/Cargo.toml`"] };
                    assert_eq!(contexts.as_slice().iter().map(String::as_str).collect::<Vec<_>>(), expected);
                    assert!(stack.pop(&mut session).unwrap().is_none());
                    continue;
                }
                if state == "wrong-root" {
                    let driver::Outcome::Rejected { cause, contexts } = outcome.unwrap().unwrap() else { panic!("wrong root rejection") };
                    let driver::Cause::Message(message) = cause else { panic!("wrong root leaf") };
                    assert_eq!(message, "root of a workspace inferred but wasn't a root: /root/Cargo.toml");
                    assert_eq!(contexts.as_slice().iter().map(String::as_str).collect::<Vec<_>>(), [
                        "error inheriting `readme` from workspace root manifest's `workspace.package.readme`",
                        "failed to parse manifest at `/p/Cargo.toml`",
                    ]);
                    assert!(roots.get(std::path::Path::new("/root/Cargo.toml"), &mut session).unwrap().is_none());
                    assert!(stack.pop(&mut session).unwrap().is_none());
                    continue;
                }
                if state == "cached-invalid" {
                    let driver::Outcome::Rejected { cause, contexts } = outcome.unwrap().unwrap() else { panic!("cached normalization rejection") };
                    let driver::Cause::Rejected(diagnostic) = cause else { panic!("cached normalization leaf") };
                    assert_eq!(diagnostic.tag, Tag::PackageInherited(crate::input::membership::manifest::schema::package::Field::Version));
                    assert_eq!(contexts.as_slice().iter().map(String::as_str).collect::<Vec<_>>(), ["failed to parse manifest at `/p/Cargo.toml`"]);
                    assert!(stack.pop(&mut session).unwrap().is_none());
                    continue;
                }
                if state == "no-root" {
                    let driver::Outcome::Rejected { cause, contexts } = outcome.unwrap().unwrap() else { panic!("search rejection") };
                    let driver::Cause::Message(message) = cause else { panic!("search leaf") };
                    assert_eq!(message, "failed to find a workspace root");
                    assert_eq!(contexts.as_slice().iter().map(String::as_str).collect::<Vec<_>>(), [
                        "error inheriting `readme` from workspace root manifest's `workspace.package.readme`",
                        "failed to parse manifest at `/p/Cargo.toml`",
                    ]);
                    assert!(stack.pop(&mut session).unwrap().is_none());
                    continue;
                }
                if state == "normalization" {
                    let driver::Outcome::Rejected { cause, contexts } = outcome.unwrap().unwrap() else { panic!("normalization rejection") };
                    assert!(matches!(cause, driver::Cause::Rejected(_)));
                    assert_eq!(contexts.as_slice().iter().map(String::as_str).collect::<Vec<_>>(), [
                        "failed to parse manifest at `/root/Cargo.toml`",
                        "error inheriting `readme` from workspace root manifest's `workspace.package.readme`",
                        "failed to parse manifest at `/p/Cargo.toml`",
                    ]);
                    assert!(stack.pop(&mut session).unwrap().is_none());
                    continue;
                }
                if matches!(state, "malformed" | "configuration" | "discovered") {
                    let driver::Outcome::Rejected { cause, contexts } = outcome.unwrap().unwrap() else { panic!("child rejection") };
                    assert!(matches!(cause, driver::Cause::Rejected(_)));
                    assert!(roots.get(std::path::Path::new("/root"), &mut session).unwrap().is_none());
                    let mut expected = Vec::new();
                    if state == "configuration" {
                        expected.push("failed to parse manifest at `/root/Cargo.toml`");
                    }
                    if discovered {
                        expected.push("failed searching for potential workspace\npackage manifest: `/p/Cargo.toml`\ninvalid potential workspace manifest: `/Cargo.toml`\n\nhelp: to avoid searching for a non-existent workspace, add `[workspace]` to the package manifest");
                    }
                    expected.push("error inheriting `readme` from workspace root manifest's `workspace.package.readme`");
                    expected.push("failed to parse manifest at `/p/Cargo.toml`");
                    assert_eq!(contexts.as_slice().iter().map(String::as_str).collect::<Vec<_>>(), expected);
                    assert!(stack.pop(&mut session).unwrap().is_none());
                    continue;
                }
                let outcome = outcome.unwrap().unwrap();
                if !matches!(state, "complete" | "complete-build") {
                    let report = match outcome {
                        driver::Outcome::Conversion(report) => { assert_eq!(state, "pending"); report },
                        driver::Outcome::Failed { report, contexts } => {
                            assert!(matches!(state, "invalid" | "nested-invalid"));
                            let mut expected = Vec::new();
                            if state == "nested-invalid" {
                                expected.extend([
                                    "failed to parse manifest at `/root/inner/Cargo.toml`",
                                    "error inheriting `readme` from workspace root manifest's `workspace.package.readme`",
                                ]);
                            }
                            expected.extend(["failed to parse manifest at `/root/Cargo.toml`",
                                "error inheriting `readme` from workspace root manifest's `workspace.package.readme`",
                                "failed to parse manifest at `/p/Cargo.toml`",
                            ]);
                            assert_eq!(contexts.as_slice().iter().map(String::as_str).collect::<Vec<_>>(), expected);
                            report
                        }
                        _ => panic!("unfinished child must retain conversion"),
                    };
                    assert_eq!(
                        report.owner.manifest(),
                        std::path::Path::new(if state == "nested-invalid" { "/root/inner/Cargo.toml" } else { "/root/Cargo.toml" })
                    );
                    assert!(report.source != source);
                    assert!(report.candidate().is_none());
                    match state {
                        "invalid" | "nested-invalid" => assert!(matches!(report.result, Err(driver::Failure::Virtual(_)))),
                        "pending" => assert!(matches!(report.result, Ok(driver::Output::PendingVirtual))),
                        _ => unreachable!(),
                    }
                    assert!(roots
                        .get(std::path::Path::new("/root/Cargo.toml"), &mut session)
                        .unwrap()
                        .is_none());
                    assert!(roots
                        .get(std::path::Path::new(if state == "nested-invalid" { "/root/inner" } else { "/root" }), &mut session)
                        .unwrap()
                        .is_some());
                    if state == "pending" { assert!(matches!(
                        stack.pop(&mut session).unwrap(),
                        Some(driver::Continuation::Child(_))
                    )); }
                    assert!(stack.pop(&mut session).unwrap().is_none());
                    continue;
                }
                let driver::Outcome::Complete(finished) = outcome else {
                    panic!("top-level manifest must finish")
                };
                let package = finished.package(&mut session).unwrap().unwrap();
                assert_eq!(package.manifest, std::path::Path::new("/p/Cargo.toml"));
                assert_eq!(package.root, std::path::Path::new("/p"));
                assert_eq!(package.readme, Some("../root/README.txt"));
                let selected: &[&[u8]] = &[b"/p/Cargo.toml", b"/p/app.rs"];
                let projection = package.project(selected, true, false, &mut session).unwrap();
                let mut expected: Vec<(&[u8], &[u8])> = vec![(b"Cargo.toml.orig", b"/p/Cargo.toml")];
                if state == "complete-build" { expected.push((b"LICENSE.txt", b"/root/LICENSE.txt")); }
                expected.extend([(b"README.txt".as_slice(), b"/root/README.txt".as_slice()), (b"app.rs".as_slice(), b"/p/app.rs".as_slice())]);
                assert_eq!(projection.physical().collect::<Vec<_>>(), expected);
                if state == "complete-build" {
                    assert_eq!(package.license_file, Some("../root/LICENSE.txt"));
                    assert_eq!(package.include, ["src/**"]);
                    assert_eq!(package.exclude, ["ignored/**"]);
                    assert_eq!(package.build_scripts().collect::<Vec<_>>(), [std::path::Path::new("/p/build.rs")]);
                } else {
                    assert!(package.license_file.is_none());
                    assert!(package.include.is_empty());
                    assert!(package.exclude.is_empty());
                    assert_eq!(package.build_scripts().count(), 0);
                }
                assert!(finished.source == source);
                assert_eq!(
                    finished.owner.manifest(),
                    std::path::Path::new("/p/Cargo.toml")
                );
                let driver::Converted::Real(real) = finished.output else {
                    panic!("real parent")
                };
                assert_eq!(real.targets.ordinary.as_slice()[0].name(), "app");
                assert!(roots
                    .get(std::path::Path::new("/root/Cargo.toml"), &mut session)
                    .unwrap()
                    .is_some());
                assert!(stack.pop(&mut session).unwrap().is_none());
            }
        );
    }
}

#[test]
fn virtual_conversion_orders_profiles_resolver_and_root_requirement() {
    use crate::input::membership::manifest::{
        load::convert::{replace::list::Failure, workspace},
        storage::slots,
    };
    for (text, expected) in [
        ("unused=1\n[workspace]\nresolver='3'\n[replace]\n'x@1.2.3'={git='https://example.com/r'}", None),
        ("", Some("manifest is missing either a `[package]` or a `[workspace]`")),
        ("[profile.dev]\nrustflags=[]", Some("manifest is missing either a `[package]` or a `[workspace]`")),
        ("[workspace]\nresolver='bad'\n[profile.dev]\nrustflags=[]", Some("feature `profile-rustflags` is required")),
    ] {
        let mut history = source::History::new();
        let mut specs = slots::Slots::new();
        let mut specs = specs.writer().unwrap();
        read!(text, Vec::new(), |prepared, owner, session, limits| {
            let mut normalized = slots::Slots::new();
            let mut normalized = normalized.writer().unwrap();
            let crate::input::membership::manifest::load::driver::Progress::Tail { owner: inputs, mut warnings, errors, progress: None } = crate::input::membership::manifest::load::driver::start(prepared, &mut normalized, &mut owner, false, &limits, &mut session).unwrap().unwrap() else { panic!("virtual normalization") };
            let mut sources = history.open(&mut session).unwrap();
            let parent = sources.manifest(std::path::Path::new("/p/Cargo.toml"), &mut session).unwrap().unwrap();
            let mut tails = slots::Slots::new();
            let mut tails = tails.writer().unwrap();
            let mut package_records = slots::Slots::new();
            let mut packages = crate::input::membership::manifest::load::convert::package::Owner::from_slots(package_records.writer().unwrap());
            let progress = crate::input::membership::manifest::load::driver::Progress::Tail { owner: inputs, warnings, errors, progress: None };
            let report = progress.convert(&mut tails, &mut specs, &mut packages, parent, &mut sources, &mut owner, true, true, &limits, &mut session).unwrap();
            if expected.is_none() {
                let candidate = report.candidate().expect("completed virtual candidate");
                assert_eq!(candidate.selected_root(std::path::Path::new("/p/member/Cargo.toml"), &mut session).unwrap().unwrap(), std::path::Path::new("/p/Cargo.toml"));
            } else {
                assert!(report.candidate().is_none());
            }
            assert!(report.source == parent);
            let report = match report.finish(&mut session).unwrap() {
                Ok(finished) => {
                    assert!(expected.is_none());
                    assert!(finished.package(&mut session).unwrap().is_none());
                    assert!(finished.source == parent);
                    let mut roots = crate::input::membership::manifest::load::roots::Roots::new();
                    let root = finished.inheritable(&mut roots, &mut session).unwrap().unwrap();
                    assert_eq!(root.directory, std::path::Path::new("/p"));
                    assert!(roots.get(std::path::Path::new("/p/Cargo.toml"), &mut session).unwrap().is_some());
                    assert!(roots.get(std::path::Path::new("/p"), &mut session).unwrap().is_none());
                    assert_eq!(finished.owner.manifest(), std::path::Path::new("/p/Cargo.toml"));
                    assert_eq!(finished.reports.as_slice().iter().map(|entry| (entry.message.as_str(), entry.critical)).collect::<Vec<_>>(), [("unused manifest key: unused", false)]);
                    let crate::input::membership::manifest::load::driver::Converted::Virtual(value) = finished.output else { panic!("virtual finalization") };
                    assert_eq!(value.resolver, Some(crate::input::membership::manifest::load::convert::resolver::Behavior::V3));
                    assert_eq!(value.replacements.as_slice().len(), 1);
                    assert!(value.replacements.as_slice()[0].dependency.fields().platform().is_none());
                    assert!(value.patches.groups().is_empty());
                    continue;
                }
                Err(report) => report,
            };
            warnings = report.warnings;
            let result = match report.result {
                Ok(crate::input::membership::manifest::load::driver::Output::Virtual(value)) => Ok(workspace::Progress::Complete(value)),
                Err(crate::input::membership::manifest::load::driver::Failure::Virtual(error)) => Err(error),
                _ => panic!("virtual dispatch"),
            };
            if let Some(expected) = expected {
                let Err(Failure::Conversion(error)) = result else { panic!("virtual error") };
                assert!(error.starts_with(expected), "{error}");
            } else {
                let Ok(workspace::Progress::Complete(value)) = result else { panic!("virtual complete") };
                assert_eq!(value.resolver, Some(crate::input::membership::manifest::load::convert::resolver::Behavior::V3));
                assert_eq!(value.replacements.as_slice().len(), 1);
                assert!(value.replacements.as_slice()[0].dependency.fields().platform().is_none());
                assert!(value.patches.groups().is_empty());
                assert_eq!(warnings.as_slice(), ["unused manifest key: unused"]);
            }
        });
    }
}

#[test]
fn final_warnings_preserve_license_then_sorted_unused_keys() {
    use crate::input::membership::manifest::load::{convert::finish, Warnings};
    let text = "z=1\nalias=1\n[package]\nname='p'\nbuild=false\nreadme=false\nlicense='MIT'\nlicense-file='LICENSE'\nunknown=1";
    read!(text, Vec::new(), |prepared, owner, session, limits| {
        let package = complete!(patches!(prepared, owner, session, limits), session);
        let mut warnings = Warnings::new();
        finish::warnings(
            package.package(),
            package.patches().original(),
            &mut warnings,
            &mut session,
        )
        .unwrap();
        assert_eq!(warnings.as_slice().len(), 4);
        assert_eq!(warnings.as_slice()[0], "only one of `license` or `license-file` is necessary\n`license` should be used if the package license can be expressed with a standard SPDX expression.\n`license-file` should be used if the package uses a non-standard license.\nSee https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields for more information.");
        assert_eq!(
            &warnings.as_slice()[1..],
            [
                "unused manifest key: alias\nhelp: alias is a valid .cargo/config.toml key",
                "unused manifest key: package.unknown",
                "unused manifest key: z"
            ]
        );
    });
}

#[test]
fn retained_package_contents_preserve_lists_and_optional_presence() {
    use crate::input::membership::manifest::load::convert::contents;
    for metabuild in ["", "metabuild=[]", "metabuild='builder'"] {
        let text = format!("cargo-features=['metabuild']\n[package]\nname='p'\nbuild=false\nreadme=false\ninclude=['z','a','z']\nexclude=['skip']\nlinks='native'\nim-a-teapot=false\ndefault-run='app'\n{metabuild}\n[package.metadata]\nnested={{values=[1,'two',true]}}");
        read!(&text, Vec::new(), |prepared, owner, session, limits| {
            let package = complete!(patches!(prepared, owner, session, limits), session);
            let retained = contents::retain(package.package().unwrap(), &mut session).unwrap();
            drop(package);
            assert_eq!(retained.include.as_slice(), ["z", "a", "z"]);
            assert_eq!(retained.exclude.as_slice(), ["skip"]);
            assert_eq!(retained.links.as_deref(), Some("native"));
            assert_eq!(retained.teapot, Some(false));
            assert_eq!(retained.default_run.as_deref(), Some("app"));
            assert!(retained.metadata.is_some());
            match metabuild {
                "" => assert!(retained.metabuild.is_none()),
                "metabuild=[]" => assert!(retained.metabuild.unwrap().as_slice().is_empty()),
                _ => assert_eq!(retained.metabuild.unwrap().as_slice(), ["builder"]),
            }
        });
    }
}

#[test]
fn package_default_native_request_precedes_forced_field_admission() {
    use crate::input::membership::manifest::{load::convert::kinds, session::Session};
    struct Account(usize);
    impl rawr_toml::document::Admission for Account {
        type Error = ();
        fn admit(&mut self, request: rawr_toml::document::Request) -> Result<(), ()> {
            if matches!(request, rawr_toml::document::Request::Items(_)) {
                self.0 += 1;
                if self.0 > 1 {
                    return Err(());
                }
            }
            Ok(())
        }
    }
    let text = "cargo-features=['per-package-target']\n[package]\nname='p'\nbuild=false\nreadme=false\ndefault-target='x.json'\nforced-target=' '";
    read!(text, Vec::new(), |prepared, owner, session, limits| {
        let package = complete!(patches!(prepared, owner, session, limits), session);
        let mut account = Account(0);
        let mut session = Session::new(&mut account);
        let result = kinds::convert(package.package(), true, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let kinds::Progress::Pending(kinds::Pending::Default { request, .. }) = result else {
            panic!("default request")
        };
        assert_eq!(request.spelling(), "x.json");
        assert_eq!(account.0, 1);
    });
}

#[test]
fn normalized_publish_policy_preserves_defaults_and_version_requirement() {
    use crate::input::membership::manifest::load::convert::publish;
    for version in ["", "version='1.2.3'\n"] {
        for policy in [
            "",
            "publish=false",
            "publish=true",
            "publish=[]",
            "publish=['z','a','z']",
        ] {
            let text = format!("[package]\nname='p'\nbuild=false\nreadme=false\n{version}{policy}");
            read!(&text, Vec::new(), |prepared, owner, session, limits| {
                let package = complete!(patches!(prepared, owner, session, limits), session);
                let result = publish::convert(package.package().unwrap(), &mut session).unwrap();
                if version.is_empty() && matches!(policy, "publish=true" | "publish=['z','a','z']")
                {
                    assert_eq!(
                        result.err().unwrap(),
                        "`package.publish` requires `package.version` be specified"
                    );
                } else {
                    let actual = result.ok().unwrap();
                    drop(package);
                    let expected = match policy {
                        "publish=true" => None,
                        "" if !version.is_empty() => None,
                        "publish=['z','a','z']" => Some(vec!["z", "a", "z"]),
                        _ => Some(vec![]),
                    };
                    assert_eq!(
                        actual.registries.as_ref().map(|values| values
                            .as_slice()
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()),
                        expected
                    );
                }
            });
        }
    }
}

#[test]
fn metadata_owns_normalized_fields_and_sorted_badges() {
    use crate::input::membership::manifest::{load::convert::metadata, schema::manifest};
    for readme in ["false", "'README.md'"] {
        let text = format!("[package]\nname='p'\nbuild=false\nreadme={readme}\ndescription='description'\nhomepage='home'\ndocumentation='docs'\nauthors=['a','a']\nlicense='MIT'\nlicense-file='LICENSE'\nrepository='repo'\nkeywords=['z','a']\ncategories=['cat']\nlinks='ffi'\nrust-version='1.80'\n[badges.z]\nz='last'\na='first'\n[badges.a]\nx='value'");
        read!(&text, Vec::new(), |prepared, owner, session, limits| {
            let package = complete!(patches!(prepared, owner, session, limits), session);
            let metadata = metadata::retain(
                package.package().unwrap(),
                package.patches().original().get(manifest::Field::Badges),
                &mut session,
            )
            .unwrap();
            drop(package);
            assert_eq!(metadata.description.as_deref(), Some("description"));
            assert_eq!(metadata.homepage.as_deref(), Some("home"));
            assert_eq!(metadata.documentation.as_deref(), Some("docs"));
            assert_eq!(
                metadata.readme.as_deref(),
                if readme == "false" {
                    None
                } else {
                    Some("README.md")
                }
            );
            assert_eq!(metadata.authors.as_slice(), ["a", "a"]);
            assert_eq!(metadata.license.as_deref(), Some("MIT"));
            assert_eq!(metadata.license_file.as_deref(), Some("LICENSE"));
            assert_eq!(metadata.repository.as_deref(), Some("repo"));
            assert_eq!(metadata.keywords.as_slice(), ["z", "a"]);
            assert_eq!(metadata.categories.as_slice(), ["cat"]);
            assert_eq!(metadata.links.as_deref(), Some("ffi"));
            assert_eq!(metadata.rust_version.unwrap().to_string(), "1.80");
            assert_eq!(metadata.badges.as_slice()[0].name, "a");
            assert_eq!(
                metadata.badges.as_slice()[1].values.as_slice(),
                [
                    (String::from("a"), String::from("first")),
                    (String::from("z"), String::from("last"))
                ]
            );
        });
    }
}

#[test]
fn combined_dependencies_preserve_final_platform_for_replace_and_patch() {
    use crate::input::membership::manifest::load::convert::dependencies::all;
    for (patch, duplicate, gated) in [
        (false, "", false),
        (true, "", false),
        (false, "r", false),
        (true, "r", false),
        (false, "other", false),
        (true, "other", false),
        (false, "", true),
        (true, "", true),
        (false, "dev-optional", false),
        (false, "missing-feature", false),
        (false, "run-ok", false),
        (false, "run-missing", false),
        (false, "compile-pending", false),
        (false, "critical", false),
        (false, "teapot", false),
        (false, "no-target", false),
    ] {
        let extra = if patch {
            "[patch.crates-io]\nx={git='https://example.com/r'}"
        } else {
            "[replace]\n'x@1.2.3'={git='https://example.com/r'}"
        };
        let platform_dependency = if duplicate == "dev-optional" {
            String::from("[dev-dependencies]\nd={git='https://example.com/d',optional=true,package='renamed'}\n")
        } else if duplicate.is_empty()
            || duplicate == "missing-feature"
            || duplicate.starts_with("run-")
            || duplicate == "teapot"
            || duplicate == "compile-pending"
            || duplicate == "critical"
        {
            String::new()
        } else {
            format!("[target.'cfg(windows)'.dependencies]\na={{git='https://example.com/{duplicate}',package='renamed'}}\n")
        };
        let version = if patch {
            "version='1.2.3-alpha+build'\npublish=false\n"
        } else {
            ""
        };
        let run = match duplicate {
            "teapot" => "im-a-teapot=false\n",
            "run-ok" => "default-run='app'\n",
            "run-missing" => "default-run='ap'\n",
            "compile-pending" => "default-target='x.json'\n",
            _ => "",
        };
        let text = format!("[package]\nname='p'\n{run}{version}edition='2021'\ndescription='composed package'\nauthors=['author','author']\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[dependencies]\na={{git='https://example.com/r'}}\n[target.'cfg(windows)']\n{platform_dependency}{extra}");
        let text = if duplicate != "no-target" {
            format!("{text}\n[[bin]]\nname='app'\npath='app.rs'")
        } else {
            text
        };
        let text = if matches!(duplicate, "critical" | "compile-pending") {
            format!("{text}\ncrate-type=['rlib']\nproc-macro=true")
        } else {
            text
        };
        let check_cfg = if duplicate == "other" {
            "42"
        } else {
            "['cfg(x)']"
        };
        let text =
            format!("{text}\n[lints.rust.unexpected_cfgs]\nlevel='warn'\ncheck-cfg={check_cfg}");
        let hint = if patch {
            ""
        } else {
            "mostly-unused={nested=['hint',42]}"
        };
        let text = format!("{text}\n[hints]\n{hint}");
        let profile = if gated { "rustflags=[]" } else { "lto=false" };
        let text = format!("{text}\n[profile.dev]\n{profile}");
        let feature = if duplicate == "missing-feature" {
            "absent?/child"
        } else {
            "a/child"
        };
        let text = format!("{text}\n[features]\ndefault-features=[]\nuse=['{feature}']");
        let mut history = source::History::new();
        let mut specs = slots::Slots::new();
        let mut specs = specs.writer().unwrap();
        let mut root = present("/p");
        if let observed::Value::Metadata(metadata) = &mut root.value {
            metadata.kind = observed::Kind::Directory;
        }
        read!(&text, vec![root], |prepared, owner, session, limits| {
            let mut normalized = slots::Slots::new();
            let mut normalized = normalized.writer().unwrap();
            let crate::input::membership::manifest::load::driver::Progress::Tail {
                owner: inputs,
                mut warnings,
                errors,
                progress: Some(tail::Progress::Complete(tail)),
            } = crate::input::membership::manifest::load::driver::start(
                prepared,
                &mut normalized,
                &mut owner,
                false,
                &limits,
                &mut session,
            )
            .unwrap()
            .unwrap()
            else {
                panic!("real normalization")
            };
            let mut sources = history.open(&mut session).unwrap();
            let parent = sources
                .manifest(std::path::Path::new("/p/Cargo.toml"), &mut session)
                .unwrap()
                .ok()
                .unwrap();
            let mut tails = slots::Slots::new();
            let mut tails = tails.writer().unwrap();
            let mut package_records = slots::Slots::new();
            let mut packages =
                crate::input::membership::manifest::load::convert::package::Owner::from_slots(
                    package_records.writer().unwrap(),
                );
            let progress = crate::input::membership::manifest::load::driver::Progress::Tail {
                owner: inputs,
                warnings,
                errors,
                progress: Some(tail::Progress::Complete(tail)),
            };
            let report = progress
                .convert(
                    &mut tails,
                    &mut specs,
                    &mut packages,
                    parent,
                    &mut sources,
                    &mut owner,
                    true,
                    true,
                    &limits,
                    &mut session,
                )
                .unwrap();
            if duplicate == "compile-pending" {
                assert!(report.candidate().is_none());
            }
            if matches!(duplicate, "" | "run-ok" | "critical") && !gated {
                let candidate = report.candidate().expect("completed real candidate");
                assert!(candidate
                    .selected_root(std::path::Path::new("/p/member/Cargo.toml"), &mut session)
                    .unwrap()
                    .is_none());
            }
            assert!(report.source == parent);
            if duplicate == "critical" {
                let finished = report
                    .finish(&mut session)
                    .unwrap()
                    .ok()
                    .expect("finalized real manifest");
                assert_eq!(
                    finished
                        .reports
                        .as_slice()
                        .iter()
                        .filter(|entry| entry.critical)
                        .count(),
                    2
                );
                let mut roots = crate::input::membership::manifest::load::roots::Roots::new();
                let error = finished
                    .inheritable(&mut roots, &mut session)
                    .unwrap()
                    .err()
                    .unwrap();
                assert_eq!(
                    error,
                    "root of a workspace inferred but wasn't a root: /p/Cargo.toml"
                );
                assert!(roots
                    .get(std::path::Path::new("/p/Cargo.toml"), &mut session)
                    .unwrap()
                    .is_none());
                continue;
            }
            warnings = report.warnings;
            if matches!(duplicate, "critical" | "compile-pending") {
                assert_eq!(report.errors.as_slice(), &[
                    "the target `app` is a binary and can't have any crate-types set (currently \"rlib\")",
                    "the target `app` is a binary and can't have `proc-macro` set `true`",
                ]);
            } else {
                assert!(report.errors.as_slice().is_empty());
            }
            let result = match report.result {
                Ok(crate::input::membership::manifest::load::driver::Output::PendingReal(
                    converted,
                )) if duplicate == "compile-pending" => {
                    assert_eq!(converted.targets.ordinary.as_slice()[0].name(), "app");
                    let all::Progress::Compile {
                        pending:
                            crate::input::membership::manifest::load::convert::kinds::Pending::Default {
                                request,
                                ..
                            },
                        ..
                    } = converted.dependencies
                    else {
                        panic!("default compile target must remain pending")
                    };
                    assert_eq!(request.spelling(), "x.json");
                    continue;
                }
                Ok(crate::input::membership::manifest::load::driver::Output::Real(value)) => {
                    Ok(value)
                }
                Err(crate::input::membership::manifest::load::driver::Failure::Real(error)) => {
                    Err(error)
                }
                _ => panic!("real dispatch"),
            };
            let result = match result {
                Ok(converted) => {
                    assert_eq!(converted.targets.ordinary.as_slice()[0].name(), "app");
                    Ok(all::Progress::Complete(converted.dependencies))
                }
                Err(
                    crate::input::membership::manifest::load::convert::real::Failure::Dependency(
                        error,
                    ),
                ) => Err(error),
                Err(
                    crate::input::membership::manifest::load::convert::real::Failure::Validation(
                        error,
                    ),
                ) if duplicate == "no-target" => {
                    assert!(error.starts_with("no targets specified in the manifest"));
                    continue;
                }
                _ => panic!("earlier conversion failed"),
            };
            if duplicate == "other" {
                let Err(crate::input::membership::manifest::load::convert::replace::list::Failure::Conversion(error)) = result else { panic!("source conflict") };
                assert_eq!(error, "Dependency 'a' has different source paths depending on the build target. Each dependency must have a single canonical source path irrespective of build target.");
                continue;
            }
            if duplicate == "dev-optional" {
                let Err(crate::input::membership::manifest::load::convert::replace::list::Failure::Conversion(error)) = result else { panic!("optional dev dependency") };
                assert_eq!(
                    error,
                    "dev-dependencies are not allowed to be optional: `d`"
                );
                continue;
            }
            if gated {
                let Err(crate::input::membership::manifest::load::convert::replace::list::Failure::Conversion(error)) = result else { panic!("profile feature gate") };
                assert!(error.starts_with("feature `profile-rustflags` is required"));
                assert!(error.contains("TEST CARGO"));
                continue;
            }
            if duplicate == "missing-feature" {
                let Err(crate::input::membership::manifest::load::convert::replace::list::Failure::MissingFeatureDependency(error)) = result else { panic!("missing feature dependency") };
                assert_eq!(error.feature, "use");
                assert_eq!(error.value.to_string(), "absent?/child");
                assert_eq!(error.dependency, "absent");
                assert!(error.weak);
                assert!(!warnings
                    .as_slice()
                    .iter()
                    .any(|warning| warning.contains("defines a feature named")));
                continue;
            }
            if duplicate == "run-missing" {
                let Err(crate::input::membership::manifest::load::convert::replace::list::Failure::Conversion(error)) = result else { panic!("missing run") };
                assert_eq!(error, "default-run target `ap` not found\n\nhelp: a target with a similar name exists: `app`");
                assert!(warnings
                    .as_slice()
                    .last()
                    .unwrap()
                    .contains("defines a feature named"));
                continue;
            }
            if duplicate == "teapot" {
                let Err(crate::input::membership::manifest::load::convert::replace::list::Failure::FeatureGate(error)) = result else { panic!("teapot presence gate") };
                assert_eq!(error.context, "the `im-a-teapot` manifest key is unstable and may not work properly in England");
                assert!(error
                    .cause
                    .starts_with("feature `test-dummy-unstable` is required"));
                assert!(warnings
                    .as_slice()
                    .last()
                    .unwrap()
                    .contains("defines a feature named"));
                continue;
            }
            let result = result.ok().unwrap();
            let all::Progress::Complete(value) = result else {
                panic!("complete")
            };
            assert!(value.kinds.default.is_none());
            assert!(value.kinds.forced.is_none());
            let contents = value.contents.unwrap();
            assert!(contents.include.as_slice().is_empty());
            assert!(contents.exclude.as_slice().is_empty());
            assert_eq!(
                contents.default_run.as_deref(),
                if duplicate == "run-ok" {
                    Some("app")
                } else {
                    None
                }
            );
            let value = value.prepared;
            assert_eq!(value.features.entries()[0].name, "default-features");
            assert_eq!(value.features.entries()[1].name, "use");
            assert_eq!(
                value.features.entries()[1].values.as_slice()[0].to_string(),
                "a/child"
            );
            assert_eq!(warnings.as_slice().last().unwrap(), "`[features]` defines a feature named `default-features`\nnote: only a feature named `default` will be enabled by default");
            assert_eq!(
                value.lint_flags.as_slice(),
                ["--warn=unexpected_cfgs", "--check-cfg", "cfg(x)"]
            );
            assert_eq!(
                value.hints.as_ref().unwrap().mostly_unused.is_some(),
                !patch
            );
            let metadata = value.metadata.as_ref().unwrap();
            let identity = value.identity.unwrap();
            assert_eq!(identity.name(), "p");
            assert_eq!(
                (
                    identity.version().major(),
                    identity.version().minor(),
                    identity.version().patch()
                ),
                if patch { (1, 2, 3) } else { (0, 0, 0) }
            );
            assert_eq!(identity.version().pre(), if patch { "alpha" } else { "" });
            assert_eq!(identity.version().build(), if patch { "build" } else { "" });
            assert!(identity.source() == parent);
            assert!(value
                .publish
                .as_ref()
                .unwrap()
                .registries
                .as_ref()
                .unwrap()
                .as_slice()
                .is_empty());
            assert!(metadata.readme.is_none());
            assert_eq!(metadata.description.as_deref(), Some("composed package"));
            assert_eq!(metadata.authors.as_slice(), ["author", "author"]);
            assert_eq!(
                value.ordinary.as_slice().len(),
                if duplicate.is_empty() || matches!(duplicate, "run-ok" | "critical") {
                    1
                } else {
                    2
                }
            );
            let dependency = if patch {
                assert!(value.replacements.as_slice().is_empty());
                value.patches.groups()[0].entries()[0].dependency()
            } else {
                assert!(value.patches.groups().is_empty());
                &value.replacements.as_slice()[0].dependency
            };
            assert_eq!(
                dependency.fields().platform().unwrap().to_string(),
                "cfg(windows)"
            );
        });
    }
}

#[test]
fn replacement_list_keeps_key_order_and_completed_prefix_at_native_boundary() {
    use crate::input::membership::manifest::load::convert::replace::list;
    for pending in [false, true] {
        let last = if pending {
            "'b@1.2'={git='https://example.com/r',artifact=['bin'],target='relative.json'}\n'z@invalid'={git='invalid'}"
        } else {
            "'b@2.3.4'={git='https://example.com/r'}"
        };
        let text = format!(
            "[package]\nname='p'\n[replace]\n'a@1.2.3'={{git='https://example.com/r'}}\n{last}"
        );
        let mut history = source::History::new();
        let mut specs = slots::Slots::new();
        let mut specs = specs.writer().unwrap();
        read!(&text, Vec::new(), |prepared, owner, session, limits| {
            let patches = patches!(prepared, owner, session, limits);
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
            let mut warnings = Warnings::new();
            let result = list::convert(
                patches.original(),
                &mut specs,
                Path::new("/p/Cargo.toml"),
                parent,
                &mut sources,
                &mut owner,
                None,
                true,
                true,
                &limits,
                &mut warnings,
                &mut session,
            )
            .unwrap()
            .ok()
            .unwrap();
            match result {
                list::Progress::Complete(values) => {
                    assert!(!pending);
                    assert_eq!(
                        values
                            .as_slice()
                            .iter()
                            .map(|entry| entry.spec.name())
                            .collect::<Vec<_>>(),
                        ["a", "b"]
                    );
                    assert_eq!(
                        values.as_slice()[1]
                            .dependency
                            .fields()
                            .requirement()
                            .unwrap()
                            .comparators()[0]
                            .major,
                        2
                    );
                }
                list::Progress::Canonical(value) => {
                    assert!(pending);
                    assert_eq!(value.completed_count(), 1);
                    assert_eq!(value.spec().name(), "b");
                    assert_eq!(value.spelling(), "relative.json");
                }
            }
        });
    }
}

#[test]
fn replacement_table_presence_precedes_invalid_key() {
    use crate::input::membership::manifest::{load::convert::replace, Span, Stop};
    for patch in ["", "[patch]\n"] {
        let text = format!("[package]\nname='p'\n{patch}[replace]\n'bad@nope'={{git='invalid'}}");
        read!(&text, Vec::new(), |prepared, owner, session, limits| {
            let patches = patches!(prepared, owner, session, limits);
            let result = replace::check_tables(patches.original(), &mut session).unwrap();
            if patch.is_empty() {
                result.unwrap();
                let span = Some(Span { start: 1, end: 9 });
                let error = replace::key("bad@nope", span, &mut session)
                    .unwrap()
                    .err()
                    .unwrap();
                assert_eq!(error.context, "replacements must specify a valid semver version to replace, but `bad@nope` does not");
                assert_eq!(error.cause.span, span);
                let cause = session.fail::<()>(Error::State);
                assert!(matches!(cause, Err(Stop::Refused)));
                assert!(matches!(
                    replace::key("valid@1.2.3", None, &mut session),
                    Err(Stop::Refused)
                ));
            } else {
                assert_eq!(
                    result.unwrap_err(),
                    "cannot specify both [replace] and [patch]"
                );
            }
        });
    }
}

#[test]
fn replacement_entry_orders_version_checks_warnings_and_native_work() {
    use crate::input::membership::manifest::{
        load::convert::{dependency, replace},
        schema::manifest,
    };
    for (key, fields, expected) in [
        (
            "d@1.2.3",
            "git='https://example.com/r',package='actual',unknown=true",
            "complete",
        ),
        (
            "d@1.2",
            "git='https://example.com/r',unknown=true",
            "version to replace",
        ),
        ("d@1.2", "git='not a url',unknown=true", "invalid url"),
        (
            "d@1.2.3",
            "version='1',git='not a url',unknown=true",
            "cannot specify a version requirement",
        ),
        (
            "d@1.2",
            "git='https://example.com/r',artifact=['bin'],target='relative.json',unknown=true",
            "pending",
        ),
    ] {
        let text = format!("[package]\nname='p'\n[replace]\n'{key}'={{{fields}}}");
        let mut history = source::History::new();
        read!(&text, Vec::new(), |prepared, owner, session, limits| {
            let patches = patches!(prepared, owner, session, limits);
            let Some(manifest::Value::Replace(map)) =
                patches.original().get(manifest::Field::Replace)
            else {
                panic!("replace")
            };
            let declaration = map.entries().next().unwrap().value();
            replace::check_tables(patches.original(), &mut session)
                .unwrap()
                .unwrap();
            let spec = replace::key(key, None, &mut session).unwrap().ok().unwrap();
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
            let mut warnings = Warnings::new();
            let result = replace::entry(
                &spec,
                declaration,
                Path::new("/p/Cargo.toml"),
                parent,
                &mut sources,
                &mut owner,
                None,
                true,
                true,
                &limits,
                &mut warnings,
                &mut session,
            )
            .unwrap();
            match result {
                Ok(dependency::Progress::Complete(value)) => {
                    assert_eq!(expected, "complete");
                    assert_eq!(value.fields().name(), "actual");
                    assert_eq!(
                        value.fields().requirement().unwrap().comparators()[0].patch,
                        Some(3)
                    );
                    assert_eq!(
                        warnings.as_slice(),
                        ["unused manifest key: replace.d.unknown"]
                    );
                }
                Ok(dependency::Progress::Canonical(value)) => {
                    assert_eq!(expected, "pending");
                    assert_eq!(value.spelling(), "relative.json");
                    assert!(warnings.as_slice().is_empty());
                }
                Err(error) => {
                    assert!(error.contains(expected), "{expected}: {error}");
                    assert!(warnings.as_slice().is_empty());
                }
            }
        });
    }
}

#[test]
fn collection_preserves_group_order_and_stops_before_later_dependency() {
    for pending in [false, true] {
        let end = if pending {
            "[build-dependencies]\ncb={git='https://example.com/r',artifact=['bin'],target=' relative.json '}\nz={git='not a url'}"
        } else {
            "[build-dependencies]\ncb={git='https://example.com/r'}\n[target.'cfg(unix)'.dependencies]\npn={git='https://example.com/r'}\n[target.'cfg(unix)'.dev-dependencies]\npd={git='https://example.com/r'}\n[target.'cfg(unix)'.build-dependencies]\npb={git='https://example.com/r'}\n[target.'cfg(windows)']"
        };
        let text = format!("[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[dependencies]\nnormal={{git='https://example.com/r'}}\n[dev-dependencies]\ndev={{git='https://example.com/r'}}\n{end}");
        let mut history = source::History::new();
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
            match gather::convert(
                &tail,
                Path::new("/p/Cargo.toml"),
                parent,
                &mut sources,
                &mut owner,
                true,
                true,
                &limits,
                &mut warnings,
                &mut session,
            )
            .unwrap()
            .ok()
            .unwrap()
            {
                gather::Progress::Canonical(value) => {
                    assert!(pending);
                    assert_eq!(value.completed_count(), 2);
                    assert_eq!(value.spelling(), "relative.json");
                    assert_eq!(value.dependency_name(), "cb");
                    assert_eq!(value.fields().name(), "cb");
                }
                gather::Progress::Complete(values) => {
                    assert!(!pending);
                    assert_eq!(values.platform().unwrap().to_string(), "cfg(windows)");
                    assert_eq!(
                        values
                            .as_slice()
                            .iter()
                            .map(|v| v.fields().name())
                            .collect::<Vec<_>>(),
                        ["normal", "dev", "cb", "pn", "pb", "pd"]
                    );
                    for value in &values.as_slice()[3..] {
                        assert_eq!(value.fields().platform().unwrap().to_string(), "cfg(unix)");
                    }
                }
            }
        });
    }
}

#[test]
fn patch_entry_warns_before_error_or_native_suspension() {
    use crate::input::membership::manifest::load::convert::patch;
    for pending in [false, true] {
        let fields = if pending {
            "artifact=['bin'],target=' relative.json '"
        } else {
            "version='bad+meta'"
        };
        let text = format!("[package]\nname='p'\n[patch.crates-io]\nd={{git='https://example.com/r',unknown=true,{fields}}}");
        let mut history = source::History::new();
        read!(&text, Vec::new(), |prepared, owner, session, limits| {
            let patches = patches!(prepared, owner, session, limits);
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
            let mut warnings = crate::input::membership::manifest::load::Warnings::new();
            let result = patch::entry::convert(
                &patches.patch().unwrap()[0].dependencies()[0],
                "crates-io",
                Path::new("/p/Cargo.toml"),
                parent,
                &mut sources,
                &mut owner,
                None,
                true,
                true,
                &limits,
                &mut warnings,
                &mut session,
            )
            .unwrap();
            assert_eq!(
                warnings.as_slice()[0],
                "unused manifest key: patch.crates-io.d.unknown"
            );
            if pending {
                let Ok(patch::entry::Progress::Canonical(value)) = result else {
                    panic!("pending")
                };
                assert_eq!(value.spelling(), "relative.json");
                assert_eq!(warnings.as_slice().len(), 1);
            } else {
                let Err(error) = result else {
                    panic!("requirement error")
                };
                assert!(error.contains("failed to parse the version requirement `bad+meta`"));
                assert_eq!(warnings.as_slice().len(), 2);
                assert!(warnings.as_slice()[1].contains("includes semver metadata"));
            }
        });
    }
}

#[test]
fn patch_groups_replace_equal_urls_after_conversion_and_stop_at_pending() {
    use crate::input::membership::manifest::load::convert::patch;
    for pending in [false, true] {
        let text = if pending {
            "[package]\nname='p'\n[patch.crates-io]\nfirst={git='https://example.com/r',artifact=['bin'],target=' relative.json '}\n[patch.z]\nlater={git='not a url'}"
        } else {
            "[package]\nname='p'\n[patch.'https://EXAMPLE.com/index']\nfirst={git='https://example.com/r',unknown=true}\n[patch.'https://example.com/index']\nsecond={git='https://example.com/r',unknown=true}"
        };
        let mut history = source::History::new();
        read!(text, Vec::new(), |prepared, owner, session, limits| {
            let patches = patches!(prepared, owner, session, limits);
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
            let mut warnings = crate::input::membership::manifest::load::Warnings::new();
            let result = patch::groups::convert(
                patches.patch(),
                Path::new("/p/Cargo.toml"),
                parent,
                &mut sources,
                &mut owner,
                None,
                true,
                true,
                &limits,
                &mut warnings,
                &mut session,
            )
            .unwrap()
            .ok()
            .unwrap();
            match result {
                patch::groups::Progress::Canonical(value) => {
                    assert!(pending);
                    assert_eq!(value.spelling(), "relative.json");
                }
                patch::groups::Progress::Complete(map) => {
                    assert!(!pending);
                    assert_eq!(map.groups().len(), 1);
                    let group = &map.groups()[0];
                    assert_eq!(
                        group.url().as_str(&mut session).unwrap(),
                        "https://example.com/index"
                    );
                    assert_eq!(group.entries().len(), 1);
                    assert_eq!(group.entries()[0].dependency().fields().name(), "second");
                    assert_eq!(
                        warnings.as_slice(),
                        [
                            "unused manifest key: patch.https://EXAMPLE.com/index.first.unknown",
                            "unused manifest key: patch.https://example.com/index.second.unknown"
                        ]
                    );
                }
            }
        });
    }
}
