use super::super::tests::Count;
use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use std::hash::{BuildHasher, Hasher};

fn encoded(ids: &[u32], patterns: &[u32]) -> key::State {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut matches = key::Empty::new().matches(&mut session).unwrap();
    for &pattern in patterns {
        matches.pattern(pattern, &mut session).unwrap();
    }
    let mut builder = matches.nfa(&mut session).unwrap();
    for &id in ids {
        builder.id(id, &mut session).unwrap();
    }
    builder.shared(&mut session).unwrap()
}

#[test]
fn actual_fixed_hasher_agrees_for_owned_and_borrowed_binary_keys() {
    let mut interner = Interner::new();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut keys = vec![
        encoded(&[], &[]),
        encoded(&[0], &[]),
        encoded(&[127, 128], &[0]),
        encoded(&[i32::MAX as u32, 0], &[0, i32::MAX as u32]),
    ];
    for length in [7, 8, 9, 15, 16, 17, 31, 32, 33, 127, 128, 255, 256, 257] {
        keys.push(encoded(&(0..length).collect::<Vec<_>>(), &[]));
    }
    assert!(keys[0].bytes().iter().all(|&byte| byte == 0));
    assert!(keys[3].bytes().contains(&0xFF));
    assert!(keys[1].bytes().starts_with(keys[0].bytes()));
    for (id, key) in keys.iter().enumerate() {
        let owned = interner.entries.hasher().hash_one(key);
        let borrowed = interner.entries.hasher().hash_one(key.bytes());
        // The pinned [u8] Hash stream is its usize length followed by bytes;
        // DefaultHasher::new initializes SipHasher13 with fixed zero keys.
        let mut expected = DefaultHasher::new();
        expected.write_usize(key.bytes().len());
        expected.write(key.bytes());
        assert_eq!(owned, expected.finish());
        assert_eq!(owned, borrowed);
        assert_eq!(interner.get(key.bytes(), &mut session).unwrap(), None);
        interner
            .insert(key.share(&mut session).unwrap(), id as u32, &mut session)
            .unwrap();
    }
    for (id, key) in keys.iter().enumerate() {
        assert_eq!(
            interner.get(key.bytes(), &mut session).unwrap(),
            Some(id as u32)
        );
    }
    // Invalid/absent byte representations are only borrowed lookups in this
    // test; the production key constructor remains private and encoded.
    for missing in [&[][..], &[0][..], &[0xFF; 9][..]] {
        assert_eq!(interner.get(missing, &mut session).unwrap(), None);
    }
}

#[test]
fn pinned_bucket_growth_and_borrowed_key_lookup_retain_shared_states() {
    let mut interner = Interner::new();
    let mut count = Count::default();
    let mut expected = Vec::new();
    for id in 0..200 {
        let key = encoded(&[id], &[]);
        let prior_capacity = interner.entries.capacity();
        let mut session = Session::new(&mut count);
        assert_eq!(interner.get(key.bytes(), &mut session).unwrap(), None);
        interner.insert(key, id, &mut session).unwrap();
        let (buckets, allocation) = growth(interner.len()).unwrap();
        assert_eq!(interner.entries.capacity(), capacity(buckets));
        if prior_capacity != interner.entries.capacity() {
            expected.push((allocation.size(), allocation.align()));
        }
        let repeated = encoded(&[id], &[]);
        assert_eq!(
            interner.get(repeated.bytes(), &mut session).unwrap(),
            Some(id)
        );
    }
    assert_eq!(interner.entries.capacity(), 224);
    assert_eq!(expected.len(), 7); // capacities 3, 7, 14, 28, 56, 112, 224
    assert_eq!(count.allocations, expected);
    assert_eq!(count.items, 600); // only get/insert/get, no opaque byte debit
    let capacity = interner.entries.capacity();
    interner.clear(&mut Session::new(&mut count)).unwrap();
    assert_eq!(interner.len(), 0);
    assert_eq!(interner.entries.capacity(), capacity);
    assert_eq!(count.items, 800); // actual 200 entry releases
    assert_eq!(count.allocations, expected);
}

#[test]
fn actual_hasher_bucket_and_tag_collisions_keep_distinct_keys_and_growth() {
    // Same initial bucket and same control tag in every table up to 32
    // buckets. The real fixed hasher is used throughout, never a constant or
    // injected production hasher. Pigeonhole guarantees 18 candidates within
    // this finite fixture domain of 17 * 4096 + 1 distinct encoded states.
    let interner = Interner::new();
    let mut groups = vec![Vec::<u32>::new(); 32 * 128];
    let mut colliding = Vec::new();
    let shift = std::mem::size_of::<usize>().min(8) * 8 - 7;
    for id in 0..=(17 * 32 * 128) {
        let state = encoded(&[id], &[]);
        let hash = interner.entries.hasher().hash_one(&state);
        let signature = (hash as usize & 31) * 128 + ((hash >> shift) as usize & 127);
        groups[signature].push(id);
        if groups[signature].len() == 18 {
            colliding = std::mem::take(&mut groups[signature]);
            break;
        }
    }
    assert_eq!(colliding.len(), 18);
    let mut interner = Interner::new();
    let mut count = Count::default();
    for (value, &id) in colliding[..17].iter().enumerate() {
        let state = encoded(&[id], &[]);
        interner
            .insert(state, value as u32, &mut Session::new(&mut count))
            .unwrap();
        for (expected, &id) in colliding[..=value].iter().enumerate() {
            let state = encoded(&[id], &[]);
            assert_eq!(
                interner
                    .get(state.bytes(), &mut Session::new(&mut count))
                    .unwrap(),
                Some(expected as u32)
            );
        }
    }
    assert_eq!(interner.entries.capacity(), 28);
    assert_eq!(count.allocations.len(), 4);
    let absent = encoded(&[colliding[17]], &[]);
    assert_eq!(
        interner
            .get(absent.bytes(), &mut Session::new(&mut count))
            .unwrap(),
        None
    );
}

#[test]
fn binary_lookup_and_duplicate_reuse_have_only_actual_event_admission() {
    #[derive(Default)]
    struct Trace(Vec<Request>);
    impl Admission for Trace {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            self.0.push(request);
            Ok(())
        }
    }
    let key = encoded(&(0..1024).collect::<Vec<_>>(), &[]);
    let repeated = encoded(&(0..1024).collect::<Vec<_>>(), &[]);
    let mut interner = Interner::new();
    let mut trace = Trace::default();
    assert_eq!(
        interner
            .get(key.bytes(), &mut Session::new(&mut trace))
            .unwrap(),
        None
    );
    interner
        .insert(key, 9, &mut Session::new(&mut trace))
        .unwrap();
    let capacity = interner.entries.capacity();
    assert_eq!(
        interner
            .get(repeated.bytes(), &mut Session::new(&mut trace))
            .unwrap(),
        Some(9)
    );
    assert_eq!(interner.len(), 1);
    assert_eq!(interner.entries.capacity(), capacity);
    let (_, allocation) = growth(1).unwrap();
    assert_eq!(
        trace.0,
        [
            Request::Items(1),
            Request::Items(1),
            Request::Allocation {
                bytes: allocation.size(),
                alignment: allocation.align()
            },
            Request::Items(1),
        ]
    );
}

#[test]
fn every_interner_refusal_preserves_entries_capacity_and_first_cause() {
    fn scenario(
        interner: &mut Interner,
        keys: Vec<key::State>,
        session: &mut Session<'_, Count>,
    ) -> Step<()> {
        for (id, key) in keys.into_iter().enumerate() {
            assert_eq!(interner.get(key.bytes(), session)?, None);
            interner.insert(key, id as u32, session)?;
        }
        interner.clear(session)
    }
    let keys = || (0..60).map(|id| encoded(&[id], &[])).collect::<Vec<_>>();
    let mut baseline = Count::default();
    scenario(
        &mut Interner::new(),
        keys(),
        &mut Session::new(&mut baseline),
    )
    .unwrap();
    let mut before = Vec::new();
    let mut capacity = 0;
    for length in 0..60 {
        before.push((length, capacity)); // lookup event
        before.push((length, capacity)); // insert event
        if length == capacity {
            before.push((length, capacity)); // prospective replacement layout
            capacity = super::capacity(growth(length + 1).unwrap().0);
        }
    }
    before.push((60, capacity)); // entry-release admission at clear
    assert_eq!(before.len(), baseline.calls);
    for (refuse, &expected) in before.iter().enumerate() {
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut interner = Interner::new();
        let owned = keys();
        let mut session = Session::new(&mut count);
        let result = scenario(&mut interner, owned, &mut session);
        assert!(matches!(result, Err(Stop::Refused)));
        let length = interner.len();
        let capacity = interner.entries.capacity();
        assert_eq!((length, capacity), expected);
        assert!(matches!(
            interner.get(&[], &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(interner.clear(&mut session), Err(Stop::Refused)));
        assert_eq!(interner.len(), length);
        assert_eq!(interner.entries.capacity(), capacity);
        assert!(matches!(session.finish(result), Err(Error::Admission(cause)) if cause == refuse));
        assert_eq!(count.calls, refuse + 1);
    }
}
