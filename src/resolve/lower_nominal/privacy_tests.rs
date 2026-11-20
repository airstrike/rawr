//! Rust's name resolution and privacy across a file's inline modules: a
//! module and its descendants see its private items and imports, a glob
//! import reaches the names its module sees (#899, #900), and a private
//! item from outside is rustc's E0603 or E0624. Each expected output and
//! error is what rustc gives for the same file.

use crate::Engine;

/// What `source`'s `main` prints, compiled as one file with its inline
/// modules.
fn printed(source: &str) -> String {
    let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut engine = Engine::new();
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    let unit = engine.parse_unit(source).unwrap();
    let evidence = crate::diagnostic::Source::new("main.rs", source).unwrap();
    let ast = engine
        .compile_unit(unit, evidence)
        .unwrap_or_else(|error| panic!("{error}"));
    engine
        .call_fn::<()>(&mut crate::Scope::new(), &ast, "main", ())
        .unwrap_or_else(|error| panic!("{error}"));
    lines.lock().unwrap().clone()
}

/// The report `rawr` prints for compiling `source`, which rustc rejects.
fn rejected(source: &str) -> String {
    let engine = Engine::new();
    let unit = engine.parse_unit(source).unwrap();
    let evidence = crate::diagnostic::Source::new("main.rs", source).unwrap();
    let error = engine
        .compile_unit(unit, evidence)
        .map(|_| ())
        .expect_err("rustc rejects this program");
    error.rendered("main.rs", source).to_string()
}

/// rustlings' `mod tests { use super::*; }` calls the root's private
/// associated functions and methods; a grandchild reaches them by
/// `super::super`, and a module's child reaches its parent's. A generic
/// type's private method named `print` (quiz3) is one of them.
#[test]
fn a_descendant_calls_its_ancestors_private_functions() {
    let output = printed(
        r#"
use std::fmt::Display;

struct Rectangle {
    width: i32,
    height: i32,
}

impl Rectangle {
    fn new(width: i32, height: i32) -> Self {
        Rectangle { width, height }
    }
    fn area(&self) -> i32 {
        self.width * self.height
    }
}

struct ReportCard<T> {
    grade: T,
    name: String,
}

impl<T: Display> ReportCard<T> {
    fn print(&self) -> String {
        format!("{} - {}", self.name, self.grade)
    }
}

mod shapes {
    pub struct Square(pub i32);

    impl Square {
        fn side(&self) -> i32 {
            self.0
        }
    }

    pub mod measure {
        pub fn perimeter(square: &super::Square) -> i32 {
            square.side() * 4
        }
    }
}

mod tests {
    use super::*;

    pub fn run() {
        let rectangle = Rectangle::new(3, 4);
        println!("{}", rectangle.area());
        let card = ReportCard { grade: 2.1, name: "Tom".to_string() };
        println!("{}", card.print());
        let card = ReportCard { grade: "A+", name: "Gary".to_string() };
        println!("{}", card.print());
    }

    pub mod deeper {
        pub fn run() {
            println!("{}", super::super::Rectangle::new(2, 5).area());
        }
    }
}

fn main() {
    tests::run();
    tests::deeper::run();
    println!("{}", shapes::measure::perimeter(&shapes::Square(3)));
}
"#,
    );
    assert_eq!(output, "12\nTom - 2.1\nGary - A+\n10\n12\n");
}

/// A private associated function or method of a module the call is
/// outside of is rustc's E0624, named as the source writes it.
#[test]
fn a_private_function_from_outside_its_module_is_e0624() {
    let report = rejected(
        r#"
mod a {
    pub struct P {
        pub x: i32,
    }
    impl P {
        fn new() -> Self {
            P { x: 3 }
        }
        pub fn make() -> Self {
            P { x: 4 }
        }
    }
}
mod b {
    pub fn t() -> i32 {
        crate::a::P::new().x
    }
}
fn main() {
    println!("{}", b::t() + a::P::make().x);
}
"#,
    );
    assert!(
        report.starts_with("error[E0624]: associated function `new` is private\n"),
        "{report}"
    );

    let report = rejected(
        r#"
mod a {
    pub struct W<T> {
        pub v: T,
    }
    impl<T: std::fmt::Display> W<T> {
        fn print(&self) -> String {
            format!("{}", self.v)
        }
    }
}
fn main() {
    let w = a::W { v: 1 };
    println!("{}", w.print());
}
"#,
    );
    assert!(
        report.starts_with("error[E0624]: method `print` is private\n"),
        "{report}"
    );
    assert!(!report.contains("__rawr_"), "{report}");
}

/// A private function is private to the module of its impl, not of its
/// type: an impl in a child module keeps it from the type's own module.
#[test]
fn a_private_function_is_private_to_its_impls_module() {
    let source = |visibility: &str| {
        format!(
            r#"
struct P(i32);
mod extra {{
    impl super::P {{
        {visibility}fn doubled(&self) -> i32 {{
            self.0 * 2
        }}
    }}
    pub fn check() -> i32 {{
        super::P(5).doubled()
    }}
}}
fn main() {{
    println!("{{}} {{}}", P(4).doubled(), extra::check());
}}
"#
        )
    };
    assert_eq!(printed(&source("pub(crate) ")), "8 10\n");
    let report = rejected(&source(""));
    assert!(
        report.starts_with("error[E0624]: method `doubled` is private\n"),
        "{report}"
    );
}

/// A path through a private module, or to a private item, from outside
/// its module is rustc's E0603.
#[test]
fn a_path_to_a_private_item_is_e0603() {
    let report = rejected(
        r#"
mod a {
    fn f() -> i32 {
        1
    }
}
fn main() {
    println!("{}", a::f());
}
"#,
    );
    assert!(
        report.starts_with("error[E0603]: function `f` is private\n"),
        "{report}"
    );

    let report = rejected(
        r#"
mod a {
    mod hidden {
        pub fn f() -> i32 {
            1
        }
    }
}
fn main() {
    println!("{}", a::hidden::f());
}
"#,
    );
    assert!(
        report.starts_with("error[E0603]: module `hidden` is private\n"),
        "{report}"
    );

    let report = rejected(
        r#"
mod a {
    struct S;
}
use a::S;
fn main() {}
"#,
    );
    assert!(
        report.starts_with("error[E0603]: struct `S` is private\n"),
        "{report}"
    );
}

/// `use super::*;` reaches the parent's own `use` imports, and a name it
/// brings in may start another `use` (`use Progress::*;`). A grandchild's
/// glob reaches what its parent's glob brought in, as the root's `use
/// Shape::*;` brings the variants.
#[test]
fn a_glob_import_reaches_the_names_its_module_sees() {
    let output = printed(
        r#"
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Progress {
    None,
    Some,
    Complete,
}

#[derive(Debug)]
enum Shape {
    Circle,
    Square,
}
use Shape::*;

fn count(map: &HashMap<String, Progress>, value: Progress) -> usize {
    map.values().filter(|v| **v == value).count()
}

mod tests {
    use super::*;
    use Progress::*;

    pub struct Named(pub Rc<String>);

    impl fmt::Display for Named {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "<{}>", self.0)
        }
    }

    pub fn run() {
        let mut map = HashMap::new();
        map.insert("a".to_string(), Complete);
        map.insert("b".to_string(), Some);
        map.insert("c".to_string(), Complete);
        println!("{} {}", count(&map, Complete), count(&map, None));
        let name = Rc::new("x".to_string());
        println!("{} {}", Named(Rc::clone(&name)), Rc::strong_count(&name));
    }

    pub mod inner {
        use super::*;

        pub fn run() {
            let shapes: HashMap<i32, Shape> = HashMap::from([(1, Circle)]);
            println!("{:?} {:?} {}", shapes, Square, Named(Rc::new("y".to_string())));
        }
    }
}

fn main() {
    tests::run();
    tests::inner::run();
}
"#,
    );
    assert_eq!(output, "2 0\n<x> 2\n{1: Circle} Square <y>\n");
}

/// A glob import of a module the importer is outside of reaches only its
/// public names: neither its private functions nor its private imports,
/// which rustc then finds nowhere (E0425).
#[test]
fn a_glob_import_from_outside_skips_private_names() {
    let report = rejected(
        r#"
mod a {
    fn f() -> i32 {
        1
    }
    pub fn g() -> i32 {
        2
    }
}
mod b {
    use crate::a::*;
    pub fn t() -> i32 {
        g() + f()
    }
}
fn main() {
    println!("{}", b::t());
}
"#,
    );
    assert!(
        report.starts_with("error[E0425]: cannot find function `f` in this scope\n"),
        "{report}"
    );

    let report = rejected(
        r#"
mod a {
    use std::collections::HashMap;
    pub fn g() -> usize {
        HashMap::<i32, i32>::new().len()
    }
}
mod b {
    use crate::a::*;
    pub fn t() -> usize {
        let h: HashMap<i32, i32> = HashMap::new();
        h.len() + g()
    }
}
fn main() {
    println!("{}", b::t());
}
"#,
    );
    assert!(
        report.starts_with("error[E0425]: cannot find type `HashMap` in this scope\n"),
        "{report}"
    );
}

/// `{self:?}` and `{self}` in a method read its receiver (#903).
#[test]
fn an_inline_format_argument_reads_self() {
    let output = printed(
        r#"
use std::fmt;

#[derive(Debug)]
enum Message {
    Move { x: i32, y: i32 },
    Quit,
}

impl Message {
    fn call(&self) {
        println!("{self:?}");
    }
}

struct Point(i32, i32);

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.0, self.1)
    }
}

impl Point {
    fn show(&self) -> String {
        format!("at {self}")
    }
}

mod tests {
    pub fn run() {
        super::Message::Move { x: 1, y: 2 }.call();
        super::Message::Quit.call();
        println!("{}", super::Point(3, 4).show());
    }
}

fn main() {
    tests::run();
}
"#,
    );
    assert_eq!(output, "Move { x: 1, y: 2 }\nQuit\nat (3, 4)\n");
}

struct NamedKey;

/// A `use` that starts at another `use`'s crate module (`use
/// keyboard::key;` after `use iced::keyboard;`) is reached through the
/// path it starts at from a child module's `use super::*;`, as a crate
/// derive's expansion reaches the item's module in iced's todos (#1044).
#[test]
fn a_child_glob_reaches_a_use_that_starts_at_another_use() {
    let mut builder =
        crate::host::facade::Builder::new("iced", crate::host::facade::Abi::new(1).unwrap())
            .unwrap();
    builder.module("keyboard").unwrap();
    builder.module("keyboard::key").unwrap();
    builder
        .type_::<NamedKey>("keyboard::key::Named", &[], &[])
        .unwrap();
    let mut engine = Engine::new();
    engine
        .register_host_facade(builder.finish().unwrap())
        .unwrap();
    let source = r#"
use iced::keyboard;
use keyboard::key;

mod shortcuts {
    use super::*;

    pub fn none() -> Option<key::Named> {
        None
    }
}

fn main() {
    println!("{}", shortcuts::none().is_none());
}
"#;
    let unit = engine.parse_unit(source).unwrap();
    let evidence = crate::diagnostic::Source::new("main.rs", source).unwrap();
    engine
        .compile_unit(unit, evidence)
        .unwrap_or_else(|error| panic!("{error}"));
}

/// A `mod` in a block is a module of the enclosing one, as rustc reads it:
/// its items are its own, its `use super::*;` reaches the enclosing
/// module's items, and its private function is rustc's E0603 from the
/// block. Its name is its block's only: two blocks' `mod helper` and the
/// module's are three modules, and outside its block the name is rustc's
/// E0433.
#[test]
fn a_module_in_a_block_sees_the_enclosing_module() {
    let output = printed(
        r#"
const BASE: i32 = 10;

mod helper {
    pub fn f() -> i32 {
        100
    }
}

fn main() {
    let first = {
        mod helper {
            pub struct T(pub i32);
            pub fn f() -> T {
                T(1)
            }
        }
        fn g(t: helper::T) -> i32 {
            t.0
        }
        let t: helper::T = helper::f();
        g(t)
    };
    let second = {
        mod helper {
            pub fn f() -> i32 {
                2
            }
        }
        use helper::f;
        f()
    };
    println!("{first} {second} {}", helper::f());
    let total = {
        mod helper {
            use super::*;
            pub fn f() -> i32 {
                g() + inner::h() + BASE
            }
            fn g() -> i32 {
                1
            }
            pub mod inner {
                pub fn h() -> i32 {
                    super::g() + 1
                }
            }
        }
        helper::f()
    };
    println!("{total}");
}
"#,
    );
    assert_eq!(output, "1 2 100\n13\n");
    let output = printed(
        r#"
fn main() {
    let first = {
        mod util {
            pub enum Out<A> {
                Value(A),
                Disabled,
            }
        }
        let out = util::Out::Value(3);
        match out {
            util::Out::Value(x) => x,
            util::Out::Disabled => 0,
        }
    };
    let second = {
        mod util {
            pub enum Out<A> {
                Value(A),
                Disabled,
            }
        }
        use util::*;
        let out: Out<i32> = Out::Disabled;
        {
            match out {
                util::Out::Value(x) => x,
                util::Out::Disabled => 7,
            }
        }
    };
    println!("{first} {second}");
}
"#,
    );
    assert_eq!(output, "3 7\n");
    let report = rejected(
        r#"
fn main() {
    mod helper {
        fn g() -> i32 {
            1
        }
    }
    println!("{}", helper::g());
}
"#,
    );
    assert!(
        report.starts_with("error[E0603]: function `g` is private\n"),
        "{report}"
    );
    let report = rejected(
        r#"
fn main() {
    {
        mod helper {
            pub fn f() -> i32 {
                1
            }
        }
        println!("{}", helper::f());
    }
    println!("{}", helper::f());
}
"#,
    );
    assert!(
        report.starts_with("error[E0433]: cannot find module or crate `helper` in this scope\n"),
        "{report}"
    );
}

/// `pub(in path)`, `pub(super)`, `pub(crate)` and `pub(self)` open an item
/// to the module they name and its descendants: functions, methods and
/// associated functions, fields (read, written, matched and named in a
/// literal), a tuple struct's constructor and a `use`'s re-export (#2332).
#[test]
fn a_restricted_visibility_opens_an_item_to_its_module() {
    let output = printed(
        r#"
mod m {
    pub mod q {
        pub(in crate::m) fn g(x: i32) -> i32 { x }
        pub(super) fn h() -> i32 { 2 }
        pub(crate) fn c() -> i32 { 3 }
        pub(self) fn k() -> i32 { 4 }
        pub fn ok() -> i32 { k() }
        pub struct S(pub i32);
        impl S {
            pub(super) fn m(&self) -> i32 { self.0 }
            pub(in crate::m) fn n() -> i32 { 5 }
            pub(crate) fn c(&self) -> i32 { self.0 + 1 }
        }
        pub struct P { pub(crate) x: i32, pub(super) y: i32 }
        pub struct T(pub(in crate::m) i32);
        pub mod deep {
            pub(in crate::m::q) fn up() -> i32 { 6 }
        }
        pub fn deeper() -> i32 { deep::up() }
    }
    pub(in crate::m) use q::g as gg;
    pub(crate) use q::c as cc;
    pub fn inside() -> i32 {
        let mut p = q::P { x: 1, y: 2 };
        p.y += 10;
        let q::P { x, y } = p;
        let q::T(t) = q::T(7);
        q::g(1) + q::h() + gg(1) + q::S(1).m() + q::S::n() + x + y + t
    }
}
fn main() {
    println!("{} {} {} {} {}", m::inside(), m::q::c(), m::q::ok(), m::cc(), m::q::deeper());
    println!("{} {}", m::q::S(4).c(), m::q::S(3).0);
}
"#,
    );
    assert_eq!(output, "30 3 4 3 6\n5 3\n");
}

/// A function restricted to a module from outside it is rustc's E0603,
/// with the function's declaration (#2332).
#[test]
fn a_restricted_function_from_outside_its_module_is_e0603() {
    let report = rejected(
        r#"mod m {
    pub mod q {
        pub(in crate::m) fn g(x: i32) -> i32 { x }
    }
}
fn main() {
    m::q::g(1);
}
"#,
    );
    assert_eq!(
        report,
        "error[E0603]: function `g` is private
 --> main.rs:7:11
  |
7 |     m::q::g(1);
  |           ^ private function
  |
note: the function `g` is defined here
 --> main.rs:3:9
  |
3 |         pub(in crate::m) fn g(x: i32) -> i32 { x }
  |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^"
    );
}

/// A method or associated function restricted to a module, from outside
/// it, is rustc's E0624, labelled at its declaration in the same file.
#[test]
fn a_restricted_method_from_outside_its_module_is_e0624() {
    let source = |call: &str| {
        format!(
            r#"mod m {{
    pub mod q {{
        pub struct S(pub i32);
        impl S {{
            pub(super) fn m(&self) -> i32 {{ self.0 }}
            pub(in crate::m) fn n() -> i32 {{ 2 }}
        }}
    }}
}}
fn main() {{
    {call};
}}
"#
        )
    };
    assert_eq!(
        rejected(&source("m::q::S(1).m()")),
        "error[E0624]: method `m` is private
  --> main.rs:11:16
   |
 5 |             pub(super) fn m(&self) -> i32 { self.0 }
   |             ----------------------------- private method defined here
...
11 |     m::q::S(1).m();
   |                ^ private method"
    );
    assert_eq!(
        rejected(&source("m::q::S::n()")),
        "error[E0624]: associated function `n` is private
  --> main.rs:11:14
   |
 6 |             pub(in crate::m) fn n() -> i32 { 2 }
   |             ------------------------------ private associated function defined here
...
11 |     m::q::S::n();
   |              ^ private associated function"
    );
}

/// A field private to a module, read or written from outside it, is
/// rustc's E0616; named in a struct literal or pattern, its E0451.
#[test]
fn a_private_field_from_outside_its_module_is_e0616_or_e0451() {
    let source = |statement: &str| {
        format!(
            "mod a {{\n    pub struct P {{ pub x: i32, pub(self) y: i32 }}\n    \
             pub struct Q(pub i32, i32);\n    pub fn p() -> P {{ P {{ x: 1, y: 2 }} }}\n    \
             pub fn q() -> Q {{ Q(1, 2) }}\n}}\nfn main() {{\n    let mut p = a::p();\n    \
             {statement}\n}}\n"
        )
    };
    let label = |line: &str, column: usize, width: usize, code: &str, field: &str| {
        let owner = if field == "1" { "Q" } else { "P" };
        format!(
            "error[{code}]: field `{field}` of struct `{owner}` is private\n --> main.rs:9:{column}\n  \
             |\n9 |     {line}\n  |     {}{} private field",
            " ".repeat(column - 5),
            "^".repeat(width)
        )
    };
    for (statement, column, code, field) in [
        ("let _ = p.y;", 15, "E0616", "y"),
        ("p.y = 5;", 7, "E0616", "y"),
        ("let _ = a::q().1;", 20, "E0616", "1"),
        ("let a::P { y, .. } = p;", 16, "E0451", "y"),
        ("let _ = a::P { x: 1, y: 2 };", 26, "E0451", "y"),
    ] {
        assert_eq!(
            rejected(&source(statement)),
            label(statement, column, 1, code, field),
            "{statement}"
        );
    }
}

/// A tuple struct with a field private to a module has a constructor as
/// private: through its path it is rustc's E0603, by its name in a call
/// E0423 and in a pattern E0532.
#[test]
fn a_tuple_struct_with_a_private_field_has_a_private_constructor() {
    let report = rejected(
        r#"mod a {
    pub struct Q(pub i32, i32);
}
fn main() {
    let _ = a::Q(1, 2);
}
"#,
    );
    assert_eq!(
        report,
        "error[E0603]: tuple struct constructor `Q` is private
 --> main.rs:5:16
  |
2 |     pub struct Q(pub i32, i32);
  |                  ------------ a constructor is private if any of the fields is private
...
5 |     let _ = a::Q(1, 2);
  |                ^ private tuple struct constructor"
    );
    let source = |statement: &str| {
        format!(
            "mod a {{\n    pub struct Q(pub i32, i32);\n    pub fn q() -> Q {{ Q(1, 2) }}\n}}\n\
             use a::Q;\nfn main() {{\n    {statement}\n}}\n"
        )
    };
    let report = rejected(&source("let _ = Q(1, 2);"));
    assert!(
        report.starts_with(
            "error[E0423]: cannot initialize a tuple struct which contains private fields\n"
        ),
        "{report}"
    );
    let report = rejected(&source("let Q(x, _) = a::q();"));
    assert!(
        report.starts_with(
            "error[E0532]: cannot match against a tuple struct which contains private fields\n"
        ),
        "{report}"
    );
}

/// A `use` binding a path reaches from outside the module its marker
/// opens it to is rustc's E0603, named by what it binds, with the import
/// and the item it refers to.
#[test]
fn an_import_from_outside_its_reach_is_e0603() {
    let report = rejected(
        r#"mod m {
    pub mod q {
        pub fn g(x: i32) -> i32 { x }
    }
    pub(in crate::m) use q::g as gg;
    pub mod inner {
        pub fn call() -> i32 { super::gg(2) }
    }
}
fn main() {
    println!("{}", m::inner::call());
    m::gg(1);
}
"#,
    );
    assert_eq!(
        report,
        "error[E0603]: function import `gg` is private
  --> main.rs:12:8
   |
12 |     m::gg(1);
   |        ^^ private function import
   |
note: the function import `gg` is defined here...
  --> main.rs:5:26
   |
 5 |     pub(in crate::m) use q::g as gg;
   |                          ^^^^^^^^^^
note: ...and refers to the function `g` which is defined here
  --> main.rs:3:9
   |
 3 |         pub fn g(x: i32) -> i32 { x }
   |         ^^^^^^^^^^^^^^^^^^^^^^^ you could import this directly"
    );
    // A private `use`, which its module's descendants alone see.
    let report = rejected(
        r#"mod a {
    pub mod b {
        pub struct N { pub x: i32 }
    }
    use b::N;
}
fn main() {
    let _: a::N;
}
"#,
    );
    assert!(
        report.starts_with("error[E0603]: struct import `N` is private\n"),
        "{report}"
    );
}

/// A re-export that reaches further than the item it re-exports is
/// rustc's E0364.
#[test]
fn a_reexport_beyond_its_item_s_reach_is_e0364() {
    let report = rejected(
        r#"mod m {
    pub mod q {
        pub(super) fn h() {}
    }
    pub(crate) use q::h as hh;
}
fn main() {}
"#,
    );
    assert_eq!(
        report,
        "error[E0364]: `h` is private, and cannot be re-exported
 --> main.rs:5:20
  |
5 |     pub(crate) use q::h as hh;
  |                    ^^^^^^^^^^
  |
note: consider marking `h` as `pub` in the imported module
 --> main.rs:5:20
  |
5 |     pub(crate) use q::h as hh;
  |                    ^^^^^^^^^^"
    );
    let report = rejected(
        "mod m {\n    pub mod q {\n        pub(crate) fn c() {}\n    }\n    pub use q::c;\n}\n\
         fn main() {}\n",
    );
    assert!(
        report.starts_with(
            "error[E0364]: `c` is only public within the crate, and cannot be re-exported \
             outside\n"
        ),
        "{report}"
    );
}

/// A restriction names an ancestor of its item's module by a path from
/// `crate`, `self` or `super`: else rustc's E0742, its E0433 for a path
/// through no module or above the crate root, or its error for a relative
/// path.
#[test]
fn a_visibility_restricted_to_no_ancestor_is_refused() {
    let first = |source: &str| {
        let report = rejected(source);
        report.lines().take(5).collect::<Vec<_>>().join("\n")
    };
    assert_eq!(
        first("mod a {\n    pub mod b {\n        pub(in crate::x) fn f() {}\n    }\n}\nmod x {}\nfn main() {}\n"),
        "error[E0742]: visibilities can only be restricted to ancestor modules
 --> main.rs:3:16
  |
3 |         pub(in crate::x) fn f() {}
  |                ^^^^^^^^"
    );
    assert_eq!(
        first("mod a {\n    pub(in crate::x) fn f() {}\n}\nfn main() {}\n"),
        "error[E0433]: cannot find `x` in `crate`
 --> main.rs:2:19
  |
2 |     pub(in crate::x) fn f() {}
  |                   ^ could not find `x` in the crate root"
    );
    assert_eq!(
        first("mod a {\n    pub(in self::y) fn g() {}\n}\nfn main() {}\n"),
        "error[E0433]: cannot find `y` in `self`
 --> main.rs:2:18
  |
2 |     pub(in self::y) fn g() {}
  |                  ^ could not find `y` in `self`"
    );
    assert_eq!(
        first("mod a {\n    pub fn f() {}\n    pub(in crate::a::f) fn h() {}\n}\nfn main() {}\n"),
        "error[E0433]: cannot find `f` in `a`
 --> main.rs:3:22
  |
3 |     pub(in crate::a::f) fn h() {}
  |                      ^ expected type, found function `f` in `a`"
    );
    assert_eq!(
        first("mod a {\n    pub(in super::super) fn z() {}\n}\nfn main() {}\n"),
        "error[E0433]: too many leading `super` keywords within `crate::a`
 --> main.rs:2:19
  |
2 |     pub(in super::super) fn z() {}
  |                   ^^^^^ this `super` would go above the crate root"
    );
    assert_eq!(
        first("pub(super) fn top() {}\nfn main() {}\n"),
        "error[E0433]: too many leading `super` keywords within `crate`
 --> main.rs:1:5
  |
1 | pub(super) fn top() {}
  |     ^^^^^ this `super` would go above the crate root"
    );
    assert_eq!(
        rejected("mod a {\n    pub(in a) fn f() {}\n}\nfn main() {}\n"),
        "error: relative paths are not supported in visibilities in 2018 edition or later
 --> main.rs:2:12
  |
2 |     pub(in a) fn f() {}
  |            ^ help: try: `crate::a`"
    );
}
