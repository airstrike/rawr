use super::*;
use crate::input::membership::manifest::{
    load::{self, features},
    schema::manifest::Original,
    value, Error as ManifestError,
};

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}
impl document::Admission for Account {
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
fn context() -> load::Context<'static> {
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

#[test]
fn only_successful_initial_checks_publish_declarations() {
    for (source, expected) in [
        ("[workspace]", true),
        ("[package]\nname='member'", false),
        ("cargo-features=['not-a-feature']\n[workspace]", false),
        ("[package]\nworkspace='..'\n[workspace]", false),
        (
            "[workspace.dependencies]\na={version='1', optional=true}",
            false,
        ),
    ] {
        let mut account = Account::default();
        let document = document::parse(source, 80, &mut account).unwrap();
        let mut session = Session::new(&mut account);
        let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
        let mut roots = Roots::new();
        let result = load::prepare(
            &original,
            Path::new("/root/Cargo.toml"),
            false,
            context(),
            &mut roots,
            &mut session,
        );
        if let Ok(preparing) = &result {
            assert!(std::ptr::eq(preparing.original, &original));
            assert_eq!(preparing.path, Path::new("/root/Cargo.toml"));
            assert!(!preparing.embedded);
        }
        assert_eq!(roots.0.len(), usize::from(expected));
        if expected {
            assert!(result.is_ok());
        }
        // A root is available before normalization exists or has succeeded.
        // This test deliberately never constructs a completed loaded manifest.
    }
}

#[test]
fn native_equal_keys_replace_values_without_changing_original_keys() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut roots = Roots::new();
    let root = Root {
        directory: Path::new("/first"),
        declaration: None,
    };
    roots
        .insert(Path::new("/root/./nested"), root, &mut session)
        .unwrap();
    let replacement = Root {
        directory: Path::new("/second"),
        declaration: None,
    };
    roots
        .insert(Path::new("/root/nested"), replacement, &mut session)
        .unwrap();
    assert_eq!(roots.0.len(), 1);
    assert_eq!(roots.0.as_slice()[0].key.as_os_str(), "/root/./nested");
    assert_eq!(
        roots
            .get(Path::new("/root/nested"), &mut session)
            .unwrap()
            .unwrap()
            .directory,
        Path::new("/second")
    );
    roots
        .insert(Path::new("/root/nested/Cargo.toml"), root, &mut session)
        .unwrap();
    assert_eq!(roots.0.len(), 2);
    assert_eq!(
        roots
            .get(Path::new("/root/nested/Cargo.toml"), &mut session)
            .unwrap()
            .unwrap()
            .directory,
        Path::new("/first")
    );
    session.finish(Ok(())).unwrap();
}

#[test]
fn cached_ancestors_are_nearest_first_with_lexical_exclusion_and_member_override() {
    for (source, requested, expected) in [
        (
            "[workspace]\nexclude=['child']",
            "/root/child/Cargo.toml",
            "/Cargo.toml",
        ),
        (
            "[workspace]\nexclude=['child']\nmembers=['child']",
            "/root/child/Cargo.toml",
            "/root/Cargo.toml",
        ),
        (
            "[workspace]\nexclude=['child/*']",
            "/root/child/a/Cargo.toml",
            "/root/Cargo.toml",
        ),
        (
            "[workspace]\nexclude=['child']",
            "/root/childish/Cargo.toml",
            "/root/Cargo.toml",
        ),
        (
            "[workspace]\nexclude=['/root/child']",
            "/root/child/a/Cargo.toml",
            "/Cargo.toml",
        ),
    ] {
        let mut account = Account::default();
        let document = document::parse(source, 80, &mut account).unwrap();
        let mut session = Session::new(&mut account);
        let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
        let mut roots = Roots::new();
        roots
            .insert(
                Path::new("/"),
                Root {
                    directory: Path::new("/"),
                    declaration: None,
                },
                &mut session,
            )
            .unwrap();
        load::prepare(
            &original,
            Path::new("/root/Cargo.toml"),
            false,
            context(),
            &mut roots,
            &mut session,
        )
        .unwrap();
        assert_eq!(
            roots
                .ancestor(Path::new(requested), &mut session)
                .unwrap()
                .unwrap(),
            Path::new(expected)
        );
        session.finish(Ok(())).unwrap();
    }
}

fn exercise<A: document::Admission>(session: &mut Session<'_, A>) -> Step<()> {
    let mut roots = Roots::new();
    let root = Root {
        directory: Path::new("/root"),
        declaration: None,
    };
    roots.insert(Path::new("/root"), root, session)?;
    roots.insert(Path::new("/root"), root, session)?;
    assert!(roots
        .ancestor(Path::new("/root/member/Cargo.toml"), session)?
        .is_some());
    Ok(())
}

#[test]
fn refusal_is_terminal_before_cache_mutation_or_allocation() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = exercise(&mut session);
    session.finish(result).unwrap();
    for refusal in 0..account.requests.len() {
        let mut refused = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut refused);
        storage::ALLOCATIONS.with(|count| count.set(0));
        assert!(exercise(&mut session).is_err());
        let allocated = storage::ALLOCATIONS.with(|count| count.get());
        assert!(exercise(&mut session).is_err());
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocated);
        let admitted = account.requests[..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(allocated, admitted);
        assert!(
            matches!(session.finish(Ok(())), Err(ManifestError::Admission(index)) if index == refusal)
        );
        assert_eq!(refused.requests, account.requests[..=refusal]);
    }
}

#[test]
fn exclusion_and_members_each_short_circuit_in_source_order() {
    let source = "[workspace]\nexclude=['absent','child','unreached']\nmembers=['other','child','also-unreached']";
    let mut account = Account::default();
    let document = document::parse(source, 80, &mut account).unwrap();
    let mut session = Session::new(&mut account);
    let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut roots = Roots::new();
    load::prepare(
        &original,
        Path::new("/root/Cargo.toml"),
        false,
        context(),
        &mut roots,
        &mut session,
    )
    .unwrap();
    let root = roots
        .get(Path::new("/root"), &mut session)
        .unwrap()
        .unwrap();
    session.finish(Ok(())).unwrap();
    let start = account.requests.len();
    let mut session = Session::new(&mut account);
    assert!(!root
        .excluded(Path::new("/root/child/Cargo.toml"), &mut session)
        .unwrap());
    session.finish(Ok(())).unwrap();
    let sizes: Vec<_> = account.requests[start..]
        .iter()
        .filter_map(|request| match request {
            document::Request::Allocation { bytes, .. } => Some(*bytes),
            _ => None,
        })
        .collect();
    assert_eq!(
        sizes,
        [
            "/root/absent".len(),
            "/root/child".len(),
            "/root/other".len(),
            "/root/child".len()
        ]
    );
}

#[test]
fn cache_and_selected_path_use_the_original_budget() {
    use crate::input::membership::manifest::tests::admits_one_short;
    use crate::input::membership::Budget;
    use crate::{input, node};

    fn run(items: usize, bytes: usize) -> (bool, input::Usage, crate::exceedance::Record) {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
        allocation.items(9).unwrap();
        allocation.bytes(27).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = Session::new(&mut budget);
        let result = exercise(&mut session);
        let ok = session.finish(result).is_ok();
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (ok, allocation.usage, allocation.exceedances())
    }
    let (ok, usage, _) = run(usize::MAX, usize::MAX);
    assert!(ok);
    assert_eq!(
        run(usage.items, usage.bytes),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        run(usage.items - 1, usage.bytes),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        run(usage.items, usage.bytes - 1),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
}

#[test]
fn candidate_configuration_selects_original_path_or_normalized_pointer() {
    for (text, expected) in [
        ("[workspace]", Some("/root/./Cargo.toml")),
        ("[workspace]\nexclude=['member']", None),
        (
            "[workspace]\nexclude=['member']\nmembers=['member']",
            Some("/root/./Cargo.toml"),
        ),
        (
            "[package]\nname='p'\nworkspace='../other'",
            Some("/other/Cargo.toml"),
        ),
        ("[package]\nname='p'", None),
    ] {
        let mut account = Account::default();
        let document = document::parse(text, 80, &mut account).unwrap();
        let mut session = Session::new(&mut account);
        let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
        let configured = load::configure(
            &original,
            Path::new("/root/./Cargo.toml"),
            false,
            context(),
            &mut session,
        )
        .unwrap();
        let path = configured
            .workspace
            .selected_root(
                Path::new("/root/./Cargo.toml"),
                Path::new("/root/member/Cargo.toml"),
                &mut session,
            )
            .unwrap();
        // Compare bytes as well: root selection copies the candidate spelling,
        // while a member pointer follows Cargo's lexical normalization.
        assert_eq!(
            path.as_ref().map(|path| path.as_os_str()),
            expected.map(std::ffi::OsStr::new)
        );
    }
}
