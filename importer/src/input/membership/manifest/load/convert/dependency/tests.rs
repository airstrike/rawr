use super::*;
use crate::input::membership::manifest::load::roots;
use crate::input::membership::manifest::{
    reference,
    schema::{dependency::Inheritable, workspace},
    value,
};
use std::path::Path;

fn checked(source: &str) -> (Result<(), String>, Vec<String>) {
    let mut account = reference::Account;
    let document = document::parse(source, 80, &mut account).unwrap();
    let mut session = Session::new(&mut account);
    let member = Inheritable::interpret(
        value::Input::raw(document.table().get("member").unwrap()),
        &mut session,
    )
    .unwrap();
    let workspace = workspace::Workspace::interpret(
        value::Input::raw(document.table().get("workspace").unwrap()),
        &mut session,
    )
    .unwrap();
    let mut warnings = Warnings::new();
    let view =
        match inherit::dependency("dep", &member, Path::new("/ws/member"), 2021, &mut session)
            .unwrap()
        {
            inherit::Progress::Direct(view) => view,
            inherit::Progress::Workspace(pending) => pending
                .resume_declared(
                    roots::Root {
                        directory: Path::new("/ws"),
                        declaration: Some(&workspace),
                    },
                    &mut warnings,
                    &mut session,
                )
                .unwrap(),
        };
    let result = fields(
        "dep",
        view.simple(),
        |field| view.get(field),
        &mut warnings,
        &mut session,
    );
    (
        session.finish(result).unwrap(),
        warnings.as_slice().to_vec(),
    )
}

#[test]
fn missing_source_precedes_git_only_keys_and_features() {
    let (result, warnings) = checked("member={branch='main',features=['dep:x/y']}\n[workspace]");
    assert_eq!(result.unwrap_err(), "dependency (dep) specified without providing a local path, Git repository, version, or workspace dependency to use");
    assert!(warnings.is_empty());
}

#[test]
fn metadata_warning_precedes_first_git_only_key_in_fixed_order() {
    for (fields, expected) in [
        ("rev='r',tag='t',branch='b'", "branch"),
        ("rev='r',tag='t'", "tag"),
        ("rev='r'", "rev"),
    ] {
        let (result, warnings) = checked(&format!(
            "member={{version='1+meta',{fields},features=['dep:x/y']}}\n[workspace]"
        ));
        assert_eq!(
            result.unwrap_err(),
            format!("key `{expected}` is ignored for dependency (dep).")
        );
        assert_eq!(warnings,["version requirement `1+meta` for dependency `dep` includes semver metadata which will be ignored, removing the metadata is recommended to avoid confusion"]);
    }
}

#[test]
fn feature_encounter_order_and_slash_before_dep_prefix_are_preserved() {
    let (result, _) = checked(
        "member={git='not parsed yet',branch='b',features=['ok','dep:x/y','dep:z']}\n[workspace]",
    );
    assert_eq!(result.unwrap_err(), "feature `dep:x/y` in dependency `dep` is not allowed to contain slashes\nIf you want to enable features of a transitive dependency, the direct dependency needs to re-export those features from the `[features]` table.");
    let (result, _) = checked("member={path='',features=['dep:z','x/y']}\n[workspace]");
    assert_eq!(result.unwrap_err(), "feature `dep:z` in dependency `dep` is not allowed to use explicit `dep:` syntax\nIf you want to enable an optional dependency, specify the name of the optional dependency without the `dep:` prefix, or specify a feature from the dependency's `[features]` table that enables the optional dependency.");
}

#[test]
fn simple_and_detailed_values_pass_only_this_early_boundary() {
    for member in [
        "'not a parsed version'",
        "{path=''}",
        "{git='',branch='b',tag='t',rev='r'}",
        "{version='',features=['ok','okay']}",
    ] {
        let (result, warnings) = checked(&format!("member={member}\n[workspace]"));
        result.unwrap();
        assert!(warnings.is_empty());
    }
    let (result, warnings) = checked("member='1+meta'\n[workspace]");
    result.unwrap();
    assert_eq!(warnings.len(), 1);
}

#[test]
fn inherited_workspace_features_are_checked_before_member_features() {
    let (result, warnings) = checked("member={workspace=true,features=['dep:member']}\n[workspace.dependencies]\ndep={version='1+meta',features=['workspace/bad']}");
    assert!(result.unwrap_err().starts_with("feature `workspace/bad`"));
    assert_eq!(warnings.len(), 1);
    let (result, _) = checked("member={workspace=true,features=['dep:member']}\n[workspace.dependencies]\ndep={version='1',features=['workspace-ok']}");
    assert!(result.unwrap_err().starts_with("feature `dep:member`"));
}

#[test]
fn every_admission_refusal_latches_before_later_work() {
    #[derive(Default)]
    struct Account {
        calls: usize,
        refused: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = usize;
        fn admit(&mut self, _: document::Request) -> Result<(), usize> {
            let call = self.calls;
            self.calls += 1;
            if self.refused == Some(call) {
                Err(call)
            } else {
                Ok(())
            }
        }
    }
    let document = document::parse(
        "member={version='1+meta',features=['okay','dep:x/y']}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let mut schema_account = reference::Account;
    let mut schema_session = Session::new(&mut schema_account);
    let member = Inheritable::interpret(
        value::Input::raw(document.table().get("member").unwrap()),
        &mut schema_session,
    )
    .unwrap();
    let inherit::Progress::Direct(view) =
        inherit::dependency("dep", &member, Path::new("/ws"), 2021, &mut schema_session).unwrap()
    else {
        panic!("direct")
    };
    schema_session.finish(Ok(())).unwrap();
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let mut warnings = Warnings::new();
    let result = fields(
        "dep",
        view.simple(),
        |field| view.get(field),
        &mut warnings,
        &mut session,
    );
    assert!(session.finish(result).unwrap().is_err());
    for index in 0..baseline.calls {
        let mut account = Account {
            refused: Some(index),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let mut warnings = Warnings::new();
        let result = fields(
            "dep",
            view.simple(),
            |field| view.get(field),
            &mut warnings,
            &mut session,
        );
        assert!(result.is_err());
        assert!(fields("dep", None, |_| None, &mut warnings, &mut session).is_err());
        assert!(matches!(session.finish(result),Err(Error::Admission(at)) if at==index));
        assert_eq!(account.calls, index + 1);
    }
}
