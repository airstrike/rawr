//! Messages name what the program wrote at the span it wrote it, never a
//! placeholder for rawr's own data such as `<module>` (#539, D664).

use crate::Engine;

/// The message and the line and column of the error compiling `source`.
fn rejected_at(source: &str) -> (String, Option<usize>, Option<usize>) {
    let error = Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("Rust rejects this program, or Rawr can't run it yet");
    let position = error.position();
    let message = error.err_type().to_string();
    assert!(!message.contains('<'), "{message}");
    (message, position.line(), position.position())
}

#[test]
fn a_free_function_call_names_the_function_alone() {
    let (message, line, column) =
        rejected_at("fn add(a: i64, b: i64) -> i64 { a + b }\npub fn run() -> i64 { add(1) }");
    assert_eq!(
        message,
        "this function takes 2 arguments but 1 argument was supplied"
    );
    assert_eq!((line, column), (Some(2), Some(23)));
}

#[test]
fn a_closure_call_names_the_closure_as_its_local() {
    let (message, line, column) =
        rejected_at("pub fn run() -> i64 { let f = |x: i64| x; f(1, 2) }");
    assert_eq!(
        message,
        "this function takes 1 argument but 2 arguments were supplied"
    );
    assert_eq!((line, column), (Some(1), Some(43)));
}

#[test]
fn drop_names_itself() {
    let (message, line, column) = rejected_at("pub fn run() -> i64 { drop(1, 2); 0 }");
    assert_eq!(
        message,
        "this function takes 1 argument but 2 arguments were supplied"
    );
    assert_eq!((line, column), (Some(1), Some(23)));
}

#[test]
fn a_free_function_signature_names_the_function_alone() {
    let (message, line, column) =
        rejected_at("fn f(a: i64) -> &mut i64 { a }\npub fn run() -> i64 { 0 }");
    assert!(
        message.starts_with("Invalid signature for 'f': "),
        "{message}"
    );
    assert_eq!((line, column), (Some(1), Some(17)));
}

/// rustc's E0403, at the second name.
#[test]
fn a_generic_parameter_named_twice_is_rustcs_e0403() {
    let (message, line, column) =
        rejected_at("fn id<T, T>(x: T) -> T { x }\npub fn run() -> i64 { 0 }");
    assert_eq!(
        message,
        "The name 'T' is already used for a generic parameter"
    );
    assert_eq!((line, column), (Some(1), Some(10)));
    let error = crate::resolve::Error::DuplicateMember {
        owner: crate::ImmutableString::new(),
        member: "T".into(),
        declaration: crate::Span::NONE,
        previous_declaration: crate::Span::NONE,
    };
    assert!(error.to_string().ends_with("(E0403)"), "{error}");
}

/// rustc's E0282, naming the parameter the call gives no type, at the call.
#[test]
fn a_generic_call_with_an_unknown_type_is_rustcs_e0282() {
    let (message, line, column) = rejected_at(
        "fn make<T: Default>() -> T { T::default() }\n\
         pub fn run() -> i64 { let _ = make(); 0 }",
    );
    assert_eq!(
        message,
        "Type annotations needed: cannot infer the type of the type parameter 'T'"
    );
    assert_eq!((line, column), (Some(2), Some(31)));
    let error = crate::resolve::Error::TypeAnnotationsNeeded {
        parameter: "T".into(),
        reference: crate::Span::NONE,
    };
    assert_eq!(
        error.category(),
        crate::resolve::Category::TypeAnnotationsNeeded
    );
    assert!(error.to_string().ends_with("(E0282)"), "{error}");
}

/// A crate signature's type the build doesn't reach is reported where the
/// program uses the item, not at no location.
#[test]
fn an_unreached_signature_type_is_reported_at_its_use() {
    let at = crate::Span::new(crate::Position::new(3, 5), crate::Position::new(3, 12));
    match super::unnamed_signature_type("iced::run", at, Some("iced::Settings")) {
        crate::resolve::Error::InvalidHostOperation {
            item,
            reason,
            operation,
        } => {
            assert_eq!(item, "iced::run");
            assert_eq!(
                reason,
                "its signature names `iced::Settings`, which is not supported in Rawr yet"
            );
            assert_eq!(operation, at);
        }
        other => panic!("{other:?}"),
    }
}
