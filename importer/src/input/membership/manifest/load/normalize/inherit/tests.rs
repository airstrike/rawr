use super::super::super::super::{reference, Error};
use super::*;

mod resources;

macro_rules! inputs {
    ($source:expr, $edition:expr, |$progress:ident, $root:ident, $warnings:ident, $session:ident| $body:block) => {{
        let document = document::parse($source, 80, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut $session = Session::new(&mut account);
        let member = Inheritable::interpret(
            value::Input::raw(document.table().get("member").unwrap()),
            &mut $session,
        )
        .unwrap();
        let declaration = workspace::Workspace::interpret(
            value::Input::raw(document.table().get("workspace").unwrap()),
            &mut $session,
        )
        .unwrap();
        let $root = roots::Root {
            directory: Path::new("/ws"),
            declaration: Some(&declaration),
        };
        #[allow(unused_mut, reason = "some expansions only inspect the warnings")]
        let mut $warnings = Warnings::new();
        let $progress = dependency(
            "dep",
            &member,
            Path::new("/ws/member"),
            $edition,
            &mut $session,
        )
        .unwrap();
        $body
    }};
}

#[test]
fn direct_preserves_simple_and_detailed_without_workspace_lookup() {
    inputs!(
        "member='1'\n[workspace]",
        2024,
        |progress, root, warnings, session| {
            let Progress::Direct(view) = progress else {
                panic!("direct")
            };
            assert_eq!(view.simple(), Some("1"));
            assert!(view.get(Field::Version).is_none());
            let _ = (root, warnings, session);
        }
    );
    inputs!(
        "member={path='../dep',default-features=false,optional=true,unknown='yes'}\n[workspace]",
        2024,
        |progress, root, warnings, session| {
            let Progress::Direct(view) = progress else {
                panic!("direct")
            };
            assert!(
                matches!(view.get(Field::Path), Some(Value::Original(primitive::Value::String(path))) if path.as_str()=="../dep")
            );
            assert_eq!(view.extras().count(), 1);
            let _ = (root, warnings, session);
        }
    );
}

#[test]
fn inheritance_merges_ordered_features_and_member_flags_only() {
    inputs!("member={workspace=true,features=['same','member'],default-features=true,optional=false,public=true,ignored='member'}\n[workspace.dependencies]\ndep={version='1',path='dep',features=['ws','same'],default_features=false,unknown='workspace'}", 2021, |progress, root, warnings, session| {
        let Progress::Workspace(request) = progress else { panic!("workspace") };
        let view = request.resume_declared(root, &mut warnings, &mut session).unwrap();
        assert!(view.simple().is_none());
        assert!(matches!(view.get(Field::Path), Some(Value::Text("../dep"))));
        let Some(Value::Features(features)) = view.get(Field::Features) else { panic!("features") };
        assert_eq!(features.iter().collect::<Vec<_>>(), ["ws", "same", "same", "member"]);
        assert!(matches!(view.get(Field::DefaultFeatures), Some(Value::Boolean(true))));
        assert!(matches!(view.get(Field::DefaultFeatures2), Some(Value::Original(primitive::Value::Boolean(false)))));
        assert!(matches!(view.get(Field::Optional), Some(Value::Original(primitive::Value::Boolean(false)))));
        assert!(matches!(view.get(Field::Public), Some(Value::Original(primitive::Value::Boolean(true)))));
        assert_eq!(view.extras().map(|(key, _)| key.text.as_str()).collect::<Vec<_>>(), ["unknown"]);
        assert!(warnings.as_slice().is_empty());
    });
}

#[test]
fn path_base_defers_rebase_and_empty_features_remain_present() {
    inputs!("member={workspace=true,features=[]}\n[workspace.dependencies]\ndep={path='dep',base='sdk',default-features=false}", 2024, |progress, root, warnings, session| {
        let Progress::Workspace(request) = progress else { panic!("workspace") };
        let view = request.resume_declared(root, &mut warnings, &mut session).unwrap();
        assert!(matches!(view.get(Field::Path), Some(Value::Original(primitive::Value::String(path))) if path.as_str()=="dep"));
        assert!(view.get(Field::Base).is_some());
        assert!(view.get(Field::Optional).is_none());
        assert!(view.get(Field::Public).is_none());
        let Some(Value::Features(features)) = view.get(Field::Features) else { panic!("present empty") };
        assert_eq!(features.iter().count(), 0);
    });
}

#[test]
fn simple_workspace_promotes_to_detailed_and_warns_before_2024() {
    inputs!(
        "member={workspace=true,default_features=false}\n[workspace.dependencies]\ndep='1'",
        2021,
        |progress, root, warnings, session| {
            let Progress::Workspace(request) = progress else {
                panic!("workspace")
            };
            let view = request
                .resume_declared(root, &mut warnings, &mut session)
                .unwrap();
            assert!(view.simple().is_none());
            assert!(matches!(view.get(Field::Version), Some(Value::Text("1"))));
            assert!(view.get(Field::Features).is_none());
            assert!(view.get(Field::DefaultFeatures).is_none());
            assert_eq!(warnings.as_slice(), ["`default-features` is ignored for dep, since `default-features` was not specified for `workspace.dependencies.dep`, this could become a hard error in the future"]);
        }
    );
}

#[test]
fn missing_dependency_and_2024_override_reject() {
    for (source, tag) in [
        (
            "member={workspace=true}\n[workspace]",
            Tag::DependencyWorkspaceMissing,
        ),
        (
            "member={workspace=true}\n[workspace.dependencies]",
            Tag::DependencyInheritedMissing,
        ),
        (
            "member={workspace=true,default-features=false}\n[workspace.dependencies]\ndep='1'",
            Tag::DependencyDefaultFeatures,
        ),
    ] {
        inputs!(source, 2024, |progress, root, warnings, session| {
            let Progress::Workspace(request) = progress else {
                panic!("workspace")
            };
            let result = request.resume_declared(root, &mut warnings, &mut session);
            assert!(result.is_err());
            assert!(
                matches!(session.finish(result), Err(Error::Invalid(diagnostic)) if diagnostic.tag == tag)
            );
        });
    }
}
