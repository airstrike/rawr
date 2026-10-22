use super::*;
use std::hash::BuildHasher;

#[test]
fn boolean_entry_updates_preserve_native_iteration_and_capacity() {
    let keys: Vec<_> = (0..80)
        .map(|n| format!("shared-prefix-dependency-{n:03}"))
        .collect();
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut table = Table::new();
    let mut native: HashMap<&str, bool, fx::Build> = HashMap::with_hasher(fx::Build);
    for (index, key) in keys.iter().enumerate().rev().chain(keys.iter().enumerate()) {
        let optional = index % 3 == 0;
        *native.entry(key.as_str()).or_insert(false) |= optional;
        if let Some(previous) = table
            .merge(key, usize::from(optional), &mut session)
            .unwrap()
        {
            table
                .replace(key, previous | usize::from(optional), &mut session)
                .unwrap();
        }
        assert_eq!(table.entries.capacity(), native.capacity());
        let expected: Vec<_> = native
            .iter()
            .map(|(&name, &optional)| (name, optional))
            .collect();
        let mut actual = Vec::new();
        let mut iter = table.iter(&mut session).unwrap();
        while let Some((name, optional)) = iter.next(&mut session).unwrap() {
            actual.push((name, optional != 0));
        }
        assert_eq!(actual, expected);
    }
}

#[test]
fn path_keys_keep_native_equality_and_first_equal_spelling() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut paths = PathTable::new();
    assert_eq!(
        paths.merge(Path::new("/a//b"), 1, &mut session).unwrap(),
        None
    );
    assert_eq!(
        paths.merge(Path::new("/a/./b"), 2, &mut session).unwrap(),
        Some(1)
    );
    assert_eq!(
        paths.merge(Path::new("/a/c"), 3, &mut session).unwrap(),
        None
    );
    let mut entries = paths.into_iter(&mut session).unwrap();
    let mut found = false;
    while let Some((key, index)) = entries.next(&mut session).unwrap() {
        if index == 1 {
            assert_eq!(key.as_os_str().as_bytes(), b"/a//b");
            found = true;
        }
    }
    assert!(found);
}

mod scenario {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-table/scenario.rs"
    ));
}

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

#[cfg(all(
    target_arch = "aarch64",
    target_pointer_width = "64",
    target_endian = "little"
))]
#[test]
fn ordered_operations_and_requested_layouts_match_pinned_map() {
    let keys = scenario::keys(|key| fx::Build.hash_one(key));
    let mut expected = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-table/observations.txt"
    ))
    .lines();
    let mut table = Table::new();
    let mut total = Account::default();
    let mut in_place = 0;
    for operation in scenario::operations() {
        let reserves = operation.0 == 'i'
            || (operation.0 == 'm' && !table.entries.contains_key(keys[operation.1].as_str()));
        if reserves
            && table.entries.len() == table.entries.capacity()
            && table.entries.len() < layout::capacity(table.buckets) / 2
        {
            in_place += 1;
        }
        let start = total.requests.len();
        let mut session = Session::new(&mut total);
        operate(&mut table, &keys, operation, &mut session).unwrap();
        session.finish(Ok(())).unwrap();
        let allocations: Vec<_> = total.requests[start..]
            .iter()
            .filter_map(|request| match request {
                document::Request::Allocation { bytes, alignment } => Some((*bytes, *alignment)),
                _ => None,
            })
            .collect();
        assert!(allocations.len() <= 1);
        let (bytes, alignment) = allocations.first().copied().unwrap_or_default();
        let mut actual = format!("{} {bytes} {alignment}", table.entries.capacity());
        for (key, value) in &table.entries {
            actual.push_str(&format!(" {key}:{value}"));
        }
        assert_eq!(actual, expected.next().unwrap(), "operation {operation:?}");
    }
    assert!(expected.next().is_none());
    assert!(
        in_place > 0,
        "scenario must reach tombstone rehash without allocation"
    );
}

#[test]
fn compact_entries_preserve_owned_result_collection_bucket_history() {
    let keys = scenario::keys(|key| fx::Build.hash_one(key));
    for count in 0..=keys.len() {
        let expected: Result<HashMap<String, usize, fx::Build>, ()> = keys[..count]
            .iter()
            .enumerate()
            .map(|(index, key)| Ok((key.clone(), index)))
            .collect();
        let expected = expected.unwrap();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let mut table = Table::new();
        for (index, key) in keys[..count].iter().enumerate() {
            table.insert(key, index, &mut session).unwrap();
        }
        assert_eq!(
            table.entries.capacity(),
            expected.capacity(),
            "prefix {count}"
        );
        assert_eq!(
            table
                .entries
                .iter()
                .map(|(&key, &index)| (key, index))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(key, &index)| (key.as_str(), index))
                .collect::<Vec<_>>(),
            "prefix {count}"
        );
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn layout_rejects_overflow_without_requesting_storage() {
    assert!(layout::growth(0).is_none());
    assert!(layout::growth(usize::MAX).is_none());
    assert!(layout::growth(isize::MAX as usize).is_none());
}

fn operate<'d, A: document::Admission>(
    table: &mut Table<'d>,
    keys: &'d [String],
    (operation, key, value): (char, usize, usize),
    session: &mut Session<'_, A>,
) -> Step<()> {
    match operation {
        'i' => {
            table.insert(&keys[key], value, session)?;
        }
        'm' => {
            table.merge(&keys[key], value, session)?;
        }
        'r' => {
            table.remove(&keys[key], session)?;
        }
        _ => return session.fail(Error::State),
    }
    Ok(())
}

#[test]
fn refusal_latches_before_table_mutation_and_blocks_later_operations() {
    let keys = scenario::keys(|key| fx::Build.hash_one(key));
    let operations = scenario::operations();
    let mut baseline = Account::default();
    let mut table = Table::new();
    let mut session = Session::new(&mut baseline);
    for &operation in &operations {
        operate(&mut table, &keys, operation, &mut session).unwrap();
    }
    session.finish(Ok(())).unwrap();
    for refusal in 0..baseline.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut table = Table::new();
        let mut session = Session::new(&mut account);
        // Prior usage participates in the same refusal sequence.
        for &operation in &operations {
            let before: Vec<_> = table
                .entries
                .iter()
                .map(|(&key, &value)| (key, value))
                .collect();
            let buckets = table.buckets;
            if operate(&mut table, &keys, operation, &mut session).is_err() {
                assert_eq!(
                    before,
                    table
                        .entries
                        .iter()
                        .map(|(&key, &value)| (key, value))
                        .collect::<Vec<_>>()
                );
                assert_eq!(buckets, table.buckets);
                assert!(table.insert("later", 0, &mut session).is_err());
                assert!(table.merge("later", 0, &mut session).is_err());
                assert!(table.get("later", &mut session).is_err());
                assert!(table.remove("later", &mut session).is_err());
                assert!(table.iter(&mut session).is_err());
                break;
            }
        }
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests.len(), refusal + 1);
    }
}

#[test]
fn iteration_preserves_order_and_uses_original_account() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut table = Table::new();
    for (index, key) in ["a", "b", "c", "d"].into_iter().enumerate() {
        table.insert(key, index, &mut session).unwrap();
    }
    let expected: Vec<_> = table
        .entries
        .iter()
        .map(|(&key, &value)| (key, value))
        .collect();
    let mut iterator = table.iter(&mut session).unwrap();
    let mut actual = Vec::new();
    while let Some(pair) = iterator.next(&mut session).unwrap() {
        actual.push(pair);
    }
    assert_eq!(actual, expected);
    session.finish(Ok(())).unwrap();
}

fn state<'d>(table: &Table<'d>) -> Vec<(&'d str, usize)> {
    table
        .entries
        .iter()
        .map(|(&key, &value)| (key, value))
        .collect()
}

#[test]
fn copy_preserves_collision_removal_history_and_subsequent_growth() {
    let keys = scenario::keys(|key| fx::Build.hash_one(key));
    let operations = scenario::operations();
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut source = Table::new();
    let mut saw_deleted = false;
    for (position, &operation) in operations.iter().enumerate() {
        operate(&mut source, &keys, operation, &mut session).unwrap();
        saw_deleted |= source.entries.capacity() < layout::capacity(source.buckets);
        let mut copy = source.try_copy(&mut session).unwrap();
        let mut expected = Table {
            entries: source.entries.clone(),
            buckets: source.buckets,
        };
        assert_eq!(copy.buckets, expected.buckets);
        assert_eq!(copy.entries.capacity(), expected.entries.capacity());
        assert_eq!(state(&copy), state(&expected));
        // The remaining scenario includes tombstone reuse, in-place rehash,
        // duplicate insertion and growth; each transition must stay identical.
        for &next in &operations[position + 1..] {
            operate(&mut copy, &keys, next, &mut session).unwrap();
            operate(&mut expected, &keys, next, &mut session).unwrap();
            assert_eq!(copy.buckets, expected.buckets);
            assert_eq!(copy.entries.capacity(), expected.entries.capacity());
            assert_eq!(state(&copy), state(&expected));
        }
    }
    assert!(saw_deleted);
    session.finish(Ok(())).unwrap();
}

#[test]
fn copy_distinguishes_singleton_from_allocated_empty() {
    for allocated in [false, true] {
        let mut account = Account::default();
        let mut source = Table::new();
        if allocated {
            let mut session = Session::new(&mut account);
            source.insert("removed", 0, &mut session).unwrap();
            source.remove("removed", &mut session).unwrap();
            session.finish(Ok(())).unwrap();
        }
        let start = account.requests.len();
        let mut session = Session::new(&mut account);
        let copy = source.try_copy(&mut session).unwrap();
        assert!(copy.entries.is_empty());
        assert_eq!(copy.buckets, source.buckets);
        assert_eq!(copy.entries.capacity(), source.entries.clone().capacity());
        session.finish(Ok(())).unwrap();
        assert!(matches!(
            account.requests[start],
            document::Request::Items(1)
        ));
        if allocated {
            let (_, allocation) = layout::growth(layout::capacity(source.buckets)).unwrap();
            assert!(
                matches!(account.requests[start + 1], document::Request::Allocation {
                bytes, alignment
            } if bytes == allocation.size() && alignment == allocation.align())
            );
        }
        assert_eq!(
            account.requests.len() - start,
            if allocated { 2 } else { 1 }
        );
    }
}

#[test]
fn copy_refusal_precedes_reservation_and_latches_original_budget() {
    struct Budget {
        items: usize,
        bytes: usize,
        calls: usize,
    }
    impl document::Admission for Budget {
        type Error = &'static str;
        fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
            self.calls += 1;
            let (remaining, amount, kind) = match request {
                document::Request::Items(amount) => (&mut self.items, amount, "items"),
                document::Request::Allocation { bytes, .. } => (&mut self.bytes, bytes, "bytes"),
                _ => return Ok(()),
            };
            *remaining = remaining.checked_sub(amount).ok_or(kind)?;
            Ok(())
        }
    }
    let (_, allocation) = layout::growth(1).unwrap();
    for (items, bytes, failure) in [
        (2, allocation.size() * 2, None),
        (1, allocation.size() * 2, Some("items")),
        (2, allocation.size() * 2 - 1, Some("bytes")),
    ] {
        let mut budget = Budget {
            items,
            bytes,
            calls: 0,
        };
        let mut session = Session::new(&mut budget);
        let mut source = Table::new();
        source.insert("existing", 7, &mut session).unwrap();
        let before = state(&source);
        let result = source.try_copy(&mut session);
        assert_eq!(state(&source), before);
        if let Some(kind) = failure {
            assert!(result.is_err());
            assert!(source.try_copy(&mut session).is_err());
            assert!(source.insert("later", 8, &mut session).is_err());
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(error)) if error == kind)
            );
            assert_eq!(budget.calls, if kind == "items" { 4 } else { 5 });
        } else {
            assert_eq!(state(&result.unwrap()), before);
            session.finish(Ok(())).unwrap();
            assert_eq!(budget.items, 0);
            assert_eq!(budget.bytes, 0);
            assert_eq!(budget.calls, 5);
        }
    }
}

#[test]
fn copied_indices_remap_without_changing_placement_or_growth() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut source = Table::new();
    for (index, key) in ["a", "b", "c", "d"].into_iter().enumerate() {
        source.insert(key, index, &mut session).unwrap();
    }
    source.remove("b", &mut session).unwrap();
    let mut copy = source.try_copy(&mut session).unwrap();
    let capacity = copy.entries.capacity();
    let buckets = copy.buckets;
    let expected: Vec<_> = state(&source)
        .into_iter()
        .map(|(key, index)| (key, 17 - index * 3))
        .collect();
    for &(key, value) in &expected {
        copy.replace(key, value, &mut session).unwrap();
    }
    assert_eq!(state(&copy), expected);
    assert_eq!(copy.entries.capacity(), capacity);
    assert_eq!(copy.buckets, buckets);
    assert_eq!(source.entries.get("a"), Some(&0));
    assert!(copy.replace("missing", 0, &mut session).is_err());
    assert!(copy.replace("a", 99, &mut session).is_err());
    assert_eq!(state(&copy), expected);
    assert!(matches!(session.finish(Ok(())), Err(Error::State)));
}
