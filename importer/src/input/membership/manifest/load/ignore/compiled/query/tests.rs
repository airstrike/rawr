use std::path::Path;

use rawr_toml::document::{Admission, Request};

use super::Matcher;
use crate::input::membership::manifest::{
    load::ignore::Builder, reference::Account, session::Session, Error, Stop,
};

fn compile(rules: &[&str]) -> Matcher {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = Builder::prepare(Path::new("/project"), &mut session).unwrap();
    for rule in rules {
        assert!(builder.add(rule, &mut session).unwrap().is_ok());
    }
    builder.build(&mut session).unwrap().ok().unwrap()
}

#[test]
fn direct_and_nearest_parent_results_preserve_whitelist_and_directory_precedence() {
    let mut matcher = compile(&[
        "cache/",
        "!cache/keep.txt",
        "*.rs",
        "!allowed/",
        "allowed/blocked/",
    ]);
    assert!(matcher.matches.is_none());
    let cases = [
        ("cache/sub/a.txt", false, true),
        ("cache/keep.txt", false, false),
        ("allowed/x.rs", false, true),
        ("allowed/x.txt", false, false),
        ("allowed/blocked/x.txt", false, true),
        ("cache", false, false),
        ("cache", true, true),
        ("elsewhere/plain", false, false),
        ("", false, false),
    ];
    for _ in 0..2 {
        for (path, is_dir, expected) in cases {
            assert_eq!(
                matcher
                    .is_ignored(Path::new(path), is_dir, &mut Session::new(&mut Account))
                    .unwrap(),
                expected,
                "{path} directory={is_dir}"
            );
        }
    }
    assert!(matcher.matches.is_some());
    let mut empty = compile(&[]);
    assert!(!empty
        .is_ignored(
            Path::new("/outside"),
            false,
            &mut Session::new(&mut Account)
        )
        .unwrap());
    assert!(empty.matches.is_none());
}

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    refuse: Option<usize>,
    calls: usize,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let index = self.calls;
        self.calls += 1;
        if self.refuse == Some(index) {
            return Err(index);
        }
        self.requests.push(request);
        Ok(())
    }
}

#[test]
fn parent_query_refusal_keeps_original_cause_and_retained_scratch() {
    let mut matcher = compile(&["cache/"]);
    let mut baseline = Count::default();
    assert!(matcher
        .is_ignored(
            Path::new("cache/sub/file"),
            false,
            &mut Session::new(&mut baseline)
        )
        .unwrap());
    for at in [0, baseline.requests.len() / 2, baseline.requests.len() - 1] {
        let mut matcher = compile(&["cache/"]);
        let mut count = Count {
            refuse: Some(at),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            matcher.is_ignored(Path::new("cache/sub/file"), false, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            matcher.is_ignored(Path::new("cache"), true, &mut session),
            Err(Stop::Refused)
        ));
        assert!(
            matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(cause)) if cause==at)
        );
        assert_eq!(count.requests, baseline.requests[..at]);
        assert_eq!(count.calls, at + 1);
    }
}
