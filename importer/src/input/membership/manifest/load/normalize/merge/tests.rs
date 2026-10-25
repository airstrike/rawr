use super::*;
use crate::input::membership::manifest::{reference, value, Error, Stop};

/// Each selected target's name and path.
type Selected = Vec<(Option<String>, Option<PathBuf>)>;

fn selection(
    source: &str,
    inferred: &[(&str, &str)],
    edition: u16,
    autodiscover: Option<bool>,
) -> (Selected, Vec<String>, bool) {
    let document = document::parse(source, 80, &mut reference::Account).unwrap();
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let original = document
        .table()
        .get("targets")
        .map(|input| Targets::interpret(value::Input::raw(input), &mut session).unwrap());
    let inferred = inferred
        .iter()
        .map(|&(name, path)| infer::Item::fixture(name, path))
        .collect::<Vec<_>>();
    let fast = are_normalized(original.as_ref(), autodiscover, &mut session).unwrap();
    let mut warnings = Warnings::new();
    let selected = if fast {
        declared(original.as_ref(), &mut session).unwrap()
    } else {
        select(
            original.as_ref(),
            &inferred,
            Context {
                root: Path::new("/p"),
                autodiscover,
                edition,
                human: "binary",
                kind: "bin",
                flag: "autobins",
            },
            &mut warnings,
            &mut session,
        )
        .unwrap()
    };
    (
        selected
            .as_slice()
            .iter()
            .map(|item| {
                (
                    item.name().map(str::to_owned),
                    item.path().map(Path::to_path_buf),
                )
            })
            .collect(),
        warnings.as_slice().to_vec(),
        fast,
    )
}

#[test]
fn explicit_false_fast_path_preserves_declared_order_and_requires_both_fields() {
    let (items, warnings, fast) = selection(
        "targets=[{name='z',path='z.rs'},{name='a',path='a.rs'}]",
        &[],
        2021,
        Some(false),
    );
    assert!(fast);
    assert_eq!(
        items
            .iter()
            .map(|item| item.0.as_deref())
            .collect::<Vec<_>>(),
        [Some("z"), Some("a")]
    );
    assert!(warnings.is_empty());
    assert!(selection("", &[], 2021, Some(false)).2);
    assert!(!selection("targets=[{name='x'}]", &[], 2021, Some(false)).2);
    assert!(!selection("targets=[]", &[], 2021, None).2);
}

#[test]
fn inference_is_suppressed_by_either_explicit_name_or_joined_path() {
    let (items, warnings, fast) = selection(
        "targets=[{name='keep',path='src/./same.rs'}]",
        &[
            ("keep", "src/other.rs"),
            ("alias", "src/same.rs"),
            ("new", "src/new.rs"),
        ],
        2021,
        None,
    );
    assert!(!fast);
    assert!(warnings.is_empty());
    assert_eq!(
        items
            .iter()
            .map(|item| item.0.as_deref())
            .collect::<Vec<_>>(),
        [Some("keep"), Some("new")]
    );
    assert_eq!(items[0].1.as_ref().unwrap().as_os_str(), "src/./same.rs");
}

#[test]
fn edition_2015_warning_uses_encounter_order_before_excluding_inference() {
    let inputs = [("z", "src/bin/z.rs"), ("a", "src/bin/a.rs")];
    let (items, warnings, _) = selection("targets=[{name='b',path='b.rs'}]", &inputs, 2015, None);
    assert_eq!(items.len(), 1);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("\n\n* src/bin/z.rs\n* src/bin/a.rs\n\nThis is likely"));
    assert!(warnings[0].starts_with("An explicit [[bin]] section"));
    let (items, warnings, _) = selection("targets=[{name='b',path='b.rs'}]", &inputs, 2018, None);
    assert_eq!(
        items
            .iter()
            .map(|item| item.0.as_deref())
            .collect::<Vec<_>>(),
        [Some("a"), Some("b"), Some("z")]
    );
    assert!(warnings.is_empty());
}

#[test]
fn absent_and_explicit_empty_targets_have_different_2015_inference_behavior() {
    let inputs = [("z", "z.rs")];
    assert_eq!(selection("", &inputs, 2015, None).0.len(), 1);
    let (items, warnings, _) = selection("targets=[]", &inputs, 2015, None);
    assert!(items.is_empty());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn formatting_admits_exact_bytes_and_rejects_an_inconsistent_second_pass() {
    struct Account {
        limit: usize,
        requested: usize,
    }
    impl document::Admission for Account {
        type Error = &'static str;
        fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
            if let document::Request::Allocation { bytes, .. } = request {
                self.requested += bytes;
                if self.requested > self.limit {
                    return Err("format budget");
                }
            }
            Ok(())
        }
    }
    for (limit, succeeds) in [(5, true), (4, false)] {
        let mut account = Account {
            limit,
            requested: 0,
        };
        let mut session = Session::new(&mut account);
        let result = target::formatted(format_args!("{}:{}", "é", "λ"), &mut session);
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(account.requested, 5);
    }
    struct Changing(std::cell::Cell<bool>);
    impl std::fmt::Display for Changing {
        fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            output.write_str(if self.0.replace(true) { "larger" } else { "x" })
        }
    }
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    assert!(matches!(
        target::formatted(
            format_args!("{}", Changing(std::cell::Cell::new(false))),
            &mut session
        ),
        Err(Stop::Refused)
    ));
    assert!(matches!(session.finish(Ok(())), Err(Error::State)));
}
