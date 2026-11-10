//! The program's own `macro_rules!` macros (#236): each program prints
//! what its rustc build prints, and each one rustc refuses is refused with
//! rustc's error.

use std::sync::{Arc, Mutex};

use crate::diagnostic::{Report, Source};
use crate::Engine;

/// What `source`'s `main` prints.
fn printed(source: &str) -> String {
    let written = Arc::new(Mutex::new(String::new()));
    let mut engine = Engine::new();
    let log = written.clone();
    engine.on_output(move |_, text| log.lock().unwrap().push_str(text));
    engine
        .eval::<()>(&format!("{source}\nmain();\n"))
        .unwrap_or_else(|error| panic!("{error}"));
    written.lock().unwrap().clone()
}

/// rustc's message and primary label for `source`, which rawr refuses.
fn refused(source: &str) -> (String, Option<String>) {
    let source = format!("{source}\nmain();\n");
    let error = Engine::new()
        .compile(&source)
        .map(|_| ())
        .expect_err("rawr refuses this program");
    let source = Source::new("main.rs", source).expect("a test source has an identity");
    let report = Report::from_parse(source, &error).expect("the error is in the source");
    assert_eq!(report.code().as_str(), "", "rustc gives these no code");
    (
        report.message().to_string(),
        report
            .labels()
            .first()
            .and_then(|label| label.message().map(str::to_string)),
    )
}

#[test]
fn rustlings_macros_print_as_rustc_builds_them() {
    // macros1 and macros4's solutions.
    assert_eq!(
        printed(
            "macro_rules! my_macro {\n    () => {\n        println!(\"Check out my macro!\");\n    };\n    \
             ($val:expr) => {\n        println!(\"Look at this other macro: {}\", $val);\n    };\n}\n\
             fn main() {\n    my_macro!();\n    my_macro!(7777);\n}\n"
        ),
        "Check out my macro!\nLook at this other macro: 7777\n"
    );
}

#[test]
fn repetitions_with_separators_expand_as_in_rust() {
    let source = r#"
macro_rules! sum {
    () => { 0 };
    ($x:expr $(, $rest:expr)* $(,)?) => { $x + sum!($($rest),*) };
}
macro_rules! make_vec {
    ($($x:expr),+ $(,)?) => {{
        let mut v = Vec::new();
        $(v.push($x);)+
        v
    }};
}
macro_rules! pairs {
    ($($k:ident => $v:expr);* $(;)?) => {
        $(println!("{} = {}", stringify!($k), $v);)*
    };
}
macro_rules! maybe {
    ($x:expr $(, $y:expr)?) => { $x $(* $y)? };
}
fn main() {
    println!("{}", sum!(1, 2, 3, 4));
    println!("{}", sum!());
    let v = make_vec![1, 2, 3,];
    println!("{:?}", v);
    pairs!(a => 1; b => 2 + 3;);
    println!("{} {}", maybe!(3), maybe!(3, 4));
}
"#;
    assert_eq!(printed(source), "10\n0\n[1, 2, 3]\na = 1\nb = 5\n3 12\n");
}

#[test]
fn a_macros_locals_are_its_own_and_a_callers_names_are_the_callers() {
    let source = r#"
macro_rules! double {
    ($e:expr) => {{
        let x = $e;
        x * 2
    }};
}
// A `let` of a name the caller passes binds it after the call.
macro_rules! define {
    ($name:ident) => {
        let $name = 5;
    };
}
macro_rules! swap_named {
    ($a:ident, $b:ident) => {
        let t = $a;
        $a = $b;
        $b = t;
    };
}
fn main() {
    let x = 10;
    println!("{}", double!(x + 1));
    println!("{}", x);
    let mut a = 1;
    let mut b = 2;
    swap_named!(a, b);
    println!("{} {}", a, b);
    define!(five);
    println!("{}", five);
}
"#;
    assert_eq!(printed(source), "22\n10\n2 1\n5\n");
}

#[test]
fn fragment_specifiers_match_what_rusts_parser_reads() {
    let source = r#"
macro_rules! show_ty {
    ($t:ty) => {{ let v: $t = Default::default(); v }};
}
macro_rules! run_block {
    ($b:block) => { $b };
}
macro_rules! test_pat {
    ($v:expr, $p:pat) => { matches!($v, $p) };
}
macro_rules! call_path {
    ($p:path) => { $p(2) };
}
macro_rules! lit {
    ($l:literal) => { $l };
}
fn square(x: i32) -> i32 { x * x }
fn main() {
    println!("{}", show_ty!(Vec<i32>).len());
    let r = run_block!({ let a = 2; a + 3 });
    println!("{}", r);
    println!("{}", test_pat!(Some(3), Some(1..=5)));
    println!("{}", call_path!(square));
    println!("{} {}", lit!(-4), lit!("text"));
}
"#;
    assert_eq!(printed(source), "0\n5\ntrue\n4\n-4 text\n");
}

#[test]
fn nested_and_recursive_calls_and_block_macros_expand() {
    let source = r#"
macro_rules! inner {
    ($x:expr) => { $x * 10 };
}
macro_rules! outer {
    ($x:expr) => { inner!($x) + 1 };
}
macro_rules! count {
    () => { 0usize };
    ($head:tt $($tail:tt)*) => { 1usize + count!($($tail)*) };
}
fn main() {
    println!("{}", outer!(4));
    println!("{}", count!(a b c d e));
    macro_rules! local {
        () => { "local" };
    }
    println!("{}", local!());
    {
        macro_rules! local {
            () => { "shadowed" };
        }
        println!("{}", local!());
    }
    println!("{}", local!());
}
"#;
    assert_eq!(printed(source), "41\n5\nlocal\nshadowed\nlocal\n");
}

#[test]
fn a_later_definition_shadows_an_earlier_one_from_where_it_is() {
    let source = r#"
macro_rules! m { () => { 1 }; }
fn first() -> i32 { m!() }
macro_rules! m { () => { 2 }; }
fn second() -> i32 { m!() }
fn main() {
    println!("{} {}", first(), second());
}
"#;
    assert_eq!(printed(source), "1 2\n");
}

#[test]
fn an_exported_macro_is_in_the_crate_roots_scope_before_its_definition() {
    let source = r#"
fn main() {
    println!("{}", answer!());
}
#[macro_export]
macro_rules! answer {
    () => { 42 };
}
"#;
    assert_eq!(printed(source), "42\n");
}

#[test]
fn a_call_no_rule_matches_is_rustcs_error() {
    let one = "macro_rules! one { ($x:ident) => { $x }; }\n";
    let no_rules = (
        "no rules expected `1`".to_string(),
        Some("no rules expected this token in macro call".to_string()),
    );
    assert_eq!(refused(&format!("{one}fn main() {{ one!(1); }}")), no_rules);
    assert_eq!(
        refused(&format!("{one}fn main() {{ let a = 1; one!(a b); }}")).0,
        "no rules expected `b`"
    );
    assert_eq!(
        refused(&format!("{one}fn main() {{ one!(); }}")),
        (
            "unexpected end of macro invocation".to_string(),
            Some("missing tokens in macro arguments".to_string())
        )
    );
}

#[test]
fn an_unknown_macro_and_one_used_before_its_definition_are_rustcs_errors() {
    assert_eq!(
        refused("fn main() { nope!(); }"),
        ("cannot find macro `nope` in this scope".to_string(), None)
    );
    // rustlings' macros2.
    assert_eq!(
        refused(
            "fn main() {\n    my_macro!();\n}\n\nmacro_rules! my_macro {\n    () => {\n        \
             println!(\"Check out my macro!\");\n    };\n}\n"
        ),
        (
            "cannot find macro `my_macro` in this scope".to_string(),
            Some("consider moving the definition of `my_macro` before this call".to_string())
        )
    );
    // A block's macro ends with its block.
    assert_eq!(
        refused("fn main() {\n    { macro_rules! m { () => { 1 }; } }\n    m!();\n}\n").0,
        "cannot find macro `m` in this scope"
    );
}

#[test]
fn a_malformed_definition_is_rustcs_error() {
    // rustlings' macros4: no `;` between the rules.
    let error = Engine::new()
        .compile(
            "macro_rules! my_macro {\n    () => {\n        println!(\"Check out my macro!\");\n    }\n    \
             ($val:expr) => {\n        println!(\"Look at this other macro: {}\", $val);\n    }\n}\n\
             fn main() {}\nmain();\n",
        )
        .map(|_| ())
        .expect_err("rustc refuses it");
    assert_eq!(
        error.to_string().lines().next(),
        Some("error: expected `;`, found `(`")
    );
}

#[test]
fn recursion_stops_at_the_crates_recursion_limit() {
    let (message, _) = refused(
        "#![recursion_limit = \"16\"]\nmacro_rules! r { () => { r!() }; }\nfn main() { r!(); }",
    );
    assert_eq!(message, "recursion limit reached while expanding `r!`");
}

#[test]
fn an_error_in_an_expansion_of_an_argument_and_the_macros_own_tokens_is_reported() {
    // tracing's `level_enabled!($lvl)` compares `$lvl` with its own
    // `LevelFilter::current()`: the comparison starts at the argument and
    // ends at the macro's tokens, which sit where it is called (#1603).
    let source = "struct Level;\nfn current() -> Level {\n    Level\n}\n\
                  macro_rules! raised {\n    ($level:expr) => {\n        $level + current()\n    };\n}\n\
                  fn main() {\n    let raised = raised!(Level);\n}\nmain();\n";
    let error = Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rustc refuses it");
    let source = Source::new("main.rs", source.to_owned()).expect("a test source has an identity");
    let report = Report::from_parse(source, &error).expect("the error is in the source");
    assert!(
        report
            .message()
            .starts_with("cannot add `Level` to `Level`"),
        "{report:?}"
    );
}
