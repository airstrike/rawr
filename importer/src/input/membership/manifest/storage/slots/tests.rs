use super::*;

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

#[test]
fn borrowers_survive_growth_in_separate_owner_scopes() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut bodies = Slots::new();
    let mut views = Slots::new();
    let mut bodies = bodies.writer().unwrap();
    let mut views = views.writer().unwrap();
    let first = bodies.push(17u64, &mut session).unwrap();
    let view = views.push(first, &mut session).unwrap();
    for value in 0..1000 {
        let body = bodies.push(value, &mut session).unwrap();
        views.push(body, &mut session).unwrap();
    }
    assert_eq!(**view, 17);
    assert!(std::ptr::eq(first, *view));
    session.finish(Ok(())).unwrap();
}

#[test]
fn every_refusal_precedes_allocation_and_publication() {
    for refusal in 0..5 {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let mut slots = Slots::new();
        let mut writer = slots.writer().unwrap();
        super::super::ALLOCATIONS.with(|count| count.set(0));
        let result = writer
            .push(1u64, &mut session)
            .and_then(|_| writer.push(2, &mut session))
            .and_then(|_| writer.push(3, &mut session));
        assert!(result.is_err());
        assert!(writer.push(3, &mut session).is_err());
        assert_eq!(
            super::super::ALLOCATIONS.with(|count| count.get()),
            usize::from(refusal >= 3)
        );
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests.len(), refusal + 1);
    }
}

#[test]
fn one_writer_preserves_entries_and_charges_full_link_layouts() {
    let mut slots = Slots::new();
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut writer = slots.writer().unwrap();
    writer.push(1u64, &mut session).unwrap();
    writer.push(2, &mut session).unwrap();
    writer.push(3, &mut session).unwrap();
    session.finish(Ok(())).unwrap();
    assert!(slots.writer().is_none());
    let layout = Layout::array::<Slot<u64>>(1).unwrap();
    assert_eq!(
        account.requests,
        vec![
            document::Request::Items(1),
            document::Request::Items(1),
            document::Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align()
            },
            document::Request::Items(1),
            document::Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align()
            },
        ]
    );
    assert_eq!(slots.first.value.get(), Some(&1));
    let second = &slots.first.next.get().unwrap()[0];
    assert_eq!(second.value.get(), Some(&2));
    assert_eq!(second.next.get().unwrap()[0].value.get(), Some(&3));
}

#[test]
fn teardown_is_iterative_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            struct Count<'a>(&'a std::cell::Cell<usize>);
            impl Drop for Count<'_> {
                fn drop(&mut self) {
                    self.0.set(self.0.get() + 1);
                }
            }
            let count = std::cell::Cell::new(0);
            let mut account = Account::default();
            let mut session = Session::new(&mut account);
            {
                let mut slots = Slots::new();
                let mut writer = slots.writer().unwrap();
                for _ in 0..30_000 {
                    writer.push(Count(&count), &mut session).unwrap();
                }
            }
            assert_eq!(count.get(), 30_000);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn actual_manifest_tiers_borrow_dynamically_added_predecessors() {
    use crate::input::membership::manifest::{load, schema::manifest::Original, value};

    let mut account = Account::default();
    let mut raw = Slots::new();
    let mut documents = Slots::new();
    let mut originals = Slots::new();
    let mut configured = Slots::new();
    let mut raw = raw.writer().unwrap();
    let mut documents = documents.writer().unwrap();
    let mut originals = originals.writer().unwrap();
    let mut configured = configured.writer().unwrap();
    let mut earlier = None;
    for _ in 0..32 {
        let mut session = Session::new(&mut account);
        let bytes = raw
            .push(
                *b"[workspace.package]\ndescription=\"hello\\nworld\"",
                &mut session,
            )
            .unwrap();
        session.finish(Ok(())).unwrap();
        // Parsing and every retained tier spend the same original account.
        let document =
            document::parse(std::str::from_utf8(bytes).unwrap(), 80, &mut account).unwrap();
        let mut session = Session::new(&mut account);
        let document = documents.push(document, &mut session).unwrap();
        let original = Original::interpret(value::Input::root(document), &mut session).unwrap();
        let original = originals.push(original, &mut session).unwrap();
        if earlier.is_none() {
            earlier = Some(original);
        }
        let loaded = load::configure(
            original,
            std::path::Path::new("/workspace/Cargo.toml"),
            false,
            load::Context {
                features: load::features::Context {
                    allow_features: None,
                    nightly_features_allowed: true,
                    channel: "nightly",
                    is_local: true,
                },
                cargo_lints: false,
                profiles: crate::input::membership::manifest::load::ProfileContext::fixture(),
            },
            &mut session,
        )
        .unwrap();
        configured.push(loaded, &mut session).unwrap();
        session.finish(Ok(())).unwrap();
    }
    use crate::input::membership::manifest::schema::{manifest, package, primitive, workspace};
    let Some(manifest::Value::Workspace(workspace)) =
        earlier.unwrap().get(manifest::Field::Workspace)
    else {
        panic!("workspace");
    };
    let Some(workspace::Value::Package(package)) = workspace.get(workspace::Field::Package) else {
        panic!("defaults");
    };
    let Some(package::Value::Local(primitive::Value::String(text))) =
        package.get(package::Field::Description)
    else {
        panic!("description");
    };
    assert_eq!(text.as_str(), "hello\nworld");
}

#[test]
fn links_spend_the_original_remaining_budget() {
    use crate::input::membership::manifest::tests::admits_one_short;
    use crate::input::membership::Budget;
    use crate::{input, node};

    fn run(items: usize, bytes: usize) -> (bool, input::Usage, crate::exceedance::Record) {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
        allocation.items(9).unwrap();
        allocation.bytes(27).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = Session::new(&mut budget);
        let mut slots = Slots::new();
        let mut writer = slots.writer().unwrap();
        let result = writer
            .push(1u64, &mut session)
            .and_then(|_| writer.push(2, &mut session))
            .and_then(|_| writer.push(3, &mut session));
        let ok = session.finish(result).is_ok();
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (ok, allocation.usage(), allocation.exceedances())
    }
    let bytes = 27 + 2 * Layout::array::<Slot<u64>>(1).unwrap().size();
    let exact = input::Usage { items: 12, bytes };
    assert_eq!(
        run(12, bytes),
        (true, exact, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(run(11, bytes), exact, input::INPUT_ITEMS_RESOURCE, 12);
    admits_one_short(
        run(12, bytes - 1),
        exact,
        input::INPUT_BYTES_RESOURCE,
        bytes,
    );
}
