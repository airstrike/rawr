use rawr::host::{facade, item, signature, type_ref};
use rawr::{Engine, ParseErrorType, capability, receiver, resolve};

#[test]
fn exact_integer_and_string_host_method_overloads_compile_under_one_spelling() {
    let mut engine = Engine::new();
    engine.register_host_facade(keyed_facade("keyed", false)).unwrap();

    engine
        .compile_executable(
            r#"
                use keyed::Value;
                enum Message { Tick }
                pub fn integer(value: Value<Message>) -> Value<Message> { value.key(1) }
                pub fn string(value: Value<Message>) -> Value<Message> { value.key("one") }
            "#,
        )
        .unwrap();
}

#[test]
fn overload_resolution_reports_positioned_no_match_and_ambiguity() {
    let mut exact = Engine::new();
    exact.register_host_facade(keyed_facade("keyed", false)).unwrap();
    let no_match = exact
        .compile_executable(
            r#"
                use keyed::Value;
                enum Message { Tick }
                pub fn invalid(value: Value<Message>) -> Value<Message> { value.key(true) }
            "#,
        )
        .unwrap_err();
    assert!(
        matches!(
            no_match.err_type(),
            ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, reason, .. })
                if item == "keyed::Value::key" && reason.contains("no form of it takes")
        ),
        "{no_match:?}"
    );
    assert!(!no_match.primary_span().is_none());

    let mut ambiguous = Engine::new();
    ambiguous.register_host_facade(keyed_facade("keyed", true)).unwrap();
    let ambiguity = ambiguous
        .compile_executable(
            r#"
                use keyed::Value;
                enum Message { Tick }
                pub fn invalid(value: Value<Message>) -> Value<Message> { value.key(1) }
            "#,
        )
        .unwrap_err();
    assert!(matches!(
        ambiguity.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, reason, .. })
            if item == "keyed::Value::key" && reason.contains("forms of it take these arguments")
    ));
    assert!(!ambiguity.primary_span().is_none());
}

#[test]
fn overload_lookup_stays_with_the_receiver_facade() {
    let first = keyed_facade("first", false);
    let second = keyed_facade("second", false);
    let mut engine = Engine::new();
    engine.register_host_facade(first).unwrap();
    engine.register_host_facade(second).unwrap();

    engine
        .compile_executable(
            r#"
                use first::Value;
                enum Message { Tick }
                pub fn key(value: Value<Message>) -> Value<Message> { value.key("first") }
            "#,
        )
        .unwrap();
}

#[test]
fn generic_capabilities_filter_candidates_without_leaking_inference() {
    let mut incapable = Engine::new();
    incapable.register_host_facade(concrete_and_generic_facade(&[capability::Kind::Clone, capability::Kind::Copy])).unwrap();
    // The rejected generic candidate returns bool. Exact Value output therefore
    // also proves its inferred signature did not escape candidate selection.
    // An owned `String` is not `Copy`; a `&str` literal would be.
    incapable
        .compile_executable(
            r#"
                use constrained::Value;
                enum Message { Tick }
                pub fn key(value: Value<Message>) -> Value<Message> { value.key(String::from("copy fails")) }
            "#,
        )
        .unwrap();

    let mut capable = Engine::new();
    capable.register_host_facade(concrete_and_generic_facade(&[capability::Kind::Clone])).unwrap();
    let ambiguity = capable
        .compile_executable(
            r#"
                use constrained::Value;
                enum Message { Tick }
                pub fn key(value: Value<Message>) -> Value<Message> { value.key("both match") }
            "#,
        )
        .unwrap_err();
    assert!(
        matches!(
            ambiguity.err_type(),
            ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, reason, .. })
                if item == "constrained::Value::key" && reason.contains("forms of it take these arguments")
        ),
        "{ambiguity:?}"
    );

    let mut constraint_only = Engine::new();
    constraint_only.register_host_facade(constraint_only_facade()).unwrap();
    constraint_only
        .compile_executable(
            r#"
                use constrained::Value;
                enum Message { Tick }
                pub fn string(value: Value<Message>) -> Value<Message> { value.key(String::from("clone only")) }
            "#,
        )
        .unwrap();
    let ambiguity = constraint_only
        .compile_executable(
            r#"
                use constrained::Value;
                enum Message { Tick }
                pub fn integer(value: Value<Message>) -> Value<Message> { value.key(1) }
            "#,
        )
        .unwrap_err();
    assert!(
        matches!(
            ambiguity.err_type(),
            ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, reason, .. })
                if item == "constrained::Value::key" && reason.contains("forms of it take these arguments")
        ),
        "{ambiguity:?}"
    );

    let no_match = constraint_only
        .compile_executable(
            r#"
                use constrained::{Opaque, Value};
                enum Message { Tick }
                pub fn opaque(value: Value<Message>, key: Opaque) -> Value<Message> {
                    value.key(key)
                }
            "#,
        )
        .unwrap_err();
    assert!(matches!(
        no_match.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, reason, .. })
            if item == "constrained::Value::key" && reason.contains("no form of it takes")
    ));
    assert!(!no_match.primary_span().is_none());
}

fn keyed_facade(root: &str, ambiguous: bool) -> facade::Definition {
    let mut builder = facade::Builder::new(root, facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Value>("Value", &["Message"], &[]).unwrap();
    builder.item(key_method(type_ref::Type::builtin(type_ref::Builtin::I64))).unwrap();
    builder.item(key_method(type_ref::Type::builtin(type_ref::Builtin::String))).unwrap();
    if ambiguous {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Message", &[]).unwrap();
        signature.type_parameter("Key", &[]).unwrap();
        signature.parameter(type_ref::Type::parameter("Key").unwrap());
        signature.output(type_ref::Type::applied("Value", [type_ref::Type::parameter("Message").unwrap()]).unwrap());
        builder.item(item::Definition::method("Value::key", receiver::Mode::Owned, signature.finish()).unwrap()).unwrap();
    }
    builder.finish().unwrap()
}

fn concrete_and_generic_facade(requirements: &[capability::Kind]) -> facade::Definition {
    let mut builder = facade::Builder::new("constrained", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Value>("Value", &["Message"], &[]).unwrap();
    builder.item(key_method(type_ref::Type::builtin(type_ref::Builtin::String))).unwrap();
    builder.item(generic_key_method_with_output(requirements, type_ref::Type::builtin(type_ref::Builtin::Bool))).unwrap();
    builder.finish().unwrap()
}

fn constraint_only_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("constrained", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Value>("Value", &["Message"], &[]).unwrap();
    builder.type_::<Opaque>("Opaque", &[], &[]).unwrap();
    builder.item(generic_key_method(&[capability::Kind::Clone])).unwrap();
    builder.item(generic_key_method(&[capability::Kind::Clone, capability::Kind::Copy])).unwrap();
    builder.finish().unwrap()
}

fn generic_key_method(requirements: &[capability::Kind]) -> item::Definition {
    generic_key_method_with_output(requirements, type_ref::Type::applied("Value", [type_ref::Type::parameter("Message").unwrap()]).unwrap())
}

fn generic_key_method_with_output(requirements: &[capability::Kind], output: type_ref::Type) -> item::Definition {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[]).unwrap();
    signature.type_parameter("Key", requirements).unwrap();
    signature.parameter(type_ref::Type::parameter("Key").unwrap());
    signature.output(output);
    item::Definition::method("Value::key", receiver::Mode::Owned, signature.finish()).unwrap()
}

fn key_method(key: type_ref::Type) -> item::Definition {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[]).unwrap();
    signature.parameter(key);
    signature.output(type_ref::Type::applied("Value", [type_ref::Type::parameter("Message").unwrap()]).unwrap());
    item::Definition::method("Value::key", receiver::Mode::Owned, signature.finish()).unwrap()
}

struct Value;
struct Opaque;
