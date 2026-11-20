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
    // A crate the program doesn't depend on is unresolved, whatever its
    // name (#1149).
    for call in [
        "tokio::spawn(async {})",
        "tokio::task::spawn_local(async {})",
    ] {
        assert_eq!(
            refused(&format!("fn main() {{\n    {call};\n}}\nmain();\n")),
            (
                "E0433".into(),
                "cannot find module or crate `tokio` in this scope".into(),
                label("use of unresolved module or unlinked crate `tokio`")
            )
        );
    }
}

#[test]
fn a_use_of_a_path_that_does_not_exist_is_e0432() {
    // rustc stops at a first segment that names no crate: E0433.
    assert_eq!(
        refused("use foo::bar;\nfn main() {}\n"),
        (
            "E0433".into(),
            "cannot find module or crate `foo` in this scope".into(),
            label("use of unresolved module or unlinked crate `foo`")
        )
    );
    // The whole import is marked, its label at the last segment.
    let source = "mod foo {}\nuse foo::bar;\nfn main() {}\n";
    let (code, message, _) = refused(source);
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0432", "unresolved import `foo::bar`")
    );
    let shown = rendered(source);
    assert!(
        shown.contains("  |     ^^^^^---\n  |          |\n  |          no `bar` in `foo`\n"),
        "{shown}"
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
    // std's macros, called or named without `!` (#1252).
    for (body, expected, name) in [
        ("print(\"hi\");", "function", "print"),
        ("println(\"hi\");", "function", "println"),
        ("std::print(\"hi\");", "function", "std::print"),
        ("let f = eprintln;", "value", "eprintln"),
    ] {
        assert_eq!(
            refused(&format!("fn main() {{\n    {body}\n}}\nmain();\n")),
            (
                "E0423".into(),
                format!("expected {expected}, found macro `{name}`"),
                label(&format!("not a {expected}"))
            ),
            "{body}"
        );
    }
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
fn a_let_without_a_value_assigned_on_a_path_that_may_have_assigned_it_is_e0384() {
    // After a branch that assigned it in one arm.
    assert_eq!(
        rendered(
            "fn main() {\n    let c = true;\n    let x;\n    if c { x = 1; }\n    x = 2;\n    \
             println!(\"{x}\");\n}\n"
        ),
        concat!(
            "error[E0384]: cannot assign twice to immutable variable `x`\n",
            " --> main.rs:5:5\n",
            "  |\n",
            "4 |     if c { x = 1; }\n",
            "  |            ----- first assignment to `x`\n",
            "5 |     x = 2;\n",
            "  |     ^^^^^ cannot assign twice to immutable variable\n",
            "  |\n",
            "help: consider making this binding mutable\n",
            "  |\n",
            "3 |     let mut x;\n",
            "  |         +++\n",
        )
    );
    // In a loop, whose next iteration assigns it again: no other
    // assignment to label.
    assert_eq!(
        rendered(
            "fn main() {\n    let x: i32;\n    loop {\n        if true { break; }\n        x = \
             1;\n    }\n}\n"
        ),
        concat!(
            "error[E0384]: cannot assign twice to immutable variable `x`\n",
            " --> main.rs:5:9\n",
            "  |\n",
            "5 |         x = 1;\n",
            "  |         ^^^^^ cannot assign twice to immutable variable\n",
            "  |\n",
            "help: consider making this binding mutable\n",
            "  |\n",
            "2 |     let mut x: i32;\n",
            "  |         +++\n",
        )
    );
    // A match arm's assignment after one before the match.
    let arm = rendered(
        "fn main() {\n    let n = 0;\n    let x;\n    x = 0;\n    match n {\n        0 => x = \
         10,\n        _ => {}\n    }\n}\n",
    );
    assert!(
        arm.starts_with(
            "error[E0384]: cannot assign twice to immutable variable `x`\n --> main.rs:6:14\n"
        ),
        "{arm}"
    );
    // Separate paths each assign it once, and a `let x;` in a loop is a
    // new local each iteration.
    let printed = |source: &str| {
        let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mut engine = Engine::new();
        let sink = lines.clone();
        engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
        engine.run(source).unwrap_or_else(|error| panic!("{error}"));
        lines.lock().unwrap().clone()
    };
    assert_eq!(
        printed(
            "fn branches(c: bool) -> i32 {\n    let x;\n    if c { x = 1; } else { x = 2; }\n    \
             x\n}\nfn arms(n: i32) -> i32 {\n    let x;\n    match n {\n        0 => x = 10,\n        \
             _ => x = 12,\n    }\n    x\n}\nfn each() -> i32 {\n    let mut total = 0;\n    for i \
             in 0..3 {\n        let y;\n        y = i * 2;\n        total += y;\n    }\n    \
             total\n}\nfn breaking() -> i32 {\n    let x;\n    loop {\n        x = 5;\n        \
             break;\n    }\n    x\n}\nprintln!(\"{} {} {} {}\", branches(true), arms(1), each(), \
             breaking());\n"
        ),
        "1 12 6 5\n"
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
fn a_diverging_variable_falls_back_to_never_before_its_bounds_are_checked() {
    // Edition 2024's fallback: the `if`'s type, which only `panic!()`
    // constrains, is `!`, and `!` isn't `Default`.
    let (code, message, _) = refused(
        "fn make<T: Default>() -> T {\n    T::default()\n}\nfn main() {\n    \
         let c = std::env::args().count() > 5;\n    \
         let x = if c { make() } else { panic!() };\n    let _ = x;\n}\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0277", "the trait bound `!: Default` is not satisfied")
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
    // A `let x;` a format string names before it has a value or a type:
    // the type check's E0282 comes before the borrow check's E0381 (#946).
    let (code, message, _) = refused("fn main() {\n    let x;\n    println!(\"{x}\");\n}\n");
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0282", "type annotations needed")
    );
    let (code, _, _) = refused("fn main() {\n    let x: i32;\n    println!(\"{x}\");\n}\n");
    assert_eq!(code, "E0381");
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

#[test]
fn the_old_engines_statements_are_rustc_syntax_errors() {
    assert_eq!(
        refused("fn main() {\n    try { 1 } catch (e) { 2 };\n}\n"),
        (
            "".into(),
            "keyword `catch` cannot follow a `try` block".into(),
            None
        )
    );
    assert_eq!(
        refused("fn main() {\n    let mut x = 0;\n    do { x += 1; } while x < 3;\n}\n"),
        (
            "".into(),
            "expected expression, found reserved keyword `do`".into(),
            label("expected expression")
        )
    );
    let eight = "expected one of `!`, `.`, `::`, `;`, `?`, `{`, `}`, or an operator";
    assert_eq!(
        refused("fn main() {\n    import \"m\" as m;\n}\n"),
        (
            "".into(),
            format!("{eight}, found `\"m\"`"),
            label("expected one of 8 possible tokens")
        )
    );
    assert_eq!(
        refused("fn main() {\n    let x = 1;\n    export x;\n}\n"),
        (
            "".into(),
            format!("{eight}, found `x`"),
            label("expected one of 8 possible tokens")
        )
    );
}

#[test]
fn the_old_engines_intrinsics_are_unknown_functions() {
    for name in [
        "__rawr_type_of",
        "__rawr_print",
        "__rawr_debug",
        "__rawr_eval",
        "__rawr_fn_ptr",
        "__rawr_is_shared",
        "__rawr_is_def_var",
        "__rawr_is_def_fn",
    ] {
        assert_eq!(
            refused(&format!("fn main() {{\n    let x = {name}(1);\n}}\n")),
            (
                "E0425".into(),
                format!("cannot find function `{name}` in this scope"),
                label("not found in this scope")
            )
        );
    }
}

#[test]
fn the_old_engines_symbols_are_rustc_syntax_errors() {
    assert_eq!(
        refused("fn main() {\n    let a = 1;\n    let b = a === 1;\n}\n"),
        ("".into(), "invalid comparison operator `===`".into(), None)
    );
    assert_eq!(
        refused("fn main() {\n    let a = 1;\n    let b = a !== 1;\n}\n"),
        ("".into(), "invalid comparison operator `!==`".into(), None)
    );
    assert_eq!(
        refused("fn main() {\n    let a := 1;\n}\n"),
        (
            "".into(),
            "expected type, found `=`".into(),
            label("expected type")
        )
    );
    assert_eq!(
        refused("fn main() {\n    let a:;\n}\n"),
        (
            "".into(),
            "expected type, found `;`".into(),
            label("expected type")
        )
    );
    let after_let = "expected one of `.`, `;`, `?`, `else`, or an operator";
    assert_eq!(
        refused(
            "fn f(a: Option<Option<i32>>) -> Option<i32> {\n    let b = a ?? 2;\n    b\n}\n\
             fn main() {}\n"
        ),
        (
            "".into(),
            format!("{after_let}, found `2`"),
            label(after_let)
        )
    );
    assert_eq!(
        refused("fn main() {\n    let v = vec![1];\n    let b = 1 !in v;\n}\n"),
        (
            "".into(),
            format!("{after_let}, found `!`"),
            label(after_let)
        )
    );
    assert_eq!(
        refused("fn main() {\n    let s = `hello`;\n}\n"),
        ("".into(), "unknown start of token: `".into(), None)
    );
}

#[test]
fn a_non_exhaustive_match_is_e0004() {
    // rustc's message, its label on the scrutinee, its note and its help.
    let missing = |declarations: &str, scrutinee: &str, arms: &str| {
        let source = format!(
            "{declarations}\nfn main() {{\n    let value = {scrutinee};\n    match value {{\n        \
             {arms}\n    }}\n}}\n"
        );
        let error = Engine::new()
            .compile(&source)
            .map(|_| ())
            .expect_err("rawr refuses a non-exhaustive match");
        let file = Source::new("main.rs", &source).expect("a test source has an identity");
        let report = Report::from_parse(file, &error).expect("the error is in the source");
        assert_eq!(report.code().as_str(), "E0004", "{source}");
        let label = report.labels().first().expect("a label on the scrutinee");
        assert_eq!(
            label.span().start(),
            crate::Position::new(4, 11),
            "{source}"
        );
        let label = label.message().unwrap_or_default().to_string();
        // The notes, and those after the enum's own note.
        let notes = report
            .notes()
            .map(str::to_string)
            .chain(
                report
                    .sections()
                    .iter()
                    .flat_map(|section| section.notes().iter().map(ToString::to_string)),
            )
            .collect::<Vec<_>>();
        // The suggestion's message, and the arm it adds.
        let suggestion = report.suggestions().first().expect("rustc suggests an arm");
        let arm = suggestion.edits()[0].replacement();
        let help = (
            suggestion.message().to_string(),
            arm.trim_start_matches(',').trim().to_string(),
        );
        (report.message().to_string(), label, notes, help)
    };
    let explicit = "ensure that all possible cases are being handled by adding a match arm with a \
                    wildcard pattern or an explicit pattern as shown";
    let or_patterns = "ensure that all possible cases are being handled by adding a match arm \
                       with a wildcard pattern, a match arm with multiple or-patterns as shown, \
                       or multiple match arms";
    let wildcard = "ensure that all possible cases are being handled by adding a match arm with a \
                    wildcard pattern as shown, or multiple match arms";
    let message = |uncovered: &str| format!("non-exhaustive patterns: {uncovered} not covered");

    // One variant.
    let (text, label, notes, help) = missing(
        "enum Message { Increment, Decrement }",
        "Message::Increment",
        "Message::Increment => {}",
    );
    assert_eq!(text, message("`Message::Decrement`"));
    assert_eq!(label, "pattern `Message::Decrement` not covered");
    assert_eq!(notes, ["the matched value is of type `Message`"]);
    assert_eq!(
        help,
        (
            explicit.to_string(),
            "Message::Decrement => todo!()".to_string()
        )
    );

    // Two variants, as one or-pattern; past three, `and N more` and `_`.
    let letters = "enum M { A, B, C, D, E }";
    let (text, label, _, help) = missing(
        letters,
        "M::A",
        "M::A => {}\n        M::B => {}\n        M::C => {}",
    );
    assert_eq!(text, message("`M::D` and `M::E`"));
    assert_eq!(label, "patterns `M::D` and `M::E` not covered");
    assert_eq!(
        help,
        (
            or_patterns.to_string(),
            "M::D | M::E => todo!()".to_string()
        )
    );
    let (text, label, _, help) = missing(letters, "M::A", "M::A => {}");
    assert_eq!(text, message("`M::B`, `M::C`, `M::D` and 1 more"));
    assert_eq!(
        label,
        "patterns `M::B`, `M::C`, `M::D` and 1 more not covered"
    );
    assert_eq!(help, (wildcard.to_string(), "_ => todo!()".to_string()));
    // A variant an arm names in part isn't reported while others are
    // missing, as in rustc.
    let (text, ..) = missing("enum N { X(bool), Y, Z }", "N::Y", "N::X(true) => {}");
    assert_eq!(text, message("`N::Y` and `N::Z`"));
    let (text, ..) = missing("enum M { A, B }", "(true, M::A)", "(true, M::A) => {}");
    assert_eq!(text, message("`(false, _)`"));

    // `Option`'s variants.
    let (text, _, notes, help) = missing("", "Some(1)", "Some(_) => {}");
    assert_eq!(text, message("`None`"));
    assert_eq!(notes, ["the matched value is of type `Option<i32>`"]);
    assert_eq!(help, (explicit.to_string(), "None => todo!()".to_string()));
    let (text, ..) = missing("", "Some(1)", "None => {}");
    assert_eq!(text, message("`Some(_)`"));
    let (text, ..) = missing("", "Some(1)", "Some(1) => {}");
    assert_eq!(text, message("`None`"));
    let (text, ..) = missing("", "&Some(true)", "Some(true) => {}");
    assert_eq!(text, message("`&None`"));

    // An integer's ranges, as rustc words them.
    let (text, label, notes, help) = missing("", "1", "1 => {}");
    assert_eq!(text, message("`i32::MIN..=0_i32` and `2_i32..=i32::MAX`"));
    assert_eq!(
        label,
        "patterns `i32::MIN..=0_i32` and `2_i32..=i32::MAX` not covered"
    );
    assert_eq!(notes, ["the matched value is of type `i32`"]);
    assert_eq!(
        help,
        (
            or_patterns.to_string(),
            "i32::MIN..=0_i32 | 2_i32..=i32::MAX => todo!()".to_string()
        )
    );
    let (text, ..) = missing(
        "",
        "1_u8",
        "0 => {}\n        5..=9 => {}\n        255 => {}",
    );
    assert_eq!(text, message("`1_u8..=4_u8` and `10_u8..=254_u8`"));
    let (text, ..) = missing("", "1_u8", "0..=254 => {}");
    assert_eq!(text, message("`u8::MAX`"));
    let (text, ..) = missing("", "1_usize", "5 => {}");
    assert_eq!(text, message("`0_usize..=4_usize` and `6_usize..`"));
    let (text, ..) = missing("", "1_isize", "0 => {}");
    assert_eq!(text, message("`..=-1_isize` and `1_isize..`"));
    let (text, ..) = missing(
        "",
        "1",
        "1 => {}\n        3 => {}\n        5 => {}\n        7 => {}",
    );
    assert_eq!(
        text,
        message("`i32::MIN..=0_i32`, `2_i32`, `4_i32` and 2 more")
    );
    let (text, ..) = missing("", "1", "-3 => {}\n        ..=-5 => {}\n        0.. => {}");
    assert_eq!(text, message("`-4_i32` and `-2_i32..=-1_i32`"));
    let (text, ..) = missing("", "1", "1 if true => {}");
    assert_eq!(text, message("`_`"));
    let (text, ..) = missing("", "'a'", "'a' => {}");
    assert_eq!(
        text,
        message("`'\\0'..='`'`, `'b'..='\\u{d7ff}'` and `'\\u{e000}'..='\\u{10ffff}'`")
    );
    let (text, ..) = missing("", "true", "true => {}");
    assert_eq!(text, message("`false`"));

    // A string, through its reference.
    let (text, label, notes, _) = missing("", "\"a\"", "\"a\" => {}");
    assert_eq!(text, message("`&_`"));
    assert_eq!(label, "pattern `&_` not covered");
    assert_eq!(notes, ["the matched value is of type `&str`"]);

    // Arms whose ranges cover every value need no `_`, as in rustc.
    Engine::new()
        .compile("fn f(x: u8) -> i32 {\n    match x {\n        0..=99 => 0,\n        100..=255 => 1,\n    }\n}\n")
        .map(|_| ())
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn a_refutable_pattern_in_a_let_or_a_for_is_e0005() {
    // rustc's message, its label on the pattern, its notes and its help.
    let refutable = |body: &str| {
        let source = format!("fn main() {{\n    let o = Some(1);\n    {body}\n}}\n");
        let error = Engine::new()
            .compile(&source)
            .map(|_| ())
            .map_or_else(|error| error, |()| panic!("rawr refuses {source}"));
        let file = Source::new("main.rs", &source).expect("a test source has an identity");
        let report = Report::from_parse(file, &error).expect("the error is in the source");
        assert_eq!(report.code().as_str(), "E0005", "{source}");
        // The pattern after `let ` or `for `, or a closure's `|`.
        let pattern = body.find('|').map_or(4, |bar| bar + 1);
        let label = report.labels().first().expect("a label on the pattern");
        assert_eq!(
            label.span().start(),
            crate::Position::new(3, 5 + u16::try_from(pattern).expect("a short body")),
            "{source}"
        );
        let label = label.message().unwrap_or_default().to_string();
        let notes = report.notes().map(str::to_string).collect::<Vec<_>>();
        let help = report
            .suggestions()
            .first()
            .map(|suggestion| suggestion.message().to_string());
        (report.message().to_string(), label, notes, help)
    };
    let inform = [
        "`let` bindings require an \"irrefutable pattern\", like a `struct` or an `enum` with \
         only one variant",
        "for more information, visit https://doc.rust-lang.org/book/ch19-02-refutability.html",
    ];

    let (text, label, notes, help) = refutable("let Some(x) = o;");
    assert_eq!(text, "refutable pattern in local binding");
    assert_eq!(label, "pattern `None` not covered");
    assert_eq!(
        notes,
        [
            inform[0],
            inform[1],
            "the matched value is of type `Option<i32>`"
        ]
    );
    assert_eq!(
        help.as_deref(),
        Some("you might want to use `let...else` to handle the variant that isn't matched")
    );
    // A pattern that binds nothing is `if let`'s.
    let (_, label, _, help) = refutable("let None = o;");
    assert_eq!(label, "pattern `Some(_)` not covered");
    assert_eq!(
        help.as_deref(),
        Some("you might want to use `if let` to ignore the variant that isn't matched")
    );
    // `for`: no `let` notes and no help.
    let (text, label, notes, help) = refutable("for Some(x) in vec![o] {}");
    assert_eq!(text, "refutable pattern in `for` loop binding");
    assert_eq!(label, "pattern `None` not covered");
    assert_eq!(notes, ["the matched value is of type `Option<i32>`"]);
    assert_eq!(help, None);
    // A literal, through a reference, and the witnesses of an integer.
    let (_, label, notes, _) = refutable("let &Some(x) = &o;");
    assert_eq!(label, "pattern `&None` not covered");
    assert_eq!(notes[2], "the matched value is of type `&Option<i32>`");
    let (_, label, _, help) = refutable("let 1 = 2;");
    assert_eq!(
        label,
        "patterns `i32::MIN..=0_i32` and `2_i32..=i32::MAX` not covered"
    );
    assert_eq!(
        help.as_deref(),
        Some("you might want to use `if let` to ignore the variants that aren't matched")
    );
    // A closure's parameter is a function argument.
    let (text, label, notes, help) = refutable("let f = |Some(x): Option<i32>| x;");
    assert_eq!(text, "refutable pattern in function argument");
    assert_eq!(label, "pattern `None` not covered");
    assert_eq!(notes, ["the matched value is of type `Option<i32>`"]);
    assert_eq!(help, None);
    // So is a function's.
    assert_eq!(
        refused("fn f(Some(x): Option<i32>) -> i32 {\n    x\n}\nfn main() {}\n"),
        (
            "E0005".into(),
            "refutable pattern in function argument".into(),
            Some("pattern `None` not covered".into())
        )
    );

    // Irrefutable patterns, `let else` and `if let` are fine.
    Engine::new()
        .compile(
            "struct P { x: i32 }\nstruct W(i32);\nenum One { Only(i32) }\nfn main() {\n    \
             let o = Some(1);\n    let P { x } = P { x: 1 };\n    let W(w) = W(2);\n    \
             let One::Only(n) = One::Only(3);\n    let (a, _) = (1, 2);\n    let _ = o;\n    \
             let Some(y) = o else { return };\n    if let Some(z) = o {\n        \
             println!(\"{x} {w} {n} {a} {y} {z}\");\n    }\n}\n",
        )
        .map(|_| ())
        .unwrap_or_else(|error| panic!("{error}"));
}

struct Style;

/// A facade `Style` whose `width` and `height` a pattern reads.
fn style_facade() -> crate::host::facade::Definition {
    use crate::host::{facade, item, signature, type_ref};
    let mut builder = facade::Builder::new("ui", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Style>("Style", &[], &[]).unwrap();
    let mut default = signature::Builder::new();
    default.output(type_ref::Type::registered("Style").unwrap());
    builder
        .item(item::Definition::associated_function("Style::default", default.finish()).unwrap())
        .unwrap();
    for field in ["width", "height"] {
        let mut read = signature::Builder::new();
        read.output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(
                item::Definition::method(
                    format!("Style::{field}"),
                    crate::receiver::Mode::Shared,
                    read.finish(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    builder.finish().unwrap()
}

/// rustc's code, message, label on the pattern and help for `source`,
/// which `engine` refuses.
fn report(engine: &Engine, source: &str) -> (String, String, String, Option<String>) {
    let error = engine
        .compile(source)
        .map(|_| ())
        .map_or_else(|error| error, |()| panic!("rawr refuses {source}"));
    let file = Source::new("main.rs", source).expect("a test source has an identity");
    let report = Report::from_parse(file, &error).expect("the error is in the source");
    let label = report
        .labels()
        .first()
        .and_then(|label| label.message())
        .unwrap_or_default()
        .to_string();
    (
        report.code().as_str().to_string(),
        report.message().to_string(),
        label,
        report.help().map(str::to_string),
    )
}

#[test]
fn refutable_patterns_rawr_lowered_elsewhere_are_e0005() {
    let refutable = |engine: &Engine, source: &str| {
        let (code, message, label, _) = report(engine, source);
        assert_eq!(code, "E0005", "{source}");
        (message, label)
    };
    let local = "refutable pattern in local binding".to_string();
    let engine = Engine::new();
    // A `let` with `ref mut` and a refutable part.
    assert_eq!(
        refutable(
            &engine,
            "fn main() {\n    let mut o = Some(1);\n    let Some(ref mut x) = o;\n}\n"
        ),
        (local.clone(), "pattern `None` not covered".into())
    );
    assert_eq!(
        refutable(
            &engine,
            "fn main() {\n    let mut t = (1, Some(2));\n    let (ref mut x, None) = t;\n}\n"
        ),
        (local.clone(), "pattern `(_, Some(_))` not covered".into())
    );
    assert_eq!(
        engine
            .eval::<i64>(
                "enum One { Only(i64) }\nfn main() -> i64 {\n    let mut t = (1, One::Only(2));\n    \
                 let (ref mut a, One::Only(b)) = t;\n    *a += b;\n    t.0\n}\nmain()\n"
            )
            .unwrap_or_else(|error| panic!("{error}")),
        3
    );
    // A slice pattern on a slice of unknown length, irrefutable only as a
    // rest pattern with no required element.
    let slice = |pattern: &str| {
        refutable(
            &engine,
            &format!(
                "fn main() {{\n    let v = vec![1i64];\n    let s: &[i64] = &v;\n    \
                 let {pattern} = s;\n}}\n"
            ),
        )
    };
    assert_eq!(
        slice("[a, ..]"),
        (local.clone(), "pattern `&[]` not covered".into())
    );
    assert_eq!(slice("[]").1, "pattern `&[_, ..]` not covered");
    assert_eq!(
        slice("[a, b]").1,
        "patterns `&[]`, `&[_]` and `&[_, _, _, ..]` not covered"
    );
    assert_eq!(
        engine
            .eval::<i64>(
                "fn main() -> i64 {\n    let v = vec![1i64, 2, 3];\n    let s: &[i64] = &v;\n    \
                 let [..] = s;\n    let [all @ ..] = s;\n    all.len() as i64 + all[2]\n}\nmain()\n"
            )
            .unwrap_or_else(|error| panic!("{error}")),
        6
    );
    // A native struct pattern with refutable fields, its witness in place
    // in what is matched.
    let mut native = Engine::new();
    native.register_host_facade(style_facade()).unwrap();
    let style = |body: &str| {
        refutable(
            &native,
            &format!("use ui::Style;\npub fn f(s: Style) -> i64 {{\n    {body}\n    0\n}}\n"),
        )
    };
    assert_eq!(
        style("let Style { width: 1, height } = s;"),
        (
            local,
            "patterns `Style { width: i64::MIN..=0_i64, .. }` and \
             `Style { width: 2_i64..=i64::MAX, .. }` not covered"
                .into()
        )
    );
    assert_eq!(
        style("let (Style { height: 0.., .. }, n) = (s, 2);").1,
        "pattern `(Style { height: i64::MIN..=-1_i64, .. }, _)` not covered"
    );
    assert_eq!(
        style("for Style { width: 0, .. } in vec![s] {}"),
        (
            "refutable pattern in `for` loop binding".into(),
            "patterns `Style { width: i64::MIN..=-1_i64, .. }` and \
             `Style { width: 1_i64..=i64::MAX, .. }` not covered"
                .into()
        )
    );
    native
        .compile_executable(
            "use ui::Style;\npub fn f(s: Style) -> i64 {\n    let Style { width, .. } = s;\n    \
             width\n}\n",
        )
        .unwrap_or_else(|error| panic!("{error}"));
    // A user enum's variant in a function parameter.
    assert_eq!(
        refutable(
            &engine,
            "enum E { A(i64), B }\nfn f(E::A(x): E) -> i64 {\n    x\n}\nfn main() {}\n"
        ),
        (
            "refutable pattern in function argument".into(),
            "pattern `E::B` not covered".into()
        )
    );
    assert_eq!(
        engine
            .eval::<i64>(
                "enum E { A(i64) }\nfn f(E::A(x): E) -> i64 {\n    x + 1\n}\nf(E::A(41))\n"
            )
            .unwrap_or_else(|error| panic!("{error}")),
        42
    );
}

#[test]
fn a_tuple_pattern_of_the_wrong_field_count_is_e0023() {
    let engine = Engine::new();
    let source =
        "enum Message { Move(i64, i64) }\nfn main() {\n    let m = Message::Move(1, 2);\n    \
                  match m {\n        Message::Move(value) => {}\n    }\n}\n";
    let (code, message, label, help) = report(&engine, source);
    assert_eq!(
        (code.as_str(), message.as_str(), label.as_str()),
        (
            "E0023",
            "this pattern has 1 field, but the corresponding tuple variant has 2 fields",
            "expected 2 fields, found 1"
        )
    );
    assert_eq!(
        help.as_deref(),
        Some("use `_` to explicitly ignore each field: `Message::Move(value, _)`")
    );
    // The label under the field patterns, and rustc's on the declared
    // fields.
    let error = engine.compile(source).map(|_| ()).unwrap_err();
    let file = Source::new("main.rs", source).unwrap();
    let rendered = render(&Report::from_parse(file, &error).unwrap());
    assert!(
        rendered.contains("|                       ^^^^^ expected 2 fields, found 1"),
        "{rendered}"
    );
    assert!(
        rendered.contains("|                     ---  --- tuple variant has 2 fields"),
        "{rendered}"
    );
    // One field that is a tuple of as many: the help wraps them.
    let (code, message, label, help) = report(
        &engine,
        "enum Message { Move((i64, i64)), Quit }\nfn main() {\n    let m = Message::Quit;\n    \
         match m {\n        Message::Move(x, y) => {}\n        _ => {}\n    }\n}\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str(), label.as_str()),
        (
            "E0023",
            "this pattern has 2 fields, but the corresponding tuple variant has 1 field",
            "expected 1 field, found 2"
        )
    );
    assert_eq!(
        help.as_deref(),
        Some("missing parentheses: `Message::Move((x, y))`")
    );
    // A tuple struct, in a `let`, a `match` and a parameter.
    for body in [
        "fn main() {\n    let p = Point(1, 2);\n    let Point(x) = p;\n}\n",
        "fn main() {\n    let p = Point(1, 2);\n    match p {\n        Point(x) => {}\n    }\n}\n",
        "fn f(Point(x): Point) {}\nfn main() {}\n",
    ] {
        let source = format!("struct Point(i64, i64);\n{body}");
        let (code, message, label, help) = report(&engine, &source);
        assert_eq!(
            (code.as_str(), message.as_str(), label.as_str()),
            (
                "E0023",
                "this pattern has 1 field, but the corresponding tuple struct has 2 fields",
                "expected 2 fields, found 1"
            ),
            "{source}"
        );
        assert_eq!(
            help.as_deref(),
            Some("use `_` to explicitly ignore each field: `Point(x, _)`")
        );
    }
}

/// The report rawr renders for `source`, which it refuses.
fn rendered(source: &str) -> String {
    let error = Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rawr refuses this program");
    let evidence = Source::new("main.rs", source).expect("a test source has an identity");
    render(&Report::from_parse(evidence, &error).expect("the error is in the source"))
}

/// A `use` in a block is in scope in that block alone: outside it the
/// names it binds are rustc's E0425 (#7). `cargo run` of the programs
/// rawr runs prints `true 1 2`.
#[test]
fn a_blocks_use_is_out_of_scope_outside_the_block() {
    let color = "#[derive(PartialEq)]\nenum Color {\n    Red,\n    Green,\n}\n";
    let outside = format!(
        "{color}fn inside() -> bool {{\n    use Color::*;\n    Red == Red && Green == Green\n}}\n\
         fn outside() -> bool {{\n    Red == Color::Red\n}}\nfn main() {{\n    \
         println!(\"{{}} {{}}\", inside(), outside());\n}}\n"
    );
    assert_eq!(
        refused(&outside),
        (
            "E0425".into(),
            "cannot find value `Red` in this scope".into(),
            label("not found in this scope")
        )
    );
    assert!(rendered(&outside).contains(" --> main.rs:11:5"));
    assert_eq!(
        refused(
            "mod m {\n    pub fn helper() -> i32 {\n        1\n    }\n}\nfn a() -> i32 {\n    use \
             m::helper;\n    helper()\n}\nfn b() -> i32 {\n    helper()\n}\nfn main() {}\n"
        ),
        (
            "E0425".into(),
            "cannot find function `helper` in this scope".into(),
            label("not found in this scope")
        )
    );
    assert_eq!(
        refused(
            "fn a() {\n    use std::collections::HashMap;\n    let _m: HashMap<i32, i32> = \
             HashMap::new();\n}\nfn b() {\n    let _m: HashMap<i32, i32> = HashMap::new();\n}\nfn \
             main() {}\n"
        ),
        (
            "E0425".into(),
            "cannot find type `HashMap` in this scope".into(),
            label("not found in this scope")
        )
    );
    // In scope in its block, the blocks and items inside it, and in each
    // block that writes the same `use`.
    let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut engine = Engine::new();
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    engine
        .run(&format!(
            "{color}mod m {{\n    pub fn one() -> i32 {{\n        1\n    }}\n}}\nfn first() -> bool \
             {{\n    use Color::*;\n    let check = || {{ Red != Green }};\n    fn inner() -> Color \
             {{\n        Green\n    }}\n    check() && inner() == Green\n}}\nfn second() -> i32 {{\n    \
             use Color::*;\n    use m::one;\n    {{\n        if Red == Red {{ one() }} else {{ 0 }}\n    \
             }}\n}}\nfn third() -> i32 {{\n    use m::one;\n    one() + 1\n}}\nfn main() {{\n    \
             println!(\"{{}} {{}} {{}}\", first(), second(), third());\n}}\nmain();\n"
        ))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(lines.lock().unwrap().as_str(), "true 1 2\n");
}

/// Only a box unsizes to a `Box<dyn Error>`: a bare value where one is
/// expected is rustc's E0308 at the value, with its `Box::new` help for an
/// error and its `Into::into` help for text (#6). `cargo run` of the
/// program rawr runs prints `oops oops 5 true`.
#[test]
fn a_value_where_a_box_dyn_error_is_expected_is_e0308() {
    let error = "use std::error::Error;\nuse std::fmt;\n#[derive(Debug)]\nstruct Oops;\nimpl \
                 fmt::Display for Oops {\n    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result \
                 {\n        write!(f, \"oops\")\n    }\n}\nimpl Error for Oops {}\nfn take(_: \
                 Box<dyn Error>) {}\n";
    let note = "   = note: for more on the distinction between the stack and the heap, read \
                https://doc.rust-lang.org/book/ch15-01-box.html, \
                https://doc.rust-lang.org/rust-by-example/std/box.html, and \
                https://doc.rust-lang.org/std/boxed/index.html\n";
    // rustc's section of the suggestion: the line it makes and its markers.
    let boxed = |line: &str, marker: &str| {
        format!(
            "{note}help: store this in the heap by calling `Box::new`\n   |\n13 |     {line}\n   |     \
             {marker}\n"
        )
    };
    let into = |ty: &str, line: &str, marker: &str| {
        format!(
            "help: call `Into::into` on this expression to convert `{ty}` into `Box<dyn \
             std::error::Error>`\n   |\n13 |     {line}\n   |     {marker}\n"
        )
    };
    for (body, at, found, help) in [
        (
            "fn fail() -> Box<dyn Error> {\n    Oops\n}\n",
            "13:5",
            "`Oops`",
            boxed("Box::new(Oops)", "+++++++++    +"),
        ),
        (
            "fn main() {\n    let e: Box<dyn Error> = Oops;\n}\n",
            "13:29",
            "`Oops`",
            boxed(
                "let e: Box<dyn Error> = Box::new(Oops);",
                "                        +++++++++    +",
            ),
        ),
        (
            "fn main() {\n    take(Oops);\n}\n",
            "13:10",
            "`Oops`",
            boxed("take(Box::new(Oops));", "     +++++++++    +"),
        ),
        (
            "fn fail() -> Result<(), Box<dyn Error>> {\n    Err(Oops)\n}\n",
            "13:9",
            "`Oops`",
            boxed("Err(Box::new(Oops))", "    +++++++++    +"),
        ),
        (
            "fn main() {\n    let e: Box<dyn Error> = \"x\";\n}\n",
            "13:29",
            "`&str`",
            into(
                "&'static str",
                "let e: Box<dyn Error> = \"x\".into();",
                "                           +++++++",
            ),
        ),
        (
            "fn main() {\n    let e: Box<dyn Error> = String::new();\n}\n",
            "13:29",
            "`String`",
            into(
                "String",
                "let e: Box<dyn Error> = String::new().into();",
                "                                     +++++++",
            ),
        ),
        (
            "fn main() {\n    let e: Box<dyn Error> = 5;\n}\n",
            "13:29",
            "integer",
            String::new(),
        ),
    ] {
        let source = format!("{error}{body}");
        let report = rendered(&source);
        assert!(
            report.starts_with(&format!(
                "error[E0308]: mismatched types\n  --> main.rs:{at}"
            )),
            "{report}"
        );
        // rustc's gutter closes the snippet before its notes and help.
        let help = if help.is_empty() {
            help
        } else {
            format!("   |\n{help}")
        };
        assert!(
            report.ends_with(&format!("expected `Box<dyn Error>`, found {found}\n{help}")),
            "{report}"
        );
    }
    // A box of an error unsizes, and `?` and `.into()` convert.
    let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut engine = Engine::new();
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    engine
        .run(&format!(
            "{error}fn parse() -> Result<i32, Box<dyn Error>> {{\n    let n: i32 = \"5\".parse()?;\n    \
             Ok(n)\n}}\nfn fail() -> Result<(), Box<dyn Error>> {{\n    Err(\"bad\".into())\n}}\nfn \
             main() {{\n    let e: Box<dyn Error> = Box::new(Oops);\n    take(Box::new(Oops));\n    \
             let r: Result<(), Box<dyn Error>> = Err(Box::new(Oops));\n    println!(\"{{}} {{}} {{}} \
             {{}}\", e, r.unwrap_err(), parse().unwrap(), fail().is_err());\n}}\nmain();\n"
        ))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(lines.lock().unwrap().as_str(), "oops oops 5 true\n");
}

/// `write!` calls the `write_fmt` of a `Write` trait, which is callable
/// only where the trait is in scope: rustc's E0599 "cannot write into
/// `String`" at the value written into, with no `use std::fmt::Write;`
/// in its module or its block (#5).
#[test]
fn a_write_with_no_write_trait_in_scope_is_e0599() {
    let string =
        "fn main() {\n    let mut s = String::new();\n    write!(s, \"{}\", 1).unwrap();\n}\n";
    assert_eq!(
        refused(string),
        ("E0599".into(), "cannot write into `String`".into(), None)
    );
    let report = rendered(string);
    assert!(report.contains(" --> main.rs:3:12"), "{report}");
    assert!(
        report.ends_with(concat!(
            "  = note: must implement `io::Write`, `fmt::Write`, or have a `write_fmt` method\n",
            "help: trait `Write` which provides `write_fmt` is implemented but not in scope; \
             perhaps you want to import it\n",
            "  |\n",
            "1 + use std::fmt::Write;\n",
            "  |\n",
        )),
        "{report}"
    );
    // `&mut s`, and a `use` in another function's block.
    let report = rendered(
        "fn other() {\n    use std::fmt::Write;\n}\nfn main() {\n    let mut s = String::new();\n    \
         writeln!(&mut s, \"{}\", 1).unwrap();\n}\n",
    );
    assert!(
        report.starts_with("error[E0599]: cannot write into `String`\n --> main.rs:6:14"),
        "{report}"
    );
    // A type of the program's that implements the trait by its path.
    assert_eq!(
        refused(
            "struct W(String);\nimpl std::fmt::Write for W {\n    fn write_str(&mut self, s: &str) \
             -> std::fmt::Result {\n        self.0.push_str(s);\n        Ok(())\n    }\n}\nfn main() \
             {\n    let mut w = W(String::new());\n    writeln!(w, \"{}\", 2).unwrap();\n}\n"
        ),
        ("E0599".into(), "cannot write into `W`".into(), None)
    );
    // In scope from the module, the block, an inline module's `use` and an
    // anonymous import, rawr writes as rustc does.
    for source in [
        "use std::fmt::Write;\nfn main() {\n    let mut s = String::new();\n    write!(s, \"{}\", \
         1).unwrap();\n}\n",
        "fn main() {\n    use std::fmt::Write as _;\n    let mut s = String::new();\n    write!(s, \
         \"{}\", 1).unwrap();\n}\n",
        "mod m {\n    use std::fmt::Write;\n    pub fn f() -> String {\n        let mut s = \
         String::new();\n        write!(s, \"x\").unwrap();\n        s\n    }\n}\nfn main() {\n    \
         m::f();\n}\n",
        "use std::fmt::{self, Write};\nstruct W(String);\nimpl Write for W {\n    fn write_str(&mut \
         self, s: &str) -> fmt::Result {\n        self.0.push_str(s);\n        Ok(())\n    }\n}\nfn \
         main() {\n    let mut w = W(String::new());\n    writeln!(w, \"{}\", 2).unwrap();\n}\n",
    ] {
        Engine::new()
            .compile(source)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}

/// An or-pattern's alternatives bind the same names (E0408), each of one
/// type (E0308), as rustc requires.
#[test]
fn an_or_pattern_binding_a_name_in_one_alternative_is_e0408() {
    for alternatives in ["Some(x) | None", "None | Some(x)"] {
        let source = format!(
            "fn main() {{\n    let o: Option<i64> = None;\n    match o {{\n        \
             {alternatives} => {{}}\n    }}\n}}\n"
        );
        assert_eq!(
            refused(&source),
            (
                "E0408".into(),
                "variable `x` is not bound in all patterns".into(),
                label("pattern doesn't bind `x`")
            ),
            "{source}"
        );
        assert!(
            rendered(&source).contains("variable not in all patterns"),
            "{}",
            rendered(&source)
        );
    }
    assert_eq!(
        refused(
            "fn main() {\n    let r: Result<i64, bool> = Ok(1);\n    match r {\n        \
             Ok(x) | Err(x) => {}\n    }\n}\n"
        ),
        (
            "E0308".into(),
            "mismatched types".into(),
            label("expected `i64`, found `bool`")
        )
    );
}

#[test]
fn negating_an_unsigned_value_is_e0600() {
    // A literal and a local alike: unsigned integers have no `Neg` (#945).
    for source in [
        "fn main() {\n    let u: u8 = -1;\n}\n",
        "fn main() {\n    let x: u8 = 3;\n    let y = -x;\n}\n",
        "fn main() {\n    let n = 4usize;\n    let m = -n;\n}\n",
    ] {
        let (code, message, label) = refused(source);
        assert_eq!(code, "E0600", "{source}");
        assert!(
            message.starts_with("cannot apply unary operator `-` to type `u"),
            "{message}"
        );
        assert_eq!(label, Some("cannot apply unary operator `-`".into()));
    }
}

#[test]
fn assigning_to_a_field_through_a_shared_receiver_is_e0594() {
    // `&self`, and a `self` not declared `mut` (#1625).
    assert_eq!(
        refused(
            "struct Counter { value: i64 }\nimpl Counter {\n    fn reset(&self) {\n        \
             self.value += 1;\n    }\n}\nfn main() {}\n"
        ),
        (
            "E0594".into(),
            "cannot assign to `self.value`, which is behind a `&` reference".into(),
            label("`self` is a `&` reference, so the data it refers to cannot be written")
        )
    );
    let (code, message, _) = refused(
        "struct Counter { value: i64 }\nimpl Counter {\n    fn reset(self) {\n        \
         self.value = 0;\n    }\n}\nfn main() {}\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "E0594",
            "cannot assign to `self.value`, as `self` is not declared as mutable"
        )
    );
    // A `&mut self` method of a field called through `&self` is E0596.
    assert_eq!(
        refused(
            "struct Child { value: i64 }\nimpl Child {\n    fn bump(&mut self) {\n        \
             self.value += 1;\n    }\n}\nstruct Parent { child: Child }\nimpl Parent {\n    \
             fn bump(&self) {\n        self.child.bump();\n    }\n}\nfn main() {}\n"
        ),
        (
            "E0596".into(),
            "cannot borrow `self.child` as mutable, as it is behind a `&` reference".into(),
            label(
                "`self` is a `&` reference, so the data it refers to cannot be borrowed as \
                 mutable"
            )
        )
    );
}

#[test]
fn changing_a_collection_behind_a_shared_reference_is_e0596() {
    // `&self` of an impl for `Vec`, and a local `&Vec` (#938).
    assert_eq!(
        refused(
            "trait T { fn f(&self); }\nimpl T for Vec<i32> { fn f(&self) { self.push(1) } }\n\
             fn main() {}\n"
        ),
        (
            "E0596".into(),
            "cannot borrow `*self` as mutable, as it is behind a `&` reference".into(),
            label("`self` is a `&` reference, so it cannot be borrowed as mutable")
        )
    );
    assert_eq!(
        refused("fn main() {\n    let mut v = vec![1];\n    let r = &v;\n    r.push(1);\n}\n"),
        (
            "E0596".into(),
            "cannot borrow `*r` as mutable, as it is behind a `&` reference".into(),
            label("`r` is a `&` reference, so it cannot be borrowed as mutable")
        )
    );
}

#[test]
fn a_move_into_an_owning_parameter_is_e0382_with_rustcs_note_and_clone_help() {
    // rustlings' move_semantics2.
    assert_eq!(
        rendered(
            "fn fill_vec(vec: Vec<i32>) -> Vec<i32> {\n    vec\n}\n\nfn main() {\n    let vec0 = \
             vec![22, 44, 66];\n    let vec1 = fill_vec(vec0);\n    assert_eq!(vec0, [22, 44, \
             66]);\n    assert_eq!(vec1, [22, 44, 66]);\n}\n"
        ),
        concat!(
            "error[E0382]: borrow of moved value: `vec0`\n",
            " --> main.rs:8:5\n",
            "  |\n",
            "6 |     let vec0 = vec![22, 44, 66];\n",
            "  |         ---- move occurs because `vec0` has type `Vec<i32>`, which does not \
             implement the `Copy` trait\n",
            "7 |     let vec1 = fill_vec(vec0);\n",
            "  |                         ---- value moved here\n",
            "8 |     assert_eq!(vec0, [22, 44, 66]);\n",
            "  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ value borrowed here after move\n",
            "  |\n",
            "note: consider changing this parameter type in function `fill_vec` to borrow instead \
             if owning the value isn't necessary\n",
            " --> main.rs:1:18\n",
            "  |\n",
            "1 | fn fill_vec(vec: Vec<i32>) -> Vec<i32> {\n",
            "  |    --------      ^^^^^^^^ this parameter takes ownership of the value\n",
            "  |    |\n",
            "  |    in this function\n",
            "help: consider cloning the value if the performance cost is acceptable\n",
            "  |\n",
            "7 |     let vec1 = fill_vec(vec0.clone());\n",
            "  |                             ++++++++\n",
        )
    );
    // A method's parameter, and a `&` of the moved local.
    assert_eq!(
        rendered(
            "struct S;\nimpl S {\n    fn eat(&self, v: Vec<i32>) -> usize {\n        v.len()\n    \
             }\n}\nfn main() {\n    let v = vec![1];\n    S.eat(v);\n    let r = &v;\n}\n",
        ),
        concat!(
            "error[E0382]: borrow of moved value: `v`\n",
            "  --> main.rs:10:13\n",
            "   |\n",
            " 8 |     let v = vec![1];\n",
            "   |         - move occurs because `v` has type `Vec<i32>`, which does not implement \
             the `Copy` trait\n",
            " 9 |     S.eat(v);\n",
            "   |           - value moved here\n",
            "10 |     let r = &v;\n",
            "   |             ^^ value borrowed here after move\n",
            "   |\n",
            "note: consider changing this parameter type in method `eat` to borrow instead if \
             owning the value isn't necessary\n",
            "  --> main.rs:3:22\n",
            "   |\n",
            " 3 |     fn eat(&self, v: Vec<i32>) -> usize {\n",
            "   |        ---           ^^^^^^^^ this parameter takes ownership of the value\n",
            "   |        |\n",
            "   |        in this method\n",
            "help: consider cloning the value if the performance cost is acceptable\n",
            "   |\n",
            " 9 |     S.eat(v.clone());\n",
            "   |            ++++++++\n",
        )
    );
    // A generic parameter: no note.
    let generic = rendered(
        "fn take<T>(t: T) -> T {\n    t\n}\nfn main() {\n    let s = String::new();\n    \
         take(s);\n    let t = s;\n}\n",
    );
    assert!(!generic.contains("this parameter"), "{generic}");
    assert!(
        generic.ends_with(
            "help: consider cloning the value if the performance cost is acceptable\n  |\n6 |     \
             take(s.clone());\n  |           ++++++++\n"
        ),
        "{generic}"
    );
}

/// rustc's note on a moved value whose type is the program's own and not
/// `Clone` marks the type's definition and the move a clone would keep
/// the value through, after the note on the parameter that took it.
#[test]
fn a_moved_value_that_is_not_clone_gets_rustcs_note_instead_of_the_clone_help() {
    let report = rendered(
        "#[derive(Debug)]\nstruct N;\nfn main() {\n    let n = N;\n    let m = n;\n    \
         println!(\"{n:?} {m:?}\");\n}\n",
    );
    assert!(
        report.ends_with(concat!(
            "  |\n",
            "note: if `N` implemented `Clone`, you could clone the value\n",
            " --> main.rs:2:1\n",
            "  |\n",
            "2 | struct N;\n",
            "  | ^^^^^^^^ consider implementing `Clone` for this type\n",
            "...\n",
            "5 |     let m = n;\n",
            "  |             - you could clone this value\n",
        )),
        "{report}"
    );
    assert!(!report.contains("help"), "{report}");
    assert_eq!(
        rendered(
            "#[derive(Debug)]\nstruct N;\nfn take(n: N) {}\nfn main() {\n    let n = N;\n    \
             take(n);\n    println!(\"{n:?}\");\n}\n"
        ),
        concat!(
            "error[E0382]: borrow of moved value: `n`\n",
            " --> main.rs:7:16\n",
            "  |\n",
            "5 |     let n = N;\n",
            "  |         - move occurs because `n` has type `N`, which does not implement the \
             `Copy` trait\n",
            "6 |     take(n);\n",
            "  |          - value moved here\n",
            "7 |     println!(\"{n:?}\");\n",
            "  |                ^ value borrowed here after move\n",
            "  |\n",
            "note: consider changing this parameter type in function `take` to borrow instead if \
             owning the value isn't necessary\n",
            " --> main.rs:3:12\n",
            "  |\n",
            "3 | fn take(n: N) {}\n",
            "  |    ----    ^ this parameter takes ownership of the value\n",
            "  |    |\n",
            "  |    in this function\n",
            "note: if `N` implemented `Clone`, you could clone the value\n",
            " --> main.rs:2:1\n",
            "  |\n",
            "2 | struct N;\n",
            "  | ^^^^^^^^ consider implementing `Clone` for this type\n",
            "...\n",
            "6 |     take(n);\n",
            "  |          - you could clone this value\n",
        )
    );
}

#[test]
fn a_use_after_a_partial_move_notes_the_moved_parts_type() {
    assert_eq!(
        rendered(
            "struct P {\n    a: String,\n    b: String,\n}\nfn main() {\n    let p = P { a: \
             String::new(), b: String::new() };\n    let a = p.a;\n    let q = p;\n}\n"
        ),
        concat!(
            "error[E0382]: use of partially moved value: `p`\n",
            " --> main.rs:8:13\n",
            "  |\n",
            "7 |     let a = p.a;\n",
            "  |             --- value partially moved here\n",
            "8 |     let q = p;\n",
            "  |             ^ value used here after partial move\n",
            "  |\n",
            "  = note: partial move occurs because `p.a` has type `String`, which does not \
             implement the `Copy` trait\n",
        )
    );
}

#[test]
fn a_moved_local_whose_literal_type_is_unsettled_names_rustcs_fallback() {
    let report = rendered(
        "fn main() {\n    let v = vec![1];\n    drop(v);\n    println!(\"{:?}\", v);\n}\n",
    );
    assert!(report.contains("has type `Vec<i32>`"), "{report}");
}

#[test]
fn a_mismatched_argument_is_underlined_whole() {
    // #1729: the argument, not its first character.
    let report = rendered(
        "fn main() {\n    let a: Option<String> = None;\n    let _ = a == Some(\"x\");\n}\n",
    );
    assert!(
        report.contains(
            " --> main.rs:3:23\n  |\n3 |     let _ = a == Some(\"x\");\n  |                       \
             ^^^ expected `String`, found `&str`\n"
        ),
        "{report}"
    );
    let report = rendered("fn take(s: String) {}\nfn main() {\n    take(1 + 23);\n}\n");
    assert!(
        report.contains("  |          ^^^^^^ expected `String`, found integer"),
        "{report}"
    );
}

#[test]
fn an_operation_known_to_overflow_or_panic_is_rustcs_deny_lint() {
    // #947: each message, label and note as rustc 1.96 gives them.
    let cases = [
        (
            "let x: u8 = 255 + 1;",
            "this arithmetic operation will overflow",
            "attempt to compute `u8::MAX + 1_u8`, which would overflow",
            "arithmetic_overflow",
        ),
        (
            "let y = 0u32 - 1;",
            "this arithmetic operation will overflow",
            "attempt to compute `0_u32 - 1_u32`, which would overflow",
            "arithmetic_overflow",
        ),
        (
            "let z = 1i32 << 40;",
            "this arithmetic operation will overflow",
            "attempt to shift left by `40_i32`, which would overflow",
            "arithmetic_overflow",
        ),
        (
            "let q = 1 / 0;",
            "this operation will panic at runtime",
            "attempt to divide `1_i32` by zero",
            "unconditional_panic",
        ),
        (
            "let r = 7 % (2 - 2);",
            "this operation will panic at runtime",
            "attempt to calculate the remainder of `7_i32` with a divisor of zero",
            "unconditional_panic",
        ),
        (
            "let s = -128i8 / -1;",
            "this operation will panic at runtime",
            "attempt to compute `i8::MIN / -1_i8`, which would overflow",
            "unconditional_panic",
        ),
        (
            "let a = [1, 2, 3];\n    let w = a[5];",
            "this operation will panic at runtime",
            "index out of bounds: the length is 3 but the index is 5",
            "unconditional_panic",
        ),
    ];
    for (body, message, label, lint) in cases {
        let source = format!("fn main() {{\n    {body}\n}}\n");
        let error = Engine::new()
            .compile(&source)
            .map(|_| ())
            .expect_err("rawr refuses this program");
        let evidence = Source::new("main.rs", &source).expect("a test source has an identity");
        let report = Report::from_parse(evidence, &error).expect("the error is in the source");
        assert_eq!(report.code().as_str(), "", "{body}");
        assert_eq!(report.message(), message, "{body}");
        assert_eq!(
            report.labels().first().and_then(|label| label.message()),
            Some(label),
            "{body}"
        );
        assert_eq!(
            report.notes().collect::<Vec<_>>(),
            [format!("`#[deny({lint})]` on by default")],
            "{body}"
        );
    }
    // The operation is underlined whole.
    assert!(rendered("fn main() {\n    let x: u8 = 255 + 1;\n}\n")
        .contains("\n2 |     let x: u8 = 255 + 1;\n  |                 ^^^^^^^ attempt"));
    // What doesn't overflow or panic runs.
    let program = "fn main() {\n    let a = -128i8;\n    let b = 0xFFu8 & 0x0F;\n    let c = \
                   255u8 << 1;\n    let d = (3 + 4) * 2;\n    let f: u64 = 1 << 40;\n    let g = \
                   [1, 2, 3][2];\n    let h = -(5 - 2);\n    let _ = (a, b, c, d, f, g, h);\n}\n";
    Engine::new()
        .compile(program)
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn assigning_through_a_shared_reference_is_e0594() {
    // A place behind a `&` parameter or local: `*r`, a field and an element
    // of a slice or array (#2305).
    let written = |reference: &str| {
        label(&format!(
            "`{reference}` is a `&` reference, so it cannot be written to"
        ))
    };
    assert_eq!(
        refused("fn bump(shared: &u64) {\n    *shared = 5;\n}\nfn main() {}\n"),
        (
            "E0594".into(),
            "cannot assign to `*shared`, which is behind a `&` reference".into(),
            written("shared")
        )
    );
    assert_eq!(
        refused("fn main() {\n    let x = 1;\n    let r = &x;\n    *r += 2;\n}\n"),
        (
            "E0594".into(),
            "cannot assign to `*r`, which is behind a `&` reference".into(),
            written("r")
        )
    );
    assert_eq!(
        refused("struct P { f: i32 }\nfn a(r: &P) {\n    r.f = 5;\n}\nfn main() {}\n"),
        (
            "E0594".into(),
            "cannot assign to `r.f`, which is behind a `&` reference".into(),
            written("r")
        )
    );
    assert_eq!(
        refused("fn d(r: &[i32]) {\n    r[0] = 5;\n}\nfn main() {}\n"),
        (
            "E0594".into(),
            "cannot assign to `r[_]`, which is behind a `&` reference".into(),
            written("r")
        )
    );
    assert_eq!(
        refused("fn e(r: &[i32; 2]) {\n    r[0] = 5;\n}\nfn main() {}\n"),
        (
            "E0594".into(),
            "cannot assign to `r[_]`, which is behind a `&` reference".into(),
            written("r")
        )
    );
    let assigned = |body: &str| {
        refused(&format!(
            "struct P {{ f: i32, a: [i32; 2], v: Vec<i32> }}\nfn g(mut r: &P, t: &(i32, i32)) \
             {{\n    {body}\n}}\nfn main() {{}}\n"
        ))
    };
    let shared = |place: &str, reference: &str| {
        (
            "E0594".to_owned(),
            format!("cannot assign to `{place}`, which is behind a `&` reference"),
            written(reference),
        )
    };
    assert_eq!(assigned("r.f += 1;"), shared("r.f", "r"));
    assert_eq!(assigned("(*r).f = 1;"), shared("r.f", "r"));
    assert_eq!(assigned("r.a[1] = 1;"), shared("r.a[_]", "r"));
    assert_eq!(assigned("t.0 = 1;"), shared("t.0", "t"));
    // A match arm's assignment is refused as a statement's.
    assert_eq!(
        assigned("match 1 {\n        0 => r.f = 1,\n        _ => {}\n    }"),
        shared("r.f", "r")
    );
    assert_eq!(
        assigned("match 1 {\n        0 => *t = (1, 2),\n        _ => {}\n    }"),
        shared("*t", "t")
    );
    assert_eq!(
        refused(
            "struct S { n: i32 }\nimpl S {\n    fn a(&self) {\n        *self = S { n: 1 };\n    \
             }\n}\nfn main() {}\n"
        ),
        shared("*self", "self")
    );
    // An element of a `Vec` is `IndexMut`'s, which borrows it mutably.
    let borrowed = |place: &str| {
        (
            "E0596".to_owned(),
            format!("cannot borrow `{place}` as mutable, as it is behind a `&` reference"),
            label("`r` is a `&` reference, so it cannot be borrowed as mutable"),
        )
    };
    assert_eq!(
        refused("fn b(r: &Vec<i32>) {\n    r[0] = 5;\n}\nfn main() {}\n"),
        borrowed("*r")
    );
    assert_eq!(assigned("r.v[0] = 1;"), borrowed("r.v"));
    // rustc marks the whole assignment, and for `IndexMut` the container.
    let rendered = |source: &str| {
        let error = Engine::new().compile(source).map(|_| ()).unwrap_err();
        let source = Source::new("main.rs", source).unwrap();
        render(&Report::from_parse(source, &error).unwrap())
    };
    let bump = rendered("fn bump(shared: &u64) { *shared = 5; }\nfn main() {}\n");
    assert!(
        bump.contains(
            "1 | fn bump(shared: &u64) { *shared = 5; }\n  |                         \
             ^^^^^^^^^^^ `shared` is a `&` reference, so it cannot be written to"
        ),
        "{bump}"
    );
    let element = rendered("fn b(r: &Vec<i32>) { r[0] = 5; }\nfn main() {}\n");
    assert!(
        element.contains(
            "1 | fn b(r: &Vec<i32>) { r[0] = 5; }\n  |                      ^ `r` is a `&` \
             reference, so it cannot be borrowed as mutable"
        ),
        "{element}"
    );
}

#[test]
fn an_assignment_to_a_value_is_e0070() {
    // rustc's message, its mark on the operator and its label on the left
    // side (#2306).
    let assigned = |body: &str| {
        rendered(&format!(
            "struct S {{ y: i64 }}\nimpl S {{ fn foo(&self) -> i64 {{ self.y }} }}\nfn foo(x: \
             i64) -> i64 {{ x }}\nfn main() {{\n    {body}\n}}\n"
        ))
    };
    let sum = assigned("let x = 1; let y = 2; (x + y) = 42;");
    let expected = [
        "error[E0070]: invalid left-hand side of assignment",
        " --> main.rs:5:35",
        "  |",
        "5 |     let x = 1; let y = 2; (x + y) = 42;",
        "  |                           ------- ^",
        "  |                           |",
        "  |                           cannot assign to this expression",
    ]
    .join("\n");
    assert!(sum.starts_with(&expected), "{sum}");
    let call = assigned("let x = 1; foo(x) = 42;");
    assert!(
        call.starts_with(
            "error[E0070]: invalid left-hand side of assignment\n --> main.rs:5:23\n  |\n5 |     \
             let x = 1; foo(x) = 42;\n  |                ------ ^\n"
        ),
        "{call}"
    );
    let literal = assigned("123 = 42;");
    assert!(
        literal.starts_with(
            "error[E0070]: invalid left-hand side of assignment\n --> main.rs:5:9\n  |\n5 |     \
             123 = 42;\n  |     --- ^\n"
        ),
        "{literal}"
    );
    let method = assigned("let x = S { y: 1 }; x.foo() = 42;");
    assert!(
        method.starts_with(
            "error[E0070]: invalid left-hand side of assignment\n --> main.rs:5:33\n  |\n5 |     \
             let x = S { y: 1 }; x.foo() = 42;\n  |                         ------- ^\n"
        ),
        "{method}"
    );
    let negation = assigned("let x = 1; -x = 2;");
    assert!(
        negation
            .starts_with("error[E0070]: invalid left-hand side of assignment\n --> main.rs:5:19\n"),
        "{negation}"
    );
    // A match arm's assignment too.
    let arm = assigned(
        "let c = true;\n    match c {\n        true => foo(1) = 2,\n        false => {}\n    }",
    );
    assert!(
        arm.starts_with(
            "error[E0070]: invalid left-hand side of assignment\n --> main.rs:7:24\n  |\n7 |         \
             true => foo(1) = 2,\n  |                 ------ ^\n"
        ),
        "{arm}"
    );
}

#[test]
fn a_compound_assignment_to_a_value_is_e0067() {
    let (code, message, label) =
        refused("fn main() {\n    let x = 1;\n    let y = 2;\n    (x + y) += 1;\n}\n");
    assert_eq!(
        (code.as_str(), message.as_str(), label),
        ("E0067", "invalid left-hand side of assignment", None)
    );
    let call = rendered("fn f() -> i64 { 1 }\nfn main() {\n    f() -= 1;\n}\n");
    assert!(
        call.starts_with(
            "error[E0067]: invalid left-hand side of assignment\n --> main.rs:3:9\n  |\n3 |     \
             f() -= 1;\n  |     --- ^^\n  |     |\n  |     cannot assign to this expression\n"
        ),
        "{call}"
    );
}

#[test]
fn an_assignment_to_a_field_of_a_temporary_is_no_e0070() {
    // `f().x` is a place in Rust, which rawr gives no place of its own yet.
    let field = rendered(
        "struct P { x: i64 }\nfn f() -> P { P { x: 0 } }\nfn main() {\n    f().x = 1;\n}\n",
    );
    assert!(!field.contains("E0070"), "{field}");
    // `*f(..)` is the place a call's `&mut` refers to, though it ends where
    // the call does.
    let dereferenced = Engine::new()
        .compile(
            "fn f(n: &mut i64) -> &mut i64 { n }\nfn main() {\n    let mut n = 1;\n    *f(&mut \
             n) = 2;\n}\n",
        )
        .map(|_| ());
    assert!(
        !matches!(
            &dereferenced,
            Err(error) if error.to_string().contains("E0070")
        ),
        "{dereferenced:?}"
    );
}

/// What rawr renders for `body`, the body of `main`.
fn rendered_main(body: &str) -> String {
    rendered(&format!(
        "fn f(x: i64) -> i64 {{ x }}\nfn main() {{\n    {body}\n}}\n"
    ))
}

#[test]
fn a_destructuring_assignment_to_an_immutable_local_is_e0384() {
    // Each place is assigned alone, and marked alone, as rustc desugars
    // the assignment (#2318).
    let immutable =
        rendered_main("let a = 1; let mut b = 2; (a, b) = (3, 4); println!(\"{a}{b}\");");
    let expected = [
        "error[E0384]: cannot assign twice to immutable variable `a`",
        " --> main.rs:3:32",
        "  |",
        "3 |     let a = 1; let mut b = 2; (a, b) = (3, 4); println!(\"{a}{b}\");",
        "  |         -                      ^ cannot assign twice to immutable variable",
        "  |         |",
        "  |         first assignment to `a`",
    ]
    .join("\n");
    assert!(immutable.starts_with(&expected), "{immutable}");
    // A `let a;` takes one destructuring assignment.
    let deferred = rendered_main("let a; let b; (a, b) = (1, 2); (a, b) = (3, 4);");
    let expected = [
        "error[E0384]: cannot assign twice to immutable variable `a`",
        " --> main.rs:3:37",
        "  |",
        "3 |     let a; let b; (a, b) = (1, 2); (a, b) = (3, 4);",
        "  |                    -                ^ cannot assign twice to immutable variable",
        "  |                    |",
        "  |                    first assignment to `a`",
    ]
    .join("\n");
    assert!(deferred.starts_with(&expected), "{deferred}");
    // A parenthesized place is the place, and the assignment is marked
    // whole.
    let parenthesized = rendered_main("let mut x = 1; (x) += 1; (x) = 3; let y = 2; (y) = 3;");
    let expected = [
        "error[E0384]: cannot assign twice to immutable variable `y`",
        " --> main.rs:3:50",
        "  |",
        "3 |     let mut x = 1; (x) += 1; (x) = 3; let y = 2; (y) = 3;",
        "  |                                           -      ^^^^^^^ cannot assign twice to \
         immutable variable",
        "  |                                           |",
        "  |                                           first assignment to `y`",
    ]
    .join("\n");
    assert!(parenthesized.starts_with(&expected), "{parenthesized}");
}

#[test]
fn a_destructuring_assignment_to_a_value_is_e0070() {
    let assigned = |body: &str, column: usize, marks: &str| {
        let rendered = rendered_main(body);
        let expected = format!(
            "error[E0070]: invalid left-hand side of assignment\n --> main.rs:3:{column}\n  |\n3 \
             |     {body}\n  |{marks}\n"
        );
        assert!(rendered.starts_with(&expected), "{rendered}");
    };
    assigned(
        "let mut a = 1; let mut b = 2; (a + 1, b) = (3, 4);",
        46,
        "                                    -----     ^",
    );
    assigned(
        "let mut a = 0; (a, 1) = (1, 2);",
        27,
        "                        -  ^",
    );
    assigned(
        "let mut a = 0; (f(a), a) = (1, 2);",
        30,
        "                     ----     ^",
    );
    // A call of a function is no tuple struct's constructor.
    assigned("let mut a = 0; f(a) = 1;", 25, "                    ---- ^");
}

#[test]
fn a_compound_assignment_to_a_tuple_is_e0067() {
    // rustc reports E0368 for `+=` on a tuple first, and E0067 beside it.
    let rendered = rendered_main("let mut a = 1; let mut b = 2; (a, b) += (3, 4);");
    let expected = [
        "error[E0067]: invalid left-hand side of assignment",
        " --> main.rs:3:42",
        "  |",
        "3 |     let mut a = 1; let mut b = 2; (a, b) += (3, 4);",
        "  |                                   ------ ^^",
        "  |                                   |",
        "  |                                   cannot assign to this expression",
    ]
    .join("\n");
    assert!(rendered.starts_with(&expected), "{rendered}");
}

#[test]
fn an_underscore_outside_an_assignee_is_rustcs_syntax_error() {
    let message = "error: in expressions, `_` can only be used on the left-hand side of an \
                   assignment";
    let compound = rendered_main("let mut a = 1; _ += 1;");
    let expected = [
        message,
        " --> main.rs:3:20",
        "  |",
        "3 |     let mut a = 1; _ += 1;",
        "  |                    ^ `_` not allowed here",
    ]
    .join("\n");
    assert!(compound.starts_with(&expected), "{compound}");
    let value = rendered_main("let x: i64 = _;");
    let expected = [
        message,
        " --> main.rs:3:18",
        "  |",
        "3 |     let x: i64 = _;",
        "  |                  ^ `_` not allowed here",
    ]
    .join("\n");
    assert!(value.starts_with(&expected), "{value}");
}

#[test]
fn a_refutable_destructuring_assignment_is_e0005() {
    let rendered = rendered_main("let mut a = 1; Some(a) = Some(2);");
    assert!(
        rendered.starts_with("error[E0005]: refutable pattern in assignment\n --> main.rs:3:20\n"),
        "{rendered}"
    );
    assert!(
        rendered.contains(
            "= note: destructuring assignments require an \"irrefutable pattern\", like a \
             `struct` or an `enum` with only one variant"
        ),
        "{rendered}"
    );
    assert!(!rendered.contains("let else"), "{rendered}");
}

#[test]
fn a_destructuring_assignment_moves_its_value_first_e0382() {
    let rendered = rendered_main(
        "let s = String::from(\"a\"); let mut t = String::new(); let mut u = String::new(); (t, \
         u) = (s, s);",
    );
    assert!(
        rendered.starts_with("error[E0382]: use of moved value: `s`\n --> main.rs:3:99\n"),
        "{rendered}"
    );
}

/// The first `lines` lines rawr renders for `body`, the body of `main`:
/// rustc's message, arrow, source line and labels.
fn rendered_head(body: &str, lines: usize) -> String {
    rendered_main(body)
        .lines()
        .take(lines)
        .collect::<Vec<_>>()
        .join("\n")
}

/// rustc's message, arrow and one marker line under the source line
/// `body`, which starts at column 5 of line 3.
fn marked(message: &str, column: usize, body: &str, marker: &str) -> String {
    [
        message.to_string(),
        format!(" --> main.rs:3:{column}"),
        "  |".to_string(),
        format!("3 |     {body}"),
        format!("  |     {marker}"),
    ]
    .join("\n")
}

#[test]
fn an_array_pattern_of_another_length_is_e0527_or_e0528() {
    let body = "let [a, b] = [1, 2, 3];";
    assert_eq!(
        rendered_head(body, 5),
        marked(
            "error[E0527]: pattern requires 2 elements but array has 3",
            9,
            body,
            "    ^^^^^^ expected 3 elements"
        )
    );
    let body = "let [a, b, c, d] = [1];";
    assert_eq!(
        rendered_head(body, 5),
        marked(
            "error[E0527]: pattern requires 4 elements but array has 1",
            9,
            body,
            "    ^^^^^^^^^^^^ expected 1 element"
        )
    );
    let body = "let [a, b, c, d, ..] = [1, 2, 3];";
    assert_eq!(
        rendered_head(body, 5),
        marked(
            "error[E0528]: pattern requires at least 4 elements but array has 3",
            9,
            body,
            "    ^^^^^^^^^^^^^^^^ pattern cannot match array of 3 elements"
        )
    );
    // A `match` arm and a destructuring assignment are refused alike.
    let body = "match [1, 2, 3] { [a, b] => {} _ => {} }";
    assert_eq!(
        rendered_head(body, 5),
        marked(
            "error[E0527]: pattern requires 2 elements but array has 3",
            23,
            body,
            "                  ^^^^^^ expected 3 elements"
        )
    );
    let body = "let (a, b); [a, b] = [1, 2, 3];";
    assert_eq!(
        rendered_head(body, 5),
        marked(
            "error[E0527]: pattern requires 2 elements but array has 3",
            17,
            body,
            "            ^^^^^^ expected 3 elements"
        )
    );
}

#[test]
fn a_tuple_pattern_of_another_arity_is_e0308() {
    let label = |body: &str| {
        let (code, message, label) = refused(&format!("fn main() {{\n    {body}\n}}\n"));
        assert_eq!(
            (code.as_str(), message.as_str()),
            ("E0308", "mismatched types"),
            "{body}"
        );
        label.expect("a label on the pattern")
    };
    let arity = "expected a tuple with 3 elements, found one with 2 elements";
    assert_eq!(label("let (a, b) = (1, 2, 3);"), arity);
    assert_eq!(label("let (a, b): (i32, i32, i32) = (1, 2, 3);"), arity);
    assert_eq!(label("match (1, 2, 3) { (a, b) => {} }"), arity);
    assert_eq!(label("let (a, b); (a, b) = (1, 2, 3);"), arity);
    assert_eq!(
        label("let (a,) = (1, 2);"),
        "expected a tuple with 2 elements, found one with 1 element"
    );
    // A `..` counts as no element.
    assert_eq!(
        label("let (a, b, c, d, ..) = (1, 2, 3);"),
        "expected a tuple with 3 elements, found one with 4 elements"
    );
    // On a value that is no tuple, the pattern is named by its shape.
    assert_eq!(label("let (a, b) = 5;"), "expected integer, found `(_, _)`");
    assert_eq!(label("let (a, ..) = 5;"), "expected integer, found `(_,)`");
    let body = "let (a, b) = (1, 2, 3);";
    assert_eq!(
        rendered_head(body, 10),
        pattern_mismatch(
            9,
            body,
            &[
                "    ^^^^^^   --------- this expression has type `({integer}, {integer}, {integer})`",
                "    |",
                "    expected a tuple with 3 elements, found one with 2 elements",
                "",
                "  = note: expected tuple `({integer}, {integer}, {integer})`",
                "             found tuple `(_, _)`",
            ]
        )
    );
}

#[test]
fn a_second_rest_in_a_pattern_is_rustcs_error() {
    // The marker line under the source, then the first `..`'s label.
    let expected = |pattern: &str, column: usize, body: &str, before: usize, gap: usize| {
        let indent = " ".repeat(before);
        let label = format!("can only be used once per {pattern} pattern");
        [
            marked(
                &format!("error: `..` {label}"),
                column,
                body,
                &format!("{indent}--{}^^ {label}", " ".repeat(gap)),
            ),
            format!("  |     {indent}|"),
            format!("  |     {indent}previously used here"),
        ]
        .join("\n")
    };
    let body = "let t = (1, 2, 3); let (a, .., ..) = t;";
    assert_eq!(rendered_head(body, 7), expected("tuple", 36, body, 27, 2));
    let body = "let t = (1, 2, 3); match t { (a, .., ..) => {} }";
    assert_eq!(rendered_head(body, 7), expected("tuple", 42, body, 33, 2));
    let body = "let (e, f); (e, .., ..) = (1, 2, 3);";
    assert_eq!(rendered_head(body, 7), expected("tuple", 25, body, 16, 2));
    let body = "let [a, .., b, ..] = [1, 2, 3];";
    assert_eq!(rendered_head(body, 7), expected("slice", 20, body, 8, 5));
}

#[test]
fn a_refutable_pattern_is_marked_whole_e0005() {
    // The marker under the pattern, which starts at `column`.
    let marker = |body: &str| {
        rendered_main(body)
            .lines()
            .nth(4)
            .expect("a line under the source")
            .to_string()
    };
    let under = |column: usize, width: usize, witness: &str| {
        format!(
            "  |{}{} pattern `{witness}` not covered",
            " ".repeat(column),
            "^".repeat(width)
        )
    };
    assert_eq!(
        marker("let o: Option<i32> = None; let Some(a) = o;"),
        under(36, 7, "None")
    );
    assert_eq!(
        marker("let o: Result<i32, i32> = Ok(1); let Ok(x) = o;"),
        under(42, 5, "Err(_)")
    );
    assert_eq!(
        marker("let o = Some((1, 2)); let Some((a, b)) = o;"),
        under(31, 12, "None")
    );
    // A user enum's tuple and struct variants, and a parameter.
    assert_eq!(
        marker("enum E { A(i32), B } let e = E::B; let E::A(x) = e;"),
        under(44, 7, "E::B")
    );
    assert_eq!(
        marker("enum E { A { x: i32 }, B } let e = E::B; let E::A { x } = e;"),
        under(50, 10, "E::B")
    );
    assert_eq!(
        marker("enum E { A(i32), B } fn g(E::A(x): E) {}"),
        under(31, 7, "E::B")
    );
    // A destructuring assignment.
    assert_eq!(
        marker("let o: Option<i32> = None; let x; Some(x) = o;"),
        under(39, 7, "None")
    );
}

/// rustc's E0308 on a pattern under the source line `body` of `main`,
/// which starts at column 5 of line 3: its arrow at `column`, then the
/// lines under the source, each after the gutter's `  |     `, but a
/// note and the lines after it, which are as they are.
fn pattern_mismatch(column: usize, body: &str, under: &[&str]) -> String {
    let mut lines = vec![
        "error[E0308]: mismatched types".to_string(),
        format!(" --> main.rs:3:{column}"),
        "  |".to_string(),
        format!("3 |     {body}"),
    ];
    let mut noted = false;
    for line in under {
        noted |= line.starts_with("  = ");
        lines.push(match *line {
            _ if noted => line.to_string(),
            "" => "  |".to_string(),
            line => format!("  |     {line}"),
        });
    }
    lines.join("\n")
}

#[test]
fn a_pattern_of_another_type_is_labelled_beside_what_typed_its_value_e0308() {
    let check = |column: usize, body: &str, under: &[&str]| {
        assert_eq!(
            rendered_head(body, 4 + under.len()),
            pattern_mismatch(column, body, under),
            "{body}"
        );
    };
    let tuple_note = [
        "",
        "  = note: expected type `{integer}`",
        "            found tuple `(_, _)`",
    ];
    // A `match`'s scrutinee, before the pattern.
    let mut under = vec![
        "      -   ^^^^^^ expected integer, found `(_, _)`",
        "      |",
        "      this expression has type `{integer}`",
    ];
    under.extend(tuple_note);
    check(15, "match 5 { (a, b) => {} }", &under);
    // A `let`'s value, after it, or its written type.
    let mut under = vec![
        "    ^^^^^^   - this expression has type `{integer}`",
        "    |",
        "    expected integer, found `(_, _)`",
    ];
    under.extend(tuple_note);
    check(9, "let (a, b) = 5;", &under);
    check(12, "if let (a, b) = 5 {}", &{
        let mut under = under.clone();
        under[0] = "       ^^^^^^   - this expression has type `{integer}`";
        under[1] = "       |";
        under[2] = "       expected integer, found `(_, _)`";
        under
    });
    let mut under = vec![
        "    ^^^^^^  --- expected due to this",
        "    |",
        "    expected `i32`, found `(_, _)`",
        "",
        "  = note: expected type `i32`",
        "            found tuple `(_, _)`",
    ];
    check(9, "let (a, b): i32 = 5;", &under);
    under[0] = " ^^^^^^  --- expected due to this";
    under[1] = " |";
    under[2] = " expected `i32`, found `(_, _)`";
    check(6, "|(a, b): i32| a;", &under);
    // A pattern below another names the whole value.
    let mut under = vec![
        "         ^^^^^^    ------- this expression has type `Option<{integer}>`",
        "         |",
        "         expected integer, found `(_, _)`",
    ];
    under.extend(tuple_note);
    check(14, "let Some((a, b)) = Some(5) else { return };", &under);
    // A `for`'s iterator, of its items' type.
    let mut under = vec![
        "    ^^^^^^    ---- this is an iterator with items of type `{integer}`",
        "    |",
        "    expected integer, found `(_, _)`",
    ];
    under.extend(tuple_note);
    check(9, "for (a, b) in 0..3 {}", &under);
    // A tuple of another arity is noted as a tuple.
    check(
        26,
        "let t = (1,2,3); let (a, b) = t;",
        &[
            "                     ^^^^^^   - this expression has type `({integer}, {integer}, {integer})`",
            "                     |",
            "                     expected a tuple with 3 elements, found one with 2 elements",
            "",
            "  = note: expected tuple `({integer}, {integer}, {integer})`",
            "             found tuple `(_, _)`",
        ],
    );
    // The note's shorter lead is padded, and a constructor's type has a
    // `_` for each argument.
    check(
        20,
        "let x = 5; let &a = x;",
        &[
            "               ^^   - this expression has type `{integer}`",
            "               |",
            "               expected integer, found `&_`",
            "",
            "  = note:   expected type `{integer}`",
            "          found reference `&_`",
        ],
    );
    check(
        20,
        "let x = 5; let &mut b = &x;",
        &[
            "               ^^^^^^   -- this expression has type `&{integer}`",
            "               |",
            "               types differ in mutability",
            "               help: to declare a mutable variable use: `mut b`",
            "",
            "  = note:      expected reference `&{integer}`",
            "          found mutable reference `&mut _`",
        ],
    );
    check(
        26,
        "let x = 5; match x { Ok(a) | Err(a) => {} }",
        &[
            "                 -   ^^^^^ expected integer, found `Result<_, _>`",
            "                 |",
            "                 this expression has type `{integer}`",
            "",
            "  = note: expected type `{integer}`",
            "             found enum `Result<_, _>`",
        ],
    );
    // A parameter's written type.
    assert_eq!(
        rendered("fn g((a, b): i32) {}\nfn main() {}\n")
            .lines()
            .skip(3)
            .take(4)
            .collect::<Vec<_>>(),
        [
            "1 | fn g((a, b): i32) {}",
            "  |      ^^^^^^  --- expected due to this",
            "  |      |",
            "  |      expected `i32`, found `(_, _)`",
        ]
    );
}

#[test]
fn a_pattern_mismatch_of_two_simple_types_has_no_note_e0308() {
    for (column, body, under) in [
        (
            15,
            "match 5 { \"a\" => {} _ => {} }",
            [
                "      -   ^^^ expected integer, found `&str`",
                "      |",
                "      this expression has type `{integer}`",
            ],
        ),
        // A `&str` matches as the `str` below it.
        (
            17,
            "match \"a\" { 5 => {} _ => {} }",
            [
                "      ---   ^ expected `str`, found integer",
                "      |",
                "      this expression has type `&str`",
            ],
        ),
        (
            30,
            "struct S(i32); match 5 { S(a) => {} }",
            [
                "                     -   ^^^^ expected integer, found `S`",
                "                     |",
                "                     this expression has type `{integer}`",
            ],
        ),
        (
            44,
            "enum E { A(i32) } let y = 5; match y { E::A(z) => {} _ => {} }",
            [
                "                                   -   ^^^^^^^ expected integer, found `E`",
                "                                   |",
                "                                   this expression has type `{integer}`",
            ],
        ),
    ] {
        let rendered = rendered_main(body);
        assert_eq!(
            rendered.lines().take(7).collect::<Vec<_>>().join("\n"),
            pattern_mismatch(column, body, &under),
            "{body}"
        );
        assert!(!rendered.contains("note"), "{rendered}");
    }
}

#[test]
fn a_slice_pattern_on_no_array_or_slice_is_e0529() {
    for (column, body, ty, marker) in [
        (9, "let [a, ..] = 5;", "{integer}", "    ^^^^^^^"),
        (
            9,
            "let [a, b] = (1, 2);",
            "({integer}, {integer})",
            "    ^^^^^^",
        ),
        (
            20,
            "match (1, 2) { [a, b] => {} }",
            "({integer}, {integer})",
            "               ^^^^^^",
        ),
        (
            26,
            "let x = 5; match x { [a, ..] => {} }",
            "{integer}",
            "                     ^^^^^^^",
        ),
    ] {
        assert_eq!(
            rendered_head(body, 5),
            marked(
                &format!("error[E0529]: expected an array or slice, found `{ty}`"),
                column,
                body,
                &format!("{marker} pattern cannot match with input type `{ty}`")
            ),
            "{body}"
        );
    }
    // A `Vec` is sliced to match one.
    let rendered = rendered_main("let v = vec![1]; let [a, ..] = v;");
    assert!(
        rendered.starts_with("error[E0529]: expected an array or slice, found `Vec<{integer}>`"),
        "{rendered}"
    );
    assert!(rendered.contains("consider slicing here"), "{rendered}");
}

/// rustc suggests slicing a `Vec` a slice pattern matches whole, as the
/// source the `[..]` makes under its own `help:`.
#[test]
fn a_slice_pattern_on_a_vec_suggests_slicing_it() {
    assert_eq!(
        rendered(
            "fn main() {\n    let v = vec![1, 2, 3];\n    match v {\n        [a, b, c] => \
             println!(\"{a}{b}{c}\"),\n        _ => {}\n    }\n}\n"
        ),
        concat!(
            "error[E0529]: expected an array or slice, found `Vec<{integer}>`\n",
            " --> main.rs:4:9\n",
            "  |\n",
            "4 |         [a, b, c] => println!(\"{a}{b}{c}\"),\n",
            "  |         ^^^^^^^^^ pattern cannot match with input type `Vec<{integer}>`\n",
            "  |\n",
            "help: consider slicing here\n",
            "  |\n",
            "3 |     match v[..] {\n",
            "  |            ++++\n",
        )
    );
    assert_eq!(
        rendered("fn main() {\n    let v = vec![1, 2, 3];\n    let [a, ..] = v;\n    println!(\"{a}\");\n}\n"),
        concat!(
            "error[E0529]: expected an array or slice, found `Vec<{integer}>`\n",
            " --> main.rs:3:9\n",
            "  |\n",
            "3 |     let [a, ..] = v;\n",
            "  |         ^^^^^^^ pattern cannot match with input type `Vec<{integer}>`\n",
            "  |\n",
            "help: consider slicing here\n",
            "  |\n",
            "3 |     let [a, ..] = v[..];\n",
            "  |                    ++++\n",
        )
    );
    // A slice pattern inside another matches no `Vec` the statement
    // names: rustc suggests nothing.
    let nested = rendered(
        "fn main() {\n    let v = vec![1];\n    match (v,) {\n        ([a],) => {}\n        _ => {}\n    \
         }\n}\n",
    );
    assert!(!nested.contains("help"), "{nested}");
}

/// A `let`'s `&mut` pattern of one binding on a `&` value: rustc's inline
/// help declares the binding `mut` instead.
#[test]
fn a_lets_mut_reference_pattern_on_a_shared_reference_suggests_a_mut_binding() {
    assert_eq!(
        rendered("fn main() {\n    let x = 5;\n    let &mut b = &x;\n    println!(\"{b}\");\n}\n"),
        concat!(
            "error[E0308]: mismatched types\n",
            " --> main.rs:3:9\n",
            "  |\n",
            "3 |     let &mut b = &x;\n",
            "  |         ^^^^^^   -- this expression has type `&{integer}`\n",
            "  |         |\n",
            "  |         types differ in mutability\n",
            "  |         help: to declare a mutable variable use: `mut b`\n",
            "  |\n",
            "  = note:      expected reference `&{integer}`\n",
            "          found mutable reference `&mut _`\n",
        )
    );
}

/// An arm's `&mut` pattern of one binding on a `&` value: rustc notes the
/// `mut` binding, in a section of its own at the pattern, and suggests
/// removing the `&mut`.
#[test]
fn an_arms_mut_reference_pattern_on_a_shared_reference_notes_a_mut_binding() {
    assert_eq!(
        rendered(
            "fn main() {\n    let x = 5;\n    match &x {\n        &mut b => println!(\"{b}\"),\n    \
             }\n}\n"
        ),
        concat!(
            "error[E0308]: mismatched types\n",
            " --> main.rs:4:9\n",
            "  |\n",
            "3 |     match &x {\n",
            "  |           -- this expression has type `&{integer}`\n",
            "4 |         &mut b => println!(\"{b}\"),\n",
            "  |         ^^^^^^ types differ in mutability\n",
            "  |\n",
            "  = note:      expected reference `&{integer}`\n",
            "          found mutable reference `&mut _`\n",
            "note: to declare a mutable binding use: `mut b`\n",
            " --> main.rs:4:9\n",
            "  |\n",
            "4 |         &mut b => println!(\"{b}\"),\n",
            "  |         ^^^^^^\n",
            "help: consider removing `&mut` from the pattern\n",
            "  |\n",
            "4 -         &mut b => println!(\"{b}\"),\n",
            "4 +         b => println!(\"{b}\"),\n",
            "  |\n",
        )
    );
    // A `&` pattern on a `&` value matches the value below it.
    let program = "fn main() {\n    let x = 5;\n    match &x {\n        &1 => {}\n        &n => \
                   println!(\"{n}\"),\n    }\n}\n";
    assert!(Engine::new().compile(program).is_ok());
}

/// A path to a private item from outside its module: rustc's E0603 notes
/// the item whole, from its visibility through its signature, in a section
/// of its own.
#[test]
fn a_private_items_definition_is_rustcs_note_of_its_own() {
    for (item, path, message, shown) in [
        (
            "fn f(a: i32) -> i32 { a }",
            "let _ = m::f(1);",
            "the function `f` is defined here",
            "^^^^^^^^^^^^^^^^^^^",
        ),
        (
            "enum E<T> { A(T) }",
            "let _: m::E<i32>;",
            "the enum `E` is defined here",
            "^^^^^^^^^",
        ),
        (
            "mod n { pub fn f() {} }",
            "m::n::f();",
            "the module `n` is defined here",
            "^^^^^",
        ),
        (
            "type A = i32;",
            "let _: m::A = 1;",
            "the type alias `A` is defined here",
            "^^^^^^^^^^^^^",
        ),
        (
            "struct P<T> where T: Copy { a: T }",
            "let _: m::P<i32>;",
            "the struct `P` is defined here",
            "^^^^^^^^^^^^^^^^^^^^^^^^^",
        ),
    ] {
        let rendered = rendered(&format!(
            "mod m {{\n    {item}\n}}\nfn main() {{\n    {path}\n}}\n"
        ));
        assert!(
            rendered.ends_with(&format!(
                "  |\nnote: {message}\n --> main.rs:2:5\n  |\n2 |     {item}\n  |     {shown}\n"
            )),
            "{rendered}"
        );
    }
}

/// A call with the wrong number of arguments: rustc's E0061 notes where
/// the callee is defined in a section of its own.
#[test]
fn an_argument_counts_callee_is_rustcs_note_of_its_own() {
    let rendered = rendered("fn f(a: i32) {}\nfn main() {\n    f(1, 2);\n}\n");
    assert!(
        rendered.contains(
            "note: function defined here\n --> main.rs:1:4\n  |\n1 | fn f(a: i32) {}\n  |    ^\n"
        ),
        "{rendered}"
    );
    assert!(!rendered.contains("- function defined here"), "{rendered}");
}

/// A macro called as a function: rustc suggests the `!` after its path,
/// and nothing for one named as a value.
#[test]
fn a_macro_called_as_a_function_suggests_its_bang() {
    assert_eq!(
        rendered("fn main() {\n    std::print(\"hi\");\n}\n"),
        concat!(
            "error[E0423]: expected function, found macro `std::print`\n",
            " --> main.rs:2:5\n",
            "  |\n",
            "2 |     std::print(\"hi\");\n",
            "  |     ^^^^^^^^^^ not a function\n",
            "  |\n",
            "help: use `!` to invoke the macro\n",
            "  |\n",
            "2 |     std::print!(\"hi\");\n",
            "  |               +\n",
        )
    );
    let value = rendered("fn main() {\n    let f = eprintln;\n}\n");
    assert!(!value.contains("help"), "{value}");
}

/// An assignment to a binding not declared `mut`: rustc suggests the
/// `mut` before its name, where the binding is declared.
#[test]
fn an_assignment_to_an_immutable_binding_suggests_declaring_it_mut() {
    for (body, suggested, marker) in [
        ("let x = 5;\n    x = 6;", "let mut x = 5;", "    +++"),
        (
            "for x in 0..3 {\n        x = 1;\n    }",
            "for mut x in 0..3 {",
            "    +++",
        ),
    ] {
        let rendered = rendered(&format!("fn main() {{\n    {body}\n}}\n"));
        assert!(
            rendered.ends_with(&format!(
                "help: consider making this binding mutable\n  |\n2 |     {suggested}\n  |     \
                 {marker}\n"
            )),
            "{rendered}"
        );
    }
    // A parameter's.
    let parameter = rendered("fn f(p: i32) {\n    p = 2;\n}\nfn main() {\n    f(1);\n}\n");
    assert!(
        parameter.ends_with(
            "help: consider making this binding mutable\n  |\n1 | fn f(mut p: i32) {\n  |      +++\n"
        ),
        "{parameter}"
    );
}

/// A restricted visibility's refusals carry rustc's codes, messages and
/// labels: an item, a method, a field, a constructor or an import used
/// outside the module its `pub(..)` opens it to, a re-export beyond it,
/// and a restriction to no ancestor (#2332).
#[test]
fn a_restricted_visibility_is_refused_with_rustcs_codes() {
    let module = "mod m {\n    pub mod q {\n        pub(super) fn h() {}\n        pub struct \
                  S(pub i32);\n        impl S {\n            pub(in crate::m) fn n(&self) {}\n        \
                  }\n        pub struct P { pub(super) y: i32 }\n        pub struct T(pub(crate) \
                  i32, pub(self) i32);\n        pub fn p() -> P { P { y: 1 } }\n    }\n    \
                  pub(in crate::m) use q::S as Shown;\n}\n";
    for (body, expected) in [
        (
            "m::q::h();",
            ("E0603", "function `h` is private", "private function"),
        ),
        (
            "m::q::S(1).n();",
            ("E0624", "method `n` is private", "private method"),
        ),
        (
            "let _ = m::q::p().y;",
            (
                "E0616",
                "field `y` of struct `P` is private",
                "private field",
            ),
        ),
        (
            "let _ = m::q::P { y: 1 };",
            (
                "E0451",
                "field `y` of struct `P` is private",
                "private field",
            ),
        ),
        (
            "let _ = m::q::T(1, 2);",
            (
                "E0603",
                "tuple struct constructor `T` is private",
                "private tuple struct constructor",
            ),
        ),
        (
            "let _: m::Shown;",
            (
                "E0603",
                "struct import `Shown` is private",
                "private struct import",
            ),
        ),
    ] {
        let (code, message, label) = refused(&format!("{module}fn main() {{\n    {body}\n}}\n"));
        assert_eq!(
            (code.as_str(), message.as_str(), label.as_deref()),
            (expected.0, expected.1, Some(expected.2)),
            "{body}"
        );
    }
    let (code, message, _) = refused(
        "mod m {\n    pub mod q {\n        pub(super) fn h() {}\n    }\n    pub(crate) use q::h \
         as hh;\n}\nfn main() {}\n",
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("E0364", "`h` is private, and cannot be re-exported")
    );
    assert_eq!(
        refused(
            "mod a {\n    pub mod b {\n        pub(in crate::x) fn f() {}\n    }\n}\nmod x {}\nfn \
             main() {}\n"
        ),
        (
            "E0742".into(),
            "visibilities can only be restricted to ancestor modules".into(),
            None
        )
    );
}

/// A script type through two modules takes its generic arguments in a
/// local type, as `let _: m::E<i32>;` does through one, and too many of
/// them are rustc's E0107 (#2333).
#[test]
fn a_type_through_modules_takes_its_generic_arguments() {
    let program = "mod m {\n    pub mod q {\n        pub struct S<T>(pub T);\n    }\n    pub \
                   struct E<T>(pub T);\n}\nfn main() {\n    let _: m::E<i32>;\n    let _: \
                   m::q::S<i32>;\n    let _x: m::q::S<i32> = m::q::S(1);\n}\n";
    assert!(Engine::new().compile(program).is_ok());
    assert_eq!(
        refused(
            "mod m {\n    pub mod q {\n        pub struct S<T>(pub T);\n    }\n}\nfn main() {\n    \
             let _: m::q::S<i32, i32>;\n}\n"
        ),
        (
            "E0107".into(),
            "struct takes 1 generic argument but 2 generic arguments were supplied".into(),
            label("expected 1 generic argument")
        )
    );
}

/// A call with arguments its callee doesn't take: rustc's E0061 is at the
/// callee, labels each extra argument with its type and number (none for
/// a call's only argument), and suggests the call without them. An
/// argument fills the next parameter its type fits.
#[test]
fn an_extra_argument_is_labelled_and_its_removal_suggested() {
    let shown = rendered("fn f(a: i32) {}\nfn main() {\n    f(1, 2);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 1 argument but 2 arguments were supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     f(1, 2);\n",
            "  |     ^    - unexpected argument #2 of type `{integer}`\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn f(a: i32) {}\n",
            "  |    ^\n",
            "help: remove the extra argument\n",
            "  |\n",
            "3 -     f(1, 2);\n",
            "3 +     f(1);\n",
            "  |\n",
        )
    );
    let shown = rendered("fn f(a: i32) {}\nfn main() {\n    f(1, \"x\", 3);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 1 argument but 3 arguments were supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     f(1, \"x\", 3);\n",
            "  |     ^    ---  - unexpected argument #3 of type `{integer}`\n",
            "  |          |\n",
            "  |          unexpected argument #2 of type `&'static str`\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn f(a: i32) {}\n",
            "  |    ^\n",
            "help: remove the extra arguments\n",
            "  |\n",
            "3 -     f(1, \"x\", 3);\n",
            "3 +     f(1);\n",
            "  |\n",
        )
    );
    let shown = rendered("fn w() {}\nfn main() {\n    w(1);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 0 arguments but 1 argument was supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     w(1);\n",
            "  |     ^ - unexpected argument of type `{integer}`\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn w() {}\n",
            "  |    ^\n",
            "help: remove the extra argument\n",
            "  |\n",
            "3 -     w(1);\n",
            "3 +     w();\n",
            "  |\n",
        )
    );
    let shown = rendered("fn s(a: &str) {}\nfn main() {\n    s(1, \"x\");\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 1 argument but 2 arguments were supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     s(1, \"x\");\n",
            "  |     ^ - unexpected argument #1 of type `{integer}`\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn s(a: &str) {}\n",
            "  |    ^\n",
            "help: remove the extra argument\n",
            "  |\n",
            "3 -     s(1, \"x\");\n",
            "3 +     s(\"x\");\n",
            "  |\n",
        )
    );
}

/// A call that gives a parameter nothing: rustc labels each run of
/// missing parameters at the parentheses, or at the argument in its place,
/// marks them where the callee is defined, and suggests a placeholder of
/// each one's type.
#[test]
fn a_missing_argument_is_labelled_marked_and_provided() {
    let shown = rendered("fn f(a: i32) {}\nfn main() {\n    f();\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 1 argument but 0 arguments were supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     f();\n",
            "  |     ^-- argument #1 of type `i32` is missing\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn f(a: i32) {}\n",
            "  |    ^ ------\n",
            "help: provide the argument\n",
            "  |\n",
            "3 |     f(/* i32 */);\n",
            "  |       +++++++++\n",
        )
    );
    let shown = rendered("fn g(a: i32, b: &str) {}\nfn main() {\n    g();\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 2 arguments but 0 arguments were supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     g();\n",
            "  |     ^-- two arguments of type `i32` and `&str` are missing\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn g(a: i32, b: &str) {}\n",
            "  |    ^ ------  -------\n",
            "help: provide the arguments\n",
            "  |\n",
            "3 |     g(/* i32 */, /* &str */);\n",
            "  |       +++++++++++++++++++++\n",
        )
    );
    let shown = rendered("fn k(a: i32, b: i32, c: i32, d: i32) {}\nfn main() {\n    k(1);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 4 arguments but 1 argument was supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     k(1);\n",
            "  |     ^--- three arguments of type `i32`, `i32`, and `i32` are missing\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn k(a: i32, b: i32, c: i32, d: i32) {}\n",
            "  |    ^         ------  ------  ------\n",
            "help: provide the arguments\n",
            "  |\n",
            "3 |     k(1, /* i32 */, /* i32 */, /* i32 */);\n",
            "  |        +++++++++++++++++++++++++++++++++\n",
        )
    );
    let shown = rendered("fn k(a: i32, b: i32, c: i32, d: i32) {}\nfn main() {\n    k();\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 4 arguments but 0 arguments were supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     k();\n",
            "  |     ^-- multiple arguments are missing\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn k(a: i32, b: i32, c: i32, d: i32) {}\n",
            "  |    ^ ------  ------  ------  ------\n",
            "help: provide the arguments\n",
            "  |\n",
            "3 |     k(/* i32 */, /* i32 */, /* i32 */, /* i32 */);\n",
            "  |       ++++++++++++++++++++++++++++++++++++++++++\n",
        )
    );
    let shown = rendered("fn u(a: ()) {}\nfn main() {\n    u();\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 1 argument but 0 arguments were supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     u();\n",
            "  |     ^-- argument #1 of type `()` is missing\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn u(a: ()) {}\n",
            "  |    ^ -----\n",
            "help: provide the argument\n",
            "  |\n",
            "3 |     u(());\n",
            "  |       ++\n",
        )
    );
    let shown = rendered("fn g(a: i32, b: &str) {}\nfn main() {\n    g(\"x\");\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 2 arguments but 1 argument was supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     g(\"x\");\n",
            "  |     ^ --- argument #1 of type `i32` is missing\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:1:4\n",
            "  |\n",
            "1 | fn g(a: i32, b: &str) {}\n",
            "  |    ^ ------\n",
            "help: provide the argument\n",
            "  |\n",
            "3 |     g(/* i32 */, \"x\");\n",
            "  |       ++++++++++\n",
        )
    );
}

/// rustc's E0061 for a tuple struct's or variant's constructor, a method
/// and an associated function: at the path or the method's name, with the
/// constructor's definition noted.
#[test]
fn a_constructors_or_methods_argument_count_is_at_its_callee() {
    let shown = rendered("struct P(i32, u8);\nfn main() {\n    let _ = P(1);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this struct takes 2 arguments but 1 argument was supplied\n",
            " --> main.rs:3:13\n",
            "  |\n",
            "3 |     let _ = P(1);\n",
            "  |             ^--- argument #2 of type `u8` is missing\n",
            "  |\n",
            "note: tuple struct defined here\n",
            " --> main.rs:1:8\n",
            "  |\n",
            "1 | struct P(i32, u8);\n",
            "  |        ^\n",
            "help: provide the argument\n",
            "  |\n",
            "3 |     let _ = P(1, /* u8 */);\n",
            "  |                ++++++++++\n",
        )
    );
    let shown = rendered("enum E { V(i32) }\nfn main() {\n    let _ = E::V(1, 2);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this enum variant takes 1 argument but 2 arguments were supplied\n",
            " --> main.rs:3:13\n",
            "  |\n",
            "3 |     let _ = E::V(1, 2);\n",
            "  |             ^^^^    - unexpected argument #2 of type `{integer}`\n",
            "  |\n",
            "note: tuple variant defined here\n",
            " --> main.rs:1:10\n",
            "  |\n",
            "1 | enum E { V(i32) }\n",
            "  |          ^\n",
            "help: remove the extra argument\n",
            "  |\n",
            "3 -     let _ = E::V(1, 2);\n",
            "3 +     let _ = E::V(1);\n",
            "  |\n",
        )
    );
    let shown =
        rendered("struct S;\nimpl S { fn m(&self, a: i32) {} }\nfn main() {\n    S.m();\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this method takes 1 argument but 0 arguments were supplied\n",
            " --> main.rs:4:7\n",
            "  |\n",
            "4 |     S.m();\n",
            "  |       ^-- argument #1 of type `i32` is missing\n",
            "  |\n",
            "note: method defined here\n",
            " --> main.rs:2:13\n",
            "  |\n",
            "2 | impl S { fn m(&self, a: i32) {} }\n",
            "  |             ^        ------\n",
            "help: provide the argument\n",
            "  |\n",
            "4 |     S.m(/* i32 */);\n",
            "  |         +++++++++\n",
        )
    );
    let shown =
        rendered("struct S;\nimpl S { fn n(a: i32) {} }\nfn main() {\n    S::n(1, 2);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 1 argument but 2 arguments were supplied\n",
            " --> main.rs:4:5\n",
            "  |\n",
            "4 |     S::n(1, 2);\n",
            "  |     ^^^^    - unexpected argument #2 of type `{integer}`\n",
            "  |\n",
            "note: associated function defined here\n",
            " --> main.rs:2:13\n",
            "  |\n",
            "2 | impl S { fn n(a: i32) {} }\n",
            "  |             ^\n",
            "help: remove the extra argument\n",
            "  |\n",
            "4 -     S::n(1, 2);\n",
            "4 +     S::n(1);\n",
            "  |\n",
        )
    );
    let shown = rendered("mod m {\n    pub fn f(a: i32) {}\n}\nfn main() {\n    m::f(1, 2);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0061]: this function takes 1 argument but 2 arguments were supplied\n",
            " --> main.rs:5:5\n",
            "  |\n",
            "5 |     m::f(1, 2);\n",
            "  |     ^^^^    - unexpected argument #2 of type `{integer}`\n",
            "  |\n",
            "note: function defined here\n",
            " --> main.rs:2:12\n",
            "  |\n",
            "2 |     pub fn f(a: i32) {}\n",
            "  |            ^\n",
            "help: remove the extra argument\n",
            "  |\n",
            "5 -     m::f(1, 2);\n",
            "5 +     m::f(1);\n",
            "  |\n",
        )
    );
    let shown = rendered("fn main() {\n    let f = |x: i64, y: i64| x;\n    f(1);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0057]: this function takes 2 arguments but 1 argument was supplied\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "3 |     f(1);\n",
            "  |     ^--- argument #2 of type `i64` is missing\n",
            "  |\n",
            "note: closure defined here\n",
            " --> main.rs:2:13\n",
            "  |\n",
            "2 |     let f = |x: i64, y: i64| x;\n",
            "  |             ^^^^^^^^^^^^^^^^\n",
            "help: provide the argument\n",
            "  |\n",
            "3 |     f(1, /* i64 */);\n",
            "  |        +++++++++++\n",
        )
    );
}

/// A program's bound a call's argument doesn't meet: rustc's E0277 is at
/// the argument, labels the callee, notes the bound where the function
/// writes it, and shows `Display`'s help at the type's definition.
#[test]
fn an_unmet_bound_is_at_the_argument_with_the_bound_noted() {
    let shown = rendered("use std::fmt::Display;\nstruct Q;\nfn show<T: Display>(t: T) {}\nfn main() {\n    show(Q);\n}\n");
    assert_eq!(
        shown,
        concat!(
            "error[E0277]: `Q` doesn't implement `std::fmt::Display`\n",
            " --> main.rs:5:10\n",
            "  |\n",
            "5 |     show(Q);\n",
            "  |     ---- ^ unsatisfied trait bound\n",
            "  |     |\n",
            "  |     required by a bound introduced by this call\n",
            "  |\n",
            "help: the trait `std::fmt::Display` is not implemented for `Q`\n",
            " --> main.rs:2:1\n",
            "  |\n",
            "2 | struct Q;\n",
            "  | ^^^^^^^^\n",
            "note: required by a bound in `show`\n",
            " --> main.rs:3:12\n",
            "  |\n",
            "3 | fn show<T: Display>(t: T) {}\n",
            "  |            ^^^^^^^ required by this bound in `show`\n",
        )
    );
}

/// A derivable bound a program's type doesn't meet: rustc suggests the
/// `#[derive]` on its own line above the type, as indented. rustc shows
/// the type's line after the added one, which annotate-snippets doesn't.
#[test]
fn an_unmet_derivable_bound_suggests_the_derive() {
    let shown = rendered(
        "struct Q;\nfn clone_it<T>(t: T) where T: Clone {}\nfn main() {\n    clone_it(Q);\n}\n",
    );
    assert!(
        shown.starts_with(concat!(
            "error[E0277]: the trait bound `Q: Clone` is not satisfied\n",
            " --> main.rs:4:14\n",
            "  |\n",
            "4 |     clone_it(Q);\n",
            "  |     -------- ^ the trait `Clone` is not implemented for `Q`\n",
            "  |     |\n",
            "  |     required by a bound introduced by this call\n",
            "  |\n",
            "note: required by a bound in `clone_it`\n",
            " --> main.rs:2:31\n",
            "  |\n",
            "2 | fn clone_it<T>(t: T) where T: Clone {}\n",
            "  |                               ^^^^^ required by this bound in `clone_it`\n",
            "help: consider annotating `Q` with `#[derive(Clone)]`\n",
            "  |\n",
            "1 + #[derive(Clone)]\n",
        )),
        "{shown}"
    );
    let shown = rendered("use std::fmt::Debug;\nmod m {\n    pub struct Q;\n}\nfn dbg<T: Debug>(t: T) {}\nfn main() {\n    dbg(m::Q);\n}\n");
    assert!(
        shown.starts_with(concat!(
            "error[E0277]: `Q` doesn't implement `Debug`\n",
            " --> main.rs:7:9\n",
            "  |\n",
            "7 |     dbg(m::Q);\n",
            "  |     --- ^^^^ the trait `Debug` is not implemented for `Q`\n",
            "  |     |\n",
            "  |     required by a bound introduced by this call\n",
            "  |\n",
            "note: required by a bound in `dbg`\n",
            " --> main.rs:5:11\n",
            "  |\n",
            "5 | fn dbg<T: Debug>(t: T) {}\n",
            "  |           ^^^^^ required by this bound in `dbg`\n",
            "help: consider annotating `Q` with `#[derive(Debug)]`\n",
            "  |\n",
            "3 +     #[derive(Debug)]\n",
        )),
        "{shown}"
    );
    let shown = rendered("mod m {\n    pub struct Q;\n}\nfn eq<T: PartialEq>(a: i32, t: T) {}\nfn main() {\n    eq(1, m::Q);\n}\n");
    assert!(
        shown.starts_with(concat!(
            "error[E0277]: can't compare `Q` with `Q`\n",
            " --> main.rs:6:11\n",
            "  |\n",
            "6 |     eq(1, m::Q);\n",
            "  |     --    ^^^^ no implementation for `Q == Q`\n",
            "  |     |\n",
            "  |     required by a bound introduced by this call\n",
            "  |\n",
            "  = help: the trait `PartialEq` is not implemented for `Q`\n",
            "note: required by a bound in `eq`\n",
            " --> main.rs:4:10\n",
            "  |\n",
            "4 | fn eq<T: PartialEq>(a: i32, t: T) {}\n",
            "  |          ^^^^^^^^^ required by this bound in `eq`\n",
            "help: consider annotating `Q` with `#[derive(PartialEq)]`\n",
            "  |\n",
            "2 +     #[derive(PartialEq)]\n",
        )),
        "{shown}"
    );
}

/// A `return;` where the function returns a value: rustc labels the
/// output type and suggests the value its type takes, as `42` for an
/// integer and `Default::default()` for a type with a default.
#[test]
fn a_return_without_a_value_suggests_one_of_the_output_type() {
    assert_eq!(
        rendered("fn f() -> i32 {\n    return;\n}\nfn main() {}\n"),
        concat!(
            "error[E0069]: `return;` in a function whose return type is not `()`\n",
            " --> main.rs:2:5\n",
            "  |\n",
            "1 | fn f() -> i32 {\n",
            "  |           --- expected `i32` because of this return type\n",
            "2 |     return;\n",
            "  |     ^^^^^^ return type is not `()`\n",
            "  |\n",
            "help: give the `return` a value of the expected type\n",
            "  |\n",
            "2 |     return 42;\n",
            "  |            ++\n",
        )
    );
    let value = |output: &str| {
        let shown = rendered(&format!(
            "struct P;\n#[derive(Default)]\nstruct D;\nfn f() -> {output} {{ return; }}\n\
             fn main() {{}}\n"
        ));
        let line = shown
            .lines()
            .rfind(|line| line.starts_with("4 | fn f()"))
            .unwrap_or_else(|| panic!("{shown}"))
            .to_string();
        let value = line.rfind("return ").expect("the suggested return") + "return ".len();
        line[value..].trim_end_matches("; }").to_string()
    };
    assert_eq!(value("String"), "Default::default()");
    assert_eq!(value("D"), "Default::default()");
    assert_eq!(value("P"), "/* value */");
    assert_eq!(value("Option<i32>"), "None");
    assert_eq!(value("Result<i32, String>"), "Ok(42)");
    assert_eq!(value("Vec<i32>"), "vec![]");
    assert_eq!(value("(i32, bool)"), "(42, false)");
    assert_eq!(value("&'static str"), "\"\"");
    assert_eq!(value("[i32; 3]"), "[42; 3]");
    assert_eq!(value("[String; 2]"), "/* value */");
    assert_eq!(value("Box<f64>"), "Box::new(3.14159)");
    assert_eq!(value("char"), "'x'");
}

/// A `let` whose local a use reads before any value: rustc suggests the
/// value after its name or its type, when the `let` declares it alone.
#[test]
fn an_uninitialized_local_suggests_assigning_a_value() {
    assert_eq!(
        rendered("fn main() {\n    let mut x: i32;\n    x += 1;\n}\n"),
        concat!(
            "error[E0381]: used binding `x` isn't initialized\n",
            " --> main.rs:3:5\n",
            "  |\n",
            "2 |     let mut x: i32;\n",
            "  |         ----- binding declared here but left uninitialized\n",
            "3 |     x += 1;\n",
            "  |     ^ `x` used here but it isn't initialized\n",
            "  |\n",
            "help: consider assigning a value\n",
            "  |\n",
            "2 |     let mut x: i32 = 42;\n",
            "  |                    ++++\n",
        )
    );
    let shown = rendered("fn main() {\n    let x;\n    let y: i32 = x;\n}\n");
    assert!(
        shown.ends_with("2 |     let x = 42;\n  |           ++++\n"),
        "{shown}"
    );
    let shown = rendered("fn main() {\n    let a  :  String ;\n    println!(\"{a}\");\n}\n");
    assert!(
        shown.ends_with(concat!(
            "2 |     let a  :  String = Default::default() ;\n",
            "  |                      ++++++++++++++++++++\n",
        )),
        "{shown}"
    );
    // A tuple's local isn't its pattern alone.
    let shown = rendered("fn main() {\n    let (f, g): (i32, i32);\n    let c = f;\n}\n");
    assert!(!shown.contains("help:"), "{shown}");
}

/// A refutable `let`: rustc writes `else` after the value of one that
/// binds names, and `if` before one that binds none.
#[test]
fn a_refutable_let_suggests_let_else_or_if_let_as_code() {
    let shown = rendered("fn main() {\n    let o: Option<i32> = None;\n    let Some(x) = o;\n}\n");
    assert!(
        shown.ends_with(concat!(
            "help: you might want to use `let...else` to handle the variant that isn't matched\n",
            "  |\n",
            "3 |     let Some(x) = o else { todo!() };\n",
            "  |                     ++++++++++++++++\n",
        )),
        "{shown}"
    );
    let shown = rendered("fn main() {\n    let o: Option<i32> = None;\n    let None = o;\n}\n");
    assert!(
        shown.ends_with(concat!(
            "help: you might want to use `if let` to ignore the variant that isn't matched\n",
            "  |\n",
            "3 |     if let None = o { todo!() };\n",
            "  |     ++              +++++++++++\n",
        )),
        "{shown}"
    );
    let shown = rendered(
        "fn main() {\n    let o: Result<i32, i32> = Ok(1);\n    let Ok(x) =\n        o;\n}\n",
    );
    assert!(
        shown.ends_with("4 |         o else { todo!() };\n  |           ++++++++++++++++\n"),
        "{shown}"
    );
}

/// A `match` that misses values: rustc writes the arm that covers them
/// after the last arm, on a line of its own when the arms are, or the
/// braces again around it for a `match` with none.
#[test]
fn a_non_exhaustive_match_suggests_the_arm_as_code() {
    let suggested = |arms: &str| {
        let shown = rendered(&format!(
            "enum E {{ A, B, C }}\nfn main() {{\n    let e = E::A;\n    {arms}\n}}\n"
        ));
        let help = shown.find("help: ").unwrap_or_else(|| panic!("{shown}"));
        let lines = shown[help..].lines().skip(2);
        lines
            .take_while(|line| line.trim() != "|")
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    };
    assert_eq!(
        suggested("match e {\n        E::A => {}\n        E::B => {}\n    }"),
        "6 ~         E::B => {},\n7 +         E::C => todo!()\n"
    );
    assert_eq!(
        suggested("match e {\n        E::A => 1,\n    };"),
        "5 ~         E::A => 1,\n6 ~         E::B | E::C => todo!(),\n"
    );
    assert_eq!(
        suggested("match e {}"),
        "4 ~     match e {\n5 +         E::A | E::B | E::C => todo!(),\n6 ~     }\n"
    );
    assert_eq!(
        suggested("let _ = match e { E::A => 1, E::B => 2 };"),
        concat!(
            "4 |     let _ = match e { E::A => 1, E::B => 2, E::C => todo!() };\n",
            "  |                                           +++++++++++++++++\n",
        )
    );
}

/// A binding a destructuring pattern declares, assigned twice: rustc
/// suggests `mut`, and `ref mut`, which changes the matched value.
#[test]
fn a_destructured_binding_assigned_twice_suggests_a_borrow_too() {
    let borrow = "help: to modify the original value, take a borrow instead\n  |\n";
    for (source, line) in [
        (
            "fn main() {\n    let t = (1, 2);\n    let (a, b) = t;\n    a = 3;\n}\n",
            "3 |     let (ref mut a, b) = t;\n  |          +++++++\n",
        ),
        (
            "struct S { x: i32 }\nfn main() {\n    let s = S { x: 1 };\n    let S { x } = s;\n    \
             x = 2;\n}\n",
            "4 |     let S { ref mut x } = s;\n  |             +++++++\n",
        ),
        (
            "fn main() {\n    let o = Some(1);\n    if let Some(a) = o { a = 2; }\n}\n",
            "3 |     if let Some(ref mut a) = o { a = 2; }\n  |                 +++++++\n",
        ),
    ] {
        let shown = rendered(source);
        assert!(shown.ends_with(&format!("{borrow}{line}")), "{shown}");
    }
    // A `let` of the binding alone suggests only `mut`.
    let shown = rendered("fn main() {\n    let a = 1;\n    a = 2;\n}\n");
    assert!(!shown.contains("ref mut"), "{shown}");
}

/// Two imports of one name: rustc marks both paths, notes the namespace
/// and suggests renaming the second with `as`.
#[test]
fn a_name_imported_twice_suggests_a_rename_as_code() {
    assert_eq!(
        rendered("use std::fmt::Result;\nuse std::io::Result;\nfn main() {}\n"),
        concat!(
            "error[E0252]: the name `Result` is defined multiple times\n",
            " --> main.rs:2:5\n",
            "  |\n",
            "1 | use std::fmt::Result;\n",
            "  |     ---------------- previous import of the type `Result` here\n",
            "2 | use std::io::Result;\n",
            "  |     ^^^^^^^^^^^^^^^ `Result` reimported here\n",
            "  |\n",
            "  = note: `Result` must be defined only once in the type namespace of this module\n",
            "help: you can use `as` to change the binding name of the import\n",
            "  |\n",
            "2 | use std::io::Result as OtherResult;\n",
            "  |                     ++++++++++++++\n",
        )
    );
}

/// A field called like a method: rustc labels the field and suggests
/// removing the arguments beside them.
#[test]
fn a_called_field_suggests_removing_the_arguments() {
    let shown = rendered(
        "struct S {\n    f: i32,\n}\nfn main() {\n    let s = S { f: 1 };\n    s.f();\n}\n",
    );
    assert!(
        shown.ends_with(concat!(
            "6 |     s.f();\n",
            "  |       ^-- help: remove the arguments\n",
            "  |       |\n",
            "  |       field, not a method\n",
        )),
        "{shown}"
    );
}

/// A local whose type nothing settles: rustc marks its binding and
/// suggests its type after its name, each unknown part named by the
/// parameter it fills.
#[test]
fn a_local_type_nothing_settles_suggests_an_explicit_type() {
    let shown = rendered(
        "fn main() {\n    let mut numbers = Vec::new();\n    let n1: u8 = 42;\n    \
         numbers.push(n1.into());\n    let n2: i8 = -1;\n    numbers.push(n2.into());\n    \
         println!(\"{numbers:?}\");\n}\n",
    );
    assert!(shown.contains("  |         ^^^^^^^^^^^   "), "{shown}");
    assert!(
        shown.ends_with(concat!(
            "help: consider giving `numbers` an explicit type, where the type for type \
             parameter `T` is specified\n",
            "  |\n",
            "2 |     let mut numbers: Vec<T> = Vec::new();\n",
            "  |                    ++++++++\n",
        )),
        "{shown}"
    );
    let shown = rendered("fn main() {\n    let x;\n    println!(\"{x}\");\n}\n");
    assert!(
        shown.ends_with(concat!(
            "help: consider giving `x` an explicit type\n",
            "  |\n",
            "2 |     let x: /* Type */;\n",
            "  |          ++++++++++++\n",
        )),
        "{shown}"
    );
}

/// A derivable trait a program's type lacks where a format string needs
/// it: rustc suggests the `#[derive]`, and shows `Display`'s help at the
/// type's definition.
#[test]
fn a_format_argument_lacking_a_trait_suggests_the_derive() {
    let shown = rendered("struct S;\nfn main() {\n    println!(\"{:?}\", S);\n}\n");
    assert!(
        shown.ends_with(concat!(
            "help: consider annotating `S` with `#[derive(Debug)]`\n",
            "  |\n",
            "1 + #[derive(Debug)]\n",
            "  |\n",
        )),
        "{shown}"
    );
    assert_eq!(
        rendered("struct S;\nfn main() {\n    println!(\"{}\", S);\n}\n"),
        concat!(
            "error[E0277]: `S` doesn't implement `std::fmt::Display`\n",
            " --> main.rs:3:20\n",
            "  |\n",
            "3 |     println!(\"{}\", S);\n",
            "  |               --   ^ `S` cannot be formatted with the default formatter\n",
            "  |               |\n",
            "  |               required by this formatting parameter\n",
            "  |\n",
            "help: the trait `std::fmt::Display` is not implemented for `S`\n",
            " --> main.rs:1:1\n",
            "  |\n",
            "1 | struct S;\n",
            "  | ^^^^^^^^\n",
            "  = note: in format strings you may be able to use `{:?}` (or {:#?} for pretty-print) \
             instead\n",
        )
    );
}

/// A function no `use` brings into scope, which another module declares:
/// rustc suggests the `use` at the module's first `use`, else at its
/// first item, as indented.
#[test]
fn a_function_another_module_declares_suggests_its_import() {
    let suggested = |source: &str| {
        let shown = rendered(source);
        let help = shown.find("help: ").unwrap_or_else(|| panic!("{shown}"));
        shown[help..].to_string()
    };
    let help = |line: &str| format!("help: consider importing this function\n  |\n{line}\n  |\n");
    assert_eq!(
        suggested("mod m {\n    pub fn f() {}\n}\nfn main() {\n    f();\n}\n"),
        help("1 + use crate::m::f;")
    );
    assert_eq!(
        suggested(
            "// a comment\n\n#[derive(Debug)]\nstruct S;\nmod m {\n    pub fn f() {}\n}\nfn \
             main() {\n    f();\n}\n"
        ),
        help("3 + use crate::m::f;")
    );
    assert_eq!(
        suggested(
            "mod m {\n    pub fn f() {}\n}\nfn g() {}\nuse std::fmt;\nfn main() {\n    f();\n}\n"
        ),
        help("5 + use crate::m::f;")
    );
    assert_eq!(
        suggested(
            "mod m {\n    pub fn f() {}\n}\nmod n {\n    pub fn g() {\n        f();\n    \
             }\n}\nfn main() {}\n"
        ),
        help("5 +     use crate::m::f;")
    );
    // A private function is no import.
    let shown = rendered("mod m {\n    fn f() {}\n}\nfn main() {\n    f();\n}\n");
    assert!(!shown.contains("help:"), "{shown}");
}

/// `&mut (place)` borrows the place, as rustc reads parentheses, and its
/// span is the whole expression.
#[test]
fn a_parenthesized_place_is_borrowed_whole() {
    Engine::new()
        .compile(
            "fn main() {\n    let mut c = 1;\n    let r = &mut (c);\n    *r = 2;\n    \
             println!(\"{c}\");\n}\n",
        )
        .map(|_| ())
        .unwrap_or_else(|error| panic!("{error}"));
    let shown = rendered(
        "fn take(v: &mut Vec<i32>) { v.push(1); }\nfn main() {\n    let v = vec![1];\n    \
         take(&mut (v));\n}\n",
    );
    assert!(
        shown.contains("4 |     take(&mut (v));\n  |          ^^^^^^^^"),
        "{shown}"
    );
}

/// A crate trait's method is callable only where the trait is in scope:
/// rustc's E0599 at the method, with its help and the `use` it suggests
/// (#2335). A trait in scope by any import or in its own module is
/// called, and an inherent method of the name comes first.
#[test]
fn a_crate_traits_method_out_of_scope_is_e0599() {
    let source = "mod m {\n    pub trait T {\n        fn t(&self) {}\n    }\n    impl T for i32 \
                  {}\n}\nfn main() {\n    1i32.t();\n}\n";
    assert_eq!(
        refused(source),
        (
            "E0599".into(),
            "no method named `t` found for type `i32` in the current scope".into(),
            label("method not found in `i32`")
        )
    );
    let report = rendered(source);
    assert!(report.contains(" --> main.rs:8:10"), "{report}");
    assert!(
        report.ends_with(concat!(
            "  = help: items from traits can only be used if the trait is in scope\n",
            "help: trait `T` which provides `t` is implemented but not in scope; perhaps you \
             want to import it\n",
            "  |\n",
            "1 + use crate::m::T;\n",
            "  |\n",
        )),
        "{report}"
    );
    // A struct's, an enum's and a reference's, from another module.
    let owner = |declared: &str, made: &str| {
        refused(&format!(
            "mod m {{\n    pub trait T {{\n        fn t(&self) -> i32 {{\n            1\n        \
             }}\n    }}\n    {declared}\n}}\nmod n {{\n    pub fn f() -> i32 {{\n        let v = \
             {made};\n        v.t()\n    }}\n}}\nfn main() {{\n    n::f();\n}}\n"
        ))
        .1
    };
    assert_eq!(
        owner("pub struct S;\n    impl T for S {}", "crate::m::S"),
        "no method named `t` found for struct `S` in the current scope"
    );
    assert_eq!(
        owner(
            "pub enum E {\n        A,\n    }\n    impl T for E {}",
            "crate::m::E::A"
        ),
        "no method named `t` found for enum `E` in the current scope"
    );
    assert_eq!(
        owner("pub struct S;\n    impl T for S {}", "&crate::m::S"),
        "no method named `t` found for reference `&S` in the current scope"
    );
    // In scope by a `use`, a glob, `as _`, a rename, `use super::*`, a
    // block's `use`, or its own module.
    let traits = "mod m {\n    pub trait T {\n        fn t(&self) -> i32 {\n            1\n        \
                  }\n    }\n    impl T for i32 {}\n    pub fn own() -> i32 {\n        2i32.t()\n    \
                  }\n    pub mod child {\n        use super::*;\n        pub fn f() -> i32 {\n            \
                  3i32.t()\n        }\n    }\n}\n";
    for caller in [
        "use m::T;\nfn f() -> i32 {\n    1i32.t()\n}\n",
        "use m::*;\nfn f() -> i32 {\n    1i32.t()\n}\n",
        "use m::T as _;\nfn f() -> i32 {\n    1i32.t()\n}\n",
        "use m::T as Other;\nfn f() -> i32 {\n    1i32.t()\n}\n",
        "fn f() -> i32 {\n    use crate::m::T;\n    1i32.t()\n}\n",
        "fn f() -> i32 {\n    m::own() + m::child::f()\n}\n",
        "use m::T;\nstruct S;\nimpl S {\n    fn t(&self) -> i32 {\n        5\n    }\n}\nimpl T \
         for S {}\nfn f() -> i32 {\n    S.t()\n}\n",
    ] {
        let source = format!("{traits}{caller}fn main() {{\n    f();\n}}\n");
        Engine::new()
            .compile(&source)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
    }
    // A block's `use` reaches its block alone.
    let (code, ..) = refused(&format!(
        "{traits}fn g() {{\n    use crate::m::T;\n}}\nfn f() -> i32 {{\n    1i32.t()\n}}\nfn \
         main() {{\n    g();\n    f();\n}}\n"
    ));
    assert_eq!(code, "E0599");
}

/// `&mut place` of a place a local not declared `mut` owns is rustc's
/// E0596 at the borrow, with its `mut` suggestion, whether or not the
/// reference is used (#2336). A place below a `&mut` local needs no `mut`.
#[test]
fn a_mut_borrow_of_a_local_not_declared_mut_is_e0596() {
    let source = "fn main() {\n    let c = 1;\n    let r = &mut c;\n}\n";
    assert_eq!(
        refused(source),
        (
            "E0596".into(),
            "cannot borrow `c` as mutable, as it is not declared as mutable".into(),
            label("cannot borrow as mutable")
        )
    );
    let report = rendered(source);
    assert!(
        report.ends_with(concat!(
            " --> main.rs:3:13\n",
            "  |\n",
            "3 |     let r = &mut c;\n",
            "  |             ^^^^^^ cannot borrow as mutable\n",
            "  |\n",
            "help: consider changing this to be mutable\n",
            "  |\n",
            "2 |     let mut c = 1;\n",
            "  |         +++\n",
        )),
        "{report}"
    );
    // At the borrow, before a use of the reference.
    let used = rendered("fn main() {\n    let c = 1;\n    let r = &mut c;\n    *r = 2;\n}\n");
    assert!(used.contains(" --> main.rs:3:13\n"), "{used}");
    let message = |body: &str| {
        refused(&format!(
            "struct S {{\n    p: (i32, i32),\n}}\nfn main() {{\n    {body}\n}}\n"
        ))
        .1
    };
    assert_eq!(
        message("let s = S { p: (1, 2) };\n    let r = &mut s.p.0;"),
        "cannot borrow `s.p.0` as mutable, as `s` is not declared as mutable"
    );
    assert_eq!(
        message("let a = [1, 2];\n    let r = &mut a[0];"),
        "cannot borrow `a[_]` as mutable, as `a` is not declared as mutable"
    );
    assert_eq!(
        message("let v = vec![1];\n    let r = &mut v[0];"),
        "cannot borrow `v` as mutable, as it is not declared as mutable"
    );
    // Below a `&mut` local, the referent is changed.
    Engine::new()
        .compile(
            "struct S {\n    a: i32,\n}\nfn main() {\n    let mut v = S { a: 1 };\n    let r = \
             &mut v;\n    let q = &mut r.a;\n    *q = 3;\n    let w = &mut *r;\n    w.a += 1;\n    \
             println!(\"{}\", v.a);\n}\n",
        )
        .map(|_| ())
        .unwrap_or_else(|error| panic!("{error}"));
}

/// What rawr renders for `body`, the body of `main` beside `struct Value`
/// and `struct G<T>`, neither with an impl of an operator's trait.
fn rendered_operator(body: &str) -> String {
    rendered(&format!(
        "struct Value {{ inner: i64 }}\nstruct G<T>(T);\nfn main() {{\n    {body}\n}}\n"
    ))
}

#[test]
fn an_operator_a_program_type_has_no_impl_for_labels_its_operands() {
    // Each operand labeled with its type, and the impl noted at the type
    // (#2340).
    let added = rendered_operator("let a = Value { inner: 1 };\n    let _ = a + a;");
    assert!(
        added.starts_with(concat!(
            "error[E0369]: cannot add `Value` to `Value`\n",
            " --> main.rs:5:15\n",
            "  |\n",
            "5 |     let _ = a + a;\n",
            "  |             - ^ - Value\n",
            "  |             |\n",
            "  |             Value\n",
            "  |\n",
            "note: an implementation of `Add` might be missing for `Value`\n",
            " --> main.rs:1:1\n",
            "  |\n",
            "1 | struct Value { inner: i64 }\n",
            "  | ^^^^^^^^^^^^ must implement `Add`\n",
        )),
        "{added}"
    );
    // The arithmetic and bit traits don't derive.
    assert!(!added.contains("derive"), "{added}");
    // A right operand of another type is the trait's argument.
    let shifted = rendered_operator("let a = Value { inner: 1 };\n    let _ = a << 1;");
    assert!(
        shifted.contains("error[E0369]: no implementation for `Value << {integer}`\n"),
        "{shifted}"
    );
    assert!(
        shifted.contains("  |             - ^^ - {integer}\n"),
        "{shifted}"
    );
    assert!(
        shifted.contains("^^^^^^^^^^^^ must implement `Shl<{integer}>`\n"),
        "{shifted}"
    );
    let ored = rendered_operator("let a = Value { inner: 1 };\n    let _ = a | a;");
    assert!(
        ored.contains("error[E0369]: no implementation for `Value | Value`\n")
            && ored.contains("must implement `BitOr`\n"),
        "{ored}"
    );
    // A reference's operator is the reference's, which no impl is noted
    // for.
    let borrowed = rendered_operator("let a = Value { inner: 1 };\n    let _ = &a * &a;");
    assert!(
        borrowed.contains("error[E0369]: cannot multiply `&Value` by `&Value`\n")
            && borrowed.contains("  |             -- ^ -- &Value\n")
            && !borrowed.contains("note:"),
        "{borrowed}"
    );
    // A generic type's impl names no argument when the operands' types
    // agree.
    let generic = rendered_operator("let g = G(1i32);\n    let _ = g - G(2i32);");
    assert!(
        generic.contains("error[E0369]: cannot subtract `G<i32>` from `G<i32>`\n")
            && generic.contains("^^^^^^^^^^^ must implement `Sub`\n"),
        "{generic}"
    );
}

#[test]
fn a_compound_assignment_a_program_type_has_no_impl_for_is_e0368() {
    // At the whole assignment, its left operand labeled (#2340).
    let added = rendered_operator("let mut a = Value { inner: 1 };\n    a += Value { inner: 2 };");
    assert!(
        added.starts_with(concat!(
            "error[E0368]: binary assignment operation `+=` cannot be applied to type `Value`\n",
            " --> main.rs:5:5\n",
            "  |\n",
            "5 |     a += Value { inner: 2 };\n",
            "  |     -^^^^^^^^^^^^^^^^^^^^^^\n",
            "  |     |\n",
            "  |     cannot use `+=` on type `Value`\n",
            "  |\n",
            "note: an implementation of `AddAssign` might be missing for `Value`\n",
        )),
        "{added}"
    );
    let xored = rendered_operator("let mut c = Value { inner: 1 };\n    c ^= 3;");
    assert!(
        xored.contains("  |     -^^^^^\n")
            && xored.contains("must implement `BitXorAssign<{integer}>`\n"),
        "{xored}"
    );
    let shifted = rendered_operator("let mut c = Value { inner: 1 };\n    c <<= 2;");
    assert!(
        shifted.contains(
            "error[E0368]: binary assignment operation `<<=` cannot be applied to type `Value`\n"
        ) && shifted.contains("must implement `ShlAssign<{integer}>`\n"),
        "{shifted}"
    );
}

#[test]
fn a_comparison_with_another_type_names_it_in_the_missing_impl() {
    // rustc finds no `PartialEq` before it reads the right operand: no
    // mismatch, and no derive, which gives no impl for another type.
    let compared = rendered_operator("let a = Value { inner: 1 };\n    let _ = a == 1;");
    assert!(
        compared
            .contains("error[E0369]: binary operation `==` cannot be applied to type `Value`\n")
            && compared.contains("  |             - ^^ - {integer}\n")
            && compared.contains("must implement `PartialEq<{integer}>`\n")
            && !compared.contains("derive"),
        "{compared}"
    );
    // A generic type's own: the trait alone, and its derive.
    let generic = rendered_operator("let g = G(1i32);\n    let _ = g == G(2i32);");
    assert!(
        generic.contains("^^^^^^^^^^^ must implement `PartialEq`\n")
            && generic.contains("help: consider annotating `G<i32>` with `#[derive(PartialEq)]`\n"),
        "{generic}"
    );
}

#[test]
fn an_unresolved_import_marks_the_whole_import() {
    // The whole import, through its `as` name, labeled at its last
    // segment (#2340).
    let module = "mod model {\n    pub struct Thing;\n}\n";
    let shown = |import: &str| rendered(&format!("{module}{import}\nfn main() {{}}\n"));
    let missing = shown("use crate::model::missing;");
    assert!(
        missing.starts_with(concat!(
            "error[E0432]: unresolved import `crate::model::missing`\n",
            " --> main.rs:4:5\n",
            "  |\n",
            "4 | use crate::model::missing;\n",
            "  |     ^^^^^^^^^^^^^^-------\n",
            "  |                   |\n",
            "  |                   no `missing` in `model`\n",
        )),
        "{missing}"
    );
    let renamed = shown("use self::model::missing as other;");
    assert!(
        renamed.contains("  |     ^^^^^^^^^^^^^-------^^^^^^^^^\n"),
        "{renamed}"
    );
    // A group's member alone.
    let grouped = shown("use crate::model::{Thing, absent};");
    assert!(
        grouped.contains("  |                           ^^^^^^ no `absent` in `model`\n"),
        "{grouped}"
    );
    let aliased = shown("use crate::model::{absent as other};");
    assert!(
        aliased.contains("  |                    ------^^^^^^^^^\n"),
        "{aliased}"
    );
    // std's modules are named from its root.
    let std = shown("use std::collections::hash_map::Nope;");
    assert!(
        std.contains("  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^----\n")
            && std.contains("no `Nope` in `collections::hash_map`\n"),
        "{std}"
    );
    // A module missing on the way is marked alone.
    let through = shown("use std::nofs::x;");
    assert!(
        through.starts_with("error[E0432]: unresolved import `std::nofs`\n")
            && through.contains("  |          ^^^^ could not find `nofs` in `std`\n"),
        "{through}"
    );
    // A group's member path from the member, not the group's prefix
    // (#2341).
    let inner = "mod model {\n    pub mod inner {}\n}\n";
    let nested = rendered(&format!(
        "{inner}use crate::model::{{inner::gone}};\nfn main() {{}}\n"
    ));
    assert!(
        nested.starts_with(
            "error[E0432]: unresolved import `crate::model::inner::gone`\n --> main.rs:4:20\n"
        ) && nested.contains("  |                    ^^^^^^^----\n"),
        "{nested}"
    );
    let std_nested = shown("use std::collections::{hash_map::Nope};");
    assert!(
        std_nested.contains("  |                        ^^^^^^^^^^----\n"),
        "{std_nested}"
    );
    // A std prefix's miss is at the missing segment: a glob's, or the
    // module `self` imports.
    for import in [
        "use std::nope::*;",
        "use std::nope::{self};",
        "use std::nope::{a, b};",
    ] {
        let prefix = shown(import);
        assert!(
            prefix.contains("  |          ^^^^ could not find `nope` in `std`\n"),
            "{prefix}"
        );
    }
    // rustc names the module a segment is missing from by its own name.
    let module = rendered(&format!(
        "{inner}use crate::model::inner::deep::gone;\nfn main() {{}}\n"
    ));
    assert!(
        module.contains("  |                          ^^^^ could not find `deep` in `inner`\n"),
        "{module}"
    );
}

#[test]
fn an_impl_of_a_path_that_names_no_trait_is_refused_as_rustc_does() {
    let shown = |header: &str| {
        rendered(&format!(
            "mod m {{\n    pub trait T {{}}\n    pub struct U;\n    pub fn g() {{}}\n}}\nstruct \
             S;\n{header} for S {{}}\nfn main() {{}}\n"
        ))
    };
    let missing = shown("impl crate::m::Nope");
    assert!(
        missing.starts_with("error[E0405]: cannot find trait `Nope` in module `crate::m`\n")
            && missing.contains("  |                ^^^^ not found in `crate::m`\n"),
        "{missing}"
    );
    let structure = shown("impl m::U");
    assert!(
        structure.starts_with("error[E0404]: expected trait, found struct `m::U`\n")
            && structure.contains("  |      ^^^^ not a trait\n"),
        "{structure}"
    );
    let function = shown("impl m::g");
    assert!(
        function.starts_with("error[E0404]: expected trait, found function `m::g`\n"),
        "{function}"
    );
    let unlinked = shown("impl nomod::T");
    assert!(
        unlinked.starts_with("error[E0433]: cannot find module or crate `nomod` in this scope\n"),
        "{unlinked}"
    );
    let std = shown("impl std::fmt::Nope");
    assert!(
        std.starts_with("error[E0405]: cannot find trait `Nope` in module `std::fmt`\n"),
        "{std}"
    );
}

/// A method or associated item the program's own struct or enum lacks:
/// rustc's E0599 names the type's kind and marks its definition (#2337).
#[test]
fn a_program_types_missing_item_names_its_kind_and_definition() {
    assert_eq!(
        rendered("struct S;\nfn main() {\n    S.nope();\n}\n"),
        concat!(
            "error[E0599]: no method named `nope` found for struct `S` in the current scope\n",
            " --> main.rs:3:7\n",
            "  |\n",
            "1 | struct S;\n",
            "  | -------- method `nope` not found for this struct\n",
            "2 | fn main() {\n",
            "3 |     S.nope();\n",
            "  |       ^^^^ method not found in `S`\n",
        )
    );
    let shown = rendered("enum E { A }\nfn main() {\n    let e = E::A;\n    e.nope();\n}\n");
    assert!(
        shown.starts_with(
            "error[E0599]: no method named `nope` found for enum `E` in the current \
                           scope\n"
        ) && shown.contains("1 | enum E { A }\n  | ------ method `nope` not found for this enum\n")
            && shown.contains("  |       ^^^^ method not found in `E`\n"),
        "{shown}"
    );
    // A generic type by its parameters, found by its arguments.
    let shown = rendered(
        "struct G<T: Copy, U>(T, U);\nfn main() {\n    let g: G<i32, bool> = G(1, true);\n    \
         g.nope();\n}\n",
    );
    assert!(
        shown.starts_with("error[E0599]: no method named `nope` found for struct `G<T, U>`")
            && shown.contains("  | -------------------- method `nope` not found for this struct\n")
            && shown.contains("  |       ^^^^ method not found in `G<i32, bool>`\n"),
        "{shown}"
    );
    // A reference, which marks no definition.
    let shown = rendered(
        "struct S;\nfn main() {\n    let mut s = S;\n    let r = &mut s;\n    r.nope();\n}\n",
    );
    assert!(
        shown.starts_with(
            "error[E0599]: no method named `nope` found for mutable reference `&mut S` in the \
             current scope\n"
        ) && !shown.contains("not found for this"),
        "{shown}"
    );
    // An associated item, a struct's or an enum's.
    let shown = rendered("struct S;\nfn main() {\n    S::nope();\n}\n");
    assert!(
        shown.starts_with(
            "error[E0599]: no associated function or constant named `nope` found for struct `S` \
             in the current scope\n"
        ) && shown.contains(
            "  | -------- associated function or constant `nope` not found for this struct\n"
        ) && shown.contains("  |        ^^^^ associated function or constant not found in `S`\n"),
        "{shown}"
    );
    let shown = rendered("enum E { A }\nfn main() {\n    let _ = E::Nope;\n}\n");
    assert!(
        shown.starts_with(
            "error[E0599]: no variant, associated function, or constant named `Nope` found for \
             enum `E` in the current scope\n"
        ),
        "{shown}"
    );
    // A field called as a method marks the struct too.
    let shown = rendered(
        "struct S {\n    f: i32,\n}\nfn main() {\n    let s = S { f: 1 };\n    s.f();\n}\n",
    );
    assert!(
        shown.starts_with(
            "error[E0599]: no method named `f` found for struct `S` in the current \
                           scope\n"
        ) && shown.contains("1 | struct S {\n  | -------- method `f` not found for this struct\n"),
        "{shown}"
    );
}

/// The `help:` rustc gives a name no `use` brings into scope, from `help:`.
fn import_help(source: &str) -> String {
    let shown = rendered(source);
    let help = shown.find("help: ").unwrap_or_else(|| panic!("{shown}"));
    shown[help..].to_string()
}

/// Several items a `use` could import are rustc's alternatives, each its
/// own `use` (#2337).
#[test]
fn an_import_with_several_candidates_offers_each() {
    assert_eq!(
        import_help(
            "mod a {\n    pub fn f() {}\n}\nmod b {\n    pub fn f() {}\n}\nfn main() {\n    f();\n}\n"
        ),
        concat!(
            "help: consider importing one of these functions\n",
            "  |\n",
            "1 + use crate::a::f;\n",
            "  |\n",
            "1 + use crate::b::f;\n",
            "  |\n",
        )
    );
    // The program's and std's, by path; of several kinds, items.
    assert_eq!(
        import_help("mod m {\n    pub fn max() {}\n}\nfn main() {\n    max();\n}\n"),
        concat!(
            "help: consider importing one of these functions\n",
            "  |\n",
            "1 + use crate::m::max;\n",
            "  |\n",
            "1 + use std::cmp::max;\n",
            "  |\n",
        )
    );
    assert!(import_help("fn f(_x: Error) {}\nfn main() {}\n")
        .starts_with("help: consider importing one of these items\n"),);
    // Past four, rustc counts the others.
    let iter = import_help("fn f(_x: Iter) {}\nfn main() {}\n");
    assert!(
        iter.starts_with("help: consider importing one of these structs\n")
            && iter.matches(" + use ").count() == 4
            && iter.contains("  |\n  = and ")
            && iter.ends_with(" other candidates\n"),
        "{iter}"
    );
}

/// A unit struct used as a value, a type and a std type such as `HashMap`
/// suggest their imports (#2337).
#[test]
fn a_value_or_type_another_module_or_std_declares_suggests_its_import() {
    let help = |noun: &str, line: &str| {
        format!("help: consider importing this {noun}\n  |\n{line}\n  |\n")
    };
    assert_eq!(
        import_help("mod m {\n    pub struct U;\n}\nfn main() {\n    let _u = U;\n}\n"),
        help("unit struct", "1 + use crate::m::U;")
    );
    assert_eq!(
        import_help("mod m {\n    pub struct T;\n}\nfn f(_t: T) {}\nfn main() {}\n"),
        help("struct", "1 + use crate::m::T;")
    );
    assert_eq!(
        import_help("mod m {\n    pub enum E { A }\n}\nfn main() {\n    let _ = E::A;\n}\n"),
        help("enum", "1 + use crate::m::E;")
    );
    assert_eq!(
        rendered("fn main() {\n    let _m = HashMap::<i32, i32>::new();\n}\n"),
        concat!(
            "error[E0433]: cannot find type `HashMap` in this scope\n",
            " --> main.rs:2:14\n",
            "  |\n",
            "2 |     let _m = HashMap::<i32, i32>::new();\n",
            "  |              ^^^^^^^ use of undeclared type `HashMap`\n",
            "  |\n",
            "help: consider importing this struct\n",
            "  |\n",
            "1 + use std::collections::HashMap;\n",
            "  |\n",
        )
    );
    let typed = rendered("fn main() {\n    let _m: HashMap<i32, i32> = HashMap::new();\n}\n");
    assert!(
        typed.starts_with("error[E0425]: cannot find type `HashMap` in this scope\n")
            && typed.contains("  |             ^^^^^^^ not found in this scope\n")
            && typed.ends_with(&help("struct", "1 + use std::collections::HashMap;")),
        "{typed}"
    );
    assert_eq!(
        import_help("fn f(_x: Ordering) {}\nfn main() {}\n"),
        concat!(
            "help: consider importing one of these enums\n",
            "  |\n",
            "1 + use std::cmp::Ordering;\n",
            "  |\n",
            "1 + use std::sync::atomic::Ordering;\n",
            "  |\n",
        )
    );
    // A call of a capitalized name reads as a tuple struct's.
    let shown =
        rendered("mod a {\n    pub struct T(pub i32);\n}\nfn main() {\n    let _t = T(1);\n}\n");
    assert!(
        shown.starts_with(
            "error[E0425]: cannot find function, tuple struct or tuple variant `T` in this scope\n"
        ) && shown.ends_with(&help("tuple struct", "1 + use crate::a::T;")),
        "{shown}"
    );
}

/// A format argument whose type lacks the trait its placeholder formats
/// with: rustc labels the placeholder and the argument (#2337).
#[test]
fn a_format_argument_lacking_a_trait_labels_its_placeholder() {
    let shown = rendered("struct S;\nfn main() {\n    println!(\"{:?}\", S);\n}\n");
    assert!(
        shown.starts_with(concat!(
            "error[E0277]: `S` doesn't implement `Debug`\n",
            " --> main.rs:3:22\n",
            "  |\n",
            "3 |     println!(\"{:?}\", S);\n",
            "  |               ----   ^ `S` cannot be formatted using `{:?}` because it doesn't \
             implement `Debug`\n",
            "  |               |\n",
            "  |               required by this formatting parameter\n",
            "  |\n",
            "  = help: the trait `Debug` is not implemented for `S`\n",
            "help: consider annotating `S` with `#[derive(Debug)]`\n",
        )),
        "{shown}"
    );
    // A placeholder that names its value is marked alone.
    let inline = rendered("struct S;\nfn main() {\n    let s = S;\n    println!(\"{s:?}\");\n}\n");
    assert!(
        inline.contains(
            "4 |     println!(\"{s:?}\");\n  |               ^^^^^ `S` cannot be formatted using \
             `{:?}` because it doesn't implement `Debug`\n  |\n"
        ),
        "{inline}"
    );
    let pretty =
        rendered("struct S;\nfn main() {\n    let _x = format!(\"{} and {:#?}\", 1, S);\n}\n");
    assert!(
        pretty.contains(
            "  |                              -----      ^ `S` cannot be formatted using"
        ),
        "{pretty}"
    );
    // A type declared elsewhere has its help and note after the snippet.
    assert!(
        rendered("fn main() {\n    let v = vec![1];\n    println!(\"{}\", v);\n}\n").ends_with(
            concat!(
                "  = help: the trait `std::fmt::Display` is not implemented for `Vec<{integer}>`\n",
                "  = note: in format strings you may be able to use `{:?}` (or {:#?} for \
                 pretty-print) instead\n",
            )
        ),
    );
}

/// A `match` that misses variants of the program's enum: rustc notes the
/// enum with each variant no arm covers (#2337).
#[test]
fn a_non_exhaustive_match_notes_the_enum_at_its_definition() {
    assert_eq!(
        rendered(
            "enum E { A, B, C }\nfn main() {\n    let e = E::A;\n    match e {\n        E::A => \
             {}\n    }\n}\n"
        ),
        concat!(
            "error[E0004]: non-exhaustive patterns: `E::B` and `E::C` not covered\n",
            " --> main.rs:4:11\n",
            "  |\n",
            "4 |     match e {\n",
            "  |           ^ patterns `E::B` and `E::C` not covered\n",
            "  |\n",
            "note: `E` defined here\n",
            " --> main.rs:1:6\n",
            "  |\n",
            "1 | enum E { A, B, C }\n",
            "  |      ^      -  - not covered\n",
            "  |             |\n",
            "  |             not covered\n",
            "  = note: the matched value is of type `E`\n",
            "help: ensure that all possible cases are being handled by adding a match arm with a \
             wildcard pattern, a match arm with multiple or-patterns as shown, or multiple match \
             arms\n",
            "  |\n",
            "5 ~         E::A => {},\n",
            "6 +         E::B | E::C => todo!()\n",
            "  |\n",
        )
    );
    // Each variant no arm covers, past the three the message names.
    let many = rendered(
        "enum E { A, B(i32), C { x: i32 }, D, F }\nfn main() {\n    let e = E::A;\n    match e \
         {\n        E::A => {}\n    }\n}\n",
    );
    assert!(many.matches("not covered\n").count() == 5, "{many}");
    // std's enums are noted in std's source, which rawr doesn't show.
    let option = rendered(
        "fn main() {\n    let o: Option<i32> = None;\n    match o {\n        Some(_) => {}\n    \
         }\n}\n",
    );
    assert!(!option.contains("defined here"), "{option}");
}

/// A std thread type's bare name resolves through a `use` alone, as in
/// rustc: unbound, it is E0425 with the imports rustc suggests, and a path
/// through a module's `use` names it (#2346).
#[test]
fn a_bare_std_thread_type_nothing_binds_is_e0425() {
    let help = |line: &str| format!("help: consider importing this struct\n  |\n{line}\n  |\n");
    assert_eq!(
        rendered("fn f(_x: JoinHandle<()>) {}\nfn main() {}\n"),
        concat!(
            "error[E0425]: cannot find type `JoinHandle` in this scope\n",
            " --> main.rs:1:10\n",
            "  |\n",
            "1 | fn f(_x: JoinHandle<()>) {}\n",
            "  |          ^^^^^^^^^^ not found in this scope\n",
            "  |\n",
            "help: consider importing this struct\n",
            "  |\n",
            "1 + use std::thread::JoinHandle;\n",
            "  |\n",
        )
    );
    assert_eq!(
        import_help("fn f(_x: Mutex) {}\nfn main() {}\n"),
        concat!(
            "help: consider importing one of these structs\n",
            "  |\n",
            "1 + use std::sync::Mutex;\n",
            "  |\n",
            "1 + use std::sync::nonpoison::Mutex;\n",
            "  |\n",
        )
    );
    assert_eq!(
        import_help("fn f(_x: Sender<i32>) {}\nfn main() {}\n"),
        concat!(
            "help: consider importing one of these structs\n",
            "  |\n",
            "1 + use std::sync::mpmc::Sender;\n",
            "  |\n",
            "1 + use std::sync::mpsc::Sender;\n",
            "  |\n",
            "1 + use std::sync::oneshot::Sender;\n",
            "  |\n",
        )
    );
    assert_eq!(
        import_help("fn f(_x: SyncSender<i32>) {}\nfn main() {}\n"),
        help("1 + use std::sync::mpsc::SyncSender;")
    );
    for source in [
        "struct S {\n    _m: Mutex<i32>,\n}\nfn main() {}\n",
        "fn main() {\n    let _v: Vec<JoinHandle<()>> = Vec::new();\n}\n",
        "fn main() {\n    let (tx, _rx) = std::sync::mpsc::channel::<i32>();\n    let _s: Sender<i32> = tx;\n}\n",
    ] {
        let (code, message, _) = refused(source);
        assert!(code == "E0425" && message.starts_with("cannot find type"), "{message}");
    }
    for source in [
        "use std::sync::mpsc;\nfn f(_x: mpsc::Sender<i32>, _y: mpsc::Receiver<i32>) {}\nfn main() {}\n",
        "use std::thread;\nfn main() {\n    let h: thread::JoinHandle<()> = thread::spawn(|| {});\n    h.join().unwrap();\n}\n",
        "use std::sync::mpsc::{Receiver, Sender};\nfn f(_x: Sender<i32>, _y: Receiver<i32>) {}\nfn main() {}\n",
        "use std::sync;\nfn main() {\n    let _m = sync::Mutex::new(1);\n}\n",
        "struct Mutex(i32);\nfn f(_x: Mutex) {}\nfn main() {\n    f(Mutex(1));\n}\n",
    ] {
        Engine::new()
            .compile(source)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
    }
}

/// Every std type Rawr represents resolves only through rustc's scopes,
/// as the thread types do: a bare name or a path through a module nothing
/// binds is rustc's error, with the import a bare name could have, and a
/// `use` of the type or of its module names it (#2346).
#[test]
fn a_std_type_resolves_only_through_a_use_or_its_std_path() {
    for (source, code, message) in [
        ("fn f(_x: Rc<i32>) {}\nfn main() {}\n", "E0425", "cannot find type `Rc` in this scope"),
        ("fn main() {\n    let _m: HashMap<i32, i32> = std::collections::HashMap::new();\n}\n", "E0425", "cannot find type `HashMap` in this scope"),
        ("fn main() {\n    let _c: Cell<i32> = std::cell::Cell::new(1);\n}\n", "E0425", "cannot find type `Cell` in this scope"),
        ("fn main() {\n    let _o: Ordering = 1.cmp(&2);\n}\n", "E0425", "cannot find type `Ordering` in this scope"),
        ("fn main() {\n    let _r: Vec<Reverse<i32>> = Vec::new();\n}\n", "E0425", "cannot find type `Reverse` in this scope"),
        ("fn main() {\n    let _p: PhantomData<i32> = std::marker::PhantomData;\n}\n", "E0425", "cannot find type `PhantomData` in this scope"),
        ("fn f() -> Result<i32, ParseIntError> {\n    \"1\".parse::<i32>()\n}\nfn main() {}\n", "E0425", "cannot find type `ParseIntError` in this scope"),
        ("fn main() {\n    let _r: RangeInclusive<i32> = 1..=2;\n}\n", "E0425", "cannot find type `RangeInclusive` in this scope"),
        ("fn main() {\n    let _c: Cow<str> = std::borrow::Cow::Borrowed(\"a\");\n}\n", "E0425", "cannot find type `Cow` in this scope"),
        ("fn main() {\n    let _r = RefCell::new(1);\n}\n", "E0433", "cannot find type `RefCell` in this scope"),
        ("fn main() {\n    let _d = VecDeque::<i32>::new();\n}\n", "E0433", "cannot find type `VecDeque` in this scope"),
        ("fn main() {\n    let _d = Duration::from_secs(1);\n}\n", "E0433", "cannot find type `Duration` in this scope"),
        ("fn main() {\n    let _i = Instant::now();\n}\n", "E0433", "cannot find type `Instant` in this scope"),
        ("fn main() {\n    let _r = Rc::new(1);\n}\n", "E0433", "cannot find type `Rc` in this scope"),
        ("fn main() {\n    match 1.cmp(&2) {\n        Ordering::Less => {}\n        _ => {}\n    }\n}\n", "E0433", "cannot find type `Ordering` in this scope"),
        ("fn main() {\n    let _r = Reverse(1);\n}\n", "E0425", "cannot find function, tuple struct or tuple variant `Reverse` in this scope"),
        ("fn f(_x: cmp::Ordering) {}\nfn main() {}\n", "E0433", "cannot find module or crate `cmp` in this scope"),
        ("fn main() {\n    let _m = collections::HashMap::<i32, i32>::new();\n}\n", "E0433", "cannot find module or crate `collections` in this scope"),
        ("fn main() {\n    let _w: rc::Weak<i32> = std::rc::Weak::new();\n}\n", "E0433", "cannot find module or crate `rc` in this scope"),
        ("fn main() {\n    let _t = time::Duration::from_secs(1);\n}\n", "E0433", "cannot find module or crate `time` in this scope"),
    ] {
        let (found, said, _) = refused(source);
        assert_eq!((found.as_str(), said.as_str()), (code, message), "{source}");
    }
    assert!(
        rendered("fn main() {\n    let _m = BTreeMap::<i32, i32>::new();\n}\n").ends_with(
            "help: consider importing this struct\n  |\n1 + use std::collections::BTreeMap;\n  |\n"
        )
    );
    for source in [
        "use std::collections;\nfn f(m: &collections::BTreeMap<i32, i32>) -> usize {\n    m.len()\n}\nfn main() {\n    let mut m = collections::BTreeMap::new();\n    m.insert(1, 2);\n    let s: collections::HashSet<i32> = collections::HashSet::new();\n    println!(\"{} {}\", f(&m), s.len());\n}\n",
        "use std::cmp;\nfn main() {\n    match 1.cmp(&2) {\n        cmp::Ordering::Less => {}\n        _ => {}\n    }\n    let _r = cmp::Reverse(3);\n}\n",
        "use std::cmp::Ordering;\nuse Ordering::*;\nfn main() {\n    match 1.cmp(&2) {\n        Less => {}\n        _ => {}\n    }\n}\n",
        "use std::rc;\nfn main() {\n    let r: rc::Rc<i32> = rc::Rc::new(1);\n    let _w: rc::Weak<i32> = rc::Rc::downgrade(&r);\n    let _a = std::sync::Arc::new(2);\n}\n",
        "use std::cell;\nfn main() {\n    let _r: cell::RefCell<i32> = cell::RefCell::new(1);\n    let _c = cell::Cell::new(2);\n}\n",
        "use std::time;\nfn main() {\n    let _d: time::Duration = time::Duration::from_secs(2);\n}\n",
        "use std::collections::hash_map::HashMap;\nfn main() {\n    let mut m: HashMap<i32, i32> = HashMap::new();\n    m.insert(1, 2);\n}\n",
        "use std::marker::PhantomData;\nstruct S<T> {\n    _p: PhantomData<T>,\n}\nfn main() {\n    let _s: S<i32> = S { _p: PhantomData };\n}\n",
        "use std::num::ParseIntError;\nfn f(t: &str) -> Result<i32, ParseIntError> {\n    t.parse::<i32>()\n}\nfn main() {\n    let _ = f(\"1\");\n}\n",
        "use std::ops::RangeInclusive;\nfn main() {\n    let _r: RangeInclusive<i32> = 1..=3;\n    let _s: std::ops::Range<i32> = 0..2;\n}\n",
        "use std::borrow::Cow;\nfn main() {\n    let _c: Cow<str> = Cow::Borrowed(\"ab\");\n}\n",
        "struct HashMap {\n    n: i32,\n}\nfn main() {\n    let m = HashMap { n: 1 };\n    let _ = m.n;\n}\n",
        "struct Ordering;\nfn main() {\n    let _o = Ordering;\n}\n",
    ] {
        Engine::new()
            .compile(source)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
    }
}

/// A call of a turbofish name nothing binds is a function rustc can't
/// find, marked over the name (#2346).
#[test]
fn a_turbofish_call_of_an_unbound_name_is_e0425_for_a_function() {
    assert_eq!(
        rendered("fn main() {\n    let _x = channel::<i32>();\n}\n"),
        concat!(
            "error[E0425]: cannot find function `channel` in this scope\n",
            " --> main.rs:2:14\n",
            "  |\n",
            "2 |     let _x = channel::<i32>();\n",
            "  |              ^^^^^^^ not found in this scope\n",
            "  |\n",
            "help: consider importing one of these functions\n",
            "  |\n",
            "1 + use std::sync::mpmc::channel;\n",
            "  |\n",
            "1 + use std::sync::mpsc::channel;\n",
            "  |\n",
            "1 + use std::sync::oneshot::channel;\n",
            "  |\n",
        )
    );
    let (_, message, label) = refused("fn main() {\n    let _e = nope::<i32>;\n}\n");
    assert_eq!(
        (message.as_str(), label.as_deref()),
        (
            "cannot find value `nope` in this scope",
            Some("not found in this scope")
        )
    );
}

/// A struct's or struct variant's witness names its fields that aren't
/// `_`, and `..` for the rest, as rustc writes it; a struct is noted at
/// its definition (#2347).
#[test]
fn a_struct_witness_names_its_fields() {
    let program = |body: &str| {
        format!(
            "enum E {{ A, B(i32), C {{ x: i32, y: bool }}, D {{}} }}\nstruct S {{ a: bool, b: \
             i32 }}\nfn main() {{\n    let e = E::A;\n    let s = S {{ a: true, b: 1 }};\n    \
             let o = Some(E::A);\n    {body}\n    let _ = (s.b, o);\n}}\n"
        )
    };
    let message = |body: &str| refused(&program(body)).1;
    assert_eq!(
        message("match e { E::A => {}, E::B(_) => {}, E::D {} => {} }"),
        "non-exhaustive patterns: `E::C { .. }` not covered"
    );
    assert_eq!(
        message("match e { E::A => {}, E::B(_) => {}, E::D {} => {}, E::C { y: true, .. } => {} }"),
        "non-exhaustive patterns: `E::C { y: false, .. }` not covered"
    );
    // An empty struct variant is `{  }`, as rustc prints it.
    assert_eq!(
        message(
            "match o { None => {}, Some(E::A) => {}, Some(E::B(_)) => {}, Some(E::C { .. }) => {} }"
        ),
        "non-exhaustive patterns: `Some(E::D {  })` not covered"
    );
    let shown = rendered(&program("match s { S { a: true, .. } => {} }"));
    assert!(
        shown.starts_with(
            "error[E0004]: non-exhaustive patterns: `S { a: false, .. }` not covered\n"
        ) && shown.contains(concat!(
            "note: `S` defined here\n",
            " --> main.rs:2:8\n",
            "  |\n",
            "2 | struct S { a: bool, b: i32 }\n",
            "  |        ^\n",
            "  = note: the matched value is of type `S`\n",
        )) && shown.contains("S { a: true, .. } => {}, S { a: false, .. } => todo!() }\n"),
        "{shown}"
    );
    let refutable = rendered(&program("let E::A = e;"));
    assert!(
        refutable.contains("patterns `E::B(_)`, `E::C { .. }` and `E::D {  }` not covered\n"),
        "{refutable}"
    );
}

/// Every member of one `use` that names nothing is one E0432, with a
/// label at each (#2347).
#[test]
fn every_unresolved_member_of_one_use_is_one_e0432() {
    let module = "mod model {\n    pub mod inner {}\n    pub struct Thing;\n}\n";
    let shown = |import: &str| rendered(&format!("{module}{import}\nfn main() {{}}\n"));
    assert_eq!(
        shown("use crate::model::{inner::{gone, other}};"),
        concat!(
            "error[E0432]: unresolved imports `crate::model::inner::gone`, \
             `crate::model::inner::other`\n",
            " --> main.rs:5:28\n",
            "  |\n",
            "5 | use crate::model::{inner::{gone, other}};\n",
            "  |                            ^^^^  ^^^^^ no `other` in `model::inner`\n",
            "  |                            |\n",
            "  |                            no `gone` in `model::inner`\n",
        )
    );
    assert_eq!(
        shown("use crate::model::{inner::gone2, inner::other2};"),
        concat!(
            "error[E0432]: unresolved imports `crate::model::inner::gone2`, \
             `crate::model::inner::other2`\n",
            " --> main.rs:5:20\n",
            "  |\n",
            "5 | use crate::model::{inner::gone2, inner::other2};\n",
            "  |                    ^^^^^^^-----  ^^^^^^^------\n",
            "  |                           |             |\n",
            "  |                           |             no `other2` in `model::inner`\n",
            "  |                           no `gone2` in `model::inner`\n",
        )
    );
    // A module missing on the way beside an item, and a renamed member.
    let mixed = shown("use crate::model::{nomod::x, absent3};");
    assert!(
        mixed.starts_with(
            "error[E0432]: unresolved imports `crate::model::nomod`, `crate::model::absent3`\n"
        ) && mixed.contains("  |                    ^^^^^     ^^^^^^^ no `absent3` in `model`\n")
            && mixed.contains("  |                    could not find `nomod` in `model`\n"),
        "{mixed}"
    );
    let renamed = shown("use crate::model::{absent4 as a4, absent5};");
    assert!(
        renamed.contains("  |                    -------^^^^^^  ^^^^^^^ no `absent5` in `model`\n"),
        "{renamed}"
    );
    // std's, in a file without modules too.
    let std = rendered("use std::collections::{HashMap, Nope, Nada};\nfn main() {}\n");
    assert!(
        std.starts_with(
            "error[E0432]: unresolved imports `std::collections::Nope`, \
             `std::collections::Nada`\n --> main.rs:1:33\n"
        ) && std.contains(
            "  |                                 ^^^^  ^^^^ no `Nada` in `collections`\n"
        ),
        "{std}"
    );
    // One member alone keeps the singular.
    assert!(shown("use crate::model::{inner::gone6, Thing as T6};")
        .starts_with("error[E0432]: unresolved import `crate::model::inner::gone6`\n"));
}

/// rustc's E0599 suggests the type's item with a similar name, in place
/// where the call's arguments fit it, else at its definition; a method's
/// label goes where it does (#2347).
#[test]
fn a_missing_method_suggests_one_with_a_similar_name() {
    let program = |body: &str| {
        rendered(&format!(
            "struct S;\nimpl S {{\n    fn go(&self) {{}}\n    fn make(n: i32) -> i32 {{ n }}\n    \
             const LIMIT: i32 = 1;\n    fn two(&self, a: i32, b: i32) -> i32 {{ a + b }}\n}}\nfn \
             main() {{\n    let s = S;\n    {body}\n}}\n"
        ))
    };
    assert_eq!(
        program("s.goo();"),
        concat!(
            "error[E0599]: no method named `goo` found for struct `S` in the current scope\n",
            "  --> main.rs:10:7\n",
            "   |\n",
            " 1 | struct S;\n",
            "   | -------- method `goo` not found for this struct\n",
            "...\n",
            "10 |     s.goo();\n",
            "   |       ^^^\n",
            "   |\n",
            "help: there is a method `go` with a similar name\n",
            "   |\n",
            "10 -     s.goo();\n",
            "10 +     s.go();\n",
            "   |\n",
        )
    );
    assert!(
        program("s.g();").ends_with("10 |     s.go();\n   |        +\n"),
        "{}",
        program("s.g();")
    );
    // An associated function keeps its label.
    let associated = program("let _ = S::mak(1);");
    assert!(
        associated
            .contains("   |                ^^^ associated function or constant not found in `S`\n")
            && associated.ends_with(concat!(
                "help: there is an associated function `make` with a similar name\n",
                "   |\n",
                "10 |     let _ = S::make(1);\n",
                "   |                   +\n",
            )),
        "{associated}"
    );
    assert!(program("let _ = S::LIMT;")
        .contains("help: there is an associated constant `LIMIT` with a similar name\n"));
    // Arguments that don't fit: the item at its definition.
    let different = program("S::goo(&s);");
    assert!(
        different.ends_with(concat!(
            "help: there is a method `go` with a similar name, but with different arguments\n",
            "  --> main.rs:3:5\n",
            "   |\n",
            " 3 |     fn go(&self) {}\n",
            "   |     ^^^^^^^^^^^^\n",
        )),
        "{different}"
    );
    let missing = program("let _ = S::mak();");
    assert!(
        missing.contains("help: there is an associated function `make` with a similar name\n")
            && missing.contains(" 4 |     fn make(n: i32) -> i32 { n }\n"),
        "{missing}"
    );
    // A trait's method, and a name with none like it.
    let traits = rendered(
        "trait T {\n    fn walk(&self);\n}\nstruct S;\nimpl T for S {\n    fn walk(&self) {}\n}\nfn \
         main() {\n    S.wlk();\n}\n",
    );
    assert!(
        traits.contains("help: there is a method `walk` with a similar name\n"),
        "{traits}"
    );
    assert!(program("s.zzz();").contains("^^^ method not found in `S`\n"));
}

/// rustc's E0599 names a type's lifetime parameters, and elides them in
/// the type it found (#2347).
#[test]
fn a_missing_method_names_the_types_lifetimes() {
    let shown = rendered(
        "struct R<'a, T> {\n    v: &'a T,\n}\nfn main() {\n    let x = 1;\n    let r = R { v: &x \
         };\n    r.nope();\n    let _ = r.v;\n}\n",
    );
    assert!(
        shown.starts_with(
            "error[E0599]: no method named `nope` found for struct `R<'a, T>` in the current \
             scope\n"
        ) && shown.contains("  |       ^^^^ method not found in `R<'_, {integer}>`\n"),
        "{shown}"
    );
    let program = |body: &str| {
        rendered(&format!(
            "struct R<'a> {{\n    v: &'a i32,\n}}\nfn main() {{\n    let x = 1;\n    let r = R \
             {{ v: &x }};\n    let _ = r.v;\n    {body}\n}}\n"
        ))
    };
    assert!(program("r.nope();").contains("method not found in `R<'_>`\n"));
    assert!(program("(&r).nope();").starts_with(
        "error[E0599]: no method named `nope` found for reference `&R<'_>` in the current scope\n"
    ));
    assert!(program("R::nope();").starts_with(
        "error[E0599]: no associated function or constant named `nope` found for struct `R<'a>`"
    ));
}

/// std's and core's items a `use` could import, as rustc lists them:
/// core's only where they aren't as many as std's, after std's; a trait
/// a bound or an `impl` names; a unit or tuple struct a value names
/// (#2347, #2349).
#[test]
fn std_and_core_import_candidates_match_rustcs() {
    let display = "help: consider importing this trait\n  |\n1 + use std::fmt::Display;\n  |\n";
    for source in [
        "struct S;\nimpl Display for S {}\nfn main() {}\n",
        "fn f<T: Display>(_x: T) {}\nfn main() {}\n",
        "fn f(_x: impl Display) {}\nfn main() {}\n",
    ] {
        let shown = rendered(source);
        assert!(
            shown.starts_with("error[E0405]: cannot find trait `Display` in this scope\n")
                && shown.ends_with(display),
            "{shown}"
        );
    }
    let iter = import_help("fn f(_x: Iter) {}\nfn main() {}\n");
    assert!(
        iter.ends_with(
            "1 + use std::collections::hash_map::Iter;\n  |\n  = and 12 other candidates\n"
        ),
        "{iter}"
    );
    let help = |noun: &str, line: &str| {
        format!("help: consider importing this {noun}\n  |\n{line}\n  |\n")
    };
    assert_eq!(
        import_help("fn main() {\n    let _r = Reverse(1);\n}\n"),
        help("tuple struct", "1 + use std::cmp::Reverse;")
    );
    assert_eq!(
        import_help("fn main() {\n    let _f = Reverse;\n}\n"),
        help("tuple struct", "1 + use std::cmp::Reverse;")
    );
    assert_eq!(
        import_help("fn main() {\n    let _p: std::marker::PhantomData<i32> = PhantomData;\n}\n"),
        help("unit struct", "1 + use std::marker::PhantomData;")
    );
    assert_eq!(
        import_help("fn main() {\n    let _w = Wrapping(1);\n}\n"),
        help("tuple struct", "1 + use std::num::Wrapping;")
    );
}
