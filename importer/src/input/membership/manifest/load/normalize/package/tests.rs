//! Connected Reader -> patches -> package tests. The module lives beside package
//! source but is included under the existing configuration-owner fixture module.
use super::*;
use crate::input::membership::manifest::{
    load::{
        self,
        normalize::{
            self,
            package::{Progress, Value},
        },
        read::Reader,
        roots::{Root, Roots},
    },
    schema::{self, package::Field, primitive},
    Diagnostic, Tag,
};

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
    ($text:expr, $records:expr, |$prepared:ident, $owner:ident, $session:ident, $limits:ident| $body:block) => {
        read_at!(
            "/p/Cargo.toml",
            false,
            $text,
            $records,
            |$prepared, $owner, $session, $limits| $body
        )
    };
}
macro_rules! read_at {
    ($path:expr, $script:expr, $text:expr, $records:expr, |$prepared:ident, $owner:ident, $session:ident, $limits:ident| $body:block) => {{
        let mut records = $records;
        records.push(file($path, $text));
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
                    Path::new($path),
                    $script,
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
macro_rules! patches {
    ($prepared:expr, $owner:ident, $session:ident, $limits:ident) => {{
        let normalize::Progress::Patches(done) = $prepared
            .normalize_patches(&mut $owner, &$limits, &mut $session)
            .unwrap()
        else {
            panic!("unexpected patch workspace request")
        };
        done
    }};
}
macro_rules! complete {
    ($patches:expr, $session:ident) => {{
        let Progress::Complete(done) = $patches.normalize_package(&mut $session).unwrap() else {
            panic!("unexpected package workspace request")
        };
        done
    }};
}
fn text<'v>(value: &'v Value<'_, '_>) -> &'v str {
    match value {
        Value::Text(text) => text.as_str(),
        Value::Original(primitive::Value::String(text)) => text.as_str(),
        Value::Original(primitive::Value::Name(name)) => name.as_str(),
        _ => panic!("expected textual package field"),
    }
}
fn absent(name: &str) -> observed::Record {
    observed::Record {
        query: observed::Query::Metadata {
            path: path(name),
            follow: true,
        },
        value: observed::Value::Absent,
    }
}

#[test]
fn connected_aliases_preserve_borrowed_fields_and_replace_all_automatic_flags() {
    for alias in ["package", "project"] {
        let source = format!("[{alias}]\nname='p'\nbuild=false\nreadme=false\nauthors=['one','two']\nautolib=true\nautobins=true\nautoexamples=true\nautotests=true\nautobenches=true\nmetadata={{nested=[1,2]}}");
        read!(&source, Vec::new(), |prepared, owner, session, limits| {
            let original = match prepared
                .original
                .get(schema::manifest::Field::Package)
                .or_else(|| prepared.original.get(schema::manifest::Field::Project))
            {
                Some(schema::manifest::Value::Package(package)) => package,
                _ => panic!("original package"),
            };
            let Some(schema::package::Value::Local(authors)) = original.get(Field::Authors) else {
                panic!("authors")
            };
            let Some(schema::package::Value::Metadata(metadata)) = original.get(Field::Metadata)
            else {
                panic!("metadata")
            };
            let done = complete!(patches!(prepared, owner, session, limits), session);
            let package = done.package().unwrap();
            assert_eq!(text(package.get(Field::Name).unwrap()), "p");
            assert!(
                matches!(package.get(Field::Authors), Some(Value::Original(actual)) if std::ptr::eq(*actual, authors))
            );
            assert!(
                matches!(package.get(Field::Metadata), Some(Value::Metadata(actual)) if std::ptr::eq(*actual, metadata))
            );
            for flag in [
                Field::Autolib,
                Field::Autobins,
                Field::Autoexamples,
                Field::Autotests,
                Field::Autobenches,
            ] {
                assert!(matches!(package.get(flag), Some(Value::Boolean(false))));
                assert_eq!(
                    package.fields().filter(|(field, _)| *field == flag).count(),
                    1
                );
            }
            assert!(matches!(
                package.get(Field::Readme),
                Some(Value::Boolean(false))
            ));
            assert!(matches!(
                package.get(Field::Build),
                Some(Value::Build(normalize::files::Build::Auto(false)))
            ));
            assert!(package.get(Field::Edition).is_none());
            assert!(!package.embedded_edition_defaulted());
            assert!(package.get(Field::CargoFeatures).is_none());
            assert!(done.patches().workspace().is_none());
            assert!(owner.ordinary.is_none());
            session.check().unwrap();
        });
    }
}

#[test]
fn virtual_manifest_skips_package_and_embedded_manifest_synthesizes_defaults() {
    read!(
        "[workspace]",
        Vec::new(),
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session);
            assert!(done.package().is_none());
            assert!(owner.ordinary.is_none());
            session.check().unwrap();
        }
    );
    read_at!(
        "/p/a-b.rs",
        true,
        "fn main() {}",
        vec![
            absent("/p/README.md"),
            absent("/p/README.txt"),
            absent("/p/README")
        ],
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session);
            let package = done.package().unwrap();
            assert_eq!(text(package.get(Field::Name).unwrap()), "a-b");
            assert_eq!(text(package.get(Field::Edition).unwrap()), "2024");
            assert!(package.embedded_edition_defaulted());
            assert!(matches!(
                package.get(Field::Build),
                Some(Value::Build(normalize::files::Build::Auto(false)))
            ));
            assert!(matches!(
                package.get(Field::Readme),
                Some(Value::Boolean(false))
            ));
            session.check().unwrap();
        }
    );
}

#[test]
fn patch_errors_and_package_field_order_win_before_later_predicates() {
    read!(
        "[package]\n[patch.z]\na={base='workspace'}",
        Vec::new(),
        |prepared, owner, session, limits| {
            assert!(matches!(
                prepared.normalize_patches(&mut owner, &limits, &mut session),
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::PatchBaseWithoutPath,
                    ..
                }))
            ));
            session.check().unwrap();
        }
    );
    for (fields, expected) in [
        (
            "version.workspace=true\nrust-version.workspace=true\nedition.workspace=true",
            Tag::PackageInherited(Field::Edition),
        ),
        (
            "version.workspace=true\nrust-version.workspace=true",
            Tag::PackageInherited(Field::RustVersion),
        ),
        ("version.workspace=true", Tag::PackageMissingName),
        (
            "name='p'\nversion.workspace=true\nauthors.workspace=true",
            Tag::PackageInherited(Field::Version),
        ),
    ] {
        let source = format!("[workspace]\n[package]\n{fields}");
        read!(&source, Vec::new(), |prepared, owner, session, limits| {
            let result = patches!(prepared, owner, session, limits).normalize_package(&mut session);
            assert!(
                matches!(result, Err(Stop::Rejected(Diagnostic { tag, .. })) if tag == expected)
            );
            session.check().unwrap(); // Missing build/readme observations were never reached.
        });
    }
}

#[test]
fn build_gate_precedes_readme_and_explicit_values_do_not_probe() {
    read!(
        "[package]\nname='p'\nbuild=['one','two']",
        Vec::new(),
        |prepared, owner, session, limits| {
            assert!(matches!(
                patches!(prepared, owner, session, limits).normalize_package(&mut session),
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::Feature(_),
                    ..
                }))
            ));
            session.check().unwrap();
        }
    );
    for (build, expected) in [
        ("true", "build.rs"),
        ("'one/../two//build.rs'", "two/build.rs"),
    ] {
        let source = format!("[package]\nname='p'\nbuild={build}\nreadme='docs/../README'");
        read!(&source, Vec::new(), |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session);
            let package = done.package().unwrap();
            let Some(Value::Build(normalize::files::Build::Single(actual))) =
                package.get(Field::Build)
            else {
                panic!("single")
            };
            assert_eq!(actual.as_str(), expected);
            assert_eq!(text(package.get(Field::Readme).unwrap()), "docs/../README");
            session.check().unwrap();
        });
    }
    read!("cargo-features=['multiple-build-scripts']\n[package]\nname='p'\nbuild=['one/../raw','two']\nreadme=false", Vec::new(), |prepared, owner, session, limits| {
        let original = match prepared.original.get(schema::manifest::Field::Package).or_else(|| prepared.original.get(schema::manifest::Field::Project)) {
                Some(schema::manifest::Value::Package(package)) => package,
                _ => panic!("original package"),
            };
        let Some(schema::package::Value::Local(primitive::Value::Build(primitive::Build::Multiple(input)))) = original.get(Field::Build) else { panic!("multiple") };
        let done = complete!(patches!(prepared, owner, session, limits), session);
        let Some(Value::Build(normalize::files::Build::Multiple(actual))) = done.package().unwrap().get(Field::Build) else { panic!("multiple output") };
        assert_eq!(actual.as_ptr(), input.as_slice().as_ptr());
        session.check().unwrap();
    });
}

#[test]
fn default_build_then_readme_stop_at_first_file() {
    read!(
        "[package]\nname='p'",
        vec![
            present("/p/build.rs"),
            absent("/p/README.md"),
            present("/p/README.txt")
        ],
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session);
            let package = done.package().unwrap();
            assert!(
                matches!(package.get(Field::Build), Some(Value::Build(normalize::files::Build::Single(actual))) if actual.as_str() == "build.rs")
            );
            assert_eq!(text(package.get(Field::Readme).unwrap()), "README.txt");
            session.check().unwrap(); // README was deliberately not recorded.
        }
    );
    // If build's observation is missing, its host refusal wins before the
    // otherwise immediate missing inherited description diagnostic.
    read!(
        "[workspace]\n[package]\nname='p'\ndescription.workspace=true",
        Vec::new(),
        |prepared, owner, session, limits| {
            assert!(matches!(
                patches!(prepared, owner, session, limits).normalize_package(&mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
        }
    );
}

#[test]
fn inheritance_uses_root_declaration_and_exposes_values_not_markers() {
    read!("[workspace.package]\nedition='2021'\nauthors=['root']\ninclude=['../raw/**']\nreadme='docs/../README'\nlicense-file='licenses/../LICENSE'\n[package]\nname='p'\nbuild=false\nedition.workspace=true\nauthors.workspace=true\ninclude.workspace=true\nreadme.workspace=true\nlicense-file.workspace=true", Vec::new(), |prepared, owner, session, limits| {
        let done = complete!(patches!(prepared, owner, session, limits), session);
        let package = done.package().unwrap();
        assert_eq!(text(package.get(Field::Edition).unwrap()), "2021");
        assert!(!package.embedded_edition_defaulted());
        assert_eq!(text(package.get(Field::Readme).unwrap()), "README");
        assert_eq!(text(package.get(Field::LicenseFile).unwrap()), "LICENSE");
        assert!(matches!(package.get(Field::Include), Some(Value::Original(primitive::Value::Strings(values))) if values.as_slice()[0].as_str() == "../raw/**"));
        assert_eq!(done.patches().workspace(), Some(Path::new("/p")));
        assert!(package.fields().all(|(field, _)| package.get(field).is_some()));
        session.check().unwrap();
    });
}

#[test]
fn inherited_readme_defaults_work_without_workspace_package_and_suspend_lazily() {
    read!(
        "[package]\nname='p'\nworkspace='../root'\nbuild=false\nreadme.workspace=true",
        vec![absent("/root/README.md"), present("/root/README.txt")],
        |prepared, owner, session, limits| {
            let Progress::Workspace(request) = patches!(prepared, owner, session, limits)
                .normalize_package(&mut session)
                .unwrap()
            else {
                panic!("inheritance request")
            };
            assert_eq!(request.manifest(), Path::new("/p/Cargo.toml"));
            assert_eq!(request.explicit_root(), Some("../root"));
            // This exercises the caller-supplied declaration boundary, not a claim
            // that Reader::prepare implements the still-outstanding full loader.
            let Progress::Complete(done) = request
                .resume_declared(
                    Root {
                        directory: Path::new("/root"),
                        declaration: None,
                    },
                    &mut session,
                )
                .unwrap()
            else {
                panic!("resume")
            };
            assert_eq!(
                text(done.package().unwrap().get(Field::Readme).unwrap()),
                "../root/README.txt"
            );
            assert_eq!(done.patches().workspace(), Some(Path::new("/root")));
            session.check().unwrap();
        }
    );
    for source in ["[workspace]\n[package]\nname='p'\nbuild=false\nreadme.workspace=true", "[workspace.package]\nreadme=false\n[package]\nname='p'\nbuild=false\nreadme.workspace=true"] {
        let records = if source.starts_with("[workspace]\n") { vec![absent("/p/README.md"), absent("/p/README.txt"), absent("/p/README")] } else { Vec::new() };
        read!(source, records, |prepared, owner, session, limits| {
            assert!(matches!(patches!(prepared, owner, session, limits).normalize_package(&mut session), Err(Stop::Rejected(Diagnostic { tag: Tag::PackageInherited(Field::Readme), .. }))));
            session.check().unwrap();
        });
    }
}

#[test]
fn patch_workspace_memo_carries_into_package_without_another_request() {
    use crate::input::membership::manifest::load::convert::{patch, platform, source};
    let mut history = source::History::new();
    let mut records = normal("");
    records.extend([
        observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_WORKSPACE".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::EnvironmentPrefix(b"CARGO_PATH_BASES_WORKSPACE_".to_vec()),
            value: observed::Value::EnvironmentPrefix(false),
        },
        present("/root/README.md"),
    ]);
    read!("cargo-features=['path-bases']\n[package]\nname='p'\nbuild=false\nreadme.workspace=true\n[patch.a]\nd={path='dep',base='workspace',unknown='yes'}", records, |prepared, owner, session, limits| {
        let normalize::Progress::Workspace(request) = prepared.normalize_patches(&mut owner, &limits, &mut session).unwrap() else { panic!("patch suspension") };
        let normalize::Progress::Patches(patches) = request.resume_declared(Root { directory: Path::new("/root"), declaration: None }, &mut owner, &limits, &mut session).unwrap() else { panic!("patch resume") };
        let mut sources = history.open(&mut session).unwrap();
        let url = crate::input::membership::manifest::spec::url::Url::parse("file:///p", &mut session).unwrap().unwrap();
        let parent = sources.intern(url, source::Origin::Path, None, None, &mut session).unwrap().ok().unwrap();
        let patch = &patches.patch().unwrap()[0].dependencies()[0];
        let platform = platform::parse("cfg(windows)", &mut session).unwrap().unwrap();
        let mut warnings = crate::input::membership::manifest::load::Warnings::new();
        let patch::entry::Progress::Complete(entry) = patch::entry::convert(
            patch, "a", Path::new("/p/Cargo.toml"), parent, &mut sources, &mut owner,
            Some(&platform), false, false, &limits,
            &mut warnings, &mut session,
        ).unwrap().ok().unwrap() else { panic!("patch conversion") };
        assert_eq!(entry.location(), Path::new("/p/Cargo.toml"));
        assert_eq!(warnings.as_slice(), ["unused manifest key: patch.a.d.unknown"]);
        let converted = entry.dependency();
        assert_eq!(converted.fields().platform().unwrap().to_string(), "cfg(windows)");
        assert_eq!(converted.fields().name(), "d");
        assert_eq!(sources.get(converted.fields().source(), &mut session).unwrap().url(&mut session).unwrap().as_str(&mut session).unwrap(), "file:///root/dep");
        let done = complete!(patches, session);
        assert_eq!(text(done.package().unwrap().get(Field::Readme).unwrap()), "../root/README.md");
        assert_eq!(done.patches().workspace(), Some(Path::new("/root")));
        assert_eq!(done.patches().patch().unwrap()[0].dependencies().len(), 1);
        session.check().unwrap();
    });
}

#[test]
fn original_failure_prevents_resume_and_preserves_first_cause() {
    read!(
        "[package]\nname='p'\nbuild=false\nreadme.workspace=true",
        Vec::new(),
        |prepared, owner, session, limits| {
            let Progress::Workspace(request) = patches!(prepared, owner, session, limits)
                .normalize_package(&mut session)
                .unwrap()
            else {
                panic!("request")
            };
            let refused: Step<()> = session.fail(Error::Overflow);
            assert!(matches!(refused, Err(Stop::Refused)));
            assert!(matches!(
                request.resume_declared(
                    Root {
                        directory: Path::new("/unobserved"),
                        declaration: None
                    },
                    &mut session
                ),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.finish(Ok(())), Err(Error::Overflow)));
        }
    );
}

fn package_budget(
    items: usize,
    bytes: usize,
) -> (
    Result<(), Error<crate::input::membership::Error>>,
    input::Usage,
    crate::exceedance::Record,
) {
    let image = image(vec![file("/p/Cargo.toml", "[workspace.package]\nlicense-file='docs/../LICENSE'\n[package]\nname='p'\nbuild='one/../build.rs'\nreadme=false\nlicense-file.workspace=true")]);
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
        assert_eq!(
            text(done.package().unwrap().get(Field::LicenseFile).unwrap()),
            "LICENSE"
        );
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
fn connected_package_admissions_use_the_original_account() {
    use crate::input::membership::manifest::tests::one_short;
    let (result, usage, _) = package_budget(usize::MAX, usize::MAX);
    result.unwrap();
    let (result, exact, exceeded) = package_budget(usage.items, usage.bytes);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    // One short of an aggregate target admits and records that axis.
    let (result, exact, exceeded) = package_budget(usage.items - 1, usage.bytes);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        usage.items
    ));
    let (result, exact, exceeded) = package_budget(usage.items, usage.bytes - 1);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes
    ));
}

#[path = "../library/tests.rs"]
mod library_normalization;

#[test]
fn embedded_explicit_edition_and_multiple_build_skip_default_and_gate() {
    read_at!(
        "/p/script.rs",
        true,
        "---cargo\n[package]\nedition='2021'\nbuild=['one','two']\nreadme=false\n---\nfn main() {}",
        Vec::new(),
        |prepared, owner, session, limits| {
            let done = complete!(patches!(prepared, owner, session, limits), session);
            let package = done.package().unwrap();
            assert_eq!(text(package.get(Field::Edition).unwrap()), "2021");
            assert!(!package.embedded_edition_defaulted());
            assert!(matches!(
                package.get(Field::Build),
                Some(Value::Build(normalize::files::Build::Auto(false)))
            ));
            session.check().unwrap();
        }
    );
}
