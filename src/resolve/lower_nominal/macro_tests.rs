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

/// rustc's message and primary label for `source`, which rawr refuses. A
/// position on the lines the calls' expansions are laid out on is reported
/// at the call, as the package's source reports it.
fn refused(source: &str) -> (String, Option<String>) {
    let source = format!("{source}\nmain();\n");
    let engine = Engine::new();
    let error = engine
        .compile(&source)
        .map(|_| ())
        .expect_err("rawr refuses this program");
    let unit = engine.parse_unit(&source).expect("the program parses");
    let source = Source::new("main.rs", source)
        .expect("a test source has an identity")
        .with_expansions(unit.source().text(), unit.expansions().into());
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

/// A repetition whose metavariables all repeat again inside it runs once
/// for each time it matched, those whose own repetition matched nothing
/// too, as rustc counts them.
#[test]
fn a_repetition_counts_its_iterations_whose_own_repetitions_matched_nothing() {
    let source = r#"
macro_rules! widths {
    ($( ( $($x:expr),* ) )*) => {{
        let mut widths: Vec<usize> = Vec::new();
        $(
            let row: Vec<i64> = vec![$($x),*];
            widths.push(row.len());
        )*
        widths
    }};
}
fn main() {
    println!("{:?}", widths!(() (1, 2) ()));
}
"#;
    assert_eq!(printed(source), "[0, 2, 0]\n");
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

/// A call among a file's items in each delimiter expands to its items,
/// `mk!();` too, whose `()` the lexer reads as one token (#2343).
#[test]
fn an_item_call_in_each_delimiter_expands() {
    let source = r#"
macro_rules! mk {
    ($name:ident, $value:expr) => { fn $name() -> i32 { $value } };
    () => { fn unit() -> i32 { 1 } };
}
mk!();
mk!(paren, 2);
mk![bracket, 3];
mk! { brace, 4 }
fn main() {
    println!("{} {} {} {}", unit(), paren(), bracket(), brace());
}
"#;
    assert_eq!(printed(source), "1 2 3 4\n");
}

/// A call where a type or a pattern goes is the type or the pattern it
/// expands to (#961), and one that expands 120 deep, which rustc's limit
/// of 128 allows, nests as deep as it does (#962).
#[test]
fn a_call_expands_where_a_type_or_a_pattern_goes_and_nests_120_deep() {
    let source = r#"
macro_rules! number {
    () => { i64 };
}
macro_rules! some {
    ($x:pat) => { Some($x) };
}
macro_rules! count {
    () => { 0 };
    ($head:tt $($tail:tt)*) => { 1 + count!($($tail)*) };
}
fn main() {
    let x: number!() = 4;
    let o: Option<Vec<number!()>> = Some(vec![x]);
    match o {
        some!(v) if v.len() == 1 => println!("one {}", v[0]),
        _ => println!("other"),
    }
    println!("{}", matches!(Some(3), some!(3 | 4)));
    let n: u32 = count!(
        a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a
        a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a
        a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a
    );
    println!("{n}");
}
"#;
    assert_eq!(printed(source), "one 4\ntrue\n120\n");
}

/// A call used as an expression declares the items of the blocks it
/// expands to, as rustc declares them before it resolves a name (#2188):
/// each call's `enum E` and `fn double` are its own block's, so two calls
/// in one block each declare them. The output is rustc's.
#[test]
fn an_expression_call_declares_the_items_of_its_blocks() {
    let source = r#"
macro_rules! pick {
    ($n:expr) => {{
        #[derive(Clone, Copy)]
        enum E { A, B }
        fn double(n: i32) -> i32 { n * 2 }
        let e = if double($n) == 0 { E::A } else { E::B };
        match e { E::A => "a", E::B => "b" }
    }};
}
fn main() {
    let first = pick!(0);
    let second = pick!(1);
    println!("{first} {second}");
}
"#;
    assert_eq!(printed(source), "a b\n");
}

/// A path through a module in a call's expansion resolves as a written
/// one does (#2192): `m::of()` names the module-level `mod m`, and
/// `m::E::A` the module a block of the expansion declares. The output is
/// rustc's.
#[test]
fn a_path_in_an_expansion_names_a_module() {
    let source = r#"
mod m {
    pub fn of() -> i32 { 7 }
}
macro_rules! pick {
    () => {{ m::of() }};
}
macro_rules! inner {
    () => {{
        mod m { #[derive(Debug)] pub enum E { A } }
        m::E::A
    }};
}
fn main() {
    let e = inner!();
    let n = pick!();
    println!("{n} {e:?}");
}
"#;
    assert_eq!(printed(source), "7 A\n");
}

/// An expression call's block declares a module whose items a closure in
/// the expansion names, as tokio's `select!` names its block's
/// `__tokio_select_util` from its `poll_fn` closure (#2188), and a call
/// in a closure's body or a call's arguments declares its own. The output
/// is rustc's.
#[test]
fn a_closure_in_an_expansion_names_the_module_its_block_declares() {
    let source = r#"
macro_rules! first {
    ($($value:expr),+) => {{
        mod util {
            #[derive(Debug)]
            pub enum Out<T> { Got(T), Empty }
        }
        let pick = |values: &[i32]| match values.first() {
            Some(value) => util::Out::Got(*value),
            None => util::Out::Empty,
        };
        pick(&[$($value),+])
    }};
}
fn main() {
    let out = first!(4, 5);
    let wrapped = Some(first!(6));
    let mapped: Vec<_> = [7, 8].iter().map(|n| first!(*n + 1)).collect();
    println!("{out:?} {wrapped:?} {mapped:?}");
}
"#;
    assert_eq!(printed(source), "Got(4) Some(Got(6)) [Got(8), Got(9)]\n");
}

/// An item in a call's expansion names the items beside it in its block
/// (#2193), as rustc resolves them there: an expression's own block, and
/// the block a statement's call is in. The output is rustc's.
#[test]
fn an_expanded_item_names_its_block_siblings() {
    let source = r#"
macro_rules! pick {
    () => {{
        #[derive(Debug)]
        enum E { A }
        fn of() -> E { E::A }
        of()
    }};
}
macro_rules! declare {
    () => {
        #[derive(Debug)]
        enum F { B }
        fn of() -> F { F::B }
    };
}
fn main() {
    let e = pick!();
    declare!();
    println!("{e:?} {:?}", of());
}
"#;
    assert_eq!(printed(source), "A B\n");
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

/// A macro that calls itself through `$crate` where an item goes, with an
/// internal rule per step, puts `#[cfg(..)]` on the call each step expands
/// to, and so expands only the items of the branch whose `cfg` holds, as
/// `cfg_if!`'s rules do (#2236). The output is rustc's.
#[test]
fn a_recursive_item_macro_keeps_the_items_whose_cfg_holds() {
    let source = r#"
#[macro_export]
macro_rules! pick {
    (if #[cfg( $($i_meta:tt)+ )] { $( $i_tokens:tt )* } $( else { $( $e_tokens:tt )* } )?) => {
        $crate::pick! {
            @__items () ;
            (( $($i_meta)+ ) ( $( $i_tokens )* )),
            $( (() ( $( $e_tokens )* )), )?
        }
    };
    (@__items ( $( ($($_:tt)*) , )* ) ; ) => {};
    (
        @__items ( $( ($($no:tt)+) , )* ) ;
        (( $( $($yes:tt)+ )? ) ( $( $tokens:tt )* )),
        $( $rest:tt , )*
    ) => {
        #[cfg(all( $( $($yes)+ , )? not(any( $( $($no)+ ),* )) ))]
        $crate::pick! { @__temp_group $( $tokens )* }
        $crate::pick! {
            @__items ( $( ($($no)+) , )* $( ($($yes)+) , )? ) ;
            $( $rest , )*
        }
    };
    (@__temp_group $( $tokens:tt )* ) => { $( $tokens )* };
}
pick! {
    if #[cfg(unix)] {
        fn kind() -> &'static str { "unix" }
    } else {
        fn kind() -> &'static str { "other" }
    }
}
fn main() {
    println!("{}", kind());
}
"#;
    assert_eq!(printed(source), "unix\n");
}

/// In a body, `crate::name!` calls the program's `#[macro_export]` macro
/// where an expression or a statement goes, and a call's `#[cfg(..)]`
/// that doesn't hold leaves it and the items it expands to out, written
/// or in an expansion. The output is rustc's.
#[test]
fn a_body_calls_an_exported_macro_by_its_crate_path_and_under_cfg() {
    let source = "#[macro_export]\nmacro_rules! one { () => { 1 }; }\n\
                  #[macro_export]\nmacro_rules! item { () => { fn f() -> i32 { 3 } }; }\n\
                  fn main() { println!(\"{}\", crate::one!()); crate::item! {} println!(\"{}\", f()); }";
    assert_eq!(printed(source), "1\n3\n");
    let source = "#[macro_export]\nmacro_rules! item { () => { fn f() -> i32 { 3 } }; }\n\
                  fn main() { #[cfg(windows)] crate::item! {} #[cfg(unix)] item! {} println!(\"{}\", f()); }";
    assert_eq!(printed(source), "3\n");
    let source = "macro_rules! wrap { ($($t:tt)*) => { #[cfg(unix)] inner! { $($t)* } }; }\n\
                  macro_rules! inner { ($($t:tt)*) => { $($t)* }; }\n\
                  fn main() { wrap! { fn f() -> i32 { 3 } } println!(\"{}\", f()); }";
    assert_eq!(printed(source), "3\n");
    let source = "macro_rules! wrap { ($($t:tt)*) => { #[cfg(windows)] inner! { $($t)* } inner! { $($t)* } }; }\n\
                  macro_rules! inner { ($($t:tt)*) => { $($t)* }; }\n\
                  fn main() { wrap! { fn f() -> i32 { 3 } } println!(\"{}\", f()); }";
    assert_eq!(printed(source), "3\n");
}

/// A body's call whose expansion starts with a braced call of a macro,
/// itself among them, is statements, as rustc parses a statement's
/// expansion: the items each call expands to are the block's, and a call
/// that expands to nothing is no statement (#2591). So `cfg_if!`'s rules
/// expand in a body. The output is rustc's.
#[test]
fn a_body_call_that_expands_to_calls_of_itself_declares_their_items() {
    let source = "macro_rules! wrap { (@x $($t:tt)*) => { $($t)* }; ($($t:tt)*) => { wrap! { @x $($t)* } wrap! { @x } }; }\n\
                  fn main() { wrap! { fn f() -> i32 { 3 } } println!(\"{}\", f()); }";
    assert_eq!(printed(source), "3\n");
    let source = r#"
macro_rules! pick {
    (if #[cfg( $($i_meta:tt)+ )] { $( $i_tokens:tt )* } $( else { $( $e_tokens:tt )* } )?) => {
        pick! {
            @__items () ;
            (( $($i_meta)+ ) ( $( $i_tokens )* )),
            $( (() ( $( $e_tokens )* )), )?
        }
    };
    (@__items ( $( ($($_:tt)*) , )* ) ; ) => {};
    (
        @__items ( $( ($($no:tt)+) , )* ) ;
        (( $( $($yes:tt)+ )? ) ( $( $tokens:tt )* )),
        $( $rest:tt , )*
    ) => {
        #[cfg(all( $( $($yes)+ , )? not(any( $( $($no)+ ),* )) ))]
        pick! { @__temp_group $( $tokens )* }
        pick! {
            @__items ( $( ($($no)+) , )* $( ($($yes)+) , )? ) ;
            $( $rest , )*
        }
    };
    (@__temp_group $( $tokens:tt )* ) => { $( $tokens )* };
}
fn main() {
    pick! {
        if #[cfg(unix)] {
            fn kind() -> &'static str { "unix" }
        } else {
            fn kind() -> &'static str { "other" }
        }
    }
    println!("{}", kind());
}
"#;
    assert_eq!(printed(source), "unix\n");
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

/// A name a call passes in keeps the caller's scope, where a `use` brought
/// it: a variant a module re-exports by name is found in an expansion as
/// it is outside one, as rustc finds it.
#[test]
fn an_argument_names_what_the_callers_use_brought() {
    let source = r#"
mod w {
    #[derive(Debug, Clone, Copy)]
    pub enum Alignment {
        Start,
        Center,
    }
    pub use Alignment::Center;
    pub fn boxed(alignment: Alignment) -> Alignment {
        alignment
    }
}
use w::Center;
macro_rules! column {
    ($($x:expr),+ $(,)?) => ([$(w::boxed($x)),+]);
}
fn main() {
    let outside = Center;
    let inside = column![w::Alignment::Start, Center];
    println!("{outside:?} {inside:?}");
}
"#;
    assert_eq!(printed(source), "Center [Start, Center]\n");
}

/// Each fragment is what rustc's parser reads (#237): an `expr` is one
/// expression, so a closure's `,`, an `if`'s or a `match`'s braces and a
/// struct expression stay in it, and a `ty`, a `pat`, a `stmt`, an
/// `item`, a `meta`, a `lifetime`, a `vis` and a suffixed `literal` are
/// each one of theirs. The output is rustc's.
#[test]
fn a_fragment_is_what_rusts_parser_reads() {
    let source = r#"
use std::collections::HashMap;
macro_rules! apply { ($f:expr, $v:expr) => { ($f)($v, $v) }; }
macro_rules! show { ($($e:expr),* $(,)?) => { $(print!("{:?} ", $e);)* println!(); }; }
macro_rules! ty_of { ($t:ty, $v:expr) => {{ let x: $t = $v; x }}; }
macro_rules! pats {
    ($v:expr, $($p:pat $(if $g:expr)? => $r:expr),+) => { match $v { $($p $(if $g)? => $r),+ } };
}
macro_rules! param { ($p:pat_param | $q:pat_param) => { 1 }; }
macro_rules! item { ($i:item) => { $i }; }
macro_rules! stmts { ($($s:stmt);*) => {{ $($s;)* }}; }
macro_rules! meta { (#[$m:meta] $i:item) => { #[$m] $i }; }
macro_rules! life { ($l:lifetime) => { fn pick<$l>(a: &$l str) -> &$l str { a } }; }
macro_rules! vis { ($v:vis struct $n:ident) => { $v struct $n; }; }
macro_rules! lit { ($($l:literal),*) => { $(print!("{} ", $l);)* println!(); }; }
macro_rules! cast { ($e:expr) => { $e as u8 }; }
struct P { x: i32 }
item!(fn made() -> i32 { 4 });
meta!(#[derive(Debug)] struct M(i32););
life!('a);
vis!(pub struct V);
fn main() {
    println!("{}", apply!(|a: i32, b: i32| a + b, 3));
    let v = vec![1, 2, 3];
    let o: Option<i32> = Some(2);
    show!(1 + 2 * 3, -v[0], !true, &v[1], v.len() as i64, v.iter().map(|x| x + 1).collect::<Vec<_>>());
    show!(if v.is_empty() { 0 } else { 1 }, match o { Some(n) if n > 1 => n, _ => 0 }, P { x: 4 }.x, (1, 2).1);
    show!({ let a = 3; a * a }, loop { break 5 }, 'outer: loop { break 'outer 6 }, [0; 3], 1..=3);
    println!("{:?}", ty_of!(HashMap<String, Vec<u8>>, HashMap::new()).len());
    println!("{} {}", ty_of!(&'static str, "hi"), ty_of!(fn(i32) -> i32, |x| x + 1)(2));
    println!("{}", pats!(Some(3), Some(1..=2) => "low", Some(x) if x > 2 => "high", None | Some(_) => "none"));
    println!("{} {}", pats!((1, 2), (a, b) => a + b), param!(Some(1) | None));
    stmts!(let a = 1; let b = a + 1; println!("{} {}", b, made()));
    let _ = V;
    println!("{:?} {}", M(2), pick("x"));
    lit!(1u8, 2.5f32, -3i64, 'c', "s", true);
    println!("{}", cast!(300 + 1));
}
"#;
    assert_eq!(
        printed(source),
        "6\n7 -1 false 2 3 [2, 3, 4] \n1 2 4 2 \n9 5 6 [0, 0, 0] 1..=3 \n0\nhi 3\nhigh\n3 1\n\
         2 4\nM(2) x\n1 2.5 -3 c s true \n45\n"
    );
}

/// rustc's error for `source`, which rawr refuses, as rawr prints it.
fn report(source: &str) -> String {
    Engine::new()
        .compile(format!("{source}\nmain();\n"))
        .map(|_| ())
        .expect_err("rawr refuses this program")
        .to_string()
}

/// A fragment followed by what Rust's follow set for it does not allow is
/// rustc's error where the macro is defined (#237).
#[test]
fn a_fragment_its_follow_set_refuses_is_rustcs_error() {
    let refused = |source: &str, message: &str, fragment: &str| {
        let report = report(source);
        assert_eq!(report.lines().next(), Some(message), "{report}");
        assert!(
            report.contains(&format!("not allowed after `{fragment}` fragments")),
            "{report}"
        );
        assert!(report.contains("= note: allowed there are: "), "{report}");
    };
    refused(
        "macro_rules! a { ($e:expr foo) => {}; }\nfn main() {}",
        "error: `$e:expr` is followed by `foo`, which is not allowed for `expr` fragments",
        "expr",
    );
    refused(
        "macro_rules! c { ($t:ty $u:ident) => {}; }\nfn main() {}",
        "error: `$t:ty` is followed by `$u:ident`, which is not allowed for `ty` fragments",
        "ty",
    );
    refused(
        "macro_rules! e { ($($e:expr),* + ) => {}; }\nfn main() {}",
        "error: `$e:expr` may be followed by `+`, which is not allowed for `expr` fragments",
        "expr",
    );
    refused(
        "fn main() { macro_rules! d { ($p:pat | $q:pat) => {}; } }",
        "error: `$p:pat` is followed by `|`, which is not allowed for `pat` fragments",
        "pat",
    );
    // What a repetition's last fragment meets again at its start, a block
    // after a type, and an item after a visibility are allowed.
    assert_eq!(
        printed(
            "macro_rules! a { ($($t:ty)*) => {}; }\nmacro_rules! b { ($t:ty $b:block) => {}; }\n\
             macro_rules! c { ($v:vis fn) => {}; }\nfn main() { println!(\"ok\"); }"
        ),
        "ok\n"
    );
}

/// A fragment the call's tokens start but don't finish is rustc's parser
/// error, labelled at the matcher's fragment, and no later rule is tried
/// (#2511). A call whose tokens don't start the fragment is still a
/// no-rules error.
#[test]
fn a_fragment_that_starts_but_does_not_parse_is_rustcs_parser_error() {
    let two = "macro_rules! two { ($a:expr) => { $a }; (1 +) => { 0 }; }\n";
    let report = report(&format!("{two}fn main() {{ let _ = two!(1 +); }}"));
    assert_eq!(
        report.lines().next(),
        Some("error: expected expression, found end of macro arguments"),
        "{report}"
    );
    assert!(report.contains("expected expression"), "{report}");
    assert!(
        report.contains("while parsing argument for this `expr` macro fragment"),
        "{report}"
    );
    let message = |definition: &str, call: &str| {
        refused(&format!(
            "macro_rules! m {{ ({definition}) => {{}}; }}\nfn main() {{ m!({call}); }}"
        ))
        .0
    };
    assert_eq!(
        message("$a:expr", "a + fn"),
        "expected expression, found keyword `fn`"
    );
    assert_eq!(
        message("[$a:expr]", "[1 +]"),
        "expected expression, found `]`"
    );
    assert_eq!(message("$a:expr", "1 as"), "expected type, found `<eof>`");
    assert_eq!(message("$a:ty", "&"), "expected type, found `<eof>`");
    assert_eq!(
        message("$a:pat", "Some(1) |"),
        "expected pattern, found `<eof>`"
    );
    assert_eq!(
        message("$a:path", "a::"),
        "expected identifier, found `<eof>`"
    );
    assert_eq!(message("$a:item", "1"), "expected an item keyword");
    assert_eq!(message("$a:expr", "fn"), "no rules expected keyword `fn`");
    assert_eq!(message("$a:ident", "1"), "no rules expected `1`");
}

/// A call no rule matches names a keyword, a reserved word and a lifetime
/// as rustc does.
#[test]
fn a_no_rules_error_names_the_token_as_rustc_does() {
    let one = "macro_rules! one { (a) => {}; }\n";
    let message = |call: &str| refused(&format!("{one}fn main() {{ one!({call}); }}")).0;
    assert_eq!(message("fn"), "no rules expected keyword `fn`");
    assert_eq!(
        message("yield"),
        "no rules expected reserved keyword `yield`"
    );
    assert_eq!(message("_"), "no rules expected reserved identifier `_`");
    assert_eq!(message("'a"), "no rules expected `'a`");
    assert_eq!(message("union"), "no rules expected `union`");
}

/// A transcriber's repetition that starts with an operator, inside a
/// separated repetition, expands to that operator between operands
/// (#2202). The output is rustc's.
#[test]
fn an_operator_repetition_expands_inside_a_separated_one() {
    let source = r#"
macro_rules! sums { ($( ($($x:expr),*) ),*) => { [$( 0 $(+ $x)* ),*] }; }
macro_rules! widths {
    ($( ($($x:expr),*) )*) => {{ let mut widths = Vec::new(); $( widths.push(0 $(+ $x * 0 + 1)*); )* widths }};
}
macro_rules! less { ($($x:expr),*) => { 10 $(- $x)* }; }
fn main() {
    println!("{:?} {:?} {}", sums!((1, 2), (3), ()), widths!((1, 2) (3) ()), less!(1, 2));
}
"#;
    assert_eq!(printed(source), "[3, 3, 0] [2, 1, 0] 7\n");
}

/// Hygiene keeps a macro's locals apart from the caller's names, and
/// leaves alone what no local is (#2068): a field's name, written
/// `Point { x }` too, a path's segment, a method's name. A format string's
/// `{x}` and a `match` arm's binding are the macro's own. The output is
/// rustc's.
#[test]
fn hygiene_renames_locals_and_leaves_fields_and_paths() {
    let source = r#"
#[derive(Debug, Clone, Copy)]
struct Point { x: i32, y: i32 }
mod staged { pub fn play(x: &i32) -> i32 { *x + 1 } }
macro_rules! show {
    ($v:expr) => {{
        let x = $v;
        let p = Point { x, y: x + 1 };
        let Point { x: a, y } = p;
        let Point { ref x, .. } = p;
        format!("{x} {a} {y} {p:?} {{x}}")
    }};
}
macro_rules! play { ($play:expr) => {{ let play = &$play; staged::play(play) }}; }
macro_rules! arm { ($o:expr, $e:expr) => { match $o { Some(v) => { v + $e } None => $e } }; }
fn main() {
    let x = 10;
    let v = 100;
    println!("{} {}", show!(x * 2), x);
    println!("{} {} {}", play!(5), arm!(Some(1), v), arm!(None::<i32>, v));
}
"#;
    assert_eq!(
        printed(source),
        "20 20 21 Point { x: 20, y: 21 } {x} 10\n6 101 100\n"
    );
}

/// An error at a macro's local names it as the macro writes it, not as
/// hygiene renamed it.
#[test]
fn an_error_names_a_macros_local_as_written() {
    let source = "macro_rules! m { () => {{ let a = String::from(\"x\"); let b = a; \
                  println!(\"{}\", a); b }}; }\nfn main() { let _ = m!(); }\nmain();\n";
    let engine = Engine::new();
    let error = engine
        .compile(source)
        .map(|_| ())
        .expect_err("rawr refuses this program");
    let unit = engine.parse_unit(source).expect("the program parses");
    let source = Source::new("main.rs", source.to_owned())
        .expect("a test source has an identity")
        .with_expansions(unit.source().text(), unit.expansions().into());
    let report = Report::from_parse(source, &error).expect("the error is in the source");
    assert_eq!(report.code().as_str(), "E0382");
    assert_eq!(report.message(), "borrow of moved value: `a`");
}
