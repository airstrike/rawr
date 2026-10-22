use rawr_toml::document;

use super::super::{session, Diagnostic, Error, Source, Span, Stop, Tag};
use super::Account;

#[test]
fn classified_source_failure_latches_before_any_further_visit_or_output() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    session.items(7).unwrap();
    let span = Some(Span { start: 3, end: 29 });
    assert!(matches!(
        session.source_reparse::<()>(span),
        Err(Stop::Refused)
    ));
    assert!(matches!(session.string(0), Err(Stop::Refused)));
    assert!(matches!(
        session.reject::<()>(Diagnostic::at(Tag::Type, None)),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.source_reparse::<()>(None),
        Err(Stop::Refused)
    ));
    assert!(
        matches!(session.finish(Ok(())), Err(Error::Source { cause: Source::SpecReparse, span: found }) if found == span)
    );
    assert_eq!(
        account.requests,
        [document::Request::Items(7), document::Request::Items(1)]
    );
}

#[test]
fn source_diagnostic_admission_and_prior_refusal_retain_first_resource_cause() {
    for refusal in [0, 1] {
        let mut account = Account {
            requests: Vec::new(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        let _ = session.items(7);
        assert!(matches!(
            session.source_reparse::<()>(None),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.source_reparse::<()>(None),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Admission("original refusal"))
        ));
        assert_eq!(account.requests.len(), refusal + 1);
    }
}

#[test]
fn ordinary_semantic_rejection_remains_distinct_from_latched_source_failure() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert!(matches!(
        session.reject::<()>(Diagnostic::at(Tag::Type, None)),
        Err(Stop::Rejected(_))
    ));
    session.check().unwrap();
    session.items(4).unwrap();
    assert!(matches!(
        session.source_reparse::<()>(None),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Source {
            cause: Source::SpecReparse,
            span: None
        })
    ));
    assert_eq!(
        account.requests,
        [
            document::Request::Items(1),
            document::Request::Items(4),
            document::Request::Items(1)
        ]
    );
}
