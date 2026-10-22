use super::super::super::super::{reference, value, Error, Stop};
use super::*;
use std::path::Path;

#[test]
fn local_absent_and_empty_tables_do_not_request_workspace() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let Progress::Complete(absent) = normalize(None, &mut session).unwrap() else {
        panic!("absent table must not load a workspace");
    };
    assert!(absent.table().is_none());
    for source in ["member={}", "member={rust={unused='warn'}}"] {
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let original = lints::Inherited::interpret(
            value::Input::raw(document.table().get("member").unwrap()),
            &mut session,
        )
        .unwrap();
        let Progress::Complete(done) = normalize(Some(&original), &mut session).unwrap() else {
            panic!("local table must not load a workspace");
        };
        assert!(std::ptr::eq(done.table().unwrap(), original.table()));
    }
}

#[test]
fn overrides_reject_before_workspace_and_missing_defaults_remain_errors() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    for (source, expected) in [
        (
            "member={workspace=true,rust={unused='warn'}}",
            Tag::LintsOverride,
        ),
        ("member={workspace=true}", Tag::LintsInheritedMissing),
    ] {
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let original = lints::Inherited::interpret(
            value::Input::raw(document.table().get("member").unwrap()),
            &mut session,
        )
        .unwrap();
        let result = match normalize(Some(&original), &mut session) {
            Ok(Progress::Workspace(request)) => request.resume_declared(
                roots::Root {
                    directory: Path::new("/unused"),
                    declaration: None,
                },
                &mut session,
            ),
            Ok(Progress::Complete(_)) => panic!("inheritance must not complete locally"),
            Err(error) => Err(error),
        };
        assert!(matches!(result, Err(Stop::Rejected(Diagnostic { tag, .. })) if tag == expected));
    }
}

#[test]
fn inherited_tables_are_borrowed_and_prior_refusal_prevents_resume() {
    for defaults in ["{}", "{rust={unused='deny'}}"] {
        let source = format!("member={{workspace=true}}\nworkspace={{lints={defaults}}}");
        let document = document::parse(&source, 80, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let original = lints::Inherited::interpret(
            value::Input::raw(document.table().get("member").unwrap()),
            &mut session,
        )
        .unwrap();
        let declaration = workspace::Workspace::interpret(
            value::Input::raw(document.table().get("workspace").unwrap()),
            &mut session,
        )
        .unwrap();
        let root = roots::Root {
            directory: Path::new("/unused"),
            declaration: Some(&declaration),
        };
        let Progress::Workspace(request) = normalize(Some(&original), &mut session).unwrap() else {
            panic!("workspace")
        };
        let done = request.resume_declared(root, &mut session).unwrap();
        let Some(workspace::Value::Lints(expected)) = declaration.get(workspace::Field::Lints)
        else {
            panic!("defaults")
        };
        assert!(std::ptr::eq(done.table().unwrap(), expected));
        let Progress::Workspace(request) = normalize(Some(&original), &mut session).unwrap() else {
            panic!("workspace")
        };
        let failed: Step<()> = session.fail(Error::Overflow);
        assert!(matches!(failed, Err(Stop::Refused)));
        assert!(matches!(
            request.resume_declared(root, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Overflow)));
    }
}
