use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Default)]
pub(super) struct Count {
    pub(super) requests: Rc<RefCell<Vec<Request>>>,
    pub(super) refuse: Option<usize>,
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

pub(super) fn expressions(globs: &[&str]) -> Vec<Expression> {
    use crate::input::membership::manifest::load::ignore::glob;
    let mut account = Account;
    let mut session = Session::new(&mut account);
    globs
        .iter()
        .map(|glob| {
            let regex = glob::parse(glob, &mut session)
                .unwrap()
                .ok()
                .unwrap()
                .regex(&mut session)
                .unwrap();
            let tree = regex.tree(&mut session).unwrap().ok().unwrap();
            glob::regex::translate::tree(&regex, &tree, &mut session)
                .unwrap()
                .ok()
                .unwrap()
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(super) enum Case {
    Default,
    DenseDecline,
    BothDecline,
    OnlyDense,
    OnlyHybrid,
    Neither,
}

impl Case {
    fn apply(self, info: &mut info::Info) {
        match self {
            Self::Default => {}
            Self::DenseDecline => info.test_dfa_limit(Some(0)),
            Self::BothDecline => {
                info.test_dfa_limit(Some(0));
                info.test_hybrid_capacity(0);
            }
            Self::OnlyDense => info.test_engines(true, false),
            Self::OnlyHybrid => info.test_engines(false, true),
            Self::Neither => info.test_engines(false, false),
        }
    }
}

pub(super) fn info(expressions: &[Expression], kind: info::Kind, case: Case) -> info::Info {
    info::Info::with(
        expressions,
        kind,
        &mut Session::new(&mut Account),
        |mut info, _| {
            case.apply(&mut info);
            Ok(info)
        },
    )
    .unwrap()
}

// Independently assemble the source's existing component operations and record
// their boundaries. Comparing all requests catches reordered/skipped ownership,
// rather than merely checking that Core contains engines at the end.
fn source_requests(
    expressions: &[Expression],
    kind: info::Kind,
    case: Case,
) -> (Vec<Request>, Vec<usize>) {
    let info = info(expressions, kind, case);
    let mut account = Count::default();
    let requests = Rc::clone(&account.requests);
    let mut session = Session::new(&mut account);
    let mut boundaries = vec![0];
    let mut compiler = Compiler::new(false, &mut session).unwrap();
    let nfa = compiler
        .build(expressions, info.nfa_size_limit(), &mut session)
        .unwrap()
        .unwrap();
    drop(compiler);
    boundaries.push(requests.borrow().len());
    let pike = pike::Pike::new(&info, &nfa, &mut session).unwrap();
    boundaries.push(requests.borrow().len());
    let backtrack = backtrack::Backtrack::new(&info, &nfa, &mut session).unwrap();
    boundaries.push(requests.borrow().len());
    assert!(info.onepass());
    assert!(!info.onepass_candidate());
    session.items(1).unwrap();
    boundaries.push(requests.borrow().len());
    let (reverse, dense) = dfa::Prepared::begin(&info, &nfa, expressions, &mut session)
        .unwrap()
        .unwrap()
        .complete(&mut session)
        .unwrap();
    boundaries.push(requests.borrow().len());
    let hybrid = if info.hybrid() && !dense.has_engine() {
        hybrid::Hybrid::new(&info, &nfa, &reverse, &mut session).unwrap()
    } else {
        hybrid::Hybrid::none()
    };
    boundaries.push(requests.borrow().len());
    drop((pike, backtrack, hybrid, dense, reverse, nfa, info));
    let recorded = requests.borrow().clone();
    (recorded, boundaries)
}

#[test]
fn constructor_requests_follow_source_order_for_dense_and_hybrid_outcomes() {
    let long = "a".repeat(64);
    for (pattern, case) in [
        ("a", Case::Default),
        (long.as_str(), Case::Default),
        ("a", Case::DenseDecline),
        ("a", Case::BothDecline),
    ] {
        let expressions = expressions(&[pattern]);
        for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
            let (expected, _) = source_requests(&expressions, kind, case);
            let mut account = Count::default();
            let core = Core::new(
                info(&expressions, kind, case),
                &expressions,
                &mut Session::new(&mut account),
            )
            .unwrap()
            .unwrap();
            assert_eq!(*account.requests.borrow(), expected);
            assert_eq!(core.onepass, Onepass::NotUseful);
            assert!(core.pre.is_none());
        }
    }
}

#[test]
fn emitted_unicode_and_group_spelling_do_not_make_onepass_useful() {
    for glob in ["", "{é,中}", "(name)", "**/[é-中].rs"] {
        let expressions = expressions(&[glob]);
        let info = info(&expressions, info::Kind::LeftmostFirst, Case::Default);
        let mut account = Count::default();
        assert!(info.onepass());
        assert_eq!(
            Onepass::new(&info, &mut Session::new(&mut account)).unwrap(),
            Onepass::NotUseful
        );
        assert_eq!(*account.requests.borrow(), [Request::Items(1)]);
    }
}

#[test]
fn dense_success_suppresses_hybrid_and_retains_all_required_nfa_owners() {
    let expressions = expressions(&["a", "a"]);
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        let core = Core::new(
            info(&expressions, kind, Case::Default),
            &expressions,
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .unwrap();
        assert!(core.dfa.has_engine());
        assert!(!core.hybrid.has_engine());
        assert_eq!(core.info.pattern_len(), 2);
        assert_eq!(Arc::strong_count(&core.info.0), 1);
        assert_eq!(
            Arc::strong_count(&core.nfa.0),
            if kind == info::Kind::All { 2 } else { 3 }
        );
        let reverse = core.reverse.as_ref().unwrap();
        assert!(reverse.0.reverse);
        assert!(!reverse.0.has_capture);
        assert_eq!(Arc::strong_count(&reverse.0), 1);
        let forward = Arc::downgrade(&core.nfa.0);
        let reverse = Arc::downgrade(&reverse.0);
        let properties = Arc::downgrade(&core.info.0);
        drop(core);
        assert!(forward.upgrade().is_none());
        assert!(reverse.upgrade().is_none());
        assert!(properties.upgrade().is_none());
    }
}

#[test]
fn heuristic_and_semantic_dense_decline_reach_hybrid_without_losing_fallback_engines() {
    let long = "a".repeat(64);
    for (pattern, case) in [(long.as_str(), Case::Default), ("a", Case::DenseDecline)] {
        let expressions = expressions(&[pattern]);
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let core = Core::new(
            info(&expressions, info::Kind::LeftmostFirst, case),
            &expressions,
            &mut session,
        )
        .unwrap()
        .unwrap();
        assert!(!core.dfa.has_engine());
        assert!(core.hybrid.has_engine());
        assert_eq!(Arc::strong_count(&core.nfa.0), 4);
        assert_eq!(Arc::strong_count(&core.reverse.as_ref().unwrap().0), 2);
        assert!(session.check().is_ok());
    }
    let expressions = expressions(&["a"]);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let core = Core::new(
        info(&expressions, info::Kind::LeftmostFirst, Case::BothDecline),
        &expressions,
        &mut session,
    )
    .unwrap()
    .unwrap();
    assert!(!core.dfa.has_engine() && !core.hybrid.has_engine());
    assert_eq!(Arc::strong_count(&core.nfa.0), 3);
    assert_eq!(Arc::strong_count(&core.reverse.as_ref().unwrap().0), 1);
    assert!(session.check().is_ok());
}

#[test]
fn engine_configuration_controls_reverse_and_hybrid_construction() {
    let expressions = expressions(&[&"a".repeat(64)]);
    for (case, reverse, hybrid) in [
        (Case::OnlyDense, true, false),
        (Case::OnlyHybrid, true, true),
        (Case::Neither, false, false),
    ] {
        let core = Core::new(
            info(&expressions, info::Kind::All, case),
            &expressions,
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .unwrap();
        assert_eq!(core.reverse.is_some(), reverse);
        assert_eq!(core.hybrid.has_engine(), hybrid);
        assert!(!core.dfa.has_engine());
    }
}

#[test]
fn nfa_semantic_failure_stops_before_engine_ownership_without_poisoning_the_session() {
    let expressions = expressions(&["a"]);
    let mut info = info(&expressions, info::Kind::All, Case::Default);
    info.test_nfa_limit(Some(0));
    let owner = Arc::downgrade(&info.0);
    let mut expected = Count::default();
    let mut compiler = Compiler::new(false, &mut Session::new(&mut expected)).unwrap();
    assert!(matches!(
        compiler
            .build(&expressions, Some(0), &mut Session::new(&mut expected))
            .unwrap(),
        Err(builder::Failure::SizeLimit(0))
    ));
    let mut account = Count::default();
    let mut session = Session::new(&mut account);
    assert!(matches!(
        Core::new(info, &expressions, &mut session).unwrap(),
        Err(builder::Failure::SizeLimit(0))
    ));
    assert!(session.check().is_ok());
    assert!(owner.upgrade().is_none());
    assert_eq!(*account.requests.borrow(), *expected.requests.borrow());
}

#[test]
fn refusal_at_each_engine_boundary_keeps_first_cause_and_never_reaches_fallback() {
    let long = "a".repeat(64);
    for (pattern, case) in [
        ("a", Case::Default),
        (long.as_str(), Case::Default),
        ("a", Case::BothDecline),
    ] {
        let expressions = expressions(&[pattern]);
        let (expected, boundaries) = source_requests(&expressions, info::Kind::LeftmostFirst, case);
        let mut refusals = Vec::new();
        for pair in boundaries.windows(2) {
            if pair[0] < pair[1] {
                refusals.extend([pair[0], pair[1] - 1]);
                if let Some(offset) = expected[pair[0]..pair[1]]
                    .iter()
                    .position(|request| matches!(request, Request::Allocation { .. }))
                {
                    refusals.push(pair[0] + offset);
                }
            }
        }
        refusals.sort_unstable();
        refusals.dedup();
        for refuse in refusals {
            let info = info(&expressions, info::Kind::LeftmostFirst, case);
            let owner = Arc::downgrade(&info.0);
            let mut account = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut session = Session::new(&mut account);
            let result = Core::new(info, &expressions, &mut session);
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert!(matches!(
                Compiler::new(false, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                session.finish(result),
                Err(Error::Admission(index)) if index == refuse
            ));
            assert!(owner.upgrade().is_none());
            assert_eq!(account.requests.borrow().as_slice(), &expected[..=refuse]);
        }
    }
}

#[test]
fn full_constructor_keeps_prior_usage_and_exact_cumulative_item_and_allocation_floors() {
    struct Budget {
        items: usize,
        bytes: usize,
    }
    impl Admission for Budget {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            let (remaining, amount) = match request {
                Request::Items(amount) => (&mut self.items, amount),
                Request::Allocation { bytes, .. } => (&mut self.bytes, bytes),
                Request::String(_) => return Ok(()),
            };
            *remaining = remaining.checked_sub(amount).ok_or(())?;
            Ok(())
        }
    }
    fn construct<A: Admission>(
        expressions: &[Expression],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Core, builder::Failure>> {
        let mut prior = super::super::storage::Array::new();
        prior.push(7_u32, session)?;
        let result = info::Info::with(
            expressions,
            info::Kind::LeftmostFirst,
            session,
            |info, session| Core::new(info, expressions, session),
        );
        drop(prior);
        result
    }
    let long = "a".repeat(64);
    for pattern in ["a", long.as_str()] {
        let expressions = expressions(&[pattern]);
        let mut count = Count::default();
        drop(
            construct(&expressions, &mut Session::new(&mut count))
                .unwrap()
                .unwrap(),
        );
        let (mut items, mut bytes) = (0, 0);
        for request in count.requests.borrow().iter() {
            match request {
                Request::Items(amount) => items += *amount,
                Request::Allocation { bytes: amount, .. } => bytes += *amount,
                Request::String(_) => {}
            }
        }
        for (item_floor, byte_floor, success) in [
            (items, bytes, true),
            (items - 1, bytes, false),
            (items, bytes - 1, false),
        ] {
            let mut budget = Budget {
                items: item_floor,
                bytes: byte_floor,
            };
            let mut session = Session::new(&mut budget);
            let result = construct(&expressions, &mut session);
            if success {
                assert!(matches!(session.finish(result), Ok(Ok(_))));
                assert_eq!((budget.items, budget.bytes), (0, 0));
            } else {
                assert!(matches!(result, Err(Stop::Refused)));
                assert!(matches!(session.finish(result), Err(Error::Admission(()))));
            }
        }
    }
}
