use super::*;
use crate::input::membership::manifest::{load::ignore::glob, reference::Account, Stop};
use rawr_toml::document::Request;
use std::{cell::RefCell, rc::Rc};

mod refusal;

#[derive(Default)]
struct Count {
    requests: Rc<RefCell<Vec<Request>>>,
    refuse: Option<usize>,
}

impl Admission for Count {
    type Error = usize;

    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let mut requests = self.requests.borrow_mut();
        let index = requests.len();
        requests.push(request);
        if self.refuse == Some(index) {
            return Err(index);
        }
        Ok(())
    }
}

fn emitted(pattern: &str) -> glob::regex::Regex {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    glob::parse(pattern, &mut session)
        .unwrap()
        .ok()
        .unwrap()
        .regex(&mut session)
        .unwrap()
}

fn compiled(patterns: &[&str], kind: info::Kind) -> Regex {
    let emitted: Vec<_> = patterns.iter().map(|pattern| emitted(pattern)).collect();
    let sources: Vec<_> = emitted.iter().collect();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let translated = glob::regex::batch::Batch::parse(&sources, &mut session)
        .unwrap()
        .ok()
        .unwrap()
        .translate(&mut session)
        .unwrap()
        .ok()
        .unwrap();
    // This is the actual batch connection. ASTs, HIRs and their source strings
    // remain owned until compile returns; the returned matcher borrows none.
    translated.compile(kind, &mut session).unwrap().unwrap()
}

fn ids(set: &Set) -> Vec<u32> {
    let mut cursor = 0;
    let mut found = Vec::new();
    while let Some(pattern) = set
        .next(&mut cursor, &mut Session::new(&mut Account))
        .unwrap()
    {
        found.push(pattern);
    }
    found
}

fn allocations(requests: &[Request]) -> usize {
    requests
        .iter()
        .filter(|request| matches!(request, Request::Allocation { .. }))
        .count()
}

#[test]
fn compiled_owner_outlives_source_trees_and_keeps_byte_empty_eoi_and_set_semantics() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        let mut regex = compiled(&["a", "?", ""], kind);
        for (bytes, expected) in [
            (b"a".as_slice(), vec![0, 1]),
            (b"\xff", vec![1]),
            (b"\0", vec![1]),
            (b"\n", vec![1]),
            (b"", vec![2]),
            (b"/", vec![]),
            (b"aa", vec![]),
        ] {
            let expected = if kind == info::Kind::All {
                expected
            } else {
                expected.into_iter().take(1).collect()
            };
            let mut account = Account;
            let mut session = Session::new(&mut account);
            assert_eq!(
                regex.is_match(bytes, &mut session).unwrap(),
                !expected.is_empty()
            );
            let mut set = Set::new(3, &mut session).unwrap();
            regex.overlapping(bytes, &mut set, &mut session).unwrap();
            assert_eq!(ids(&set), expected);
        }
        let mut unicode = compiled(&["é"], kind);
        assert!(unicode
            .is_match("é".as_bytes(), &mut Session::new(&mut Account))
            .unwrap());
        assert!(!unicode
            .is_match(b"\xc3", &mut Session::new(&mut Account))
            .unwrap());
    }
}

#[test]
fn impossible_queries_leave_cache_absent_and_existing_pattern_ids_untouched() {
    let mut regex = compiled(&["ab", "cd"], info::Kind::All);
    let mut set = Set::new(2, &mut Session::new(&mut Account)).unwrap();
    set.insert(1, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    for bytes in [b"a".as_slice(), b"abc", b""] {
        let mut count = Count::default();
        let mut session = Session::new(&mut count);
        assert!(!regex.is_match(bytes, &mut session).unwrap());
        regex.overlapping(bytes, &mut set, &mut session).unwrap();
        assert!(regex.execution.cache.is_none());
        assert_eq!(ids(&set), [1]);
        assert_eq!(allocations(&count.requests.borrow()), 0);
    }
    regex
        .overlapping(b"ab", &mut set, &mut Session::new(&mut Account))
        .unwrap();
    assert_eq!(ids(&set), [0, 1]);
    assert!(regex.execution.cache.is_some());
}

#[test]
fn each_owner_starts_cold_then_retains_its_own_full_cache() {
    let mut first = compiled(&["a"], info::Kind::LeftmostFirst);
    let mut second = compiled(&["a"], info::Kind::LeftmostFirst);
    assert!(!Arc::ptr_eq(&first.imp.strategy, &second.imp.strategy));
    assert!(first.execution.cache.is_none() && second.execution.cache.is_none());
    let mut cold = Count::default();
    assert!(first.is_match(b"a", &mut Session::new(&mut cold)).unwrap());
    assert!(allocations(&cold.requests.borrow()) > 0);
    assert!(first.execution.cache.is_some());
    assert!(second.execution.cache.is_none());
    let mut warm = Count::default();
    assert!(first.is_match(b"a", &mut Session::new(&mut warm)).unwrap());
    assert_eq!(allocations(&warm.requests.borrow()), 0);
    let mut distinct = Count::default();
    assert!(second
        .is_match(b"a", &mut Session::new(&mut distinct))
        .unwrap());
    assert_eq!(*cold.requests.borrow(), *distinct.requests.borrow());
    assert!(!std::ptr::eq(
        first.execution.cache.as_ref().unwrap(),
        second.execution.cache.as_ref().unwrap(),
    ));
}

#[test]
fn direct_factory_retains_the_source_strategy_and_info_lifetimes() {
    let mut regex = compiled(&["a"], info::Kind::LeftmostFirst);
    assert_eq!(Arc::strong_count(&regex.imp), 1);
    assert_eq!(Arc::strong_count(&regex.imp.strategy), 2);
    assert_eq!(Arc::strong_count(&regex.imp.info.0), 2);
    assert!(Arc::ptr_eq(&regex.imp.strategy, &regex.execution.factory));
    let strategy = Arc::downgrade(&regex.imp.strategy);
    let info = Arc::downgrade(&regex.imp.info.0);
    assert!(regex
        .is_match(b"a", &mut Session::new(&mut Account))
        .unwrap());
    // Destructure in the declared/source drop order. The independent factory
    // keeps Core and its Info alive after the compiled implementation drops.
    let Regex { imp, execution } = regex;
    drop(imp);
    assert_eq!(strategy.strong_count(), 1);
    assert_eq!(info.strong_count(), 1);
    let Execution { factory, cache } = execution;
    drop(factory);
    assert!(strategy.upgrade().is_none());
    assert!(info.upgrade().is_none());
    assert!(cache.is_some());
    drop(cache);
}

fn expression(pattern: &str) -> Expression {
    // Property-only tests may inspect non-emitted input. They never install an
    // unanchored strategy or accept a missing constructor as semantic decline.
    let mut text = glob::regex::Text::new();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    text.append(pattern.as_bytes(), &mut session).unwrap();
    let source = glob::regex::Regex(text);
    let tree = source.tree(&mut session).unwrap().ok().unwrap();
    glob::regex::translate::tree(&source, &tree, &mut session)
        .unwrap()
        .ok()
        .unwrap()
}

#[test]
fn impossible_properties_keep_minimum_none_and_exact_anchor_guards() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    info::Info::with(&[], info::Kind::All, &mut session, |info, _| {
        assert_eq!(info.minimum_len(), None);
        assert!(!info.anchored_start() && !info.anchored_end());
        let mut count = Count::default();
        assert!(!search::impossible(&info, 0, &mut Session::new(&mut count)).unwrap());
        assert_eq!(*count.requests.borrow(), [Request::Items(1)]);
        Ok(())
    })
    .unwrap();
    for (pattern, long_impossible) in [
        ("(?-u)a", false),
        ("(?-u)^a", false),
        ("(?-u)a$", false),
        ("(?-u)^a$", true),
        ("(?-u)^a*$", false),
    ] {
        let expression = expression(pattern);
        info::Info::with(
            &[expression],
            info::Kind::All,
            &mut session,
            |info, session| {
                assert_eq!(search::impossible(&info, 8, session)?, long_impossible);
                Ok(())
            },
        )
        .unwrap();
    }
}

#[test]
fn failing_anchored_hir_has_no_minimum_but_still_constructs_the_full_query_cache() {
    // Exercise the real byte-class translator's empty complement, rather than
    // mutating Info properties to manufacture a minimum=None fixture.
    let hir = expression(r"(?-u)^[^\x00-\xFF]$");
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut regex = Regex::new(&[hir], info::Kind::All, &mut session)
        .unwrap()
        .unwrap();
    assert_eq!(regex.imp.info.minimum_len(), None);
    assert!(regex.imp.info.anchored_start() && regex.imp.info.anchored_end());
    assert!(regex.execution.cache.is_none());
    assert!(!regex.is_match(b"", &mut session).unwrap());
    assert!(regex.execution.cache.is_some());
}

#[test]
fn zero_patterns_are_not_mislabeled_as_an_anchored_meta_constructor() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    assert!(matches!(
        Regex::new(&[], info::Kind::All, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish::<()>(Err(Stop::Refused)),
        Err(Error::State)
    ));
    // An empty glob is a nonempty pattern set and does reach the real engine.
    let mut empty = compiled(&[""], info::Kind::LeftmostFirst);
    assert!(empty
        .is_match(b"", &mut Session::new(&mut Account))
        .unwrap());
    assert!(empty.execution.cache.is_some());
}

#[test]
fn post_core_strategy_guards_evaluate_source_short_circuit_order() {
    for (kind, expected) in [(info::Kind::All, 6), (info::Kind::LeftmostFirst, 7)] {
        let regex = compiled(&["a"], kind);
        let mut count = Count::default();
        strategy::finish(&regex.imp.info, &mut Session::new(&mut count)).unwrap();
        assert_eq!(*count.requests.borrow(), vec![Request::Items(1); expected]);
    }
}

#[test]
fn semantic_nfa_failure_is_preserved_and_releases_the_shared_info() {
    let expressions = [expression("(?-u)^a$")];
    let mut account = Account;
    let mut session = Session::new(&mut account);
    info::Info::with(
        &expressions,
        info::Kind::All,
        &mut session,
        |mut info, session| {
            info.test_nfa_limit(Some(0));
            let retained = Arc::downgrade(&info.0);
            assert!(matches!(
                Regex::from_info(info, &expressions, session)?,
                Err(Failure(builder::Failure::SizeLimit(0)))
            ));
            assert!(retained.upgrade().is_none());
            session.check()
        },
    )
    .unwrap();
}
