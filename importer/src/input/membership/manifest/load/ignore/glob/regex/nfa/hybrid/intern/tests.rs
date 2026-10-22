use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use std::hash::{Hash, Hasher};

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    refuse: Option<usize>,
    calls: usize,
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

fn key(ids: &[u32]) -> key::State {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = key::Empty::new()
        .matches(&mut session)
        .unwrap()
        .nfa(&mut session)
        .unwrap();
    for &id in ids {
        builder.id(id, &mut session).unwrap();
    }
    builder.shared(&mut session).unwrap()
}

fn hash(value: impl Hash) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

#[test]
fn actual_hasher_borrowed_keys_preserve_zero_high_length_prefix_and_replacement() {
    let mut interner = Interner::new();
    for (id, ids) in [
        vec![],
        vec![0],
        vec![0, 0],
        vec![127],
        vec![128],
        vec![16_384],
        vec![16_384, 0],
    ]
    .into_iter()
    .enumerate()
    {
        let state = key(&ids);
        assert_eq!(hash(&state), hash(state.bytes()));
        let bytes = state.bytes().to_vec();
        let original = state.weak();
        interner
            .insert(state, id as u32, &mut Session::new(&mut Account))
            .unwrap();
        let equal = key(&ids);
        let incoming = equal.weak();
        assert_eq!(equal.bytes(), bytes);
        interner
            .insert(equal, id as u32 + 100, &mut Session::new(&mut Account))
            .unwrap();
        assert!(original.upgrade().is_some());
        assert!(incoming.upgrade().is_none());
        assert_eq!(
            interner
                .get(&bytes, &mut Session::new(&mut Account))
                .unwrap(),
            Some(id as u32 + 100)
        );
    }
    assert_eq!(interner.len(), 7);
}

#[test]
fn exhausted_capacity_replacement_reserves_before_equality_and_refuses_before_mutation() {
    for refuse in [None, Some(0), Some(1)] {
        let mut interner = Interner::new();
        for id in 0..3 {
            interner
                .insert(key(&[id]), id, &mut Session::new(&mut Account))
                .unwrap();
        }
        assert_eq!(interner.entries.capacity(), 3);
        let mut count = Count {
            refuse,
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        let incoming = key(&[1]);
        let incoming_weak = incoming.weak();
        let result = interner.insert(incoming, 999, &mut session);
        if let Some(refusal) = refuse {
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(interner.clear(&mut session), Err(Stop::Refused)));
            assert_eq!(interner.entries.capacity(), 3);
            assert_eq!(interner.entries.get(key(&[1]).bytes()), Some(&1));
            assert_eq!(count.calls, refusal + 1);
        } else {
            result.unwrap();
            assert_eq!(interner.len(), 3);
            assert_eq!(interner.entries.capacity(), 7);
            let (_, layout) = dfa::intern::growth(4).unwrap();
            assert_eq!(
                count.requests,
                [
                    Request::Items(1),
                    Request::Allocation {
                        bytes: layout.size(),
                        alignment: layout.align()
                    }
                ]
            );
            assert_eq!(interner.entries.get(key(&[1]).bytes()), Some(&999));
        }
        assert!(incoming_weak.upgrade().is_none());
    }
}

#[test]
fn retained_bucket_epochs_release_keys_without_reallocating_below_high_water() {
    let mut interner = Interner::new();
    for id in 0..29 {
        interner
            .insert(key(&[id]), id, &mut Session::new(&mut Account))
            .unwrap();
    }
    let capacity = interner.entries.capacity();
    assert_eq!(capacity, 56);
    for _ in 0..5 {
        let weak: Vec<_> = interner.entries.keys().map(key::State::weak).collect();
        let mut count = Count::default();
        interner.clear(&mut Session::new(&mut count)).unwrap();
        assert!(weak.iter().all(|weak| weak.upgrade().is_none()));
        assert_eq!(interner.entries.capacity(), capacity);
        for id in 0..capacity as u32 {
            interner
                .insert(key(&[id]), id, &mut Session::new(&mut count))
                .unwrap();
        }
        assert!(count
            .requests
            .iter()
            .all(|request| matches!(request, Request::Items(_))));
        assert_eq!(interner.len(), capacity);
    }
}

#[test]
fn actual_fixed_hasher_collision_group_survives_replacement_clear_and_reinsertion() {
    let mut groups = std::collections::BTreeMap::<u64, Vec<u32>>::new();
    let mut colliders = None;
    for id in 0..100_000 {
        let hash = hash(key(&[id]));
        // Same start bucket through 16 buckets and same high control tag.
        let shift = std::mem::size_of::<usize>().min(8) * 8 - 7;
        let signature = (hash & 15) | (((hash >> shift) & 127) << 4);
        let group = groups.entry(signature).or_default();
        group.push(id);
        if group.len() == 8 {
            colliders = Some(group.clone());
            break;
        }
    }
    let colliders =
        colliders.expect("fixed deterministic fixture search must find a collision group");
    let mut interner = Interner::new();
    for epoch in 0..3 {
        for (index, &id) in colliders.iter().enumerate() {
            interner
                .insert(key(&[id]), index as u32, &mut Session::new(&mut Account))
                .unwrap();
        }
        for (index, &id) in colliders.iter().enumerate() {
            assert_eq!(
                interner
                    .get(key(&[id]).bytes(), &mut Session::new(&mut Account))
                    .unwrap(),
                Some(index as u32)
            );
            interner
                .insert(key(&[id]), epoch + 100, &mut Session::new(&mut Account))
                .unwrap();
        }
        assert_eq!(interner.len(), colliders.len());
        interner.clear(&mut Session::new(&mut Account)).unwrap();
    }
}
