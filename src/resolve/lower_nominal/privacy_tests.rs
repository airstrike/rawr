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
