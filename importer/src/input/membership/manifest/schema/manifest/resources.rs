use rawr_toml::document;

use super::{Field, Original, Value};
use crate::input::membership::manifest::{reference, session, storage, value, Error, Stop};
use crate::{cargo, input, node};

const SOURCE: &str = r#"
cargo-features=['profile-rustflags']
unknown={nested=[1,2]}
package={name='pkg',version='1.2.3',other=false}
project={name='legacy',mystery='x'}
bin=[{name='first',ignored=true},{name='second',other=1}]
features={default=['first'],first=[]}
dependencies={serde={version='1',extra={not_ignored=true}}}
target={unix={ignored='x',dependencies={local={path='../local'}}}}
workspace={members=['member'],package={other=false}}
profile={dev={opt-level=1,other='x',package={'foo@1.2'={debug=2,extra=false}},build-override={lto=false,mystery='x'}}}
"#;

#[derive(Default)]
struct Meter {
    requests: Vec<document::Request>,
    refuse: Option<usize>,
}

impl document::Admission for Meter {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn prior() -> Meter {
    Meter {
        requests: vec![
            document::Request::Items(7),
            document::Request::Allocation {
                bytes: 31,
                alignment: 1,
            },
        ],
        refuse: None,
    }
}

#[test]
fn full_original_keeps_both_package_aliases_and_exact_unused_paths() {
    let document = document::parse(SOURCE, 80, &mut reference::Account).unwrap();
    let mut account = prior();
    let mut session = session::Session::new(&mut account);
    let result = Original::interpret(value::Input::root(&document), &mut session);
    let output = session.finish(result).unwrap();
    assert!(matches!(
        output.get(Field::Package),
        Some(Value::Package(_))
    ));
    assert!(matches!(
        output.get(Field::Project),
        Some(Value::Package(_))
    ));
    let actual: Vec<_> = output.unused().unique().collect();
    assert_eq!(
        actual,
        [
            "bin.0.ignored",
            "bin.1.other",
            "package.other",
            "profile.dev.build-override.mystery",
            "profile.dev.other",
            "profile.dev.package.foo@1.2.extra",
            "project.mystery",
            "target.unix.ignored",
            "unknown",
            "workspace.package.other",
        ]
    );
}

#[test]
fn every_original_refusal_retains_first_cause_and_blocks_all_later_work() {
    for source in [
        SOURCE,
        "unknown=1e9999",
        "package={name='ok',cargo-features={bad=1e9999}}",
    ] {
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let mut full = prior();
        let mut session = session::Session::new(&mut full);
        let result = Original::interpret(value::Input::root(&document), &mut session);
        assert!(matches!(
            session.finish(result),
            Ok(_) | Err(Error::Invalid(_))
        ));
        for index in 2..full.requests.len() {
            let mut account = prior();
            account.refuse = Some(index);
            let mut session = session::Session::new(&mut account);
            assert!(matches!(
                Original::interpret(value::Input::root(&document), &mut session),
                Err(Stop::Refused)
            ));
            let allocations = storage::ALLOCATIONS.with(|value| value.get());
            assert!(matches!(
                Original::interpret(value::Input::root(&document), &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(allocations, storage::ALLOCATIONS.with(|value| value.get()));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(actual)) if actual == index)
            );
            assert_eq!(account.requests, full.requests[..=index]);
        }
    }
}

#[test]
fn whole_original_preserves_original_item_and_byte_floors() {
    use crate::input::membership::manifest::tests::one_short;
    let document = document::parse(SOURCE, 80, &mut reference::Account).unwrap();
    let mut meter = prior();
    let mut session = session::Session::new(&mut meter);
    let result = Original::interpret(value::Input::root(&document), &mut session);
    assert!(session.finish(result).is_ok());
    let (mut items, mut bytes) = (0, 0);
    for request in &meter.requests {
        match request {
            document::Request::Items(value) => items += value,
            document::Request::Allocation { bytes: value, .. } => bytes += value,
            _ => {}
        }
    }
    for (item_floor, byte_floor) in [(0, 0), (1, 0), (0, 1)] {
        let limits = cargo::Limits::standard();
        let mut allocation = input::Allocation::new(&limits);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        allocation.item_limit = items - item_floor;
        allocation.byte_limit = bytes - byte_floor;
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .unwrap();
        let mut budget = input::membership::Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let result = Original::interpret(value::Input::root(&document), &mut session);
        let result = session.finish(result);
        // A one-short item or byte target still admits the whole original
        // and records only that axis at the exact total; nothing latches.
        assert!(result.is_ok());
        assert_eq!(allocation.usage(), input::Usage { items, bytes });
        assert!(!allocation.refused);
        let exceeded = allocation.exceedances();
        match (item_floor, byte_floor) {
            (0, 0) => assert!(!exceeded.is_exceeded()),
            (1, 0) => assert!(one_short(&exceeded, input::INPUT_ITEMS_RESOURCE, items)),
            _ => assert!(one_short(&exceeded, input::INPUT_BYTES_RESOURCE, bytes)),
        }
    }
}
