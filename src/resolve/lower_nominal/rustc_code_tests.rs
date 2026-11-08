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
        let notes = report.notes().map(str::to_string).collect::<Vec<_>>();
        let help = report.help().unwrap_or_default().to_string();
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
    assert_eq!(help, format!("{explicit}: `Message::Decrement => todo!()`"));

    // Two variants, as one or-pattern; past three, `and N more` and `_`.
    let letters = "enum M { A, B, C, D, E }";
    let (text, label, _, help) = missing(
        letters,
        "M::A",
        "M::A => {}\n        M::B => {}\n        M::C => {}",
    );
    assert_eq!(text, message("`M::D` and `M::E`"));
    assert_eq!(label, "patterns `M::D` and `M::E` not covered");
    assert_eq!(help, format!("{or_patterns}: `M::D | M::E => todo!()`"));
    let (text, label, _, help) = missing(letters, "M::A", "M::A => {}");
    assert_eq!(text, message("`M::B`, `M::C`, `M::D` and 1 more"));
    assert_eq!(
        label,
        "patterns `M::B`, `M::C`, `M::D` and 1 more not covered"
    );
    assert_eq!(help, format!("{wildcard}: `_ => todo!()`"));
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
    assert_eq!(help, format!("{explicit}: `None => todo!()`"));
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
        format!("{or_patterns}: `i32::MIN..=0_i32 | 2_i32..=i32::MAX => todo!()`")
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
        let help = report.help().map(str::to_string);
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
        Some(
            "you might want to use `let else` to handle the variant that isn't matched: \
             `let Some(x) = o else { todo!() };`"
        )
    );
    // A pattern that binds nothing is `if let`'s.
    let (_, label, _, help) = refutable("let None = o;");
    assert_eq!(label, "pattern `Some(_)` not covered");
    assert_eq!(
        help.as_deref(),
        Some(
            "you might want to use `if let` to ignore the variant that isn't matched: \
             `if let None = o { todo!() }`"
        )
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
        Some(
            "you might want to use `if let` to ignore the variants that aren't matched: \
             `if let 1 = 2 { todo!() }`"
        )
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
