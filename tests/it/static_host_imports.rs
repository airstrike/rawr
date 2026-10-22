use rawr::host::{facade, item, signature, type_ref};
use rawr::{Engine, ParseErrorType, capability, resolve};

#[test]
fn static_facade_imports_resolve_modules_items_and_unqualified_types() {
    let mut engine = Engine::new();
    engine.register_host_facade(test_facade()).unwrap();

    let source = r#"
use iced::Center;
use iced::widget::{Column, button};

enum Message { Tick }
struct Screen { column: Column<'_, Message> }
"#;
    let executable = engine.compile_executable(source).unwrap();

    assert_eq!(executable.type_count(), 2);
    assert!(engine.eval::<rawr::Dynamic>("iced::run()").is_err());
}

#[test]
fn static_import_failures_are_positioned_resolution_errors() {
    let mut engine = Engine::new();
    engine.register_host_facade(test_facade()).unwrap();

    let unknown = engine.compile("use iced::widget::Missing;").unwrap_err();
    assert!(matches!(
        unknown.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownImport { path, .. })
            if path == "iced::widget::Missing"
    ));
    assert_eq!(unknown.primary_span().start().position(), Some(19));

    let invalid_group = engine.compile("use iced::Center::{x};").unwrap_err();
    assert!(matches!(
        invalid_group.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidImport { path, .. })
            if path == "iced::Center"
    ));
    assert_eq!(invalid_group.primary_span().start().position(), Some(5));

    let duplicate = engine.compile("use iced::Result;\nstruct Result;").unwrap_err();
    assert!(matches!(
        duplicate.err_type(),
        ParseErrorType::Resolution(resolve::Error::DuplicateItem { name, .. })
            if name == "Result"
    ));
    assert_eq!(duplicate.primary_span().start().line(), Some(1));
    assert_eq!(duplicate.secondary_span().unwrap().start().line(), Some(2));
}

#[test]
fn facade_type_arity_ignores_erased_lifetimes_but_checks_type_arguments() {
    let mut engine = Engine::new();
    engine.register_host_facade(test_facade()).unwrap();

    engine.compile("use iced::widget::Column; enum Message { Tick } struct Good { value: Column<'_, Message> }").unwrap();

    let error = engine
        .compile("use iced::widget::Column; enum Message { Tick } struct Bad { value: Column<i64, Message> }")
        .unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidTypeArguments {
            ty,
            expected: 1,
            actual: 2,
            ..
        }) if ty == "Column"
    ));
}

#[test]
fn derives_use_explicit_host_capabilities_and_legacy_types_remain_opaque() {
    let mut engine = Engine::new();
    engine.register_host_facade(test_facade()).unwrap();
    engine.compile("use iced::Payload; #[derive(Clone)] struct Wrapper { value: Payload }").unwrap();

    engine.register_type_with_name::<Opaque>("Opaque");
    let error = engine.compile("#[derive(Clone)] struct Wrapper { value: Opaque }").unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidDerive {
            capability,
            ..
        }) if capability == "Clone"
    ));

    assert!(engine.eval::<Opaque>("make_opaque()").is_err());
}

#[test]
fn facade_operations_reject_direct_calls_and_value_references() {
    let mut engine = Engine::new();
    engine.register_host_facade(test_facade()).unwrap();

    for source in ["use iced::load; pub fn start() { load() }", "use iced::load; pub fn start() { let operation = load; () }"] {
        let error = engine.compile_executable(source).unwrap_err();
        assert!(matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, .. })
                if item == "iced::load"
        ));
        assert!(!error.primary_span().is_none());
    }
}

#[test]
fn inclusive_integer_ranges_are_exact_static_host_operands() {
    let mut engine = Engine::new();
    engine.register_host_facade(test_facade()).unwrap();

    engine
        .compile_executable(
            r#"
use iced::range_value;
pub fn selected() -> i64 { range_value(0..=100, 42) }
"#,
        )
        .unwrap();

    for (source, expected) in [
        ("use iced::range_value; pub fn selected() -> i64 { range_value(0..=\"100\", 42) }", "{integer}"),
        ("use iced::range_value; pub fn selected() -> i64 { range_value(0..=100, true) }", "i64"),
    ] {
        let error = engine.compile_executable(source).unwrap_err();
        assert!(error.to_string().contains(expected), "range diagnostic did not contain `{expected}`: {error}");
        assert!(!error.primary_span().is_none());
    }
}

#[test]
fn inclusive_integer_range_changes_the_canonical_facade_fingerprint() {
    let ranged = range_facade(type_ref::Builtin::InclusiveRange);
    let scalar = range_facade(type_ref::Builtin::I64);

    assert_ne!(ranged.fingerprint(), scalar.fingerprint());
    assert_eq!(ranged.fingerprint(), range_facade(type_ref::Builtin::InclusiveRange).fingerprint());
}

#[test]
fn task_perform_accepts_only_exact_positioned_operation_selectors() {
    let mut engine = Engine::new();
    engine.register_host_facade(test_facade()).unwrap();

    let valid = r#"
use iced;
use iced::{Task, combine, load};
enum Message { Loaded(String), Count(i64) }
pub fn loaded(value: String) -> Message { Message::Loaded(value) }
pub fn count(value: i64) -> Message { Message::Count(value) }
pub fn string_task() -> Task<Message> { Task::perform(iced::load(), loaded) }
pub fn integer_task() -> Task<Message> { Task::perform(load(1), count) }
pub fn many_task() -> Task<Message> { Task::perform(combine(1, " rawr"), loaded) }
"#;
    engine.compile_executable(valid).unwrap();

    let wrong_input = valid.replace("Task::perform(load(1), count)", "Task::perform(load(true), count)");
    let error = engine.compile_executable(&wrong_input).unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, .. })
            if item == "iced::load"
    ));
    assert!(!error.primary_span().is_none());

    let wrong_arity = valid.replace("Task::perform(load(1), count)", "Task::perform(load(1, 2), count)");
    let error = engine.compile_executable(&wrong_arity).unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, .. })
            if item == "iced::load"
    ));
    assert!(!error.primary_span().is_none());

    let missing = valid.replace("Task::perform(load(1), count)", "Task::perform(missing(1), count)");
    let error = engine.compile_executable(&missing).unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, .. })
            if item == "missing"
    ));
    assert!(!error.primary_span().is_none());
}

fn test_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    builder.module("widget").unwrap();
    builder.type_::<ResultMarker>("Result", &[], &[]).unwrap();
    builder.type_::<Payload>("Payload", &[], &[capability::Kind::Clone]).unwrap();
    builder.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    builder.required_type::<TaskMarker>("Task", &["Message"], &[]).unwrap();

    builder.item(item::Definition::value("Center", type_ref::Type::registered("Result").unwrap()).unwrap()).unwrap();
    builder.item(item::Definition::function("run", signature::Builder::new().finish()).unwrap()).unwrap();

    let mut load = signature::Builder::new();
    load.output(type_ref::Type::builtin(type_ref::Builtin::String));
    builder.item(item::Definition::operation("load", load.finish()).unwrap()).unwrap();
    let mut load = signature::Builder::new();
    load.parameter(type_ref::Type::builtin(type_ref::Builtin::I64)).output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::operation("load", load.finish()).unwrap()).unwrap();

    let mut combine = signature::Builder::new();
    combine.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    combine.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    combine.output(type_ref::Type::builtin(type_ref::Builtin::String));
    builder.item(item::Definition::operation("combine", combine.finish()).unwrap()).unwrap();

    let mut range_value = signature::Builder::new();
    range_value.parameter(type_ref::Type::builtin(type_ref::Builtin::InclusiveRange));
    range_value.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    range_value.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::function("range_value", range_value.finish()).unwrap()).unwrap();

    let mut perform = signature::Builder::new();
    perform.type_parameter("Output", &[]).unwrap();
    perform.type_parameter("Message", &[]).unwrap();
    perform.parameter(type_ref::Type::parameter("Output").unwrap());
    perform.parameter(type_ref::Type::callable(type_ref::Type::parameter("Output").unwrap(), type_ref::Type::parameter("Message").unwrap()));
    perform.output(type_ref::Type::applied("Task", [type_ref::Type::parameter("Message").unwrap()]).unwrap());
    builder.item(item::Definition::associated_function("Task::perform", perform.finish()).unwrap()).unwrap();

    let mut button = signature::Builder::new();
    button.type_parameter("Message", &[]).unwrap();
    button.output(type_ref::Type::applied("widget::Column", [type_ref::Type::parameter("Message").unwrap()]).unwrap());
    builder.item(item::Definition::function("widget::button", button.finish()).unwrap()).unwrap();
    builder.finish().unwrap()
}

fn range_facade(input: type_ref::Builtin) -> facade::Definition {
    let mut builder = facade::Builder::new("range", facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = signature::Builder::new();
    signature.parameter(type_ref::Type::builtin(input));
    signature.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::function("identity", signature.finish()).unwrap()).unwrap();
    builder.finish().unwrap()
}

#[derive(Clone)]
struct Payload;

#[derive(Clone)]
struct Opaque;

struct ResultMarker;
struct Column;
struct TaskMarker;
