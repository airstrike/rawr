use super::*;
use crate::input::membership::manifest::reference;

#[test]
fn exact_replacement_requirement_owns_pre_and_omits_build() {
    use crate::input::membership::manifest::{spec::Spec, Stop};
    let mut source = reference::Account;
    let mut session = Session::new(&mut source);
    let spec = Spec::parse("pkg@1.2.3-alpha.1+build.7", None, &mut session).unwrap();
    let full = spec.full_version(&mut session).unwrap().unwrap();
    drop(spec);
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = Requirement::exact(&full, &mut session);
    let requirement = session.finish(result).unwrap();
    let requests = account.requests.len();
    let [value] = requirement.comparators() else {
        panic!("one comparator")
    };
    assert!(matches!(value.op, Op::Exact));
    assert_eq!(
        (value.major, value.minor, value.patch, value.pre()),
        (1, Some(2), Some(3), "alpha.1")
    );
    assert_ne!(value.pre().as_ptr(), full.pre().as_ptr());
    let allocations: Vec<_> = account
        .requests
        .iter()
        .filter_map(|request| match request {
            document::Request::Allocation { bytes, .. } => Some(*bytes),
            _ => None,
        })
        .collect();
    assert_eq!(
        allocations,
        [std::mem::size_of::<Comparator>(), "alpha.1".len()]
    );
    for failure in 0..requests {
        let mut account = Account {
            refuse: Some(failure),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            Requirement::exact(&full, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            Requirement::exact(&full, &mut session),
            Err(Stop::Refused)
        ));
        assert!(session.finish(Ok(())).is_err());
        assert_eq!(account.requests.len(), failure + 1);
    }
    drop(full);
    assert_eq!(requirement.comparators()[0].pre(), "alpha.1");
}

#[test]
fn complete_retained_comparators_match_pinned_semver() {
    let mut cases = [
        "*",
        "x",
        "X",
        "  * ",
        "1",
        "^1",
        ">=1.2.3-alpha.1, <2.0.0-beta",
        "=1.2.3+ignored",
        ">1,<=2,~3.4,^4.5.6",
        "1.*",
        "^1.*",
        "1.2.*",
        "1.2.3-long-prerelease+long-build-metadata",
    ]
    .map(str::to_owned)
    .to_vec();
    cases.push(vec![">=1.2.3-abcdefghijk"; 32].join(","));
    for text in cases {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let result = Requirement::parse(&text, &mut session);
        let actual = session.finish(result).unwrap().unwrap();
        let expected = semver::VersionReq::parse(&text).unwrap();
        assert_eq!(
            actual.comparators().len(),
            expected.comparators.len(),
            "{text}"
        );
        for (actual, expected) in actual.comparators().iter().zip(&expected.comparators) {
            assert_eq!(
                format!("{:?}", actual.op),
                format!("{:?}", expected.op),
                "{text}"
            );
            assert_eq!(
                (actual.major, actual.minor, actual.patch, actual.pre()),
                (
                    expected.major,
                    expected.minor,
                    expected.patch,
                    expected.pre.as_str()
                ),
                "{text}"
            );
        }
    }
}

#[test]
fn rejection_categories_match_existing_shared_grammar() {
    let mut cases = [
        "",
        " ",
        "*,1",
        "1,*",
        "1,x,2",
        "1.x.2",
        "1.2.3-01",
        "1.2.3+",
        "1.2.3-abc,foo",
        "1.2.3\t",
        "1.2.3,",
        "1.2.3|2",
        "1.2.3+é",
    ]
    .map(str::to_owned)
    .to_vec();
    cases.push(vec!["1"; 33].join(","));
    for text in cases {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let result = Requirement::parse(&text, &mut session);
        let actual = session.finish(result).unwrap().unwrap_err();
        let expected = partial::capture(&text, |_| Ok::<_, std::convert::Infallible>(()));
        assert!(matches!(expected,Err(partial::Failure::Grammar(error)) if error==actual));
        assert!(semver::VersionReq::parse(&text).is_err(), "{text}");
    }
}

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refuse: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

#[test]
fn identifiers_are_owned_before_later_failure_but_final_vector_is_not() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = Requirement::parse("1.2.3-abcdefghijk+abcdefghijklmnop,?", &mut session);
    assert!(session.finish(result).unwrap().is_err());
    let allocations = account
        .requests
        .iter()
        .filter_map(|request| match request {
            document::Request::Allocation { bytes, alignment } => Some((*bytes, *alignment)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(allocations, [(11, 1), (16, 1)]);
}

#[test]
fn grammar_error_cannot_hide_earlier_identifier_refusal() {
    let mut account = Account {
        refuse: Some(3),
        ..Account::default()
    };
    let mut session = Session::new(&mut account);
    let result = Requirement::parse("1.2.3-abcdefghijk+?", &mut session);
    assert!(matches!(session.finish(result), Err(Error::Admission(3))));
    assert_eq!(account.requests.len(), 4);
}

#[test]
fn every_original_admission_refusal_remains_terminal() {
    for text in [
        "*",
        "1.2.3-abcdefghijk+abcdefghijklmnop,>=2.0.0-z",
        "1.2.3-abcdefghijk,?",
    ] {
        let mut baseline = Account::default();
        let mut session = Session::new(&mut baseline);
        let result = Requirement::parse(text, &mut session);
        assert!(session.finish(result).is_ok());
        for refusal in 0..baseline.requests.len() {
            let mut account = Account {
                refuse: Some(refusal),
                ..Account::default()
            };
            let mut session = Session::new(&mut account);
            let result = Requirement::parse(text, &mut session);
            assert!(result.is_err());
            assert!(Requirement::parse("*", &mut session).is_err());
            assert!(
                matches!(session.finish(result),Err(Error::Admission(index)) if index==refusal)
            );
            assert_eq!(account.requests.len(), refusal + 1);
        }
    }
}
