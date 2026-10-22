use super::*;
use crate::input::membership::manifest::{Source, Stop};

#[allow(
    dead_code,
    reason = "pinned upstream source is included verbatim; Definition::root serves only its standalone reproduction harness"
)]
mod reference {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-definition/definition.rs"
    ));
}

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

fn definitions() -> Vec<(Definition<'static>, reference::Definition)> {
    vec![
        (Definition::BuiltIn, reference::Definition::BuiltIn),
        (
            Definition::Path(Path::new("/project/.cargo/config.toml")),
            reference::Definition::Path("/project/.cargo/config.toml".into()),
        ),
        (
            Definition::Path(Path::new("/different/config.toml")),
            reference::Definition::Path("/different/config.toml".into()),
        ),
        (
            Definition::Environment("CARGO_PATH_BASES_WORKSPACE"),
            reference::Definition::Environment("CARGO_PATH_BASES_WORKSPACE".into()),
        ),
        (
            Definition::Environment("OTHER"),
            reference::Definition::Environment("OTHER".into()),
        ),
        (Definition::Cli(None), reference::Definition::Cli(None)),
        (
            Definition::Cli(Some(Path::new("/custom/place/options"))),
            reference::Definition::Cli(Some("/custom/place/options".into())),
        ),
        (
            Definition::Cli(Some(Path::new("/tmp/config.toml"))),
            reference::Definition::Cli(Some("/tmp/config.toml".into())),
        ),
    ]
}

#[test]
fn priorities_and_same_class_equality_match_unchanged_cargo() {
    let definitions = definitions();
    for (left, expected_left) in &definitions {
        for (right, expected_right) in &definitions {
            assert_eq!(left.cmp(right), expected_left.cmp(expected_right));
            assert_eq!(left == right, expected_left == expected_right);
            assert_eq!(
                left.is_higher_priority(right),
                expected_left.is_higher_priority(expected_right)
            );
        }
    }
}

#[test]
fn resolved_paths_match_unchanged_roots_and_native_join() {
    let cwd = Path::new("/cargo/work");
    for (definition, expected) in definitions() {
        for raw in [
            "",
            "child",
            "../child",
            "/absolute",
            "{workspace}/child",
            "./a//b",
        ] {
            let mut account = Account::default();
            let mut session = Session::new(&mut account);
            let result = definition.resolve(raw, cwd, &mut session);
            let actual = session.finish(result).unwrap();
            assert_eq!(actual.as_os_str(), expected.root(cwd).join(raw).as_os_str());
        }
    }
}

#[test]
fn verified_second_parent_failure_precedes_relative_or_absolute_join() {
    let file = Path::new("/config.toml");
    let cwd = Path::new("/cargo/work");
    for (definition, expected) in [
        (
            Definition::Path(file),
            reference::Definition::Path(file.into()),
        ),
        (
            Definition::Cli(Some(file)),
            reference::Definition::Cli(Some(file.into())),
        ),
    ] {
        for raw in ["child", "/absolute"] {
            assert!(std::panic::catch_unwind(|| expected.root(cwd).join(raw)).is_err());
            let mut account = Account::default();
            let mut session = Session::new(&mut account);
            let result = definition.resolve(raw, cwd, &mut session);
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(
                Definition::BuiltIn.resolve("after", cwd, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                session.finish(Ok(())),
                Err(Error::Source {
                    cause: Source::ConfigDefinitionSecondParent,
                    span: None,
                })
            ));
            assert_eq!(
                account.requests.len(),
                1,
                "only the cause is admitted; no join or later work"
            );
            assert!(matches!(account.requests[0], document::Request::Items(1)));
        }
    }
}

#[test]
fn prior_refusal_and_cause_admission_refusal_keep_first_cause() {
    for refusal in [0, 1] {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let _ = session.items(3);
        let definition = Definition::Path(Path::new("/config.toml"));
        assert!(matches!(
            definition.resolve("child", Path::new("/cwd"), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests.len(), refusal + 1);
    }
}

#[test]
fn missing_first_parent_is_not_classified_as_the_verified_source_case() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    // A directory/root spelling is not a successfully loaded config file.
    let result = Definition::Path(Path::new("/")).resolve("child", Path::new("/cwd"), &mut session);
    assert!(matches!(session.finish(result), Err(Error::State)));
    assert!(account.requests.is_empty());
}
