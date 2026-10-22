use super::*;
use crate::input::membership::manifest::{reference, Error};

fn parsed(text: &str) -> Result<String, String> {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let result = match parse(text, &mut session).unwrap() {
        Ok(url) => Ok(url.as_str(&mut session).unwrap().to_owned()),
        Err(error) => Err(error),
    };
    session.finish(Ok(())).unwrap();
    result
}

#[test]
fn accepted_urls_preserve_existing_parser_serialization() {
    for (input, expected) in [
        ("HTTPS://EXAMPLE.COM:443/a/../b", "https://example.com/b"),
        ("ssh://git@example.com/repo", "ssh://git@example.com/repo"),
        ("git:thing", "git:thing"),
    ] {
        assert_eq!(parsed(input).unwrap(), expected);
    }
}

#[test]
fn ordinary_parse_errors_keep_exact_cargo_wrapper() {
    for (input, error) in [
        ("relative", "relative URL without a base"),
        ("http://", "empty host"),
        ("https://a:bad", "invalid port number"),
    ] {
        assert_eq!(
            parsed(input).unwrap_err(),
            format!("invalid url `{input}`: {error}")
        );
    }
}

#[test]
fn git_suggestion_replaces_only_first_colon_and_retains_unparsed_bytes() {
    for (input, suggested) in [
        ("git@host:repo:branch", "git@host/repo:branch"),
        ("git@host", "git@host"),
        ("git@:repo", "git@/repo"),
    ] {
        assert_eq!(parsed(input).unwrap_err(),format!("invalid url `{input}`: relative URL without a base; try using `ssh://{suggested}` instead"));
    }
    assert_eq!(
        parsed(" git@host:repo").unwrap_err(),
        "invalid url ` git@host:repo`: relative URL without a base"
    );
}

#[derive(Default)]
struct Account {
    calls: usize,
    refused: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, _: document::Request) -> Result<(), usize> {
        let call = self.calls;
        self.calls += 1;
        if self.refused == Some(call) {
            Err(call)
        } else {
            Ok(())
        }
    }
}

#[test]
fn every_parser_suggestion_and_message_refusal_stays_terminal() {
    for input in ["git@host:repo:branch", "http://", "https://example.com"] {
        let mut baseline = Account::default();
        let mut session = Session::new(&mut baseline);
        let result = parse(input, &mut session);
        assert!(session.finish(result).is_ok());
        for index in 0..baseline.calls {
            let mut account = Account {
                refused: Some(index),
                ..Account::default()
            };
            let mut session = Session::new(&mut account);
            let result = parse(input, &mut session);
            assert!(result.is_err());
            assert!(parse("git@later:repo", &mut session).is_err());
            assert!(matches!(session.finish(result),Err(Error::Admission(at)) if at==index));
            assert_eq!(account.calls, index + 1);
        }
    }
}

#[test]
fn parser_refusal_cannot_enter_git_suggestion_branch() {
    let input = "git@host:repo";
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    assert!(matches!(Url::parse(input, &mut session), Ok(Err(_))));
    session.finish(Ok(())).unwrap();
    let index = baseline.calls - 1;
    let mut account = Account {
        refused: Some(index),
        ..Account::default()
    };
    let mut session = Session::new(&mut account);
    let result = parse(input, &mut session);
    assert!(matches!(session.finish(result),Err(Error::Admission(at)) if at==index));
    assert_eq!(account.calls, index + 1);
}

#[test]
fn native_path_adapter_preserves_constructor_output_and_exact_lossy_error() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let result = path(Path::new("/a/../b c"), &mut session).unwrap();
    let Ok(url) = result else {
        panic!("absolute path")
    };
    assert_eq!(url.as_str(&mut session).unwrap(), "file:///a/../b%20c");
    let result = path(Path::new(OsStr::from_bytes(b"relative/\xff")), &mut session).unwrap();
    let Err(error) = result else {
        panic!("relative path")
    };
    assert_eq!(error, "invalid path url `relative/�`");
    session.finish(Ok(())).unwrap();
}

#[test]
fn path_constructor_and_error_message_refusals_remain_terminal() {
    for input in ["relative", "/absolute path"] {
        let mut baseline = Account::default();
        let mut session = Session::new(&mut baseline);
        let result = path(Path::new(input), &mut session);
        assert!(session.finish(result).is_ok());
        for index in 0..baseline.calls {
            let mut account = Account {
                refused: Some(index),
                ..Account::default()
            };
            let mut session = Session::new(&mut account);
            let result = path(Path::new(input), &mut session);
            assert!(result.is_err());
            assert!(path(Path::new("relative"), &mut session).is_err());
            assert!(matches!(session.finish(result),Err(Error::Admission(at)) if at==index));
            assert_eq!(account.calls, index + 1);
        }
    }
}
