use super::*;
use crate::input::membership::manifest::{reference, value};

fn check_text(source: &str) -> Result<(), String> {
    let mut account = reference::Account;
    let document = document::parse(source, 80, &mut account).unwrap();
    let mut session = Session::new(&mut account);
    let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let result = check(&original, &mut session);
    session.finish(result).unwrap()
}
fn message(field: &str) -> String {
    format!("this virtual manifest specifies a `{field}` section, which is not allowed")
}

#[test]
fn every_present_field_including_empty_collections_requires_package() {
    for (source, field) in [
        ("badges={}", "badges"),
        ("features={}", "features"),
        ("lib={}", "lib"),
        ("bin=[]", "bin"),
        ("example=[]", "example"),
        ("test=[]", "test"),
        ("bench=[]", "bench"),
        ("dependencies={}", "dependencies"),
        ("dev-dependencies={}", "dev-dependencies"),
        ("dev_dependencies={}", "dev-dependencies"),
        ("build-dependencies={}", "build-dependencies"),
        ("build_dependencies={}", "build-dependencies"),
        ("target={}", "target"),
        ("lints={}", "lints"),
        ("hints={}", "hints"),
    ] {
        assert_eq!(check_text(source), Err(message(field)), "{source}");
    }
}

#[test]
fn source_field_priority_overrides_document_order_and_alias_spelling() {
    let fields = [
        "badges={}",
        "features={}",
        "lib={}",
        "bin=[]",
        "example=[]",
        "test=[]",
        "bench=[]",
        "dependencies={}",
        "dev_dependencies={}",
        "build_dependencies={}",
        "target={}",
        "lints={}",
        "hints={}",
    ];
    let names = [
        "badges",
        "features",
        "lib",
        "bin",
        "example",
        "test",
        "bench",
        "dependencies",
        "dev-dependencies",
        "build-dependencies",
        "target",
        "lints",
        "hints",
    ];
    for first in 0..fields.len() {
        let source = fields[first..]
            .iter()
            .rev()
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(check_text(&source), Err(message(names[first])));
    }
    assert_eq!(
        check_text("build_dependencies={}\ndev-dependencies={}\ndev_dependencies={}"),
        Err(message("dev-dependencies"))
    );
}

#[test]
fn workspace_and_non_package_sections_are_not_rejected_by_this_helper() {
    for source in [
        "",
        "workspace={}\nprofile={}\npatch={}\nreplace={}",
        "cargo-features=[]\nunknown={}",
    ] {
        assert_eq!(check_text(source), Ok(()));
    }
}

#[test]
fn admission_refusal_is_terminal_and_not_a_semantic_virtual_error() {
    struct Refuse;
    impl document::Admission for Refuse {
        type Error = &'static str;
        fn admit(&mut self, _: document::Request) -> Result<(), Self::Error> {
            Err("original account exhausted")
        }
    }
    let mut account = reference::Account;
    let document = document::parse("features={}", 80, &mut account).unwrap();
    let mut parse = Session::new(&mut account);
    let original = Original::interpret(value::Input::root(&document), &mut parse).unwrap();
    parse.finish(Ok(())).unwrap();
    let mut refused = Refuse;
    let mut session = Session::new(&mut refused);
    let result = check(&original, &mut session);
    assert!(result.is_err());
    assert!(check(&original, &mut session).is_err());
    assert!(session.finish(result).is_err());
}
