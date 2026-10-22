use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use std::hash::{BuildHasher, Hasher};

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    calls: usize,
    refuse: Option<usize>,
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

fn allocations(count: &Count) -> Vec<(usize, usize)> {
    count
        .requests
        .iter()
        .filter_map(|request| match *request {
            Request::Allocation { bytes, alignment } => Some((bytes, alignment)),
            _ => None,
        })
        .collect()
}

fn ordered(map: &Map<usize>) -> Vec<(Vec<u8>, Vec<usize>)> {
    map.entries
        .iter()
        .map(|(key, rows)| (key.clone(), rows.values.clone()))
        .collect()
}

fn indices() -> Indices {
    let mut table = Indices::new();
    for (id, text) in ["alpha", "beta", "gamma"].into_iter().enumerate() {
        table
            .add(id, text.to_owned(), &mut Session::new(&mut Account))
            .unwrap();
    }
    table
}

#[test]
fn exact_fnv_hashes_owned_and_borrowed_binary_keys_with_native_length_prefix() {
    for bytes in [
        vec![],
        vec![0],
        vec![0, 0],
        vec![0xff],
        vec![0x80, 0xff, 0],
        b"abc".to_vec(),
        b"abcd".to_vec(),
    ] {
        let mut expected = 0xcbf29ce484222325u64;
        for byte in bytes
            .len()
            .to_ne_bytes()
            .into_iter()
            .chain(bytes.iter().copied())
        {
            expected = (expected ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        let build = fnv::Build::default();
        assert_eq!(build.hash_one(&bytes), expected);
        assert_eq!(build.hash_one(bytes.as_slice()), expected);
        let mut split = build.build_hasher();
        split.write_usize(bytes.len());
        for part in bytes.chunks(2) {
            split.write(part);
        }
        assert_eq!(split.finish(), expected);
    }
    assert_ne!(
        fnv::Build::default().hash_one(b"abc".as_slice()),
        fnv::Build::default().hash_one(b"abcd".as_slice())
    );
}

#[test]
fn full_occupied_entry_retains_key_and_buckets_before_value_growth() {
    let mut table = indices();
    assert_eq!(table.0.entries.capacity(), 3);
    let stored_key = table
        .0
        .entries
        .get_key_value(b"alpha".as_slice())
        .unwrap()
        .0
        .as_ptr();
    let mut count = Count::default();
    table
        .add(10, "alpha".to_owned(), &mut Session::new(&mut count))
        .unwrap();
    assert!(allocations(&count).is_empty());
    assert_eq!(table.0.entries.capacity(), 3);
    assert_eq!(
        table
            .0
            .entries
            .get_key_value(b"alpha".as_slice())
            .unwrap()
            .0
            .as_ptr(),
        stored_key
    );
    for index in [11, 12] {
        table
            .add(index, "alpha".to_owned(), &mut Session::new(&mut Account))
            .unwrap();
    }
    let mut count = Count::default();
    table
        .add(13, "alpha".to_owned(), &mut Session::new(&mut count))
        .unwrap();
    assert_eq!(table.0.entries.capacity(), 3);
    assert_eq!(
        allocations(&count),
        [(
            8 * std::mem::size_of::<usize>(),
            std::mem::align_of::<usize>()
        )]
    );
    assert_eq!(
        table
            .get(b"alpha", &mut Session::new(&mut Account))
            .unwrap(),
        Some(&[0, 10, 11, 12, 13][..])
    );
}

#[test]
fn full_vacant_entry_reserves_actual_entry_layout_before_empty_list_and_append() {
    let mut table = indices();
    let mut count = Count::default();
    table
        .add(3, "delta".to_owned(), &mut Session::new(&mut count))
        .unwrap();
    let (_, layout) = layout::growth::<usize>(4).unwrap();
    assert_eq!(
        allocations(&count),
        [
            (layout.size(), layout.align()),
            (
                4 * std::mem::size_of::<usize>(),
                std::mem::align_of::<usize>()
            )
        ]
    );
    assert_eq!(table.0.entries.capacity(), 7);
    assert_eq!(
        table
            .get(b"delta", &mut Session::new(&mut Account))
            .unwrap(),
        Some(&[3][..])
    );

    let row_refusal = count
        .requests
        .iter()
        .enumerate()
        .filter_map(|(at, request)| matches!(request, Request::Allocation { .. }).then_some(at))
        .nth(1)
        .unwrap();
    let mut table = indices();
    let mut refused = Count {
        refuse: Some(row_refusal),
        ..Count::default()
    };
    let mut session = Session::new(&mut refused);
    assert!(matches!(
        table.add(3, "delta".to_owned(), &mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(table.0.entries.capacity(), 7);
    assert!(table.0.entries[b"delta".as_slice()].values.is_empty());
    assert!(matches!(
        table.contains(b"delta", &mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(refused.calls, row_refusal + 1);
}

#[test]
fn ordinary_destination_insert_clones_key_then_reserves_even_for_full_replacement() {
    let mut table = Compiled::new();
    for extension in [b".a".as_slice(), b".b", b".c"] {
        table
            .insert_empty(extension, &mut Session::new(&mut Account))
            .unwrap();
    }
    assert_eq!(table.0.entries.capacity(), 3);
    let old_key = table
        .0
        .entries
        .get_key_value(b".a".as_slice())
        .unwrap()
        .0
        .as_ptr();
    let mut count = Count::default();
    table
        .insert_empty(b".a", &mut Session::new(&mut count))
        .unwrap();
    let (_, layout) = layout::growth::<(usize, nfa::Regex)>(4).unwrap();
    assert_eq!(
        allocations(&count),
        [(2, 1), (layout.size(), layout.align())]
    );
    assert_eq!(table.0.entries.capacity(), 7);
    assert_eq!(
        table
            .0
            .entries
            .get_key_value(b".a".as_slice())
            .unwrap()
            .0
            .as_ptr(),
        old_key
    );
    assert!(table
        .get_mut(b".a", &mut Session::new(&mut Account))
        .unwrap()
        .unwrap()
        .is_empty());
}

fn collisions() -> Vec<Vec<u8>> {
    // Same h2 and initial bucket under the actual FNV policy. This exercises
    // real probing/equality; it does not claim manufactured equal u64 hashes.
    let mut groups: Vec<Vec<u64>> = (0..1024).map(|_| Vec::new()).collect();
    for candidate in 0..32768u64 {
        let key = candidate.to_ne_bytes().to_vec();
        let hash = fnv::Build::default().hash_one(&key);
        let group = ((hash >> 57) as usize) * 8 + (hash as usize & 7);
        groups[group].push(candidate);
        if groups[group].len() == 12 {
            return groups[group]
                .iter()
                .map(|candidate| candidate.to_ne_bytes().to_vec())
                .collect();
        }
    }
    panic!("bounded FNV collision fixture must fill one probe group");
}

#[test]
fn actual_collision_growth_and_borrowed_lookup_preserve_direct_std_bucket_order() {
    let mut table = Map::<usize>::new();
    let mut source: HashMap<Vec<u8>, Vec<usize>, fnv::Build> =
        HashMap::with_hasher(fnv::Build::default());
    let keys = collisions();
    for (id, key) in keys.iter().enumerate() {
        table
            .entry(key.clone(), &mut Session::new(&mut Account))
            .unwrap()
            .push(id, &mut Session::new(&mut Account))
            .unwrap();
        source.entry(key.clone()).or_insert(vec![]).push(id);
        assert_eq!(table.entries.capacity(), source.capacity());
        assert_eq!(
            ordered(&table),
            source
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        );
    }
    for (id, key) in keys.iter().enumerate().rev() {
        table
            .entry(key.clone(), &mut Session::new(&mut Account))
            .unwrap()
            .push(id + 100, &mut Session::new(&mut Account))
            .unwrap();
        source.entry(key.clone()).or_insert(vec![]).push(id + 100);
        assert_eq!(table.entries[key.as_slice()].values, source[key.as_slice()]);
    }
    for key in [vec![], vec![0], vec![0xff], vec![0; 9]] {
        assert_eq!(
            table.entries.get(key.as_slice()).map(|r| &r.values),
            source.get(key.as_slice())
        );
    }
    assert_eq!(ordered(&table), source.into_iter().collect::<Vec<_>>());
}

#[test]
fn owned_regex_groups_consume_actual_bucket_order_and_keep_text_ownership_until_yield() {
    let mut table = Regexes::new();
    let mut source: HashMap<Vec<u8>, Vec<(usize, String)>, fnv::Build> =
        HashMap::with_hasher(fnv::Build::default());
    let mut pointers = Vec::new();
    for index in 0..80 {
        let key = format!(".ext{}", index % 29);
        let regex = format!("(?-u)^literal{index}$");
        pointers.push(regex.as_ptr());
        source
            .entry(key.as_bytes().to_vec())
            .or_insert(vec![])
            .push((index, regex.clone()));
        table
            .add(index, key, regex, &mut Session::new(&mut Account))
            .unwrap();
    }
    let mut groups = table.into_iter(&mut Session::new(&mut Account)).unwrap();
    let mut expected = source.into_iter();
    while let Some((key, rows)) = groups.next(&mut Session::new(&mut Account)).unwrap() {
        let (expected_key, expected_rows) = expected.next().unwrap();
        assert_eq!(key, expected_key);
        // The source inserts its destination key here, before consuming rows.
        let mut values = rows.into_iter(&mut Session::new(&mut Account)).unwrap();
        for (expected_id, expected_text) in expected_rows {
            let (id, text) = values
                .next(&mut Session::new(&mut Account))
                .unwrap()
                .unwrap();
            assert_eq!((id, text.as_str()), (expected_id, expected_text.as_str()));
            assert_eq!(text.as_ptr(), pointers[id]);
        }
        assert!(values
            .next(&mut Session::new(&mut Account))
            .unwrap()
            .is_none());
    }
    assert!(expected.next().is_none());
}

#[test]
fn every_entry_and_lookup_refusal_is_terminal_and_keeps_the_original_request_prefix() {
    for (literal, index) in [("alpha", 9), ("delta", 3)] {
        let mut table = indices();
        for index in [10, 11, 12] {
            table
                .add(index, "alpha".to_owned(), &mut Session::new(&mut Account))
                .unwrap();
        }
        let mut complete = Count::default();
        table
            .add(index, literal.to_owned(), &mut Session::new(&mut complete))
            .unwrap();
        for refuse in 0..complete.calls {
            let mut table = indices();
            for index in [10, 11, 12] {
                table
                    .add(index, "alpha".to_owned(), &mut Session::new(&mut Account))
                    .unwrap();
            }
            let mut count = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                table.add(index, literal.to_owned(), &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                table.contains(literal.as_bytes(), &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                table.get(literal.as_bytes(), &mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refuse)
            );
            assert_eq!(count.calls, refuse + 1);
            assert_eq!(count.requests, complete.requests[..refuse]);
        }
    }
}

#[test]
fn source_text_limits_remain_distinct_from_binary_hash_and_clone_work() {
    struct Limit;
    impl Admission for Limit {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            if matches!(request, Request::String(length) if length > 8) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    let mut table = Regexes::new();
    let mut account = Limit;
    let mut session = Session::new(&mut account);
    assert!(matches!(
        table.add(0, ".rs".to_owned(), "(?-u)^abc$".to_owned(), &mut session),
        Err(Stop::Refused)
    ));
    assert!(table.0.entries.is_empty());
    let table = indices();
    let mut count = Count::default();
    assert!(table
        .contains(b"alpha", &mut Session::new(&mut count))
        .unwrap());
    assert_eq!(count.requests, [Request::Items(1)]);
    let mut count = Count::default();
    let copied = copy(&[0, 0xff, 0x80], &mut Session::new(&mut count)).unwrap();
    assert_eq!(copied, [0, 0xff, 0x80]);
    assert_eq!(
        count.requests,
        [
            Request::Items(1),
            Request::Allocation {
                bytes: 3,
                alignment: 1
            },
            Request::Items(3)
        ]
    );
}

fn compiled(glob: &str) -> nfa::Regex {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let parsed = super::super::parse(glob, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    let emitted = parsed.regex(&mut session).unwrap();
    let batch = super::super::regex::batch::Batch::parse(&[&emitted], &mut session)
        .unwrap()
        .ok()
        .unwrap();
    let translated = batch.translate(&mut session).unwrap().ok().unwrap();
    translated
        .compile(nfa::Kind::LeftmostFirst, &mut session)
        .unwrap()
        .ok()
        .unwrap()
}

#[test]
fn concrete_compiled_rows_keep_independent_regex_cache_histories_and_global_order() {
    let mut table = Compiled::new();
    table
        .insert_empty(b".rs", &mut Session::new(&mut Account))
        .unwrap();
    assert!(table
        .get_mut(b".rs", &mut Session::new(&mut Account))
        .unwrap()
        .unwrap()
        .is_empty());
    table
        .push(b".rs", 7, compiled("a.rs"), &mut Session::new(&mut Account))
        .unwrap();
    table
        .push(b".rs", 2, compiled("b.rs"), &mut Session::new(&mut Account))
        .unwrap();
    let rows = table
        .get_mut(b".rs", &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!([rows[0].0, rows[1].0], [7, 2]);
    for (index, bytes) in [(0, b"a.rs"), (1, b"b.rs")] {
        let mut cold = Count::default();
        assert!(rows[index]
            .1
            .is_match(bytes, &mut Session::new(&mut cold))
            .unwrap());
        assert!(!allocations(&cold).is_empty());
        let mut warm = Count::default();
        assert!(rows[index]
            .1
            .is_match(bytes, &mut Session::new(&mut warm))
            .unwrap());
        assert!(allocations(&warm).is_empty());
    }
    assert!(!rows[0]
        .1
        .is_match(b"b.rs", &mut Session::new(&mut Account))
        .unwrap());
    assert!(!rows[1]
        .1
        .is_match(b"a.rs", &mut Session::new(&mut Account))
        .unwrap());
}

#[test]
fn destination_and_consuming_iteration_refusals_stop_before_the_next_operation() {
    let mut table = Compiled::new();
    table
        .insert_empty(b".rs", &mut Session::new(&mut Account))
        .unwrap();
    let mut count = Count::default();
    table
        .push(b".rs", 0, compiled("a.rs"), &mut Session::new(&mut count))
        .unwrap();
    for refusal in 0..count.calls {
        let mut table = Compiled::new();
        table
            .insert_empty(b".rs", &mut Session::new(&mut Account))
            .unwrap();
        let mut refused = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut refused);
        assert!(matches!(
            table.push(b".rs", 0, compiled("a.rs"), &mut session),
            Err(Stop::Refused)
        ));
        assert!(table.0.entries[b".rs".as_slice()].values.is_empty());
        assert!(matches!(
            table.get_mut(b".rs", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refusal));
        assert_eq!(refused.requests, count.requests[..refusal]);
        assert_eq!(refused.calls, refusal + 1);
    }

    let mut builder = Regexes::new();
    builder
        .add(
            0,
            ".rs".to_owned(),
            "(?-u)^a$".to_owned(),
            &mut Session::new(&mut Account),
        )
        .unwrap();
    let mut groups = builder.into_iter(&mut Session::new(&mut Account)).unwrap();
    let mut count = Count {
        refuse: Some(0),
        ..Count::default()
    };
    let mut session = Session::new(&mut count);
    assert!(matches!(groups.next(&mut session), Err(Stop::Refused)));
    assert_eq!(groups.entries.len(), 1);
    assert!(matches!(groups.next(&mut session), Err(Stop::Refused)));
    assert_eq!(count.calls, 1);

    let (_, rows) = groups.entries.next().unwrap();
    let mut values = Texts(rows)
        .into_iter(&mut Session::new(&mut Account))
        .unwrap();
    let mut count = Count {
        refuse: Some(0),
        ..Count::default()
    };
    let mut session = Session::new(&mut count);
    assert!(matches!(values.next(&mut session), Err(Stop::Refused)));
    assert_eq!(values.entries.len(), 1);
    assert!(matches!(values.next(&mut session), Err(Stop::Refused)));
    assert_eq!(count.calls, 1);
}

#[test]
fn partial_consuming_drop_and_replacement_keep_direct_std_bucket_and_row_order() {
    use std::{cell::RefCell, rc::Rc};
    struct Probe {
        id: usize,
        log: Rc<RefCell<Vec<usize>>>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            self.log.borrow_mut().push(self.id);
        }
    }
    fn run(adapted: bool, consume: bool) -> Vec<usize> {
        let log = Rc::new(RefCell::new(Vec::with_capacity(64)));
        let mut table = Map::<Probe>::new();
        let mut source: HashMap<Vec<u8>, Vec<Probe>, fnv::Build> =
            HashMap::with_hasher(fnv::Build::default());
        for index in 0..30 {
            let key = format!("k{}", index % 11).into_bytes();
            let value = Probe {
                id: index,
                log: Rc::clone(&log),
            };
            if adapted {
                table
                    .entry(key, &mut Session::new(&mut Account))
                    .unwrap()
                    .push(value, &mut Session::new(&mut Account))
                    .unwrap();
            } else {
                source.entry(key).or_insert(vec![]).push(value);
            }
        }
        if consume {
            if adapted {
                let mut iter = table.entries.into_iter();
                let (key, rows) = iter.next().unwrap();
                drop(key);
                let mut values = rows.values.into_iter();
                drop(values.next().unwrap());
                drop(values);
                drop(iter);
            } else {
                let mut iter = source.into_iter();
                let (key, rows) = iter.next().unwrap();
                drop(key);
                let mut values = rows.into_iter();
                drop(values.next().unwrap());
                drop(values);
                drop(iter);
            }
        } else if adapted {
            table
                .insert_empty(b"k3".to_vec(), &mut Session::new(&mut Account))
                .unwrap();
            drop(table);
        } else {
            source.insert(b"k3".to_vec(), vec![]);
            drop(source);
        }

        log.borrow().clone()
    }
    for consume in [false, true] {
        let source = run(false, consume);
        assert_eq!(source.len(), 30);
        assert_eq!(run(true, consume), source);
    }
}
