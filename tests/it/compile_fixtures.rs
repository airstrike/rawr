use rawr::{Engine, ParseError, ParseErrorType, host, resolve};
use std::fs;
use std::path::{Path, PathBuf};

const PASS_FIXTURES: &[&str] = &["forward_items", "counter_headers", "receivers", "static_iced_use"];
const FAIL_FIXTURES: &[&str] = &["duplicate_item", "unknown_header_type", "malformed_receiver", "unsupported_body_lowering", "unknown_static_import"];

#[test]
fn compile_pass_fixtures_resolve_with_source_spans() {
    for name in PASS_FIXTURES {
        let source = fixture(name, "pass", "rs");
        let expected = fixture(name, "pass", "expect");
        let engine = Engine::new();
        let unit = engine.parse_unit(&source).unwrap_or_else(|error| panic!("compile-pass fixture `{name}` did not parse: {error}"));

        assert!(unit.items().iter().all(|item| !item.span().is_none()), "compile-pass fixture `{name}` contains an item without a source span");

        let item_spans = unit.items().iter().map(|item| item.span().to_string()).collect::<Vec<_>>().join(",");
        let program = resolve::resolve(unit, &fixture_registry()).unwrap_or_else(|error| panic!("compile-pass fixture `{name}` did not resolve: {error}"));

        assert_eq!(program.source().text(), source);

        let actual = format!("category=compile-pass\nitems={}\ntypes={}\nitem-spans={item_spans}\n", program.item_count(), program.type_count());
        assert_eq!(actual, expected, "compile-pass fixture `{name}` changed");
    }
}

#[test]
fn compile_fail_fixtures_match_structured_diagnostics() {
    for name in FAIL_FIXTURES {
        let source = fixture(name, "fail", "rs");
        let expected = fixture(name, "fail", "expect");
        let mut engine = Engine::new();
        engine.register_host_facade(fixture_facade()).unwrap();
        let error = engine.compile(&source).unwrap_err();
        let actual = failure_record(&error);

        assert_eq!(actual, expected, "compile-fail fixture `{name}` changed");
    }
}

fn fixture_registry() -> host::Registry {
    let mut registry = host::Registry::new();
    registry.install(fixture_facade()).unwrap();
    registry
}

fn fixture_facade() -> host::facade::Definition {
    let mut facade = host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap();
    facade.module("widget").unwrap();
    facade.type_::<IcedResult>("Result", &[], &[]).unwrap();
    facade.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    facade
        .item(host::item::Definition::value("Center", host::type_ref::Type::registered("Result").unwrap()).unwrap())
        .unwrap();
    for name in ["button", "column", "text"] {
        facade
            .item(host::item::Definition::function(format!("widget::{name}"), host::signature::Builder::new().finish()).unwrap())
            .unwrap();
    }
    facade.finish().unwrap()
}

struct Column;
struct IcedResult;

fn failure_record(error: &ParseError) -> String {
    let (category, name) = match error.err_type() {
        ParseErrorType::MalformedReceiver(found, _) => ("malformed receiver".to_string(), found.to_string()),
        ParseErrorType::Resolution(error) => (error.category().to_string(), error.name().to_string()),
        other => panic!("expected structured compile diagnostic, got {other:?}"),
    };
    let secondary = error.secondary_span().map_or_else(|| "none".to_string(), |span| span.to_string());

    format!("category={category}\nname={name}\nprimary={}\nsecondary={secondary}\nmessage={error}\n", error.primary_span())
}

fn fixture(name: &str, outcome: &str, extension: &str) -> String {
    let path = fixture_path(name, outcome, extension);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read fixture `{}`: {error}", path.display()))
}

fn fixture_path(name: &str, outcome: &str, extension: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("compile")
        .join(outcome)
        .join(format!("{name}.{extension}"))
}
