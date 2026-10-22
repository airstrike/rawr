use rawr_toml::document;

use super::super::{schema, session, storage, value, walk, Error};
use super::{parse, Account};
use crate::input::membership::Budget;
use crate::{cargo, input, node};

fn through_temporary_capture<'d>(
    document: &'d document::Document<'d>,
    account: &mut Account,
) -> value::Projection<'d> {
    let mut session = session::Session::new(account);
    let result = (|| {
        let captured = walk::run(super::input(document), value::Mode::Content, &mut session)?;
        let converted = walk::run(
            value::Input::replay(&captured, 0),
            value::Mode::Toml,
            &mut session,
        )?;
        drop(captured);
        Ok(converted)
    })();
    session.finish(result).unwrap()
}

#[test]
fn converted_payload_outlives_temporary_capture_without_reborrowing_it() {
    // The decoded string is owned by Document, while the synthetic datetime
    // text lives inline in the captured value. Both must survive stage teardown.
    let document = parse("value={text=\"decoded\\ntext\",time=2024-01-02T03:04:05.123Z}");
    let mut account = Account::default();
    let converted = through_temporary_capture(&document, &mut account);
    let value::Kind::Map(children) = converted.nodes.as_slice()[0].kind else {
        panic!("converted root map")
    };
    let first = &converted.edges.as_slice()[children.first.unwrap()];
    let value::Kind::String(value::Text::Borrowed(text)) =
        converted.nodes.as_slice()[first.child].kind
    else {
        panic!("converted string borrows the original document")
    };
    assert_eq!(text, "decoded\ntext");
    let document::Value::Table(table) = document.table().get("value").unwrap().value() else {
        panic!("source map")
    };
    let document::Value::String(original) = table.get("text").unwrap().value() else {
        panic!("source string")
    };
    assert_eq!(text.as_ptr(), original.as_str().as_ptr());
    let last = &converted.edges.as_slice()[children.last.unwrap()];
    let value::Kind::DateTime(date) = converted.nodes.as_slice()[last.child].kind else {
        panic!("converted datetime owns its components")
    };
    assert_eq!(date.to_string(), "2024-01-02T03:04:05.123Z");
}

fn growth_fixture() -> document::Document<'static> {
    parse("value={z=['one','two','three','four','five','six','seven','eight','nine'],b={q=1,a=2},a=2024-01-02T03:04:05.123456789+12:30}")
}

#[test]
fn every_owned_growth_item_and_string_refusal_stops_before_allocation() {
    let document = growth_fixture();
    let mut full = Account::default();
    walk::project(super::input(&document), value::Mode::SerdeValue, &mut full).unwrap();
    assert!(full
        .requests
        .iter()
        .any(|request| matches!(request, document::Request::String(_))));
    assert!(
        full.requests
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count()
            >= 8
    );
    for refusal in 0..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        storage::ALLOCATIONS.with(|count| count.set(0));
        let result = walk::project(
            super::input(&document),
            value::Mode::SerdeValue,
            &mut account,
        );
        assert!(
            matches!(result, Err(Error::Admission("original refusal"))),
            "request {refusal}"
        );
        assert_eq!(account.requests, full.requests[..=refusal]);
        let allocations = full.requests[..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(
            storage::ALLOCATIONS.with(|count| count.get()),
            allocations,
            "request {refusal}"
        );
    }
}

#[test]
fn allocator_bonus_does_not_authorize_an_unadmitted_extent() {
    let mut values = storage::bonus();
    let mut account = Account {
        refusal: Some(1),
        ..Account::default()
    };
    let mut session = session::Session::new(&mut account);
    storage::ALLOCATIONS.with(|count| count.set(0));
    let result = values.push(5, &mut session);
    assert!(matches!(
        session.finish(result),
        Err(Error::Admission("original refusal"))
    ));
    assert_eq!(values.as_slice(), [1, 2, 3, 4]);
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    assert_eq!(
        account.requests,
        [
            document::Request::Items(1),
            document::Request::Allocation {
                bytes: 8 * std::mem::size_of::<u64>(),
                alignment: std::mem::align_of::<u64>()
            },
        ]
    );
}

fn budgeted<'d>(
    document: &'d document::Document<'d>,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
) -> (bool, input::Usage, crate::exceedance::Record) {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(item_limit, byte_limit, string_limit);
    allocation.items(7).unwrap();
    allocation.bytes(19).unwrap();
    let accepted = walk::project(
        super::input(document),
        value::Mode::SerdeValue,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (accepted, allocation.usage, allocation.exceedances())
}

#[test]
fn actual_original_account_accepts_exact_and_records_each_floor_with_prior_usage() {
    let document = growth_fixture();
    let (accepted, exact, _) = budgeted(&document, usize::MAX, usize::MAX, usize::MAX);
    assert!(accepted);
    assert!(exact.items > 7 && exact.bytes > 19);
    assert_eq!(
        budgeted(&document, exact.items, exact.bytes, 35),
        (true, exact, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    super::admits_one_short(
        budgeted(&document, exact.items - 1, exact.bytes, 35),
        exact,
        input::INPUT_ITEMS_RESOURCE,
        exact.items,
    );
    super::admits_one_short(
        budgeted(&document, exact.items, exact.bytes - 1, 35),
        exact,
        input::INPUT_BYTES_RESOURCE,
        exact.bytes,
    );
    // The synthetic datetime is the longest string, exactly 35 bytes.
    // One short admits the same work and records only the string axis.
    let (okay, usage, exceeded) = budgeted(&document, exact.items, exact.bytes, 34);
    assert!(okay, "a string target is not an admission gate");
    assert_eq!(usage, exact, "the exact charge is still spent");
    let mut axes = exceeded.axes();
    let (Some(axis), None) = (axes.next(), axes.next()) else {
        panic!("only the string axis is recorded: {exceeded:?}");
    };
    assert_eq!(
        (
            axis.name,
            axis.target,
            axis.first_requested,
            axis.high_water
        ),
        ("membership decoded TOML string bytes", 34, 35, 35)
    );
}

fn deep_source(leaf: &str) -> String {
    let mut source = String::from("value={");
    for _ in 0..79 {
        for _ in 0..78 {
            source.push_str("a.");
        }
        source.push_str("a={");
    }
    source.push_str("leaf=");
    source.push_str(leaf);
    for _ in 0..80 {
        source.push('}');
    }
    source
}

#[test]
fn deeply_compounded_generic_success_and_failure_have_flat_teardown() {
    let source = deep_source("1");
    let document = parse(&source);
    let projected = walk::project(
        super::input(&document),
        value::Mode::SerdeValue,
        &mut Account::default(),
    )
    .unwrap();
    assert!(projected.nodes.len() > 6_000);
    drop(projected);
    drop(document);
    let source = deep_source("340282366920938463463374607431768211456");
    let document = parse(&source);
    assert!(
        matches!(walk::project(super::input(&document), value::Mode::SerdeValue, &mut Account::default()), Err(Error::Invalid(error)) if error.tag == super::super::Tag::IntegerOverflow)
    );
    drop(document);
}

#[test]
fn already_captured_deep_content_can_be_ignored_at_the_exact_original_floor() {
    let source = deep_source("1");
    let document = parse(&source);
    let graph = node::GraphBudget::new();
    let mut measured = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    measured.items(7).unwrap();
    measured.bytes(19).unwrap();
    walk::project(
        super::input(&document),
        value::Mode::Content,
        &mut Budget {
            graph: &graph,
            allocation: &mut measured,
        },
    )
    .unwrap();
    let exact = measured.usage;

    let mut allocation = input::Allocation::with_limits(exact.items, exact.bytes, usize::MAX);
    allocation.items(7).unwrap();
    allocation.bytes(19).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let captured =
        walk::project(super::input(&document), value::Mode::Content, &mut budget).unwrap();
    assert!(captured.nodes.len() > 6_000);
    assert_eq!(budget.allocation.usage, exact);
    storage::ALLOCATIONS.with(|count| count.set(0));
    let ignored = walk::project(
        value::Input::replay(&captured, 0),
        value::Mode::Ignored,
        &mut budget,
    )
    .unwrap();
    assert!(ignored.nodes.as_slice().is_empty());
    assert_eq!(budget.allocation.usage, exact);
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
}

#[test]
fn pure_interpretation_uses_no_filesystem_hash_or_candidate_comparison_event() {
    let document = growth_fixture();
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    for _ in 0..64 {
        graph.comparison().unwrap();
    }
    let limits = cargo::Limits::standard();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    walk::project(
        super::input(&document),
        value::Mode::SerdeValue,
        &mut budget,
    )
    .unwrap();
}

#[test]
fn native_acquisition_stored_host_document_and_target_share_one_original_account() {
    use crate::input::membership::{document, host, live, stored};
    use std::os::unix::ffi::OsStrExt;

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!("rawr-manifest-chain-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let fixture = Fixture(root);
    let path = fixture.0.join("Cargo.toml");
    std::fs::write(
        &path,
        b"[lib]\nname='borrowed'\npath='src/lib.rs'\ncrate-type=['lib']\n[metadata]\nz=1\na=2\n",
    )
    .unwrap();

    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    allocation.items(11).unwrap();
    allocation.bytes(23).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let acquired = live::manifest(path.as_os_str().as_bytes(), &mut budget).unwrap();
    let acquired_usage = budget.allocation.usage;
    let retained =
        stored::Stored::from_records(std::slice::from_ref(acquired.record()), &mut budget).unwrap();
    let encoded = retained.encode(&mut budget).unwrap();
    let decoded = stored::Stored::decode(&encoded, &mut budget).unwrap();
    let mut host = host::Host::from_image(&decoded, budget).unwrap();
    let file = host.file(path.as_os_str().as_bytes()).unwrap().unwrap();
    let parsed = document::parse(file, &limits, host.budget()).unwrap();
    let before_typed = host.budget().allocation.usage;
    assert!(before_typed.items > acquired_usage.items && before_typed.bytes > acquired_usage.bytes);
    let mut session = session::Session::new(host.budget());
    let result = schema::Target::interpret(
        value::Input::raw(parsed.table().get("lib").unwrap()),
        &mut session,
    );
    let target = session.finish(result).unwrap();
    assert_eq!(
        super::keys(target.values(), 0),
        ["name", "path", "crate-type"]
    );
    walk::project(
        value::Input::raw(parsed.table().get("metadata").unwrap()),
        value::Mode::Toml,
        host.budget(),
    )
    .unwrap();
    let after = host.budget().allocation.usage;
    assert!(after.items > before_typed.items && after.bytes > before_typed.bytes);
    host.finish().unwrap();
    assert_eq!(allocation.usage, after);
}
