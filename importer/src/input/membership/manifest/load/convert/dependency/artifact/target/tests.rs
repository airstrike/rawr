use super::*;
use crate::input::membership::manifest::{reference, Stop};

#[test]
fn compile_target_does_not_apply_artifact_assume_literal() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    for spelling in ["target", " target "] {
        let Ok(Ok(Progress::Ready(Target::Tuple(value)))) = compile(spelling, false, &mut session)
        else {
            panic!("tuple")
        };
        assert_eq!(value, "target");
    }
}

#[test]
fn literal_assume_is_before_trim_and_json_gate_before_nul() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    assert!(matches!(
        parse("target", false, &mut session),
        Ok(Ok(Progress::Ready(Target::Assume)))
    ));
    assert!(
        matches!(parse(" target ",false,&mut session),Ok(Ok(Progress::Ready(Target::Tuple(value))))if value=="target")
    );
    let Ok(Err(error)) = parse("x\0.json", false, &mut session) else {
        panic!("gate")
    };
    assert!(error.starts_with("`.json` target specs require"));
    let Ok(Err(error)) = parse("x\0.json", true, &mut session) else {
        panic!("nul")
    };
    assert_eq!(
        error,
        "target path `x\0.json` is not a valid file: file name contained an unexpected NUL byte"
    );
    session.finish(Ok(())).unwrap();
}

#[test]
fn canonical_success_needs_no_json_suffix_and_retains_full_path_then_stem() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let Ok(Ok(Progress::Canonical(request))) = parse("alias.json", true, &mut session) else {
        panic!("request")
    };
    let Ok(Ok(Target::Json { path, short })) =
        request.canonical(b"/real/target.data", &mut session)
    else {
        panic!("canonical")
    };
    assert_eq!(path, "/real/target.data");
    assert_eq!(short, "target");
    session.finish(Ok(())).unwrap();
}

#[test]
fn unicode_error_precedes_stem_and_root_is_exact_source_cause() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let Ok(Ok(Progress::Canonical(request))) = parse("alias.json", true, &mut session) else {
        panic!("request")
    };
    let Ok(Err(error)) = request.canonical(b"/bad/\xff", &mut session) else {
        panic!("unicode")
    };
    assert_eq!(error, "target path `alias.json` is not valid unicode");
    let Ok(Ok(Progress::Canonical(request))) = parse("alias.json", true, &mut session) else {
        panic!("request")
    };
    let result = request.canonical(b"/", &mut session);
    assert!(matches!(result, Err(Stop::Refused)));
    assert!(parse("target", false, &mut session).is_err());
    assert!(matches!(
        session.finish(result),
        Err(Error::Source {
            cause: Source::CompileTargetStem,
            span: None
        })
    ));
}

#[test]
fn cause_admission_refusal_wins_after_full_path_ownership() {
    #[derive(Default)]
    struct Account {
        calls: usize,
        refused: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = usize;
        fn admit(&mut self, _: document::Request) -> Result<(), usize> {
            let at = self.calls;
            self.calls += 1;
            if self.refused == Some(at) {
                Err(at)
            } else {
                Ok(())
            }
        }
    }
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let request = Request {
        spelling: "alias.json",
        _input: None,
    };
    let result = request.canonical(b"/", &mut session);
    assert!(session.finish(result).is_err());
    for at in 0..baseline.calls {
        let mut account = Account {
            refused: Some(at),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let result = Request {
            spelling: "alias.json",
            _input: None,
        }
        .canonical(b"/", &mut session);
        assert!(matches!(session.finish(result),Err(Error::Admission(index))if index==at));
        assert_eq!(account.calls, at + 1);
    }
}

#[test]
fn long_nul_input_allocation_precedes_diagnostic_and_refusal_is_terminal() {
    #[derive(Default)]
    struct Account {
        requests: Vec<document::Request>,
        refused: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = usize;
        fn admit(&mut self, request: document::Request) -> Result<(), usize> {
            let index = self.requests.len();
            self.requests.push(request);
            if self.refused == Some(index) {
                Err(index)
            } else {
                Ok(())
            }
        }
    }
    let name = format!("{}\0.json", "a".repeat(384));
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = parse(&name, true, &mut session);
    let Ok(Err(error)) = session.finish(result) else {
        panic!("local NUL")
    };
    assert_eq!(
        error,
        format!(
            "target path `{name}` is not a valid file: file name contained an unexpected NUL byte"
        )
    );
    let allocations: Vec<_> = account
        .requests
        .iter()
        .filter_map(|request| match request {
            document::Request::Allocation { bytes, .. } => Some(*bytes),
            _ => None,
        })
        .collect();
    assert_eq!(allocations, [name.len() + 1, error.len()]);
    for at in 0..account.requests.len() {
        let mut account = Account {
            refused: Some(at),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let result = parse(&name, true, &mut session);
        assert!(result.is_err());
        assert!(parse("target", true, &mut session).is_err());
        assert!(matches!(session.finish(result), Err(Error::Admission(index)) if index == at));
        assert_eq!(account.requests.len(), at + 1);
    }
}
