#![cfg(not(feature = "no_function"))]

use rawr::host::{self, signature as host_signature, type_ref as host_type};
use rawr::item::{self, type_ref};
use rawr::{Engine, ParseErrorType, resolve};

#[test]
fn parser_retains_ordered_tuple_members_and_complete_span() {
    let source = "struct State; enum Message { Tick } pub fn boot() -> (State, Message) { () }";
    let unit = Engine::new().parse_unit(source).unwrap();
    let item::Parsed::Function(boot) = &unit.items()[2] else {
        panic!("expected boot function");
    };
    let Some(type_ref::Type::Tuple { members, span }) = &boot.output else {
        panic!("expected tuple output");
    };

    assert_eq!(members.len(), 2);
    assert!(matches!(&members[0], type_ref::Type::Path(path) if path.segments[0].name.text == "State"));
    assert!(matches!(&members[1], type_ref::Type::Path(path) if path.segments[0].name.text == "Message"));
    assert_eq!(&source[span.start().position().unwrap() - 1..span.end().position().unwrap() - 1], "(State, Message)");
}

#[test]
fn tuple_header_members_resolve_without_runtime_tuple_values() {
    Engine::new().compile_executable("struct Header { pair: (i64, bool) }").unwrap();
}

#[test]
fn tuple_members_resolve_normally_and_report_inner_failures() {
    let source = "struct State; pub fn boot() -> (State, Missing) { () }";
    let error = Engine::new().compile_executable(source).unwrap_err();

    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownType { name, .. }) if name == "Missing"
    ));
    assert_eq!(error.primary_span().start().position(), Some(40));
}

#[test]
fn singleton_tuple_types_are_positioned_unsupported_syntax() {
    // `(i64)` is `i64` in parentheses, as in Rust, so `()` mismatches it.
    let error = Engine::new().compile_executable("pub fn broken() -> (i64) { () }").unwrap_err();
    assert!(
        matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::TypeMismatch { expected, .. }) if expected == "i64"
        ),
        "{error:?}"
    );
    for source in ["pub fn broken() -> (i64,) { () }"] {
        let error = Engine::new().compile_executable(source).unwrap_err();
        assert!(matches!(error.err_type(), ParseErrorType::MalformedTypeTuple(1, _)), "{error:?}");
        assert_eq!(error.primary_span().start().position(), Some(20));
        assert_eq!(error.primary_span().end().position(), Some(if source.contains(",)") { 26 } else { 25 }));
    }
}

#[test]
fn host_signature_tuples_validate_arity_and_fingerprint_structure() {
    let int = host_type::Type::builtin(host_type::Builtin::I64);
    let boolean = host_type::Type::builtin(host_type::Builtin::Bool);
    assert!(matches!(host_type::Type::tuple([]), Err(host::Error::InvalidTupleArity { actual: 0 })));
    assert!(matches!(host_type::Type::tuple([int.clone()]), Err(host::Error::InvalidTupleArity { actual: 1 })));

    let ordered = host_fingerprint(host_type::Type::tuple([int.clone(), boolean.clone()]).unwrap());
    let reversed = host_fingerprint(host_type::Type::tuple([boolean, int]).unwrap());
    let unit = host_fingerprint(host_type::Type::unit());

    assert_ne!(ordered, reversed);
    assert_ne!(ordered, unit);
}

fn host_fingerprint(output: host_type::Type) -> host_signature::Fingerprint {
    let mut signature = host_signature::Builder::new();
    signature.output(output);
    signature.finish().fingerprint()
}
