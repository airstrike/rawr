//! Programs where rawr once answered differently from rustc. Each one
//! rawr runs prints what rustc's build of it prints, and each one it
//! refuses is refused with rustc's code and message.

use crate::Engine;

/// What `source` prints.
fn printed(source: &str) -> String {
    let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut engine = Engine::new();
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    engine.run(source).unwrap_or_else(|error| panic!("{error}"));
    lines.lock().unwrap().clone()
}

/// The report `rawr` prints for `source`, which it refuses.
fn refused(source: &str) -> String {
    Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rawr refuses this program")
        .to_string()
}

#[test]
fn language_get_of_a_range_is_an_optional_slice() {
    // std's `SliceIndex` for a range: `Option<&[T]>`, `None` out of bounds
    // (#772).
    let output = printed(
        r#"
fn main() {
    let v = vec![1, 2, 3, 4];
    let s: Option<&[i32]> = v.get(1..3);
    println!("{:?} {:?} {:?}", s, v.get(..2), v[..].get(2..));
    let a = [1, 2, 3];
    println!("{:?} {:?}", a.get(1..=2), a.get(1));
    println!("{:?} {:?}", v.get(3..9), v.get(3..1));
}
main();
"#,
    );
    assert_eq!(
        output,
        "Some([2, 3]) Some([1, 2]) Some([3, 4])\nSome([2, 3]) Some(2)\nNone None\n"
    );
    // The slice borrows `v`, as `get(i)`'s element does.
    let report = refused(
        r#"
fn main() {
    let mut v = vec![1, 2, 3];
    let s = v.get(0..2);
    v.push(4);
    println!("{:?}", s);
}
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable"
        ),
        "{report}"
    );
}

#[test]
fn language_a_mut_self_method_on_a_local_not_declared_mut_is_e0596() {
    // rustc's E0596 for a program type's `&mut self` method, as for std's
    // (#774).
    let report = refused(
        r#"
struct Counter {
    n: i32,
}

impl Counter {
    fn bump(&mut self) {
        self.n += 1;
    }
}

fn main() {
    let c = Counter { n: 0 };
    c.bump();
    println!("{}", c.n);
}
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0596]: cannot borrow `c` as mutable, as it is not declared as mutable\n"
        ),
        "{report}"
    );
}

#[test]
fn language_text_into_with_no_target_is_e0283() {
    // std has many `From<String>` impls: the use must give the target
    // (#736).
    let report = refused("fn main() { let x = String::new().into(); }\nmain();\n");
    assert!(
        report.starts_with("error[E0283]: type annotations needed\n"),
        "{report}"
    );
    // A use that gives it. `Err("text".into())` boxed as `dyn Error` is
    // `language_box_dyn_error_holds_any_error`'s.
    let output = printed(
        r#"
struct Named {
    name: String,
}

fn shout(text: String) -> String {
    text.to_uppercase()
}

fn main() {
    let a: String = "a".into();
    let b = "b".into();
    let c: String = b;
    let named = Named { name: "n".into() };
    println!("{a} {c} {} {}", named.name, shout("s".into()));
}
main();
"#,
    );
    assert_eq!(output, "a b n S\n");
}

#[test]
fn language_a_std_type_with_a_wrong_argument_count_is_e0107() {
    // rustc's E0107, from std's declaration: its parameters, defaults and
    // unstable ones counted (#752).
    for (ty, message) in [
        (
            "Vec<i32, i32, i32>",
            "struct takes at most 2 generic arguments but 3 generic arguments were supplied",
        ),
        ("Vec", "missing generics for struct `Vec`"),
        (
            "Option<i32, i32>",
            "enum takes 1 generic argument but 2 generic arguments were supplied",
        ),
        (
            "Result<i32>",
            "enum takes 2 generic arguments but 1 generic argument was supplied",
        ),
        (
            "HashMap<i32>",
            "struct takes at least 2 generic arguments but 1 generic argument was supplied",
        ),
    ] {
        let report = refused(&format!(
            "use std::collections::HashMap;\nfn f(x: {ty}) {{}}\nfn main() {{}}\nmain();\n"
        ));
        assert!(
            report.starts_with(&format!("error[E0107]: {message}\n")),
            "{report}"
        );
    }
}

#[test]
fn language_an_unstable_std_method_is_e0658() {
    // std declares it, behind a feature stable Rust can't enable (#771).
    // `Option::map_or_default`, the issue's example, is stable in the
    // table's toolchain.
    let report =
        refused("fn main() { let s = String::from(\"ab\"); let c = s.into_chars(); }\nmain();\n");
    assert!(
        report.starts_with("error[E0658]: use of unstable library feature `string_into_chars`\n"),
        "{report}"
    );
    assert!(report.contains("issues/133125"), "{report}");
}

#[test]
fn language_an_operator_its_left_type_has_no_impl_of_is_e0369() {
    // rustc's E0369, in its words for each operator (#733).
    for (expression, message) in [
        ("\"a\" + \"b\"", "cannot add `&str` to `&str`"),
        ("true - false", "cannot subtract `bool` from `bool`"),
        ("\"a\" * \"b\"", "cannot multiply `&str` by `&str`"),
        ("true / false", "cannot divide `bool` by `bool`"),
        (
            "\"a\" % \"b\"",
            "cannot calculate the remainder of `&str` divided by `&str`",
        ),
    ] {
        let report = refused(&format!("fn main() {{ let x = {expression}; }}\nmain();\n"));
        assert!(
            report.starts_with(&format!("error[E0369]: {message}\n")),
            "{report}"
        );
    }
}

#[test]
fn language_a_deref_mut_impl_may_name_self_target() {
    // `DerefMut`'s `Self::Target` is its `Deref` impl's, as rustc
    // normalizes it (#858).
    let output = printed(
        r#"
use std::ops::{Deref, DerefMut};

struct W(Vec<i32>);

impl Deref for W {
    type Target = Vec<i32>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for W {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn main() {
    let mut w = W(vec![1]);
    w.push(2);
    println!("{:?} {}", w.0, w.len());
}
main();
"#,
    );
    assert_eq!(output, "[1, 2] 2\n");
    // A type other than the `Target` is still refused (rustc's E0053).
    let report = refused(
        r#"
use std::ops::{Deref, DerefMut};
struct W(Vec<i32>, String);
impl Deref for W {
    type Target = Vec<i32>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for W {
    fn deref_mut(&mut self) -> &mut String {
        &mut self.1
    }
}
fn main() {}
main();
"#,
    );
    assert!(
        report.contains("has an incompatible type for trait `DerefMut`"),
        "{report}"
    );
}

#[test]
fn language_deref_of_a_program_type_goes_through_its_deref() {
    // `*w` is `*Deref::deref(&w)`, and `*DerefMut::deref_mut(&mut w)`
    // where it is assigned (#859).
    let output = printed(
        r#"
use std::ops::{Deref, DerefMut};

struct W(Vec<i32>);

impl Deref for W {
    type Target = Vec<i32>;
    fn deref(&self) -> &Vec<i32> {
        println!("deref");
        &self.0
    }
}

impl DerefMut for W {
    fn deref_mut(&mut self) -> &mut Vec<i32> {
        println!("deref_mut");
        &mut self.0
    }
}

struct N(i32);

impl Deref for N {
    type Target = i32;
    fn deref(&self) -> &i32 {
        &self.0
    }
}

impl DerefMut for N {
    fn deref_mut(&mut self) -> &mut i32 {
        &mut self.0
    }
}

fn main() {
    let mut w = W(vec![1]);
    println!("{:?}", *w);
    *w = vec![5, 6];
    println!("{:?}", w.0);
    let mut n = N(2);
    *n += 3;
    let m = *n + 1;
    println!("{} {}", n.0, m);
}
main();
"#,
    );
    assert_eq!(output, "deref\n[1]\nderef_mut\n[5, 6]\n5 6\n");
    // rustc's E0594 with no `DerefMut`, and E0507 for a move out of it.
    let program = |body: &str| {
        format!(
            r#"
use std::ops::Deref;
struct R(String);
impl Deref for R {{
    type Target = String;
    fn deref(&self) -> &String {{
        &self.0
    }}
}}
fn main() {{
    let r = R(String::from("a"));
    {body}
}}
main();
"#
        )
    };
    for (body, expected) in [
        (
            r#"*r = String::from("x");"#,
            "error[E0594]: cannot assign to data in dereference of `R`\n",
        ),
        (
            r#"let s = *r; println!("{s}");"#,
            "error[E0507]: cannot move out of dereference of `R`\n",
        ),
    ] {
        let report = refused(&program(body));
        assert!(report.starts_with(expected), "{report}");
    }
}

#[test]
fn language_text_into_takes_the_type_inference_gives_it() {
    // A branch's `.into()` takes the type the other branch, the return
    // type or a block's `let` gives it, as rustc infers it (#860).
    let output = printed(
        r#"
use std::borrow::Cow;

fn f(b: bool) -> Cow<'static, str> {
    if b { "x".into() } else { Cow::Owned(String::new()) }
}

fn g() -> Cow<'static, str> {
    "y".into()
}

fn h(b: bool) -> Cow<'static, str> {
    match b {
        true => "t".into(),
        false => {
            "f".into()
        }
    }
}

fn s(b: bool) -> String {
    if b { "s".into() } else { String::new() }
}

fn main() {
    println!("{:?} {:?} {:?} {:?} {}", f(true), f(false), g(), h(false), s(true));
    let c: Cow<str> = if true { "a".into() } else { "b".into() };
    let d: Cow<str> = {
        let t = "blk";
        t.into()
    };
    let x: Cow<str> = match 1 {
        1 => "one".into(),
        _ => Cow::Borrowed("other"),
    };
    println!("{c} {d} {x}");
}
main();
"#,
    );
    assert_eq!(output, "\"x\" \"\" \"y\" \"f\" s\na blk one\n");
}

#[test]
fn language_a_script_value_of_a_cow_shows_its_payload() {
    // A `Cow`'s `Debug` and `Display` are its payload's, as std's are, for
    // a script's final value as for `{:?}` (#861).
    let value = Engine::new()
        .eval::<crate::Dynamic>(
            "use std::borrow::Cow;\nlet c: Cow<str> = Cow::Borrowed(\"a\");\nc\n",
        )
        .unwrap();
    assert_eq!(format!("{value:?} {value}"), "\"a\" a");
}

#[test]
fn language_return_of_an_assignment_is_unit() {
    // An assignment expression is `()` (#862).
    let output = printed(
        r#"
#[derive(Default, Debug)]
struct P {
    x: i32,
}

fn reset(p: &mut P) {
    return *p = P::default()
}

fn main() {
    let mut p = P { x: 3 };
    reset(&mut p);
    println!("{:?} {}", p, p.x);
}
main();
"#,
    );
    assert_eq!(output, "P { x: 0 } 0\n");
    // In a function returning `i32`, a mismatch, as rustc's E0308 is.
    let report = refused("fn bump(n: &mut i32) -> i32 { return *n = 3 }\nfn main() {}\nmain();\n");
    assert!(
        report.contains("error[E0308]: mismatched types")
            && report.contains("expected `i32`, found `()`"),
        "{report}"
    );
}

/// rustc's errors for `!`: a `-> !` body that can finish is a type
/// mismatch, which expects `!` and finds `()`. A `!` anywhere but a
/// function's, a closure's or a `fn` pointer's output is E0658, the
/// unstable `never_type`. An `Fn` trait's output is such a place.
#[test]
fn language_never_type_errors_are_rustcs() {
    for (source, expected) in [
        (
            "fn f() -> ! { println!(\"hi\"); }\nfn main() { f() }",
            "expected `!`, found `()`",
        ),
        (
            "fn f() -> ! {}\nfn main() { f() }",
            "expected `!`, found `()`",
        ),
        (
            "fn f() -> ! { let mut n = 0; while n < 3 { n += 1; } }\nfn main() { f() }",
            "expected `!`, found `()`",
        ),
        (
            "fn f() -> ! { 5 }\nfn main() { f() }",
            "expected `!`, found integer",
        ),
        (
            "fn main() { let c = || -> ! { 1 }; c(); }",
            "expected `!`, found integer",
        ),
    ] {
        let report = refused(&format!("{source}\nmain();\n"));
        assert!(report.contains(expected), "{source}: {report}");
    }
    for source in [
        "fn main() { let x: ! = panic!(); }",
        "fn f(x: !) {}\nfn main() {}",
        "struct S { a: ! }\nfn main() {}",
        "fn f() -> Option<!> { None }\nfn main() {}",
        "fn main() { let v: Vec<!> = Vec::new(); }",
        "fn run(f: impl Fn(&str) -> !) {}\nfn main() {}",
        "fn run<F: FnOnce() -> !>(f: F) {}\nfn main() {}",
        "fn main() { let f: Box<dyn Fn() -> !> = Box::new(|| panic!()); }",
    ] {
        let report = refused(&format!("{source}\nmain();\n"));
        assert!(
            report.starts_with("error[E0658]: the `!` type is experimental\n")
                && report.contains("issues/35121"),
            "{source}: {report}"
        );
    }
}

#[test]
fn language_let_else_parse_into_a_field_type_runs() {
    // rustlings from_into: `parse`'s target comes from the struct field the
    // binding fills (#906).
    let output = printed(
        r#"
#[derive(Debug)]
struct Person {
    name: String,
    age: u8,
}

impl Default for Person {
    fn default() -> Self {
        Self { name: String::from("John"), age: 30 }
    }
}

impl From<&str> for Person {
    fn from(s: &str) -> Self {
        let mut split = s.split(',');
        let (Some(name), Some(age), None) = (split.next(), split.next(), split.next()) else {
            return Self::default();
        };
        if name.is_empty() {
            return Self::default();
        }
        let Ok(age) = age.parse() else {
            return Self::default();
        };
        Self { name: name.into(), age }
    }
}

fn main() {
    let p1 = Person::from("Mark,20");
    println!("{p1:?}");
    let p2: Person = "Gerald,70".into();
    println!("{p2:?}");
    println!("{:?}", Person::from("Mark,twenty"));
    println!("{:?}", Person::from("Mark,300"));
}
main();
"#,
    );
    assert_eq!(
        output,
        "Person { name: \"Mark\", age: 20 }\nPerson { name: \"Gerald\", age: 70 }\n\
         Person { name: \"John\", age: 30 }\nPerson { name: \"John\", age: 30 }\n"
    );
}

#[test]
fn language_collect_into_a_result_from_the_return_type_runs() {
    // rustlings iterators3: the `Result` target is known only from the
    // function's return type (#906).
    let output = printed(
        r#"
#[derive(Debug, PartialEq, Eq)]
enum DivisionError {
    DivideByZero,
    NotDivisible,
}

fn divide(a: i64, b: i64) -> Result<i64, DivisionError> {
    if b == 0 {
        return Err(DivisionError::DivideByZero);
    }
    if a % b != 0 {
        return Err(DivisionError::NotDivisible);
    }
    Ok(a / b)
}

fn result_with_list() -> Result<Vec<i64>, DivisionError> {
    let numbers = [27, 297, 38502, 81];
    let division_results = numbers.into_iter().map(|n| divide(n, 27));
    division_results.collect()
}

fn failing_list() -> Result<Vec<i64>, DivisionError> {
    let numbers = [27, 28, 0];
    numbers.into_iter().map(|n| divide(n, 27)).collect()
}

fn list_of_results() -> Vec<Result<i64, DivisionError>> {
    let numbers = [27, 297, 38502, 81];
    let division_results = numbers.into_iter().map(|n| divide(n, 27));
    division_results.collect()
}

fn main() {
    println!("{:?}", result_with_list());
    println!("{:?}", failing_list());
    println!("{:?}", list_of_results());
    assert_eq!(result_with_list(), Ok(vec![1, 11, 1426, 3]));
}
main();
"#,
    );
    assert_eq!(
        output,
        "Ok([1, 11, 1426, 3])\nErr(NotDivisible)\n[Ok(1), Ok(11), Ok(1426), Ok(3)]\n"
    );
    // Rust stops at the first `Err` or `None`: the closure runs no further.
    // A set or a map from the return type is built as one.
    let output = printed(
        r#"
use std::collections::{BTreeSet, HashMap};
fn set() -> BTreeSet<i32> { vec![3, 1, 3].into_iter().collect() }
fn map() -> HashMap<i32, i32> { vec![(1, 2), (1, 5)].into_iter().collect() }
fn main() {
    let o: Option<String> = vec![Some("a"), Some("b")].into_iter().collect();
    let mut n = 0;
    let r: Result<Vec<i32>, String> = (0..10)
        .map(|i| {
            n += 1;
            if i == 3 { Err(format!("bad {i}")) } else { Ok(i) }
        })
        .collect();
    let none = vec![Some(1), None, Some(3)].into_iter().collect::<Option<Vec<i32>>>();
    println!("{o:?} {r:?} {n} {none:?} {:?} {:?}", set(), map().get(&1));
}
main();
"#,
    );
    assert_eq!(
        output,
        "Some(\"ab\") Err(\"bad 3\") 4 None {1, 3} Some(5)\n"
    );
    for (source, expected) in [
        (
            "fn f() -> Vec<String> { vec![1, 2].into_iter().collect() }",
            "`Vec<String>` cannot be built from an iterator over elements of type `{integer}`",
        ),
        (
            "use std::collections::HashMap;\nfn f() -> HashMap<i32, i32> { vec![1].into_iter().collect() }",
            "`HashMap<i32, i32>` cannot be built from an iterator over elements of type `{integer}`",
        ),
    ] {
        let report = refused(&format!("{source}\nfn main() {{}}\nmain();\n"));
        assert!(
            report.starts_with("error[E0277]: a value of type ") && report.contains(expected),
            "{source}: {report}"
        );
    }
}

#[test]
fn language_a_mut_self_trait_method_on_a_vec_changes_it() {
    // rustlings traits2: `mut self` is a local the method may change (#907).
    let output = printed(
        r#"
trait AppendBar {
    fn append_bar(self) -> Self;
}

impl AppendBar for Vec<String> {
    fn append_bar(mut self) -> Self {
        self.push(String::from("Bar"));
        self
    }
}

fn main() {
    let mut foo = vec![String::from("Foo")].append_bar();
    println!("{:?}", foo);
    assert_eq!(foo.pop().unwrap(), "Bar");
    assert_eq!(foo.pop().unwrap(), "Foo");
}
main();
"#,
    );
    assert_eq!(output, "[\"Foo\", \"Bar\"]\n");
    // Through a closure's capture, `self` is changed in place, or refused;
    // never a copy changed and dropped.
    let source = r#"
trait Grow {
    fn grow(self) -> Self;
}

impl Grow for Vec<i32> {
    fn grow(mut self) -> Self {
        let mut add = |n: i32| self.push(n);
        add(7);
        self
    }
}

fn main() {
    println!("{:?}", vec![3].grow());
}
main();
"#;
    let mut engine = Engine::new();
    let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    if engine.run(source).is_ok() {
        assert_eq!(*lines.lock().unwrap(), "[3, 7]\n");
    }
}

#[test]
fn language_a_mutating_method_on_a_temporary_vec_runs() {
    // rustlings clippy3: rustc changes the temporary and drops it (#907).
    let output = printed(
        r#"
fn main() {
    let my_empty_vec = vec![1, 2, 3, 4, 5].resize(0, 5);
    println!("This Vec is empty, see? {my_empty_vec:?}");
    let mut n = vec![3, 1, 2];
    n.clone().push(4);
    println!("{n:?}");
    n.push(5);
    println!("{n:?}");
}
main();
"#,
    );
    assert_eq!(
        output,
        "This Vec is empty, see? ()\n[3, 1, 2]\n[3, 1, 2, 5]\n"
    );
    // A place reached through a reference is changed where it is.
    let output = printed(
        r#"
fn main() {
    let mut v = vec![1];
    let r = &mut v;
    (*r).push(2);
    println!("{:?}", v);
}
main();
"#,
    );
    assert_eq!(output, "[1, 2]\n");
}

#[test]
fn language_a_recursive_type_without_indirection_is_e0072() {
    // rustlings box1 (#911).
    let report = refused(
        r#"
#[derive(PartialEq, Debug)]
enum List {
    Cons(i32, List),
    Nil,
}

fn create_empty_list() -> List {
    todo!()
}

fn main() {
    println!("{:?}", create_empty_list());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0072]: recursive type `List` has infinite size\n"),
        "{report}"
    );
    for source in [
        "struct Node { value: i32, next: Node }\nfn main() {}",
        "struct A { b: B }\nstruct B { a: Option<A> }\nfn main() {}",
        "struct P(i32, (u8, P));\nfn main() {}",
    ] {
        let report = refused(&format!("{source}\nmain();\n"));
        assert!(
            report.starts_with("error[E0072]: recursive type"),
            "{source}: {report}"
        );
    }
    printed(
        r#"
enum List {
    Cons(i32, Box<List>),
    Nil,
}
struct Tree { children: Vec<Tree>, parent: Option<Box<Tree>> }
fn main() {
    let _ = List::Cons(1, Box::new(List::Nil));
    let _ = Tree { children: Vec::new(), parent: None };
}
main();
"#,
    );
}

#[test]
fn language_collecting_unit_items_into_a_vec_of_i32_is_e0277() {
    // rustlings vecs2: refused even in a function nothing calls (#911).
    let report = refused(
        r#"
fn vec_map(input: &[i32]) -> Vec<i32> {
    input
        .iter()
        .map(|element| {
            // ???
        })
        .collect()
}

fn main() {}
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0277]: a value of type `Vec<i32>` cannot be built from an iterator \
             over elements of type `()`\n"
        ),
        "{report}"
    );
}
