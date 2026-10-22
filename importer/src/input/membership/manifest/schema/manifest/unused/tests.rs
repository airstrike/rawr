use rawr_toml::document;

use crate::input::membership::manifest::{reference, session, value};

use super::super::Original;
use super::{package, render};

mod resources;

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut reference::Account).unwrap()
}

fn source<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}

fn interpret<'d>(document: &'d document::Document<'d>) -> Original<'d> {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Original::interpret(source(document), &mut session);
    session.finish(result).unwrap()
}

const SOURCE: &str = r#"value={
    before=[{deep=true}],
    package={first=1,version={workspace=true,extra=[]},last=2,metadata={ordinary=true}},
    lib={extra=true},
    bin=[{name='one',extra=[]},{name='two',other=[]}],
    target={'cfg(unix)'={extra=[]}},
    hints={extra=[],mostly-unused={ordinary=true}},
    workspace={first=1,package={extra=[]},last=2},
    profile={dev={first=1,build-override={inside=2},last=3,package={'a:1.2'={old=1},'a@1.2'={new=2}}}},
    dependencies={a={workspace=true,extra={ordinary=true}}},
    lints={rust={unused={level='warn',extra={ordinary=true}}}},
    after=true
}"#;

#[test]
fn all_callback_families_preserve_success_encounter_order_and_replaced_profiles() {
    let document = parse(SOURCE);
    let original = interpret(&document);
    let expected = [
        "before",
        "package.first",
        "package.version.extra",
        "package.last",
        "lib.extra",
        "bin.0.extra",
        "bin.1.other",
        "target.cfg(unix).extra",
        "hints.extra",
        "workspace.first",
        "workspace.package.extra",
        "workspace.last",
        "profile.dev.first",
        "profile.dev.build-override.inside",
        "profile.dev.last",
        "profile.dev.package.a:1.2.old",
        "profile.dev.package.a@1.2.new",
        "after",
    ];
    assert_eq!(original.unused().ordered().collect::<Vec<_>>(), expected);
    let mut sorted = expected.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(original.unused().unique().collect::<Vec<_>>(), sorted);
    for index in original.unused().unique.as_slice() {
        let actual = original.unused().ordered.as_slice()[*index].as_str();
        assert_eq!(
            actual.as_ptr(),
            original.unused().ordered().nth(*index).unwrap().as_ptr()
        );
    }
}

#[test]
fn rendered_collisions_and_empty_components_match_cargo_stringify() {
    let document = parse(
        "value={'package.edition.extra'=1,package={edition={workspace=true,extra=2}},''=3,'.'=4}",
    );
    let original = interpret(&document);
    assert_eq!(
        original.unused().ordered().collect::<Vec<_>>(),
        ["package.edition.extra", "package.edition.extra", "", "."]
    );
    assert_eq!(
        original.unused().unique().collect::<Vec<_>>(),
        ["", ".", "package.edition.extra"]
    );
    let key = |text| {
        package::Position::Map(value::Key {
            text: value::Text::Borrowed(text),
            span: None,
        })
    };
    for (path, expected) in [
        (vec![key(""), key(""), key("a")], "a".to_owned()),
        (vec![key("a"), key(""), key("b")], "a..b".to_owned()),
        (
            vec![key("λ.β"), package::Position::Sequence(0)],
            "λ.β.0".to_owned(),
        ),
        (
            vec![package::Position::Sequence(usize::MAX)],
            usize::MAX.to_string(),
        ),
    ] {
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        assert_eq!(render(&path, &mut session).unwrap(), expected);
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn raw_datetime_is_one_callback_and_generic_datetime_does_not_visit_trailing_fields() {
    let document = parse(
        r#"value={
        hints=2026-09-05T01:02:03Z,
        package={metadata={'$__toml_private_datetime'='2026-09-05T01:02:03Z',later=340282366920938463463374607431768211456}},
        unknown={nested={ignored=true}}
    }"#,
    );
    let original = interpret(&document);
    assert_eq!(
        original.unused().ordered().collect::<Vec<_>>(),
        ["hints.$__toml_private_datetime", "unknown"]
    );
}

#[test]
fn profile_ancestry_is_iterative_and_retains_every_original_location() {
    let depth = 60;
    let mut source = String::from("[value.profile.dev");
    for _ in 0..depth {
        source.push_str(".build-override");
    }
    source.push_str("]\nunknown=true\n");
    let document = parse(&source);
    let original = interpret(&document);
    let expected = format!("profile.dev{}.unknown", ".build-override".repeat(depth));
    assert_eq!(
        original.unused().ordered().collect::<Vec<_>>(),
        [expected.as_str()]
    );
    assert_eq!(
        original.unused().unique().collect::<Vec<_>>(),
        [expected.as_str()]
    );
    drop(original);
}
