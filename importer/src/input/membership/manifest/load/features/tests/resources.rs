use rawr_toml::document;

use super::{Feature, Features, CONTEXT};
use crate::input::membership::manifest::{
    load::Warnings, reference, schema::manifest, session, storage, value, Error, Stop,
};

#[derive(Default)]
struct Meter {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Meter {
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
fn borrowed_activation_needs_no_heap_or_copied_input_layout() {
    let document = document::parse(
        "cargo-features=['metabuild','open-namespaces','profile-rustflags']",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let mut account = Meter::default();
    let mut session = session::Session::new(&mut account);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let before = storage::ALLOCATIONS.with(|count| count.get());
    let features = Features::interpret(&original, CONTEXT, &mut warnings, &mut session).unwrap();
    assert_eq!(features.activated(&mut session).unwrap().len(), 3);
    assert!(features
        .is_enabled(Feature::Metabuild, &mut session)
        .unwrap());
    features
        .require(Feature::ProfileRustflags, &mut session)
        .unwrap();
    assert_eq!(before, storage::ALLOCATIONS.with(|count| count.get()));
    assert!(warnings.as_slice().is_empty());
    session.finish(Ok(())).unwrap();
    assert_eq!(
        &account.requests[account.requests.len() - 4..],
        &[
            document::Request::String(9),
            document::Request::String(15),
            document::Request::String(17),
            document::Request::Items(1),
        ]
    );
}

#[test]
fn every_constructor_admission_refusal_is_latched_and_preserves_spent_work() {
    // Five warnings cross the admitted Buffer capacity and exercise full
    // replacement ownership, then a semantic query creates its bounded error.
    let document = document::parse(
        "cargo-features=['edition','strip','rust-version','resolver','edition2024']",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let prior = vec![document::Request::Items(7), document::Request::String(31)];
    let mut full = Meter {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = session::Session::new(&mut full);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let features = Features::interpret(&original, CONTEXT, &mut warnings, &mut session).unwrap();
    assert!(matches!(
        features.require(Feature::Metabuild, &mut session),
        Err(Stop::Rejected(_))
    ));
    session.finish(Ok(())).unwrap();

    for refusal in prior.len()..full.requests.len() {
        let mut meter = Meter {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut meter);
        let mut warnings = Warnings::new();
        let result = manifest::Original::interpret(value::Input::root(&document), &mut session)
            .and_then(|original| {
                let features =
                    Features::interpret(&original, CONTEXT, &mut warnings, &mut session)?;
                let result = features.require(Feature::Metabuild, &mut session);
                assert!(matches!(
                    features.is_enabled(Feature::Edition, &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(
                    features.activated(&mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(
                    features.require(Feature::Edition, &mut session),
                    Err(Stop::Refused)
                ));
                result
            });
        assert!(matches!(result, Err(Stop::Refused)), "{refusal}");
        let before = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            warnings.push(&["must not allocate"], &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(before, storage::ALLOCATIONS.with(|count| count.get()));
        assert!(matches!(session.finish(result), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(meter.requests, full.requests[..=refusal], "{refusal}");
    }
}

#[test]
fn diagnostic_refusal_is_terminal_instead_of_a_consumable_feature_rejection() {
    let document =
        document::parse("cargo-features=['unknown']", 80, &mut reference::Account).unwrap();
    let mut initial = reference::Account;
    let mut session = session::Session::new(&mut initial);
    let original =
        manifest::Original::interpret(value::Input::root(&document), &mut session).unwrap();
    session.finish(Ok(())).unwrap();
    // Isolate the actual borrowed constructor's two admissions: source scalar,
    // then fixed error item. This account deliberately refuses that error item.
    let mut meter = Meter {
        requests: Vec::new(),
        refusal: Some(1),
    };
    let mut session = session::Session::new(&mut meter);
    let mut warnings = Warnings::new();
    assert!(matches!(
        Features::interpret(&original, CONTEXT, &mut warnings, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        Features::interpret(&original, CONTEXT, &mut warnings, &mut session),
        Err(Stop::Refused)
    ));
    assert!(warnings.as_slice().is_empty());
    assert!(matches!(session.finish(Ok(())), Err(Error::Admission(1))));
    assert_eq!(
        meter.requests,
        [document::Request::String(7), document::Request::Items(1)]
    );
}
