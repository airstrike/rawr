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
        Err(NOT_LONG_ENOUGH),
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
        Err(NOT_LONG_ENOUGH),
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
        Err(NOT_LONG_ENOUGH),
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
        Err(NOT_LONG_ENOUGH),
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
    // A tuple's members swapped.
    signature_verdict(
        r#"
fn f<'a, 'b>(x: (&'a i32, &'b i32)) -> (&'b i32, &'a i32) { x }
fn main() { let (a, b) = (1, 2); println!("{:?}", f((&a, &b))); }
main();
"#,
        Err(NOT_LONG_ENOUGH),
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
        Err(NOT_LONG_ENOUGH),
    );
    signature_verdict(
        r#"
fn f<'a, 'b>(x: &'a mut Vec<&'b i32>) -> &'a mut Vec<&'a i32> { x }
fn main() { let y = 1; let mut v = vec![&y]; println!("{}", f(&mut v).len()); }
main();
"#,
        Err(NOT_LONG_ENOUGH),
    );
    signature_verdict(
        r#"
use std::cell::Cell;
fn f<'a, 'b: 'a>(x: &'a Cell<&'b i32>) -> &'a Cell<&'a i32> { x }
fn main() { let y = 1; let c = Cell::new(&y); println!("{}", f(&c).get()); }
main();
"#,
        Err(NOT_LONG_ENOUGH),
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
        Err(NOT_LONG_ENOUGH),
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
        Err(NOT_LONG_ENOUGH),
    );
    signature_verdict(
        r#"
fn f<'a, 'b>(x: Option<&'a i32>, y: &'b i32) -> Option<&'b i32> { if y > &0 { x } else { Some(y) } }
fn main() { let (a, b) = (1, 2); println!("{:?}", f(Some(&a), &b)); }
main();
"#,
        Err(NOT_LONG_ENOUGH),
    );
    // rustc's E0621 where the parameter's lifetime is elided.
    signature_verdict(
        r#"
fn f<'a>(x: & &'a i32) -> &'a &'a i32 { x }
fn main() { let y = 1; let r = &y; println!("{}", f(&r)); }
main();
"#,
        Err("error[E0621]: explicit lifetime required in the type of `x`"),
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
        Err(NOT_LONG_ENOUGH),
    );
    signature_verdict(
        r#"
fn first<'a, 'b>(_x: &'a Vec<i32>, y: &'b Vec<i32>) -> &'a i32 { &y[0] }
fn main() { println!("{}", first(&vec![1], &vec![2])); }
main();
"#,
        Err(NOT_LONG_ENOUGH),
    );
    signature_verdict(
        r#"
struct S { name: String }
impl S { fn pick<'a>(&self, _o: &'a str) -> &'a str { self.name.as_str() } }
fn main() { let s = S { name: String::from("n") }; println!("{}", s.pick("x")); }
main();
"#,
        Err(NOT_LONG_ENOUGH),
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

/// rustc's verdict on `source`, whose functions are checked against their
/// own signatures (#170): the report starts with `Err`'s, or the program
/// prints `Ok`'s.
fn signature_verdict(source: &str, verdict: Result<&str, &str>) {
    match verdict {
        Ok(output) => assert_eq!(printed(source), output),
        Err(report) => {
            let refused = Engine::new()
                .compile(source)
                .err()
                .unwrap_or_else(|| panic!("rustc refuses this program:{source}"))
                .to_string();
            assert!(refused.starts_with(report), "{refused}");
        }
    }
}

/// rustc's report of a region a function's body doesn't relate to its
/// signature's as the types require.
const NOT_LONG_ENOUGH: &str = "error: lifetime may not live long enough";

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
