use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;
use std::{alloc::Layout, sync::Arc};

#[derive(Default)]
pub(super) struct Count {
    pub(super) calls: usize,
    pub(super) refuse: Option<usize>,
    pub(super) allocations: Vec<(usize, usize)>,
    pub(super) items: usize,
}

impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let call = self.calls;
        self.calls += 1;
        if self.refuse == Some(call) {
            return Err(call);
        }
        match request {
            Request::Allocation { bytes, alignment } => self.allocations.push((bytes, alignment)),
            Request::Items(items) => self.items += items,
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn prepared(
    globs: &[&str],
    kind: info::Kind,
) -> (Vec<Expression>, info::Info, finalize::Nfa) {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let expressions: Vec<_> = globs
        .iter()
        .map(|glob| {
            let regex = super::super::super::super::parse(glob, &mut session)
                .unwrap()
                .ok()
                .unwrap()
                .regex(&mut session)
                .unwrap();
            let ast = regex.tree(&mut session).unwrap().ok().unwrap();
            super::super::super::translate::tree(&regex, &ast, &mut session)
                .unwrap()
                .ok()
                .unwrap()
        })
        .collect();
    let (info, nfa) = info::Info::with(&expressions, kind, &mut session, |info, session| {
        let mut compiler = Compiler::new(false, session)?;
        let nfa = compiler
            .build(&expressions, info.nfa_size_limit(), session)?
            .ok()
            .unwrap();
        Ok((info, nfa))
    })
    .unwrap();
    (expressions, info, nfa)
}

fn with_states(states: usize) -> (Vec<Expression>, info::Info, finalize::Nfa) {
    let (_, _, one) = prepared(&["a"], info::Kind::LeftmostFirst);
    let pattern = "a".repeat(states - one.0.states.len() + 1);
    let prepared = prepared(&[&pattern], info::Kind::LeftmostFirst);
    assert_eq!(prepared.2 .0.states.len(), states);
    prepared
}

fn layout<T>(length: usize) -> (usize, usize) {
    let layout = Layout::array::<T>(length).unwrap();
    (layout.size(), layout.align())
}

fn terminal_patterns<'a>(dense: &'a initial::Initial, bytes: &[u8]) -> &'a [u32] {
    let mut id = dense.starts.table.values[8]; // Anchored + Text.
    for class in bytes
        .iter()
        .map(|byte| usize::from(dense.classes[usize::from(*byte)]))
        .chain(std::iter::once(usize::from(dense.classes[255]) + 1))
    {
        id = dense.transitions.values[id as usize + class];
    }
    if id == 0 || id < dense.special[2] || id > dense.special[3] {
        return &[];
    }
    let index = ((id - dense.special[2]) as usize >> dense.stride2) * 2;
    let pair = &dense.matches.slices.values[index..index + 2];
    &dense.matches.patterns.values[pair[0] as usize..(pair[0] + pair[1]) as usize]
}

#[test]
fn finished_pair_uses_all_for_reverse_and_accounts_final_eager_regex_builder() {
    let (expressions, info, nfa) = prepared(&["a", "a"], info::Kind::LeftmostFirst);
    let stage = Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap()
        .determinize(&mut Session::new(&mut Account))
        .unwrap();
    let mut count = Count::default();
    let stage = stage.finish(&mut Session::new(&mut count)).unwrap();
    let Forward::Built(pair) = &stage.forward else {
        panic!("both dense constructors must finish");
    };
    assert_eq!(terminal_patterns(&pair.forward, b"a"), &[0]);
    assert_eq!(terminal_patterns(&pair.reverse, b"a"), &[0, 1]);
    assert!(!pair.forward.starts.anchored_only);
    assert!(pair.reverse.starts.anchored_only);
    assert!(pair.reverse.starts.table.values[..6]
        .iter()
        .all(|&id| id == 0));
    assert_eq!(pair.reverse.starts.universal_unanchored, None);
    assert!(stage.reverse.0.reverse);
    assert_eq!(Arc::strong_count(&nfa.0), 1);
    let mut compiler_count = Count::default();
    drop(Compiler::new(false, &mut Session::new(&mut compiler_count)).unwrap());
    assert!(!compiler_count.allocations.is_empty());
    assert_eq!(
        &count.allocations[count.allocations.len() - compiler_count.allocations.len()..],
        compiler_count.allocations.as_slice()
    );
}

#[test]
fn reverse_semantic_limit_failure_follows_forward_finish_and_preserves_reverse_nfa() {
    let (expressions, info, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut stage = Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap()
        .determinize(&mut Session::new(&mut Account))
        .unwrap();
    let Forward::Unshuffled(forward) = &mut stage.forward else {
        panic!("subset graph expected");
    };
    // This fault injection is after forward determinization's size checks.
    // Forward finishing has no new check; the reverse builder must reach one.
    forward.pending.config.size_limit = Some(0);
    let reverse = Arc::downgrade(&stage.reverse.0);
    let mut count = Count::default();
    let stage = stage.finish(&mut Session::new(&mut count)).unwrap();
    assert!(matches!(
        stage.forward,
        Forward::Rejected(initial::Failure::DfaSize(0))
    ));
    assert!(reverse.upgrade().is_some());
    assert!(!count.allocations.is_empty());
    assert!(Session::new(&mut count).check().is_ok());
    drop(stage);
    assert!(reverse.upgrade().is_none());
}

#[test]
fn finished_pair_refusal_never_becomes_semantic_fallback_or_a_new_account() {
    let (expressions, info, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    fn operation<A: Admission>(
        expressions: &[Expression],
        info: &info::Info,
        nfa: &finalize::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(23)?;
        let stage = Prepared::begin(info, nfa, expressions, session)?
            .unwrap()
            .determinize(session)?
            .finish(session)?;
        assert!(matches!(stage.forward, Forward::Built(_)));
        Ok(())
    }
    let mut count = Count::default();
    operation(&expressions, &info, &nfa, &mut Session::new(&mut count)).unwrap();
    let calls = count.calls;
    for refusal in [0, calls / 4, calls / 2, calls * 3 / 4, calls - 1] {
        let mut count = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            operation(&expressions, &info, &nfa, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            operation(&expressions, &info, &nfa, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(count.calls, refusal + 1);
    }
}

#[test]
fn skipped_and_rejected_constructors_do_not_enter_reverse_dense_finish() {
    let (expressions, info, nfa) = with_states(31);
    let stage = Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    let mut count = Count::default();
    let mut stage = stage
        .determinize(&mut Session::new(&mut count))
        .unwrap()
        .finish(&mut Session::new(&mut count))
        .unwrap();
    assert!(matches!(stage.forward, Forward::Skipped));
    assert_eq!(count.calls, 0);
    stage.forward = Forward::Rejected(initial::Failure::States);
    let stage = stage.finish(&mut Session::new(&mut count)).unwrap();
    assert!(matches!(
        stage.forward,
        Forward::Rejected(initial::Failure::States)
    ));
    assert_eq!(count.calls, 0);
}

#[test]
fn reverse_is_compiled_without_captures_before_forward_gate_and_pending_owns_builder() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        let (expressions, info, nfa) = prepared(&["*.rs"], kind);
        let stage = Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut Account))
            .unwrap()
            .ok()
            .unwrap();
        assert!(stage.reverse.0.reverse);
        assert!(!stage.reverse.0.utf8);
        assert!(!stage.reverse.0.has_capture);
        assert_eq!(stage.reverse.0.groups.slot_len(), 0);
        assert_eq!(stage.reverse.0.starts.len(), 1);
        assert!(stage
            .reverse
            .0
            .states
            .iter()
            .all(|state| !matches!(state, finalize::State::Capture { .. })));
        let Forward::Pending(pending) = &stage.forward else {
            panic!("forward must remain pending")
        };
        assert!(std::ptr::eq(pending.nfa, &nfa));
        assert_eq!(Arc::strong_count(&nfa.0), 1);
        assert_eq!(pending.config.kind, kind);
        assert!(!pending.config.anchored_only);
        assert_eq!(pending.config.size_limit, Some(10 * (1 << 10)));
        assert_eq!(info.dfa_state_limit(), Some(30));
        assert_eq!(info.dfa_size_limit(), Some(40 * (1 << 10)));
        let weak_reverse = Arc::downgrade(&stage.reverse.0);
        drop(stage);
        assert!(weak_reverse.upgrade().is_none());
    }
}

#[test]
fn thirty_states_enters_dense_builder_and_thirty_one_skips_only_after_reverse_build() {
    for states in [30, 31] {
        let (expressions, info, nfa) = with_states(states);
        let mut reverse_count = Count::default();
        let mut reverse_session = Session::new(&mut reverse_count);
        let mut compiler = Compiler::new(true, &mut reverse_session).unwrap();
        let reverse = compiler
            .build(&expressions, info.nfa_size_limit(), &mut reverse_session)
            .unwrap()
            .ok()
            .unwrap();
        let mut count = Count::default();
        let stage = Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut count))
            .unwrap()
            .ok()
            .unwrap();
        assert_eq!(stage.reverse.0.states.len(), reverse.0.states.len());
        assert_eq!(
            &count.allocations[..reverse_count.allocations.len()],
            reverse_count.allocations.as_slice()
        );
        if states == 30 {
            assert!(matches!(stage.forward, Forward::Pending(_)));
            assert!(count.allocations.len() > reverse_count.allocations.len());
        } else {
            assert!(matches!(stage.forward, Forward::Skipped));
            assert_eq!(count.allocations, reverse_count.allocations);
            assert_eq!(count.calls, reverse_count.calls + 1);
        }
    }
}

#[test]
fn dense_prologue_allocates_compiler_then_dead_quit_rows_start_table_and_accel_header() {
    for patterns in [vec![], vec!["a"], vec!["a", "b", ""]] {
        let (_, info, nfa) = prepared(&patterns, info::Kind::All);
        let mut compiler_count = Count::default();
        drop(Compiler::new(false, &mut Session::new(&mut compiler_count)).unwrap());
        let mut count = Count::default();
        let pending = Pending::begin(
            &nfa,
            Config {
                kind: info.kind(),
                anchored_only: false,
                size_limit: Some(10 * (1 << 10)),
            },
            &mut Session::new(&mut count),
        )
        .unwrap()
        .ok()
        .unwrap();
        assert_eq!(
            &count.allocations[..compiler_count.allocations.len()],
            compiler_count.allocations.as_slice()
        );
        let initial = &pending.dense;
        let stride = initial.stride();
        let mut allocations = vec![layout::<u32>(stride.max(4))];
        if stride > 2 {
            allocations.push(layout::<u32>(2 * stride));
        }
        allocations.push(layout::<u32>(6 * (2 + patterns.len())));
        allocations.push(layout::<u32>(1));
        assert_eq!(
            &count.allocations[compiler_count.allocations.len()..],
            allocations.as_slice()
        );
        assert_eq!(initial.transitions.values.len(), 2 * stride);
        assert!(initial.transitions.values.iter().all(|&id| id == 0));
        assert_eq!(initial.starts.table.values.len(), 6 * (2 + patterns.len()));
        assert_eq!(
            initial.starts.table.admitted,
            initial.starts.table.values.len()
        );
        assert!(initial.starts.table.values.iter().all(|&id| id == 0));
        assert_eq!(initial.starts.stride, 6);
        assert_eq!(initial.starts.pattern_len, Some(patterns.len()));
        assert_eq!(initial.starts.universal_unanchored, None);
        assert_eq!(initial.starts.universal_anchored, None);
        assert_eq!(initial.special, [0; 8]);
        assert_eq!(initial.accelerators.values, [0]);
        assert_eq!(initial.accelerators.admitted, 1);
        assert!(initial.matches.slices.values.is_empty());
        assert!(initial.matches.patterns.values.is_empty());
        assert_eq!(initial.matches.pattern_len, patterns.len());
        assert_eq!(
            initial.memory_usage(),
            (2 * stride + 6 * (2 + patterns.len()) + 1) * 4
        );
    }
}

#[test]
fn source_start_map_and_byte_classes_are_rebuilt_with_end_of_input_stride() {
    let (_, _, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let initial = initial::Initial::new(&nfa, false, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!(initial.classes, nfa.0.classes);
    assert_eq!(
        initial.classes[..usize::from(b'a')],
        vec![0; usize::from(b'a')]
    );
    assert_eq!(initial.classes[usize::from(b'a')], 1);
    assert!(initial.classes[usize::from(b'a') + 1..]
        .iter()
        .all(|&class| class == 2));
    assert_eq!(initial.stride(), 4);
    assert_eq!(initial.stride2, 2);
    for byte in u8::MIN..=u8::MAX {
        let expected = match byte {
            b'\n' => initial::Start::LineLF,
            b'\r' => initial::Start::LineCR,
            b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => initial::Start::WordByte,
            _ => initial::Start::NonWordByte,
        };
        assert_eq!(initial.starts.byte_map[usize::from(byte)], expected);
    }
    assert!(!initial.flags.has_empty);
    assert!(!initial.flags.utf8);
    assert!(initial.flags.anchored);
    let (_, _, nfa) = prepared(&["*"], info::Kind::LeftmostFirst);
    assert!(
        initial::Initial::new(&nfa, false, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap()
            .flags
            .has_empty
    );
}

#[test]
fn anchored_reverse_dense_prologue_keeps_both_start_groups_and_no_capture_slots() {
    let (expressions, _, _) = prepared(&["a", "b"], info::Kind::All);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let nfa = Compiler::new(true, &mut session)
        .unwrap()
        .build(&expressions, None, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    let pending = Pending::begin(
        &nfa,
        Config {
            kind: info::Kind::All,
            anchored_only: true,
            size_limit: Some(10 * (1 << 10)),
        },
        &mut session,
    )
    .unwrap()
    .ok()
    .unwrap();
    assert!(pending.dense.starts.anchored_only);
    assert_eq!(pending.dense.starts.table.values.len(), 24);
    assert_eq!(pending.dense.starts.pattern_len, Some(2));
    assert_eq!(pending.config.kind, info::Kind::All);
    assert_eq!(pending.nfa.0.groups.slot_len(), 0);
}

#[test]
fn reverse_semantic_failure_precedes_dense_work_and_does_not_latch_the_session() {
    let (expressions, info, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut expected = Count::default();
    let mut session = Session::new(&mut expected);
    let mut compiler = Compiler::new(true, &mut session).unwrap();
    assert!(matches!(
        compiler.build(&expressions, Some(0), &mut session).unwrap(),
        Err(builder::Failure::SizeLimit(0))
    ));
    let mut count = Count::default();
    let mut session = Session::new(&mut count);
    assert!(matches!(
        Prepared::begin_with_nfa_limit(&info, &nfa, &expressions, Some(0), &mut session).unwrap(),
        Err(builder::Failure::SizeLimit(0))
    ));
    assert!(session.check().is_ok());
    assert_eq!(count.allocations, expected.allocations);
    assert_eq!(count.calls, expected.calls);
}

#[test]
fn every_preparation_refusal_is_terminal_and_preserves_original_error_without_skip() {
    let (expressions, info, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut baseline = Count::default();
    drop(
        Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut baseline))
            .unwrap()
            .ok()
            .unwrap(),
    );
    for refuse in 0..baseline.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let result = Prepared::begin(&info, &nfa, &expressions, &mut session);
        assert!(matches!(result, Err(Stop::Refused)));
        assert!(matches!(
            Prepared::begin(&info, &nfa, &expressions, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(result), Err(Error::Admission(cause)) if cause == refuse));
        assert_eq!(account.calls, refuse + 1);
        assert_eq!(Arc::strong_count(&nfa.0), 1);
    }
}

#[test]
fn every_skipped_branch_refusal_still_stops_before_a_successful_skip() {
    let (expressions, info, nfa) = with_states(31);
    let mut baseline = Count::default();
    drop(
        Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut baseline))
            .unwrap()
            .ok()
            .unwrap(),
    );
    // Target both reverse startup and the exact final gate. A refusal at the
    // gate must not return an ordinary source-directed skip.
    for refuse in [0, baseline.calls - 1] {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            Prepared::begin(&info, &nfa, &expressions, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert_eq!(account.calls, refuse + 1);
    }
}

#[test]
fn exact_original_account_floor_includes_prior_reverse_and_initial_dense_ownership() {
    struct Budget {
        items: usize,
        bytes: usize,
        item_limit: usize,
        byte_limit: usize,
    }
    impl Admission for Budget {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            match request {
                Request::Items(amount) => {
                    if self.items + amount > self.item_limit {
                        return Err(());
                    }
                    self.items += amount;
                }
                Request::Allocation { bytes, .. } => {
                    if self.bytes + bytes > self.byte_limit {
                        return Err(());
                    }
                    self.bytes += bytes;
                }
                _ => {}
            }
            Ok(())
        }
    }
    fn operation<A: Admission>(
        expressions: &[Expression],
        info: &info::Info,
        nfa: &finalize::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut prior = super::super::storage::Array::new();
        prior.resize(17, 3_usize, session)?;
        session.items(37)?;
        let stage = Prepared::begin(info, nfa, expressions, session)?
            .ok()
            .unwrap();
        assert!(matches!(stage.forward, Forward::Pending(_)));
        Ok(())
    }
    let (expressions, info, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut count = Count::default();
    operation(&expressions, &info, &nfa, &mut Session::new(&mut count)).unwrap();
    let items = count.items;
    let bytes = count
        .allocations
        .iter()
        .map(|&(bytes, _)| bytes)
        .sum::<usize>();
    for (item_limit, byte_limit, success) in [
        (items, bytes, true),
        (items - 1, bytes, false),
        (items, bytes - 1, false),
    ] {
        let mut account = Budget {
            items: 0,
            bytes: 0,
            item_limit,
            byte_limit,
        };
        let mut session = Session::new(&mut account);
        assert_eq!(
            operation(&expressions, &info, &nfa, &mut session).is_ok(),
            success
        );
        if success {
            assert_eq!((account.items, account.bytes), (items, bytes));
        } else {
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert!(matches!(
                operation(&expressions, &info, &nfa, &mut session),
                Err(Stop::Refused)
            ));
        }
    }
}
