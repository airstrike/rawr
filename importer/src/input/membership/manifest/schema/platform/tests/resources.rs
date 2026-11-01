use rawr_toml::document;

use super::super::super::super::{session, storage, Error, Stop};
use super::{parse, Platform};

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
            Err("platform refusal")
        } else {
            Ok(())
        }
    }
}

const FIXTURE: &str = "value={dependencies={z={workspace=true,features=['a','b','c','d','e'],extra={x=[1]}},a={version='1',extra=[true]}},build-dependencies={a='2'},build_dependencies={a={path='../a'}},dev-dependencies={a='3'},dev_dependencies={a={workspace=true}},unknown={nested=[1,2]}}";

fn prior() -> Account {
    let mut account = Account::default();
    document::Admission::admit(&mut account, document::Request::Items(11)).unwrap();
    document::Admission::admit(&mut account, document::Request::String(7)).unwrap();
    account
}

#[test]
fn every_refusal_stops_all_alias_visitors_before_further_admission_or_allocation() {
    let document = parse(FIXTURE);
    let mut full = prior();
    let mut session = session::Session::new(&mut full);
    let result = Platform::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    assert!(full.requests.len() > 60);
    for refusal in 2..full.requests.len() {
        let mut account = prior();
        account.refusal = Some(refusal);
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        assert!(matches!(
            Platform::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            Platform::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Admission("platform refusal"))
        ));
        assert_eq!(
            account.requests,
            full.requests[..=refusal],
            "request {refusal}"
        );
        let admitted = full.requests[2..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(allocations, admitted, "request {refusal}");
    }
}
