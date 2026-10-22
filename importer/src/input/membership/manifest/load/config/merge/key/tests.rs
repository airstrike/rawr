use super::*;
use crate::input::membership::manifest::{Error, Stop};

#[allow(dead_code)]
mod reference {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-merge-key/reference.rs"
    ));
}

#[derive(Default)]
struct Account {
    count: usize,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, _: document::Request) -> Result<(), usize> {
        let index = self.count;
        self.count += 1;
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

#[test]
fn list_patterns_match_unchanged_pinned_key_program() {
    for parts in [
        vec![],
        vec!["credential-alias"],
        vec!["credential-alias", "name"],
        vec!["credential-alias", "name", "extra"],
        vec!["doc", "browser"],
        vec!["host", "runner"],
        vec!["registries", "my-registry", "credential-provider"],
        vec!["registry", "credential-provider"],
        vec!["target", "aarch64-apple-darwin", "runner"],
        vec!["build", "rustflags"],
        vec!["DOC", "browser"],
        vec!["target", "é", "runner"],
    ] {
        let mut expected = reference::ConfigKey::new();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let mut actual = Key::new();
        for part in parts {
            expected.push(part);
            actual.push(part, &mut session).unwrap();
        }
        for pattern in NON_MERGEABLE_LISTS
            .iter()
            .copied()
            .chain(["*", "*.*", "target.*", ""])
        {
            assert_eq!(
                actual.matches(pattern, &mut session).unwrap(),
                expected.matches(pattern),
                "{pattern}"
            );
        }
        assert_eq!(
            actual.is_nonmergeable_list(&mut session).unwrap(),
            NON_MERGEABLE_LISTS
                .iter()
                .any(|pattern| expected.matches(pattern))
        );
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn occupied_sibling_paths_remain_appended_as_in_pinned_merge_helper() {
    let mut expected = reference::ConfigKey::new();
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut actual = Key::new();
    for part in ["doc", "browser"] {
        expected.push(part);
        actual.push(part, &mut session).unwrap();
    }
    assert!(actual.is_nonmergeable_list(&mut session).unwrap());
    // merge_helper has no pop after returning from the first occupied entry.
    for part in ["host", "runner"] {
        expected.push(part);
        actual.push(part, &mut session).unwrap();
    }
    assert!(!actual.is_nonmergeable_list(&mut session).unwrap());
    assert!(!expected.matches("host.runner"));
    assert_eq!(
        actual.parts.as_slice(),
        &["doc", "browser", "host", "runner"]
    );
    session.finish(Ok(())).unwrap();
}

#[test]
fn original_account_refusal_stops_both_growth_and_matching() {
    fn run(session: &mut Session<'_, Account>) -> Step<()> {
        let mut key = Key::new();
        for part in ["registries", "a", "credential-provider", "another", "key"] {
            key.push(part, session)?;
            key.is_nonmergeable_list(session)?;
        }
        Ok(())
    }
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let result = run(&mut session);
    session.finish(result).unwrap();
    for refusal in 0..baseline.count {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(run(&mut session), Err(Stop::Refused)));
        assert!(matches!(run(&mut session), Err(Stop::Refused)));
        assert!(matches!(session.finish(Ok(())),Err(Error::Admission(index)) if index==refusal));
        assert_eq!(account.count, refusal + 1);
    }
}
