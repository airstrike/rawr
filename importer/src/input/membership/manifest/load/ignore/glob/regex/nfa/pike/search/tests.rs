use super::super::{info, storage};
use super::*;
use crate::input::membership::manifest::{reference::Account, storage::Buffer, Stop};
use rawr_toml::document::Request;

mod first;

#[derive(Default)]
struct Count {
    calls: usize,
    refuse: Option<usize>,
    allocations: Vec<usize>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let call = self.calls;
        self.calls += 1;
        if self.refuse == Some(call) {
            return Err(call);
        }
        if let Request::Allocation { bytes, .. } = request {
            self.allocations.push(bytes);
        }
        Ok(())
    }
}

fn engine(globs: &[&str], kind: Kind) -> Pike {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let expressions: Vec<_> = globs
        .iter()
        .map(|glob| {
            let regex = super::super::super::super::super::parse(glob, &mut session)
                .unwrap()
                .ok()
                .unwrap()
                .regex(&mut session)
                .unwrap();
            let ast = regex.tree(&mut session).unwrap().ok().unwrap();
            super::super::super::super::translate::tree(&regex, &ast, &mut session)
                .unwrap()
                .ok()
                .unwrap()
        })
        .collect();
    info::Info::with(&expressions, kind, &mut session, |info, session| {
        let mut compiler = super::super::super::compile::Compiler::new(false, session)?;
        let nfa = compiler.build(&expressions, None, session)?.ok().unwrap();
        Pike::new(&info, &nfa, session)
    })
    .unwrap()
}

fn ids(set: &Set) -> Vec<u32> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cursor = 0;
    let mut ids = Vec::new();
    while let Some(id) = set.next(&mut cursor, &mut session).unwrap() {
        ids.push(id);
    }
    ids
}

fn search(engine: &Pike, input: &[u8]) -> Vec<u32> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cache = engine.create_cache(&mut session).unwrap();
    let mut set = Set::new(engine.nfa.0.starts.len(), &mut session).unwrap();
    engine
        .overlapping(&mut cache, input, &mut set, &mut session)
        .unwrap();
    ids(&set)
}

#[test]
fn emitted_patterns_report_all_matches_with_byte_unicode_and_eof_semantics() {
    let re = engine(
        &["a*", "a", "?", "", "{é,中}", "[!a]", "src/**/x.rs"],
        Kind::All,
    );
    for (input, expected) in [
        (&b"a"[..], vec![0, 1, 2]),
        (&b"ab"[..], vec![0]),
        (&b""[..], vec![3]),
        ("é".as_bytes(), vec![4]),
        ("中".as_bytes(), vec![4]),
        (&b"\xff"[..], vec![2, 5]),
        (&b"\n"[..], vec![2, 5]),
        // A bracket class preserves its own ranges; literal_separator only
        // changes the wildcard tokens, not this explicit negated class.
        (&b"/"[..], vec![5]),
        (&b"src/x.rs"[..], vec![6]),
        (&b"src/a/b/x.rs"[..], vec![6]),
        (&b"src/x.rs.backup"[..], vec![]),
    ] {
        assert_eq!(search(&re, input), expected, "input={input:?}");
    }
    assert!(search(&engine(&[], Kind::All), b"").is_empty());
    assert_eq!(search(&engine(&["*", "*"], Kind::All), b""), [0, 1]);
}

#[test]
fn repeated_search_clears_active_state_but_accumulates_pattern_ids_until_clear() {
    let re = engine(&["a", "b", "?", "*"], Kind::All);
    let mut account = Count::default();
    let mut cache = cache::Lazy::none();
    let mut set = Set::new(4, &mut Session::new(&mut account)).unwrap();
    re.overlapping(&mut cache, b"a", &mut set, &mut Session::new(&mut account))
        .unwrap();
    assert_eq!(ids(&set), [0, 2, 3]);
    let allocated = account.allocations.len();
    let table = cache.0.as_ref().unwrap().curr.slots.table.values.as_ptr();
    re.overlapping(&mut cache, b"b", &mut set, &mut Session::new(&mut account))
        .unwrap();
    assert_eq!(ids(&set), [0, 1, 2, 3]);
    assert_eq!(account.allocations.len(), allocated);
    // The two tables can swap; their ownership persists with zero active slots.
    let active = cache.0.as_ref().unwrap();
    assert!(
        active.curr.slots.table.values.as_ptr() == table
            || active.next.slots.table.values.as_ptr() == table
    );
    assert_eq!(
        (
            active.curr.slots.for_captures,
            active.next.slots.for_captures
        ),
        (0, 0)
    );
    assert!(!active.curr.slots.table.values.is_empty());
    set.clear(&mut Session::new(&mut account)).unwrap();
    re.overlapping(&mut cache, b"/", &mut set, &mut Session::new(&mut account))
        .unwrap();
    assert!(ids(&set).is_empty());
    re.overlapping(&mut cache, b"", &mut set, &mut Session::new(&mut account))
        .unwrap();
    assert_eq!(ids(&set), [3]);
    assert_eq!(account.allocations.len(), allocated);
}

#[test]
fn source_match_kind_and_reversed_union_order_are_preserved() {
    // Adapt regex 1.12.3 testdata/set.toml's duplicate-pattern basic10 case
    // to emitted whole-path globs; no general regex parsing is introduced.
    assert_eq!(search(&engine(&["a", "a"], Kind::All), b"a"), [0, 1]);
    assert_eq!(search(&engine(&["a", "a"], Kind::LeftmostFirst), b"a"), [0]);
    assert_eq!(
        search(&engine(&["*", "a", "?"], Kind::All), b"a"),
        [0, 1, 2]
    );
    assert_eq!(
        search(&engine(&["*", "a", "?"], Kind::LeftmostFirst), b"a"),
        [0]
    );
    for count in [2, 3] {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = super::super::super::builder::Builder::new();
        let mut starts = Buffer::new();
        for _ in 0..count {
            builder.start_pattern(&mut session).unwrap().unwrap();
            let matched = builder.add_match(&mut session).unwrap().unwrap();
            let captured = builder
                .capture_start(matched, &mut session)
                .unwrap()
                .unwrap();
            builder.finish_pattern(captured, &mut session).unwrap();
            starts.push(captured, &mut session).unwrap();
        }
        let start = builder
            .add(
                super::super::super::builder::State::UnionReverse { alternates: starts },
                &mut session,
            )
            .unwrap()
            .unwrap();
        let nfa = builder
            .finalize(start, start, false, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let mut re = Pike {
            kind: Kind::LeftmostFirst,
            nfa,
        };
        assert_eq!(search(&re, b""), [count - 1]);
        re.kind = Kind::All;
        assert_eq!(search(&re, b""), (0..count).collect::<Vec<_>>());
    }
}

#[test]
fn epsilon_cycles_terminate_and_keep_epsilon_nodes_in_insertion_order() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = super::super::super::builder::Builder::new();
    builder.start_pattern(&mut session).unwrap().unwrap();
    let matched = builder.add_match(&mut session).unwrap().unwrap();
    let captured = builder
        .capture_start(matched, &mut session)
        .unwrap()
        .unwrap();
    let root = Id(builder.states().len() as u32);
    let mut alternates = Buffer::new();
    alternates.push(root, &mut session).unwrap();
    alternates.push(captured, &mut session).unwrap();
    assert_eq!(
        builder
            .add(
                super::super::super::builder::State::Union { alternates },
                &mut session
            )
            .unwrap()
            .unwrap(),
        root
    );
    builder.finish_pattern(root, &mut session).unwrap();
    let nfa = builder
        .finalize(root, root, false, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    let re = Pike {
        kind: Kind::All,
        nfa,
    };
    let mut cache = cache::Lazy::none();
    let mut set = Set::new(1, &mut session).unwrap();
    re.overlapping(&mut cache, b"", &mut set, &mut session)
        .unwrap();
    assert_eq!(ids(&set), [0]);
    let active = &cache.0.as_ref().unwrap().curr;
    assert_eq!(
        &active.dense.values[..active.length],
        &[root, captured, matched]
    );
    assert!(cache.0.as_ref().unwrap().stack.values.is_empty());
}

#[test]
fn sparse_transitions_scan_in_order_and_stop_before_later_ranges() {
    let re = engine(&["[az]"], Kind::All);
    let sparse = re
        .nfa
        .0
        .states
        .iter()
        .position(|s| matches!(s, State::Sparse { .. }))
        .unwrap();
    let mut cache = cache::Lazy::none();
    let cache = cache.get(&re, &mut Session::new(&mut Account)).unwrap();
    cache.setup(&mut Session::new(&mut Account)).unwrap();
    let mut before_first = Count::default();
    assert_eq!(
        re.next(
            &mut cache.stack,
            &mut cache.curr,
            &mut cache.next,
            b"\0",
            0,
            Id(sparse as u32),
            &mut Session::new(&mut before_first)
        )
        .unwrap(),
        None
    );
    let mut between = Count::default();
    assert_eq!(
        re.next(
            &mut cache.stack,
            &mut cache.curr,
            &mut cache.next,
            b"b",
            0,
            Id(sparse as u32),
            &mut Session::new(&mut between)
        )
        .unwrap(),
        None
    );
    assert_eq!(between.calls, before_first.calls + 1);
    assert_eq!(search(&re, b"a"), [0]);
    assert_eq!(search(&re, b"z"), [0]);
    assert!(search(&re, b"b").is_empty());
}

#[test]
fn full_or_undersized_pattern_sets_finish_the_current_batch() {
    let re = engine(&["", ""], Kind::All);
    let mut calls = Vec::new();
    for capacity in [0, 1, 2] {
        let mut cache = cache::Lazy::none();
        cache.get(&re, &mut Session::new(&mut Account)).unwrap();
        let mut set = Set::new(capacity, &mut Session::new(&mut Account)).unwrap();
        let mut account = Count::default();
        re.overlapping(&mut cache, b"", &mut set, &mut Session::new(&mut account))
            .unwrap();
        assert_eq!(ids(&set), (0..capacity as u32).collect::<Vec<_>>());
        calls.push(account.calls);
    }
    assert_eq!(calls, vec![calls[0]; 3]);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut set = Set::new(2, &mut session).unwrap();
    assert_eq!(set.insert(1, &mut session).unwrap(), Ok(true));
    assert_eq!(set.insert(1, &mut session).unwrap(), Ok(false));
    assert_eq!(
        set.insert(9, &mut session).unwrap(),
        Err(super::super::set::Full {
            attempted: 9,
            capacity: 2,
        })
    );
    session.check().unwrap();
    assert_eq!(ids(&set), [1]);
    assert_eq!(set.insert(0, &mut session).unwrap(), Ok(true));
    assert_eq!(ids(&set), [0, 1]);
}

#[test]
fn every_search_refusal_is_terminal_including_after_a_partial_match() {
    let re = engine(&["a", "a", "a", "a", "a", "a", "a", "a"], Kind::All);
    let mut baseline = Count::default();
    let mut cache = cache::Lazy::none();
    let mut set = Set::new(1, &mut Session::new(&mut baseline)).unwrap();
    let before = baseline.calls;
    re.overlapping(&mut cache, b"a", &mut set, &mut Session::new(&mut baseline))
        .unwrap();
    // Cache arrays plus initial and widened continuation ownership are reached.
    assert!(baseline.allocations.len() >= 9);
    let mut refused_after_match = false;
    for refuse in before..baseline.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut cache = cache::Lazy::none();
        let mut set = Set::new(1, &mut session).unwrap();
        assert!(matches!(
            re.overlapping(&mut cache, b"a", &mut set, &mut session),
            Err(Stop::Refused)
        ));
        refused_after_match |= set.is_full();
        assert!(matches!(set.clear(&mut session), Err(Stop::Refused)));
        assert!(matches!(set.next(&mut 0, &mut session), Err(Stop::Refused)));
        assert!(matches!(
            re.overlapping(&mut cache, b"a", &mut set, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refuse));
        assert_eq!(account.calls, refuse + 1);
    }
    assert!(refused_after_match);
}

#[test]
fn set_ownership_clear_and_ordered_iteration_are_prospectively_admitted() {
    let mut count = Count::default();
    let mut set = Set::new(5, &mut Session::new(&mut count)).unwrap();
    assert_eq!(count.allocations, [5]);
    for refuse in 0..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(Set::new(5, &mut session), Err(Stop::Refused)));
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert_eq!(account.calls, refuse + 1);
    }
    for pattern in [4, 1, 3] {
        set.insert(pattern, &mut Session::new(&mut count))
            .unwrap()
            .unwrap();
    }
    assert_eq!(ids(&set), [1, 3, 4]);
    let before = count.calls;
    set.clear(&mut Session::new(&mut count)).unwrap();
    for refuse in before..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut set = Set::new(5, &mut session).unwrap();
        for pattern in [4, 1, 3] {
            set.insert(pattern, &mut session).unwrap().unwrap();
        }
        assert!(matches!(set.clear(&mut session), Err(Stop::Refused)));
        assert!(matches!(set.next(&mut 0, &mut session), Err(Stop::Refused)));
        assert_eq!(account.calls, refuse + 1);
    }
    let mut set = Set::new(5, &mut Session::new(&mut Account)).unwrap();
    set.insert(4, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    let mut count = Count::default();
    assert_eq!(
        set.next(&mut 0, &mut Session::new(&mut count)).unwrap(),
        Some(4)
    );
    for refuse in 0..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        assert!(matches!(
            set.next(&mut 0, &mut Session::new(&mut account)),
            Err(Stop::Refused)
        ));
        assert_eq!(account.calls, refuse + 1);
    }
}

#[test]
fn continuation_growth_refusal_retains_old_frames_and_never_uses_bonus_capacity() {
    let mut stack = storage::Array::new();
    let mut count = Count::default();
    stack
        .push(cache::Frame::Explore(Id(7)), &mut Session::new(&mut count))
        .unwrap();
    let before = count.calls;
    stack
        .extend_reverse(
            &[Id(1), Id(2), Id(3), Id(4), Id(5)],
            &mut Session::new(&mut count),
        )
        .unwrap();
    assert_eq!(stack.values.len(), 6);
    assert!(matches!(stack.values[1], cache::Frame::Explore(Id(5))));
    assert!(matches!(stack.values[5], cache::Frame::Explore(Id(1))));
    for refuse in before..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut stack = storage::Array::new();
        stack
            .push(cache::Frame::Explore(Id(7)), &mut session)
            .unwrap();
        assert!(matches!(
            stack.extend_reverse(&[Id(1), Id(2), Id(3), Id(4), Id(5)], &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(stack.values[0], cache::Frame::Explore(Id(7))));
        assert!(matches!(stack.pop(&mut session), Err(Stop::Refused)));
        assert_eq!(account.calls, refuse + 1);
    }
    let mut values = Vec::with_capacity(64);
    values.push(cache::Frame::Explore(Id(7)));
    let mut stack = storage::Array {
        values,
        admitted: 1,
    };
    let mut account = Count::default();
    stack
        .push(
            cache::Frame::Explore(Id(8)),
            &mut Session::new(&mut account),
        )
        .unwrap();
    assert_eq!(
        account.allocations,
        [4 * std::mem::size_of::<cache::Frame>()]
    );
}
