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
