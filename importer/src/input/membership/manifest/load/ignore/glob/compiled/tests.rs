use super::{Set, Strategy};
use crate::input::membership::manifest::{
    load::ignore::{glob, paths::Candidate},
    reference::Account,
    session::Session,
    storage::Buffer,
    Error, Stop,
};
use rawr_toml::document::{Admission, Request};
use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    refuse: Option<usize>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn rules() -> Vec<glob::Built> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    [
        "s*/file.*",
        "src/**",
        "src/*.rs",
        "src/file.rs",
        "**/file.rs",
        "**/*.rs",
        "**/src/file.rs",
    ]
    .into_iter()
    .map(|text| glob::build(text, &mut session).unwrap().ok().expect("glob"))
    .collect()
}

fn compile(rules: &[glob::Built]) -> Set {
    Set::build(rules, &mut Session::new(&mut Account))
        .unwrap()
        .ok()
        .expect("compiled set")
}

fn candidate(bytes: &[u8]) -> Candidate<'_> {
    Candidate::prepare(
        Path::new(OsStr::from_bytes(bytes)),
        &mut Session::new(&mut Account),
    )
    .unwrap()
}

#[test]
fn all_seven_strategies_publish_sorted_global_ids_and_reuse_query_state() {
    let rules = rules();
    let mut set = compile(&rules);
    assert_eq!(set.len(), 7);
    assert!(matches!(
        set.strategies.as_slice(),
        [
            Strategy::Extension(_),
            Strategy::Basename(_),
            Strategy::Literal(_),
            Strategy::Suffix(_),
            Strategy::Prefix(_),
            Strategy::Required(_),
            Strategy::Regex(_),
        ]
    ));
    let mut found = Buffer::new();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let cases: &[(&[u8], &[usize])] = &[
        (b"src/file.rs", &[0, 1, 2, 3, 4, 5, 6]),
        (b"x/src/file.rs", &[4, 5, 6]),
        (b"src/\xff.rs", &[1, 2, 5]),
        (b"missing", &[]),
    ];
    for _ in 0..2 {
        for &(path, expected) in cases {
            found.push(usize::MAX, &mut session).unwrap();
            let candidate = candidate(path);
            set.matches_into(&candidate, &mut found, &mut session)
                .unwrap();
            assert_eq!(found.as_slice(), expected, "{path:?}");
            assert_eq!(
                set.is_match(&candidate, &mut session).unwrap(),
                !expected.is_empty()
            );
        }
    }
}

#[test]
fn empty_set_clears_existing_ids_without_constructing_strategies() {
    let mut count = Count::default();
    let mut set = Set::build(&[], &mut Session::new(&mut count))
        .unwrap()
        .ok()
        .expect("empty");
    assert_eq!(count.requests, [Request::Items(1)]);
    assert_eq!(set.len(), 0);
    assert_eq!(set.strategies.capacity(), 0);
    let mut found = Buffer::new();
    found.push(7, &mut Session::new(&mut Account)).unwrap();
    let candidate = candidate(b"");
    set.matches_into(&candidate, &mut found, &mut Session::new(&mut count))
        .unwrap();
    assert!(found.as_slice().is_empty());
    assert!(!set
        .is_match(&candidate, &mut Session::new(&mut count))
        .unwrap());
    assert!(count
        .requests
        .iter()
        .all(|request| matches!(request, Request::Items(_))));
}

#[test]
fn refusal_keeps_original_cause_through_clear_strategy_work_and_final_ordering() {
    let rules = rules();
    let candidate = candidate(b"src/file.rs");
    let mut complete = Count::default();
    let mut set = compile(&rules);
    let mut found = Buffer::new();
    found
        .push(usize::MAX, &mut Session::new(&mut Account))
        .unwrap();
    set.matches_into(&candidate, &mut found, &mut Session::new(&mut complete))
        .unwrap();
    let last = complete.requests.len() - 1;
    for refuse in [0, 1, complete.requests.len() / 2, last] {
        let mut set = compile(&rules);
        let mut found = Buffer::new();
        found
            .push(usize::MAX, &mut Session::new(&mut Account))
            .unwrap();
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            set.matches_into(&candidate, &mut found, &mut session),
            Err(Stop::Refused)
        ));
        if refuse == 0 {
            assert_eq!(found.as_slice(), [usize::MAX]);
        }
        if refuse == 1 {
            assert!(found.as_slice().is_empty());
        }
        assert!(matches!(
            set.is_match(&candidate, &mut session),
            Err(Stop::Refused)
        ));
        assert!(
            matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(index)) if index == refuse)
        );
        assert_eq!(count.requests, complete.requests[..=refuse]);
    }
}
