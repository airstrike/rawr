//! A call with the wrong number of arguments reads as rustc's E0061, or
//! E0057 for a closure, at the call, with rustc's note at the callee's
//! definition (#544, D664).

use crate::resolve::{Callee, Category, Error};
use crate::{Engine, ParseErrorType};

/// The resolution error, and the report as `rawr` prints it, of compiling
/// `source`.
fn rejected(source: &str) -> (Error, String) {
    let error = Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rustc rejects this program");
    let ParseErrorType::Resolution(resolution) = error.err_type() else {
        panic!("{error}");
    };
    (resolution.clone(), error.to_string())
}

#[test]
fn a_free_function_reads_as_e0061_with_its_definition() {
    let (error, report) =
        rejected("fn add(a: i64, b: i64) -> i64 { a + b }\npub fn run() -> i64 { add(1) }");
    assert_eq!(error.category(), Category::ArgumentCount);
    assert!(matches!(
        error,
        Error::ArgumentCount {
            callee: Callee::Function,
            expected: 2,
            found: 1,
            ..
        }
    ));
    assert!(
        report.starts_with(
            "error[E0061]: this function takes 2 arguments but 1 argument was supplied\n"
        ),
        "{report}"
    );
    assert!(report.contains("function defined here"), "{report}");
    assert!(report.contains("help: provide the argument"), "{report}");
    assert!(!report.contains("Invalid call"), "{report}");
}

#[test]
fn extra_arguments_say_were_and_remove() {
    let (_, report) =
        rejected("fn add(a: i64, b: i64) -> i64 { a + b }\npub fn run() -> i64 { add(1, 2, 3) }");
    assert!(
        report.starts_with(
            "error[E0061]: this function takes 2 arguments but 3 arguments were supplied\n"
        ),
        "{report}"
    );
    assert!(
        report.contains("help: remove the extra argument"),
        "{report}"
    );
}

#[test]
fn a_closure_reads_as_e0057_with_its_header() {
    let (error, report) = rejected("pub fn run() -> i64 { let f = |x: i64| x; f(1, 2) }");
    assert!(matches!(
        error,
        Error::ArgumentCount {
            callee: Callee::Closure,
            expected: 1,
            found: 2,
            ..
        }
    ));
    assert!(
        report.starts_with(
            "error[E0057]: this function takes 1 argument but 2 arguments were supplied\n"
        ),
        "{report}"
    );
    assert!(report.contains("closure defined here"), "{report}");
    let Error::ArgumentCount {
        definition: Some(header),
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    // `|x: i64|`, both pipes.
    assert_eq!(
        (header.start().position(), header.end().position()),
        (Some(31), Some(39))
    );
}

#[test]
fn a_method_reads_as_e0061_at_its_name() {
    let (error, report) = rejected(
        "struct S;\nimpl S { fn m(&self, a: i64) -> i64 { a } }\npub fn run() -> i64 { S.m() }",
    );
    let Error::ArgumentCount {
        callee: Callee::Method,
        expected: 1,
        found: 0,
        call,
        definition: Some(definition),
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    // rustc points at `m` and its parentheses, and notes `m` in the impl.
    assert_eq!(
        (call.start().line(), call.start().position()),
        (Some(3), Some(25))
    );
    assert_eq!(
        (definition.start().line(), definition.start().position()),
        (Some(2), Some(13))
    );
    assert!(
        report.starts_with(
            "error[E0061]: this method takes 1 argument but 0 arguments were supplied\n"
        ),
        "{report}"
    );
    assert!(report.contains("method defined here"), "{report}");
}

#[test]
fn an_associated_function_says_function() {
    let (error, report) =
        rejected("struct S;\nimpl S { fn n() -> i64 { 0 } }\npub fn run() -> i64 { S::n(1) }");
    assert!(matches!(
        error,
        Error::ArgumentCount {
            callee: Callee::AssociatedFunction,
            ..
        }
    ));
    assert!(
        report.starts_with(
            "error[E0061]: this function takes 0 arguments but 1 argument was supplied\n"
        ),
        "{report}"
    );
    assert!(
        report.contains("associated function defined here"),
        "{report}"
    );
}

#[test]
fn a_builtin_method_reads_as_e0061() {
    let (_, report) = rejected("pub fn run() -> Option<i64> { Some(1).map() }");
    assert!(
        report.starts_with(
            "error[E0061]: this method takes 1 argument but 0 arguments were supplied\n"
        ),
        "{report}"
    );
}

#[test]
fn a_tuple_variant_and_struct_read_as_e0061() {
    let (_, report) = rejected("enum E { A(i64) }\npub fn run() { let _ = E::A(); }");
    assert!(
        report.starts_with(
            "error[E0061]: this enum variant takes 1 argument but 0 arguments were supplied\n"
        ),
        "{report}"
    );
    let (_, report) = rejected("struct T(i64, i64);\npub fn run() { let _ = T(1); }");
    assert!(
        report.starts_with(
            "error[E0061]: this struct takes 2 arguments but 1 argument was supplied\n"
        ),
        "{report}"
    );
}
