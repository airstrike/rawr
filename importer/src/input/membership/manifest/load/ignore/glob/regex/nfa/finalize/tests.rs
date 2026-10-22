use super::super::{builder, compile::Compiler};
use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};

fn expression(glob: &str) -> super::super::super::translate::Expression {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let regex = super::super::super::super::parse(glob, &mut session)
        .unwrap()
        .ok()
        .unwrap()
        .regex(&mut session)
        .unwrap();
    let tree = regex.tree(&mut session).unwrap().ok().unwrap();
    super::super::super::translate::tree(&regex, &tree, &mut session)
        .unwrap()
        .ok()
        .unwrap()
}

// Test-only ordered graph interpreter. It exercises final transitions and
// capture slots, without providing any production matching implementation.
fn first(graph: &Graph, start: Id, bytes: &[u8]) -> Option<(u32, Vec<Option<usize>>)> {
    let mut stack = vec![(start, 0, vec![None; 2 * graph.groups.ranges.len()])];
    let mut seen = Vec::new();
    while let Some((id, at, mut slots)) = stack.pop() {
        if seen.contains(&(id, at)) {
            continue;
        }
        seen.push((id, at));
        match &graph.states[id.index()] {
            State::ByteRange { trans } => {
                if bytes
                    .get(at)
                    .is_some_and(|&byte| trans.start <= byte && byte <= trans.end)
                {
                    stack.push((trans.next, at + 1, slots));
                }
            }
            State::Sparse { transitions } => {
                for trans in transitions.iter().rev() {
                    if bytes
                        .get(at)
                        .is_some_and(|&byte| trans.start <= byte && byte <= trans.end)
                    {
                        stack.push((trans.next, at + 1, slots.clone()));
                    }
                }
            }
            State::Look { look, next } => {
                if (*look == 1 && at == 0) || (*look == 2 && at == bytes.len()) {
                    stack.push((*next, at, slots));
                }
            }
            State::Capture { next, slot, .. } => {
                slots[*slot as usize] = Some(at);
                stack.push((*next, at, slots));
            }
            State::Union { alternates } => {
                for &next in alternates.iter().rev() {
                    stack.push((next, at, slots.clone()));
                }
            }
            State::BinaryUnion { alt1, alt2 } => {
                stack.push((*alt2, at, slots.clone()));
                stack.push((*alt1, at, slots));
            }
            State::Fail => {}
            State::Match { pattern } => return Some((*pattern, slots)),
        }
    }
    None
}

#[test]
fn finalized_batches_preserve_priority_captures_direction_and_properties() {
    let expressions = [
        expression("a*"),
        expression("abc"),
        expression("{é,中}"),
        expression(""),
    ];
    for reverse in [false, true] {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut compiler = Compiler::new(reverse, &mut session).unwrap();
        let nfa = compiler
            .build(&expressions, None, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let graph = &nfa.0;
        assert_eq!(graph.reverse, reverse);
        assert!(!graph.utf8);
        assert_eq!(graph.line_terminator, b'\n');
        assert_eq!(graph.anchored, graph.unanchored);
        assert_eq!(graph.starts.len(), 4);
        assert_eq!(graph.has_capture, !reverse);
        assert!(graph.has_empty);
        assert_eq!(graph.looks, 3);
        assert_eq!(graph.prefix_looks, 3);
        assert_eq!(graph.groups.ranges.len(), if reverse { 0 } else { 4 });
        for (text, expected) in [
            ("abc", Some(0)),
            ("é", Some(2)),
            ("中", Some(2)),
            ("", Some(3)),
            ("z", None),
        ] {
            let mut bytes = text.as_bytes().to_vec();
            if reverse {
                bytes.reverse();
            }
            let result = first(graph, graph.anchored, &bytes);
            assert_eq!(
                result.as_ref().map(|(pattern, _)| *pattern),
                expected,
                "{text} / {reverse}"
            );
            if let Some((pattern, slots)) = result
                && !reverse
            {
                assert_eq!(slots[pattern as usize * 2], Some(0));
                assert_eq!(slots[pattern as usize * 2 + 1], Some(bytes.len()));
                assert_eq!(slots.iter().filter(|slot| slot.is_some()).count(), 2);
            }
        }
        assert_eq!(first(graph, graph.starts[1], b"abc").is_some(), !reverse);
        let weak = Arc::downgrade(&nfa.0);
        drop(nfa);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn finalization_eliminates_empty_chains_and_singleton_unions_without_reordering() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = builder::Builder::new();
    builder.start_pattern(&mut session).unwrap().unwrap();
    let start = builder
        .add(builder::State::Empty { next: Id(1) }, &mut session)
        .unwrap()
        .unwrap();
    let mut alternates = Buffer::new();
    alternates.push(Id(2), &mut session).unwrap();
    builder
        .add(builder::State::UnionReverse { alternates }, &mut session)
        .unwrap()
        .unwrap();
    builder
        .add(
            builder::State::ByteRange {
                trans: Transition {
                    start: b'a',
                    end: b'z',
                    next: Id(3),
                },
            },
            &mut session,
        )
        .unwrap()
        .unwrap();
    builder
        .add(builder::State::Empty { next: Id(4) }, &mut session)
        .unwrap()
        .unwrap();
    builder.add_match(&mut session).unwrap().unwrap();
    builder.finish_pattern(start, &mut session).unwrap();
    let nfa = builder
        .finalize(start, Id(1), false, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    assert_eq!(
        builder.states().len(),
        5,
        "finalization borrows reusable builder storage"
    );
    assert_eq!(nfa.0.states.len(), 2);
    assert_eq!(
        (nfa.0.anchored, nfa.0.unanchored, nfa.0.starts[0]),
        (Id(0), Id(0), Id(0))
    );
    assert!(matches!(
        nfa.0.states[0],
        State::ByteRange {
            trans: Transition { next: Id(1), .. }
        }
    ));
    assert!(matches!(nfa.0.states[1], State::Match { pattern: 0 }));
    assert_eq!(first(&nfa.0, nfa.0.anchored, b"q").unwrap().0, 0);
    assert!(first(&nfa.0, nfa.0.anchored, b"0").is_none());
    assert_eq!(nfa.0.classes[b'a' as usize], nfa.0.classes[b'z' as usize]);
    assert_ne!(nfa.0.classes[b'a' as usize], nfa.0.classes[b'`' as usize]);
    assert_ne!(nfa.0.classes[b'z' as usize], nfa.0.classes[b'{' as usize]);
    assert!(!nfa.0.has_empty);
}

#[test]
fn sparse_and_union_reductions_keep_ordered_final_transitions() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = builder::Builder::new();
    for _ in 0..3 {
        builder.start_pattern(&mut session).unwrap().unwrap();
        let matched = builder.add_match(&mut session).unwrap().unwrap();
        builder.finish_pattern(matched, &mut session).unwrap();
    }
    let mut roots = Vec::new();
    for count in 0..4 {
        let mut alternates = Buffer::new();
        for index in 0..count {
            alternates.push(Id(index), &mut session).unwrap();
        }
        roots.push(
            builder
                .add(builder::State::UnionReverse { alternates }, &mut session)
                .unwrap()
                .unwrap(),
        );
    }
    let mut sparse = Vec::new();
    for count in 0..3 {
        let mut transitions = Buffer::new();
        for index in 0..count {
            transitions
                .push(
                    Transition {
                        start: b'a' + index as u8,
                        end: b'a' + index as u8,
                        next: Id(index),
                    },
                    &mut session,
                )
                .unwrap();
        }
        sparse.push(
            builder
                .add(builder::State::Sparse { transitions }, &mut session)
                .unwrap()
                .unwrap(),
        );
    }
    let nfa = builder
        .finalize(roots[3], roots[2], false, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    assert_eq!(first(&nfa.0, nfa.0.anchored, b"").unwrap().0, 2);
    assert_eq!(first(&nfa.0, nfa.0.unanchored, b"").unwrap().0, 1);
    assert!(matches!(nfa.0.states[3], State::Fail));
    assert!(matches!(
        nfa.0.states[4],
        State::BinaryUnion {
            alt1: Id(1),
            alt2: Id(0)
        }
    ));
    assert!(
        matches!(&nfa.0.states[5], State::Union { alternates } if alternates == &[Id(2), Id(1), Id(0)])
    );
    assert!(matches!(nfa.0.states[6], State::Fail));
    assert!(matches!(
        nfa.0.states[7],
        State::ByteRange {
            trans: Transition { next: Id(0), .. }
        }
    ));
    assert!(
        matches!(&nfa.0.states[8], State::Sparse { transitions } if transitions.iter().map(|t| t.next).eq([Id(0), Id(1)]))
    );
    assert_eq!(nfa.0.memory_extra, 3 * 4 + 2 * 8);
}

struct Count {
    calls: usize,
    refuse: usize,
}
impl Admission for Count {
    type Error = ();
    fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
        let call = self.calls;
        self.calls += 1;
        if call == self.refuse {
            Err(())
        } else {
            Ok(())
        }
    }
}

#[test]
fn every_finalization_refusal_is_terminal_before_publication() {
    let expressions = [expression("{a,b}*"), expression("")];
    let mut account = Account;
    let mut setup = Session::new(&mut account);
    let mut compiler = Compiler::new(false, &mut setup).unwrap();
    let starts = compiler
        .many(&expressions, None, &mut setup)
        .unwrap()
        .ok()
        .unwrap();
    let mut complete = Count {
        calls: 0,
        refuse: usize::MAX,
    };
    compiler
        .finalize(
            starts.anchored,
            starts.unanchored,
            &mut Session::new(&mut complete),
        )
        .unwrap()
        .ok()
        .unwrap();
    for refuse in 0..complete.calls {
        let mut account = Count { calls: 0, refuse };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            compiler.finalize(starts.anchored, starts.unanchored, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert_eq!(account.calls, refuse + 1);
    }
}

#[test]
fn semantic_capture_failure_and_batch_size_failure_keep_original_distinction() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    assert!(matches!(
        captures::Groups::build(&[Buffer::new()], &mut session).unwrap(),
        Err(captures::Failure::MissingGroups(0))
    ));
    session.check().unwrap();
    let expressions = [expression("x")];
    let mut compiler = Compiler::new(false, &mut session).unwrap();
    assert!(matches!(
        compiler.build(&expressions, Some(0), &mut session).unwrap(),
        Err(builder::Failure::SizeLimit(0))
    ));
    session.check().unwrap();
    assert!(compiler
        .build(&expressions, None, &mut session)
        .unwrap()
        .is_ok());
    let empty = compiler
        .build(&[], None, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    assert!(empty.0.starts.is_empty());
    assert!(empty.0.groups.ranges.is_empty());
    assert!(!empty.0.has_empty);
    assert!(matches!(
        empty.0.states[empty.0.anchored.index()],
        State::Fail
    ));
}
