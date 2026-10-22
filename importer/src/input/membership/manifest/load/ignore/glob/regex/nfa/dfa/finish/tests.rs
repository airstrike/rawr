use super::super::{
    info, subset,
    tests::{prepared, Count},
};
use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;
use std::collections::BTreeMap;

struct Snapshot {
    transitions: Vec<u32>,
    classes: [u8; 256],
    starts: Vec<u32>,
    patterns: BTreeMap<u32, Vec<u32>>,
}

impl Snapshot {
    fn before(dense: &initial::Initial, matches: &Buffer<(u32, Array<u32>)>) -> Self {
        Self {
            transitions: dense.transitions.values.clone(),
            classes: dense.classes,
            starts: dense.starts.table.values.clone(),
            patterns: matches
                .as_slice()
                .iter()
                .map(|(id, pids)| (*id, pids.values.clone()))
                .collect(),
        }
    }

    fn after(dense: &initial::Initial) -> Self {
        Self {
            transitions: dense.transitions.values.clone(),
            classes: dense.classes,
            starts: dense.starts.table.values.clone(),
            patterns: dense
                .matches
                .slices
                .values
                .as_chunks::<2>()
                .0
                .iter()
                .enumerate()
                .map(|(i, pair)| {
                    (
                        dense.special[MIN_MATCH] + (i << dense.stride2) as u32,
                        dense.matches.patterns.values
                            [pair[0] as usize..(pair[0] + pair[1]) as usize]
                            .to_vec(),
                    )
                })
                .collect(),
        }
    }

    fn walk(&self, start: usize, bytes: &[u8]) -> Vec<Vec<u32>> {
        let mut id = self.starts[start];
        let mut result = vec![self.patterns.get(&id).cloned().unwrap_or_default()];
        for class in bytes
            .iter()
            .map(|byte| usize::from(self.classes[usize::from(*byte)]))
            .chain(std::iter::once(usize::from(self.classes[255]) + 1))
        {
            id = self.transitions[id as usize + class];
            result.push(self.patterns.get(&id).cloned().unwrap_or_default());
        }
        result
    }
}

fn assert_same(before: &Snapshot, after: &Snapshot) {
    assert_eq!(before.starts.len(), after.starts.len());
    for start in 0..before.starts.len() {
        assert_eq!(before.walk(start, &[]), after.walk(start, &[]));
        for first in 0..=255u8 {
            assert_eq!(before.walk(start, &[first]), after.walk(start, &[first]));
            for second in [0, b'a', b'b', b'c', b'd', b'/', b'.', 255] {
                assert_eq!(
                    before.walk(start, &[first, second]),
                    after.walk(start, &[first, second])
                );
            }
        }
    }
}

#[test]
fn shuffle_and_finish_preserve_all_start_groups_delayed_matches_and_eoi() {
    for (globs, kind) in [
        (vec![], info::Kind::All),
        (vec![""], info::Kind::LeftmostFirst),
        (vec!["*.rs"], info::Kind::LeftmostFirst),
        (vec!["a", "a*", "*a", ""], info::Kind::All),
        (vec!["a", "a*", "*a", ""], info::Kind::LeftmostFirst),
        (vec!["**/x", "[ab]", "?.rs"], info::Kind::All),
    ] {
        let (_, _, nfa) = prepared(&globs, kind);
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut dense = initial::Initial::new(&nfa, false, &mut session)
            .unwrap()
            .unwrap();
        let subsets = subset::run(&nfa, &mut dense, kind, None, None, &mut session)
            .unwrap()
            .unwrap();
        let before = Snapshot::before(&dense, &subsets.matches);
        subsets.shuffle(&mut dense, &mut session).unwrap().unwrap();
        rest(&mut dense, &mut session).unwrap();
        assert_same(&before, &Snapshot::after(&dense));
        assert_eq!(dense.special[MIN_START], 0);
        assert_eq!(dense.special[MAX_START], 0);
        for array in [
            &dense.transitions,
            &dense.starts.table,
            &dense.matches.slices,
            &dense.matches.patterns,
        ] {
            assert_eq!(array.admitted, array.values.len());
        }
        assert_eq!(dense.matches.pattern_len, globs.len());
    }
}

fn synthetic() -> initial::Initial {
    let (_, _, nfa) = prepared(&["a", "b"], info::Kind::All);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut dense = initial::Initial::new(&nfa, false, &mut session)
        .unwrap()
        .unwrap();
    dense.classes = std::array::from_fn(|byte| byte as u8);
    dense.stride2 = 9;
    dense.transitions = Array::new();
    dense.transitions.resize(9 * 512, 0, &mut session).unwrap();
    for (index, needle, target) in [(2, b'a', 3), (5, b'b', 3), (6, b'c', 2), (7, b'd', 3)] {
        dense.transitions.values[index * 512..index * 512 + 256].fill(index as u32 * 512);
        dense.transitions.values[index * 512 + usize::from(needle)] = target * 512;
    }
    dense.starts.table.values.fill(4 * 512);
    dense.starts.table.values[6..12].fill(5 * 512);
    dense.special = [5 * 512, 512, 2 * 512, 3 * 512, 0, 0, 4 * 512, 5 * 512];
    dense.matches.slices.values = vec![0, 1, 1, 1];
    dense.matches.slices.admitted = 4;
    dense.matches.patterns.values = vec![1, 0];
    dense.matches.patterns.admitted = 2;
    dense
}

#[test]
fn acceleration_moves_match_normal_and_start_ranges_and_revisits_swapped_normal() {
    let mut dense = synthetic();
    let before = Snapshot::after(&dense);
    accelerate::run(&mut dense, &mut Session::new(&mut Account)).unwrap();
    assert_same(&before, &Snapshot::after(&dense));
    assert_eq!(&dense.special[MIN_MATCH..=MAX_MATCH], &[2 * 512, 3 * 512]);
    assert_eq!(&dense.special[MIN_START..=MAX_START], &[6 * 512, 7 * 512]);
    assert_eq!(&dense.special[MIN_ACCEL..=MAX_ACCEL], &[3 * 512, 6 * 512]);
    let words = &dense.accelerators.values;
    assert_eq!(words[0], 4);
    let needles: Vec<_> = words[1..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|words| words[0].to_ne_bytes())
        .collect();
    assert_eq!(
        needles,
        [
            [1, b'a', 0, 0],
            [1, b'd', 0, 0],
            [1, b'c', 0, 0],
            [1, b'b', 0, 0]
        ]
    );
    assert_eq!(dense.matches.patterns.values, [0, 1]);
}

#[test]
fn accelerator_rejects_space_four_needles_and_pure_self_loop_but_ignores_eoi() {
    let mut dense = synthetic();
    let id = 8 * 512;
    for (needles, expected) in [
        (vec![], None),
        (vec![b' '], None),
        (vec![0, 1, 2, 3], None),
        (vec![0, 128, 255], Some([3, 0, 128, 255, 0, 0, 0, 0])),
    ] {
        dense.transitions.values[id..id + 256].fill(id as u32);
        for needle in needles {
            dense.transitions.values[id + usize::from(needle)] = 0;
        }
        dense.transitions.values[id + 256] = 0;
        assert_eq!(
            accelerate::state(&dense, id as u32, &mut Session::new(&mut Account)).unwrap(),
            expected
        );
    }
}

#[derive(Default)]
struct Trace {
    calls: usize,
    allocations: Vec<usize>,
}
impl Admission for Trace {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        if matches!(request, Request::Allocation { .. }) {
            self.allocations.push(self.calls);
        }
        self.calls += 1;
        Ok(())
    }
}

#[test]
fn finishing_refusal_at_every_allocation_is_terminal_on_the_original_session() {
    let mut trace = Trace::default();
    rest(&mut synthetic(), &mut Session::new(&mut trace)).unwrap();
    assert!(trace.allocations.len() > 10);
    let calls = trace.calls;
    for refusal in trace
        .allocations
        .into_iter()
        .chain([0, calls / 2, calls - 1])
    {
        let mut dense = synthetic();
        let mut count = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(rest(&mut dense, &mut session), Err(Stop::Refused)));
        assert!(matches!(rest(&mut dense, &mut session), Err(Stop::Refused)));
        assert_eq!(count.calls, refusal + 1);
    }
}

#[test]
fn universal_starts_short_circuit_and_anchored_only_leaves_unanchored_absent() {
    let mut count = Count::default();
    assert_eq!(
        universal(&[4, 5, 4, 4, 4, 4], &mut Session::new(&mut count)).unwrap(),
        None
    );
    assert_eq!(count.items, 2);
    let mut dense = synthetic();
    dense.starts.anchored_only = true;
    rest(&mut dense, &mut Session::new(&mut Account)).unwrap();
    assert_eq!(dense.starts.universal_unanchored, None);
    assert_eq!(
        dense.starts.universal_anchored,
        Some(dense.starts.table.values[6])
    );
}

#[test]
fn pattern_offset_uses_small_index_max_and_compaction_keeps_accel_growth() {
    assert_eq!(
        pattern_offset(i32::MAX as usize - 1),
        Ok(i32::MAX as u32 - 1)
    );
    assert_eq!(
        pattern_offset(i32::MAX as usize),
        Err(initial::Failure::MatchPatternIds)
    );
    let mut dense = synthetic();
    rest(&mut dense, &mut Session::new(&mut Account)).unwrap();
    assert!(dense.accelerators.admitted > dense.accelerators.values.len());
    for array in [
        &dense.transitions,
        &dense.starts.table,
        &dense.matches.slices,
        &dense.matches.patterns,
    ] {
        assert_eq!(array.admitted, array.values.len());
    }
}
