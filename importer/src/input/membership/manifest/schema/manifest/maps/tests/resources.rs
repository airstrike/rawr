use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Error, Stop};

use super::{input, parse, Family};

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

const CASES: [(Family, &str); 5] = [
    (Family::Features, "value={z=['one','two','three','four','five'],a=[],b=['x'],c=['y'],d=['z']}"),
    (Family::Platforms, "value={z={dependencies={a={workspace=true,extra={x=[1]}}},unknown=[1]},a={other=[false]},b={},c={},d={}}"),
    (Family::Patch, "value={z={a={path='../a',features=['one','two','three','four','five'],extra={x=[1]}}},a={b='2'},b={},c={},d={}}"),
    (Family::Replace, "value={z={version='1',extra=[1,2,3]},a='2',b={path='../x'},c='3',d='4'}"),
    (Family::Targets, "value=[{name='a',unknown=[1]},{name='b','other.field'=[false]},{name='c'},{name='d'},{name='e',required-features=['one','two','three','four','five']}]"),
];

fn prior() -> Account {
    let mut account = Account::default();
    document::Admission::admit(&mut account, document::Request::Items(11)).unwrap();
    document::Admission::admit(&mut account, document::Request::String(7)).unwrap();
    account
}

#[test]
fn every_family_refuses_before_the_unadmitted_allocation_and_stays_terminal() {
    for (family, source) in CASES.into_iter().chain([
        (Family::Features, "value={'dep:invalid'=1e9999}"),
        (
            Family::Patch,
            "value={source={a={extra=9223372036854775808}}}",
        ),
        (Family::Targets, "value=[{name='a',unknown=1e9999}]"),
    ]) {
        let document = parse(source);
        let mut full = prior();
        let mut session = session::Session::new(&mut full);
        let result = family.run(input(&document), &mut session);
        let result = session.finish(result);
        assert!(
            matches!(result, Ok(_) | Err(Error::Invalid(_))),
            "{family:?}"
        );
        for refusal in 2..full.requests.len() {
            let mut account = prior();
            account.refusal = Some(refusal);
            let mut session = session::Session::new(&mut account);
            storage::ALLOCATIONS.with(|count| count.set(0));
            assert!(
                matches!(
                    family.run(input(&document), &mut session),
                    Err(Stop::Refused)
                ),
                "{family:?}: request {refusal}"
            );
            let allocations = storage::ALLOCATIONS.with(|count| count.get());
            assert!(matches!(
                family.run(input(&document), &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(allocations, storage::ALLOCATIONS.with(|count| count.get()));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
            );
            assert_eq!(
                account.requests,
                full.requests[..=refusal],
                "{family:?}: request {refusal}"
            );
            let admitted = full.requests[2..refusal]
                .iter()
                .filter(|request| matches!(request, document::Request::Allocation { .. }))
                .count();
            assert_eq!(allocations, admitted, "{family:?}: request {refusal}");
        }
    }
}

#[test]
fn wide_reverse_maps_complete_conversion_and_keep_sorted_payloads() {
    let mut source = String::from("value={");
    for index in (0..1025).rev() {
        if index != 1024 {
            source.push(',');
        }
        source.push_str(&format!("key{index:04}='value{index:04}'"));
    }
    source.push('}');
    let document = parse(&source);
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let output = super::Replace::interpret(input(&document), &mut session).unwrap();
    assert_eq!(output.entries().len(), 1025);
    for (index, entry) in output.entries().enumerate() {
        assert_eq!(entry.origin().text.as_str(), format!("key{index:04}"));
        let crate::input::membership::manifest::schema::dependency::direct::Kind::Simple(text) =
            entry.value().kind()
        else {
            panic!("simple dependency");
        };
        assert_eq!(text.as_str(), format!("value{index:04}"));
    }
    drop(output);
    session.finish(Ok(())).unwrap();
}
