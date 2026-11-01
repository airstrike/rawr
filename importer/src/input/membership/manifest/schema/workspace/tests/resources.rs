use rawr_toml::document;

use super::super::super::super::{session, storage, Error, Stop};
use super::{parse, Workspace};

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
            Err("workspace refusal")
        } else {
            Ok(())
        }
    }
}

const FIXTURE: &str = "value={members=['a','b','c','d','e'],exclude=['tmp'],default-members=['a'],resolver='3',metadata={z=[1,-0.0],a={text='decoded text'}},package={version='1.2.3+build',unknown={x=[true]}},dependencies={z={path='../z',extra=[1,2]},a='1'},lints={rust={unused={level='warn',priority=-1,extra=[true]}}},outer=true}";

fn prior() -> Account {
    let mut account = Account::default();
    document::Admission::admit(&mut account, document::Request::Items(11)).unwrap();
    document::Admission::admit(&mut account, document::Request::String(7)).unwrap();
    account
}

#[test]
fn every_refusal_preserves_the_original_prefix_and_precedes_storage() {
    let document = parse(FIXTURE);
    let mut full = prior();
    let mut session = session::Session::new(&mut full);
    let result = Workspace::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    assert!(full.requests.len() > 60);
    for refusal in 2..full.requests.len() {
        let mut account = prior();
        account.refusal = Some(refusal);
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        assert!(matches!(
            Workspace::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            Workspace::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Admission("workspace refusal"))
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
