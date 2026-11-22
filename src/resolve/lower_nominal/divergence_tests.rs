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
fn language_an_operator_on_a_struct_without_its_trait_is_rustcs_error() {
    // A crate type with no impl of the operator's trait, in rustc's code,
    // words and span for each operator (#1615): a binary operator's error
    // is at the operator, `+=`'s and `-`'s at the whole expression.
    for (expression, message, column) in [
        (
            "Level + Level",
            "error[E0369]: cannot add `Level` to `Level`",
            19,
        ),
        (
            "Level <= Level",
            "error[E0369]: binary operation `<=` cannot be applied to type `Level`",
            19,
        ),
        (
            "Level == Level",
            "error[E0369]: binary operation `==` cannot be applied to type `Level`",
            19,
        ),
        (
            "Level | Level",
            "error[E0369]: no implementation for `Level | Level`",
            19,
        ),
        (
            "{ let mut l = Level; l += Level; }",
            "error[E0368]: binary assignment operation `+=` cannot be applied to type `Level`",
            34,
        ),
        (
            "-Level",
            "error[E0600]: cannot apply unary operator `-` to type `Level`",
            13,
        ),
        // The one impl takes its `Rhs`, which `Other` mismatches.
        ("Other + Other", "error[E0308]: mismatched types", 21),
    ] {
        let report = refused(&format!(
            "struct Level;\nstruct Other;\nimpl std::ops::Add<i32> for Other {{\n    \
             type Output = i32;\n    fn add(self, _: i32) -> i32 {{\n        0\n    }}\n}}\n\
             fn main() {{\n    let x = {expression};\n}}\nmain();\n"
        ));
        assert!(
            report.starts_with(&format!("{message}\n  --> <script>:10:{column}")),
            "{expression}: {report}"
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
    // A `Result` or an `Option` of a map or a set collects the payloads
    // into one, as its `FromIterator` does (#937).
    let output = printed(
        r#"
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
fn main() {
    let words: Result<HashMap<String, usize>, String> =
        ["a", "bb", "a"].iter().map(|w| Ok((w.to_string(), w.len()))).collect();
    let words = words.unwrap();
    let failed: Result<BTreeMap<i32, i32>, String> =
        vec![Ok((2, 1)), Err("x".to_string()), Ok((1, 1))].into_iter().collect();
    let set: Option<BTreeSet<i32>> = vec![Some(3), Some(1), Some(3)].into_iter().collect();
    let none: Option<HashSet<i32>> = vec![Some(1), None].into_iter().collect();
    let sorted: Result<BTreeMap<i32, char>, ()> = vec![Ok((2, 'b')), Ok((1, 'a'))].into_iter().collect();
    println!("{} {:?} {failed:?} {set:?} {none:?} {sorted:?}", words.len(), words.get("bb"));
}
main();
"#,
    );
    assert_eq!(
        output,
        "2 Some(2) Err(\"x\") Some({1, 3}) None Ok({1: 'a', 2: 'b'})\n"
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

/// With several `From` impls an unsuffixed literal fits, rustc's literal
/// fallback (`i32`, `f64`) chooses the impl, through `T::from`, `.into()`
/// and `From::from` (#8). `cargo run` prints `i32 i32 i32 f64 i64 f64 i32
/// 300`. With no impl of the fallback type, rustc's E0277 names it.
#[test]
fn language_a_literal_with_several_from_impls_takes_its_fallback_type() {
    let impls = r#"
struct S(&'static str);
impl From<i32> for S { fn from(_: i32) -> Self { S("i32") } }
impl From<i64> for S { fn from(_: i64) -> Self { S("i64") } }
impl From<u8> for S { fn from(_: u8) -> Self { S("u8") } }
struct F(&'static str);
impl From<f32> for F { fn from(_: f32) -> Self { F("f32") } }
impl From<f64> for F { fn from(_: f64) -> Self { F("f64") } }
"#;
    assert_eq!(
        printed(&format!(
            r#"{impls}
fn main() {{
    let a = S::from(5);
    let b: S = 5.into();
    let c: S = From::from(7);
    let d = F::from(1.5);
    let e = S::from(5i64);
    let f: F = 2.5.into();
    let n = 300;
    let g = S::from(n);
    println!("{{}} {{}} {{}} {{}} {{}} {{}} {{}} {{}}", a.0, b.0, c.0, d.0, e.0, f.0, g.0, n);
}}
main();
"#
        )),
        "i32 i32 i32 f64 i64 f64 i32 300\n"
    );
    let report = refused(
        r#"
struct T(i32);
impl From<i64> for T { fn from(_: i64) -> Self { T(1) } }
impl From<u8> for T { fn from(_: u8) -> Self { T(2) } }
fn main() { let t = T::from(5); println!("{}", t.0); }
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0277]: the trait bound `T: From<i32>` is not satisfied\n --> <script>:5:21"
        ),
        "{report}"
    );
}

/// The first lines of the panic `cargo run` of `source` prints, from
/// `thread 'main' panicked at main.rs:<line>:<column>:` to its message.
fn panicked(source: &str) -> String {
    let engine = Engine::new();
    let evidence = crate::diagnostic::Source::new("main.rs", source).unwrap();
    let unit = engine.parse_unit(source).unwrap();
    let (ast, positions) = engine
        .compile_unit_with_positions(unit, evidence.clone())
        .unwrap_or_else(|error| panic!("{error}"));
    let error = engine
        .call_fn::<crate::Dynamic>(&mut crate::Scope::new(), &ast, "main", ())
        .expect_err("the program panics");
    let report = crate::diagnostic::Report::from_evaluation_located(
        evidence,
        positions.translate(&error),
        &error,
    )
    .unwrap();
    let panic = crate::diagnostic::panic(&report);
    panic.lines().take(2).collect::<Vec<_>>().join("\n")
}

/// An index past a slice's end panics at the expression's start, as
/// rustc places a built-in index's panic and as an array's does (#35).
#[test]
fn language_a_slice_index_panics_at_the_expression_start() {
    assert_eq!(
        panicked(
            "fn get(s: &[i32], i: usize) -> i32 {\n    let x = 0;\n    s[i] + x\n}\nfn main() {\n    \
             let i: usize = \"5\".parse().unwrap();\n    println!(\"{}\", get(&[1, 2, 3], i));\n}\n"
        ),
        "thread 'main' panicked at main.rs:3:5:\nindex out of bounds: the len is 3 but the index is 5"
    );
    assert_eq!(
        panicked(
            "fn main() {\n    let i: usize = \"5\".parse().unwrap();\n    let v = vec![1, 2, 3];\n    \
             let s: &[i32] = &v;\n    println!(\"{}\", s[i]);\n}\n"
        ),
        "thread 'main' panicked at main.rs:5:20:\nindex out of bounds: the len is 3 but the index is 5"
    );
}

/// Integer division and remainder by zero, and their overflow, panic with
/// rustc's message at the expression's start, as `cargo run` prints them
/// (#37), as do a slice range out of bounds (at its `[`), an integer
/// `pow` that overflows and an integer `clamp` whose bounds are reversed.
#[test]
fn language_division_by_zero_panics_as_rustc_words_and_places_it() {
    let prelude = "fn main() {\n    let d: i32 = \"0\".parse().unwrap();\n    let n: i64 = \
                   \"0\".parse().unwrap();\n    let v = vec![1, 2, 3];\n    let start: usize = \
                   \"5\".parse().unwrap();\n    let hi: i32 = \"1\".parse().unwrap();\n";
    for (line, at, message) in [
        (
            "println!(\"{}\", 10 / d);",
            "7:20",
            "attempt to divide by zero",
        ),
        (
            "println!(\"{}\", 10 % d);",
            "7:20",
            "attempt to calculate the remainder with a divisor of zero",
        ),
        (
            "println!(\"{}\", 10i64 / n);",
            "7:20",
            "attempt to divide by zero",
        ),
        (
            "println!(\"{}\", 10i64 % n);",
            "7:20",
            "attempt to calculate the remainder with a divisor of zero",
        ),
        (
            "let mut x = 10;\n    x /= d;",
            "8:5",
            "attempt to divide by zero",
        ),
        (
            "println!(\"{}\", i32::MIN % (d - 1));",
            "7:20",
            "attempt to calculate the remainder with overflow",
        ),
        (
            "println!(\"{}\", i64::MIN / (n - 1));",
            "7:20",
            "attempt to divide with overflow",
        ),
        (
            "println!(\"{:?}\", &v[start..]);",
            "7:24",
            "range start index 5 out of range for slice of length 3",
        ),
        (
            "println!(\"{:?}\", &v[1..=start]);",
            "7:24",
            "range end index 5 out of range for slice of length 3",
        ),
        (
            "println!(\"{}\", 3.clamp(5, hi));",
            "7:22",
            "min > max. min = 5, max = 1",
        ),
    ] {
        assert_eq!(
            panicked(&format!("{prelude}    {line}\n}}\n")),
            format!("thread 'main' panicked at main.rs:{at}:\n{message}"),
            "{line}"
        );
    }
}

/// A closure's `?` converts its error into the error the closure's tail
/// gives, through `From`, whichever `?` comes first; one whose body never
/// names another returns the operand's own (#1977).
#[test]
fn a_closure_s_question_mark_converts_into_its_tail_s_error() {
    let source = r#"
#[derive(Debug)]
struct Parse;
#[derive(Debug)]
struct Range;
#[derive(Debug)]
enum Error {
    Parse(Parse),
    Range(Range),
}
impl From<Parse> for Error {
    fn from(error: Parse) -> Self {
        Error::Parse(error)
    }
}
impl From<Range> for Error {
    fn from(error: Range) -> Self {
        Error::Range(error)
    }
}
fn a(ok: bool) -> Result<i32, Parse> {
    if ok { Ok(1) } else { Err(Parse) }
}
fn b(ok: bool) -> Result<i32, Range> {
    if ok { Ok(2) } else { Err(Range) }
}
fn main() {
    let both = |x: bool, y: bool| Ok::<_, Error>(a(x)? + b(y)?);
    println!("{:?} {:?} {:?}", both(true, true), both(false, true), both(true, false));
    let first = |x: bool| {
        let n = a(x)?;
        Ok(n + 1)
    };
    let n: Result<i32, Parse> = first(true);
    println!("{n:?} {:?}", first(false));
}
main();
"#;
    assert_eq!(
        printed(source),
        "Ok(3) Err(Parse(Parse)) Err(Range(Range))\nOk(2) Err(Parse)\n"
    );
}

/// A `{:?}` piece of a value whose type holds a variable only a diverging
/// expression constrains is written after the fallback makes it `!`, as
/// rustc checks the piece's `Debug` once the body is read (#2131). The
/// written piece is longer than its placeholder. The output is rustc's
/// build of the same program.
#[test]
fn a_format_piece_whose_type_holds_a_diverging_variable_waits_for_the_fallback() {
    let source = r#"
fn main() {
    let v: Vec<i32> = Vec::new();
    let o: Vec<_> = v.iter().map(|_| panic!()).collect();
    println!("{:?} {}", o, o.len());
    let w = vec![3, 4];
    let pairs: Vec<_> = w.iter().map(|x| (x, v.iter().map(|_| todo!()).collect::<Vec<_>>())).collect();
    println!("{:?}", pairs);
    let n: Option<_> = None.map(|x: i32| -> _ { if x > 0 { unreachable!() } else { panic!() } });
    println!("{:?} {:#?}", n, o);
}
main();
"#;
    assert_eq!(printed(source), "[] 0\n[(3, []), (4, [])]\nNone []\n");
}

/// A constructor's payload that never comes, as `Some(return)`'s, is a
/// variable a later use settles or the fallback makes `!`, so the value is
/// `Copy` once the fallback makes it `Option<!>`, and a later assignment
/// may give it another type (#2134). The output is rustc's build of the
/// same program.
#[test]
fn a_value_whose_payload_never_comes_is_copied_after_the_fallback() {
    let source = r#"
fn pick(stop: bool) -> i32 {
    let p = if stop { Some(return 1) } else { None };
    let q = p;
    println!("{:?} {:?} {:?}", p, (p, 1), q);
    let r: Result<i32, _> = if stop { Err(return 2) } else { Ok(3) };
    let s = r;
    println!("{:?} {:?}", r, s);
    0
}
fn assigned() -> i32 {
    let mut t = Some(return 4);
    t = Some(String::from("t"));
    println!("{:?}", t);
    0
}
fn main() {
    println!("{} {} {}", pick(false), pick(true), assigned());
    let p = Some(return);
    println!("{:?} {:?}", p, (p, 1));
}
main();
"#;
    assert_eq!(printed(source), "None (None, 1) None\nOk(3) Ok(3)\n0 1 4\n");
}

/// An element that never comes, as `vec![return 2]`'s, is a variable a
/// later use or another element settles, so the vector takes a `String`
/// pushed after it (#2139). The output is rustc's build of the same
/// program.
#[test]
fn an_element_that_never_comes_is_settled_by_a_later_use() {
    let source = r#"
fn pushed() -> i32 {
    let mut v = vec![return 2];
    v.push(String::new());
    println!("{:?}", v);
    0
}
fn mixed(stop: bool) -> i32 {
    let a = if stop { [return 3, 4] } else { [5, 6] };
    println!("{:?}", a);
    0
}
fn main() {
    println!("{} {} {}", pushed(), mixed(false), mixed(true));
}
main();
"#;
    assert_eq!(printed(source), "[5, 6]\n2 0 3\n");
}

/// `()` is a scrutinee as any other value is, matched by `()`, `_` or a
/// binding, through a reference too. The output is rustc's build of the
/// same program.
#[test]
fn a_unit_value_is_matched() {
    let source = r#"
fn main() {
    let out = ();
    let a = match &out { () => true, _ => false };
    let b = match out { () => 1 };
    println!("{a} {b}");
}
main();
"#;
    assert_eq!(printed(source), "true 1\n");
}

/// A trait in scope whose `update(&self, ..)` shares its name with an
/// inherent `update(&mut self, ..)` is the one rustc's probe finds on a
/// value: the `&` step comes before the `&mut` one. rustc counts the
/// arguments against the trait's and refuses with E0061, as rawr does.
#[test]
fn a_trait_method_found_at_the_shared_step_counts_the_arguments() {
    let report = refused(
        r#"
trait Program {
    fn update(&self, a: u32, b: u32, c: u32, d: u32) -> u32;
}
struct Grid {
    n: u32,
}
impl Grid {
    fn update(&mut self, message: u32) {
        self.n += message;
    }
}
impl Program for Grid {
    fn update(&self, a: u32, b: u32, c: u32, d: u32) -> u32 {
        a + b + c + d + self.n
    }
}
struct App {
    grid: Grid,
}
fn main() {
    let mut app = App { grid: Grid { n: 1 } };
    app.grid.update(2);
    println!("{}", app.grid.n);
    println!("{}", Program::update(&app.grid, 1, 1, 1, 1));
}
main();
"#,
    );
    assert!(
        report
            .starts_with("error[E0061]: this method takes 4 arguments but 1 argument was supplied"),
        "{report}"
    );
}

/// An inherent method and a trait method of one name with different
/// receivers, on a value, a `&mut` and a `&` receiver. Each step of rustc's
/// probe takes the receiver by value, then by `&`, then by `&mut`, and the
/// first method found wins, inherent or not. The output is rustc's build of
/// the same program.
#[test]
fn an_inherent_and_a_trait_method_of_one_name_follow_rustc_s_probe() {
    let source = r#"
trait Shared {
    fn tag(&self) -> i32;
}
trait Exclusive {
    fn mark(&mut self) -> i32;
}
struct Cell {
    n: i32,
}
impl Cell {
    fn tag(&mut self) -> i32 {
        self.n += 1;
        100 + self.n
    }
    fn mark(&self) -> i32 {
        200 + self.n
    }
}
impl Shared for Cell {
    fn tag(&self) -> i32 {
        300 + self.n
    }
}
impl Exclusive for Cell {
    fn mark(&mut self) -> i32 {
        self.n += 10;
        400 + self.n
    }
}
fn main() {
    let mut c = Cell { n: 0 };
    let value = (c.tag(), c.mark());
    let exclusive = &mut c;
    let through_mut = (exclusive.tag(), exclusive.mark());
    let shared = &c;
    let through_shared = (shared.tag(), shared.mark());
    println!("{:?} {:?} {:?} {}", value, through_mut, through_shared, c.n);
}
main();
"#;
    assert_eq!(printed(source), "(300, 200) (101, 411) (311, 211) 11\n");
}

/// rustc's resolver puts the trait an impl implements in scope inside
/// that impl, for any receiver (#2458): `Tri.area()` in `impl m::Shape for
/// Sq` needs no `use m::Shape`. Outside the impl it does (E0599). The
/// output is rustc's build of the same program.
#[test]
fn the_trait_an_impl_implements_is_in_scope_inside_it() {
    let source = r#"
mod m {
    pub trait Shape {
        fn area(&self) -> i32;
        fn twice(&self) -> i32;
    }
}
struct Sq(i32);
struct Tri;
impl m::Shape for Tri {
    fn area(&self) -> i32 { 7 }
    fn twice(&self) -> i32 { 14 }
}
impl m::Shape for Sq {
    fn area(&self) -> i32 { self.0 * self.0 }
    fn twice(&self) -> i32 { self.area() * 2 + Tri.area() }
}
fn main() {
    println!("{}", m::Shape::twice(&Sq(3)));
}
main();
"#;
    assert_eq!(printed(source), "25\n");
    let report = refused(
        r#"
mod m { pub trait Shape { fn area(&self) -> i32; } }
struct Sq(i32);
impl m::Shape for Sq { fn area(&self) -> i32 { self.0 } }
impl Sq { fn go(&self) -> i32 { self.area() } }
fn main() { println!("{}", Sq(2).go()); }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0599]: no method named `area` found for reference `&Sq`"),
        "{report}"
    );
}

/// Two traits in scope giving a type the same method at the same probe
/// step is rustc's E0034. With one of them out of scope, as a glob import
/// of one module brings only its trait, the other is the method (#767).
#[test]
fn two_traits_in_scope_with_one_method_are_e0034() {
    let report = refused(
        r#"
trait Left { fn choose(&self) -> i32; }
trait Right { fn choose(&self) -> i32; }
struct Both;
impl Left for Both { fn choose(&self) -> i32 { 1 } }
impl Right for Both { fn choose(&self) -> i32 { 2 } }
fn main() { println!("{}", Both.choose()); }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0034]: multiple applicable items in scope"),
        "{report}"
    );
    let source = r#"
mod a { pub trait Size { fn size(&self) -> i32; } }
mod b { pub trait Size { fn size(&self) -> i32; } }
struct Line;
impl a::Size for Line { fn size(&self) -> i32 { 1 } }
impl b::Size for Line { fn size(&self) -> i32 { 2 } }
use b::*;
fn main() { println!("{}", Line.size()); }
main();
"#;
    assert_eq!(printed(source), "2\n");
    // Inside `impl Display` only `Display` is in scope, so `self.0.fmt(f)`
    // is its `fmt` and not `Debug`'s.
    let source = r#"
use std::fmt;
#[derive(Debug)]
struct Inner(i32);
impl fmt::Display for Inner {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "in{}", self.0) }
}
struct Outer(Inner);
impl fmt::Display for Outer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { self.0.fmt(f) }
}
impl fmt::Debug for Outer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { fmt::Debug::fmt(&self.0, f) }
}
fn main() { println!("{} {:?}", Outer(Inner(1)), Outer(Inner(2))); }
main();
"#;
    assert_eq!(printed(source), "in1 Inner(2)\n");
}

/// `(*w).push(1)` on a program type is `DerefMut::deref_mut(&mut w)`'s
/// place, as rustc's `convert_place_derefs_to_mutable` makes it, and
/// `(*get(&mut v)).push(1)` changes `v` (#873). With `Deref` alone it is
/// rustc's E0596. The output is rustc's build of the same program.
#[test]
fn a_place_deref_a_mut_method_is_called_on_goes_through_deref_mut() {
    let source = r#"
use std::ops::{Deref, DerefMut};
struct W(Vec<i32>);
impl Deref for W {
    type Target = Vec<i32>;
    fn deref(&self) -> &Vec<i32> { println!("deref"); &self.0 }
}
impl DerefMut for W {
    fn deref_mut(&mut self) -> &mut Vec<i32> { println!("deref_mut"); &mut self.0 }
}
fn get(v: &mut Vec<i32>) -> &mut Vec<i32> { v }
fn main() {
    let mut w = W(vec![]);
    (*w).push(1);
    w.push(2);
    println!("{}", (*w).len());
    println!("{:?}", w.0);
    let mut v = vec![];
    (get(&mut v)).push(1);
    (*get(&mut v)).push(2);
    println!("{:?}", v);
}
main();
"#;
    assert_eq!(
        printed(source),
        "deref_mut\nderef_mut\nderef\n2\n[1, 2]\n[1, 2]\n"
    );
    let report = refused(
        r#"
use std::ops::Deref;
struct W(Vec<i32>);
impl Deref for W { type Target = Vec<i32>; fn deref(&self) -> &Vec<i32> { &self.0 } }
fn main() { let mut w = W(vec![1]); (*w).push(1); println!("{}", w.len()); }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0596]: cannot borrow data in dereference of `W` as mutable"),
        "{report}"
    );
}

/// `w[i]` of a program type with no `Index` impl indexes its `Deref`
/// target, and `w[i] = x` its `DerefMut` target, as rustc's index
/// auto-derefs (#817). With `Deref` alone the assignment is rustc's E0596.
/// The output is rustc's build of the same program.
#[test]
fn an_index_goes_through_a_program_s_deref() {
    let source = r#"
use std::ops::{Deref, DerefMut};
struct W(Vec<i32>);
impl Deref for W { type Target = Vec<i32>; fn deref(&self) -> &Vec<i32> { &self.0 } }
impl DerefMut for W { fn deref_mut(&mut self) -> &mut Vec<i32> { &mut self.0 } }
fn main() {
    let mut w = W(vec![1, 2]);
    w[0] = 5;
    w[1] += 1;
    println!("{} {} {:?}", w[0], w[1], &w[..]);
}
main();
"#;
    assert_eq!(printed(source), "5 3 [5, 3]\n");
    let report = refused(
        r#"
use std::ops::Deref;
struct W(Vec<i32>);
impl Deref for W { type Target = Vec<i32>; fn deref(&self) -> &Vec<i32> { &self.0 } }
fn main() { let mut w = W(vec![1]); w[0] = 5; println!("{}", w[0]); }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0596]: cannot borrow data in dereference of `W` as mutable"),
        "{report}"
    );
}

/// `&mut w` where `&mut Vec<i64>` is taken coerces through the program's
/// `DerefMut`, and the callee changes `w` (#816). The output is rustc's
/// build of the same program.
#[test]
fn a_mut_reference_coerces_through_a_program_s_deref_mut() {
    let source = r#"
use std::ops::{Deref, DerefMut};
struct W(Vec<i64>);
impl Deref for W { type Target = Vec<i64>; fn deref(&self) -> &Vec<i64> { &self.0 } }
impl DerefMut for W { fn deref_mut(&mut self) -> &mut Vec<i64> { &mut self.0 } }
fn push_through(v: &mut Vec<i64>) { v.push(9); }
fn main() {
    let mut w = W(vec![]);
    push_through(&mut w);
    push_through(w.deref_mut());
    println!("{:?}", w.0);
}
main();
"#;
    assert_eq!(printed(source), "[9, 9]\n");
}

/// rustc resolves an impl's trait path in the type namespace, so
/// `page::Page` is the trait the module `page` holds, through a `use
/// crate::page::{self}` too. A module the program declares at that path
/// is rustc's E0404. A native trait is a module of rawr's host, which
/// was E0404 too (#2452); only a module the program declares is now. The
/// output and the report are rustc's for the same programs.
#[test]
fn an_impl_s_trait_path_through_a_module_names_the_trait() {
    let source = r#"
mod page {
    pub trait Page { fn title(&self) -> String; }
}
mod component {
    pub mod badge {
        use crate::page::{self};
        pub struct BadgePage;
        impl page::Page for BadgePage { fn title(&self) -> String { "badge".to_string() } }
    }
}
use page::Page;
fn main() { println!("{}", component::badge::BadgePage.title()); }
main();
"#;
    assert_eq!(printed(source), "badge\n");
    let report = refused(
        r#"
mod page { pub mod Page {} }
struct B;
impl page::Page for B {}
fn main() {}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0404]: expected trait, found module `page::Page`"),
        "{report}"
    );
}

/// rustc reads a `where` bound on an associated type of a parameter
/// (`I::Item: AsRef<dyn Item>`, `<I as IntoIterator>::Item: Debug`) and
/// uses it for method lookup and formatting in the body (#383). The output
/// is rustc's for the same program.
#[test]
fn a_where_bound_on_an_associated_type_bounds_it() {
    let source = r#"
use std::fmt::{Debug, Display};
trait Item { fn price(&self) -> i64; }
struct Book(i64);
impl Item for Book { fn price(&self) -> i64 { self.0 } }
fn total<I>(items: I) -> i64
where
    I: IntoIterator,
    I::Item: AsRef<dyn Item>,
{
    items.into_iter().fold(0, |sum, item| sum + item.as_ref().price())
}
fn show<I>(items: I) -> String
where
    I: Iterator,
    I::Item: Display,
{
    items.map(|x| format!("<{x}>")).collect::<Vec<_>>().join(",")
}
fn count<I>(items: I) -> usize
where
    I: IntoIterator,
    <I as IntoIterator>::Item: Debug,
{
    let mut n = 0;
    for x in items { println!("{:?}", x); n += 1; }
    n
}
fn largest<I>(items: I) -> Option<I::Item>
where
    I: IntoIterator,
    I::Item: PartialOrd + Copy,
{
    let mut best = None;
    for x in items {
        best = match best { Some(b) if x <= b => Some(b), _ => Some(x) };
    }
    best
}
fn main() {
    let cart: Vec<Box<dyn Item>> = vec![Box::new(Book(3)), Box::new(Book(4))];
    println!("{}", total(&cart));
    println!("{}", show(vec![1, 2].into_iter()));
    println!("{}", count(vec!["a", "b"]));
    println!("{:?}", largest(vec![3, 9, 4]));
}
main();
"#;
    assert_eq!(printed(source), "7\n<1>,<2>\n\"a\"\n\"b\"\n2\nSome(9)\n");
}

/// rustc reads a `where` bound on a compound type (`Vec<T>: Debug`, `&'a
/// T: Display`, `Vec<T>: Summary`) as what the body may assume, which each
/// call checks (#383). The output is rustc's for the same program.
#[test]
fn a_where_bound_on_a_compound_type_holds_in_the_body() {
    let source = r#"
use std::fmt::{Debug, Display};
trait Summary { fn summary(&self) -> String; }
impl<T: Debug> Summary for Vec<T> {
    fn summary(&self) -> String { format!("{} items: {:?}", self.len(), self) }
}
fn dbg_vec<T>(v: Vec<T>) -> String
where
    Vec<T>: Debug,
{
    format!("{:?}", v)
}
fn by_ref<'a, T>(t: &'a T) -> String
where
    &'a T: Display,
{
    format!("[{}]", t)
}
fn sum<T>(v: Vec<T>) -> String
where
    Vec<T>: Summary,
{
    v.summary()
}
fn main() {
    println!("{}", dbg_vec(vec![1u8, 2]));
    println!("{}", by_ref(&7));
    println!("{}", sum(vec![1, 2]));
}
main();
"#;
    assert_eq!(printed(source), "[1, 2]\n[7]\n2 items: [1, 2]\n");
}

/// A call whose type misses a `where` bound on an associated type is
/// rustc's E0277 at the argument, naming the type and the bound. The body
/// has only the bounds written, and rustc names the associated type
/// `<I as Iterator>::Item` there (#383).
#[test]
fn an_unmet_where_bound_on_an_associated_type_is_e0277() {
    let report = refused(
        r#"
use std::fmt::Display;
fn show<I>(items: I) -> String
where
    I: Iterator,
    I::Item: Display,
{
    items.map(|x| format!("<{x}>")).collect::<Vec<_>>().join(",")
}
fn main() {
    println!("{}", show(vec![vec![1]].into_iter()));
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Vec<{integer}>` doesn't implement `std::fmt::Display`"),
        "{report}"
    );
    assert!(
        report.contains("required by this bound in `show`"),
        "{report}"
    );
    let report = refused(
        r#"
use std::fmt::Debug;
fn show<I>(items: I) -> String
where
    I: Iterator,
    I::Item: Debug,
{
    let mut out = String::new();
    for x in items { out = format!("{}", x); }
    out
}
fn main() { println!("{}", show(vec![1].into_iter())); }
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0277]: `<I as Iterator>::Item` doesn't implement `std::fmt::Display`"
        ),
        "{report}"
    );
}

/// A call whose type misses a `where` bound on a compound type is rustc's
/// E0277 at the argument (#383).
#[test]
fn an_unmet_where_bound_on_a_compound_type_is_e0277() {
    let report = refused(
        r#"
fn by_ref<'a, T>(t: &'a T) -> String
where
    &'a T: std::fmt::Display,
{
    format!("[{}]", t)
}
struct Opaque;
fn main() { println!("{}", by_ref(&Opaque)); }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Opaque` doesn't implement `std::fmt::Display`"),
        "{report}"
    );
    assert!(
        report.contains("required by this bound in `by_ref`"),
        "{report}"
    );
}

#[test]
fn language_a_generic_bounded_by_from_str_parses_as_std_does() {
    // `T: FromStr` in a generic body: `s.parse()` and `T::from_str(s)`
    // run std's impl for the type each call gives `T`, by its kind, as
    // rustc's monomorphised body does (#2464, #2465).
    let output = printed(
        r#"
use std::str::FromStr;

fn parsed<T: FromStr>(s: &str) -> Option<T> {
    s.parse().ok()
}

fn from_str<T: FromStr>(s: &str) -> Option<T> {
    T::from_str(s).ok()
}

fn turbofish<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.parse::<T>().ok()
}

#[derive(Debug)]
struct Point {
    x: i32,
    y: i32,
}

impl FromStr for Point {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (x, y) = s.split_once(',').ok_or("no comma".to_string())?;
        Ok(Point {
            x: x.parse().map_err(|_| "bad x".to_string())?,
            y: y.parse().map_err(|_| "bad y".to_string())?,
        })
    }
}

fn main() {
    let n: Option<u8> = parsed("200");
    println!("{:?} {:?} {:?}", n, parsed::<u8>("300"), parsed::<u8>("-1"));
    println!("{:?} {:?}", parsed::<i8>("-128"), parsed::<i8>("128"));
    println!("{:?} {:?}", parsed::<i64>("-9000000000"), parsed::<u64>("18446744073709551615"));
    println!("{:?} {:?}", parsed::<f64>("2.5"), parsed::<f32>("0.1"));
    println!("{:?} {:?}", parsed::<String>("hi"), parsed::<bool>("true"));
    println!("{:?} {:?}", parsed::<char>("x"), parsed::<char>("xy"));
    let m: Option<u8> = from_str("200");
    println!("{:?} {:?} {:?}", m, from_str::<u8>("300"), from_str::<bool>("no"));
    println!("{:?} {:?}", turbofish::<i32>("42"), turbofish::<u16>("70000"));
    println!("{:?} {:?}", parsed::<Point>("1,2"), from_str::<Point>("1;2"));
}
main();
"#,
    );
    assert_eq!(
        output,
        "Some(200) None None\nSome(-128) None\nSome(-9000000000) Some(18446744073709551615)\n\
         Some(2.5) Some(0.1)\nSome(\"hi\") Some(true)\nSome('x') None\nSome(200) None None\n\
         Some(42) None\nSome(Point { x: 1, y: 2 }) None\n"
    );
}

#[test]
fn language_a_generic_bounded_by_a_std_trait_calls_its_functions() {
    // `T::default()`, `T::from(b)` and a parse whose error is `T::Err`, at
    // the types each call gives `T`.
    let output = printed(
        r#"
use std::str::FromStr;

fn fallback<T: Default>() -> T {
    T::default()
}

fn widened<T: From<u8>>(b: u8) -> T {
    T::from(b)
}

fn checked<T: FromStr>(s: &str) -> Result<T, T::Err> {
    s.parse()
}

#[derive(Debug, Default)]
struct Tally {
    n: u32,
    label: String,
}

fn main() {
    let n: i32 = fallback();
    let s: String = fallback();
    println!("{:?} {:?} {:?} {:?}", n, s, fallback::<u8>(), fallback::<Tally>());
    let w: u32 = widened(7);
    println!("{} {} {}", w, widened::<i64>(9), widened::<u16>(255));
    println!("{:?} {:?}", checked::<u8>("12"), checked::<u8>("x").is_err());
    match checked::<i32>("q") {
        Ok(v) => println!("{v}"),
        Err(e) => println!("{e}"),
    }
}
main();
"#,
    );
    assert_eq!(
        output,
        "0 \"\" 0 Tally { n: 0, label: \"\" }\n7 9 255\nOk(12) true\n\
         invalid digit found in string\n"
    );
}

#[test]
fn language_a_from_str_bound_no_impl_meets_is_e0277() {
    // rustc's E0277 at the call, for a program type without `FromStr`.
    let report = refused(
        r#"
use std::str::FromStr;

fn parsed<T: FromStr>(s: &str) -> Option<T> {
    s.parse().ok()
}

struct Plain;

fn main() {
    let p: Option<Plain> = parsed("x");
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `Plain: FromStr` is not satisfied"),
        "{report}"
    );
    assert!(
        report.contains("11 |     let p: Option<Plain> = parsed(\"x\");"),
        "{report}"
    );
    // And at the parse, for a generic the body doesn't bound by it.
    let report = refused(
        "fn bad<T>(s: &str) -> Option<T> {\n    s.parse().ok()\n}\nfn main() {}\nmain();\n",
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `T: FromStr` is not satisfied"),
        "{report}"
    );
}

#[test]
fn a_returned_reference_widened_to_static_inside_another_is_refused() {
    // rustc 1.99: "lifetime may not live long enough" at `f2`'s `x`. rawr
    // ran it and printed 7.
    signature_verdict(
        r#"
use std::cell::Cell;
fn f1<'a, 'b>(x: &'a &'b Cell<u32>) -> &'b Cell<u32> { *x }
fn f2<'a, 'b>(x: &'a &'a Cell<u32>) -> &'a &'static Cell<u32> { x }
fn f3<'a>(x: &'a &'a Cell<u32>) -> &'static Cell<u32> { f1(f2(x)) }
fn f4<'a>(x: &'a Cell<u32>) -> &'static Cell<u32> { f3(&x) }
fn main() { let c = Cell::new(7); let s = f4(&c); println!("{}", s.get()); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:4:65
  |
4 | fn f2<'a, 'b>(x: &'a &'a Cell<u32>) -> &'a &'static Cell<u32> { x }
  |       -- lifetime `'a` defined here                             ^ returning this value requires that `'a` must outlive `'static`
"#),
    );
    // rustc accepts `f1`, `f3` and `f4` against `f2`'s signature.
    signature_verdict(
        r#"
use std::cell::Cell;
fn f1<'a, 'b>(x: &'a &'b Cell<u32>) -> &'b Cell<u32> { *x }
fn f2<'a, 'b>(_x: &'a &'a Cell<u32>) -> &'a &'static Cell<u32> { unimplemented!() }
fn f3<'a>(x: &'a &'a Cell<u32>) -> &'static Cell<u32> { f1(f2(x)) }
fn f4<'a>(x: &'a Cell<u32>) -> &'static Cell<u32> { f3(&x) }
fn main() { let c = Cell::new(7); if false { f4(&c); } println!("{}", f1(&&c).get()); }
main();
"#,
        Ok("7\n"),
    );
    // rustc: `f1(x)` is a `&'a Cell`, not a `&'static Cell`.
    signature_verdict(
        r#"
use std::cell::Cell;
fn f1<'a, 'b>(x: &'a &'b Cell<u32>) -> &'b Cell<u32> { *x }
fn f3<'a>(x: &'a &'a Cell<u32>) -> &'static Cell<u32> { f1(x) }
fn main() { let c = Cell::new(7); let r = &c; println!("{}", f3(&r).get()); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:4:57
  |
4 | fn f3<'a>(x: &'a &'a Cell<u32>) -> &'static Cell<u32> { f1(x) }
  |       -- lifetime `'a` defined here                     ^^^^^ returning this value requires that `'a` must outlive `'static`
"#),
    );
}

#[test]
fn a_struct_s_two_lifetimes_are_told_apart() {
    // rustc: `p.a` is a `&'a i32`, not a `&'b i32`.
    signature_verdict(
        r#"
struct Pair<'a, 'b> { a: &'a i32, b: &'b i32 }
fn second<'a, 'b>(p: &Pair<'a, 'b>) -> &'b i32 { p.a }
fn main() { let (x, y) = (1, 2); let p = Pair { a: &x, b: &y }; println!("{}", second(&p)); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:50
  |
3 | fn second<'a, 'b>(p: &Pair<'a, 'b>) -> &'b i32 { p.a }
  |           --  -- lifetime `'b` defined here      ^^^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |           |
  |           lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    signature_verdict(
        r#"
struct Pair<'a, 'b> { a: &'a i32, b: &'b i32 }
fn second<'a, 'b>(p: &Pair<'a, 'b>) -> &'b i32 { p.b }
fn first<'a, 'b>(p: Pair<'a, 'b>) -> &'a i32 { p.a }
fn main() { let (x, y) = (1, 2); let p = Pair { a: &x, b: &y }; println!("{} {}", second(&p), first(p)); }
main();
"#,
        Ok("2 1\n"),
    );
    // A method's `self`, of a struct with two lifetimes.
    signature_verdict(
        r#"
struct P<'a, 'b> { a: &'a i32, b: &'b i32 }
impl<'a, 'b> P<'a, 'b> {
    fn get_b(&self) -> &'b i32 { self.a }
}
fn main() { let (x, y) = (1, 2); let p = P { a: &x, b: &y }; println!("{} {}", p.get_b(), p.b); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:4:34
  |
3 | impl<'a, 'b> P<'a, 'b> {
  |      --  -- lifetime `'b` defined here
  |      |
  |      lifetime `'a` defined here
4 |     fn get_b(&self) -> &'b i32 { self.a }
  |                                  ^^^^^^ method was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    signature_verdict(
        r#"
struct P<'a, 'b> { a: &'a i32, b: &'b i32 }
impl<'a, 'b> P<'a, 'b> {
    fn get_b(&self) -> &'b i32 { self.b }
    fn get_a(&self) -> &i32 { self.a }
}
fn main() { let (x, y) = (1, 2); let p = P { a: &x, b: &y }; println!("{} {}", p.get_b(), p.get_a()); }
main();
"#,
        Ok("2 1\n"),
    );
    signature_verdict(
        r#"
fn f<'a, 'b>(x: (&'a i32, &'b i32)) -> (&'b i32, &'a i32) { (x.1, x.0) }
fn main() { let (a, b) = (1, 2); println!("{:?}", f((&a, &b))); }
main();
"#,
        Ok("(2, 1)\n"),
    );
}

#[test]
fn what_a_mut_reference_or_a_cell_holds_is_invariant() {
    // rustc: a `&'a mut &'b i32` is no `&'a mut &'a i32`, though `'b: 'a`.
    signature_verdict(
        r#"
fn f<'a, 'b>(x: &'a mut &'b i32) -> &'a mut &'a i32 { x }
fn main() { let y = 1; let mut r = &y; *f(&mut r) = &2; println!("{}", r); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:55
  |
2 | fn f<'a, 'b>(x: &'a mut &'b i32) -> &'a mut &'a i32 { x }
  |      --  -- lifetime `'b` defined here                ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of a mutable reference to `&i32`
  = note: mutable references are invariant over their type parameter
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    signature_verdict(
        r#"
fn f<'a, 'b>(x: &'a mut Vec<&'b i32>) -> &'a mut Vec<&'a i32> { x }
fn main() { let y = 1; let mut v = vec![&y]; println!("{}", f(&mut v).len()); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:65
  |
2 | fn f<'a, 'b>(x: &'a mut Vec<&'b i32>) -> &'a mut Vec<&'a i32> { x }
  |      --  -- lifetime `'b` defined here                          ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of a mutable reference to `Vec<&i32>`
  = note: mutable references are invariant over their type parameter
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    signature_verdict(
        r#"
use std::cell::Cell;
fn f<'a, 'b: 'a>(x: &'a Cell<&'b i32>) -> &'a Cell<&'a i32> { x }
fn main() { let y = 1; let c = Cell::new(&y); println!("{}", f(&c).get()); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:63
  |
3 | fn f<'a, 'b: 'a>(x: &'a Cell<&'b i32>) -> &'a Cell<&'a i32> { x }
  |      --  -- lifetime `'b` defined here                        ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of the type `Cell<&i32>`, which makes the generic argument `&i32` invariant
  = note: the struct `Cell<T>` is invariant over the parameter `T`
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    // A struct whose field is a cell is invariant in its lifetime.
    signature_verdict(
        r#"
use std::cell::Cell;
struct S<'a> { c: Cell<&'a i32> }
fn f<'a, 'b: 'a>(x: &'a S<'b>) -> &'a S<'a> { x }
fn main() { let y = 1; let s = S { c: Cell::new(&y) }; println!("{}", f(&s).c.get()); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:4:47
  |
4 | fn f<'a, 'b: 'a>(x: &'a S<'b>) -> &'a S<'a> { x }
  |      --  -- lifetime `'b` defined here        ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of the type `S<'_>`, which makes the generic argument `'_` invariant
  = note: the struct `S<'a>` is invariant over the parameter `'a`
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    // rustc accepts each of these: a shared reference, the same lifetimes,
    // or a struct whose field is covariant.
    signature_verdict(
        r#"
use std::cell::Cell;
struct S<'a> { c: &'a i32 }
fn shared<'a, 'b>(x: &'a mut &'b i32) -> &'a &'a i32 { x }
fn same<'a, 'b>(x: &'a mut &'b i32) -> &'a mut &'b i32 { x }
fn cell<'a>(x: &'a Cell<&'a i32>) -> &'a Cell<&'a i32> { x }
fn get<'a, 'b>(x: &'a Cell<&'b i32>) -> &'b i32 { x.get() }
fn covariant<'a, 'b: 'a>(x: &'a S<'b>) -> &'a S<'a> { x }
fn main() {
    let y = 1;
    let mut r = &y;
    println!("{}", shared(&mut r));
    println!("{}", same(&mut r));
    let c = Cell::new(&y);
    let s = S { c: &y };
    println!("{} {} {} {}", r, cell(&c).get(), get(&c), covariant(&s).c);
}
main();
"#,
        Ok("1\n1\n1 1 1 1\n"),
    );
}

#[test]
fn a_borrow_returned_as_a_longer_one_is_refused() {
    signature_verdict(
        r#"
fn f<'a>(x: &'a &'a i32) -> &'a &'static i32 { x }
fn main() { let y = 1; let r = &y; println!("{}", f(&r)); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:48
  |
2 | fn f<'a>(x: &'a &'a i32) -> &'a &'static i32 { x }
  |      -- lifetime `'a` defined here             ^ returning this value requires that `'a` must outlive `'static`
"#),
    );
    // rustc's E0621 where the parameter's lifetime is elided.
    signature_verdict(
        r#"
fn f<'a>(x: & &'a i32) -> &'a &'a i32 { x }
fn main() { let y = 1; let r = &y; println!("{}", f(&r)); }
main();
"#,
        Err(r#"
error[E0621]: explicit lifetime required in the type of `x`
 --> <script>:2:41
  |
2 | fn f<'a>(x: & &'a i32) -> &'a &'a i32 { x }
  |                                         ^ lifetime `'a` required
  |
help: add explicit lifetime `'a` to the type of `x`
  |
2 | fn f<'a>(x: &'a &'a i32) -> &'a &'a i32 { x }
  |              ++
"#),
    );
    signature_verdict(
        r#"
fn f<'a>(x: &'a &'static i32) -> &'static i32 { *x }
fn g<'a>(x: &'a &'static i32) -> &'a &'a i32 { x }
fn h<'a: 'b, 'b>(x: Option<&'a i32>, y: &'b i32) -> Option<&'b i32> { if y > &0 { x } else { Some(y) } }
fn main() { let r: &'static i32 = &1; println!("{} {} {:?}", f(&r), g(&r), h(Some(&2), &3)); }
main();
"#,
        Ok("1 1 Some(2)\n"),
    );
}

#[test]
fn a_parameter_s_type_implies_its_lifetimes_outlive_each_other() {
    // rustc accepts each: `&'a &'b i32` and `&'a Holder<'b>` imply
    // `'b: 'a`.
    signature_verdict(
        r#"
struct Holder<'b> { x: &'b i32 }
fn inner<'a, 'b>(x: &'a &'b i32) -> &'a i32 { *x }
fn either<'a, 'b>(x: &'a &'b i32, y: &'b i32) -> &'a i32 { if *x > y { y } else { x } }
fn field<'a, 'b>(p: &'a Holder<'b>) -> &'a i32 { p.x }
fn main() {
    let (a, b) = (1, 2);
    let r = &a;
    let h = Holder { x: &b };
    println!("{} {} {}", inner(&r), either(&r, &b), field(&h));
}
main();
"#,
        Ok("1 1 2\n"),
    );
}

#[test]
fn a_reborrow_lasts_within_the_reference_it_goes_through() {
    // rustc: `&**x` is within `'a` alone, and `&y[0]` within `'b`.
    signature_verdict(
        r#"
fn f<'a>(x: &'a &'a i32) -> &'static i32 { &**x }
fn main() { let y = 1; let r = &y; println!("{}", f(&r)); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:44
  |
2 | fn f<'a>(x: &'a &'a i32) -> &'static i32 { &**x }
  |      -- lifetime `'a` defined here         ^^^^ returning this value requires that `'a` must outlive `'static`
"#),
    );
    signature_verdict(
        r#"
fn first<'a, 'b>(_x: &'a Vec<i32>, y: &'b Vec<i32>) -> &'a i32 { &y[0] }
fn main() { println!("{}", first(&vec![1], &vec![2])); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:66
  |
2 | fn first<'a, 'b>(_x: &'a Vec<i32>, y: &'b Vec<i32>) -> &'a i32 { &y[0] }
  |          --  -- lifetime `'b` defined here                       ^^^^^ function was supposed to return data with lifetime `'a` but it is returning data with lifetime `'b`
  |          |
  |          lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`
"#),
    );
    signature_verdict(
        r#"
struct S { name: String }
impl S { fn pick<'a>(&self, _o: &'a str) -> &'a str { self.name.as_str() } }
fn main() { let s = S { name: String::from("n") }; println!("{}", s.pick("x")); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:55
  |
3 | impl S { fn pick<'a>(&self, _o: &'a str) -> &'a str { self.name.as_str() } }
  |                  --  -                                ^^^^^^^^^^^^^^^^^^ method was supposed to return data with lifetime `'a` but it is returning data with lifetime `'1`
  |                  |   |
  |                  |   let's call the lifetime of this reference `'1`
  |                  lifetime `'a` defined here
  |
help: consider reusing a named lifetime parameter and update trait if needed
  |
3 | impl S { fn pick<'a>(&'a self, _o: &'a str) -> &'a str { self.name.as_str() } }
  |                       ++
"#),
    );
    // rustc accepts each: the reference reborrowed through outlives the
    // output.
    signature_verdict(
        r#"
struct P<'a> { a: &'a String, b: &'a i32 }
impl<'a> P<'a> {
    fn name(&self) -> &'a str { self.a.as_str() }
    fn number(&self) -> &'a i32 { &*self.b }
}
fn inner<'a, 'b>(x: &'a &'b i32) -> &'b i32 { &**x }
fn through<'a, 'b>(x: &'a &'b i32) -> &'b i32 { let y = x; &**y }
fn second<'a, 'b>(_x: &'a Vec<i32>, y: &'b Vec<i32>) -> &'b i32 { &y[0] }
fn member<'a, 'b>(x: &'a (&'b String, i32)) -> &'b str { x.0.as_str() }
fn main() {
    let (s, n) = (String::from("s"), 1);
    let p = P { a: &s, b: &n };
    let r = &n;
    let pair = (&s, 2);
    println!("{} {} {} {}", p.name(), p.number(), inner(&r), through(&r));
    println!("{} {}", second(&vec![1], &vec![2]), member(&pair));
}
main();
"#,
        Ok("s 1 1 1\n2 s\n"),
    );
}

#[test]
fn a_lifetime_error_names_the_lifetimes_and_variance_rustc_names() {
    // An elided lifetime is `'1`.
    signature_verdict(
        r#"fn f(x: &i32) -> &'static i32 { x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:33
  |
1 | fn f(x: &i32) -> &'static i32 { x }
  |         -                       ^ returning this value requires that `'1` must outlive `'static`
  |         |
  |         let's call the lifetime of this reference `'1`
"#),
    );
    // The receiver's elided lifetime is its `&`'s.
    signature_verdict(
        r#"struct S(i32);
impl S { fn h(&self) -> &'static i32 { &self.0 } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:40
  |
2 | impl S { fn h(&self) -> &'static i32 { &self.0 } }
  |               -                        ^^^^^^^ returning this value requires that `'1` must outlive `'static`
  |               |
  |               let's call the lifetime of this reference `'1`
"#),
    );
    // An associated function without a receiver.
    signature_verdict(
        r#"struct S;
impl S { fn g<'a, 'b>(x: &'a i32, _y: &'b i32) -> &'b i32 { x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:61
  |
2 | impl S { fn g<'a, 'b>(x: &'a i32, _y: &'b i32) -> &'b i32 { x } }
  |               --  -- lifetime `'b` defined here             ^ associated function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |               |
  |               lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    // `'static` where a `&mut` makes the position invariant.
    signature_verdict(
        r#"fn f<'a>(x: &'a mut &'static i32) -> &'a mut &'a i32 { x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:56
  |
1 | fn f<'a>(x: &'a mut &'static i32) -> &'a mut &'a i32 { x }
  |      -- lifetime `'a` defined here                     ^ returning this value requires that `'a` must outlive `'static`
  |
  = note: requirement occurs because of a mutable reference to `&i32`
  = note: mutable references are invariant over their type parameter
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    // A crate enum's lifetime.
    signature_verdict(
        r#"use std::cell::Cell;
enum E<'a> { C(Cell<&'a i32>) }
fn f<'a, 'b: 'a>(x: &'a E<'b>) -> &'a E<'a> { x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:47
  |
3 | fn f<'a, 'b: 'a>(x: &'a E<'b>) -> &'a E<'a> { x }
  |      --  -- lifetime `'b` defined here        ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of the type `E<'_>`, which makes the generic argument `'_` invariant
  = note: the enum `E<'a>` is invariant over the parameter `'a`
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    // A crate struct's type parameter.
    signature_verdict(
        r#"use std::cell::Cell;
struct W<T> { c: Cell<T> }
fn f<'a, 'b: 'a>(x: &'a W<&'b i32>) -> &'a W<&'a i32> { x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:57
  |
3 | fn f<'a, 'b: 'a>(x: &'a W<&'b i32>) -> &'a W<&'a i32> { x }
  |      --  -- lifetime `'b` defined here                  ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of the type `W<&i32>`, which makes the generic argument `&i32` invariant
  = note: the struct `W<T>` is invariant over the parameter `T`
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    // A lock rustc names by its path.
    signature_verdict(
        r#"use std::sync::Mutex;
fn f<'a, 'b: 'a>(x: &'a Mutex<&'b i32>) -> &'a Mutex<&'a i32> { x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:65
  |
2 | fn f<'a, 'b: 'a>(x: &'a Mutex<&'b i32>) -> &'a Mutex<&'a i32> { x }
  |      --  -- lifetime `'b` defined here                          ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of the type `std::sync::Mutex<&i32>`, which makes the generic argument `&i32` invariant
  = note: the struct `std::sync::Mutex<T>` is invariant over the parameter `T`
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    // E0621 for a `&mut` parameter, which rustc doesn't suggest a lifetime for.
    signature_verdict(
        r#"fn f<'a>(x: &mut &'a i32) -> &'a mut &'a i32 { x }
fn main() {}
main();
"#,
        Err(r#"
error[E0621]: explicit lifetime required in the type of `x`
 --> <script>:1:48
  |
1 | fn f<'a>(x: &mut &'a i32) -> &'a mut &'a i32 { x }
  |                                                ^ lifetime `'a` required
  |
  = help: see <https://doc.rust-lang.org/nomicon/borrow-splitting.html> for more information about lifetime errors related to mutable references
"#),
    );
}

/// rustc's verdict on `source`, whose functions are checked against their
/// own signatures (#170): `Err`'s report, or the program prints `Ok`'s.
fn signature_verdict(source: &str, verdict: Result<&str, &str>) {
    match verdict {
        Ok(output) => assert_eq!(printed(source), output),
        Err(report) => assert_eq!(refused(source).trim_end(), report.trim()),
    }
}

#[test]
fn a_program_impl_of_a_std_trait_has_its_provided_methods() {
    // rustc gives an impl of `io::Write`, `fmt::Write`, `io::Read` and
    // `PartialOrd` each provided method it doesn't override (#2461).
    let output = printed(
        r#"
use std::io::{Read, Write};
struct Buf { data: Vec<u8> }
impl Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let n = b.len().min(2);
        self.data.extend_from_slice(&b[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
struct Source { data: Vec<u8>, at: usize }
impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = (self.data.len() - self.at).min(buf.len()).min(2);
        buf[..n].copy_from_slice(&self.data[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}
fn main() {
    let mut b = Buf { data: Vec::new() };
    b.write_all(b"hi ").unwrap();
    write!(b, "{}-{}", 1, 2).unwrap();
    writeln!(b, "!").unwrap();
    print!("{}", String::from_utf8(b.data).unwrap());
    let mut r = Source { data: b"hello".to_vec(), at: 0 };
    let mut text = String::new();
    println!("{} {}", r.read_to_string(&mut text).unwrap(), text);
    let mut r = Source { data: b"abc".to_vec(), at: 0 };
    let (mut two, mut four) = ([0u8; 2], [0u8; 4]);
    r.read_exact(&mut two).unwrap();
    println!("{:?} {:?}", two, r.read_exact(&mut four).map_err(|e| e.kind()));
}
main();
"#,
    );
    assert_eq!(output, "hi 1-2!\n5 hello\n[97, 98] Err(UnexpectedEof)\n");
    let output = printed(
        r#"
use std::fmt::Write;
struct S { s: String }
impl Write for S {
    fn write_str(&mut self, t: &str) -> std::fmt::Result { self.s.push_str(t); Ok(()) }
}
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
struct V(i32);
#[derive(PartialEq, Debug)]
struct F(f64);
impl PartialOrd for F {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> { self.0.partial_cmp(&o.0) }
}
fn less<T: PartialOrd>(a: &T, b: &T) -> bool { a.lt(b) }
fn main() {
    let mut s = S { s: String::new() };
    write!(s, "x{}", 3).unwrap();
    s.write_char('y').unwrap();
    println!("{}", s.s);
    let (a, b) = (V(1), V(2));
    println!("{} {} {:?} {:?} {:?}", a.lt(&b), a.ge(&b), a.max(b), a.min(b), V(9).clamp(a, b));
    println!("{} {} {}", F(1.0).le(&F(1.0)), F(2.0).gt(&F(f64::NAN)), 3.lt(&4));
    println!("{} {} {}", less(&1, &2), less(&"b", &"a"), less(&a, &b));
}
main();
"#,
    );
    assert_eq!(
        output,
        "x3y\ntrue false V(2) V(1) V(2)\ntrue false true\ntrue false true\n"
    );
}

#[test]
fn an_impl_trait_output_is_known_by_its_bound() {
    // rustc runs a function written `-> impl Trait` of a program or std
    // trait (#2462), and a method call on its value is the bound's, never
    // the hidden type's inherent namesake (#2099).
    let output = printed(
        r#"
use std::fmt::{Debug, Display};
trait Shape { fn area(&self) -> f64; fn name(&self) -> String { String::from("shape") } }
trait Build { fn build(&self) -> String; }
trait Named { type Out; fn get(&self) -> Self::Out; }
struct Sq(f64);
struct Ci(f64);
impl Sq { fn build(&self, extra: i32) -> String { format!("inherent {extra}") } }
impl Build for Sq { fn build(&self) -> String { format!("trait {}", self.0) } }
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } fn name(&self) -> String { "square".into() } }
impl Shape for Ci { fn area(&self) -> f64 { 3.0 * self.0 * self.0 } }
impl Named for Sq { type Out = i64; fn get(&self) -> i64 { self.0 as i64 } }
fn make(k: f64) -> impl Shape { if k > 1.0 { Sq(k) } else { Sq(1.0) } }
fn builder() -> impl Build { Sq(1.0) }
fn show(s: impl Shape) -> String { format!("{} {}", s.name(), s.area()) }
fn twice(s: &impl Shape) -> f64 { s.area() * 2.0 }
fn total<T: Shape>(v: &[T]) -> f64 { v.iter().map(|s| s.area()).sum() }
fn shown() -> impl Display { 42 }
fn debugged() -> impl Debug { (1, "a") }
fn named() -> impl Named<Out = i64> { Sq(3.0) }
fn main() {
    let a = make(2.0);
    println!("{} {} {} {}", a.name(), a.area(), twice(&a), builder().build());
    println!("{} {}", show(make(3.0)), show(Ci(1.0)));
    let shapes: Vec<Box<dyn Shape>> = vec![Box::new(Sq(1.0)), Box::new(Ci(1.0))];
    println!("{}", shapes.iter().map(|s| s.name()).collect::<Vec<_>>().join(" "));
    println!("{}", total(&[make(1.0), make(3.0)]));
    println!("{} {:?} {}", shown(), debugged(), named().get() + 1);
}
main();
"#,
    );
    assert_eq!(
        output,
        "square 4 8 trait 1\nsquare 9 shape 3\nsquare shape\n10\n42 (1, \"a\") 4\n"
    );
}

#[test]
fn a_generic_body_reads_its_value_as_rustc_does() {
    // rustc clones the reference of `&T` for an unbounded `T` (#2474), and
    // a generic value's or a trait object's `{:?}` of a tuple writes a
    // tuple (#2473).
    let output = printed(
        r#"
use std::fmt::Debug;
fn d<U: Debug>(u: U) -> String { format!("{:?}", u) }
fn f<T>(t: T) -> usize { let r = &t; let _c = r.clone(); 1 }
fn main() {
    println!("{} {} {} {}", d(('x', 'x')), d((1, "a", 2.5)), d(vec![(1, 2)]), d((7,)));
    let boxed: Box<dyn Debug> = Box::new((u64::MAX, 'b'));
    println!("{:?} {}", boxed, f(String::new()));
}
main();
"#,
    );
    assert_eq!(
        output,
        "('x', 'x') (1, \"a\", 2.5) [(1, 2)] (7,)\n(18446744073709551615, 'b') 1\n"
    );
}

#[test]
fn an_impl_trait_output_is_its_functions_opaque_type() {
    // rustc's `-> impl Shape` is a type known by its bounds alone, so a
    // trait no `dyn` could name runs: a `-> Self` method returns the opaque
    // type, and a generic method takes any argument (#2480).
    let output = printed(
        r#"
trait Shape {
    fn area(&self) -> f64;
    fn scaled(&self, k: f64) -> Self;
    fn tag<T: std::fmt::Debug>(&self, t: T) -> String { format!("{:?}", t) }
}
#[derive(Debug, Clone)]
struct Sq(f64);
impl Shape for Sq {
    fn area(&self) -> f64 { self.0 * self.0 }
    fn scaled(&self, k: f64) -> Self { Sq(self.0 * k) }
}
fn make() -> impl Shape { Sq(2.0) }
fn twice(s: impl Shape) -> f64 { s.area() * 2.0 }
fn main() {
    let s = make();
    println!("{} {}", s.area(), s.tag((1, "x")));
    let b = s.scaled(3.0);
    println!("{}", b.area());
    println!("{}", twice(make()));
}
main();
"#,
    );
    assert_eq!(output, "4 (1, \"x\")\n36\n8\n");
}

#[test]
fn an_impl_trait_output_has_each_of_its_bounds() {
    // rustc's opaque type has every written bound, std's too: several
    // traits, `Clone`, `Copy`, `PartialEq`, `Default`, `Into<String>`, an
    // associated type, by-value `self`, and a trait with a static function
    // (#2480).
    let output = printed(
        r#"
trait Shape { fn area(&self) -> f64; }
#[derive(Debug, Clone, PartialEq)] struct Sq(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } }
fn make() -> impl Shape + std::fmt::Debug { Sq(2.0) }
fn cloned() -> impl Clone { Sq(2.0) }
fn equal() -> impl PartialEq { 3i64 }
fn text() -> impl Into<String> { "hi" }
fn sized() -> impl Sized { 3i64 }
fn sent() -> impl Send { 3i64 }
fn copied() -> impl Copy { 3i64 }
fn defaulted() -> impl Default { 3i64 }
fn main() {
    let s = make();
    println!("{} {:?}", s.area(), s);
    let _t = cloned().clone();
    let text: String = text().into();
    let z = copied();
    let (_w, _v) = (z, z);
    let (_x, _y, _d) = (sized(), sent(), defaulted());
    println!("{} {}", equal() == equal(), text);
}
main();
"#,
    );
    assert_eq!(output, "4 Sq(2.0)\ntrue hi\n");
    let output = printed(
        r#"
trait Bag { type Item: std::fmt::Debug; fn get(&self) -> Self::Item; }
struct B(i64);
impl Bag for B { type Item = i64; fn get(&self) -> i64 { self.0 } }
trait Eat { fn eat(self) -> i64; }
struct A(i64);
impl Eat for A { fn eat(self) -> i64 { self.0 } }
trait Zero { fn zero() -> Self; fn val(&self) -> i64; }
struct Z(i64);
impl Zero for Z { fn zero() -> Self { Z(0) } fn val(&self) -> i64 { self.0 } }
fn bag() -> impl Bag { B(3) }
fn food() -> impl Eat { A(4) }
fn zero() -> impl Zero { Z(5) }
fn pair() -> impl std::fmt::Debug { (1, "x") }
fn items() -> impl Iterator<Item = impl std::fmt::Display> { vec![1, 2].into_iter() }
fn main() {
    let x = bag().get();
    let a = zero();
    let z = Z::zero();
    println!("{:?} {} {} {}", x, food().eat(), a.val(), z.val());
    println!("{:?}", pair());
    for x in items() { println!("{}", x); }
}
main();
"#,
    );
    assert_eq!(output, "3 4 5 0\n(1, \"x\")\n1\n2\n");
    // A generic function's, an inherent method's, a nested function's and a
    // trait method's opaque output.
    let output = printed(
        r#"
trait Shape { fn area(&self) -> f64; }
struct Sq(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } }
impl Sq { fn grown(&self) -> impl Shape { Sq(self.0 * 2.0) } }
trait Grow { fn grow(&self) -> impl Shape; }
impl Grow for Sq { fn grow(&self) -> impl Shape { Sq(self.0 + 1.0) } }
fn wrap<T: Shape>(t: T) -> impl Shape { t }
fn main() {
    fn inner() -> impl Shape { Sq(5.0) }
    let s = Sq(1.0);
    println!("{} {} {} {}", wrap(Sq(3.0)).area(), s.grown().area(), s.grow().area(), inner().area());
}
main();
"#,
    );
    assert_eq!(output, "9 4 4 25\n");
}

#[test]
fn an_impl_trait_output_is_known_only_by_its_bounds() {
    // rustc fills an opaque type with one hidden type, which callers never
    // see, and each function's opaque type is its own (#2480).
    let program = |body: &str| {
        format!(
            r#"
trait Shape {{ fn area(&self) -> f64; }}
#[derive(Debug, Clone, PartialEq)] struct Sq(f64);
#[derive(Debug, Clone, PartialEq)] struct Ci(f64);
impl Shape for Sq {{ fn area(&self) -> f64 {{ self.0 * self.0 }} }}
impl Shape for Ci {{ fn area(&self) -> f64 {{ 3.0 * self.0 * self.0 }} }}
impl Sq {{ fn side(&self) -> f64 {{ self.0 }} }}
{body}
main();
"#
        )
    };
    let report = refused(&program(
        "fn make(b: bool) -> impl Shape { if b { return Sq(1.0); } Ci(2.0) }\n\
         fn main() { println!(\"{}\", make(true).area()); }",
    ));
    assert!(
        report.starts_with("error[E0308]: mismatched types"),
        "{report}"
    );
    assert!(report.contains("expected `Sq`, found `Ci`"), "{report}");
    let report = refused(&program(
        "fn make() -> impl Shape { Sq(2.0) }\n\
         fn main() { let s = make(); println!(\"{}\", s.side()); }",
    ));
    assert!(
        report
            .starts_with("error[E0599]: no method named `side` found for opaque type `impl Shape`"),
        "{report}"
    );
    let report = refused(&program(
        "fn make() -> impl Shape { Sq(2.0) }\n\
         fn main() { let s: Sq = make(); println!(\"{}\", s.area()); }",
    ));
    assert!(
        report.starts_with("error[E0308]: mismatched types"),
        "{report}"
    );
    assert!(
        report.contains("expected `Sq`, found opaque type"),
        "{report}"
    );
    let report = refused(&program(
        "fn m1() -> impl Shape { Sq(2.0) }\n\
         fn m2() -> impl Shape { Ci(2.0) }\n\
         fn main() { let mut x = m1(); x = m2(); println!(\"{}\", x.area()); }",
    ));
    assert!(
        report.starts_with("error[E0308]: mismatched types"),
        "{report}"
    );
}

#[test]
fn a_generic_filled_by_an_opaque_output_dispatches_on_its_hidden_type() {
    // rustc dispatches `T::zero()` on the hidden type of the opaque type
    // that fills `T`, which the caller still knows only by its bounds (#2491).
    let output = printed(
        r#"
trait Zero { fn zero() -> Self; fn val(&self) -> i64; }
struct Z(i64);
impl Zero for Z { fn zero() -> Self { Z(0) } fn val(&self) -> i64 { self.0 } }
fn make() -> impl Zero { Z(5) }
fn same<T: Zero>(_: &T) -> T { T::zero() }
fn main() {
    let a = make();
    let b = same(&a);
    println!("{} {}", a.val(), b.val());
}
main();
"#,
    );
    assert_eq!(output, "5 0\n");
    // A function written after its caller, and a builtin hidden type.
    let output = printed(
        r#"
fn main() {
    let n = number();
    let z = zeroed(&n);
    let b = same(&make());
    println!("{} {} {}", n, z, b.val());
}
trait Zero { fn zero() -> Self; fn val(&self) -> i64; }
#[derive(Default)]
struct Z(i64);
impl Zero for Z { fn zero() -> Self { Z(1) } fn val(&self) -> i64 { self.0 } }
fn zeroed<T: Default>(_: &T) -> T { T::default() }
fn same<T: Zero>(_: &T) -> T { T::zero() }
fn number() -> impl Default + std::fmt::Display { 7 }
fn make() -> impl Zero { Z(5) }
main();
"#,
    );
    assert_eq!(output, "7 0 1\n");
}

#[test]
fn an_opaque_output_is_send_as_its_hidden_type_is() {
    // rustc's auto traits see through an opaque type to its hidden type.
    let program = |field: &str, value: &str| {
        format!(
            r#"
use std::rc::Rc;
trait Shape {{ fn area(&self) -> f64; }}
struct Sq({field});
impl Shape for Sq {{ fn area(&self) -> f64 {{ 2.0 }} }}
fn make() -> impl Shape {{ Sq({value}) }}
fn main() {{
    let s = make();
    let h = std::thread::spawn(move || s.area());
    println!("{{}}", h.join().unwrap());
}}
main();
"#
        )
    };
    assert_eq!(printed(&program("f64", "2.0")), "2\n");
    // rawr names the hidden type, as it names a captured `Sq` itself.
    let report = refused(&program("Rc<f64>", "Rc::new(2.0)"));
    assert!(
        report.starts_with("error[E0277]: `Sq` cannot be sent between threads safely"),
        "{report}"
    );
    assert!(
        report.contains("required by a bound in `std::thread::spawn`"),
        "{report}"
    );
}

#[test]
fn a_std_path_through_another_module_s_import_is_std_s() {
    // rustc resolves `super::fmt::Display` through the parent's `use
    // std::fmt;`, and `super::super::HashMap` through the root's glob, to
    // std's items, in a type, a trait, a value and a pattern (#2468).
    let output = printed(
        r#"
use std::fmt;
use std::cmp::Ordering;
use std::collections::*;
struct X;
mod m {
    pub struct Y;
    impl super::fmt::Display for Y {
        fn fmt(&self, f: &mut super::fmt::Formatter) -> super::fmt::Result { write!(f, "Y") }
    }
    pub mod n {
        pub struct Z;
        impl super::super::fmt::Debug for Z {
            fn fmt(&self, f: &mut super::super::fmt::Formatter) -> crate::fmt::Result { write!(f, "Z") }
        }
        pub fn count() -> usize {
            let mut h: super::super::HashMap<i32, i32> = super::super::HashMap::new();
            h.insert(1, crate::cmp::max(1, 2));
            h.len()
        }
    }
    pub fn rank(o: super::Ordering) -> i32 {
        match o { super::Ordering::Less => 1, crate::Ordering::Equal => 2, _ => 3 }
    }
}
use std::cmp;
impl crate::fmt::Display for X {
    fn fmt(&self, f: &mut self::fmt::Formatter<'_>) -> fmt::Result { write!(f, "X") }
}
fn main() {
    println!("{} {} {:?} {} {}", X, m::Y, m::n::Z, m::n::count(), m::rank(Ordering::Equal));
}
main();
"#,
    );
    assert_eq!(output, "X Y Z 1 2\n");
    // A std module std has no item of the name in.
    let program = |item: &str| {
        format!(
            r#"
use std::fmt;
mod m {{
    pub struct X;
    {item}
}}
fn main() {{}}
main();
"#
        )
    };
    let report = refused(&program(
        "impl super::fmt::Displayy for X { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) } }",
    ));
    assert!(
        report.starts_with("error[E0405]: cannot find trait `Displayy` in module `super::fmt`"),
        "{report}"
    );
    let report = refused(&program("pub fn f() -> i32 { super::fmt::nothing() }"));
    assert!(
        report.starts_with("error[E0425]: cannot find function `nothing` in module `super::fmt`"),
        "{report}"
    );
}

#[test]
fn a_pub_use_of_a_std_item_or_a_module_re_exports_it() {
    // rustc re-exports a std trait, type, function or module, and a module
    // of the crate, by `pub use` (#2469).
    let output = printed(
        r#"
mod m {
    pub use std::fmt::Display as Show;
    pub use std::fmt;
    pub use std::collections::HashMap;
    pub use std::cmp::max;
}
mod a {
    pub mod b {
        pub struct S(pub i32);
        pub trait T { fn t(&self) -> i32; }
        impl T for S { fn t(&self) -> i32 { self.0 } }
        pub enum E { One, Two }
    }
}
mod r {
    pub use crate::a::b;
    pub use crate::a::b as c;
}
mod n {
    use crate::m::Show;
    pub struct X;
    impl Show for X {
        fn fmt(&self, f: &mut crate::m::fmt::Formatter) -> super::m::fmt::Result { write!(f, "X") }
    }
    pub fn boxed() -> Box<dyn super::m::Show> { Box::new(X) }
}
use r::c::T;
fn rank(s: &r::b::S, e: r::c::E) -> i32 {
    let k = match e { r::b::E::One => 1, crate::r::c::E::Two => 2 };
    s.t() + k
}
fn main() {
    let mut h: m::HashMap<i32, i32> = m::HashMap::new();
    h.insert(1, m::max(3, 4));
    let s = r::c::S(5);
    println!("{} {} {:?} {} {}", n::X, n::boxed(), h.get(&1), rank(&s, r::b::E::Two), s.t());
}
main();
"#,
    );
    assert_eq!(output, "X X Some(4) 7 5\n");
    // A private `use` re-exports nothing: rustc names the std item's kind.
    let report = refused(
        r#"
mod m {
    use std::fmt::Display;
}
struct X;
impl m::Display for X {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, "X") }
}
fn main() {}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0603]: trait import `Display` is private"),
        "{report}"
    );
    let report = refused(
        r#"
mod m {
    use std::collections::HashMap;
}
fn main() {
    let h: m::HashMap<i32, i32> = m::HashMap::new();
    println!("{}", h.len());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0603]: struct import `HashMap` is private"),
        "{report}"
    );
}

#[test]
fn an_opaque_output_is_one_type_per_type_argument() {
    // rustc's opaque type captures its function's type parameters, so
    // `wrap::<Sq>` and `wrap::<Ci>` return two types, and a bound naming a
    // parameter is read at the call's argument (#2492).
    let report = refused(
        r#"
trait Shape { fn area(&self) -> f64; }
struct Sq(f64);
struct Ci(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } }
impl Shape for Ci { fn area(&self) -> f64 { 3.0 * self.0 * self.0 } }
fn wrap<T: Shape>(t: T) -> impl Shape { t }
fn main() {
    let mut x = wrap(Sq(1.0));
    x = wrap(Ci(2.0));
    println!("{}", x.area());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0308]: mismatched types"),
        "{report}"
    );
    assert!(report.contains("expected `Sq`, found `Ci`"), "{report}");
    let output = printed(
        r#"
fn conv<T: Into<String>>(t: T) -> impl Into<T> { t }
fn items<T: Clone>(t: T) -> impl Iterator<Item = T> { vec![t.clone(), t].into_iter() }
fn main() {
    let s: &str = conv("hi").into();
    let v = items(3).collect::<Vec<i64>>();
    println!("{} {:?}", s, v);
    let mut x = items(1i64);
    x = items(2i64);
    println!("{:?}", x.collect::<Vec<_>>());
}
main();
"#,
    );
    assert_eq!(output, "hi [3, 3]\n[2, 2]\n");
}

#[test]
fn a_generic_opaque_outputs_hidden_type_is_its_calls_type_argument() {
    // rustc's hidden type of `wrap::<Z>`'s opaque type is `Z`, so a generic
    // call taking it dispatches on `Z`, and it is `Send` as `Z` is (#2492).
    let output = printed(
        r#"
trait Zero { fn zero() -> Self; fn val(&self) -> i64; }
struct Z(i64);
impl Zero for Z { fn zero() -> Self { Z(0) } fn val(&self) -> i64 { self.0 } }
fn wrap<T: Zero>(t: T) -> impl Zero { t }
fn same<T: Zero>(_: &T) -> T { T::zero() }
fn zeroed<T: Default>(_: &T) -> T { T::default() }
fn num<T: Default + std::fmt::Display>(t: T) -> impl Default + std::fmt::Display { t }
fn main() {
    let a = wrap(Z(5));
    let b = same(&a);
    let n = num(7);
    let z = zeroed(&n);
    println!("{} {} {} {}", a.val(), b.val(), n, z);
}
main();
"#,
    );
    assert_eq!(output, "5 0 7 0\n");
    let report = refused(
        r#"
use std::rc::Rc;
trait Shape { fn area(&self) -> f64; }
struct Sq(Rc<f64>);
impl Shape for Sq { fn area(&self) -> f64 { 2.0 } }
fn wrap<T: Shape>(t: T) -> impl Shape { t }
fn main() {
    let s = wrap(Sq(Rc::new(2.0)));
    let h = std::thread::spawn(move || s.area());
    println!("{}", h.join().unwrap());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Sq` cannot be sent between threads safely"),
        "{report}"
    );
}

#[test]
fn a_generic_impls_opaque_output_is_read_at_its_receivers_type_arguments() {
    // rustc's opaque type of a method captures its impl's type parameters
    // too, so `W<i64>`'s `items` gives `i64`s (#2492).
    let output = printed(
        r#"
struct W<T>(Vec<T>);
impl<T: Clone> W<T> {
    fn items(&self) -> impl Iterator<Item = T> + '_ { self.0.iter().cloned() }
    fn first(&self) -> impl Into<Option<T>> { self.0.first().cloned() }
}
fn main() {
    let w = W(vec![1i64, 2]);
    let total: i64 = w.items().sum();
    let first: Option<i64> = w.first().into();
    let names = W(vec![String::from("a")]);
    let joined: Vec<String> = names.items().collect();
    println!("{} {:?} {:?}", total, first, joined);
}
main();
"#,
    );
    assert_eq!(output, "3 Some(1) [\"a\"]\n");
}

#[test]
fn a_std_from_into_a_float_widens_an_integer() {
    // std's `From<u8> for f64` and the other integers a float holds
    // exactly, through a generic's bound and concretely (#2475).
    let output = printed(
        r#"
fn widened<T: From<u8>>(b: u8) -> T { T::from(b) }
fn into_float<T: Into<f64>>(t: T) -> f64 { t.into() }
fn main() {
    let x: f64 = widened::<f64>(3);
    let y: f64 = 4u8.into();
    let z = f32::from(5u16);
    let w: f64 = (-7i32).into();
    println!("{} {} {} {} {}", x, y, z, w, into_float(9u32) / 2.0);
}
main();
"#,
    );
    assert_eq!(output, "3 4 5 -7 4.5\n");
    let report = refused("fn main() { let x = f64::from(5u64); println!(\"{}\", x); }\nmain();");
    assert!(
        report.contains("the trait bound `f64: From<u64>` is not satisfied"),
        "{report}"
    );
}

#[test]
fn a_program_hasher_has_the_provided_writes() {
    // rustc gives an impl of `Hasher` `write_u8` through `write_isize`,
    // each writing the integer's native-endian bytes (#2481).
    let output = printed(
        r#"
use std::hash::Hasher;
struct Sum(u64, usize);
impl Hasher for Sum {
    fn finish(&self) -> u64 { self.0 }
    fn write(&mut self, bytes: &[u8]) {
        self.1 += 1;
        for (i, b) in bytes.iter().enumerate() { self.0 += (i as u64 + 1) * *b as u64; }
    }
}
fn feed<H: Hasher>(h: &mut H) -> u64 {
    h.write_u16(0x0102);
    h.write_i64(-2);
    h.finish()
}
fn main() {
    let mut h = Sum(0, 0);
    h.write_u8(3);
    h.write_u32(0x0102);
    h.write_i8(-1);
    h.write_usize(1);
    h.write_u128(1);
    h.write_i32(-1);
    println!("{} {}", h.finish(), h.1);
    let mut g = Sum(0, 0);
    println!("{} {}", feed(&mut g), g.1);
}
main();
"#,
    );
    assert_eq!(output, "2814 6\n9183 2\n");
}

#[test]
fn by_ref_of_a_program_iterator_goes_on_where_it_stopped() {
    // `by_ref` lends the iterator, and std's `impl Iterator for &mut I`
    // advances it in place (#2482).
    let output = printed(
        r#"
struct C(i64);
impl Iterator for C {
    type Item = i64;
    fn next(&mut self) -> Option<i64> { self.0 += 1; if self.0 < 8 { Some(self.0) } else { None } }
}
fn main() {
    let mut c = C(0);
    for x in c.by_ref() {
        if x == 2 { break; }
    }
    let s: i64 = c.by_ref().take(2).sum();
    let n = c.by_ref().next();
    let mut rest = Vec::new();
    let r = &mut c; for x in r { rest.push(x); }
    println!("{} {:?} {:?} {}", s, n, rest, c.0);
}
main();
"#,
    );
    assert_eq!(output, "7 Some(5) [6, 7] 8\n");
    let report = refused(
        r#"
struct C(i64);
impl Iterator for C {
    type Item = i64;
    fn next(&mut self) -> Option<i64> { self.0 += 1; if self.0 < 8 { Some(self.0) } else { None } }
}
fn main() {
    let c = C(0);
    let v: Vec<i64> = c.by_ref().take(2).collect();
    println!("{:?}", v);
}
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0596]: cannot borrow `c` as mutable, as it is not declared as mutable"
        ),
        "{report}"
    );
}

#[test]
fn a_generic_over_a_std_writer_runs_stds_impl() {
    // std's `impl fmt::Write for String` and `impl io::Write for Vec<u8>`
    // are what a generic bounded by the trait runs, and `write!` is the
    // writer's `write_fmt` (#2483).
    let output = printed(
        r#"
use std::fmt::Write;
fn put<W: Write>(w: &mut W, n: i32) -> std::fmt::Result {
    w.write_str("ab")?;
    w.write_char('c')?;
    w.write_fmt(format_args!("<{}>", n))?;
    write!(w, "[{}]", n)?;
    writeln!(w)
}
fn main() {
    let mut s = String::new();
    put(&mut s, 1).unwrap();
    s.write_str("d").unwrap();
    s.write_char('e').unwrap();
    writeln!(s, "f{}", 2).unwrap();
    print!("{}", s);
}
main();
"#,
    );
    assert_eq!(output, "abc<1>[1]\ndef2\n");
    let output = printed(
        r#"
use std::io::Write;
fn put<W: Write>(w: &mut W) -> std::io::Result<()> {
    w.write_all(b"hi")?;
    writeln!(w, " {}", 3)?;
    w.flush()
}
fn main() {
    let mut v: Vec<u8> = Vec::new();
    put(&mut v).unwrap();
    let n = v.write(b"!").unwrap();
    v.write_all(b"?").unwrap();
    write!(v, "{}", 4).unwrap();
    println!("{} {:?}", n, v);
}
main();
"#,
    );
    assert_eq!(output, "1 [104, 105, 32, 51, 10, 33, 63, 52]\n");
}

#[test]
fn fmt_arguments_is_a_written_type() {
    // `fmt::Arguments` names what `format_args!` makes, so `write_fmt` is
    // called and overridden as rustc has it (#2484).
    let output = printed(
        r#"
use std::fmt::{self, Arguments, Write};
struct Up(String);
impl Write for Up {
    fn write_str(&mut self, s: &str) -> fmt::Result { self.0.push_str(&s.to_uppercase()); Ok(()) }
}
struct Tagged(String);
impl Write for Tagged {
    fn write_str(&mut self, s: &str) -> fmt::Result { self.0.push_str(s); Ok(()) }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        self.0.push('#');
        self.write_str(&args.to_string())
    }
}
fn show(args: Arguments<'_>) -> String { format!("[{}]", args) }
fn main() {
    let mut u = Up(String::new());
    u.write_fmt(format_args!("a{}", 1)).unwrap();
    write!(u, "b{}", 2).unwrap();
    let args: fmt::Arguments = format_args!("c");
    u.write_fmt(args).unwrap();
    let mut t = Tagged(String::new());
    write!(t, "x{}", 1).unwrap();
    t.write_fmt(format_args!("y")).unwrap();
    println!("{} {} {}", u.0, t.0, show(format_args!("z{}", 3)));
}
main();
"#,
    );
    assert_eq!(output, "A1B2C #x1#y [z3]\n");
}

#[test]
fn an_iterators_field_wins_over_its_method_of_the_name() {
    // rustc reads `x.last` with no `(` as the field, through a `&mut` too,
    // and `x.last()` as `Iterator::last`.
    let output = printed(
        r#"
struct Lines { next: i64, last: i64, count: i64 }
impl Iterator for Lines {
    type Item = i64;
    fn next(&mut self) -> Option<i64> {
        if self.next > self.last {
            return None;
        }
        self.count += 1;
        self.next += 1;
        Some(self.next - 1)
    }
}
fn main() {
    let mut l = Lines { next: 1, last: 4, count: 0 };
    let first = l.next();
    let fields = (l.next, l.last, l.count);
    let r = &mut l;
    let through = (r.next, r.last, r.count);
    let last = Lines { next: 1, last: 3, count: 0 }.last();
    let count = Lines { next: 2, last: 5, count: 0 }.count();
    println!("{:?} {:?} {:?} {:?} {}", first, fields, through, last, count);
}
main();
"#,
    );
    assert_eq!(output, "Some(1) (2, 4, 1) (2, 4, 1) Some(3) 4\n");
}

#[test]
fn a_mut_reborrow_lasts_within_every_reference_it_goes_through() {
    // rustc: `&mut **x` is within `'a`, and `self.a` within the `&mut
    // self` it goes through (#2476).
    signature_verdict(
        r#"
fn f<'a, 'b>(x: &'a mut &'b mut i32) -> &'b mut i32 { &mut **x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:55
  |
2 | fn f<'a, 'b>(x: &'a mut &'b mut i32) -> &'b mut i32 { &mut **x }
  |      --  -- lifetime `'b` defined here                ^^^^^^^^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    signature_verdict(
        r#"
struct S<'a> { a: &'a mut i32 }
impl<'a> S<'a> { fn get(&mut self) -> &'a mut i32 { self.a } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:53
  |
3 | impl<'a> S<'a> { fn get(&mut self) -> &'a mut i32 { self.a } }
  |      --                 -                           ^^^^^^ method was supposed to return data with lifetime `'a` but it is returning data with lifetime `'1`
  |      |                  |
  |      |                  let's call the lifetime of this reference `'1`
  |      lifetime `'a` defined here
"#),
    );
    // A `&mut self` moved out of is moved.
    signature_verdict(
        r#"
struct S { a: i32 }
impl S {
    fn get(&mut self) -> &mut i32 {
        let t = self;
        t.a += 1;
        &mut self.a
    }
}
fn main() {}
main();
"#,
        Err(r#"
error[E0382]: borrow of moved value: `self`
 --> <script>:7:9
  |
4 |     fn get(&mut self) -> &mut i32 {
  |            --------- move occurs because `self` has type `&mut S`, which does not implement the `Copy` trait
5 |         let t = self;
  |                 ---- value moved here
6 |         t.a += 1;
7 |         &mut self.a
  |         ^^^^^^^^^^^ value borrowed here after move
"#),
    );
    // So is one given to a generic parameter, which rustc doesn't reborrow.
    signature_verdict(
        r#"
struct X(u32);
impl X {
    fn f(&mut self) {
        generic(self);
        self.0 += 1;
    }
}
fn generic<T>(_x: T) {}
fn main() {}
main();
"#,
        Err(r#"
error[E0382]: use of moved value: `self`
 --> <script>:6:9
  |
4 |     fn f(&mut self) {
  |          --------- move occurs because `self` has type `&mut X`, which does not implement the `Copy` trait
5 |         generic(self);
  |                 ---- value moved here
6 |         self.0 += 1;
  |         ^^^^^^^^^^^ value used here after move
  |
help: consider creating a fresh reborrow of `self` here
  |
5 |         generic(&mut *self);
  |                 ++++++
"#),
    );
    // So is any `&mut` a `let` takes without its type written.
    signature_verdict(
        r#"
fn f(r: &mut i32) {
    let t = r;
    *t += 1;
    *r += 1;
}
fn main() {}
main();
"#,
        Err(r#"
error[E0382]: use of moved value: `r`
 --> <script>:5:5
  |
2 | fn f(r: &mut i32) {
  |      - move occurs because `r` has type `&mut i32`, which does not implement the `Copy` trait
3 |     let t = r;
  |             - value moved here
4 |     *t += 1;
5 |     *r += 1;
  |     ^^^^^^^ value used here after move
"#),
    );
    // rustc accepts each: the reborrow is within the output's lifetime.
    signature_verdict(
        r#"
struct S<'a> { a: &'a mut i32 }
impl<'a> S<'a> {
    fn get(&mut self) -> &mut i32 { self.a }
    fn whole(&'a mut self) -> &'a mut i32 { self.a }
}
fn inner<'a, 'b>(x: &'a mut &'b mut i32) -> &'a mut i32 { &mut **x }
fn take(s: &mut i32) -> &mut i32 { let t = s; t }
fn main() {
    let mut m = 5;
    let mut r = &mut m;
    *inner(&mut r) += 1;
    *take(&mut m) += 1;
    println!("{}", m);
}
main();
"#,
        Ok("7\n"),
    );
}

#[test]
fn a_range_slice_lasts_within_the_str_it_slices() {
    // rustc: `&x[1..]` of a `&'a str` is within `'a` (#2477).
    signature_verdict(
        r#"
fn f<'a, 'b>(x: &'a str, _y: &'b str) -> &'b str { &x[1..] }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:52
  |
2 | fn f<'a, 'b>(x: &'a str, _y: &'b str) -> &'b str { &x[1..] }
  |      --  -- lifetime `'b` defined here             ^^^^^^^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    // So is one of a `&'a [i32]`.
    signature_verdict(
        r#"
fn f<'a, 'b>(x: &'a [i32], _y: &'b [i32]) -> &'b [i32] { &x[1..] }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:58
  |
2 | fn f<'a, 'b>(x: &'a [i32], _y: &'b [i32]) -> &'b [i32] { &x[1..] }
  |      --  -- lifetime `'b` defined here                   ^^^^^^^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
}

#[test]
fn a_borrow_stored_through_a_mut_parameter_outlives_what_it_refers_to() {
    // rustc: what a `&mut` parameter refers to keeps the borrows stored in
    // it for its own lifetimes (#2478).
    signature_verdict(
        r#"
fn f<'a, 'b>(v: &mut Vec<&'a i32>, x: &'b i32) { v.push(x); }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:50
  |
2 | fn f<'a, 'b>(v: &mut Vec<&'a i32>, x: &'b i32) { v.push(x); }
  |      --  -- lifetime `'b` defined here           ^^^^^^^^^ argument requires that `'b` must outlive `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`
  = note: requirement occurs because of a mutable reference to `Vec<&i32>`
  = note: mutable references are invariant over their type parameter
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
"#),
    );
    signature_verdict(
        r#"
fn f<'a, 'b>(r: &mut &'a i32, x: &'b i32) { *r = x; }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:45
  |
2 | fn f<'a, 'b>(r: &mut &'a i32, x: &'b i32) { *r = x; }
  |      --  -- lifetime `'b` defined here      ^^^^^^ assignment requires that `'b` must outlive `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`
"#),
    );
    signature_verdict(
        r#"
struct H<'a> { x: &'a i32 }
fn f<'a, 'b>(h: &mut H<'a>, x: &'b i32) { h.x = x; }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:43
  |
3 | fn f<'a, 'b>(h: &mut H<'a>, x: &'b i32) { h.x = x; }
  |      --  -- lifetime `'b` defined here    ^^^^^^^ assignment requires that `'b` must outlive `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`
"#),
    );
    // rustc runs each: the stored borrow outlives the lifetime it is
    // stored at.
    signature_verdict(
        r#"
struct H<'a> { x: &'a i32 }
fn push<'a, 'b: 'a>(v: &mut Vec<&'a i32>, x: &'b i32) { v.push(x); }
fn set<'a, 'b: 'a>(r: &mut &'a i32, x: &'b i32) { *r = x; }
fn field<'a>(h: &mut H<'a>, x: &'a i32) { h.x = x; }
fn slice<'a, 'b>(x: &'a str, _y: &'b str) -> &'a str { &x[1..] }
fn main() {
    let (a, b, c) = (1, 2, 3);
    let mut v = vec![&a];
    push(&mut v, &b);
    let mut r = &a;
    set(&mut r, &c);
    let mut h = H { x: &a };
    field(&mut h, &b);
    println!("{:?} {} {} {}", v, r, h.x, slice("hey", "x"));
}
main();
"#,
        Ok("[1, 2] 3 2 ey\n"),
    );
}

#[test]
fn a_fn_field_is_contravariant_and_a_struct_implies_its_bounds() {
    // rustc: `F<'a>` of a `fn(&'a i32)` field is contravariant in `'a`,
    // and `S<'a, 'b>` of a `&'a &'b i32` field implies `'b: 'a` (#2479).
    signature_verdict(
        r#"
struct S<'a> { f: fn(&'a i32) }
fn f<'a, 'b>(x: S<'a>, _y: &'b i32) -> S<'b> { x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:3:48
  |
3 | fn f<'a, 'b>(x: S<'a>, _y: &'b i32) -> S<'b> { x }
  |      --  -- lifetime `'b` defined here         ^ function was supposed to return data with lifetime `'a` but it is returning data with lifetime `'b`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`
"#),
    );
    signature_verdict(
        r#"
struct F<'a> { f: fn(&'a i32) -> i32 }
struct S<'a, 'b> { x: &'a &'b i32 }
fn shorten<'a, 'b: 'a>(x: F<'a>, _y: &'b i32) -> F<'b> { x }
fn implied<'a, 'b>(s: S<'a, 'b>) -> &'a i32 { *s.x }
fn one(x: &i32) -> i32 { *x + 1 }
fn main() {
    let n = 1;
    let f = shorten(F { f: one }, &n);
    let r = &n;
    println!("{} {}", (f.f)(&n), implied(S { x: &r }));
}
main();
"#,
        Ok("2 1\n"),
    );
}

#[test]
fn a_where_clause_of_an_impl_struct_or_trait_bounds_another_type() {
    // rustc reads `where Vec<T>: Debug` on an impl, a struct and a trait as
    // on a function: the body assumes it, and a use proves it (#2470).
    let output = printed(
        r#"
use std::fmt::Debug;
struct S<T>(T);
impl<T> S<T> where Vec<T>: Debug {
    fn show(&self, v: Vec<T>) { println!("{:?}", v); }
}
struct P<T>(T) where Vec<T>: Debug;
trait Show<T> where Vec<T>: Debug {
    fn show(&self, v: Vec<T>) { println!("{:?}", v); }
}
struct A;
impl Show<i32> for A {}
fn main() {
    S(1).show(vec![1, 2]);
    let p = P(3);
    println!("{}", p.0);
    A.show(vec![4]);
}
main();
"#,
    );
    assert_eq!(output, "[1, 2]\n3\n[4]\n");
    // Building the struct proves its clause.
    let report = refused(
        r#"
use std::fmt::Debug;
struct Opaque;
struct P<T>(T) where Vec<T>: Debug;
fn main() { let p = P(Opaque); let _ = p; }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Opaque` doesn't implement `Debug`"),
        "{report}"
    );
    assert!(
        report.contains("  = note: required for `Vec<Opaque>` to implement `Debug`\n"),
        "{report}"
    );
}

#[test]
fn a_bound_met_through_a_conditional_impl_proves_the_impls_own_bounds() {
    // rustc proves `Wrapper<Opaque>: Summary` through `impl<T: Debug>
    // Summary for Wrapper<T>`, whose `Opaque: Debug` fails (#2471).
    let program = |derive: &str| {
        format!(
            r#"
use std::fmt::Debug;
{derive}
struct Opaque;
struct Wrapper<T>(T);
trait Summary {{ fn s(&self) -> String; }}
impl<T: Debug> Summary for Wrapper<T> {{ fn s(&self) -> String {{ format!("w({{:?}})", self.0) }} }}
fn sum<W: Summary>(w: W) {{ println!("{{}}", w.s()); }}
fn main() {{ sum(Wrapper(Opaque)); }}
main();
"#
        )
    };
    assert_eq!(printed(&program("#[derive(Debug)]")), "w(Opaque)\n");
    let report = refused(&program(""));
    assert!(
        report.starts_with(
            r#"error[E0277]: `Opaque` doesn't implement `Debug`
 --> <script>:9:25
  |
9 | fn main() { sum(Wrapper(Opaque)); }
  |             ---         ^^^^^^ the trait `Debug` is not implemented for `Opaque`
  |             |
  |             required by a bound introduced by this call
  |
  = note: add `#[derive(Debug)]` to `Opaque` or manually `impl Debug for Opaque`
help: the trait `Summary` is conditionally implemented for `Wrapper<T>`
 --> <script>:7:1
  |
7 | impl<T: Debug> Summary for Wrapper<T> { fn s(&self) -> String { format!("w({:?})", self.0) } }
  | ^^^^^^^^-----^^^^^^^^^^^^^^^^^^^^^^^^
  |         |
  |         unsatisfied requirement introduced here: `Opaque: Debug`
note: required for `Wrapper<Opaque>` to implement `Summary`
 --> <script>:7:16
  |
7 | impl<T: Debug> Summary for Wrapper<T> { fn s(&self) -> String { format!("w({:?})", self.0) } }
  |         -----  ^^^^^^^     ^^^^^^^^^^
  |         |
  |         unsatisfied trait bound introduced here
note: required by a bound in `sum`
 --> <script>:8:11
  |
8 | fn sum<W: Summary>(w: W) { println!("{}", w.s()); }
  |           ^^^^^^^ required by this bound in `sum`
help: consider annotating `Opaque` with `#[derive(Debug)]`
"#
        ),
        "{report}"
    );
    // A `where` clause on another type: the innermost type it needs.
    let report = refused(
        r#"
use std::fmt::Debug;
struct Opaque;
struct Wrapper<T>(T);
trait Summary { fn s(&self) -> String; }
impl<T> Summary for Wrapper<T> where Vec<T>: Debug { fn s(&self) -> String { String::from("w") } }
fn sum<W: Summary>(w: W) { println!("{}", w.s()); }
fn main() { sum(Wrapper(1)); sum(Wrapper(Opaque)); }
main();
"#,
    );
    assert!(report.starts_with(r#"error[E0277]: `Opaque` doesn't implement `Debug`
 --> <script>:8:34
  |
8 | fn main() { sum(Wrapper(1)); sum(Wrapper(Opaque)); }
  |                              --- ^^^^^^^^^^^^^^^ the trait `Debug` is not implemented for `Opaque`
  |                              |
  |                              required by a bound introduced by this call
  |
  = note: add `#[derive(Debug)]` to `Opaque` or manually `impl Debug for Opaque`
help: the trait `Summary` is conditionally implemented for `Wrapper<T>`
 --> <script>:6:1
  |
6 | impl<T> Summary for Wrapper<T> where Vec<T>: Debug { fn s(&self) -> String { String::from("w") } }
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^-----
  |                                              |
  |                                              unsatisfied requirement introduced here: `Vec<Opaque>: Debug`
  = note: required for `Vec<Opaque>` to implement `Debug`
note: required for `Wrapper<Opaque>` to implement `Summary`
 --> <script>:6:9
  |
6 | impl<T> Summary for Wrapper<T> where Vec<T>: Debug { fn s(&self) -> String { String::from("w") } }
  |         ^^^^^^^     ^^^^^^^^^^               ----- unsatisfied trait bound introduced here
note: required by a bound in `sum`
 --> <script>:7:11
  |
7 | fn sum<W: Summary>(w: W) { println!("{}", w.s()); }
  |           ^^^^^^^ required by this bound in `sum`
help: consider annotating `Opaque` with `#[derive(Debug)]`
"#), "{report}");
}

#[test]
fn a_hand_written_std_impl_holds_where_its_own_bounds_do() {
    // `impl<T> Debug for W<T>` asks nothing of `T`, and `impl<T: Clone>
    // Debug for W<T>` asks `Clone`, not `Debug` (#2471).
    let program = |bound: &str, main: &str| {
        format!(
            r#"
use std::fmt;
#[derive(Debug)]
struct Opaque;
struct W<T>(T);
impl<T{bound}> fmt::Debug for W<T> {{ fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {{ write!(f, "W") }} }}
fn show<T: fmt::Debug>(t: T) {{ println!("{{:?}}", t); }}
fn main() {{ {main} }}
main();
"#
        )
    };
    let main = "show(W(Opaque)); println!(\"{:?}\", vec![W(Opaque)]);";
    assert_eq!(printed(&program("", main)), "W\n[W]\n");
    let report = refused(&program(": Clone", "show(W(Opaque));"));
    assert!(report.starts_with(r#"error[E0277]: the trait bound `Opaque: Clone` is not satisfied
 --> <script>:8:20
  |
8 | fn main() { show(W(Opaque)); }
  |             ----   ^^^^^^ the trait `Clone` is not implemented for `Opaque`
  |             |
  |             required by a bound introduced by this call
  |
help: the trait `Debug` is conditionally implemented for `W<T>`
 --> <script>:6:1
  |
6 | impl<T: Clone> fmt::Debug for W<T> { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "W") } }
  | ^^^^^^^^-----^^^^^^^^^^^^^^^^^^^^^
  |         |
  |         unsatisfied requirement introduced here: `Opaque: Clone`
note: required for `W<Opaque>` to implement `Debug`
 --> <script>:6:16
  |
6 | impl<T: Clone> fmt::Debug for W<T> { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "W") } }
  |         -----  ^^^^^^^^^^     ^^^^
  |         |
  |         unsatisfied trait bound introduced here
note: required by a bound in `show`
 --> <script>:7:12
  |
7 | fn show<T: fmt::Debug>(t: T) { println!("{:?}", t); }
  |            ^^^^^^^^^^ required by this bound in `show`
help: consider annotating `Opaque` with `#[derive(Clone)]`
"#), "{report}");
    // A formatted value proves the impl it formats by.
    let report = refused(&program(": Clone", "println!(\"{:?}\", W(Opaque));"));
    assert!(report.starts_with(r#"error[E0277]: the trait bound `Opaque: Clone` is not satisfied
 --> <script>:8:30
  |
8 | fn main() { println!("{:?}", W(Opaque)); }
  |                       ----   ^^^^^^^^^ the trait `Clone` is not implemented for `Opaque`
  |                       |
  |                       required by this formatting parameter
  |
help: the trait `Debug` is conditionally implemented for `W<T>`
 --> <script>:6:1
  |
6 | impl<T: Clone> fmt::Debug for W<T> { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "W") } }
  | ^^^^^^^^-----^^^^^^^^^^^^^^^^^^^^^
  |         |
  |         unsatisfied requirement introduced here: `Opaque: Clone`
note: required for `W<Opaque>` to implement `Debug`
 --> <script>:6:16
  |
6 | impl<T: Clone> fmt::Debug for W<T> { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "W") } }
  |         -----  ^^^^^^^^^^     ^^^^
  |         |
  |         unsatisfied trait bound introduced here
help: consider annotating `Opaque` with `#[derive(Clone)]`
"#), "{report}");
}

#[test]
fn an_unmet_bound_names_the_innermost_type_with_each_impl_that_needed_it() {
    // rustc names `Opaque` of `Option<Vec<Opaque>>: Debug`, and each std
    // impl that needed it, folding a run of one trait's (#2472).
    let program = |main: &str| {
        format!(
            r#"
use std::fmt::Debug;
struct Opaque;
#[derive(Debug)]
struct W<T>(T);
fn show<T: Debug>(t: T) {{ println!("{{:?}}", t); }}
fn main() {{ {main} }}
main();
"#
        )
    };
    let report = refused(&program("show(Some(vec![Opaque]));"));
    assert!(
        report.starts_with(
            r#"error[E0277]: `Opaque` doesn't implement `Debug`
 --> <script>:7:23
  |
7 | fn main() { show(Some(vec![Opaque])); }
  |             ----      ^^^^^^^^^^^^ the trait `Debug` is not implemented for `Opaque`
  |             |
  |             required by a bound introduced by this call
  |
"#
        ),
        "{report}"
    );
    assert!(
        report.contains(
            r#"  = note: required for `Vec<Opaque>` to implement `Debug`
  = note: 1 redundant requirement hidden
  = note: required for `Option<Vec<Opaque>>` to implement `Debug`
note: required by a bound in `show`
"#
        ),
        "{report}"
    );
    let report = refused(&program("show(Some((1, Opaque)));"));
    assert!(
        report.contains(
            r#"7 | fn main() { show(Some((1, Opaque))); }
  |             ----          ^^^^^^ the trait `Debug` is not implemented for `Opaque`
  |             |
  |             required by a bound introduced by this call
  |
  = note: add `#[derive(Debug)]` to `Opaque` or manually `impl Debug for Opaque`
  = note: required for `({integer}, Opaque)` to implement `Debug`
  = note: 1 redundant requirement hidden
  = note: required for `Option<({integer}, Opaque)>` to implement `Debug`
note: required by a bound in `show`
"#
        ),
        "{report}"
    );
    // A derive bounds its parameters.
    let report = refused(&program("show(vec![W(Opaque)]);"));
    assert!(
        report.contains(
            r#"note: required for `W<Opaque>` to implement `Debug`
 --> <script>:5:8
  |
4 | #[derive(Debug)]
  |          ----- in this derive macro expansion
5 | struct W<T>(T);
  |        ^ - type parameter would need to implement `Debug`
  = help: consider manually implementing `Debug` to avoid undesired bounds
  = note: 1 redundant requirement hidden
  = note: required for `Vec<W<Opaque>>` to implement `Debug`
note: required by a bound in `show`
"#
        ),
        "{report}"
    );
    // A formatted value, which a derive's bound also refuses.
    let report = refused(&program("let v = vec![W(Opaque)]; println!(\"{:?}\", v);"));
    assert!(
        report.contains(
            r#"  = help: the trait `Debug` is not implemented for `Opaque`
  = note: add `#[derive(Debug)]` to `Opaque` or manually `impl Debug for Opaque`
note: required for `W<Opaque>` to implement `Debug`
 --> <script>:5:8
  |
4 | #[derive(Debug)]
  |          ----- in this derive macro expansion
5 | struct W<T>(T);
  |        ^ - type parameter would need to implement `Debug`
  = help: consider manually implementing `Debug` to avoid undesired bounds
  = note: 1 redundant requirement hidden
  = note: required for `Vec<W<Opaque>>` to implement `Debug`
"#
        ),
        "{report}"
    );
}

#[test]
fn a_program_display_impl_holds_where_its_own_bounds_do() {
    // `impl<T: Display> Display for Shown<T>` asks `Display` of `T`, which
    // a formatted value and a bound both prove (#2471).
    let program = |main: &str| {
        format!(
            r#"
use std::fmt::{{self, Display}};
struct Opaque;
struct Shown<T>(T);
impl<T: Display> Display for Shown<T> {{ fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {{ write!(f, "<{{}}>", self.0) }} }}
fn show<T: Display>(t: T) {{ println!("{{}}", t); }}
fn main() {{ {main} }}
main();
"#
        )
    };
    assert_eq!(
        printed(&program("show(Shown(3)); println!(\"{}\", Shown(\"t\"));")),
        "<3>\n<t>\n"
    );
    let report = refused(&program("println!(\"{}\", Shown(Opaque));"));
    assert_eq!(
        report,
        r#"error[E0277]: `Opaque` doesn't implement `std::fmt::Display`
 --> <script>:7:28
  |
7 | fn main() { println!("{}", Shown(Opaque)); }
  |                       --   ^^^^^^^^^^^^^ `Opaque` cannot be formatted with the default formatter
  |                       |
  |                       required by this formatting parameter
  |
help: the trait `std::fmt::Display` is not implemented for `Opaque`
 --> <script>:3:1
  |
3 | struct Opaque;
  | ^^^^^^^^^^^^^
  = note: in format strings you may be able to use `{:?}` (or {:#?} for pretty-print) instead
help: the trait `std::fmt::Display` is conditionally implemented for `Shown<T>`
 --> <script>:5:1
  |
5 | impl<T: Display> Display for Shown<T> { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "<{}>", self.0) } }
  | ^^^^^^^^-------^^^^^^^^^^^^^^^^^^^^^^
  |         |
  |         unsatisfied requirement introduced here: `Opaque: std::fmt::Display`
note: required for `Shown<Opaque>` to implement `std::fmt::Display`
 --> <script>:5:18
  |
5 | impl<T: Display> Display for Shown<T> { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "<{}>", self.0) } }
  |         -------  ^^^^^^^     ^^^^^^^^
  |         |
  |         unsatisfied trait bound introduced here"#
    );
    let report = refused(&program("show(Shown(Opaque));"));
    assert!(
        report.starts_with(
            r#"error[E0277]: `Opaque` doesn't implement `std::fmt::Display`
 --> <script>:7:24
  |
7 | fn main() { show(Shown(Opaque)); }
  |             ----       ^^^^^^ unsatisfied trait bound
  |             |
  |             required by a bound introduced by this call
  |
help: the trait `std::fmt::Display` is not implemented for `Opaque`
"#
        ),
        "{report}"
    );
}

#[test]
fn a_conditional_impl_holds_through_generic_and_blanket_bounds() {
    // An impl's bound met by a caller's own bound, a blanket impl, a
    // reference, an opaque output or a nested impl, as rustc proves each.
    let output = printed(
        r#"
use std::fmt::Debug;
trait Two { fn two(&self) -> i32 { 2 } }
impl<X: Debug> Two for X {}
trait Summary { fn s(&self) -> String; }
struct Wrapper<T>(T);
impl<T: Debug> Summary for Wrapper<T> { fn s(&self) -> String { format!("w({:?})", self.0) } }
struct Twice<T>(T);
impl<T: Two> Summary for Twice<T> { fn s(&self) -> String { format!("{}", self.0.two()) } }
impl Summary for i32 { fn s(&self) -> String { String::from("i") } }
struct Pair<A, B>(A, B);
impl<A: Summary, B: Summary> Summary for Pair<A, B> { fn s(&self) -> String { format!("{}{}", self.0.s(), self.1.s()) } }
fn sum<W: Summary>(w: W) { println!("{}", w.s()); }
fn outer<U: Debug>(u: U) { sum(Wrapper(u)); }
fn via_ref<U: Debug>(u: &U) { sum(Wrapper(u)); }
fn make() -> impl Debug { 7 }
fn main() {
    outer(1);
    via_ref(&"r");
    sum(Wrapper(make()));
    sum(Twice(1));
    sum(Pair(Pair(1, Twice(2)), Wrapper(vec![Some(3)])));
    let boxed: Box<dyn Summary> = Box::new(Wrapper(4));
    println!("{}", boxed.s());
}
main();
"#,
    );
    assert_eq!(output, "w(1)\nw(\"r\")\nw(7)\n2\ni2w([Some(3)])\nw(4)\n");
}

fn crate_facade() -> crate::host::facade::Definition {
    use crate::host::{facade, type_def, type_ref};
    use rawr_import::surface::identity::{Form, Selector};
    let mut builder = facade::Builder::new("crt", facade::Abi::new(1).unwrap()).unwrap();
    let mut allocation = crate::host::TestImportAllocation::default();
    let field = |name: &str, ty: type_ref::Type| {
        type_def::Field::new(name.into(), ty, Selector::Unavailable)
    };
    let variant = |index: u32, name: &str, fields: Vec<type_def::Field>| {
        let form = if fields.is_empty() {
            Form::Unit
        } else {
            Form::Tuple { arity: 1 }
        };
        type_def::Variant::new(
            name.into(),
            fields.into_boxed_slice(),
            true,
            Selector::EnumVariant {
                index,
                name: name.into(),
                form,
            },
        )
    };
    let variants = vec![
        variant(
            0,
            "On",
            vec![field("0", type_ref::Type::builtin(type_ref::Builtin::Bool))],
        ),
        variant(
            1,
            "Level",
            vec![field("0", type_ref::Type::builtin(type_ref::Builtin::I64))],
        ),
        variant(2, "Off", Vec::new()),
    ];
    builder
        .imported_type(
            "Flag",
            type_def::import::Identity::new([0x61; 32]),
            &[crate::capability::Kind::Clone],
            Box::default(),
            Box::default(),
            type_def::Shape::Enum {
                variants: variants.into_boxed_slice(),
                non_exhaustive: false,
            }
            .into(),
            &mut allocation,
        )
        .unwrap();
    builder
        .imported_type(
            "Slot",
            type_def::import::Identity::new([0x62; 32]),
            &[crate::capability::Kind::Clone],
            Box::default(),
            Box::default(),
            type_def::Shape::Struct {
                form: type_def::Form::Record,
                constructor: Some(Selector::Unavailable),
                fields: Box::from([
                    field(
                        "a",
                        type_ref::Type::option(type_ref::Type::builtin(type_ref::Builtin::I64)),
                    ),
                    field("b", type_ref::Type::builtin(type_ref::Builtin::Bool)),
                ]),
                constructible: true,
                unsupported: None,
                layout: None,
            }
            .into(),
            &mut allocation,
        )
        .unwrap();
    builder.finish().unwrap()
}

fn crate_lowered(source: &str) -> String {
    use crate::diagnostic::{render, Report, Source};
    let mut engine = Engine::new();
    engine.register_host_facade(crate_facade()).unwrap();
    let program = crate::resolve::resolve(
        engine.parse_unit(source).expect("the program parses"),
        &engine.host_registry,
    )
    .expect("the program's headers resolve");
    let mut allocation = crate::host::TestImportAllocation::default();
    match super::imported(
        &engine,
        &program,
        &engine.host_registry.share(),
        &mut allocation,
        None,
    ) {
        Ok(_) => String::new(),
        Err(failure) => {
            let error = failure.into_error();
            let evidence = Source::new("main.rs", source).expect("a test source has an identity");
            render(&Report::from_parse(evidence, &error).expect("the error is in the source"))
        }
    }
}

/// The report `rawr` renders for `source`, which it refuses, as rustc's.
fn rendered(source: &str) -> String {
    use crate::diagnostic::{render, Report, Source};
    let error = Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rawr refuses this program");
    let evidence = Source::new("main.rs", source).expect("a test source has an identity");
    render(&Report::from_parse(evidence, &error).expect("the error is in the source"))
}

#[test]
fn a_match_whose_arm_rebinds_a_mut_options_name_runs() {
    // An arm's `Some(first)` below a `first: &mut Option<_>` binds the
    // value's `&mut`, as rustc's default binding mode does (#342).
    let output = printed(
        r#"
fn bump(first: &mut Option<i32>) {
    match first {
        Some(first) => *first += 1,
        None => {}
    }
}
fn main() {
    let mut o = Some(5);
    bump(&mut o);
    let mut items = vec![Some(1), None];
    for item in items.iter_mut() {
        match item {
            Some(item) => *item *= 10,
            None => {}
        }
    }
    let mut q = Some(String::from("x"));
    let s = &mut q;
    match s {
        Some(s) => s.push('y'),
        None => {}
    }
    println!("{:?} {:?} {:?}", o, items, q);
}
main();
"#,
    );
    assert_eq!(output, "Some(6) [Some(10), None] Some(\"xy\")\n");
}

#[test]
fn an_empty_match_on_a_type_with_no_values_runs() {
    // `Infallible` and an enum with no variants have no values, so a match
    // on one needs no arm and an `Err` of one needs no pattern, as rustc
    // has it (#620).
    let output = printed(
        r#"
use std::convert::Infallible;
enum Void {}
fn never(message: Infallible) -> i32 {
    match message {}
}
fn void(v: Void) -> String {
    match v {}
}
fn main() {
    let r: Result<i32, Infallible> = Ok(3);
    let Ok(n) = r;
    let m = match r {
        Ok(v) => v,
        Err(e) => match e {},
    };
    let only = match r {
        Ok(v) => v,
    };
    let _ = never;
    let _ = void;
    println!("{n} {m} {only}");
}
main();
"#,
    );
    assert_eq!(output, "3 3 3\n");
    // A match with no arms on a type that isn't an enum calls the type
    // non-empty, and a reference always has values.
    assert_eq!(
        rendered(r#"fn main() {
    let x = 3;
    match x {}
}
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: type `i32` is non-empty\n",
        " --> main.rs:3:11\n",
        "  |\n",
        "3 |     match x {}\n",
        "  |           ^\n",
        "  |\n",
        "  = note: the matched value is of type `i32`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern as shown\n",
        "  |\n",
        "3 ~     match x {\n",
        "4 +         _ => todo!(),\n",
        "5 ~     }\n",
        "  |\n",
    )
    );
    assert_eq!(
        rendered(r#"enum E { A }
fn f(e: &E) -> i32 { match e {} }
fn main() {}
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: type `&E` is non-empty\n",
        " --> main.rs:2:28\n",
        "  |\n",
        "2 | fn f(e: &E) -> i32 { match e {} }\n",
        "  |                            ^\n",
        "  |\n",
        "note: `E` defined here\n",
        " --> main.rs:1:6\n",
        "  |\n",
        "1 | enum E { A }\n",
        "  |      ^\n",
        "  = note: the matched value is of type `&E`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern as shown\n",
        "  |\n",
        "2 ~ fn f(e: &E) -> i32 { match e {\n",
        "3 +     _ => todo!(),\n",
        "  |\n",
    )
    );
    assert_eq!(
        rendered(r#"enum Void {}
fn f(r: &Void) -> i32 { match r {} }
fn main() {}
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: type `&Void` is non-empty\n",
        " --> main.rs:2:31\n",
        "  |\n",
        "2 | fn f(r: &Void) -> i32 { match r {} }\n",
        "  |                               ^\n",
        "  |\n",
        "note: `Void` defined here\n",
        " --> main.rs:1:6\n",
        "  |\n",
        "1 | enum Void {}\n",
        "  |      ^^^^\n",
        "  = note: the matched value is of type `&Void`\n",
        "  = note: references are always considered inhabited\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern as shown\n",
        "  |\n",
        "2 ~ fn f(r: &Void) -> i32 { match r {\n",
        "3 +     _ => todo!(),\n",
        "  |\n",
    )
    );
}

#[test]
fn a_witness_below_a_constructor_names_rustcs_ranges() {
    // An integer's or a `char`'s literals and ranges split its values
    // below a constructor as at the top of a match (#1287, #1306), and each
    // missing constructor comes before the witnesses of the parts after it.
    assert!(rendered(
        "fn main() {\n    let o = Some(1);\n    match o {\n        Some(1) => {}\n        \
         None => {}\n    }\n}\n"
    )
    .starts_with(
        "error[E0004]: non-exhaustive patterns: `Some(i32::MIN..=0_i32)` and \
         `Some(2_i32..=i32::MAX)` not covered\n"
    ));
    assert_eq!(
        rendered(r#"fn main() {
    let t = (1, true);
    match t {
        (1, true) => {}
        (_, false) => {}
    }
}
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: `(i32::MIN..=0_i32, true)` and `(2_i32..=i32::MAX, true)` not covered\n",
        " --> main.rs:3:11\n",
        "  |\n",
        "3 |     match t {\n",
        "  |           ^ patterns `(i32::MIN..=0_i32, true)` and `(2_i32..=i32::MAX, true)` not covered\n",
        "  |\n",
        "  = note: the matched value is of type `(i32, bool)`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern, a match arm with multiple or-patterns as shown, or multiple match arms\n",
        "  |\n",
        "5 ~         (_, false) => {},\n",
        "6 +         (i32::MIN..=0_i32, true) | (2_i32..=i32::MAX, true) => todo!()\n",
        "  |\n",
    )
    );
    assert_eq!(
        rendered(r#"fn main() {
    let mut t = (1, 3);
    let (ref mut x, 3) = t;
}
"#),
        concat!(
        "error[E0005]: refutable pattern in local binding\n",
        " --> main.rs:3:9\n",
        "  |\n",
        "3 |     let (ref mut x, 3) = t;\n",
        "  |         ^^^^^^^^^^^^^^ patterns `(_, i32::MIN..=2_i32)` and `(_, 4_i32..=i32::MAX)` not covered\n",
        "  |\n",
        "  = note: `let` bindings require an \"irrefutable pattern\", like a `struct` or an `enum` with only one variant\n",
        "  = note: for more information, visit https://doc.rust-lang.org/book/ch19-02-refutability.html\n",
        "  = note: the matched value is of type `(i32, i32)`\n",
        "help: you might want to use `let...else` to handle the variants that aren't matched\n",
        "  |\n",
        "3 |     let (ref mut x, 3) = t else { todo!() };\n",
        "  |                            ++++++++++++++++\n",
    )
    );
    assert_eq!(
        rendered(r#"fn main() {
    let t = (1, 'a');
    match t {
        (0, 'a') => {}
        (_, 'b'..='z') => {}
    }
}
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: `(i32::MIN..=-1_i32, '\\0'..='a')`, `(i32::MIN..=-1_i32, '{'..='\\u{d7ff}')`, `(i32::MIN..=-1_i32, '\\u{e000}'..='\\u{10ffff}')` and 3 more not covered\n",
        " --> main.rs:3:11\n",
        "  |\n",
        "3 |     match t {\n",
        "  |           ^ patterns `(i32::MIN..=-1_i32, '\\0'..='a')`, `(i32::MIN..=-1_i32, '{'..='\\u{d7ff}')`, `(i32::MIN..=-1_i32, '\\u{e000}'..='\\u{10ffff}')` and 3 more not covered\n",
        "  |\n",
        "  = note: the matched value is of type `(i32, char)`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern as shown, or multiple match arms\n",
        "  |\n",
        "5 ~         (_, 'b'..='z') => {},\n",
        "6 +         _ => todo!()\n",
        "  |\n",
    )
    );
    assert_eq!(
        rendered(r#"fn main() {
    let x = &5u8;
    match x {
        &0 => {}
        &(1..=254) => {}
    }
}
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: `&u8::MAX` not covered\n",
        " --> main.rs:3:11\n",
        "  |\n",
        "3 |     match x {\n",
        "  |           ^ pattern `&u8::MAX` not covered\n",
        "  |\n",
        "  = note: the matched value is of type `&u8`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern or an explicit pattern as shown\n",
        "  |\n",
        "5 ~         &(1..=254) => {},\n",
        "6 +         &u8::MAX => todo!()\n",
        "  |\n",
    )
    );
}

#[test]
fn a_constant_path_is_a_pattern_and_a_range_end() {
    // A constant's path is its value in a pattern, alone or at either end
    // of a range, as rustc reads it (#1288), and a one-name constant is no
    // binding.
    let output = printed(
        r#"
const LO: i32 = 3;
struct W;
impl W {
    const TOP: i64 = 10;
}
fn sign(x: i32) -> i32 {
    match x {
        i32::MIN..=-5 => -2,
        -4..=-1 => -1,
        0 => 0,
        1..LO => 1,
        LO..=i32::MAX => 2,
    }
}
fn top(x: i64) -> i64 {
    match x {
        0..W::TOP => 1,
        W::TOP..=i64::MAX => 2,
        i64::MIN..0 => 3,
    }
}
fn exact(x: i32) -> i32 {
    match x {
        LO => 1,
        _ => 2,
    }
}
fn letter(x: char, n: u8, o: Option<i8>) -> i32 {
    let a = match x {
        char::MIN..='a' => 1,
        'b'..=char::MAX => 2,
    };
    let b = match n {
        u8::MAX => 10,
        _ => 20,
    };
    let c = match o {
        Some(i8::MIN..=-1) => 100,
        Some(0..=i8::MAX) => 200,
        None => 300,
    };
    a + b + c
}
fn main() {
    println!("{} {} {} {} {}", sign(-9), sign(-2), sign(0), sign(2), sign(7));
    println!("{} {} {} {} {}", top(3), top(10), top(-1), exact(3), exact(4));
    println!("{} {}", letter('a', 255, Some(-3)), letter('z', 4, None));
}
main();
"#,
    );
    assert_eq!(output, "-2 -1 0 1 2\n1 2 3 1 2\n111 322\n");
    assert_eq!(
        rendered(r#"fn f(x: Option<i8>) -> i32 {
    match x {
        Some(i8::MIN..=-1) => 1,
        None => 3,
    }
}
fn main() {}
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: `Some(0_i8..=i8::MAX)` not covered\n",
        " --> main.rs:2:11\n",
        "  |\n",
        "2 |     match x {\n",
        "  |           ^ pattern `Some(0_i8..=i8::MAX)` not covered\n",
        "  |\n",
        "  = note: the matched value is of type `Option<i8>`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern or an explicit pattern as shown\n",
        "  |\n",
        "4 ~         None => 3,\n",
        "5 ~         Some(0_i8..=i8::MAX) => todo!(),\n",
        "  |\n",
    )
    );
}

#[test]
fn a_struct_pattern_is_a_parameter_pattern() {
    // A function's or a closure's parameter may be a struct's pattern, as
    // rustc's (#1307), and one that misses some values is E0005.
    let output = printed(
        r#"
struct P { x: i64, y: i64 }
struct Style { width: i64, height: i64 }
struct T(i64, i64);
fn f(P { x, .. }: P) -> i64 { x }
fn g(Style { width, height: h }: Style) -> i64 { width * h }
fn t(T(a, b): T) -> i64 { a - b }
fn main() {
    let c = |P { x, y }: P| x + y;
    println!("{} {} {} {}", f(P { x: 1, y: 2 }), g(Style { width: 3, height: 4 }), t(T(9, 4)), c(P { x: 5, y: 6 }));
}
main();
"#,
    );
    assert_eq!(output, "1 12 5 11\n");
    assert_eq!(
        rendered(r#"struct P { x: i64, y: i64 }
fn f(P { x: 1, y }: P) -> i64 { y }
fn main() {}
"#),
        concat!(
        "error[E0005]: refutable pattern in function argument\n",
        " --> main.rs:2:6\n",
        "  |\n",
        "2 | fn f(P { x: 1, y }: P) -> i64 { y }\n",
        "  |      ^^^^^^^^^^^^^ patterns `P { x: i64::MIN..=0_i64, .. }` and `P { x: 2_i64..=i64::MAX, .. }` not covered\n",
        "  |\n",
        "note: `P` defined here\n",
        " --> main.rs:1:8\n",
        "  |\n",
        "1 | struct P { x: i64, y: i64 }\n",
        "  |        ^\n",
        "  = note: the matched value is of type `P`\n",
    )
    );
}

#[cfg(feature = "rust-import")]
#[test]
fn a_crate_values_patterns_cover_it_by_the_patterns_below() {
    // A crate enum's variant is covered by the literals of its field's
    // patterns (#812), a crate struct by its fields' patterns together
    // (#1972), and what they miss is rustc's witnesses over every field
    // (#1308).
    assert_eq!(
        crate_lowered(
            "fn f(f: crt::Flag) -> i64 {\n    match f {\n        crt::Flag::On(true) => 1,\n        \
             crt::Flag::On(false) => 2,\n        crt::Flag::Level(0) => 3,\n        \
             crt::Flag::Level(i64::MIN..=-1) => 4,\n        crt::Flag::Level(1..) => 5,\n        \
             crt::Flag::Off => 6,\n    }\n}\n0\n"
        ),
        ""
    );
    assert_eq!(
        crate_lowered(
            "fn f(s: crt::Slot) -> i64 {\n    match s {\n        crt::Slot { a: Some(_), .. } => 1,\n        \
             crt::Slot { a: None, .. } => 2,\n    }\n}\n0\n"
        ),
        ""
    );
    assert_eq!(
        crate_lowered(r#"fn f(f: crt::Flag) -> i64 {
    match f {
        crt::Flag::On(true) => 1,
        crt::Flag::Level(_) => 3,
        crt::Flag::Off => 6,
    }
}
0
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: `Flag::On(false)` not covered\n",
        " --> main.rs:2:11\n",
        "  |\n",
        "2 |     match f {\n",
        "  |           ^ pattern `Flag::On(false)` not covered\n",
        "  |\n",
        "  = note: the matched value is of type `crt::Flag`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern or an explicit pattern as shown\n",
        "  |\n",
        "5 ~         crt::Flag::Off => 6,\n",
        "6 ~         Flag::On(false) => todo!(),\n",
        "  |\n",
    )
    );
    assert_eq!(
        crate_lowered(r#"fn f(s: crt::Slot) -> i64 {
    match s {
        crt::Slot { a: Some(_), b: true } => 1,
        crt::Slot { a: None, .. } => 2,
    }
}
0
"#),
        concat!(
        "error[E0004]: non-exhaustive patterns: `Slot { a: Some(_), b: false }` not covered\n",
        " --> main.rs:2:11\n",
        "  |\n",
        "2 |     match s {\n",
        "  |           ^ pattern `Slot { a: Some(_), b: false }` not covered\n",
        "  |\n",
        "  = note: the matched value is of type `crt::Slot`\n",
        "help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern or an explicit pattern as shown\n",
        "  |\n",
        "4 ~         crt::Slot { a: None, .. } => 2,\n",
        "5 ~         Slot { a: Some(_), b: false } => todo!(),\n",
        "  |\n",
    )
    );
    assert_eq!(
        crate_lowered(r#"fn f(s: crt::Slot) -> i64 {
    let crt::Slot { a: Some(_) | None, b: true } = s;
    0
}
0
"#),
        concat!(
        "error[E0005]: refutable pattern in local binding\n",
        " --> main.rs:2:9\n",
        "  |\n",
        "2 |     let crt::Slot { a: Some(_) | None, b: true } = s;\n",
        "  |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ patterns `Slot { a: None, b: false }` and `Slot { a: Some(_), b: false }` not covered\n",
        "  |\n",
        "  = note: `let` bindings require an \"irrefutable pattern\", like a `struct` or an `enum` with only one variant\n",
        "  = note: for more information, visit https://doc.rust-lang.org/book/ch19-02-refutability.html\n",
        "  = note: the matched value is of type `crt::Slot`\n",
        "help: you might want to use `let...else` to handle the variants that aren't matched\n",
        "  |\n",
        "2 |     let crt::Slot { a: Some(_) | None, b: true } = s else { todo!() };\n",
        "  |                                                      ++++++++++++++++\n",
    )
    );
    assert_eq!(
        crate_lowered(r#"fn f(f: crt::Flag) -> i64 {
    let crt::Flag::Level(3) = f;
    0
}
0
"#),
        concat!(
        "error[E0005]: refutable pattern in local binding\n",
        " --> main.rs:2:9\n",
        "  |\n",
        "2 |     let crt::Flag::Level(3) = f;\n",
        "  |         ^^^^^^^^^^^^^^^^^^^ patterns `Flag::On(_)` and `Flag::Off` not covered\n",
        "  |\n",
        "  = note: `let` bindings require an \"irrefutable pattern\", like a `struct` or an `enum` with only one variant\n",
        "  = note: for more information, visit https://doc.rust-lang.org/book/ch19-02-refutability.html\n",
        "  = note: the matched value is of type `crt::Flag`\n",
        "help: you might want to use `let...else` to handle the variants that aren't matched\n",
        "  |\n",
        "2 |     let crt::Flag::Level(3) = f else { todo!() };\n",
        "  |                                 ++++++++++++++++\n",
    )
    );
    assert_eq!(
        crate_lowered(r#"fn f(s: crt::Slot) -> i64 {
    let (crt::Slot { a: Some(n), .. }, 1) = (s, 2);
    n
}
0
"#),
        concat!(
        "error[E0005]: refutable pattern in local binding\n",
        " --> main.rs:2:9\n",
        "  |\n",
        "2 |     let (crt::Slot { a: Some(n), .. }, 1) = (s, 2);\n",
        "  |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ pattern `(Slot { a: None, .. }, _)` not covered\n",
        "  |\n",
        "  = note: `let` bindings require an \"irrefutable pattern\", like a `struct` or an `enum` with only one variant\n",
        "  = note: for more information, visit https://doc.rust-lang.org/book/ch19-02-refutability.html\n",
        "  = note: the matched value is of type `(crt::Slot, i32)`\n",
        "help: you might want to use `let...else` to handle the variant that isn't matched\n",
        "  |\n",
        "2 |     let (crt::Slot { a: Some(n), .. }, 1) = (s, 2) else { todo!() };\n",
        "  |                                                    ++++++++++++++++\n",
    )
    );
}

#[test]
fn cfg_attr_applies_its_attributes_to_items_fields_and_variants() {
    // `#[cfg_attr(..)]` applies what it lists where its predicate holds,
    // a `cfg_attr` among them under its own too, on an item, a field and a
    // variant, and `all()` and `any()` hold and fail as rustc's do (#603).
    // The output is rustc's.
    let output = printed(
        r#"
#[cfg_attr(all(), derive(Debug, Clone))]
struct P { x: i32 }
#[cfg_attr(all(), cfg_attr(all(), derive(Debug)))]
struct S;
#[derive(Debug)]
struct F { a: i32, #[cfg(any())] b: i32, #[cfg_attr(all(), cfg(any()))] c: i32 }
#[derive(Debug)]
enum E { A, #[cfg(any())] B, C(#[cfg(any())] u8, i32), D { #[cfg_attr(all(), cfg(any()))] x: i32, y: i32 } }
#[derive(Debug, Default)]
enum G { #[cfg_attr(all(), default)] H, I }
fn main() {
    let n = match E::A { E::A => 0, E::C(v) => v, E::D { y } => y };
    println!("{:?} {:?} {:?} {}", P { x: 1 }.clone(), S, F { a: 2 }, n);
    println!("{:?} {:?} {:?} {}", E::C(3), E::D { y: 4 }, G::default(), cfg!(all()) && !cfg!(any()));
    let _ = G::I;
}
main();
"#,
    );
    assert_eq!(
        output,
        "P { x: 1 } S F { a: 2 } 0\nC(3) D { y: 4 } H true\n"
    );
    // A `cfg_attr` whose predicate fails applies nothing.
    let report = refused(
        r#"
#[cfg_attr(all(), cfg_attr(any(), derive(Debug)))]
struct T;
fn main() { println!("{:?}", T); }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `T` doesn't implement `Debug`"),
        "{report}"
    );
}

#[test]
fn an_attribute_no_derive_declares_is_rustcs_error() {
    // A derive helper such as `#[serde(..)]` with only std's derives on the
    // item names nothing in scope (#604).
    for source in [
        "#[derive(Debug)]\nstruct S { #[serde(default)] a: i32 }\nfn main() {}\nmain();\n",
        "#[serde(rename_all = \"camelCase\")]\nstruct S;\nfn main() {}\nmain();\n",
        "enum E { #[serde(rename = \"x\")] A }\nfn main() {}\nmain();\n",
        "fn main() { #[derive(Debug)] struct L { #[serde(skip)] a: i32 } }\nmain();\n",
    ] {
        let report = refused(source);
        assert!(
            report.starts_with("error: cannot find attribute `serde` in this scope"),
            "{report}"
        );
    }
    // rustc's own attributes and a tool's are in scope.
    let output = printed(
        r#"
#[rustfmt::skip]
#[must_use]
#[inline]
#[deprecated(note = "x")]
#[doc = "d"]
fn f() -> i32 { 1 }
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum E { #[default] A }
fn main() { println!("{} {:?}", f(), E::A); }
main();
"#,
    );
    assert_eq!(output, "1 A\n");
}

#[test]
fn every_range_form_is_a_value_of_std_s_range_type() {
    // `a..b`, `a..=b`, `a..`, `..b`, `..=b` and `..` of integers, chars
    // and floats, with their fields, `contains` and `Debug` (#1106, #1260),
    // and the 128-bit ones as ranges, not iterators (#1063).
    let output = printed(
        r#"
use std::ops::{Range, RangeInclusive};
fn main() {
    let r = 1..4;
    let half: Range<i8> = -3..0;
    println!("{:?} {:?} {:?} {} {} {}", r, 1..=4, 'a'..'c', r.start, r.end, half.start);
    let to = ..2;
    let all = ..;
    println!("{:?} {:?} {:?} {:?} {}", to, all, 2.., ..=3, to.end);
    println!("{} {} {}", (0.5..1.0).contains(&0.7), (..1.5).contains(&2.0), (3..).contains(&2));
    println!("{:?} {:?} {:?}", 0.5..=1.0, 0.5..1.0, 0.5..);
    let wide: RangeInclusive<i128> = 0..=5;
    let unsigned = 0u128..=3;
    let narrow = 0i128..5;
    println!("{:?} {} {:?} {:?} {}", wide, wide.clone().sum::<i128>(), unsigned, narrow, narrow.start);
    let huge = (i128::MAX - 2)..=i128::MAX;
    println!("{:?} {}", huge, huge.clone().count());
}
main();
"#,
    );
    assert_eq!(
        output,
        "1..4 1..=4 'a'..'c' 1 4 -3\n..2 .. 2.. ..=3 2\ntrue false false\n\
         0.5..=1.0 0.5..1.0 0.5..\n0..=5 15 0..=3 0..5 0\n\
         170141183460469231731687303715884105725..=170141183460469231731687303715884105727 3\n"
    );
}

#[test]
fn ranges_derive_compare_and_key_sets_as_std_s_do() {
    // std's ranges are `Clone`, `Debug`, `PartialEq`, `Eq` and `Hash`
    // where their element is, and `Range` and `RangeFull` are `Default`
    // (#1062).
    let output = printed(
        r#"
use std::collections::{BTreeSet, HashSet};
use std::ops::{Range, RangeFrom, RangeFull, RangeInclusive, RangeTo};
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct S { a: Range<usize>, b: RangeInclusive<isize> }
#[derive(Debug, Default)]
struct D { r: Range<usize>, all: RangeFull }
struct Opts { from: RangeFrom<i32>, to: RangeTo<i32>, letters: RangeInclusive<char> }
fn main() {
    let s = S { a: 1..3, b: -1..=1 };
    let t = s.clone();
    println!("{:?} {} {}", s, s == t, s.a.len());
    println!("{:?}", D::default());
    println!("{} {} {}", (1..4) == (1..4), (1..=4) != (1..=5), (..) == (..));
    let mut h = HashSet::new();
    h.insert(1..3);
    h.insert(1..3);
    let b: BTreeSet<RangeInclusive<u8>> = BTreeSet::new();
    println!("{} {}", h.len(), b.len());
    let o = Opts { from: 3.., to: ..4, letters: 'x'..='z' };
    let first: Vec<i32> = o.from.clone().take(2).collect();
    println!("{:?} {:?} {:?} {} {:?}", o.from, o.to, o.letters, o.to.contains(&3), first);
}
main();
"#,
    );
    assert_eq!(
        output,
        "S { a: 1..3, b: -1..=1 } true 2\nD { r: 0..0, all: .. }\ntrue true true\n1 0\n\
         3.. ..4 'x'..='z' true [3, 4]\n"
    );
}

#[test]
fn a_range_s_methods_and_its_iterator_are_std_s() {
    // `len`, `is_empty`, `rev`, `step_by`, `start()`, `end()`,
    // `into_inner()`, and `next()`, which steps the range where it is.
    let output = printed(
        r#"
fn main() {
    let r = 1..5;
    println!("{} {} {}", r.len(), r.is_empty(), (5..1).is_empty());
    let back: Vec<i32> = r.clone().rev().collect();
    let odd: Vec<i32> = r.step_by(2).collect();
    println!("{:?} {:?}", back, odd);
    let i = 1..=3;
    println!("{} {} {:?} {} {}", i.start(), i.end(), i.clone().into_inner(), i == (1..=3), (1..=1).is_empty());
    let mut it = 0..3;
    println!("{:?}", it.next());
    while let Some(x) = it.next() { print!("{x} "); }
    println!("{:?}", it);
    let mut c = 'x'..;
    println!("{:?} {:?} {:?}", c.next(), c.next(), c);
    let mut n = 0..=3;
    n.next();
    let rest: Vec<i32> = n.clone().collect();
    println!("{:?} {:?} {:?}", n, rest, (1..=3).into_iter());
}
main();
"#,
    );
    assert_eq!(
        output,
        "4 false true\n[4, 3, 2, 1] [1, 3]\n1 3 (1, 3) true false\nSome(0)\n1 2 3..3\n\
         Some('x') Some('y') 'z'..\n1..=3 [1, 2, 3] 1..=3\n"
    );
    // `ExactSizeIterator` is std's for a `RangeInclusive<i16>`, not an
    // `i64` one, and its fields are private.
    let report = refused("fn main() { let r = 1..=4i64; println!(\"{}\", r.len()); }");
    assert!(
        report.starts_with("error[E0599]: no method named `len` found"),
        "{report}"
    );
    let report = refused("fn main() { let r = 1..=4; println!(\"{}\", r.start); }");
    assert!(
        report.starts_with(
            "error[E0616]: field `start` of struct `std::ops::RangeInclusive` is private"
        ),
        "{report}"
    );
}

#[test]
fn a_range_value_is_its_own_iterator_where_one_is_taken() {
    // A range kept, passed, returned, boxed or given to a generic `T` is
    // the range, and an iterator where the code takes one (#1105).
    let output = printed(
        r#"
use std::ops::Range;
fn make(n: usize) -> Range<usize> { 0..n }
fn total(r: Range<usize>) -> usize { r.sum() }
fn upto(n: u32) -> impl Iterator<Item = u32> { 0..n }
fn sum(it: impl Iterator<Item = i64>) -> i64 { it.sum() }
fn gather<I: IntoIterator<Item = i32>>(items: I) -> i32 { items.into_iter().sum() }
fn show<T: std::fmt::Debug>(t: T) -> String { format!("{:?}", t) }
fn main() {
    let r = make(4);
    println!("{:?} {} {}", r, total(r.clone()), r.len());
    for i in r { print!("{i} "); }
    println!("{:?}", upto(3).collect::<Vec<_>>());
    println!("{} {} {} {}", sum(1..=3), sum(1..3), gather(1..4), gather(1..=4));
    let boxed: Box<dyn Iterator<Item = i32>> = Box::new(0..3);
    let ranges = vec![0..2, 3..5];
    let flat: Vec<i32> = ranges.iter().flat_map(|r| r.clone()).collect();
    println!("{} {:?} {:?} {:?}", boxed.count(), ranges, flat, Some(1..4));
    println!("{} {} {} {}", show(1..4), show(..), show('a'..'c'), show(2.5..=3.0));
    let mut w = vec![0];
    w.extend(1..3);
    let z: Vec<(i64, char)> = (1..=3).zip("abc".chars()).collect();
    println!("{:?} {:?}", w, z);
}
main();
"#,
    );
    assert_eq!(
        output,
        "0..4 6 4\n0 1 2 3 [0, 1, 2]\n6 3 6 10\n3 [0..2, 3..5] [0, 1, 3, 4] Some(1..4)\n\
         1..4 .. 'a'..'c' 2.5..=3.0\n[0, 1, 2] [(1, 'a'), (2, 'b'), (3, 'c')]\n"
    );
}

#[test]
fn a_range_kept_in_a_local_drains_and_slices_and_moves() {
    // `v.drain(r)` and `&v[r]` read a range a local holds, as std's
    // `RangeBounds` does, and iterating a range moves it.
    let output = printed(
        r#"
fn main() {
    let mut v = vec![1, 2, 3, 4, 5, 6];
    let r = 1..3;
    let gone: Vec<i32> = v.drain(r).collect();
    let tail = 2..;
    println!("{:?} {:?} {:?}", v, gone, &v[tail]);
    let all = ..;
    let t: Vec<i32> = v.drain(all).collect();
    println!("{:?} {:?}", v, t);
}
main();
"#,
    );
    assert_eq!(output, "[1, 4, 5, 6] [2, 3] [5, 6]\n[] [1, 4, 5, 6]\n");
    for program in [
        "fn main() { let r = 0..3; let v: Vec<i32> = r.collect(); println!(\"{:?} {:?}\", v, r); }",
        "fn main() { let r = 0..3; for x in r { print!(\"{x}\"); } println!(\"{:?}\", r); }",
    ] {
        let report = refused(program);
        assert!(
            report.starts_with("error[E0382]: borrow of moved value: `r`"),
            "{report}"
        );
    }
}

#[test]
fn a_mut_local_takes_a_new_borrow_of_a_field_or_an_element() {
    // `r = &mut s.right` and `e = &mut v[1]` move a `&mut` local on to a
    // part of a local, in a loop too, as rustc's build runs them (#121).
    let output = printed(
        r#"
struct S { left: i32, right: i32 }
struct Node { val: i32, kids: Vec<i32> }
fn main() {
    let mut s = S { left: 1, right: 2 };
    let mut v = vec![10, 20];
    let mut r = &mut s.left;
    *r += 10;
    r = &mut s.right;
    *r += 20;
    let mut e = &mut v[0];
    *e += 1;
    e = &mut v[1];
    *e += 2;
    println!("{} {} {:?}", s.left, s.right, v);
    let mut nodes = vec![Node { val: 1, kids: vec![] }, Node { val: 2, kids: vec![] }];
    let mut whole = 0;
    let mut r = &mut whole;
    for i in 0..2 {
        *r += 1;
        r = &mut nodes[i].val;
        *r *= 10;
    }
    *r += 5;
    let mut k = &mut nodes[0].kids;
    k.push(1);
    k = &mut nodes[1].kids;
    k.push(2);
    println!("{} {} {} {:?} {:?}", whole, nodes[0].val, nodes[1].val, nodes[0].kids, nodes[1].kids);
    let mut t = (1, 2);
    let mut a = [5, 6, 7];
    let mut m = &mut t.0;
    *m += 1;
    m = &mut t.1;
    *m += 1;
    let i = 2;
    m = &mut a[i];
    *m += 1;
    println!("{:?} {:?}", t, a);
}
main();
"#,
    );
    assert_eq!(
        output,
        "11 22 [11, 22]\n1 11 25 [1] [2]\n(2, 3) [5, 6, 8]\n"
    );
    // The new borrow conflicts as rustc's does.
    let report = refused(
        r#"
struct S { left: i32, right: i32 }
fn main() {
    let mut s = S { left: 1, right: 2 };
    let mut r = &mut s.left;
    r = &mut s.right;
    let q = &mut s.right;
    *r += 1;
    *q += 1;
}
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0499]: cannot borrow `s.right` as mutable more than once at a time"
        ),
        "{report}"
    );
    let report = refused(
        r#"
struct S { left: i32, right: i32 }
fn main() {
    let mut s = S { left: 1, right: 2 };
    let mut r = &mut s.left;
    *r += 1;
    r = &mut s.right;
    s.right = 5;
    *r += 1;
    println!("{}", s.left);
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0506]: cannot assign to `s.right` because it is borrowed"),
        "{report}"
    );
}

#[test]
fn a_mut_chosen_by_a_branch_refers_to_the_place_the_run_chose() {
    // `if c { &mut a } else { &mut b }` and a `match` choosing a field
    // give a `&mut` local the place the run chose, as rustc's build runs
    // them, in a loop and in a `&mut self` method too (#122).
    let output = printed(
        r#"
struct S { x: Vec<i32>, y: Vec<i32>, z: Vec<i32> }
struct T { a: i32, b: i32 }
impl T {
    fn bump(&mut self, first: bool) {
        let r = if first { &mut self.a } else { &mut self.b };
        *r += 1;
    }
}
fn main() {
    let mut a = 1;
    let mut b = 2;
    let c = a < b;
    let r = if c { &mut a } else { &mut b };
    *r += 10;
    println!("{} {}", a, b);
    let mut s = S { x: vec![], y: vec![], z: vec![] };
    for k in 0..6 {
        let r = match k % 3 { 0 => &mut s.x, 1 => { &mut s.y } _ => if k > 4 { &mut s.z } else { &mut s.x } };
        r.push(k);
    }
    println!("{:?} {:?} {:?}", s.x, s.y, s.z);
    let mut r = &mut a;
    for i in 0..4 {
        *r += i;
        r = if i % 2 == 0 { &mut b } else { &mut a };
    }
    *r += 100;
    println!("{} {}", a, b);
    let mut v = vec![1, 2, 3];
    let mut w = 0;
    let i = 2;
    let e = if i < v.len() { &mut v[i] } else { &mut w };
    *e *= 7;
    println!("{:?} {}", v, w);
    let mut t = T { a: 0, b: 0 };
    t.bump(true);
    t.bump(false);
    t.bump(false);
    println!("{} {}", t.a, t.b);
}
main();
"#,
    );
    assert_eq!(
        output,
        "11 2\n[0, 2, 3] [1, 4] [5]\n113 6\n[1, 2, 21] 0\n1 2\n"
    );
    // A loan of one branch is not live through the other, nor through a
    // loop's next iteration, as a shared one's isn't.
    let output = printed(
        r#"
fn main() {
    let mut x = vec![1];
    let y = vec![2];
    for k in 0..3 {
        let r = if k > 1 { &x } else { &y };
        println!("{:?}", r);
        x.push(k);
    }
    let mut p = vec![];
    let mut q = vec![];
    for k in 0..3 {
        let r = if k > 1 { &mut p } else { &mut q };
        r.push(k);
        p.push(9);
    }
    println!("{:?} {:?}", p, q);
}
main();
"#,
    );
    assert_eq!(output, "[2]\n[2]\n[1, 0, 1]\n[9, 9, 2, 9] [0, 1]\n");
    let report = refused(
        r#"
fn main() {
    let mut a = 1;
    let mut b = 2;
    let c = a < b;
    let r = if c { &mut a } else { &mut b };
    a += 1;
    *r += 10;
    println!("{} {}", a, b);
}
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0503]: cannot use `a` because it was mutably borrowed\n --> <script>:7:5"
        ),
        "{report}"
    );
}

#[test]
fn a_mut_self_chain_runs_on_a_temporary_kept_to_the_statement_end() {
    // `Config::new().time().io().build()` of the program's own `&mut self
    // -> &mut Self` methods: the temporary is made once and lives to the
    // statement's end, as rustc keeps it (#2074).
    let program = |main: &str| {
        format!(
            r#"
struct Config {{ time: bool, io: bool, n: u32 }}
impl Config {{
    fn new() -> Config {{ println!("new"); Config {{ time: false, io: false, n: 0 }} }}
    fn time(&mut self) -> &mut Self {{ self.time = true; self.n += 1; self }}
    fn io(&mut self) -> &mut Self {{ self.io = true; self.n += 10; self }}
    fn build(&self) -> String {{ format!("{{}} {{}} {{}}", self.time, self.io, self.n) }}
}}
fn main() {{ {main} }}
main();
"#
        )
    };
    let output = printed(&program(
        r#"let s = Config::new().time().io().build();
    println!("{}", s);
    println!("{}", Config::new().io().time().time().build());"#,
    ));
    assert_eq!(output, "new\ntrue true 11\nnew\ntrue true 12\n");
    let report = refused(&program(
        r#"let c = Config::new().time(); println!("{}", c.n);"#,
    ));
    assert!(
        report.starts_with("error[E0716]: temporary value dropped while borrowed"),
        "{report}"
    );
}

#[test]
fn a_mut_local_in_an_array_or_vec_literal_moves_into_it() {
    // rustc reborrows a `&mut` only where a reference is expected, so
    // `vec![r]` and `[r]` move `r` (#2502).
    for literal in ["vec![r]", "[r]"] {
        let report = refused(&format!(
            "fn main() {{ let mut x = 1; let r = &mut x; let v = {literal}; *r += 1; drop(v); }}\nmain();\n"
        ));
        assert!(
            report.starts_with("error[E0382]: use of moved value: `r`"),
            "{report}"
        );
    }
    // A `Vec<&mut i32>` parameter expects a reference: `r` is reborrowed.
    let output = printed(
        r#"
fn take(v: Vec<&mut i32>) { println!("{}", v.len()); }
fn main() { let mut x = 1; let r = &mut x; take(vec![r]); *r += 1; println!("{}", x); }
main();
"#,
    );
    assert_eq!(output, "1\n2\n");
}

#[test]
fn a_mut_stored_in_a_value_reaches_its_place() {
    // A `&mut` in a struct, a tuple struct, a tuple, `Some` and `vec!`
    // reaches the place it borrows, as rustc's build runs them (#184,
    // #2503); a derived `Debug` writes what each refers to.
    let output = printed(
        r#"
#[derive(Debug, PartialEq, PartialOrd)]
struct S<'a> { a: &'a mut i32 }
struct D<'a>(&'a mut Vec<i32>);
fn main() {
    let mut x = 1;
    let s = S { a: &mut x };
    *s.a += 5;
    println!("{:?}", s);
    let mut z = 6;
    let mut w = 7;
    let (t, u) = (S { a: &mut z }, S { a: &mut w });
    println!("{} {} {}", s == t, s == u, s < u);
    println!("{}", x);
    let mut v = vec![1];
    let d = D(&mut v);
    d.0.push(2);
    println!("{:?}", v);
    let mut y = 3;
    let o = Some(&mut y);
    if let Some(r) = o {
        *r = 7;
    }
    println!("{}", y);
    let mut a = 1;
    let mut b = 2;
    let mut refs = vec![&mut a, &mut b];
    *refs[0] += 5;
    *refs[1] *= 3;
    for r in refs {
        *r += 10;
    }
    println!("{} {}", a, b);
}
main();
"#,
    );
    assert_eq!(output, "S { a: 6 }\ntrue false true\n6\n[1, 2]\n7\n16 16\n");
}

#[test]
fn an_option_of_a_mut_kept_in_a_local_reaches_its_place() {
    // `v.last_mut()` kept in a local holds `Some` of the element's place,
    // so a write through its `unwrap()` changes `v` (#195).
    let output = printed(
        r#"
fn main() {
    let mut v = vec![1, 2, 3];
    let mut o = v.last_mut();
    let r = o.unwrap();
    *r = 9;
    println!("{:?}", v);
    let e = v.get_mut(1);
    if let Some(e) = e {
        *e = 5;
    }
    match v.first_mut() {
        Some(x) => *x *= 7,
        None => {}
    }
    let f = v.first_mut();
    match f {
        Some(x) => *x += 1,
        None => {}
    }
    let none = v.get_mut(10).is_none();
    println!("{:?} {}", v, none);
    let mut w = vec![String::from("a")];
    let t = w.get_mut(0);
    t.unwrap().push('c');
    println!("{:?}", w);
}
main();
"#,
    );
    assert_eq!(output, "[1, 2, 9]\n[8, 5, 9] true\n[\"ac\"]\n");
}

#[test]
fn a_mut_field_reborrowed_through_self_reaches_its_place() {
    // `&mut *self.a` of a `&mut` field returns the place the field refers
    // to, and a method changes it through `self` (#2503).
    let output = printed(
        r#"
struct S<'a> { a: &'a mut i32 }
impl<'a> S<'a> {
    fn whole(&mut self) -> &mut i32 { &mut *self.a }
    fn bump(&mut self) { *self.a += 1; }
}
fn main() {
    let mut x = 1;
    let mut s = S { a: &mut x };
    s.bump();
    *s.whole() += 10;
    println!("{}", x);
}
main();
"#,
    );
    assert_eq!(output, "12\n");
}

#[test]
fn disjoint_stored_borrows_of_one_local_run_without_a_data_race() {
    // Two stored `&mut`s of one local's parts, and a method on one part
    // given a stored `&mut` of another, run as rustc's build runs them:
    // the part a call is lent is a cell of its own (D1113).
    let output = printed(
        r#"
#[derive(Debug)]
struct S<'a> { a: &'a mut i32, b: &'a mut i32 }
impl<'a> S<'a> {
    fn add_to_a(&mut self) { *self.a += *self.b; }
}
fn sw(a: &mut i32, b: &mut i32) { let t = *a; *a = *b; *b = t; }
struct Buf { data: Vec<u8> }
struct State { n: u32 }
struct Ctx { buf: Buf, state: State }
struct Writer<'a> { out: &'a mut Buf }
struct Lent<'a> { state: &'a mut State }
impl State {
    fn step(&mut self, w: &mut Writer) { self.n += 1; w.out.data.push(self.n as u8); }
}
fn main() {
    let mut p = (1, 2);
    let t = (&mut p.0, &mut p.1);
    std::mem::swap(t.0, t.1);
    println!("{:?}", p);
    let mut s = S { a: &mut p.0, b: &mut p.1 };
    s.add_to_a();
    sw(s.a, s.b);
    println!("{:?}", s);
    println!("{:?}", p);
    let mut ctx = Ctx { buf: Buf { data: vec![] }, state: State { n: 0 } };
    let mut w = Writer { out: &mut ctx.buf };
    ctx.state.step(&mut w);
    let lent = Lent { state: &mut ctx.state };
    lent.state.step(&mut w);
    println!("{:?} {}", ctx.buf.data, ctx.state.n);
}
main();
"#,
    );
    assert_eq!(output, "(2, 1)\nS { a: 1, b: 3 }\n(1, 3)\n[1, 2] 2\n");
}

#[test]
fn a_mut_stored_in_a_value_keeps_its_loan() {
    // The borrow checker's verdicts hold for a `&mut` in a value, as
    // rustc's do.
    let report = refused(
        r#"
struct D<'a>(&'a mut Vec<i32>);
fn main() {
    let mut v = vec![];
    let d = D(&mut v);
    v.push(1);
    d.0.push(2);
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0499]: cannot borrow `v` as mutable more than once at a time"),
        "{report}"
    );
    let report = refused(
        r#"
struct S<'a> { a: &'a mut i32 }
fn main() {
    let mut x = 1;
    let s = S { a: &mut x };
    x += 1;
    *s.a += 1;
    println!("{}", x);
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0503]: cannot use `x` because it was mutably borrowed"),
        "{report}"
    );
}

#[test]
fn every_lifetime_a_returned_value_must_outlive_is_reported() {
    // rustc reports one error for each lifetime that must outlive another,
    // the lifetimes it names numbered across them, then the help that
    // relates them (#2487).
    signature_verdict(
        r#"fn f<'a, 'b>(x: (&'a i32, &'b i32)) -> (&'b i32, &'a i32) { x }
fn main() { let (a, b) = (1, 2); println!("{:?}", f((&a, &b))); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:61
  |
1 | fn f<'a, 'b>(x: (&'a i32, &'b i32)) -> (&'b i32, &'a i32) { x }
  |      --  -- lifetime `'b` defined here                      ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`

error: lifetime may not live long enough
 --> <script>:1:61
  |
1 | fn f<'a, 'b>(x: (&'a i32, &'b i32)) -> (&'b i32, &'a i32) { x }
  |      --  -- lifetime `'b` defined here                      ^ function was supposed to return data with lifetime `'a` but it is returning data with lifetime `'b`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`

help: `'a` and `'b` must be the same: replace one with the other
"#),
    );
    signature_verdict(
        r#"use std::cell::Cell;
fn f<'a, 'b>(x: Cell<&'a i32>) -> Cell<&'b i32> { x }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:51
  |
2 | fn f<'a, 'b>(x: Cell<&'a i32>) -> Cell<&'b i32> { x }
  |      --  -- lifetime `'b` defined here            ^ function was supposed to return data with lifetime `'a` but it is returning data with lifetime `'b`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`
  = note: requirement occurs because of the type `Cell<&i32>`, which makes the generic argument `&i32` invariant
  = note: the struct `Cell<T>` is invariant over the parameter `T`
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance

error: lifetime may not live long enough
 --> <script>:2:51
  |
2 | fn f<'a, 'b>(x: Cell<&'a i32>) -> Cell<&'b i32> { x }
  |      --  -- lifetime `'b` defined here            ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
  = note: requirement occurs because of the type `Cell<&i32>`, which makes the generic argument `&i32` invariant
  = note: the struct `Cell<T>` is invariant over the parameter `T`
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance

help: `'b` and `'a` must be the same: replace one with the other
"#),
    );
    signature_verdict(
        r#"fn f<'a, 'b, 'c>(x: &'a i32, y: &'b i32) -> (&'c i32, &'c i32) { (x, y) }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:66
  |
1 | fn f<'a, 'b, 'c>(x: &'a i32, y: &'b i32) -> (&'c i32, &'c i32) { (x, y) }
  |      --      -- lifetime `'c` defined here                       ^^^^^^ function was supposed to return data with lifetime `'c` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'c`

error: lifetime may not live long enough
 --> <script>:1:66
  |
1 | fn f<'a, 'b, 'c>(x: &'a i32, y: &'b i32) -> (&'c i32, &'c i32) { (x, y) }
  |          --  -- lifetime `'c` defined here                       ^^^^^^ function was supposed to return data with lifetime `'c` but it is returning data with lifetime `'b`
  |          |
  |          lifetime `'b` defined here
  |
  = help: consider adding the following bound: `'b: 'c`

help: the following changes may resolve your lifetime errors
  |
  = help: add bound `'a: 'c`
  = help: add bound `'b: 'c`
"#),
    );
    signature_verdict(
        r#"fn f<'a, 'b, 'c>(x: &'a i32) -> (&'b i32, &'c i32) { (x, x) }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:54
  |
1 | fn f<'a, 'b, 'c>(x: &'a i32) -> (&'b i32, &'c i32) { (x, x) }
  |      --  -- lifetime `'b` defined here               ^^^^^^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    signature_verdict(
        r#"struct S;
impl S { fn g(&self, x: &Vec<&i32>) -> &Vec<&i32> { x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:53
  |
2 | impl S { fn g(&self, x: &Vec<&i32>) -> &Vec<&i32> { x } }
  |               -         -                           ^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
  |               |         |
  |               |         let's call the lifetime of this reference `'1`
  |               let's call the lifetime of this reference `'2`
  |
help: consider introducing a named lifetime parameter and update trait if needed
  |
2 | impl S { fn g<'a>(&self, x: &'a Vec<&'a i32>) -> &'a Vec<&'a i32> { x } }
  |              ++++            ++      ++           ++      ++

error: lifetime may not live long enough
 --> <script>:2:53
  |
2 | impl S { fn g(&self, x: &Vec<&i32>) -> &Vec<&i32> { x } }
  |               -              -                      ^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'3`
  |               |              |
  |               |              let's call the lifetime of this reference `'3`
  |               let's call the lifetime of this reference `'2`
  |
help: consider introducing a named lifetime parameter and update trait if needed
  |
2 | impl S { fn g<'a>(&self, x: &Vec<&'a i32>) -> &'a Vec<&'a i32> { x } }
  |              ++++                 ++           ++      ++
"#),
    );
}

#[test]
fn a_returned_branch_is_marked_where_it_borrows() {
    // rustc marks the branch of an `if` or a `match` the value takes its
    // borrow from (#2488).
    signature_verdict(
        r#"fn f<'a, 'b>(x: Option<&'a i32>, y: &'b i32) -> Option<&'b i32> { if y > &0 { x } else { Some(y) } }
fn main() { let (a, b) = (1, 2); println!("{:?}", f(Some(&a), &b)); }
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:79
  |
1 | fn f<'a, 'b>(x: Option<&'a i32>, y: &'b i32) -> Option<&'b i32> { if y > &0 { x } else { Some(y) } }
  |      --  -- lifetime `'b` defined here                                        ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    signature_verdict(
        r#"fn f<'a, 'b>(x: Option<&'a i32>, y: &'b i32) -> Option<&'b i32> { match y { 0 => Some(y), _ => x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:96
  |
1 | fn f<'a, 'b>(x: Option<&'a i32>, y: &'b i32) -> Option<&'b i32> { match y { 0 => Some(y), _ => x } }
  |      --  -- lifetime `'b` defined here                                                         ^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`
"#),
    );
    signature_verdict(
        r#"fn g<'a, 'b>(x: &'a i32, y: &'b i32, c: bool) -> (&'b i32, &'a i32) { if c { (x, y) } else { (y, x) } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:78
  |
1 | fn g<'a, 'b>(x: &'a i32, y: &'b i32, c: bool) -> (&'b i32, &'a i32) { if c { (x, y) } else { (y, x) } }
  |      --  -- lifetime `'b` defined here                                       ^^^^^^ function was supposed to return data with lifetime `'b` but it is returning data with lifetime `'a`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'a: 'b`

error: lifetime may not live long enough
 --> <script>:1:78
  |
1 | fn g<'a, 'b>(x: &'a i32, y: &'b i32, c: bool) -> (&'b i32, &'a i32) { if c { (x, y) } else { (y, x) } }
  |      --  -- lifetime `'b` defined here                                       ^^^^^^ function was supposed to return data with lifetime `'a` but it is returning data with lifetime `'b`
  |      |
  |      lifetime `'a` defined here
  |
  = help: consider adding the following bound: `'b: 'a`

help: `'a` and `'b` must be the same: replace one with the other
"#),
    );
}

#[test]
fn two_elided_lifetimes_are_given_a_named_one() {
    // rustc suggests one named lifetime for two elided ones: written in the
    // output, or else at the shorter one's reference, and at the longer
    // one's, a new one declared or the function's first reused (#2489).
    // A store through a `&mut` parameter too.
    signature_verdict(
        r#"fn g(v: &mut Vec<&i32>, x: &i32) { v.push(x); }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:1:36
  |
1 | fn g(v: &mut Vec<&i32>, x: &i32) { v.push(x); }
  |                  -         -       ^^^^^^^^^ argument requires that `'1` must outlive `'2`
  |                  |         |
  |                  |         let's call the lifetime of this reference `'1`
  |                  let's call the lifetime of this reference `'2`
  |
  = note: requirement occurs because of a mutable reference to `Vec<&i32>`
  = note: mutable references are invariant over their type parameter
  = help: see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about variance
help: consider introducing a named lifetime parameter
  |
1 | fn g<'a>(v: &mut Vec<&'a i32>, x: &'a i32) { v.push(x); }
  |     ++++              ++           ++
"#),
    );
    signature_verdict(
        r#"struct S;
impl S { fn g(&self, x: &i32) -> &i32 { x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:41
  |
2 | impl S { fn g(&self, x: &i32) -> &i32 { x } }
  |               -         -               ^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
  |               |         |
  |               |         let's call the lifetime of this reference `'1`
  |               let's call the lifetime of this reference `'2`
  |
help: consider introducing a named lifetime parameter and update trait if needed
  |
2 | impl S { fn g<'a>(&self, x: &'a i32) -> &'a i32 { x } }
  |              ++++            ++          ++
"#),
    );
    signature_verdict(
        r#"struct S;
impl S { fn g<T>(&self, x: &i32, _t: T) -> &i32 { x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:51
  |
2 | impl S { fn g<T>(&self, x: &i32, _t: T) -> &i32 { x } }
  |                  -         -                      ^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
  |                  |         |
  |                  |         let's call the lifetime of this reference `'1`
  |                  let's call the lifetime of this reference `'2`
  |
help: consider introducing a named lifetime parameter and update trait if needed
  |
2 | impl S { fn g<'a, T>(&self, x: &'a i32, _t: T) -> &'a i32 { x } }
  |               +++               ++                 ++
"#),
    );
    signature_verdict(
        r#"struct S;
impl S { fn g(&self, x: &'_ i32) -> &'_ i32 { x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:47
  |
2 | impl S { fn g(&self, x: &'_ i32) -> &'_ i32 { x } }
  |               -         -                     ^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
  |               |         |
  |               |         let's call the lifetime of this reference `'1`
  |               let's call the lifetime of this reference `'2`
  |
help: consider introducing a named lifetime parameter and update trait if needed
  |
2 - impl S { fn g(&self, x: &'_ i32) -> &'_ i32 { x } }
2 + impl S { fn g<'a>(&self, x: &'a i32) -> &'a i32 { x } }
  |
"#),
    );
    signature_verdict(
        r#"struct S;
impl S { fn g(&self, x: &Vec<&i32>) -> &i32 { x[0] } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:47
  |
2 | impl S { fn g(&self, x: &Vec<&i32>) -> &i32 { x[0] } }
  |               -              -                ^^^^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
  |               |              |
  |               |              let's call the lifetime of this reference `'1`
  |               let's call the lifetime of this reference `'2`
  |
help: consider introducing a named lifetime parameter and update trait if needed
  |
2 | impl S { fn g<'a>(&self, x: &Vec<&'a i32>) -> &'a i32 { x[0] } }
  |              ++++                 ++           ++
"#),
    );
    signature_verdict(
        r#"struct S;
impl S { fn g<'b>(&self, x: &i32, _y: &'b i32) -> &i32 { x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:58
  |
2 | impl S { fn g<'b>(&self, x: &i32, _y: &'b i32) -> &i32 { x } }
  |                   -         -                            ^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
  |                   |         |
  |                   |         let's call the lifetime of this reference `'1`
  |                   let's call the lifetime of this reference `'2`
  |
help: consider reusing a named lifetime parameter and update trait if needed
  |
2 | impl S { fn g<'b>(&self, x: &'b i32, _y: &'b i32) -> &'b i32 { x } }
  |                              ++                       ++
"#),
    );
    signature_verdict(
        r#"struct S;
impl S { fn g(&self, x: & i32) -> & i32 { x } }
fn main() {}
main();
"#,
        Err(r#"
error: lifetime may not live long enough
 --> <script>:2:43
  |
2 | impl S { fn g(&self, x: & i32) -> & i32 { x } }
  |               -         -                 ^ method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
  |               |         |
  |               |         let's call the lifetime of this reference `'1`
  |               let's call the lifetime of this reference `'2`
  |
help: consider introducing a named lifetime parameter and update trait if needed
  |
2 | impl S { fn g<'a>(&self, x: &'a  i32) -> &'a  i32 { x } }
  |              ++++            ++           ++
"#),
    );
}

#[test]
fn a_use_of_a_declared_name_is_e0255() {
    // rustc's E0255 at the second of a `use` and a declaration of one name
    // (#2497).
    signature_verdict(
        r#"mod a { pub mod inner {} pub use self::inner; }
fn main() {}
main();
"#,
        Err(r#"
error[E0255]: the name `inner` is defined multiple times
 --> <script>:1:34
  |
1 | mod a { pub mod inner {} pub use self::inner; }
  |         -------------            ^^^^^^^^^^^ `inner` reimported here
  |         |
  |         previous definition of the module `inner` here
  |
  = note: `inner` must be defined only once in the type namespace of this module
help: you can use `as` to change the binding name of the import
  |
1 | mod a { pub mod inner {} pub use self::inner as other_inner; }
  |                                              ++++++++++++++
"#),
    );
    signature_verdict(
        r#"mod a { pub use self::inner; pub mod inner {} }
fn main() {}
main();
"#,
        Err(r#"
error[E0255]: the name `inner` is defined multiple times
 --> <script>:1:30
  |
1 | mod a { pub use self::inner; pub mod inner {} }
  |                 -----------  ^^^^^^^^^^^^^ `inner` redefined here
  |                 |
  |                 previous import of the module `inner` here
  |
  = note: `inner` must be defined only once in the type namespace of this module
help: you can use `as` to change the binding name of the import
  |
1 | mod a { pub use self::inner as other_inner; pub mod inner {} }
  |                             ++++++++++++++
"#),
    );
    signature_verdict(
        r#"mod m { pub fn f() {} }
use m::f;
fn f() {}
fn main() {}
main();
"#,
        Err(r#"
error[E0255]: the name `f` is defined multiple times
 --> <script>:3:1
  |
2 | use m::f;
  |     ---- previous import of the value `f` here
3 | fn f() {}
  | ^^^^^^ `f` redefined here
  |
  = note: `f` must be defined only once in the value namespace of this module
help: you can use `as` to change the binding name of the import
  |
2 | use m::f as other_f;
  |          ++++++++++
"#),
    );
    signature_verdict(
        r#"mod m { pub struct S; }
use m::S;
pub struct S<T>(T);
fn main() {}
main();
"#,
        Err(r#"
error[E0255]: the name `S` is defined multiple times
 --> <script>:3:1
  |
2 | use m::S;
  |     ---- previous import of the type `S` here
3 | pub struct S<T>(T);
  | ^^^^^^^^^^^^^^^^^^^ `S` redefined here
  |
  = note: `S` must be defined only once in the type namespace of this module
help: you can use `as` to change the binding name of the import
  |
2 | use m::S as OtherS;
  |          +++++++++
"#),
    );
}

#[test]
fn a_mismatched_argument_is_marked_at_the_argument() {
    // rustc's E0308 at the argument, the callee labelled, where the
    // expectation gives a generic call's parameter its type or the
    // parameter's type is the function's own (#2498).
    signature_verdict(
        r#"trait Shape { fn area(&self) -> f64; }
struct Sq(f64);
struct Ci(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } }
impl Shape for Ci { fn area(&self) -> f64 { self.0 } }
fn id<T: Shape>(t: T) -> T { t }
fn main() { let mut x = id(Sq(1.0)); x = id(Ci(2.0)); println!("{}", x.area()); }
main();
"#,
        Err(r#"
error[E0308]: mismatched types
 --> <script>:7:45
  |
7 | fn main() { let mut x = id(Sq(1.0)); x = id(Ci(2.0)); println!("{}", x.area()); }
  |                                          -- ^^^^^^^ expected `Sq`, found `Ci`
  |                                          |
  |                                          arguments to this function are incorrect
  |
help: the return type of this call is `Ci` due to the type of the argument passed
 --> <script>:7:42
  |
7 | fn main() { let mut x = id(Sq(1.0)); x = id(Ci(2.0)); println!("{}", x.area()); }
  |                                          ^^^-------^
  |                                             |
  |                                             this argument influences the return type of `id`
note: function defined here
 --> <script>:6:4
  |
6 | fn id<T: Shape>(t: T) -> T { t }
  |    ^^           ----
"#),
    );
    signature_verdict(
        r#"struct Sq(f64);
struct Ci(f64);
fn wrap<T>(t: T) -> Vec<T> { vec![t] }
fn main() { let _v: Vec<Sq> = wrap(Ci(2.0)); }
main();
"#,
        Err(r#"
error[E0308]: mismatched types
 --> <script>:4:36
  |
4 | fn main() { let _v: Vec<Sq> = wrap(Ci(2.0)); }
  |                               ---- ^^^^^^^ expected `Sq`, found `Ci`
  |                               |
  |                               arguments to this function are incorrect
  |
help: the return type of this call is `Ci` due to the type of the argument passed
 --> <script>:4:31
  |
4 | fn main() { let _v: Vec<Sq> = wrap(Ci(2.0)); }
  |                               ^^^^^-------^
  |                                    |
  |                                    this argument influences the return type of `wrap`
note: function defined here
 --> <script>:3:4
  |
3 | fn wrap<T>(t: T) -> Vec<T> { vec![t] }
  |    ^^^^    ----
"#),
    );
    signature_verdict(
        r#"struct Sq(f64);
struct Ci(f64);
fn take(t: Sq) -> Sq { t }
fn main() { let _s = take(Ci(2.0)); }
main();
"#,
        Err(r#"
error[E0308]: mismatched types
 --> <script>:4:27
  |
4 | fn main() { let _s = take(Ci(2.0)); }
  |                      ---- ^^^^^^^ expected `Sq`, found `Ci`
  |                      |
  |                      arguments to this function are incorrect
  |
note: function defined here
 --> <script>:3:4
  |
3 | fn take(t: Sq) -> Sq { t }
  |    ^^^^ -----
"#),
    );
}

#[test]
fn an_untyped_closure_adds_at_the_type_its_call_gives() {
    // `a + b` of two closure parameters nothing has typed waits for the
    // call, as rustc's obligation does, and is checked at that kind
    // (#2512). The output is rustc's.
    let output = printed(
        r#"
fn main() {
    let f = |a, b| a + b;
    let g = |a, b| a * b;
    println!("{} {} {}", (|a, b| a + b)(3, 3), f(3, 3), g(2.5, 2.0));
    println!("{} {}", (|a, b| a - b)(7u8, 2), (|a, b| a + b)(1.5f32, 0.1));
}
main();
"#,
    );
    assert_eq!(output, "6 6 5\n5 1.6\n");
    let error = Engine::new()
        .run("fn main() { let f = |a, b| a + b; let x: i8 = f(100i8, 100); println!(\"{}\", x); }\nmain();\n")
        .expect_err("an `i8` sum past 127 panics");
    assert!(
        error.to_string().contains("attempt to add with overflow"),
        "{error}"
    );
}

#[test]
fn a_program_traits_generic_method_through_its_path_takes_its_turbofish() {
    // `Conv::conv::<u16>(&s)` and `<S as Conv>::conv` of a generic method
    // run the impl's body at the call's type (#2513). The output is
    // rustc's.
    let output = printed(
        r#"
struct S(u8);
trait Conv { fn conv<T: From<u8>>(&self) -> T; }
impl Conv for S { fn conv<T: From<u8>>(&self) -> T { T::from(self.0) } }
fn main() {
    let s = S(3);
    let b: f64 = Conv::conv(&s);
    println!("{} {:?} {}", Conv::conv::<u16>(&s), b, <S as Conv>::conv::<u32>(&s));
}
main();
"#,
    );
    assert_eq!(output, "3 3.0 3\n");
}

#[test]
fn a_trait_for_str_is_called_through_its_path_on_a_str() {
    // `Shout::shout("hi")` takes `"hi"` as `&Self` of `impl Shout for str`,
    // as `<str as ToString>::to_string("x")` does std's (#2514). The
    // output is rustc's.
    let output = printed(
        r#"
trait Shout { fn shout(&self) -> String; }
impl Shout for str { fn shout(&self) -> String { self.to_uppercase() } }
impl Shout for i32 { fn shout(&self) -> String { format!("{}!", self) } }
fn main() {
    println!("{} {} {}", Shout::shout("hi"), Shout::shout(&5), <str as Shout>::shout("x"));
    println!("{}", <str as ToString>::to_string("y"));
}
main();
"#,
    );
    assert_eq!(output, "HI 5! X\ny\n");
}

#[test]
fn an_iterator_parameter_with_no_item_written_has_its_methods() {
    // `I: Iterator` with no `Item = T` has `I::Item` as a parameter of its
    // own, so `i.count()` and `i.collect()` are the iterator's (#2507).
    // The output is rustc's.
    let output = printed(
        r#"
use std::fmt::Debug;
fn count<I: Iterator>(i: I) -> usize { i.count() }
fn listed<I>(i: I) -> Vec<I::Item> where I: Iterator, I::Item: Debug { i.collect() }
fn first<I: IntoIterator>(i: I) -> Option<I::Item> { i.into_iter().next() }
fn main() {
    println!("{} {:?} {:?}", count([1, 2].iter()), listed(vec![3, 4].into_iter()), first(vec!["a"]));
}
main();
"#,
    );
    assert_eq!(output, "2 [3, 4] Some(\"a\")\n");
}

#[test]
fn a_method_whose_impl_bounds_fail_exists_but_is_e0599() {
    // rustc's probe finds `show` of `impl<T: Debug> S<T>` and refuses it
    // for `S<Opaque>` (#2504).
    let report = refused(
        r#"use std::fmt::Debug;
struct Opaque;
struct S<T>(T);
impl<T: Debug> S<T> { fn show(&self) { println!("{:?}", self.0); } }
fn main() { S(Opaque).show(); }
main();
"#,
    );
    assert!(
        report.starts_with(
            r#"error[E0599]: the method `show` exists for struct `S<Opaque>`, but its trait bounds were not satisfied
 --> <script>:5:23
  |
2 | struct Opaque;
  | ------------- doesn't satisfy `Opaque: Debug`
3 | struct S<T>(T);
  | ----------- method `show` not found for this struct
4 | impl<T: Debug> S<T> { fn show(&self) { println!("{:?}", self.0); } }
5 | fn main() { S(Opaque).show(); }
  |                       ^^^^ method cannot be called on `S<Opaque>` due to unsatisfied trait bounds
  |
note: trait bound `Opaque: Debug` was not satisfied
 --> <script>:4:9
  |
4 | impl<T: Debug> S<T> { fn show(&self) { println!("{:?}", self.0); } }
  |         ^^^^^  ----
  |         |
  |         unsatisfied trait bound introduced here
help: consider annotating `Opaque` with `#[derive(Debug)]`
"#
        ),
        "{report}"
    );
    // Two bounds of one type are listed together, and the derive names
    // both.
    let report = refused(
        r#"use std::fmt::Debug;
struct Opaque;
struct S<T>(T);
impl<T: Debug + Clone> S<T> { fn show(&self) { println!("{:?}", self.0); } }
fn main() { let s = S(Opaque); let r = &s; r.show(); }
main();
"#,
    );
    assert!(
        report.starts_with(
            r#"error[E0599]: the method `show` exists for reference `&S<Opaque>`, but its trait bounds were not satisfied
 --> <script>:5:46
  |
2 | struct Opaque;
  | ------------- doesn't satisfy `Opaque: Clone` or `Opaque: Debug`
...
5 | fn main() { let s = S(Opaque); let r = &s; r.show(); }
  |                                              ^^^^ method cannot be called on `&S<Opaque>` due to unsatisfied trait bounds
  |
note: the following trait bounds were not satisfied:
      `Opaque: Clone`
      `Opaque: Debug`
 --> <script>:4:9
  |
4 | impl<T: Debug + Clone> S<T> { fn show(&self) { println!("{:?}", self.0); } }
  |         ^^^^^   ^^^^^  ----
  |         |       |
  |         |       unsatisfied trait bound introduced here
  |         unsatisfied trait bound introduced here
help: consider annotating `Opaque` with `#[derive(Clone, Debug)]`
"#
        ),
        "{report}"
    );
}

#[test]
fn a_traits_bounds_on_its_parameters_are_checked_at_its_impls() {
    // `impl Show<Opaque> for A` of `trait Show<T> where T: Debug` is
    // refused at its header, as rustc refuses it (#2505).
    for (bound, column) in [("<T> where T: Debug", 24), ("<T: Debug>", 15)] {
        let report = refused(&format!(
            "use std::fmt::Debug;\nstruct Opaque;\nstruct A;\ntrait Show{bound} {{ fn show(&self) {{}} }}\nimpl Show<Opaque> for A {{}}\nfn main() {{}}\nmain();\n"
        ));
        let expected = format!(
            r#"error[E0277]: `Opaque` doesn't implement `Debug`
 --> <script>:5:6
  |
5 | impl Show<Opaque> for A {{}}
  |      ^^^^^^^^^^^^ the trait `Debug` is not implemented for `Opaque`
  |
  = note: add `#[derive(Debug)]` to `Opaque` or manually `impl Debug for Opaque`
note: required by a bound in `Show`
 --> <script>:4:{column}
"#
        );
        assert!(report.starts_with(&expected), "{report}");
        assert!(
            report.contains("required by this bound in `Show`\nhelp: consider annotating `Opaque` with `#[derive(Debug)]`\n"),
            "{report}"
        );
    }
}

#[test]
fn a_borrow_of_a_block_tail_temporary_kept_by_a_let_is_e0716() {
    // A block tail's temporaries end with the block, as edition 2024 has
    // it, so the `let` can't keep a borrow of one (#159).
    for program in [
        "fn main() { let r = { String::from(\"a\").as_str() }; println!(\"{r}\"); }\nmain();",
        "fn main() { let v = vec![String::from(\"a\")]; let r: &str = { v.clone()[0].as_str() }; println!(\"{r}\"); }\nmain();",
    ] {
        let report = refused(program);
        assert!(
            report.starts_with("error[E0716]: temporary value dropped while borrowed"),
            "{report}"
        );
    }
    // A borrow the tail extends lives on with the `let`.
    let output = printed(
        r#"
fn main() {
    let r = { &String::from("a") };
    let s: &str = { let _t = String::from("b"); &String::from("c") };
    println!("{r} {s}");
}
main();
"#,
    );
    assert_eq!(output, "a c\n");
}

#[test]
fn a_block_or_a_box_keeps_the_borrows_of_its_value() {
    // The value of a block is its tail's and a `Box`'s is what it boxes,
    // with the borrows they hold (#147).
    for value in [
        "{ let c = || &v; c() }",
        "Box::new(|| v.len())",
        "Box::new(&v)",
    ] {
        let report = refused(&format!(
            "fn main() {{ let mut v = vec![1]; let r = {value}; v.push(2); let _ = &r; }}\nmain();"
        ));
        assert!(
            report.starts_with(
                "error[E0502]: cannot borrow `v` as mutable because it is also borrowed as \
                 immutable"
            ),
            "{value}: {report}"
        );
    }
    let report = refused(
        r#"
fn make() -> Box<dyn Fn() -> usize> {
    let v = vec![1];
    Box::new(|| v.len())
}
fn main() { println!("{}", make()()); }
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0373]: closure may outlive the current function, but it borrows `v`, which \
             is owned by the current function"
        ),
        "{report}"
    );
    let output = printed(
        r#"
fn main() {
    let mut v = vec![1, 2];
    let n = { let a = &v; a.len() };
    v.push(3);
    let r = { let a = &v; a };
    let b = Box::new(&v);
    println!("{n} {r:?} {}", b.len());
}
main();
"#,
    );
    assert_eq!(output, "2 [1, 2, 3] 3\n");
}

#[test]
fn a_move_out_of_a_static_is_e0507() {
    // A static is a place, which no use moves out of (#192).
    let program = |main: &str| {
        format!(
            "struct Pair {{ name: String, n: i32 }}\nstatic PAIR: Pair = Pair {{ name: String::new(), n: 2 }};\nstatic NAME: String = String::new();\nfn take(s: String) -> usize {{ s.len() }}\nfn main() {{ {main} }}\nmain();"
        )
    };
    for (main, message) in [
        ("take(NAME);", "cannot move out of static item `NAME`"),
        ("let s = NAME;", "cannot move out of static item `NAME`"),
        (
            "let s = PAIR.name;",
            "cannot move out of `PAIR.name` as `PAIR` is a static item",
        ),
        (
            "static LOCAL: String = String::new(); let s = LOCAL;",
            "cannot move out of static item `LOCAL`",
        ),
    ] {
        let report = refused(&program(main));
        assert!(
            report.starts_with(&format!("error[E0507]: {message}")),
            "{main}: {report}"
        );
    }
    let output = printed(&program(
        "let n = PAIR.n; let r = &PAIR.name; let c = PAIR.name.clone(); println!(\"{n} [{r}] [{c}] {}\", NAME.len());",
    ));
    assert_eq!(output, "2 [] [] 0\n");
}

#[test]
fn a_static_of_a_type_that_is_not_sync_is_e0277() {
    // Every thread shares a static, so its type is `Sync` (#1563).
    let report = refused(
        "use std::cell::RefCell;\nstatic HITS: RefCell<i32> = RefCell::new(0);\nfn main() {}\nmain();",
    );
    assert_eq!(
        report,
        r#"error[E0277]: `RefCell<i32>` cannot be shared between threads safely
 --> <script>:2:14
  |
2 | static HITS: RefCell<i32> = RefCell::new(0);
  |              ^^^^^^^^^^^^ `RefCell<i32>` cannot be shared between threads safely
  |
  = help: the trait `Sync` is not implemented for `RefCell<i32>`
  = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` instead
  = note: shared static variables must have a type that implements `Sync`"#
    );
    let report = refused(
        "use std::cell::Cell;\nfn main() {\n    static C: Cell<i32> = Cell::new(0);\n    C.set(1);\n}\nmain();",
    );
    assert_eq!(
        report,
        r#"error[E0277]: `Cell<i32>` cannot be shared between threads safely
 --> <script>:3:15
  |
3 |     static C: Cell<i32> = Cell::new(0);
  |               ^^^^^^^^^ `Cell<i32>` cannot be shared between threads safely
  |
  = help: the trait `Sync` is not implemented for `Cell<i32>`
  = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` or `std::sync::atomic::AtomicI32` instead
  = note: shared static variables must have a type that implements `Sync`"#
    );
    let report = refused(
        r#"use std::cell::Cell;
struct Inner {
    c: Cell<f64>,
}
struct Outer {
    n: i32,
    inner: Inner,
}
static O: Outer = Outer { n: 1, inner: Inner { c: Cell::new(0.0) } };
static E: (i32, Cell<u8>) = (1, Cell::new(0));
fn main() {}
main();"#,
    );
    assert_eq!(
        report,
        r#"error[E0277]: `Cell<f64>` cannot be shared between threads safely
 --> <script>:9:11
  |
9 | static O: Outer = Outer { n: 1, inner: Inner { c: Cell::new(0.0) } };
  |           ^^^^^ `Cell<f64>` cannot be shared between threads safely
  |
  = help: within `Outer`, the trait `Sync` is not implemented for `Cell<f64>`
  = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock`
note: required because it appears within the type `Inner`
 --> <script>:2:8
  |
2 | struct Inner {
  |        ^^^^^
note: required because it appears within the type `Outer`
 --> <script>:5:8
  |
5 | struct Outer {
  |        ^^^^^
  = note: shared static variables must have a type that implements `Sync`"#
    );
    let report = refused(
        "use std::cell::Cell;\nstatic E: (i32, Cell<u8>) = (1, Cell::new(0));\nfn main() {}\nmain();",
    );
    assert_eq!(
        report,
        r#"error[E0277]: `Cell<u8>` cannot be shared between threads safely
 --> <script>:2:11
  |
2 | static E: (i32, Cell<u8>) = (1, Cell::new(0));
  |           ^^^^^^^^^^^^^^^ `Cell<u8>` cannot be shared between threads safely
  |
  = help: within `(i32, Cell<u8>)`, the trait `Sync` is not implemented for `Cell<u8>`
  = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` or `std::sync::atomic::AtomicU8` instead
  = note: required because it appears within the type `(i32, Cell<u8>)`
  = note: shared static variables must have a type that implements `Sync`"#
    );
    // A `Mutex` is `Sync`.
    let output = printed(
        "use std::sync::Mutex;\nstatic M: Mutex<i32> = Mutex::new(4);\nfn main() { println!(\"{}\", *M.lock().unwrap()); }\nmain();",
    );
    assert_eq!(output, "4\n");
}

#[test]
fn temporaries_drop_where_rustc_2024_drops_them() {
    // A temporary drops at the end of its statement, a block tail's before
    // the block's locals, a condition's before the body, an `if let`
    // subject's before the `else`, and one a `let` extends with the block
    // (#870).
    let output = printed(
        r#"
struct D(&'static str);
impl Drop for D {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}
impl D {
    fn label(&self) -> &'static str {
        self.0
    }
    fn again(&self) -> D {
        D("again")
    }
    fn ok(&self) -> bool {
        true
    }
}
struct W<'a>(&'a D);
fn main() {
    let a = D("a");
    println!("{}", a.again().label());
    let n = D("let").label().len();
    println!("after let {n}");
    let x = {
        let _l = D("local");
        D("tail").label().len()
    };
    println!("after block {x}");
    if D("cond").ok() {
        println!("if body");
    }
    let mut once = true;
    while D("while").ok() && once {
        once = false;
        println!("while body");
    }
    match D("scrutinee").ok() {
        _ => println!("match arm"),
    }
    if let false = D("if let").ok() {
        println!("if let body");
    } else {
        println!("if let else");
    }
    D("statement").ok();
    let r = &D("extended");
    let s = Some(&D("some"));
    let w = W(&D("w"));
    let t = (&D("t1"), D("t2"));
    println!("end {} {} {} {}", r.0, s.unwrap().0, (w.0).0, (t.0).0);
}
main();
"#,
    );
    assert_eq!(
        output,
        "again\ndrop again\ndrop let\nafter let 3\ndrop tail\ndrop local\nafter block 4\n\
         drop cond\nif body\ndrop while\nwhile body\ndrop while\nmatch arm\ndrop scrutinee\n\
         drop if let\nif let else\ndrop statement\nend extended some w t1\ndrop t2\ndrop t1\n\
         drop w\ndrop some\ndrop extended\ndrop a\n"
    );
    // A `while let` subject's temporaries drop with each iteration, and at
    // a jump out of the body.
    let output = printed(
        r#"
struct D(&'static str);
impl Drop for D {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}
impl D {
    fn pick(&self, n: i32) -> Option<i32> {
        if n < 3 { Some(n) } else { None }
    }
}
fn early(n: i32) -> i32 {
    if let Some(k) = D("early").pick(n) {
        if k == 0 {
            return 10;
        }
        println!("past {k}");
    }
    n
}
fn main() {
    let mut n = 0;
    while let Some(k) = D("loop").pick(n) {
        n += 1;
        if k == 0 {
            continue;
        }
        println!("body {k}");
        if k == 2 {
            break;
        }
    }
    println!("{} {}", early(0), early(1));
}
main();
"#,
    );
    assert_eq!(
        output,
        "drop loop\nbody 1\ndrop loop\nbody 2\ndrop loop\ndrop early\npast 1\ndrop early\n10 1\n"
    );
}

#[test]
fn an_if_let_subject_temporary_ends_before_the_else() {
    // A temporary of an `if let` subject whose drop uses a borrow holds it
    // through the consequent and not the alternative, as edition 2024 has
    // it (#870).
    let program = |consequent: &str, alternative: &str| {
        format!(
            "struct G<'a>(&'a Vec<i32>);\nimpl Drop for G<'_> {{ fn drop(&mut self) {{}} }}\nimpl G<'_> {{ fn first(&self) -> Option<i32> {{ self.0.first().copied() }} }}\nfn main() {{ let mut v: Vec<i32> = Vec::new(); if let Some(x) = G(&v).first() {{ {consequent} }} else {{ {alternative} }} println!(\"{{v:?}}\"); }}\nmain();"
        )
    };
    assert_eq!(
        printed(&program("println!(\"{x}\");", "v.push(1);")),
        "[1]\n"
    );
    let report = refused(&program("v.push(x);", "println!(\"none\");"));
    assert!(
        report.starts_with(
            "error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable"
        ),
        "{report}"
    );
}

#[test]
fn a_static_is_one_value_its_first_read_makes_and_every_read_shares() {
    // A program's static and a body's, of a `Mutex` or of a struct holding
    // one, keep what each call and each thread puts in them, and a copy of
    // a static changes apart from it (#2519). The output is rustc's.
    let output = printed(
        r#"
use std::sync::Mutex;
static HITS: Mutex<Vec<i32>> = Mutex::new(Vec::new());
struct Log { lines: Mutex<Vec<&'static str>> }
static LOG: Log = Log { lines: Mutex::new(Vec::new()) };
static FIRST: Option<i32> = Some(1);
fn hit(n: i32) { HITS.lock().unwrap().push(n); }
fn tick() -> i32 {
    static TICKS: Mutex<i32> = Mutex::new(0);
    let mut ticks = TICKS.lock().unwrap();
    *ticks += 1;
    *ticks
}
fn main() {
    hit(1);
    hit(2);
    std::thread::spawn(|| hit(3)).join().unwrap();
    LOG.lines.lock().unwrap().push("a");
    LOG.lines.lock().unwrap().push("b");
    let mut first = FIRST;
    if let Some(n) = first.as_mut() { *n += 1; }
    println!("hits {:?}", HITS.lock().unwrap());
    println!("{:?} {} {} {}", LOG.lines.lock().unwrap(), tick(), tick(), tick());
    println!("{:?} {:?}", first, FIRST);
}
main();
"#,
    );
    assert_eq!(
        output,
        "hits [1, 2, 3]\n[\"a\", \"b\"] 1 2 3\nSome(2) Some(1)\n"
    );
}

#[test]
fn a_static_keeps_its_value_across_generations_until_its_initializer_changes() {
    // Each run is a generation of one program: a static's value is kept by
    // its path, type and initializer, a body static's by its site too, so
    // an edit of a function keeps both, and an edit of the initializer
    // starts a new value (#2519, D862).
    let generation = |push: i32, initializer: &str| {
        printed(&format!(
            r#"
use std::sync::Mutex;
static GENERATIONS: Mutex<Vec<i32>> = {initializer};
fn record() -> i32 {{
    static CALLS: Mutex<i32> = Mutex::new(0);
    *CALLS.lock().unwrap() += 1;
    GENERATIONS.lock().unwrap().push({push});
    *CALLS.lock().unwrap()
}}
fn main() {{ let calls = record(); println!("{{:?}} {{}}", GENERATIONS.lock().unwrap(), calls); }}
main();
"#
        ))
    };
    assert_eq!(generation(1, "Mutex::new(Vec::new())"), "[1] 1\n");
    assert_eq!(generation(2, "Mutex::new(Vec::new())"), "[1, 2] 2\n");
    assert_eq!(generation(3, "Mutex::new(vec![0])"), "[0, 3] 3\n");
}

#[test]
fn a_closure_that_changes_self_reaches_self_where_it_is_kept() {
    // A closure kept in a local, called later or nested, changes the
    // receiver's fields where they are, as rustc's capture of them
    // borrows them (#189, D1113).
    let output = printed(
        r#"
struct S { a: i32, b: Vec<i32>, items: Vec<i32> }
impl S {
    fn total(&mut self) {
        self.items.iter().for_each(|x| self.a += x);
    }
    fn run(&mut self) -> usize {
        let mut add = |x: i32| { self.b.push(x); self.b.len() };
        add(4);
        let n = add(5);
        self.a += 1;
        n
    }
    fn nested(&mut self) {
        let mut f = |k: i32| { let mut g = |j: i32| self.b.push(j * k); g(1); g(2); };
        f(10);
    }
}
fn fill(v: &mut Vec<i32>) {
    let mut add = |x: i32| v.push(x);
    add(7);
    add(8);
}
fn main() {
    let mut s = S { a: 0, b: vec![], items: vec![1, 2, 3] };
    s.total();
    let n = s.run();
    s.nested();
    let mut v = vec![];
    fill(&mut v);
    let mut list = vec![S { a: 0, b: vec![], items: vec![5] }];
    list[0].total();
    list[0].run();
    println!("{} {} {:?} {:?} {} {:?}", s.a, n, s.b, v, list[0].a, list[0].b);
}
main();
"#,
    );
    assert_eq!(output, "7 2 [4, 5, 10, 20] [7, 8] 6 [4, 5]\n");
}

#[test]
fn a_mut_local_moves_where_no_reference_is_expected() {
    // rustc moves a `&mut` local given whole where the expected type is
    // no reference: `Some(r)`, a tuple, a generic field, a `let s;` of no
    // type (#2502).
    for (shape, holder) in [
        ("", "let o = Some(r);"),
        ("", "let o = (r, 2);"),
        ("struct G<T>(T);", "let o = G(r);"),
        ("struct N<T> { t: T }", "let o = N { t: r };"),
        ("", "let o; o = r;"),
    ] {
        let report = refused(&format!(
            "{shape}\nfn main() {{ let mut x = 1; let r = &mut x; {holder} *r += 1; drop(o); }}\nmain();\n"
        ));
        assert!(
            report.starts_with("error[E0382]: use of moved value: `r`"),
            "{holder}: {report}"
        );
    }
    // Where a reference is expected, `r` is reborrowed, and a `let s;`
    // given a `&mut` reaches its place.
    let output = printed(
        r#"
struct G<T>(T);
fn take(o: Option<&mut i32>) { if let Some(m) = o { *m += 1; } }
fn main() {
    let mut x = 1;
    let r = &mut x;
    take(Some(r));
    let t: (&mut i32, i32) = (r, 2);
    *t.0 += 1;
    let o = Some::<&mut i32>(r);
    drop(o);
    let s: &mut i32;
    s = r;
    *s += 1;
    *r += 1;
    let mut y = 1;
    let g = G(&mut y);
    *g.0 += 1;
    let mut z = 1;
    let u;
    u = &mut z;
    *u += 5;
    println!("{} {} {}", x, y, z);
}
main();
"#,
    );
    assert_eq!(output, "5 2 6\n");
}

#[test]
fn a_mut_parameter_a_function_keeps_is_the_callers_place() {
    // A `&mut` parameter whose lifetime the output, another parameter or
    // the impl names is lent as the reference to the caller's place, so
    // the value keeping it reaches that place; two of one local's parts
    // stay apart (D1113).
    let output = printed(
        r#"
struct S<'a> { a: &'a mut i32 }
impl<'a> S<'a> {
    fn new(a: &'a mut i32) -> S<'a> { S { a } }
    fn inc(&mut self) { *self.a += 1; }
}
fn pair<'a>(n: &mut i32, a: &'a mut i32) -> S<'a> { *n += 1; S { a } }
struct T<'a> { x: &'a mut i32, y: &'a mut i32 }
impl<'a> T<'a> {
    fn new(x: &'a mut i32, y: &'a mut i32) -> T<'a> { T { x, y } }
    fn bump(&mut self) { *self.x += 1; *self.y += 2; }
}
struct P { l: i32, r: i32 }
struct Bag<T> { items: Vec<T> }
impl<T> Bag<T> {
    fn add(&mut self, t: T) { self.items.push(t); }
}
fn keep<T>(t: T) -> Vec<T> { vec![t] }
fn main() {
    let mut x = 1;
    let mut s = S::new(&mut x);
    s.inc();
    s.inc();
    println!("{}", x);
    let mut p = (1, 2);
    let t = pair(&mut p.0, &mut p.1);
    *t.a += 10;
    println!("{:?}", p);
    let mut q = P { l: 1, r: 2 };
    let mut u = T::new(&mut q.l, &mut q.r);
    u.bump();
    std::mem::swap(u.x, u.y);
    u.bump();
    println!("{} {}", q.l, q.r);
    let mut a = 1;
    let mut b = 2;
    let mut bag = Bag { items: Vec::new() };
    bag.add(&mut a);
    bag.add(&mut b);
    for e in bag.items { *e += 10; }
    let mut c = 1;
    let r = &mut c;
    for e in keep(r) { *e += 4; }
    println!("{} {} {}", a, b, c);
}
main();
"#,
    );
    assert_eq!(output, "3\n(2, 12)\n5 4\n11 12 5\n");
}

#[test]
fn an_option_of_a_mut_given_or_matched_reaches_its_place() {
    // `f(v.get_mut(i))` gives the callee `Some` of the element's place,
    // and `if let Some(m) = &mut o` of an `Option<&mut T>` reaches it
    // through `**m`.
    let output = printed(
        r#"
fn bump(o: Option<&mut i32>) { if let Some(r) = o { *r += 100; } }
fn main() {
    let mut v = vec![1, 2, 3];
    bump(v.get_mut(1));
    bump(v.last_mut());
    bump(v.get_mut(9));
    println!("{:?}", v);
    let mut x = 1;
    let mut o: Option<&mut i32> = Some(&mut x);
    if let Some(m) = &mut o { **m += 1; }
    if let Some(m) = o.as_mut() { **m += 1; }
    match &mut o { Some(m) => **m += 10, None => {} }
    println!("{}", x);
}
main();
"#,
    );
    assert_eq!(output, "[1, 102, 103]\n13\n");
}

#[test]
fn a_derived_hash_of_a_mut_field_hashes_what_it_refers_to() {
    // rustc derives `Hash` for a `&mut T` field from `T`.
    let output = printed(
        r#"
use std::collections::HashSet;
#[derive(Hash, PartialEq, Eq, Debug)]
struct W<'a> { a: &'a mut i32, n: u8 }
fn main() {
    let mut p = (3, 3, 4);
    let mut set = HashSet::new();
    set.insert(W { a: &mut p.0, n: 1 });
    set.insert(W { a: &mut p.1, n: 1 });
    set.insert(W { a: &mut p.2, n: 1 });
    let mut x = 5;
    let mut y = 5;
    let mut t = HashSet::new();
    t.insert(W { a: &mut x, n: 2 });
    println!("{} {}", set.len(), t.contains(&W { a: &mut y, n: 2 }));
}
main();
"#,
    );
    assert_eq!(output, "2 true\n");
}

#[test]
fn a_swap_releases_the_value_of_a_static_the_next_generation_declares_otherwise() {
    // A swap to a generation that declares a static alike keeps its value.
    // One whose initializer changed holds no value while the swap is
    // checked, and drops the old one once admitted; a refused swap puts it
    // back (#2519, D862).
    use crate::place::statics::{apart, kept, After};
    let program = |initializer: &str| {
        format!(
            "use std::sync::Mutex;\nstatic SWAPPED: Mutex<Vec<i32>> = {initializer};\n\
             fn main() {{ SWAPPED.lock().unwrap().push(1); println!(\"{{:?}}\", SWAPPED.lock().unwrap()); }}\nmain();\n"
        )
    };
    let alike = program("Mutex::new(Vec::new())");
    let edited = Engine::new()
        .compile_executable(program("Mutex::new(vec![0])"))
        .unwrap_or_else(|error| panic!("{error}"));
    let same = Engine::new()
        .compile_executable(&alike)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(printed(&alike), "[1]\n");
    let ours = same.kept_statics();
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert!(edited.kept_statics().is_disjoint(&ours));
    // The statics of the tests running beside this one stay declared.
    let declaring = |declared: std::collections::BTreeSet<crate::ImmutableString>| {
        let mut others = kept();
        others.retain(|key| !ours.contains(key));
        others.extend(declared);
        others
    };
    apart(|| declaring(same.kept_statics()), || ((), After::Release));
    assert_eq!(printed(&alike), "[1, 1]\n");
    let held = apart(
        || declaring(edited.kept_statics()),
        || (kept().is_superset(&ours), After::Restore),
    );
    assert!(held, "a withdrawn static keeps its slot, without its value");
    assert_eq!(printed(&alike), "[1, 1, 1]\n");
    apart(|| declaring(edited.kept_statics()), || ((), After::Release));
    assert!(kept().is_disjoint(&ours));
    assert_eq!(printed(&alike), "[1]\n");
}

#[test]
fn std_errors_box_walk_their_sources_and_downcast() {
    // `?` boxes io, parse and the program's own errors as `dyn Error`,
    // which keeps each error, its type and its `source`: a chain walks,
    // `downcast_ref`, `is` and a box's `downcast` give the error back, and
    // a `&e` is a `&dyn Error` (#798, #2091). A method's tail `match`
    // coerces each arm to its output, and `let n: i32 = s.parse()?`
    // converts the parse's error by `From`.
    let output = printed(
        r#"
use std::error::Error;
use std::fmt;
use std::io;
use std::num::ParseIntError;
#[derive(Debug)]
enum AppError {
    Io(io::Error),
    Parse(ParseIntError),
    Config(String),
}
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(_) => write!(f, "io failed"),
            AppError::Parse(_) => write!(f, "parse failed"),
            AppError::Config(what) => write!(f, "bad config: {what}"),
        }
    }
}
impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Io(e) => Some(e),
            AppError::Parse(e) => Some(e),
            AppError::Config(_) => None,
        }
    }
}
impl From<ParseIntError> for AppError {
    fn from(e: ParseIntError) -> Self {
        AppError::Parse(e)
    }
}
fn read_number(text: &str) -> Result<i32, AppError> {
    let n: i32 = text.trim().parse()?;
    if n < 0 {
        return Err(AppError::Config(format!("{n} is negative")));
    }
    Ok(n)
}
fn report(e: &dyn Error) {
    print!("{e}");
    let mut cause = e.source();
    while let Some(c) = cause {
        print!(" <- {c}");
        cause = c.source();
    }
    println!();
}
fn run(step: u8) -> Result<i32, Box<dyn Error>> {
    match step {
        0 => Ok(read_number("x1")?),
        1 => Err(Box::new(AppError::Io(io::Error::new(io::ErrorKind::NotFound, "gone")))),
        2 => Ok(read_number("-4")?),
        3 => Ok("zz".parse::<i32>()?),
        4 => Err(Box::new(io::Error::other("raw io"))),
        _ => Err("plain text".into()),
    }
}
fn main() {
    for step in 0..6 {
        let e = run(step).unwrap_err();
        report(e.as_ref());
        println!("{:?}", e);
        if let Some(io) = e.downcast_ref::<io::Error>() {
            println!("io {:?}", io.kind());
        }
        if let Some(p) = e.downcast_ref::<ParseIntError>() {
            println!("parse {p}");
        }
        println!("{} {}", e.is::<AppError>(), e.is::<io::Error>());
        match e.downcast::<AppError>() {
            Ok(app) => println!("app {app}"),
            Err(other) => println!("other {other}"),
        }
    }
    report(&AppError::Parse("q".parse::<u8>().unwrap_err()));
    println!("{:?}", read_number("7"));
}
main();
"#,
    );
    assert_eq!(
        output,
        "parse failed <- invalid digit found in string\n\
         Parse(ParseIntError { kind: InvalidDigit })\ntrue false\napp parse failed\n\
         io failed <- gone\nIo(Custom { kind: NotFound, error: \"gone\" })\ntrue false\n\
         app io failed\n\
         bad config: -4 is negative\nConfig(\"-4 is negative\")\ntrue false\n\
         app bad config: -4 is negative\n\
         invalid digit found in string\nParseIntError { kind: InvalidDigit }\n\
         parse invalid digit found in string\nfalse false\n\
         other invalid digit found in string\n\
         raw io\nCustom { kind: Other, error: \"raw io\" }\nio Other\nfalse true\n\
         other raw io\n\
         plain text\n\"plain text\"\nfalse false\nother plain text\n\
         parse failed <- invalid digit found in string\nOk(7)\n"
    );
    // A `dyn Error`'s downcasts take an error type, as rustc's `T: Error`.
    let report = refused(
        "use std::error::Error;\n\
         fn main() { let e: Box<dyn Error> = \"x\".into(); println!(\"{}\", e.is::<String>()); }",
    );
    assert!(
        report.starts_with(
            "error[E0277]: the trait bound `String: std::error::Error` is not satisfied"
        ),
        "{report}"
    );
}

#[test]
fn a_generic_error_boxes_by_its_bound() {
    // `E: Error` is `Debug` and `Display` through `Error`'s supertraits,
    // so a generic wrapper implements `Error` and lends its error as a
    // `&dyn Error`. An `Arc` of an error is an `Arc<dyn Error>`.
    let output = printed(
        r#"
use std::error::Error;
use std::fmt;
#[derive(Debug)]
struct Context<E> { what: &'static str, inner: E }
impl<E: fmt::Display> fmt::Display for Context<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "{}", self.what) }
}
impl<E: Error + 'static> Error for Context<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> { Some(&self.inner) }
}
fn depth<E: Error>(e: &E) -> usize {
    let mut n = 0;
    let mut cause = e.source();
    while let Some(c) = cause { n += 1; cause = c.source(); }
    n
}
fn main() {
    let inner = "x".parse::<i32>().unwrap_err();
    let e: Box<dyn Error> = Box::new(Context { what: "loading", inner: inner.clone() });
    println!("{} <- {}", e, e.source().unwrap());
    println!("{}", depth(&Context { what: "w", inner }));
    let shared: std::sync::Arc<dyn Error + Send + Sync> =
        std::sync::Arc::new(std::io::Error::other("shared"));
    println!("{} {}", shared, std::sync::Arc::clone(&shared).to_string());
}
main();
"#,
    );
    assert_eq!(
        output,
        "loading <- invalid digit found in string\n1\nshared shared\n"
    );
}

#[test]
fn dyn_any_downcasts_by_the_values_type() {
    // A value boxed or lent as `dyn Any` keeps its type, which `is`,
    // `downcast_ref` and a box's `downcast` compare (#1284).
    let output = printed(
        r#"
use std::any::Any;
#[derive(Debug)]
struct Point { x: i32 }
fn describe(value: &dyn Any) -> String {
    if let Some(n) = value.downcast_ref::<i32>() {
        format!("i32 {n}")
    } else if let Some(s) = value.downcast_ref::<String>() {
        format!("string {s}")
    } else if let Some(p) = value.downcast_ref::<Point>() {
        format!("point {}", p.x)
    } else if value.is::<&str>() {
        "a str".to_string()
    } else {
        "unknown".to_string()
    }
}
fn main() {
    let items: Vec<Box<dyn Any>> = vec![
        Box::new(7i32),
        Box::new(String::from("hi")),
        Box::new(Point { x: 3 }),
        Box::new("slice"),
        Box::new(2.5f64),
    ];
    for item in &items {
        println!("{}", describe(item.as_ref()));
    }
    println!("{}", describe(&41));
    let sent: Box<dyn Any + Send> = Box::new(Point { x: 9 });
    match sent.downcast::<Point>() {
        Ok(p) => println!("got {:?}", p),
        Err(_) => println!("not a point"),
    }
    let other: Box<dyn Any> = Box::new(1u8);
    match other.downcast::<i32>() {
        Ok(_) => println!("i32"),
        Err(back) => println!("still any: {}", back.is::<u8>()),
    }
    println!("{:?}", items[0]);
}
main();
"#,
    );
    assert_eq!(
        output,
        "i32 7\nstring hi\npoint 3\na str\nunknown\ni32 41\ngot Point { x: 9 }\n\
         still any: true\nAny { .. }\n"
    );
}

#[test]
fn std_env_var_and_exit_code_run() {
    // A program without the environment reads no variable, as `env!` has
    // none: `VarError::NotPresent`, which boxes as a `dyn Error` (#645).
    // An `ExitCode` is std's value (#2088).
    let output = printed(
        r#"
use std::env::{self, VarError};
use std::error::Error;
use std::process::ExitCode;
fn need() -> Result<String, Box<dyn Error>> {
    Ok(env::var("RAWR_UNSET")?)
}
fn main() {
    match env::var("HOME") {
        Ok(v) => println!("set {v}"),
        Err(VarError::NotPresent) => println!("not present"),
        Err(VarError::NotUnicode(s)) => println!("bad {:?}", s),
    }
    let e = env::var("HOME").unwrap_err();
    println!("{e} | {e:?} | {}", e == VarError::NotPresent);
    println!("{:?} {:?}", env::var_os("HOME"), need().map_err(|e| e.to_string()));
    let code = ExitCode::from(4);
    let into: ExitCode = 9u8.into();
    println!("{:?} {:?} {}", code, into, ExitCode::SUCCESS == ExitCode::from(0));
}
main();
"#,
    );
    assert_eq!(
        output,
        "not present\nenvironment variable not found | NotPresent | true\n\
         None Err(\"environment variable not found\")\n\
         ExitCode(unix_exit_status(4)) ExitCode(unix_exit_status(9)) true\n"
    );
}

/// What `source` prints, run with `access`.
fn printed_with(source: &str, access: crate::Access) -> String {
    let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut engine = Engine::new();
    engine.set_access(access);
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    engine.run(source).unwrap_or_else(|error| panic!("{error}"));
    lines.lock().unwrap().clone()
}

#[test]
fn str_matches_match_indices_and_replacen_run_as_stds() {
    // A `&str` or `char` pattern's matches, their byte offsets, and the
    // first `n` replaced (#2090).
    let output = printed(
        r#"
fn main() {
    let t = "abcXXabcYYabc";
    let v: Vec<&str> = t.matches("abc").collect();
    let w: Vec<(usize, &str)> = t.match_indices("abc").collect();
    println!("{:?} {:?}", v, w);
    println!("{} {} {}", t.replacen("abc", "-", 2), t.replacen('X', "", 1), t.matches('Y').count());
    let owned = String::from("a,b,,c");
    let at: Vec<usize> = owned.match_indices(',').map(|(i, _)| i).collect();
    println!("{:?} {}", at, owned.replacen(",", ";", 0));
}
main();
"#,
    );
    assert_eq!(
        output,
        "[\"abc\", \"abc\", \"abc\"] [(0, \"abc\"), (5, \"abc\"), (10, \"abc\")]\n\
         -XX-YYabc abcXabcYYabc 2\n[1, 3, 4] a,b,,c\n"
    );
}

#[test]
fn string_drain_and_replace_range_change_the_string_as_stds() {
    // std's `String::drain` and `replace_range` of a `RangeBounds<usize>`
    // (#2521).
    let output = printed(
        r#"
fn main() {
    let mut s = String::from("hello");
    s.replace_range(..2, "x");
    let mut h = String::from("hello he");
    let d: String = h.drain(..2).collect();
    println!("{} {} {}", s, h, d);
    let mut t = String::from("abcdef");
    let r = 1..=2;
    t.replace_range(r, "XYZ");
    let tail: Vec<char> = t.drain(4..).collect();
    println!("{} {:?}", t, tail);
    t.replace_range(.., "all");
    println!("{}", t);
}
main();
"#,
    );
    assert_eq!(output, "xllo llo he he\naXYZ ['d', 'e', 'f']\nall\n");
    // A field's and an element's string change in place.
    let output = printed(
        r#"
struct Named { name: String }
fn main() {
    let mut n = Named { name: String::from("héllo") };
    let gone: String = n.name.drain(1..3).collect();
    n.name.replace_range(0..1, "J");
    let mut v = vec![String::from("abc")];
    v[0].replace_range(1.., "Z");
    println!("{} {} {:?}", n.name, gone, v);
}
main();
"#,
    );
    assert_eq!(output, "Jllo é [\"aZ\"]\n");
    assert_eq!(
        panicked("fn main() {\n    let mut s = String::from(\"héllo\");\n    s.drain(..2);\n}\n"),
        "thread 'main' panicked at main.rs:3:7:\nassertion failed: self.is_char_boundary(end)"
    );
    let report = refused("fn main() { let s = String::from(\"hello\"); s.drain(..2); }");
    assert!(
        report.starts_with(
            "error[E0596]: cannot borrow `s` as mutable, as it is not declared as mutable"
        ),
        "{report}"
    );
}

#[test]
fn std_path_and_path_buf_methods_run_as_stds() {
    // `Path::new`, its readers and file system queries, and `PathBuf`'s
    // changes (#2087). A path the program may not read is no file.
    let directory = std::env::temp_dir().join(format!("rawr-path-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("bar.txt"), "x").unwrap();
    let source = format!(
        r#"
use std::path::{{Path, PathBuf}};
fn main() {{
    let p = Path::new("/tmp/foo/bar.txt");
    println!("{{:?}} {{:?}} {{:?}} {{:?}}", p.extension(), p.file_name(), p.file_stem(), p.parent());
    let j = p.parent().unwrap().join("baz.rs");
    println!("{{}} {{}} {{}}", p.display(), j.display(), p.with_extension("md").display());
    let mut b = PathBuf::from("/tmp");
    b.push("x.txt");
    println!("{{}} {{:?}}", b.set_extension("md"), b);
    b.set_file_name("y");
    println!("{{:?}}", b);
    let popped = (b.pop(), b.pop());
    println!("{{:?}} {{:?}}", popped, b);
    let q: &Path = b.as_path();
    println!("{{}} {{}} {{}}", q.is_absolute(), Path::new("a/b").is_relative(), q.starts_with("/"));
    let dir = PathBuf::from({directory:?});
    let file = dir.join("bar.txt");
    println!("{{}} {{}} {{}}", dir.exists(), dir.is_dir(), dir.is_file());
    println!("{{}} {{}} {{}}", file.exists(), file.is_file(), file.with_file_name("none").exists());
}}
main();
"#
    );
    let output = printed_with(&source, crate::Access::all());
    let denied = printed(&source);
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(
        output,
        "Some(\"txt\") Some(\"bar.txt\") Some(\"bar\") Some(\"/tmp/foo\")\n\
         /tmp/foo/bar.txt /tmp/foo/baz.rs /tmp/foo/bar.md\n\
         true \"/tmp/x.md\"\n\"/tmp/y\"\n(true, true) \"/\"\n\
         true true true\ntrue true false\ntrue true false\n"
    );
    assert!(
        denied.ends_with("false false false\nfalse false false\n"),
        "{denied}"
    );
    // `Path::new(&S)` of a borrowed `String`, and a `&Path` given back
    // owned.
    let output = printed(
        r#"
use std::path::{Path, PathBuf};
fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().to_string()
}
fn main() {
    let s = String::from("/a/b.rs");
    let p = Path::new(&s);
    let owned: PathBuf = p.to_path_buf();
    let again = PathBuf::from(p);
    let t = &s;
    println!("{} {} {:?} {} {}", name(p), name(&owned), again, p == owned.as_path(), Path::new(t).display());
}
main();
"#,
    );
    assert_eq!(output, "b.rs b.rs \"/a/b.rs\" true /a/b.rs\n");
    // A method std declares that rawr doesn't run is no unknown member.
    let report = refused(
        "use std::path::Path;\nfn main() { let p = Path::new(\"/a/b\"); p.strip_prefix(\"/a\"); }",
    );
    assert!(
        report.starts_with("error: rawr doesn't run `Path::strip_prefix` yet"),
        "{report}"
    );
}

#[test]
fn a_mut_str_of_a_string_has_strs_methods() {
    // `&mut *s`, `as_mut_str()` and a `&mut String` given for a `&mut str`
    // lend the `String`'s text, which `str`'s own changes write (#821).
    let output = printed(
        r#"
fn shout(r: &mut str) {
    r.make_ascii_uppercase();
}
fn main() {
    let mut s = String::from("abc");
    let r: &mut str = &mut *s;
    r.make_ascii_uppercase();
    println!("{}", s);
    let mut t = String::from("xyz");
    shout(&mut t);
    t.make_ascii_lowercase();
    let mut u = String::from("Hello");
    u.as_mut_str().make_ascii_lowercase();
    println!("{} {}", t, u);
}
main();
"#,
    );
    assert_eq!(output, "ABC\nxyz hello\n");
    // A `str` has none of a `String`'s changes, and a `&str` changes
    // nothing.
    for (program, expected) in [
        (
            "fn f(r: &mut str) { r.push_str(\"x\"); }\nfn main() {}\n",
            "error[E0599]: no method named `push_str` found for mutable reference `&mut str` in \
             the current scope",
        ),
        (
            "fn main() { let s = \"abc\"; s.make_ascii_uppercase(); }",
            "error[E0596]: cannot borrow `*s` as mutable, as it is behind a `&` reference",
        ),
        (
            "fn main() { let s = String::from(\"a\"); s.make_ascii_uppercase(); }",
            "error[E0596]: cannot borrow `s` as mutable, as it is not declared as mutable",
        ),
    ] {
        let report = refused(program);
        assert!(report.starts_with(expected), "{report}");
    }
}

#[test]
fn text_and_sequence_comparisons_are_stds_impls() {
    // `trim` gives a `&str`, which a `String` is compared with only by
    // `PartialEq`, and an array is no `PartialEq<Vec<U>>` (#703).
    let output = printed(
        r#"
fn main() {
    let s = String::from("abc");
    let v = vec![1, 2];
    let a = [1, 2];
    println!("{} {} {}", s.as_str() < "b", s == " abc ".trim(), " abc ".trim() == s);
    println!("{} {} {} {} {}", v == a, &v[..] == v, v[..] == v, v == &a[..], a == v[..]);
}
main();
"#,
    );
    assert_eq!(output, "true true true\ntrue true true true true\n");
    for (program, expected) in [
        (
            "fn main() { let s = String::from(\"abc\"); println!(\"{}\", s < \" a \".trim()); }",
            "error[E0308]: mismatched types",
        ),
        (
            "fn main() { let s = String::from(\"abc\"); println!(\"{}\", \" a \".trim() < s); }",
            "error[E0308]: mismatched types",
        ),
        (
            "fn main() { let a = [1, 2]; let v = vec![1, 2]; println!(\"{}\", a == v); }",
            "error[E0277]: can't compare `[{integer}; 2]` with `Vec<{integer}>`",
        ),
    ] {
        let report = refused(program);
        assert!(report.starts_with(expected), "{report}");
    }
}

#[test]
fn string_from_utf8_lossy_is_a_cow_of_the_text() {
    // Borrowed of valid UTF-8, owned with U+FFFD for an invalid sequence.
    let output = printed(
        r#"
use std::borrow::Cow;
fn main() {
    let good = vec![104, 105];
    let bad = [104, 0xff, 105];
    let a = String::from_utf8_lossy(&good);
    let b = String::from_utf8_lossy(&bad);
    let kind = |c: &Cow<str>| match c {
        Cow::Borrowed(_) => "borrowed",
        Cow::Owned(_) => "owned",
    };
    println!("{} {} {} {}", a, b, kind(&a), kind(&b));
    let owned: String = b.into_owned();
    println!("{:?} {} {}", owned, a.len(), a == "hi");
}
main();
"#,
    );
    assert_eq!(
        output,
        "hi h\u{fffd}i borrowed owned\n\"h\u{fffd}i\" 2 true\n"
    );
    // A path's and an `OsString`'s `to_string_lossy` is std's `Cow<str>`
    // too, which an `io::Error` takes as its message.
    let output = printed(
        r#"
use std::borrow::Cow;
use std::ffi::OsString;
use std::path::Path;
fn main() {
    let a = Path::new("x").to_string_lossy().into_owned();
    let os = OsString::from("z");
    let c: Cow<str> = os.to_string_lossy();
    let d: String = Path::new("y").to_string_lossy().to_string();
    let e = Path::new("w").to_string_lossy();
    println!("{a} {c} {d} {} {} {:?}", e.len(), e == "w", e);
    let error = std::io::Error::other(Path::new("bad").to_string_lossy());
    let kind = std::io::Error::new(std::io::ErrorKind::NotFound, String::from("gone"));
    println!("{} {}", error, kind);
}
main();
"#,
    );
    assert_eq!(output, "x z y 1 true \"w\"\nbad gone\n");
}

/// What the package of `files`, written under the build directory as
/// `name`, prints when it runs.
fn printed_package(name: &str, files: &[(&str, &str)]) -> String {
    use std::fs;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/graph-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let mut answers = crate::project::cargo::Answers::new();
    let target = crate::project::Target::discover(&root, &mut answers).unwrap();
    let graph = crate::package::Graph::load(&Engine::new(), &target, &mut answers)
        .unwrap_or_else(|error| panic!("{error}"));
    let lines = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut engine = Engine::new();
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    let (ast, main) = engine
        .compile_package(graph)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut scope = crate::Scope::new();
    let _: crate::Dynamic = engine
        .eval_ast_with_scope::<crate::Dynamic>(&mut scope, &ast)
        .unwrap();
    let _: crate::Dynamic = engine
        .call_fn_with_options::<crate::Dynamic>(
            crate::CallFnOptions::new()
                .eval_ast(false)
                .rewind_scope(false),
            &mut scope,
            &ast,
            main.expect("the package has a main"),
            (),
        )
        .unwrap();
    let _ = fs::remove_dir_all(&root);
    lines.lock().unwrap().clone()
}

#[test]
fn a_tuple_struct_s_constructor_takes_a_turbofish() {
    // `W::<i32>(1)`, as rustc reads a generic tuple struct's constructor:
    // called, uncalled, through a module, and an enum's variant (#2215).
    let output = printed(
        r#"
#[derive(Debug)] struct W<T>(T);
#[derive(Debug)] enum E<T> { V(T) }
mod m { #[derive(Debug)] pub struct P<T>(pub T); }
fn main() {
    let w = W::<i32>(1);
    let e = E::<u8>::V(2);
    let p = m::P::<i8>(3);
    let f = W::<u16>;
    println!("{:?} {:?} {:?} {:?}", w, e, p, f(4));
}
main();
"#,
    );
    assert_eq!(output, "W(1) V(2) P(3) W(4)\n");
    // The turbofish's type is the field's.
    let report = refused("struct W<T>(T);\nfn main() { let w = W::<i32>(\"a\"); }\n");
    assert!(
        report.starts_with("error[E0308]: mismatched types"),
        "{report}"
    );
}

#[test]
fn a_path_through_a_module_names_its_items_as_rustc_s_does() {
    // A tuple struct's constructor, a variant pattern and a generic type
    // in a `let` annotation through a module (#429, #572, #2203), and two
    // functions' local items of one name (#1036).
    let output = printed(
        r#"
mod canvas {
    #[derive(Debug)] pub struct Degrees(pub f32);
    pub mod inner { #[derive(Debug)] pub struct R(pub u8); }
}
mod bezier {
    pub enum Curve<T> { Drawn(T), Empty }
    pub mod k { pub enum E { A { x: i32 }, B } }
    #[derive(Debug)] pub struct Pair<A, B>(pub A, pub B);
}
fn a() -> i32 { enum K { A } impl K { fn v(&self) -> i32 { 1 } } K::A.v() }
fn b() -> i32 { enum K { B(i32) } impl K { fn v(&self) -> i32 { match self { K::B(n) => *n } } } K::B(5).v() }
fn main() {
    let v: Vec<canvas::inner::R> = vec![1, 2].into_iter().map(canvas::inner::R).collect();
    println!("{:?} {:?}", canvas::Degrees(90.0), v);
    let curve = bezier::Curve::Drawn(3);
    let bezier::Curve::Drawn(n) = &curve else { return };
    let e = bezier::k::E::A { x: 4 };
    match e { bezier::k::E::A { x } => println!("{n} {x}"), bezier::k::E::B => {} }
    let p: bezier::Pair<u8, Vec<bezier::Curve<i8>>> = bezier::Pair(1, vec![bezier::Curve::Empty]);
    println!("{} {} {}", p.1.len(), a(), b());
}
main();
"#,
    );
    assert_eq!(output, "Degrees(90.0) [R(1), R(2)]\n3 4\n1 1 5\n");
}

#[test]
fn a_top_level_statement_names_an_inline_module_s_paths() {
    // A script's statements resolve paths into its inline modules as a
    // body does (#1141, #1348).
    let output = printed(
        r#"
mod picks {
    #[derive(Debug)] pub struct Count(pub i32);
    pub fn twice(n: i32) -> i32 { n * 2 }
    pub mod deep { pub fn f() -> i32 { 7 } }
}
println!("{:?} {} {}", picks::Count(2), picks::twice(3), picks::deep::f());
let picks::Count(n) = picks::Count(5);
println!("{n}");
"#,
    );
    assert_eq!(output, "Count(2) 6 7\n5\n");
}

#[test]
fn a_block_s_use_is_unseen_by_a_child_module_and_a_std_glob_shadows_the_prelude() {
    // A body's `use` binds in its block alone, not for a child module's
    // `use super::*;` (#1052).
    let report = refused(
        "mod outer {\n    pub fn f() -> usize { use std::collections::HashMap; \
         let m: HashMap<i32, i32> = HashMap::new(); m.len() }\n    pub mod inner { use super::*; \
         pub fn g() -> usize { let m: HashMap<i32, i32> = HashMap::new(); m.len() } }\n}\n\
         fn main() { println!(\"{} {}\", outer::f(), outer::inner::g()); }\n",
    );
    assert!(
        report.starts_with("error[E0425]: cannot find type `HashMap` in this scope"),
        "{report}"
    );
    // `use std::fmt::*;` makes `Result` `fmt::Result`, and `use
    // std::io::*;` makes it `io::Result` (#753).
    let output = printed(
        r#"
mod text {
    use std::fmt::*;
    pub struct P;
    impl Display for P { fn fmt(&self, f: &mut Formatter<'_>) -> Result { write!(f, "p") } }
    pub fn g() -> Result { Ok(()) }
}
mod bytes {
    use std::io::*;
    pub fn g() -> Result<u8> { Ok(1) }
}
fn main() { println!("{} {:?} {:?}", text::P, text::g(), bytes::g()); }
main();
"#,
    );
    assert_eq!(output, "p Ok(()) Ok(1)\n");
}

#[test]
fn a_std_item_or_module_with_no_use_is_rustc_s_error() {
    // A bare std name no `use` brings in (#807).
    for (program, error, help) in [
        (
            "fn main() { let r = Reverse(3); }\n",
            "error[E0425]: cannot find function, tuple struct or tuple variant `Reverse` in this \
             scope",
            "help: consider importing this tuple struct\n  |\n1 + use std::cmp::Reverse;",
        ),
        (
            "struct S<T> { p: PhantomData<T> }\nfn main() {}\n",
            "error[E0425]: cannot find type `PhantomData` in this scope",
            "help: consider importing this struct\n  |\n1 + use std::marker::PhantomData;",
        ),
    ] {
        let report = refused(program);
        assert!(report.starts_with(error), "{report}");
        assert!(report.contains(help), "{report}");
    }
    // A module-relative std path with no `use` of its module, and the
    // imports rustc offers: the module for a path of two segments, the
    // item after it for a longer one, which the path then names directly
    // (#808).
    let report = refused("fn main() {\n    let d = time::Duration::from_secs(1);\n}\n");
    assert!(
        report.starts_with("error[E0433]: cannot find module or crate `time` in this scope"),
        "{report}"
    );
    assert!(
        report.contains(
            "help: consider importing this struct\n  |\n1 + use std::time::Duration;\n  |\n\
             help: if you import `Duration`, refer to it directly\n  |\n\
             2 -     let d = time::Duration::from_secs(1);\n\
             2 +     let d = Duration::from_secs(1);\n  |"
        ),
        "{report}"
    );
    let report = refused(
        "fn main() {\n    let m: collections::HashMap<i32, i32> = Default::default();\n}\n",
    );
    assert!(
        report
            .contains("help: consider importing this module\n  |\n1 + use std::collections;\n  |"),
        "{report}"
    );
    let report =
        refused("fn main() {\n    match 1.cmp(&2) { cmp::Ordering::Less => {} _ => {} }\n}\n");
    assert!(
        report.contains(
            "help: consider importing one of these enums\n  |\n1 + use std::cmp::Ordering;\n  |\n\
             1 + use std::sync::atomic::Ordering;\n"
        ),
        "{report}"
    );
    let report = refused(
        "fn main() {\n    let (mut a, mut b) = (1, 2);\n    mem::swap(&mut a, &mut b);\n}\n",
    );
    assert!(
        report.starts_with("error[E0433]: cannot find module or crate `mem` in this scope")
            && report.contains("help: consider importing this module\n  |\n1 + use std::mem;\n  |"),
        "{report}"
    );
    let report = refused("fn main() {\n    let t = io::stdin();\n}\n");
    assert!(
        report.contains("help: consider importing this module\n  |\n1 + use std::io;\n  |"),
        "{report}"
    );
    // With the `use`, each runs.
    let output = printed(
        r#"
use std::{collections, mem, time};
use std::cmp::Reverse;
use std::marker::PhantomData;
struct S<T> { p: PhantomData<T> }
fn main() {
    let (mut a, mut b) = (1, 2);
    mem::swap(&mut a, &mut b);
    let m: collections::HashMap<i32, i32> = collections::HashMap::new();
    let _s: S<i32> = S { p: PhantomData };
    println!("{a} {b} {:?} {:?} {}", time::Duration::from_secs(1), m, Reverse(1) < Reverse(0));
}
main();
"#,
    );
    assert_eq!(output, "2 1 1s {} true\n");
}

#[test]
fn a_non_prelude_std_trait_s_method_is_callable_only_where_it_is_in_scope() {
    // `io::Write`'s `write_all` on a `Vec<u8>` with no `use` of it:
    // rustc's E0599 and its help (#1427).
    let report =
        refused("fn main() { let mut v: Vec<u8> = Vec::new(); v.write_all(b\"hi\").unwrap(); }\n");
    assert!(
        report.starts_with(
            "error[E0599]: no method named `write_all` found for struct `Vec<u8>` in the current \
             scope"
        ),
        "{report}"
    );
    assert!(
        report.contains(
            "  = help: items from traits can only be used if the trait is in scope\n\
             help: trait `Write` which provides `write_all` is implemented but not in scope; \
             perhaps you want to import it\n  |\n1 + use std::io::Write;\n"
        ),
        "{report}"
    );
    // `write!` into a `String` with only `io::Write` in scope is the same
    // E0599 as with none: a `String` has `fmt::Write`'s (#1428).
    let report = refused(
        "use std::io::Write;\nfn main() { let mut s = String::new(); write!(s, \"x\").unwrap(); }\n",
    );
    assert!(
        report.starts_with("error[E0599]: cannot write into `String`"),
        "{report}"
    );
    assert!(report.contains("1 + use std::fmt::Write;"), "{report}");
    // In scope by its `use`, or by `io`'s prelude glob, each runs.
    for scope in ["use std::io::Write;", "use std::io::prelude::*;"] {
        let program = format!(
            "{scope}\nfn main() {{\n    let mut v: Vec<u8> = Vec::new();\n    \
             v.write_all(b\"ab\").unwrap();\n    write!(v, \"c\").unwrap();\n    \
             println!(\"{{:?}}\", v);\n}}\nmain();\n"
        );
        assert_eq!(printed(&program), "[97, 98, 99]\n", "{scope}");
    }
}

#[test]
fn a_mod_s_path_attribute_names_its_file() {
    // `#[path]` from the root, from a file that isn't a `mod.rs` (its own
    // directory), and inside an inline module (that module's directory); a
    // module in a file `#[path]` names owns that file's directory (#605).
    let output = printed_package(
        "path-attribute",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"paths\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
            ),
            (
                "src/main.rs",
                "// rawr\n#[path = \"other/thing.rs\"]\nmod thing;\nmod parity;\nmod outer {\n    \
                 #[path = \"deep.rs\"]\n    pub mod deep;\n}\nfn main() {\n    \
                 println!(\"{} {} {} {}\", thing::f(), thing::kid::g(), parity::emulated::h(), \
                 outer::deep::d());\n}\n",
            ),
            ("src/other/thing.rs", "// rawr\npub mod kid;\npub fn f() -> i32 { 1 }\n"),
            ("src/other/kid.rs", "// rawr\npub fn g() -> i32 { 2 }\n"),
            ("src/parity.rs", "// rawr\n#[path = \"parity_impl.rs\"]\npub mod emulated;\n"),
            ("src/parity_impl.rs", "// rawr\npub fn h() -> i32 { 3 }\n"),
            ("src/outer/deep.rs", "// rawr\npub fn d() -> i32 { 4 }\n"),
        ],
    );
    assert_eq!(output, "1 2 3 4\n");
}

#[test]
fn a_kept_view_of_a_range_of_elements_reaches_them() {
    // A `&mut [T]` kept from `as_mut_slice()`, `make_contiguous()` or
    // `&mut v[a..]`, in a local or a struct's field, changes the
    // collection, its `first_mut` and `last_mut` included, as rustc's build
    // runs it (#800, D1113).
    let output = printed(
        r#"
use std::collections::VecDeque;
struct W<'a> {
    s: &'a mut [i32],
}
fn main() {
    let mut v = vec![3, 1, 2];
    let s = v.as_mut_slice();
    s.sort();
    s[0] = 10;
    println!("{:?}", v);
    let mut d: VecDeque<i32> = VecDeque::new();
    d.push_back(3);
    d.push_front(5);
    d.push_back(1);
    let c = d.make_contiguous();
    c.sort();
    println!("{:?}", d);
    let mut w = vec![3, 1, 2, 5];
    let s = &mut w[1..];
    s.reverse();
    s[0] += 100;
    s.sort();
    println!("{:?} {}", s, s.len());
    let t = &mut s[1..];
    t[0] = 9;
    if let Some(x) = s.first_mut() {
        *x = 7;
    }
    if let Some(x) = s.last_mut() {
        *x += 1;
    }
    w.push(4);
    println!("{:?}", w);
    let mut x = vec![1, 2, 3];
    let held = W { s: &mut x[1..] };
    held.s[0] = 7;
    println!("{:?}", x);
}
main();
"#,
    );
    assert_eq!(
        output,
        "[10, 2, 3]\n[1, 3, 5]\n[1, 2, 105] 3\n[3, 7, 9, 106, 4]\n[1, 7, 3]\n"
    );
}

#[test]
fn split_at_mut_and_chunks_mut_views_reach_their_collection() {
    // Each half `split_at_mut` gives and each chunk of `chunks_mut` is a
    // view of the collection's own elements: two halves are used side by
    // side, from closures and from scoped threads, with no data race, as
    // rustc's build runs them (D1113).
    let output = printed(
        r#"
fn halves(s: &mut [i32]) {
    let (a, b) = s.split_at_mut(1);
    a[0] = b[0] + b[1];
    for c in b.chunks_mut(1) {
        c[0] *= 2;
    }
}
fn main() {
    let mut v = vec![1, 2, 3, 4, 5];
    let (a, b) = v.split_at_mut(2);
    a[0] = 10;
    b[0] += a[1];
    std::mem::swap(&mut a[1], &mut b[2]);
    println!("{:?} {:?}", a, b);
    println!("{:?}", v);
    for c in v.chunks_mut(2) {
        c[0] *= 10;
        c.reverse();
    }
    println!("{:?}", v);
    let mut u = vec![1, 2, 3];
    halves(&mut u);
    println!("{:?}", u);
    let mut w = vec![3, 1, 2, 5, 4];
    let (a, b) = w.split_at_mut(2);
    a.sort_by_key(|x| b[0] - *x);
    b.sort_by(|x, y| a[0].cmp(x).then(x.cmp(y)));
    println!("{:?}", w);
    let mut t = vec![1, 2, 3, 4, 5];
    let (a, b) = t.split_at_mut(2);
    std::thread::scope(|scope| {
        scope.spawn(|| a.reverse());
        scope.spawn(|| b.sort_by(|x, y| y.cmp(x)));
    });
    println!("{:?}", t);
}
main();
"#,
    );
    assert_eq!(
        output,
        "[10, 5] [5, 4, 2]\n[10, 5, 5, 4, 2]\n[5, 100, 4, 50, 20]\n[5, 4, 6]\n[3, 1, 4, 5, \
         2]\n[2, 1, 5, 4, 3]\n"
    );
}

#[test]
fn a_kept_iter_mut_reaches_each_element() {
    // A kept `iter_mut()` used other than by `next()`: a local walked by a
    // `for`, `zip`, `rev`, `enumerate`, `filter`, `for_each`, collected into
    // a `Vec<&mut T>`, passed to a function, and two of its items held at
    // once, as rustc's build runs them (#806, D1113).
    let output = printed(
        r#"
fn bump<'a>(it: impl Iterator<Item = &'a mut i32>) {
    for x in it {
        *x += 1;
    }
}
struct S {
    v: Vec<i32>,
}
fn main() {
    let mut v = vec![1, 2, 3];
    let it = v.iter_mut();
    for x in it {
        *x *= 2;
    }
    let mut w = vec![1, 2, 3];
    let a = vec![10, 20, 30];
    for (x, y) in w.iter_mut().zip(a.iter()) {
        *x += y;
    }
    let mut u = vec![1, 2, 3];
    let refs: Vec<&mut i32> = u.iter_mut().collect();
    for r in refs {
        *r += 1;
    }
    let mut q = vec![1, 2, 3];
    q.iter_mut().rev().for_each(|x| *x -= 1);
    println!("{:?} {:?} {:?} {:?}", v, w, u, q);
    let mut e = vec![1, 2, 3];
    let mut it = e.iter_mut().enumerate();
    let (i, x) = it.next().unwrap();
    *x = 100 + i as i32;
    for (i, x) in it {
        *x += i as i32 * 10;
    }
    println!("{:?}", e);
    let mut p = vec![1, 2, 3];
    bump(p.iter_mut());
    let mut it = p.iter_mut();
    it.next();
    let rest: Vec<&mut i32> = it.collect();
    for r in rest {
        *r *= 10;
    }
    println!("{:?}", p);
    let mut n = vec![1, 2, 3];
    let mut it = n.iter_mut();
    let a = it.next().unwrap();
    let b = it.next().unwrap();
    *a += 10;
    *b += 20;
    std::mem::swap(a, b);
    *it.next().unwrap() += 5;
    println!("{:?}", n);
    let mut f = vec![1, 2, 3, 4];
    let evens: Vec<&mut i32> = f.iter_mut().filter(|x| **x % 2 == 0).collect();
    for e in evens {
        *e = 0;
    }
    let mut s = S { v: vec![1, 2, 3] };
    let refs: Vec<&mut i32> = s.v.iter_mut().rev().collect();
    *refs.into_iter().next().unwrap() = 9;
    println!("{:?} {:?}", f, s.v);
}
main();
"#,
    );
    assert_eq!(
        output,
        "[2, 4, 6] [11, 22, 33] [2, 3, 4] [0, 1, 2]\n[100, 12, 23]\n[2, 30, 40]\n[22, 11, \
         8]\n[1, 0, 3, 0] [1, 2, 9]\n"
    );
}

#[test]
fn a_kept_view_holds_its_collection_borrowed() {
    // While a kept `iter_mut()`, a `split_at_mut` half, a chunk, a range's
    // view or the references collected from them live, the collection is
    // borrowed mutably: rustc's E0499 and E0502 (#805).
    let refused_with = |body: &str, report: &str| {
        let refusal = refused(&format!(
            "fn main() {{\n    let mut v = vec![1, 2, 3];\n{body}}}\nmain();\n"
        ));
        assert!(refusal.starts_with(report), "{body}: {refusal}");
    };
    let twice = "error[E0499]: cannot borrow `v` as mutable more than once at a time";
    let shared =
        "error[E0502]: cannot borrow `v` as immutable because it is also borrowed as mutable";
    refused_with(
        "    let mut it = v.iter_mut();\n    v.len();\n    it.next();\n",
        shared,
    );
    refused_with(
        "    let (a, _b) = v.split_at_mut(1);\n    let c = &mut v;\n    a[0] = 1;\n    \
         c.push(1);\n",
        twice,
    );
    refused_with(
        "    let s = v.as_mut_slice();\n    v.push(4);\n    s[0] = 1;\n",
        twice,
    );
    refused_with(
        "    let mut refs: Vec<&mut i32> = v.iter_mut().collect();\n    println!(\"{}\", \
         v.len());\n    *refs[0] = 1;\n",
        shared,
    );
    refused_with(
        "    let mut a = v.iter_mut();\n    let mut b = v.iter_mut();\n    a.next();\n    \
         b.next();\n",
        twice,
    );
    refused_with(
        "    for c in v.chunks_mut(2) {\n        v.push(c[0]);\n    }\n",
        twice,
    );
    refused_with(
        "    let s = &mut v[1..];\n    println!(\"{}\", v[0]);\n    s[0] = 1;\n",
        shared,
    );
}

#[test]
fn a_view_out_of_its_range_panics_as_rustc() {
    // std's panics for a view's bounds, where rustc's build places them.
    for (body, at, message) in [
        (
            "    let (a, b) = v.split_at_mut(5);\n    a[0] = b[0];\n",
            "3:20",
            "mid > len",
        ),
        (
            "    let s = &mut v[1..9];\n    s[0] = 1;\n",
            "3:19",
            "range end index 9 out of range for slice of length 3",
        ),
        (
            "    let s = &mut v[1..];\n    s[5] = 1;\n",
            "4:5",
            "index out of bounds: the len is 2 but the index is 5",
        ),
        (
            "    for c in v.chunks_mut(0) {\n        c[0] = 1;\n    }\n",
            "3:16",
            "chunk size must be non-zero",
        ),
    ] {
        let source = format!("fn main() {{\n    let mut v = vec![1, 2, 3];\n{body}}}\n");
        assert_eq!(
            panicked(&source),
            format!("thread 'main' panicked at main.rs:{at}:\n{message}"),
            "{body}"
        );
    }
}

#[test]
fn a_generic_call_runs_the_impl_of_its_type_argument_among_kinds() {
    // A trait with impls for f32 and f64, two integer kinds, or &str and
    // String runs the impl rustc's monomorphisation picks by `T`, through
    // a method, a trait function, a nested generic, a closure and an
    // inherent impl's parameter (#1177).
    let output = printed(
        r#"
trait Half { fn half(&self) -> String; }
impl Half for f32 { fn half(&self) -> String { format!("f32 {}", self / 2.0) } }
impl Half for f64 { fn half(&self) -> String { format!("f64 {}", self / 2.0) } }
impl Half for u8 { fn half(&self) -> String { format!("u8 {}", self / 2) } }
impl Half for i64 { fn half(&self) -> String { format!("i64 {}", self / 2) } }
trait Kind { fn kind(&self) -> &'static str; fn name() -> &'static str; }
impl Kind for &str { fn kind(&self) -> &'static str { "str" } fn name() -> &'static str { "&str" } }
impl Kind for String { fn kind(&self) -> &'static str { "String" } fn name() -> &'static str { "String" } }
struct W<T>(T);
impl<T: Half> W<T> { fn go(&self) -> String { self.0.half() } }
fn twice<T: Half>(x: &T) -> String { format!("{} {}", x.half(), inner(x)) }
fn inner<U: Half>(x: &U) -> String { x.half() }
fn later<T: Half + Copy>(x: T) -> String { let f = move || x.half(); f() }
fn kind<T: Kind>(x: T) -> String { format!("{} {}", x.kind(), T::name()) }
fn main() {
    println!("{} | {}", twice(&3.0f32), twice(&3.0f64));
    println!("{} | {} | {}", twice(&9u8), later(9i64), W(1.0f64).go());
    println!("{} | {}", kind("a"), kind(String::from("b")));
}
main();
"#,
    );
    assert_eq!(
        output,
        "f32 1.5 f32 1.5 | f64 1.5 f64 1.5\nu8 4 u8 4 | i64 4 | f64 0.5\nstr &str | String String\n"
    );
    // `x.into()` of a `T: Into<Bar>` runs the `From` impl for `T`.
    let output = printed(
        r#"
struct Bar(String);
impl From<&str> for Bar { fn from(s: &str) -> Bar { Bar(format!("str:{s}")) } }
impl From<String> for Bar { fn from(s: String) -> Bar { Bar(format!("string:{s}")) } }
impl From<u8> for Bar { fn from(n: u8) -> Bar { Bar(format!("u8:{n}")) } }
impl From<i64> for Bar { fn from(n: i64) -> Bar { Bar(format!("i64:{n}")) } }
fn into<T: Into<Bar>>(x: T) -> String { x.into().0 }
fn main() {
    println!("{} {} {} {}", into("a"), into(String::from("b")), into(3u8), into(4i64));
}
main();
"#,
    );
    assert_eq!(output, "str:a string:b u8:3 i64:4\n");
}

#[test]
fn an_f32_passed_to_a_generic_writes_as_an_f32() {
    // A generic body's `{}` and `{:?}` of an `f32` write the `f32`, and of
    // an `f64` Rust's `f64` text, with its precision (#1177).
    let output = printed(
        r#"
use std::fmt::{Debug, Display};
fn show<T: Display>(x: T) -> String { format!("{} {:.2}", x, x) }
fn debug<T: Debug>(x: T) -> String { format!("{:?}", x) }
fn main() {
    println!("{} | {} | {}", show(0.1f32), show(2.0f64), show(1.0f32 / 3.0));
    println!("{} | {} | {}", debug(0.1f32), debug(vec![0.1f32, 2.0]), debug((0.1f32, 0.2f64)));
}
main();
"#,
    );
    assert_eq!(
        output,
        "0.1 0.10 | 2 2.00 | 0.33333334 0.33\n0.1 | [0.1, 2.0] | (0.1, 0.2)\n"
    );
}

#[test]
fn a_consuming_iterator_method_moves_the_iterator_local() {
    // `it.collect()`, `it.count()`, `for x in it` and passing `it` take it
    // by value, as for a range (#2523). A clone or `next()` keeps it.
    let output = printed(
        r#"
fn main() {
    let words = "a b c".split(' ');
    let n = words.clone().count();
    let all: Vec<&str> = words.collect();
    let mut chars = "xyz".chars();
    let first = chars.next();
    let rest: String = chars.collect();
    println!("{} {:?} {:?} {}", n, all, first, rest);
}
main();
"#,
    );
    assert_eq!(output, "3 [\"a\", \"b\", \"c\"] Some('x') yz\n");
    for program in [
        "fn main() { let it = vec![1, 2].into_iter(); let a: Vec<i32> = it.collect(); \
         let b: Vec<i32> = it.collect(); println!(\"{:?} {:?}\", a, b); }",
        "fn main() { let it = \"a b\".split(' '); let n = it.count(); println!(\"{} {}\", n, it.count()); }",
        "fn main() { let it = vec![1, 2].into_iter(); for x in it { print!(\"{x}\"); } \
         println!(\"{}\", it.count()); }",
        "fn take<I: Iterator<Item = i32>>(i: I) -> i32 { i.sum() }\n\
         fn main() { let it = vec![1, 2].into_iter(); println!(\"{} {}\", take(it), take(it)); }",
    ] {
        let report = refused(program);
        assert!(
            report.starts_with("error[E0382]: use of moved value: `it`"),
            "{report}"
        );
    }
}

#[test]
fn an_iterator_parameter_without_its_item_has_iterator_methods() {
    // `I: Iterator` with no `Item = X`: its item is `<I as Iterator>::Item`,
    // which the call fills (#2507).
    let output = printed(
        r#"
struct Count { n: u32 }
impl Iterator for Count {
    type Item = u32;
    fn next(&mut self) -> Option<u32> { if self.n < 3 { self.n += 1; Some(self.n) } else { None } }
}
fn count<I: Iterator>(i: I) -> usize { i.count() }
fn first<I: Iterator>(mut i: I) -> Option<I::Item> { i.next() }
fn all<I>(i: I) -> Vec<I::Item> where I: Iterator { i.collect() }
fn main() {
    println!("{} {}", count(vec![1, 2, 3].into_iter()), count("a b".split(' ')));
    println!("{:?} {:?} {:?}", first(vec!['x'].into_iter()), first(Count { n: 0 }), all(Count { n: 1 }));
}
main();
"#,
    );
    assert_eq!(output, "3 2\nSome('x') Some(1) [2, 3]\n");
}
