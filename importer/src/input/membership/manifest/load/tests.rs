use std::path::Path;

use rawr_toml::document;

use super::{
    lints::Context,
    workspace::{self, Configuration},
    Warnings,
};
use crate::input::membership::manifest::{
    reference, schema::manifest::Original, session, storage, value, Error, Stop, Tag,
};

const CONTEXT: Context = Context {
    cargo_lints: false,
    nightly_features_allowed: true,
};

/// Whether the manifest is a workspace root and a member's root, or the failure's tag.
type Inspected = Result<(bool, Option<String>), Tag>;

fn inspect(source: &str, embedded: bool, context: Context) -> (Inspected, Vec<String>) {
    let document = document::parse(source, 80, &mut reference::Account).unwrap();
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let result = workspace::configure(
        &original,
        Path::new("/project/Cargo.toml"),
        embedded,
        context,
        &mut warnings,
        &mut session,
    );
    let projected = match result {
        Ok(Configuration::Root {
            directory,
            declaration,
        }) => {
            assert_eq!(directory, Path::new("/project"));
            assert_eq!(declaration.is_none(), embedded);
            Ok((true, None))
        }
        Ok(Configuration::Member { root }) => {
            Ok((false, root.map(|text| text.as_str().to_owned())))
        }
        Err(Stop::Rejected(error)) => Err(error.tag),
        Err(Stop::Refused) => panic!("unexpected refusal"),
    };
    (projected, warnings.as_slice().to_vec())
}

#[test]
fn workspace_and_legacy_aliases_preserve_package_precedence_and_embedded_early_return() {
    for (source, expected) in [
        ("", Ok((false, None))),
        (
            "project={name='old',workspace='..'}",
            Ok((false, Some("..".into()))),
        ),
        (
            "package={name='new'}\nproject={name='old',workspace='..'}",
            Ok((false, None)),
        ),
        (
            "workspace={}\nproject={name='old',workspace='..'}",
            Err(Tag::WorkspaceConflict),
        ),
        (
            "workspace={}\npackage={name='new'}\nproject={name='old',workspace='..'}",
            Ok((true, None)),
        ),
    ] {
        let (actual, warnings) = inspect(source, false, CONTEXT);
        assert_eq!(actual, expected, "{source}");
        assert!(warnings.is_empty());
    }
    let source = "package={name='pkg',workspace='..'}\nworkspace={dependencies={bad={optional=true}},lints={rust={'rust::bad'='deny'}}}";
    assert_eq!(
        inspect(source, false, CONTEXT).0,
        Err(Tag::WorkspaceConflict)
    );
    assert_eq!(inspect(source, true, CONTEXT), (Ok((true, None)), vec![]));
}

#[test]
fn lints_precede_dependency_rejections_and_dependency_checks_precede_all_extra_warnings() {
    let source = "workspace={lints={rust={'rust::bad'='deny'}},dependencies={a={optional=true}}}";
    assert_eq!(inspect(source, false, CONTEXT).0, Err(Tag::LintName));
    for (dependencies, tag) in [
        ("z={optional=true},a={public=true}", Tag::WorkspacePublic),
        ("a={optional=true,public=true}", Tag::WorkspaceOptional),
        ("a={extra=1},z={optional=true}", Tag::WorkspaceOptional),
    ] {
        let source = format!("workspace={{dependencies={{{dependencies}}}}}");
        assert_eq!(inspect(&source, false, CONTEXT), (Err(tag), vec![]));
    }
    let (result, warnings) = inspect(
        "workspace={dependencies={z={extra=1},a={z=1,a=2,optional=false,public=false}}}",
        false,
        CONTEXT,
    );
    assert!(result.is_ok());
    assert_eq!(
        warnings,
        [
            "unused manifest key: workspace.dependencies.a.a",
            "unused manifest key: workspace.dependencies.a.z",
            "unused manifest key: workspace.dependencies.z.extra"
        ]
    );
}

#[test]
fn lint_tool_sorting_unknown_skip_config_insertion_order_and_exact_exception() {
    let source = r#"workspace.lints={zzz={'rust::ignored'='deny'},rust={unexpected_cfgs={level='warn',z=1,check-cfg=['cfg(test)'],a=2}},cargo={foo='warn'}}"#;
    let (result, warnings) = inspect(source, false, CONTEXT);
    assert!(result.is_ok());
    assert_eq!(warnings, [
        "unused manifest key `lints.cargo` (may be supported in a future version)\n\nconsider passing `-Zcargo-lints` to enable this feature.",
        "unused manifest key: `lints.rust.unexpected_cfgs.z`",
        "unused manifest key: `lints.rust.unexpected_cfgs.a`",
        "unrecognized lint tool `lints.zzz`, specifying unrecognized tools may break in the future.\nsupported tools: cargo, clippy, rust, rustdoc",
    ]);
    assert!(inspect(
        "workspace.lints.cargo={}",
        false,
        Context {
            cargo_lints: true,
            ..CONTEXT
        }
    )
    .1
    .is_empty());
    assert_eq!(inspect("workspace.lints.cargo={}", false, Context { nightly_features_allowed: false, ..CONTEXT }).1, [
        "unused manifest key `lints.cargo` (may be supported in a future version)\n\nthis Cargo does not support nightly features, but if you\nswitch to nightly channel you can pass\n`-Zcargo-lints` to enable this feature."
    ]);
    for tool in ["cargo", "clippy", "rust", "rustdoc"] {
        for name in ["::", "rust::foo", "clippy::foo", "other::foo::bar"] {
            let source = format!("workspace.lints.{tool}={{'{name}'='deny'}}");
            assert_eq!(inspect(&source, false, CONTEXT).0, Err(Tag::LintName));
        }
    }
}

#[derive(Default)]
struct Meter {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}
impl document::Admission for Meter {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

#[test]
fn configuration_rejections_match_all_executed_pinned_cargo_cases() {
    let rows = include_str!("../../../../../reference/manifest/workspace-observations.jsonl");
    let mut count = 0;
    for line in rows.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let (result, _) = inspect(row["source"].as_str().unwrap(), false, CONTEXT);
        let tag = result.err().map(|tag| format!("{tag:?}"));
        assert_eq!(tag.as_deref(), row["tag"].as_str(), "{}", row["name"]);
        count += 1;
    }
    assert_eq!(count, 25);
}

#[test]
fn actual_original_budget_has_exact_warning_layout_and_records_string_floors() {
    use crate::input::membership::manifest::tests::one_short;
    use crate::{input, node};
    let document = document::parse(
        "workspace={dependencies={x={extra=1}},lints={cargo={}}}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let run = |items, bytes, string| {
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .unwrap();
        let mut allocation = input::Allocation::with_limits(items, bytes, string);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut budget = input::membership::Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let mut warnings = Warnings::new();
        let result =
            Original::interpret(value::Input::root(&document), &mut session).and_then(|original| {
                workspace::configure(
                    &original,
                    Path::new("/project/Cargo.toml"),
                    false,
                    CONTEXT,
                    &mut warnings,
                    &mut session,
                )
                .map(|_| ())
            });
        let success = session.finish(result).is_ok();
        let maximum = warnings
            .as_slice()
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or(0);
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (
            success,
            allocation.usage(),
            maximum,
            allocation.exceedances(),
        )
    };
    let (success, usage, string, _) = run(usize::MAX, usize::MAX, usize::MAX);
    assert!(success);
    assert_eq!(
        run(usage.items, usage.bytes, string),
        (true, usage, string, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    let (success, spent, _, exceeded) = run(usage.items - 1, usage.bytes, string);
    assert!(success && spent == usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        usage.items
    ));
    let (success, spent, _, exceeded) = run(usage.items, usage.bytes - 1, string);
    assert!(success && spent == usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes
    ));
    // One short of the longest warning string records the decoded-string
    // axis; the warning is still built and the same work is spent.
    let (success, spent, maximum, exceeded) = run(usage.items, usage.bytes, string - 1);
    assert!(success && spent == usage && maximum == string);
    assert!(
        one_short(&exceeded, "membership decoded TOML string bytes", string),
        "{exceeded:?}"
    );
    let axis = exceeded
        .axis("membership decoded TOML string bytes")
        .unwrap();
    assert_eq!(
        (axis.target, axis.first_requested),
        (string as u64 - 1, string as u64)
    );
}

fn initial_context() -> super::Context<'static> {
    super::Context {
        features: super::features::Context {
            allow_features: None,
            nightly_features_allowed: true,
            channel: "nightly",
            is_local: true,
        },
        cargo_lints: false,
        profiles: crate::input::membership::manifest::load::ProfileContext::fixture(),
    }
}

#[test]
fn complete_original_precedes_features_which_precede_workspace_checks() {
    for (source, expected) in [
        ("cargo-features=['unknown']\npackage={name=123}", Tag::Type),
        (
            "cargo-features=['unknown']\npackage={name='x',workspace='..'}\nworkspace={}",
            Tag::Feature(super::features::Invalid::Unknown { index: 0 }),
        ),
        (
            "cargo-features=['edition']\npackage={name='x',workspace='..'}\nworkspace={}",
            Tag::WorkspaceConflict,
        ),
    ] {
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result =
            Original::interpret(value::Input::root(&document), &mut session).and_then(|original| {
                super::configure(
                    &original,
                    Path::new("/project/Cargo.toml"),
                    false,
                    initial_context(),
                    &mut session,
                )
                .map(|_| ())
            });
        assert!(
            matches!(result, Err(Stop::Rejected(error)) if error.tag == expected),
            "{source}"
        );
    }
}

#[test]
fn joined_configuration_owns_feature_then_workspace_warnings_and_all_refusals() {
    let document = document::parse(
        "cargo-features=['edition']\nworkspace={lints={cargo={}}}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let prior = vec![
        document::Request::Items(7),
        document::Request::Allocation {
            bytes: 31,
            alignment: 1,
        },
    ];
    let mut full = Meter {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = session::Session::new(&mut full);
    let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let configured = super::configure(
        &original,
        Path::new("/project/Cargo.toml"),
        false,
        initial_context(),
        &mut session,
    )
    .unwrap();
    assert_eq!(configured.warnings.as_slice().len(), 2);
    assert!(configured.warnings.as_slice()[0].starts_with("the cargo feature `edition`"));
    assert!(configured.warnings.as_slice()[1].starts_with("unused manifest key `lints.cargo`"));
    session.finish(Ok(())).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut meter = Meter {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut meter);
        let result =
            Original::interpret(value::Input::root(&document), &mut session).and_then(|original| {
                super::configure(
                    &original,
                    Path::new("/project/Cargo.toml"),
                    false,
                    initial_context(),
                    &mut session,
                )
                .map(|_| ())
            });
        assert!(matches!(result, Err(Stop::Refused)), "{refusal}");
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(meter.requests, full.requests[..=refusal]);
    }
}

#[test]
fn every_warning_growth_and_configuration_refusal_latches_original_account() {
    let document = document::parse(
        "workspace={dependencies={x={a=1,b=2}},lints={cargo={},other={}}}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let prior = vec![document::Request::Items(7), document::Request::String(31)];
    let mut full = Meter {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = session::Session::new(&mut full);
    let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let result = workspace::configure(
        &original,
        Path::new("/project/Cargo.toml"),
        false,
        CONTEXT,
        &mut warnings,
        &mut session,
    );
    session.finish(result).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut meter = Meter {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut meter);
        let result = Original::interpret(value::Input::root(&document), &mut session);
        let mut warnings = Warnings::new();
        if let Ok(original) = result {
            assert!(matches!(
                workspace::configure(
                    &original,
                    Path::new("/project/Cargo.toml"),
                    false,
                    CONTEXT,
                    &mut warnings,
                    &mut session
                ),
                Err(Stop::Refused)
            ));
        } else {
            assert!(matches!(result, Err(Stop::Refused)));
        }
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            warnings.push(&["after failure"], &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(allocations, storage::ALLOCATIONS.with(|count| count.get()));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(meter.requests, full.requests[..=refusal], "{refusal}");
    }
}
