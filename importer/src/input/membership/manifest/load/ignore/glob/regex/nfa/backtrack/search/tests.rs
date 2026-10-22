use super::super::{
    info::Kind,
    tests::{wrapper, Count},
    Backtrack, Config,
};
use super::*;
use crate::input::membership::manifest::{reference::Account, storage::Buffer, Stop};

fn found(wrapper: &Backtrack, bytes: &[u8]) -> Option<Hit> {
    wrapper
        .0
        .as_ref()
        .unwrap()
        .search(
            &mut cache::Lazy::none(),
            bytes,
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .unwrap()
}

#[test]
fn emitted_search_keeps_priority_byte_unicode_and_end_of_input_semantics() {
    let engine = wrapper(
        &["a*", "a", "?", "", "{é,中}", "[!a]", "src/**/x.rs"],
        Kind::LeftmostFirst,
    );
    for (input, pattern) in [
        (&b"a"[..], Some(0)),
        (&b"ab"[..], Some(0)),
        (&b""[..], Some(3)),
        ("é".as_bytes(), Some(4)),
        ("中".as_bytes(), Some(4)),
        (&b"\xff"[..], Some(2)),
        (&b"\n"[..], Some(2)),
        (&b"/"[..], Some(5)),
        (&b"src/x.rs"[..], Some(6)),
        (&b"src/a/b/x.rs"[..], Some(6)),
        (&b"src/x.rs.backup"[..], None),
    ] {
        assert_eq!(
            found(&engine, input),
            pattern.map(|pattern| Hit {
                pattern,
                offset: input.len()
            }),
            "input={input:?}"
        );
    }
    assert_eq!(found(&wrapper(&[], Kind::LeftmostFirst), b""), None);
    assert_eq!(
        found(&wrapper(&["*", "*"], Kind::LeftmostFirst), b""),
        Some(Hit {
            pattern: 0,
            offset: 0
        })
    );
    assert_eq!(
        found(&wrapper(&["*", "a", "?"], Kind::LeftmostFirst), b"a"),
        Some(Hit {
            pattern: 0,
            offset: 1
        })
    );
}

#[test]
fn boolean_wrapper_skips_all_or_earliest_long_input_before_initializing_cache() {
    let all = wrapper(&["*.rs"], Kind::All);
    let first = wrapper(&["*.rs"], Kind::LeftmostFirst);
    let mut lazy = cache::Lazy::none();
    let mut count = Count::default();
    assert_eq!(
        all.is_match(&mut lazy, b"foo.rs", &mut Session::new(&mut count))
            .unwrap(),
        None
    );
    assert!(lazy.0.is_none());
    assert_eq!(
        first
            .is_match(&mut lazy, &[b'x'; 129], &mut Session::new(&mut count))
            .unwrap(),
        None
    );
    assert!(lazy.0.is_none());
    assert!(count.allocations.is_empty());
    assert_eq!(
        first
            .is_match(&mut lazy, b"foo.rs", &mut Session::new(&mut count))
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        first
            .is_match(&mut lazy, b"foo.js", &mut Session::new(&mut count))
            .unwrap(),
        Some(false)
    );
}

#[test]
fn single_byte_wildcard_and_negated_class_cover_every_byte_without_utf8_filtering() {
    let question = wrapper(&["?"], Kind::LeftmostFirst);
    let negated = wrapper(&["[!a]"], Kind::LeftmostFirst);
    let mut question_cache = cache::Lazy::none();
    let mut negated_cache = cache::Lazy::none();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    for byte in u8::MIN..=u8::MAX {
        assert_eq!(
            question
                .is_match(&mut question_cache, &[byte], &mut session)
                .unwrap(),
            Some(byte != b'/')
        );
        assert_eq!(
            negated
                .is_match(&mut negated_cache, &[byte], &mut session)
                .unwrap(),
            Some(byte != b'a')
        );
    }
    assert_eq!(
        question
            .is_match(&mut question_cache, "é".as_bytes(), &mut session)
            .unwrap(),
        Some(false)
    );
    assert_eq!(
        question
            .is_match(&mut question_cache, b"", &mut session)
            .unwrap(),
        Some(false)
    );
}

#[test]
fn greedy_retries_agree_with_exhaustive_short_string_language() {
    let re = wrapper(&["a*a*a*b"], Kind::LeftmostFirst);
    let mut lazy = cache::Lazy::none();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    for length in 0..=9 {
        for bits in 0..(1 << length) {
            let bytes: Vec<_> = (0..length)
                .map(|bit| if bits & (1 << bit) == 0 { b'a' } else { b'b' })
                .collect();
            let expected = bytes.first() == Some(&b'a')
                && bytes.last() == Some(&b'b')
                && bytes.iter().filter(|&&byte| byte == b'a').count() >= 3;
            assert_eq!(
                re.is_match(&mut lazy, &bytes, &mut session).unwrap(),
                Some(expected),
                "bytes={bytes:?}"
            );
        }
    }
    assert_eq!(
        re.is_match(&mut lazy, b"aaa/b", &mut session).unwrap(),
        Some(false)
    );
}

#[test]
fn successful_search_keeps_pending_alternatives_until_next_setup_without_captures() {
    let re = wrapper(&["a", "a", "a"], Kind::LeftmostFirst);
    let engine = re.0.as_ref().unwrap();
    let mut lazy = cache::Lazy::none();
    let mut count = Count::default();
    let hit = engine
        .search(&mut lazy, b"a", &mut Session::new(&mut count))
        .unwrap()
        .unwrap();
    assert_eq!(
        hit,
        Some(Hit {
            pattern: 0,
            offset: 1
        })
    );
    assert_eq!(lazy.0.as_ref().unwrap().stack.values.len(), 2);
    assert!(lazy
        .0
        .as_ref()
        .unwrap()
        .stack
        .values
        .iter()
        .all(|frame| matches!(frame, cache::Frame::Step { .. })));
    let allocations = count.allocations.len();
    let stack_pointer = lazy.0.as_ref().unwrap().stack.values.as_ptr();
    assert_eq!(
        engine
            .search(&mut lazy, b"b", &mut Session::new(&mut count))
            .unwrap()
            .unwrap(),
        None
    );
    assert!(lazy.0.as_ref().unwrap().stack.values.is_empty());
    assert_eq!(
        lazy.0.as_ref().unwrap().stack.values.as_ptr(),
        stack_pointer
    );
    assert_eq!(count.allocations.len(), allocations);
}

#[test]
fn epsilon_cycles_stop_at_visited_pairs_and_reversed_union_priority_is_preserved() {
    use super::super::super::builder;
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = builder::Builder::new();
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
            .add(builder::State::Union { alternates }, &mut session)
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
    let engine = Engine {
        config: Config {
            pre: Some(None),
            visited_capacity: None,
        },
        nfa,
    };
    let mut lazy = cache::Lazy::none();
    assert_eq!(
        engine
            .search(&mut lazy, b"", &mut session)
            .unwrap()
            .unwrap(),
        Some(Hit {
            pattern: 0,
            offset: 0
        })
    );
    assert_eq!(
        lazy.0
            .as_ref()
            .unwrap()
            .visited
            .bitset
            .values
            .iter()
            .map(|word| word.count_ones())
            .sum::<u32>(),
        3
    );
    assert!(lazy.0.as_ref().unwrap().stack.values.is_empty());

    for count in [2, 3] {
        let mut builder = builder::Builder::new();
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
                builder::State::UnionReverse { alternates: starts },
                &mut session,
            )
            .unwrap()
            .unwrap();
        let nfa = builder
            .finalize(start, start, false, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let engine = Engine {
            config: Config {
                pre: Some(None),
                visited_capacity: None,
            },
            nfa,
        };
        assert_eq!(
            engine
                .search(&mut cache::Lazy::none(), b"", &mut session)
                .unwrap()
                .unwrap(),
            Some(Hit {
                pattern: count - 1,
                offset: 0
            })
        );
    }
}

#[test]
fn every_search_refusal_is_terminal_with_fresh_or_reused_cache() {
    let re = wrapper(&["{a*,b*,c*}"], Kind::LeftmostFirst);
    for reused in [false, true] {
        let mut count = Count::default();
        let mut lazy = cache::Lazy::none();
        if reused {
            re.is_match(&mut lazy, b"bbb", &mut Session::new(&mut Account))
                .unwrap();
        }
        assert_eq!(
            re.is_match(&mut lazy, b"cccc", &mut Session::new(&mut count))
                .unwrap(),
            Some(true)
        );
        for refuse in 0..count.calls {
            let mut lazy = cache::Lazy::none();
            if reused {
                re.is_match(&mut lazy, b"bbb", &mut Session::new(&mut Account))
                    .unwrap();
            }
            let mut account = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut session = Session::new(&mut account);
            assert!(matches!(
                re.is_match(&mut lazy, b"cccc", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert!(matches!(
                re.is_match(&mut lazy, b"zzz", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                re.is_match(&mut lazy, &[b'x'; 129], &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(account.calls, refuse + 1);
        }
    }
}

#[test]
fn branch_stack_extension_reserves_before_moving_reversed_frames() {
    let alternatives = [Id(1), Id(2), Id(3), Id(4), Id(5)];
    let mut count = Count::default();
    let mut stack = Array::<cache::Frame>::new();
    stack
        .extend_reverse(&alternatives, 17, &mut Session::new(&mut count))
        .unwrap();
    assert_eq!(stack.admitted, 5);
    assert_eq!(
        count.allocations,
        [(
            5 * std::mem::size_of::<cache::Frame>(),
            std::mem::align_of::<cache::Frame>()
        )]
    );
    assert_eq!(
        stack
            .values
            .iter()
            .map(|frame| match frame {
                cache::Frame::Step { sid, at: 17 } => sid.index(),
                _ => panic!("step"),
            })
            .collect::<Vec<_>>(),
        [5, 4, 3, 2, 1]
    );
    for refuse in 0..count.calls {
        let mut stack = Array::<cache::Frame>::new();
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            stack.extend_reverse(&alternatives, 17, &mut session),
            Err(Stop::Refused)
        ));
        assert!(stack.values.len() <= alternatives.len());
        assert!(stack.values.len() <= stack.admitted);
        assert!(matches!(
            stack.extend_reverse(&alternatives, 17, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(account.calls, refuse + 1);
    }
}
