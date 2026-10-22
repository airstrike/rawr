use super::super::builder::State;
use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
fn expression(glob: &str) -> super::super::super::translate::Expression {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let source = super::super::super::super::parse(glob, &mut session)
        .unwrap()
        .ok()
        .unwrap()
        .regex(&mut session)
        .unwrap();
    let ast = source.tree(&mut session).unwrap().ok().unwrap();
    super::super::super::translate::tree(&source, &ast, &mut session)
        .unwrap()
        .ok()
        .unwrap()
}
fn accepts(compiler: &Compiler, start: super::super::builder::Id, bytes: &[u8]) -> bool {
    let mut pending = vec![(start, 0)];
    let mut seen = Vec::new();
    while let Some((id, pos)) = pending.pop() {
        if seen.contains(&(id, pos)) {
            continue;
        }
        seen.push((id, pos));
        match &compiler.builder.states()[id.index()] {
            State::Match { .. } if pos == bytes.len() => return true,
            State::Empty { next }
            | State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. } => pending.push((*next, pos)),
            State::Look { look, next }
                if (*look == 1 && pos == 0) || (*look == 2 && pos == bytes.len()) =>
            {
                pending.push((*next, pos))
            }
            State::Union { alternates } | State::UnionReverse { alternates } => {
                pending.extend(alternates.as_slice().iter().map(|id| (*id, pos)))
            }
            State::ByteRange { trans }
                if bytes
                    .get(pos)
                    .is_some_and(|b| trans.start <= *b && *b <= trans.end) =>
            {
                pending.push((trans.next, pos + 1))
            }
            State::Sparse { transitions } => {
                for t in transitions.as_slice() {
                    if bytes.get(pos).is_some_and(|b| t.start <= *b && *b <= t.end) {
                        pending.push((t.next, pos + 1));
                    }
                }
            }
            _ => {}
        }
    }
    false
}
#[test]
fn emitted_globs_preserve_forward_and_reverse_language() {
    for (glob, cases) in [
        (
            "src/**/{foo,bar}?.rs",
            vec![
                ("src/foo1.rs", true),
                ("src/x/bar2.rs", true),
                ("src/foo.rs", false),
                ("other/foo1.rs", false),
            ],
        ),
        (
            "{a*,b?}",
            vec![
                ("a", true),
                ("aaa", true),
                ("bc", true),
                ("b", false),
                ("ax/y", false),
            ],
        ),
        (
            "{é,中}",
            vec![("é", true), ("中", true), ("e", false), ("é中", false)],
        ),
        (
            "{foo,bar}",
            vec![
                ("foo", true),
                ("bar", true),
                ("fo", false),
                ("foobar", false),
            ],
        ),
    ] {
        let expression = expression(glob);
        for reverse in [false, true] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let mut compiler = Compiler::new(reverse, &mut session).unwrap();
            let fragment = compiler
                .compile(expression.view(), &mut session)
                .unwrap()
                .unwrap();
            // Test sealing only: production startup/captures/finalization remain separate.
            let end = compiler
                .builder
                .add(State::Match { pattern: 0 }, &mut session)
                .unwrap()
                .unwrap();
            compiler
                .builder
                .patch(fragment.end, end, &mut session)
                .unwrap()
                .unwrap();
            for (input, expected) in &cases {
                let mut bytes = input.as_bytes().to_vec();
                if reverse {
                    bytes.reverse();
                }
                assert_eq!(
                    accepts(&compiler, fragment.start, &bytes),
                    *expected,
                    "{glob} / {input} / reverse={reverse}"
                );
            }
        }
    }
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
fn connected_dispatch_refuses_terminally_during_nested_work() {
    let expression = expression("{foo,bar}/**/{a*,b?}");
    for reverse in [false, true] {
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        exercise(expression.view(), reverse, &mut Session::new(&mut count))
            .unwrap()
            .unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                exercise(expression.view(), reverse, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}

fn exercise<A: Admission>(
    view: View<'_>,
    reverse: bool,
    session: &mut Session<'_, A>,
) -> Step<Result<Fragment, Failure>> {
    Compiler::new(reverse, session)?.pattern(view, session)
}

#[test]
fn batches_preserve_anchoring_pattern_ids_and_empty_failure() {
    let expressions = [expression("a*"), expression("{foo,bar}")];
    for reverse in [false, true] {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut compiler = Compiler::new(reverse, &mut session).unwrap();
        let starts = compiler
            .many(&expressions, Some(10 * 1024 * 1024), &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let State::Empty { next } = &compiler.builder.states()[starts.unanchored.index()] else {
            panic!("anchored prefix");
        };
        assert_eq!(*next, starts.anchored);
        let patterns: Vec<_> = compiler
            .builder
            .states()
            .iter()
            .filter_map(|state| match state {
                State::Match { pattern } => Some(*pattern),
                _ => None,
            })
            .collect();
        assert_eq!(patterns, vec![0, 1]);
        for (input, expected) in [
            ("a", true),
            ("aaa", true),
            ("foo", true),
            ("bar", true),
            ("xfoo", false),
            ("z", false),
        ] {
            let mut bytes = input.as_bytes().to_vec();
            if reverse {
                bytes.reverse();
            }
            assert_eq!(accepts(&compiler, starts.unanchored, &bytes), expected);
        }
        let empty = compiler
            .many(&[], None, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        assert!(!accepts(&compiler, empty.unanchored, b""));
        assert!(matches!(
            compiler.builder.states()[empty.anchored.index()],
            State::Fail
        ));
    }
}

#[test]
fn unanchored_prefix_and_size_refusal_preserve_rebuild_behavior() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let expressions = [super::super::super::translate::Expression::any_byte(&mut session).unwrap()];
    for reverse in [false, true] {
        let mut compiler = Compiler::new(reverse, &mut session).unwrap();
        assert!(matches!(
            compiler.many(&expressions, Some(0), &mut session).unwrap(),
            Err(Failure::SizeLimit(0))
        ));
        let starts = compiler
            .many(&expressions, None, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        assert!(accepts(&compiler, starts.anchored, b"x"));
        assert!(!accepts(&compiler, starts.anchored, b"xyz"));
        assert!(accepts(&compiler, starts.unanchored, b"xyz"));
        assert!(!accepts(&compiler, starts.unanchored, b""));
    }
}

#[test]
fn batch_refusal_is_terminal_across_prefix_and_patterns() {
    let mut account = Account;
    let mut setup = Session::new(&mut account);
    let expressions = [
        expression("{foo,bar}"),
        super::super::super::translate::Expression::any_byte(&mut setup).unwrap(),
    ];
    fn run<A: Admission>(
        expressions: &[super::super::super::translate::Expression],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        assert!(Compiler::new(false, session)?
            .many(expressions, None, session)?
            .is_ok());
        Ok(())
    }
    let mut count = Count {
        calls: 0,
        refuse: usize::MAX,
    };
    run(&expressions, &mut Session::new(&mut count)).unwrap();
    for refuse in 0..count.calls {
        let mut count = Count { calls: 0, refuse };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            run(&expressions, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert_eq!(count.calls, refuse + 1);
    }
}

#[test]
fn pattern_wrapping_registers_matches_and_directional_captures() {
    let first = expression("a*");
    let second = expression("{foo,bar}");
    for reverse in [false, true] {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut compiler = Compiler::new(reverse, &mut session).unwrap();
        for (index, expression) in [first.view(), second.view()].into_iter().enumerate() {
            let fragment = compiler.pattern(expression, &mut session).unwrap().unwrap();
            let State::Match { pattern } = &compiler.builder.states()[fragment.end.index()] else {
                panic!("pattern terminates in match");
            };
            assert_eq!(*pattern, index as u32);
            assert_eq!(
                matches!(
                    compiler.builder.states()[fragment.start.index()],
                    State::CaptureStart { .. }
                ),
                !reverse
            );
        }
        let captures = compiler
            .builder
            .states()
            .iter()
            .filter(|state| matches!(state, State::CaptureStart { .. } | State::CaptureEnd { .. }))
            .count();
        assert_eq!(captures, if reverse { 0 } else { 4 });
    }
}
