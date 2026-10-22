use super::*;
use crate::input::membership::manifest::{reference, Source};

fn checked(scripts: &[&str]) -> Result<(), String> {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let result = check(scripts, &mut session);
    session.finish(result).unwrap()
}

#[test]
fn empty_unique_and_exact_stem_spelling() {
    assert_eq!(checked(&[]), Ok(()));
    assert_eq!(
        checked(&["build.rs", "other/build2.rs", ".hidden", "nested/a.b.rs"]),
        Ok(())
    );
    assert_eq!(checked(&["first/build.rs", "second/build.txt", "third/build.rs"]),Err("found build scripts with duplicate file stems, but all build scripts must have a unique file stem\n  for stem `build`: first/build.rs, second/build.txt, third/build.rs".into()));
    assert_eq!(checked(&[".hidden", "nested/.hidden"]),Err("found build scripts with duplicate file stems, but all build scripts must have a unique file stem\n  for stem `.hidden`: .hidden, nested/.hidden".into()));
}

#[test]
fn duplicate_groups_keep_existing_pinned_table_iteration_and_input_path_order() {
    let scripts = [
        "first/z.rs",
        "first/a.rs",
        "first/m.rs",
        "second/a.txt",
        "second/z.txt",
        "second/m.txt",
    ];
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut table = Table::new();
    for (index, stem) in ["z", "a", "m"].into_iter().enumerate() {
        table.merge(stem, index, &mut session).unwrap();
    }
    let mut order = table.into_iter(&mut session).unwrap();
    let mut expected = "found build scripts with duplicate file stems, but all build scripts must have a unique file stem".to_owned();
    while let Some((stem, _)) = order.next(&mut session).unwrap() {
        expected.push_str(&format!(
            "\n  for stem `{stem}`: first/{stem}.rs, second/{stem}.txt"
        ));
    }
    session.finish(Ok(())).unwrap();
    assert_eq!(checked(&scripts), Err(expected));
}

#[test]
fn later_missing_stem_beats_earlier_duplicates_and_latches_before_following_script() {
    for bad in ["", "/", "..", "a/..", "."] {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let result = check(
            &["first/x.rs", "second/x.rs", bad, "later.rs"],
            &mut session,
        );
        assert!(result.is_err());
        assert!(check(&["valid.rs"], &mut session).is_err());
        assert!(matches!(
            session.finish(result),
            Err(Error::Source {
                cause: Source::BuildScriptStem,
                span: None
            })
        ));
    }
}

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refuse: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

#[test]
fn prior_and_cause_admission_refusals_win_without_later_work() {
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let result = check(&["first/x.rs", "second/x.rs", "/"], &mut session);
    assert!(matches!(
        session.finish(result),
        Err(Error::Source {
            cause: Source::BuildScriptStem,
            ..
        })
    ));
    let cause = baseline.requests.len() - 1;
    assert!(matches!(
        baseline.requests[cause],
        document::Request::Items(1)
    ));
    for refused in [0, cause] {
        let mut account = Account {
            refuse: Some(refused),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let result = check(
            &["first/x.rs", "second/x.rs", "/", "later.rs"],
            &mut session,
        );
        assert!(check(&["other.rs"], &mut session).is_err());
        assert!(matches!(session.finish(result),Err(Error::Admission(index)) if index == refused));
        assert_eq!(account.requests.len(), refused + 1);
    }
}
