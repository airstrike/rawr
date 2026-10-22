use super::super::{Attempt, Hybrid};
use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;
use std::alloc::Layout;

#[derive(Default)]
struct Count {
    calls: usize,
    refuse: Option<usize>,
    requests: Vec<Request>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let call = self.calls;
        self.calls += 1;
        if self.refuse == Some(call) {
            return Err(call);
        }
        self.requests.push(request);
        Ok(())
    }
}

pub(in super::super) fn prepared(
    globs: &[&str],
    kind: info::Kind,
) -> (info::Info, finalize::Nfa, finalize::Nfa) {
    use crate::input::membership::manifest::load::ignore::glob;
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let expressions: Vec<_> = globs
        .iter()
        .map(|glob| {
            let regex = glob::parse(glob, &mut session)
                .unwrap()
                .ok()
                .unwrap()
                .regex(&mut session)
                .unwrap();
            let ast = regex.tree(&mut session).unwrap().ok().unwrap();
            glob::regex::translate::tree(&regex, &ast, &mut session)
                .unwrap()
                .ok()
                .unwrap()
        })
        .collect();
    info::Info::with(&expressions, kind, &mut session, |info, session| {
        let forward = Compiler::new(false, session)?
            .build(&expressions, info.nfa_size_limit(), session)?
            .unwrap();
        let reverse = Compiler::new(true, session)?
            .build(&expressions, info.nfa_size_limit(), session)?
            .unwrap();
        Ok((info, forward, reverse))
    })
    .unwrap()
}

fn allocations(requests: &[Request]) -> Vec<(usize, usize)> {
    requests
        .iter()
        .filter_map(|request| match *request {
            Request::Allocation { bytes, alignment } => Some((bytes, alignment)),
            _ => None,
        })
        .collect()
}

fn expected_floor(nfa: &finalize::Nfa) -> usize {
    let states = nfa.0.states.len();
    let patterns = nfa.0.starts.len();
    let state_size = std::mem::size_of::<Arc<[u8]>>();
    let stride = (usize::from(nfa.0.classes[255]) + 2).next_power_of_two();
    let maximum = 9 + patterns * 4 + states * 5;
    5 * stride * 4
        + (6 + 6 * patterns) * 4
        + 3 * (state_size + 9)
        + 2 * (state_size + maximum)
        + 5 * (state_size + 4)
        + 2 * states * 4
        + states * 4
        + maximum
}

#[test]
fn constructor_floor_keeps_pinned_projection_and_reached_dead_state_ownership() {
    for globs in [vec![], vec![""], vec!["a"], vec!["*.rs", "**/x", "a"]] {
        let (_, nfa, _) = prepared(&globs, info::Kind::All);
        let mut count = Count::default();
        let minimum =
            minimum_capacity(&nfa, &nfa.0.classes, &mut Session::new(&mut count)).unwrap();
        assert_eq!(minimum, expected_floor(&nfa));
        let byte_vec = Layout::array::<u8>(9).unwrap();
        let arc = Layout::new::<[usize; 2]>()
            .extend(byte_vec)
            .unwrap()
            .0
            .pad_to_align();
        assert_eq!(
            allocations(&count.requests),
            [
                (byte_vec.size(), byte_vec.align()),
                (arc.size(), arc.align())
            ]
        );
        assert!(!count
            .requests
            .iter()
            .any(|request| matches!(request, Request::String(_))));
    }
}

#[test]
fn exact_capacity_floor_declines_only_below_floor_and_keeps_builder_before_clone() {
    let (_, nfa, _) = prepared(&["a"], info::Kind::LeftmostFirst);
    let minimum = expected_floor(&nfa);
    let mut compiler_count = Count::default();
    drop(Compiler::new(false, &mut Session::new(&mut compiler_count)).unwrap());
    for capacity in [minimum - 1, minimum, minimum + 1] {
        let mut count = Count::default();
        let result = Machine::build(
            &nfa,
            info::Kind::LeftmostFirst,
            capacity,
            &mut Session::new(&mut count),
        )
        .unwrap();
        assert_eq!(
            &allocations(&count.requests)[..allocations(&compiler_count.requests).len()],
            allocations(&compiler_count.requests).as_slice()
        );
        if capacity < minimum {
            assert!(
                matches!(result, Err(Failure::CacheCapacity { minimum: actual, given }) if actual == minimum && given == capacity)
            );
            assert_eq!(Arc::strong_count(&nfa.0), 1);
        } else {
            let machine = result.unwrap();
            assert_eq!(machine.cache_capacity, capacity);
            assert_eq!(machine.classes, nfa.0.classes);
            assert_eq!(
                machine.stride2,
                (usize::from(nfa.0.classes[255]) + 2)
                    .next_power_of_two()
                    .trailing_zeros() as usize
            );
            assert_eq!(machine.quitset, [0, 0]);
            assert_eq!(Arc::strong_count(&nfa.0), 2);
            drop(machine);
            assert_eq!(Arc::strong_count(&nfa.0), 1);
        }
    }
}

#[test]
fn pair_keeps_ten_mib_per_direction_and_all_reverse_without_anchored_only_change() {
    for kind in [info::Kind::LeftmostFirst, info::Kind::All] {
        let (info, nfa, reverse) = prepared(&["*.rs", "a"], kind);
        let mut count = Count::default();
        let hybrid = Hybrid::new(&info, &nfa, &reverse, &mut Session::new(&mut count)).unwrap();
        let Attempt::Built(pair) = &hybrid.0 else {
            panic!("engine must build");
        };
        assert_eq!(pair.forward.config.match_kind, Some(kind));
        assert_eq!(pair.reverse.config.match_kind, Some(info::Kind::All));
        for machine in [&pair.forward, &pair.reverse] {
            assert_eq!(machine.cache_capacity, 10 * (1 << 20));
            assert_eq!(machine.config.starts_for_each_pattern, Some(true));
            assert_eq!(machine.config.minimum_cache_clear_count, Some(Some(3)));
            assert_eq!(machine.config.minimum_bytes_per_state, Some(Some(10)));
            assert_eq!(machine.config.skip_cache_capacity_check, Some(false));
            assert_eq!(machine.config.specialize_start_states, Some(false));
            assert!(matches!(machine.config.pre, Some(None)));
            assert_eq!(
                machine.start_map,
                dfa::Start::byte_map(
                    machine.nfa.0.line_terminator,
                    &mut Session::new(&mut Account)
                )
                .unwrap()
            );
        }
        assert_eq!(Arc::strong_count(&nfa.0), 2);
        assert_eq!(Arc::strong_count(&reverse.0), 2);
        let mut compiler_count = Count::default();
        drop(Compiler::new(false, &mut Session::new(&mut compiler_count)).unwrap());
        let actual = allocations(&count.requests);
        let last = allocations(&compiler_count.requests);
        assert_eq!(&actual[actual.len() - last.len()..], last.as_slice());
        drop(hybrid);
        assert_eq!(Arc::strong_count(&nfa.0), 1);
        assert_eq!(Arc::strong_count(&reverse.0), 1);
    }
}

#[test]
fn forward_semantic_failure_skips_reverse_and_final_builder_without_poisoning_session() {
    let (info, nfa, reverse) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut single = Count::default();
    let single_result =
        Machine::build(&nfa, info.kind(), 0, &mut Session::new(&mut single)).unwrap();
    assert!(single_result.is_err());
    let mut pair = Count::default();
    let mut session = Session::new(&mut pair);
    let hybrid = Hybrid::with_capacity(&info, &nfa, &reverse, 0, &mut session).unwrap();
    assert!(matches!(
        hybrid.0,
        Attempt::Rejected(Failure::CacheCapacity { given: 0, .. })
    ));
    assert!(session.check().is_ok());
    assert_eq!(pair.calls, single.calls + 1);
    assert_eq!(allocations(&pair.requests), allocations(&single.requests));
    assert_eq!(Arc::strong_count(&nfa.0), 1);
    assert_eq!(Arc::strong_count(&reverse.0), 1);
}

#[test]
fn every_constructor_refusal_is_terminal_and_releases_nfa_clones() {
    let (info, nfa, reverse) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut count = Count::default();
    Hybrid::new(&info, &nfa, &reverse, &mut Session::new(&mut count)).unwrap();
    let calls = count.calls;
    for refusal in 0..calls {
        let mut count = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            Hybrid::new(&info, &nfa, &reverse, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            Hybrid::new(&info, &nfa, &reverse, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(count.calls, refusal + 1);
        assert_eq!(Arc::strong_count(&nfa.0), 1);
        assert_eq!(Arc::strong_count(&reverse.0), 1);
    }
}

#[test]
fn resource_refusal_in_capacity_temporary_precedes_semantic_decline() {
    let (info, nfa, reverse) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut count = Count::default();
    Hybrid::with_capacity(&info, &nfa, &reverse, 0, &mut Session::new(&mut count)).unwrap();
    let allocations: Vec<_> = count
        .requests
        .iter()
        .enumerate()
        .filter_map(|(at, request)| matches!(request, Request::Allocation { .. }).then_some(at))
        .collect();
    assert!(allocations.len() >= 3);
    for refusal in allocations.into_iter().rev().take(2) {
        let mut count = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        assert!(matches!(
            Hybrid::with_capacity(&info, &nfa, &reverse, 0, &mut Session::new(&mut count)),
            Err(Stop::Refused)
        ));
        assert_eq!(count.calls, refusal + 1);
    }
}

#[test]
fn reverse_semantic_decline_follows_complete_forward_and_drops_both_nfa_clones() {
    let (info, nfa, mut reverse) = prepared(&["a"], info::Kind::LeftmostFirst);
    let graph = Arc::get_mut(&mut reverse.0).unwrap();
    graph.states.extend((0..32).map(|_| finalize::State::Fail));
    let capacity = expected_floor(&nfa);
    assert!(expected_floor(&reverse) > capacity);
    let mut forward_count = Count::default();
    let forward = Machine::build(
        &nfa,
        info.kind(),
        capacity,
        &mut Session::new(&mut forward_count),
    )
    .unwrap()
    .unwrap();
    drop(forward);
    let mut reverse_count = Count::default();
    assert!(Machine::build(
        &reverse,
        info::Kind::All,
        capacity,
        &mut Session::new(&mut reverse_count)
    )
    .unwrap()
    .is_err());
    let mut count = Count::default();
    let hybrid = Hybrid::with_capacity(
        &info,
        &nfa,
        &reverse,
        capacity,
        &mut Session::new(&mut count),
    )
    .unwrap();
    assert!(
        matches!(hybrid.0, Attempt::Rejected(Failure::CacheCapacity { minimum, given }) if minimum == expected_floor(&reverse) && given == capacity)
    );
    let mut expected = forward_count.requests;
    expected.extend(reverse_count.requests);
    assert_eq!(&count.requests[1..], expected.as_slice());
    assert_eq!(Arc::strong_count(&nfa.0), 1);
    assert_eq!(Arc::strong_count(&reverse.0), 1);
}

#[test]
fn constructor_retains_the_original_cumulative_item_and_allocation_floor() {
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
        info: &info::Info,
        nfa: &finalize::Nfa,
        reverse: &finalize::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let prior = finalize::storage::repeat(7_usize, 17, session)?;
        session.items(31)?;
        let result = Hybrid::new(info, nfa, reverse, session)?;
        assert!(matches!(result.0, Attempt::Built(_)));
        drop(prior);
        Ok(())
    }
    let (info, nfa, reverse) = prepared(&["*.rs"], info::Kind::LeftmostFirst);
    let mut count = Count::default();
    operation(&info, &nfa, &reverse, &mut Session::new(&mut count)).unwrap();
    let items = count
        .requests
        .iter()
        .map(|request| match request {
            Request::Items(amount) => *amount,
            _ => 0,
        })
        .sum();
    let bytes = count
        .requests
        .iter()
        .map(|request| match request {
            Request::Allocation { bytes, .. } => *bytes,
            _ => 0,
        })
        .sum();
    for (item_limit, byte_limit, success) in [
        (items, bytes, true),
        (items - 1, bytes, false),
        (items, bytes - 1, false),
    ] {
        let mut budget = Budget {
            items: 0,
            bytes: 0,
            item_limit,
            byte_limit,
        };
        let mut session = Session::new(&mut budget);
        assert_eq!(
            operation(&info, &nfa, &reverse, &mut session).is_ok(),
            success
        );
        if !success {
            assert!(matches!(
                operation(&info, &nfa, &reverse, &mut session),
                Err(Stop::Refused)
            ));
        }
        if success {
            assert_eq!((budget.items, budget.bytes), (items, bytes));
        }
    }
}
