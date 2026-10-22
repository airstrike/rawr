use super::*;
use crate::input::membership::manifest::{reference, schema::dependency::Inheritable, value};
use std::path::Path;

fn run(
    source: &str,
    kind: dependencies::Kind,
    bindeps: bool,
    json: bool,
) -> Result<String, String> {
    let mut account = reference::Account;
    let document = document::parse(source, 80, &mut account).unwrap();
    let mut session = Session::new(&mut account);
    let member = Inheritable::interpret(
        value::Input::raw(document.table().get("dep").unwrap()),
        &mut session,
    )
    .unwrap();
    let inherit::Progress::Direct(view) =
        inherit::dependency("dep", &member, Path::new("/p"), 2021, &mut session).unwrap()
    else {
        panic!("direct")
    };
    let result = convert(
        "dep",
        |field| view.get(field),
        Context {
            kind,
            bindeps,
            json_target_spec: json,
        },
        &mut session,
    )
    .unwrap();
    let result = result.map(|progress| match progress {
        Progress::Complete(None) => "none".to_owned(),
        Progress::Complete(Some(artifact)) => {
            format!("{:?}/{}", artifact.kinds.as_slice(), artifact.is_lib)
        }
        Progress::Canonical(pending) => format!(
            "pending:{}:{}",
            pending.dependency_name(),
            pending.spelling()
        ),
    });
    session.finish(Ok(())).unwrap();
    result
}

#[test]
fn gate_parse_conflict_duplicates_target_order_is_preserved() {
    for (source,bindeps,expected) in [
        ("dep={artifact=['wrong'],target=''}",false,"`artifact = …` requires `-Z bindeps` (dep)"),
        ("dep={artifact=['bin','bin:x','wrong'],target=''}",true,"'wrong' is not a valid artifact specifier"),
        ("dep={artifact=['bin','bin','bin:x'],target=''}",true,"Cannot specify both 'bin' and 'bin:<name>' binary artifacts, as 'bin' selects all available binaries."),
        ("dep={artifact=['cdylib','cdylib'],target=''}",true,"Found 1 duplicate binary artifact"),
        ("dep={artifact=['staticlib','staticlib','staticlib'],target=''}",true,"Found 2 duplicate binary artifacts"),
        ("dep={artifact=[],target=''}",true,"target was empty"),
        ("dep={artifact=[],target='target'}",true,"`target = \"target\"` in normal- or dev-dependencies has no effect (dep)"),
    ] {assert_eq!(run(source,dependencies::Kind::Normal,bindeps,false).unwrap_err(),expected);}
}

#[test]
fn kinds_preserve_input_order_empty_binary_suffix_and_empty_list() {
    assert_eq!(
        run(
            "dep={artifact=['staticlib','bin:','cdylib'],lib=true}",
            dependencies::Kind::Build,
            true,
            false
        )
        .unwrap(),
        "[Staticlib, SelectedBinary(\"\"), Cdylib]/true"
    );
    assert_eq!(
        run(
            "dep={artifact=[],target='target'}",
            dependencies::Kind::Build,
            true,
            false
        )
        .unwrap(),
        "[]/false"
    );
}

#[test]
fn no_artifact_preserves_lib_presence_before_target() {
    assert_eq!(
        run(
            "dep={lib=false,target=''}",
            dependencies::Kind::Normal,
            false,
            false
        )
        .unwrap_err(),
        "'lib' specifier cannot be used without an 'artifact = …' value (dep)"
    );
    assert_eq!(
        run("dep={target=''}", dependencies::Kind::Normal, false, false).unwrap_err(),
        "'target' specifier cannot be used without an 'artifact = …' value (dep)"
    );
    assert_eq!(
        run("dep={}", dependencies::Kind::Normal, false, false).unwrap(),
        "none"
    );
}

#[test]
fn json_operation_is_pending_with_exact_trimmed_spelling() {
    for name in ["relative.json", "/absolute.json", "../a/../target.json"] {
        assert_eq!(
            run(
                &format!("dep={{artifact='bin',target='  {name}  '}}"),
                dependencies::Kind::Build,
                true,
                true
            )
            .unwrap(),
            format!("pending:dep:{name}")
        );
    }
}

#[test]
fn kind_refusal_is_terminal_at_every_admission() {
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
    let result = kinds(["bin:a", "cdylib", "staticlib"].into_iter(), &mut session);
    assert!(session.finish(result).unwrap().is_ok());
    for at in 0..baseline.calls {
        let mut account = Account {
            refused: Some(at),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let result = kinds(["bin:a", "cdylib", "staticlib"].into_iter(), &mut session);
        assert!(result.is_err());
        assert!(matches!(session.finish(result),Err(Error::Admission(index))if index==at));
        assert_eq!(account.calls, at + 1);
    }
}

#[test]
fn repeated_long_prefix_comparisons_are_admitted_and_refusal_is_terminal() {
    #[derive(Default)]
    struct Account {
        comparisons: usize,
        refuse: bool,
    }
    impl document::Admission for Account {
        type Error = ();
        fn admit(&mut self, request: document::Request) -> Result<(), ()> {
            if matches!(request, document::Request::Items(bytes) if bytes >= 1024) {
                self.comparisons += 1;
                if self.refuse {
                    return Err(());
                }
            }
            Ok(())
        }
    }
    let names = ['d', 'c', 'b', 'a'].map(|suffix| format!("bin:{}{suffix}", "x".repeat(1024)));
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = kinds(names.iter().map(String::as_str), &mut session);
    assert!(session.finish(result).unwrap().is_ok());
    assert!(account.comparisons > names.len());
    let mut account = Account {
        refuse: true,
        ..Account::default()
    };
    let mut session = Session::new(&mut account);
    let result = kinds(names.iter().map(String::as_str), &mut session);
    assert!(result.is_err());
    assert!(kinds(["bin:short"].into_iter(), &mut session).is_err());
    assert!(matches!(session.finish(result), Err(Error::Admission(()))));
    assert_eq!(account.comparisons, 1);
}

#[test]
fn fallible_heap_sort_preserves_wrapper_trace_and_aborts_immediately() {
    let input = [7, 2, 2, 9, 0, 4, 1];
    let trace = std::cell::RefCell::new(Vec::new());
    let mut expected = input;
    storage::sort(&mut expected, |left, right| {
        trace.borrow_mut().push((*left, *right));
        left.cmp(right)
    });
    let trace = trace.into_inner();
    let mut actual = input;
    let mut observed = Vec::new();
    storage::try_sort(&mut actual, |left, right| {
        observed.push((*left, *right));
        Ok::<_, usize>(left.cmp(right))
    })
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(observed, trace);
    for stop in 0..trace.len() {
        let mut values = input;
        let mut observed = Vec::new();
        let result = storage::try_sort(&mut values, |left, right| {
            let index = observed.len();
            observed.push((*left, *right));
            if index == stop {
                Err(index)
            } else {
                Ok(left.cmp(right))
            }
        });
        assert_eq!(result, Err(stop));
        assert_eq!(observed, trace[..=stop]);
    }
}
