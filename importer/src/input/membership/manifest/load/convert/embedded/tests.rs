use super::*;
use crate::input::membership::manifest::{reference, value};

fn checked(text: &str) -> Result<(), String> {
    let mut account = reference::Account;
    let document = document::parse(text, 80, &mut account).unwrap();
    let mut session = Session::new(&mut account);
    let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let result = check(&original, &mut session);
    session.finish(result).unwrap()
}

#[test]
fn every_field_counts_presence_including_false_empty_and_aliases() {
    for (source, name) in [
        ("workspace={}", "workspace"),
        ("lib={}", "lib"),
        ("bin=[]", "bin"),
        ("example=[]", "example"),
        ("test=[]", "test"),
        ("bench=[]", "bench"),
        ("build-dependencies={}", "build-dependencies"),
        ("build_dependencies={}", "build_dependencies"),
        ("package.workspace=''", "package.workspace"),
        ("package.build=false", "package.build"),
        ("package.metabuild=[]", "package.metabuild"),
        ("package.links=''", "package.links"),
        ("package.autolib=false", "package.autolib"),
        ("package.autobins=false", "package.autobins"),
        ("package.autoexamples=false", "package.autoexamples"),
        ("package.autotests=false", "package.autotests"),
        ("package.autobenches=false", "package.autobenches"),
        ("package.default-run=''", "package.default-run"),
    ] {
        assert_eq!(
            checked(source),
            Err(format!("`{name}` is not allowed in embedded manifests")),
            "{source}"
        );
    }
}

#[test]
fn full_error_list_uses_source_priority_and_does_not_collapse_aliases() {
    let source = "package.default-run=''\npackage.autobenches=false\npackage.autotests=false\npackage.autoexamples=false\npackage.autobins=false\npackage.autolib=false\npackage.links=''\npackage.metabuild=[]\npackage.build=false\npackage.workspace=''\nbuild_dependencies={}\nbuild-dependencies={}\nbench=[]\ntest=[]\nexample=[]\nbin=[]\nlib={}\nworkspace={}";
    assert_eq!(checked(source), Err("`workspace`, `lib`, `bin`, `example`, `test`, `bench`, `build-dependencies`, `build_dependencies`, `package.workspace`, `package.build`, `package.metabuild`, `package.links`, `package.autolib`, `package.autobins`, `package.autoexamples`, `package.autotests`, `package.autobenches`, `package.default-run` are not allowed in embedded manifests".into()));
}

#[test]
fn original_package_precedes_project_and_project_uses_package_labels() {
    assert_eq!(
        checked("project.build=false"),
        Err("`package.build` is not allowed in embedded manifests".into())
    );
    assert_eq!(checked("package={}\nproject.build=false"), Ok(()));
    assert_eq!(
        checked("project.build=false\npackage.links=''"),
        Err("`package.links` is not allowed in embedded manifests".into())
    );
}

#[test]
fn permitted_original_fields_are_not_confused_with_synthetic_normalized_fields() {
    assert_eq!(checked(""), Ok(()));
    assert_eq!(checked("package.name='p'\npackage.edition='2024'\npackage.version='1.0.0'\ndependencies={}\ndev-dependencies={}\ndev_dependencies={}\nfeatures={}\nlints={}\nhints={}\nprofile={}\npatch={}\nreplace={}\npackage.include=[]\npackage.exclude=[]\npackage.publish=false"), Ok(()));
}

#[test]
fn original_account_refusal_cannot_be_rendered_as_embedded_semantic_failure() {
    struct Refuse;
    impl document::Admission for Refuse {
        type Error = &'static str;
        fn admit(&mut self, _: document::Request) -> Result<(), Self::Error> {
            Err("original exhausted")
        }
    }
    let mut account = reference::Account;
    let document = document::parse("bin=[]", 80, &mut account).unwrap();
    let mut parse = Session::new(&mut account);
    let original = Original::interpret(value::Input::root(&document), &mut parse).unwrap();
    parse.finish(Ok(())).unwrap();
    let mut account = Refuse;
    let mut session = Session::new(&mut account);
    let result = check(&original, &mut session);
    assert!(result.is_err());
    assert!(check(&original, &mut session).is_err());
    assert!(session.finish(result).is_err());
}
