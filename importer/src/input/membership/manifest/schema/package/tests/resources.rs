use rawr_toml::document;

use super::super::super::super::{session, storage, Error};
use super::{parse, Defaults, Package};

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = &'static str;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err("original refusal")
        } else {
            Ok(())
        }
    }
}

const FIXTURE: &str = "value={name='package',version=' 1.2.3-pre+build ',authors=['a','b','c','d','e'],build=['a.rs','b.rs'],metabuild='build',readme=false,publish=['registry'],edition={workspace=true,unknown={a=[1]}},other=[2],metadata={x=[1,true,'s']}}";

#[test]
fn each_refusal_stops_package_allocations_and_cannot_be_discarded() {
    let document = parse(FIXTURE);
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let result = Package::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    assert!(full.requests.len() > 40);
    for refusal in 0..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        let result = Package::interpret(super::input(&document), &mut session);
        assert!(result.is_err());
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Admission("original refusal"))
        ));
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
fn default_badge_growth_refusal_precedes_every_owned_layout() {
    let document = parse("value={version='1.2.3',badges={z={e='e',d='d',c='c',b='b',a='a'},y={s='y'},x={s='x'},w={s='w'},v={s='v'}},include=['a','b','c','d','e'],rust-version='1.96'}");
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let result = Defaults::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    for refusal in 0..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        let result = Defaults::interpret(super::input(&document), &mut session);
        assert!(matches!(
            session.finish(result),
            Err(Error::Admission("original refusal"))
        ));
        assert_eq!(account.requests, full.requests[..=refusal]);
        let allocations = full.requests[..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
    }
}

#[test]
fn package_metadata_and_ignored_descendants_have_bounded_stack_teardown() {
    for field in ["metadata", "unknown"] {
        let mut source = format!("value={{{field}={{");
        for _ in 0..78 {
            for _ in 0..78 {
                source.push_str("a.");
            }
            source.push_str("a={");
        }
        source.push_str("leaf=1");
        for _ in 0..80 {
            source.push('}');
        }
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(super::input(&document), &mut session);
        let package = session.finish(result).unwrap();
        if field == "metadata" {
            let Some(super::super::Value::Metadata(value)) =
                package.get(super::super::Field::Metadata)
            else {
                panic!("metadata")
            };
            assert!(value.nodes.len() > 6_000);
        } else {
            assert_eq!(package.ignored().len(), 1);
        }
        drop(package);
        drop(document);
    }
}
