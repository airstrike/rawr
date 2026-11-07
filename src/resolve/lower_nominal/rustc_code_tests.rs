//! Programs rustc refuses, which rawr refuses with rustc's code, message
//! and label: rustc's own codes, never one rawr made up, and a plain
//! `error:` where rustc gives none.

use crate::diagnostic::{render, Report, Source};
use crate::Engine;

/// rustc's code (empty when it has none), message and primary label for
/// `source`, which rawr refuses.
fn refused(source: &str) -> (String, String, Option<String>) {
    let error = Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rawr refuses this program");
    let source = Source::new("main.rs", source).expect("a test source has an identity");
    let report = Report::from_parse(source, &error).expect("the error is in the source");
    (
        report.code().as_str().to_string(),
        report.message().to_string(),
        report
            .labels()
            .first()
            .and_then(|label| label.message().map(str::to_string)),
    )
}

fn label(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[test]
fn a_type_mismatch_is_e0308() {
    assert_eq!(
        refused("fn main() {\n    let x: i32 = \"a\";\n}\nmain();\n"),
        (
            "E0308".into(),
            "mismatched types".into(),
            label("expected `i32`, found `&str`")
        )
    );
    // A function body with no tail returns `()` (rustlings' functions5).
    let (code, message, _) = refused("fn square(n: i32) -> i32 {\n    n * n;\n}\nmain();\n");
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0308", "mismatched types")
    );
    // An `if` without `else` as a statement: its block must be `()`.
    assert_eq!(
        refused(
            "fn main() {\n    let c = true;\n    if c { 1 }\n    println!(\"x\");\n}\nmain();\n"
        ),
        (
            "E0308".into(),
            "mismatched types".into(),
            label("expected `()`, found integer")
        )
    );
}

#[test]
fn an_if_without_else_whose_value_is_wanted_is_e0317() {
    let wanted = |source: &str| {
        let (code, message, label) = refused(source);
        assert_eq!(code, "E0317", "{source}");
        assert_eq!(message, "`if` may be missing an `else` clause");
        label
    };
    assert_eq!(
        wanted("fn main() {\n    let c = true;\n    let x: i32 = if c { 1 };\n}\nmain();\n"),
        label("expected integer, found `()`")
    );
    assert_eq!(
        wanted("fn f(c: bool) -> i32 {\n    if c { 1 }\n}\nmain();\n"),
        label("expected integer, found `()`")
    );
    // A block that is `()` where the function returns `i32`.
    assert_eq!(
        wanted("fn f(c: bool) -> i32 {\n    if c { return 1; }\n}\nmain();\n"),
        label("expected `i32`, found `()`")
    );
}

#[test]
fn a_return_without_a_value_in_a_function_with_an_output_is_e0069() {
    assert_eq!(
        refused("fn f() -> i32 {\n    return;\n}\nmain();\n"),
        (
            "E0069".into(),
            "`return;` in a function whose return type is not `()`".into(),
            label("return type is not `()`")
        )
    );
}

#[test]
fn a_name_nothing_declares_is_e0425() {
    assert_eq!(
        refused("fn main() {\n    println!(\"{}\", y);\n}\nmain();\n"),
        (
            "E0425".into(),
            "cannot find value `y` in this scope".into(),
            label("not found in this scope")
        )
    );
    assert_eq!(
        refused("fn main() {\n    nothing(1);\n}\nmain();\n"),
        (
            "E0425".into(),
            "cannot find function `nothing` in this scope".into(),
            label("not found in this scope")
        )
    );
    assert_eq!(
        refused("fn f(_x: Foo) {}\n"),
        (
            "E0425".into(),
            "cannot find type `Foo` in this scope".into(),
            label("not found in this scope")
        )
    );
    // An inline `format!` argument (rustlings' primitive_types5).
    let (code, message, _) = refused("fn main() {\n    println!(\"{name}\");\n}\nmain();\n");
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0425", "cannot find value `name` in this scope")
    );
    // A name in a std path's arguments (rustlings' smart_pointers3).
    assert_eq!(
        refused(
            "use std::thread;\nfn main() {\n    let h = thread::spawn(move || {\n        \
             let n: u32 = numbers.iter().sum();\n        println!(\"{n}\");\n    });\n    \
             h.join().unwrap();\n}\n"
        ),
        (
            "E0425".into(),
            "cannot find value `numbers` in this scope".into(),
            label("not found in this scope")
        )
    );
}

#[test]
fn a_paths_unknown_first_segment_is_e0433() {
    assert_eq!(
        refused("fn main() {\n    let x = Foo::new();\n}\nmain();\n"),
        (
            "E0433".into(),
            "cannot find type `Foo` in this scope".into(),
            label("use of undeclared type `Foo`")
        )
    );
    assert_eq!(
        refused("fn main() {\n    let x = nothing::f();\n}\nmain();\n"),
        (
            "E0433".into(),
            "cannot find module or crate `nothing` in this scope".into(),
            label("use of unresolved module or unlinked crate `nothing`")
        )
    );
}

#[test]
fn a_use_of_a_path_that_does_not_exist_is_e0432() {
    let (code, message, _) = refused("use foo::bar;\nfn main() {}\n");
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0432", "unresolved import `foo::bar`")
    );
}

#[test]
fn a_name_used_as_another_kind_of_item_is_e0423() {
    assert_eq!(
        refused("struct Foo { a: i32 }\nfn main() {\n    let x = Foo;\n}\nmain();\n"),
        (
            "E0423".into(),
            "expected value, found struct `Foo`".into(),
            label("not a value")
        )
    );
    // rustlings' macros1.
    let (code, message, _) = refused(
        "macro_rules! my_macro {\n    () => {};\n}\nfn main() {\n    my_macro();\n}\nmain();\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0423", "expected function, found macro `my_macro`")
    );
}

#[test]
fn mutating_a_variable_not_declared_mut_is_rustcs_borrow_error() {
    assert_eq!(
        refused("fn main() {\n    let x = 3;\n    x = 5;\n}\nmain();\n"),
        (
            "E0384".into(),
            "cannot assign twice to immutable variable `x`".into(),
            label("cannot assign twice to immutable variable")
        )
    );
    let (code, message, _) = refused("fn main() {\n    let x = 3;\n    x += 5;\n}\nmain();\n");
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0384", "cannot assign twice to immutable variable `x`")
    );
    let (code, message, _) = refused(
        "struct P { x: i32 }\nfn main() {\n    let p = P { x: 1 };\n    p.x = 5;\n}\nmain();\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "E0594",
            "cannot assign to `p.x`, as `p` is not declared as mutable"
        )
    );
    let (code, message, _) =
        refused("fn main() {\n    let v = vec![1];\n    v[0] = 2;\n}\nmain();\n");
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "E0596",
            "cannot borrow `v` as mutable, as it is not declared as mutable"
        )
    );
    // rustc checks the value's type first (rustlings' variables5).
    let (code, ..) = refused("fn main() {\n    let n = \"three\";\n    n = 3;\n}\nmain();\n");
    assert_eq!(code, "E0308");
}

#[test]
fn assigning_through_an_rc_or_arc_is_e0594() {
    // rustlings' threads2: an `Arc` has no `DerefMut`.
    let assigned = |pointer: &str, body: &str| {
        let (code, message, _) = refused(&format!(
            "use std::rc::Rc;\nuse std::sync::Arc;\nstruct S {{\n    n: u32,\n    \
             inner: (u32, u32),\n}}\nfn main() {{\n    \
             let p = {pointer}::new(S {{ n: 0, inner: (0, 0) }});\n    {body}\n}}\n"
        ));
        assert_eq!(code, "E0594", "{pointer}: {body}");
        message
    };
    let arc = "cannot assign to data in an `Arc`";
    assert_eq!(assigned("Arc", "p.n += 1;"), arc);
    assert_eq!(assigned("Arc", "p.n = 2;"), arc);
    assert_eq!(assigned("Arc", "p.inner.0 = 2;"), arc);
    assert_eq!(assigned("Arc", "*p = S { n: 1, inner: (0, 0) };"), arc);
    assert_eq!(
        assigned("Rc", "p.n += 1;"),
        "cannot assign to data in an `Rc`"
    );
}

#[test]
fn an_operator_no_impl_covers_is_e0277() {
    assert_eq!(
        refused("fn main() {\n    let n: i32 = \"5\".parse()?;\n}\nmain();\n"),
        (
            "E0277".into(),
            "the `?` operator can only be used in a function that returns `Result` or `Option` \
             (or another type that implements `FromResidual`)"
                .into(),
            label("cannot use the `?` operator in a function that returns `()`")
        )
    );
    assert_eq!(
        refused("fn f(total: f64, n: usize) -> f64 {\n    total / n\n}\n"),
        (
            "E0277".into(),
            "cannot divide `f64` by `usize`".into(),
            label("no implementation for `f64 / usize`")
        )
    );
}

#[test]
fn errors_rustc_gives_without_a_code_print_a_plain_error() {
    let plain = |source: &str| {
        let error = Engine::new().compile(source).map(|_| ()).unwrap_err();
        let source = Source::new("main.rs", source).unwrap();
        let report = Report::from_parse(source, &error).unwrap();
        assert_eq!(report.code().as_str(), "");
        render(&report)
    };
    assert!(plain("fn main() {\n    printline!(\"hi\");\n}\nmain();\n")
        .starts_with("error: cannot find macro `printline` in this scope\n"));
    assert!(plain("fn call_me(num:) {}\n").starts_with("error: expected type, found `)`\n"));
    assert!(plain("fn sale_price(price: i64) -> {\n    price\n}\n")
        .starts_with("error: expected type, found `{`\n"));
    assert!(plain("fn compare(a: ???, b: ???) -> bool {\n    true\n}\n")
        .starts_with("error: expected identifier, found `,`\n"));
    assert!(plain("const NUMBER = 3;\nfn main() {}\n")
        .starts_with("error: missing type for `const` item\n"));
    // rustlings' tests1 and tests2 leave the condition for the learner.
    let assert = plain("fn main() {\n    assert!();\n}\n");
    assert!(
        assert.starts_with("error: macro requires a boolean expression as an argument\n"),
        "{assert}"
    );
    assert!(
        assert.contains("^^^^^^^^^ boolean expression required"),
        "{assert}"
    );
    assert!(plain("fn main() {\n    assert!( );\n}\n")
        .starts_with("error: macro requires a boolean expression as an argument\n"));
    assert!(plain("fn main() {\n    assert!(, \"m\");\n}\n")
        .starts_with("error: expected expression, found `,`\n"));
    // rustlings' tests2 leaves `assert_eq!()`'s operands.
    let assert_eq = plain("fn main() {\n    assert_eq!();\n}\n");
    assert!(
        assert_eq.starts_with("error: unexpected end of macro invocation\n"),
        "{assert_eq}"
    );
    assert!(
        assert_eq.contains("^^^^^^^^^^^^ missing tokens in macro arguments"),
        "{assert_eq}"
    );
    assert!(plain("fn main() {\n    assert_ne!(1,);\n}\n")
        .starts_with("error: unexpected end of macro invocation\n"));
    assert!(plain("fn main() {\n    assert_eq!(, 1);\n}\n")
        .starts_with("error: no rules expected `,`\n"));
}

#[test]
fn a_let_used_before_its_assignment_is_e0381() {
    // rustlings' variables3.
    assert_eq!(
        refused("fn main() {\n    let x: i32;\n    println!(\"Number {x}\");\n}\n"),
        (
            "E0381".into(),
            "used binding `x` isn't initialized".into(),
            label("`x` used here but it isn't initialized")
        )
    );
    // Only one branch gives it a value.
    assert_eq!(
        refused(
            "fn f(c: bool) -> i32 {\n    let x: i32;\n    if c {\n        x = 1;\n    }\n    x\n}\n"
        ),
        (
            "E0381".into(),
            "used binding `x` is possibly-uninitialized".into(),
            label("`x` used here but it is possibly-uninitialized")
        )
    );
    // A move after its assignment is E0382 as for any local.
    let (code, _, _) = refused(
        "fn main() {\n    let s;\n    s = String::from(\"a\");\n    let t = s;\n    \
         println!(\"{s} {t}\");\n}\n",
    );
    assert_eq!(code, "E0382");
}

#[test]
fn a_let_without_a_value_assigned_twice_is_e0384() {
    assert_eq!(
        refused("fn main() {\n    let x;\n    x = 1;\n    x = 2;\n    println!(\"{x}\");\n}\n"),
        (
            "E0384".into(),
            "cannot assign twice to immutable variable `x`".into(),
            label("cannot assign twice to immutable variable")
        )
    );
}

#[test]
fn a_let_without_a_value_or_a_type_where_one_is_needed_is_e0283() {
    // rustlings' variables2: the comparison needs `x`'s type.
    let (code, message, _) = refused(
        "fn main() {\n    let x;\n    if x == 10 {\n        println!(\"ten\");\n    }\n}\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0283", "type annotations needed")
    );
    let (code, _, _) = refused("fn main() {\n    let x;\n    let y = x + 1;\n}\n");
    assert_eq!(code, "E0284");
}

#[test]
fn a_borrow_assigned_to_a_let_without_a_value_is_checked() {
    // rustlings' lifetimes2: `string2` ends while `result` holds it.
    let (code, message, _) = refused(
        "fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {\n    \
         if x.len() > y.len() { x } else { y }\n}\n\
         fn main() {\n    let string1 = String::from(\"long string is long\");\n    \
         let result;\n    {\n        let string2 = String::from(\"xyz\");\n        \
         result = longest(&string1, &string2);\n    }\n    \
         println!(\"The longest string is '{result}'\");\n}\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0597", "`string2` does not live long enough")
    );
}

#[test]
fn a_type_nothing_settles_where_rustc_needs_it_is_e0282() {
    // rustlings' generics1: two `.into()`s give the element no type.
    assert_eq!(
        refused(
            "fn main() {\n    let mut numbers = Vec::new();\n    let n1: u8 = 42;\n    \
             numbers.push(n1.into());\n    let n2: i8 = -1;\n    numbers.push(n2.into());\n    \
             println!(\"{numbers:?}\");\n}\n"
        ),
        (
            "E0282".into(),
            "type annotations needed for `Vec<_>`".into(),
            None
        )
    );
    // rustlings' iterators1: a method of a local `todo!()` gave.
    let (code, message, _) = refused(
        "fn main() {\n    let mut it = todo!();\n    assert_eq!(it.next(), Some(&\"a\"));\n}\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0282", "type annotations needed")
    );
    // rustlings' threads1: a `Vec` nothing pushes to, whose elements are
    // formatted.
    assert_eq!(
        refused(
            "fn main() {\n    let mut results = Vec::new();\n    if results.len() != 10 {\n        \
             panic!(\"no\");\n    }\n    for (i, result) in results.into_iter().enumerate() {\n        \
             println!(\"{i} {result}\");\n    }\n}\n"
        ),
        (
            "E0282".into(),
            "type annotations needed for `Vec<_>`".into(),
            None
        )
    );
    // An `.into()` no use gives a target.
    let (code, message, _) = refused("fn main() {\n    let n: u8 = 1;\n    let x = n.into();\n}\n");
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0283", "type annotations needed")
    );
}

#[test]
fn a_macro_call_where_a_pattern_goes_is_e0532() {
    // rustlings' smart_pointers4.
    assert_eq!(
        refused("fn main() {\n    let x = Some(1);\n    assert!(matches!(x, todo!()));\n}\n"),
        (
            "E0532".into(),
            "expected a pattern, found a function call".into(),
            label("not a tuple struct or tuple variant")
        )
    );
}

#[test]
fn the_first_error_is_the_first_body_the_program_writes() {
    // rustlings' conversions4: an impl's empty body before `main`.
    let (code, _, label) = refused(
        "struct C;\nimpl TryFrom<i16> for C {\n    type Error = ();\n    \
         fn try_from(t: i16) -> Result<Self, Self::Error> {}\n}\n\
         fn main() {\n    let y: i32 = \"a\";\n}\n",
    );
    assert_eq!(code, "E0308");
    assert_eq!(
        label.as_deref(),
        Some("expected `Result<C, ()>`, found `()`")
    );
}
