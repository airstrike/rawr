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
    // rustc names the type in the hidden type that isn't `Send`.
    let report = refused(&program("Rc<f64>", "Rc::new(2.0)"));
    assert!(
        report.starts_with("error[E0277]: `Rc<f64>` cannot be sent between threads safely"),
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
        report.starts_with("error[E0277]: `Rc<f64>` cannot be sent between threads safely"),
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
fn a_trait_methods_opaque_output_is_each_impls_own() {
    // rustc's `<A as Make>::make` is its own opaque type for each impl, so
    // a generic `T::zero()` dispatches on each impl's hidden type, through
    // a generic body's instances and a provided method kept by each impl
    // (D1109 stage 4).
    let output = printed(
        r#"
trait Zero { fn zero() -> Self; fn val(&self) -> i64; }
struct Z(i64);
struct W(i64);
impl Zero for Z { fn zero() -> Self { Z(1) } fn val(&self) -> i64 { self.0 } }
impl Zero for W { fn zero() -> Self { W(2) } fn val(&self) -> i64 { self.0 * 10 } }
trait Make { fn make(&self) -> impl Zero; }
struct A; struct B;
impl Make for A { fn make(&self) -> impl Zero { Z(5) } }
impl Make for B { fn make(&self) -> impl Zero { W(6) } }
fn same<T: Zero>(_: &T) -> T { T::zero() }
fn both<M: Make>(m: &M) -> i64 { let v = m.make(); same(&v).val() + v.val() }
fn main() {
    let a = A.make();
    println!("{} {}", same(&a).val(), same(&B.make()).val());
    println!("{} {}", both(&A), both(&B));
}
main();
"#,
    );
    assert_eq!(output, "1 20\n6 80\n");
    let output = printed(
        r#"
trait Zero { fn zero() -> Self; fn val(&self) -> i64; }
#[derive(Clone)] struct A(i64);
#[derive(Clone)] struct B(i64);
impl Zero for A { fn zero() -> Self { A(7) } fn val(&self) -> i64 { self.0 } }
impl Zero for B { fn zero() -> Self { B(8) } fn val(&self) -> i64 { self.0 } }
trait Make: Clone + Zero { fn me(&self) -> impl Zero + Clone { self.clone() } }
impl Make for A {}
impl Make for B {}
fn same<T: Zero>(_: &T) -> T { T::zero() }
fn main() {
    println!("{} {}", same(&A(1).me()).val(), same(&B(2).me()).val());
    println!("{} {}", A(1).me().val(), B(2).me().val());
}
main();
"#,
    );
    assert_eq!(output, "7 8\n1 2\n");
    // Two impls' opaque types are apart, and a trait with one is no
    // `dyn` type (E0038).
    let program = |body: &str| {
        format!(
            r#"
trait Shape {{ fn area(&self) -> f64; }}
struct Sq(f64);
struct Ci(f64);
impl Shape for Sq {{ fn area(&self) -> f64 {{ self.0 * self.0 }} }}
impl Shape for Ci {{ fn area(&self) -> f64 {{ 3.0 * self.0 * self.0 }} }}
trait Grow {{ fn grow(&self) -> impl Shape; }}
impl Grow for Sq {{ fn grow(&self) -> impl Shape {{ Sq(self.0 + 1.0) }} }}
impl Grow for Ci {{ fn grow(&self) -> impl Shape {{ Ci(self.0 + 1.0) }} }}
fn main() {{ {body} }}
main();
"#
        )
    };
    let report = refused(&program(
        "let mut x = Sq(1.0).grow(); x = Ci(1.0).grow(); println!(\"{}\", x.area());",
    ));
    assert!(
        report.contains("expected opaque type, found a different opaque type"),
        "{report}"
    );
    let report = refused(&program(
        "let b: Box<dyn Grow> = Box::new(Sq(1.0)); println!(\"{}\", b.grow().area());",
    ));
    assert!(
        report.contains("references an `impl Trait` type in its return type"),
        "{report}"
    );
}

#[test]
fn an_impl_may_refine_its_traits_impl_trait_output() {
    // rustc lets an impl write its own type for its trait's `impl Trait`
    // output (RFC 3245), which its callers then know, and proves the
    // trait's bounds of it where it is written.
    let output = printed(
        r#"
trait Shape { fn area(&self) -> f64; }
struct Sq(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } }
impl Sq { fn side(&self) -> f64 { self.0 } }
trait Grow { fn grow(&self) -> impl Shape; }
impl Grow for Sq { fn grow(&self) -> Sq { Sq(self.0 + 1.0) } }
fn grown<T: Grow>(t: &T) -> f64 { t.grow().area() }
fn main() { println!("{} {}", Sq(1.0).grow().side(), grown(&Sq(2.0))); }
main();
"#,
    );
    assert_eq!(output, "2 9\n");
    let report = refused(
        r#"
trait Shape { fn area(&self) -> f64; }
struct Sq(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } }
trait Grow { fn grow(&self) -> impl Shape; }
impl Grow for Sq { fn grow(&self) -> i64 { 3 } }
fn main() { println!("{}", Sq(1.0).grow()); }
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `i64: Shape` is not satisfied"),
        "{report}"
    );
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
fn a_mut_of_a_std_iterator_field_steps_the_field() {
    // std's `impl Iterator for &mut I` of a field: a `for`, `by_ref`, a
    // generic `&mut I` and a kept chain each go on where the last stopped
    // (#2500).
    let output = printed(
        r#"
struct S { it: std::vec::IntoIter<i32> }
fn first_two<I: Iterator<Item = i32>>(i: &mut I) -> Vec<i32> { i.take(2).collect() }
impl S {
    fn word(&mut self) -> Option<i32> {
        for w in &mut self.it { return Some(w * 10); }
        None
    }
    fn pair(&mut self) -> Vec<i32> { self.it.by_ref().take(2).collect() }
    fn kept(&mut self) -> Option<i32> {
        let mut two = (&mut self.it).skip(1);
        two.next()
    }
}
fn main() {
    let mut s = S { it: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].into_iter() };
    let a: Vec<i32> = s.it.by_ref().take(2).collect();
    let b = first_two(&mut s.it);
    println!("{:?} {:?} {:?} {:?} {:?}", a, b, s.word(), s.pair(), s.kept());
    let mut c = Vec::new();
    for x in &mut s.it { c.push(x); }
    println!("{:?} {:?}", c, s.it.next());
}
main();
"#,
    );
    assert_eq!(
        output,
        "[1, 2] [3, 4] Some(50) [6, 7] Some(9)\n[10, 11, 12] None\n"
    );
    let report = refused(
        r#"
struct S { it: std::vec::IntoIter<i32> }
fn main() {
    let s = S { it: vec![1, 2].into_iter() };
    for x in &mut s.it { println!("{}", x); }
}
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0596]: cannot borrow `s.it` as mutable, as `s` is not declared as mutable"
        ),
        "{report}"
    );
    let report = refused(
        r#"
struct S { it: std::vec::IntoIter<i32> }
impl S {
    fn all(&self) { for x in &mut self.it { println!("{}", x); } }
}
fn main() { S { it: vec![1].into_iter() }.all(); }
main();
"#,
    );
    assert!(
        report.starts_with(
            "error[E0596]: cannot borrow `self.it` as mutable, as it is behind a `&` reference"
        ),
        "{report}"
    );
}

#[test]
fn a_declared_std_io_trait_keeps_a_std_receivers_own_method() {
    // The program's `impl Read` and `impl Write`, and `dyn` of each, put
    // std's traits in its trait table; a `Cursor`'s `read_to_string` and
    // `Stdout`'s `write_all` and `flush` are still std's own (#1286).
    let output = printed(
        r#"
use std::io::{Cursor, Read, Write};
struct Z(u8);
impl Read for Z {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.0 == 0 { return Ok(0); }
        buf[0] = b'a' + self.0;
        self.0 -= 1;
        Ok(1)
    }
}
struct Sink(Vec<u8>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn all(r: &mut dyn Read) -> String { let mut s = String::new(); r.read_to_string(&mut s).unwrap(); s }
fn put(w: &mut dyn Write) { w.write_all(b"w").unwrap(); }
fn main() {
    let mut c = Cursor::new(b"xy".to_vec());
    let mut s = String::new();
    let n = c.read_to_string(&mut s).unwrap();
    let mut z = Z(3);
    let mut sink = Sink(Vec::new());
    put(&mut sink);
    sink.write_all(b"!").unwrap();
    let mut out = std::io::stdout();
    out.write_all(b"out\n").unwrap();
    out.flush().unwrap();
    println!("{} {:?} {:?} {:?}", n, s, all(&mut z), sink.0);
}
main();
"#,
    );
    assert_eq!(output, "out\n2 \"xy\" \"dcb\" [119, 33]\n");
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
fn drain_takes_a_range_value_no_place_holds() {
    // A range a call gives is std's `RangeBounds<usize>` too: `drain` and
    // `replace_range` read its bounds once, after the call runs (#2522).
    let output = printed(
        r#"
fn make() -> std::ops::Range<usize> { println!("make"); 1..3 }
fn upto() -> std::ops::RangeToInclusive<usize> { ..=1 }
fn main() {
    let mut v = vec![1, 2, 3, 4, 5];
    let d: Vec<i32> = v.drain(make()).collect();
    println!("{:?} {:?}", d, v);
    let e: Vec<i32> = v.drain(upto()).collect();
    println!("{:?} {:?}", e, v);
    let mut s = String::from("hello");
    let t: String = s.drain(make()).collect();
    s.replace_range(upto(), "J");
    println!("{} {}", t, s);
}
main();
"#,
    );
    assert_eq!(output, "make\n[2, 3] [1, 4, 5]\n[1, 4] [5]\nmake\nel Jo\n");
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

/// The package of `files`, written under the build directory as `name`,
/// loaded, with its directory.
fn loaded_package(
    name: &str,
    files: &[(&str, &str)],
) -> (std::path::PathBuf, crate::package::Graph) {
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
    (root, graph)
}

/// What the package of `files`, written under the build directory as
/// `name`, prints when it runs.
fn printed_package(name: &str, files: &[(&str, &str)]) -> String {
    use std::fs;
    let (root, graph) = loaded_package(name, files);
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

#[test]
fn a_struct_variant_holds_its_fields_borrows() {
    // `E::V { r: &a }` keeps `a` borrowed while the value lives, as a
    // struct literal and a tuple variant do (#133).
    let assigned = "error[E0506]: cannot assign to `a` because it is borrowed";
    refused_as(&[
        (
            "#[derive(Debug)] enum E<'a> { V { r: &'a i32 } }
fn main() { let mut a = 1; let e = E::V { r: &a }; a = 2; println!(\"{:?} {}\", e, a); }",
            assigned,
        ),
        (
            "#[derive(Debug)] enum E<'a> { V { n: i32, r: &'a i32 } }
use E::V;
fn main() { let mut a = 1; let e = V { r: &a, n: 1 }; a = 2; println!(\"{:?} {}\", e, a); }",
            assigned,
        ),
    ]);
    let output = printed(
        r#"
#[derive(Debug)]
enum E<'a> { V { n: i32, r: &'a i32 }, W }
impl<'a> E<'a> {
    fn make(r: &'a i32) -> Self { Self::V { r, n: 0 } }
}
fn main() {
    let a = 1;
    let mut b = 5;
    let e = E::V { r: &a, n: b };
    b = 2;
    if let E::V { r, n } = e {
        println!("{} {} {} {:?} {:?}", r, n, b, E::make(&a), E::W);
    }
}
main();
"#,
    );
    assert_eq!(output, "1 5 2 V { n: 0, r: 1 } W\n");
}

#[test]
fn a_closure_parameter_shadowing_a_borrowed_local_runs() {
    // `|b: i32|` shadows `b` only in the closure, where the `&mut b` held
    // in `r` is still `b` (#139).
    let output = printed(
        r#"
fn main() {
    let mut b = 10;
    let r = &mut b;
    let mut f = |b: i32| { *r += b; };
    f(3);
    println!("{}", b);
    let mut c = 10;
    let s = &mut c;
    let add = |(a, c): (i32, i32)| a + c;
    *s += add((1, 2));
    let mut g = move |x: i32, c| { *s += x * c; };
    g(2, 5);
    println!("{}", c);
    let mut d = 1;
    let t = &mut d;
    let h = |d: i32| d | 4;
    *t += h(1);
    println!("{}", d);
}
main();
"#,
    );
    assert_eq!(output, "13\n23\n6\n");
}

#[test]
fn into_owned_through_a_reference_is_e0507() {
    // `Cow::into_owned` takes the `Cow`, which a reference can't give up
    // (#819).
    let program = |body: &str| {
        format!(
            "use std::borrow::Cow;
struct H<'a> {{ r: &'a Cow<'a, str> }}
fn get<'a>(c: &'a Cow<'a, str>) -> &'a Cow<'a, str> {{ c }}
fn main() {{
    let mut c: Cow<str> = Cow::Borrowed(\"hi\");
    {body}
}}"
        )
    };
    let cases = [
        (
            "let r = &c; let s: String = r.into_owned(); println!(\"{s}\");",
            "error[E0507]: cannot move out of `*r` which is behind a shared reference",
        ),
        (
            "let rr = &&c; let s = rr.into_owned(); println!(\"{s}\");",
            "error[E0507]: cannot move out of `**rr` which is behind a shared reference",
        ),
        (
            "let h = H { r: &c }; let s = h.r.into_owned(); println!(\"{s}\");",
            "error[E0507]: cannot move out of `*h.r` which is behind a shared reference",
        ),
        (
            "let s = get(&c).into_owned(); println!(\"{s}\");",
            "error[E0507]: cannot move out of a shared reference",
        ),
        (
            "let m = &mut c; let s = m.into_owned(); println!(\"{s}\");",
            "error[E0507]: cannot move out of `*m` which is behind a mutable reference",
        ),
    ];
    for (body, expected) in cases {
        let report = refused(&format!("{}\nmain();\n", program(body)));
        assert!(report.starts_with(expected), "{body}\n{report}");
    }
    let output = printed(
        r#"
use std::borrow::Cow;
fn main() {
    let c: Cow<str> = Cow::Borrowed("ok");
    let r = &c;
    let t = r.clone().into_owned();
    println!("{} {}", t, c.into_owned());
}
main();
"#,
    );
    assert_eq!(output, "ok ok\n");
}

#[test]
fn a_binding_arm_through_a_reference_keeps_its_loans() {
    // `match r { n => .. }` on a `&String` or a `&i32` binds the
    // reference, which keeps what it borrows (#118).
    refused_as(&[
        (
            "fn main() { let mut s = String::from(\"a\"); let r = &s; \
             let n = match r { n => n }; s.push('b'); println!(\"{} {}\", n, s); }",
            "error[E0502]: cannot borrow `s` as mutable because it is also borrowed as immutable",
        ),
        (
            "fn main() { let mut x = 1; let r = &x; let n = match r { n => n }; x = 2; \
             println!(\"{} {}\", n, x); }",
            "error[E0506]: cannot assign to `x` because it is borrowed",
        ),
    ]);
    let output = printed(
        r#"
fn main() {
    let mut x = 5;
    let r = &x;
    let m = match r { n => *n + 1 };
    let k = match r { 0 => 0, n if *n > 3 => n + 10, n => n * 2 };
    x = 1;
    let mut s = String::from("ab");
    let t = &s;
    let u = match t { n if n.len() > 5 => n.clone(), n => format!("{}!", n) };
    let l = match t { n => n.len() };
    s.push('c');
    let w: &str = "hi";
    let z = match &w { &"x" => 0, n => n.len() };
    println!("{} {} {} {} {} {} {} {}", m, k, x, u, l, s, z, match &s { n => n.len() });
}
main();
"#,
    );
    assert_eq!(output, "6 15 1 ab! 2 abc 2 3\n");
}

#[test]
fn returning_a_borrow_of_a_reference_local_is_e0515() {
    // `&r` returned as a `&&T` borrows the local `r` (#157). Returned
    // where a `&T` is, a deref coercion reborrows what `r` refers to.
    let local = "error[E0515]: cannot return reference to local variable `r`";
    refused_as(&[
        (
            "fn bad<'a>(v: &'a Vec<i32>) -> &'a &'a Vec<i32> { let r = v; &r }
fn main() { let v = vec![1]; println!(\"{:?}\", bad(&v)); }",
            local,
        ),
        (
            "fn bad<'a>(v: &'a str) -> &'a &'a str { let r = v; return &r; }
fn main() { println!(\"{:?}\", bad(\"a\")); }",
            local,
        ),
    ]);
    let output = printed(
        r#"
fn good<'a>(v: &'a Vec<i32>) -> &'a Vec<i32> { let r = v; &r }
fn text<'a>(v: &'a String) -> &'a str { let r = v; &r }
fn main() {
    let v = vec![1];
    let s = String::from("x");
    println!("{:?} {}", good(&v), text(&s));
}
main();
"#,
    );
    assert_eq!(output, "[1] x\n");
}

#[test]
fn a_local_stored_through_a_parameter_does_not_live_long_enough() {
    // What a body stores through a parameter, `self` among them, outlives
    // the body (#171).
    let short = "error[E0597]: `x` does not live long enough";
    refused_as(&[
        (
            "struct Holder<'a> { r: &'a i32 }
impl<'a> Holder<'a> { fn local(&mut self) { let x = 5; self.r = &x; } }
fn main() { let a = 1; let mut h = Holder { r: &a }; h.local(); println!(\"{}\", h.r); }",
            short,
        ),
        (
            "struct Holder<'a> { r: &'a i32 }
fn local<'a>(h: &mut Holder<'a>) { let x = 5; h.r = &x; }
fn main() { let a = 1; let mut h = Holder { r: &a }; local(&mut h); println!(\"{}\", h.r); }",
            short,
        ),
        (
            "fn local<'a>(v: &mut Vec<&'a i32>) { let x = 5; v.push(&x); }
fn main() { let a = 1; let mut v = vec![&a]; local(&mut v); println!(\"{:?}\", v); }",
            short,
        ),
        (
            "fn push<'a>(v: &mut Vec<&'a String>) { v.push(&String::from(\"t\")); }
fn main() { let a = String::new(); let mut v = vec![&a]; push(&mut v); println!(\"{:?}\", v); }",
            "error[E0716]: temporary value dropped while borrowed",
        ),
        (
            "struct Holder<'a> { r: &'a i32 }
impl<'a> Holder<'a> { fn store(&mut self, y: &i32) { self.r = y; } }
fn main() { let a = 1; let mut h = Holder { r: &a }; h.store(&a); println!(\"{}\", h.r); }",
            "error[E0621]: explicit lifetime required in the type of `y`",
        ),
    ]);
    let output = printed(
        r#"
struct Holder<'a> { r: &'a i32 }
impl<'a> Holder<'a> {
    fn five(&mut self) { self.r = &5; }
    fn through(&mut self, y: &'a i32) { self.r = &*y; }
}
fn push<'a>(v: &mut Vec<&'a i32>, y: &'a i32) { v.push(&7); v.push(y); v.push(&*y); }
fn main() {
    let a = 1;
    let b = 2;
    let mut h = Holder { r: &a };
    h.five();
    print!("{} ", h.r);
    h.through(&b);
    let mut v = vec![&a];
    push(&mut v, &b);
    println!("{} {:?}", h.r, v);
}
main();
"#,
    );
    assert_eq!(output, "5 2 [1, 7, 2, 2]\n");
}

#[test]
fn a_structs_declared_lifetime_bounds_place_its_fields() {
    // `'b: 'a` puts what is in `'b` in `'a` too, and `T: 'a` what is in
    // `T`, while a field read in `'b` holds only its own borrows (#172).
    let output = printed(
        r#"
struct P<'a, 'b: 'a> { x: &'a i32, y: &'b i32 }
struct W<'a, T: 'a> { x: &'a i32, t: T }
fn shorter<'a, 'b>(p: &P<'a, 'b>) -> &'a i32 { p.y }
fn main() {
    let mut a = 1;
    let b = 2;
    let c = 3;
    let mut p = P { x: &a, y: &b };
    println!("{} {}", p.x, shorter(&p));
    p.y = &c;
    let r = p.y;
    a += 1;
    let w = W { x: &b, t: &c };
    println!("{} {} {} {}", r, a, w.x, w.t);
}
main();
"#,
    );
    assert_eq!(output, "1 2\n3 2 2 3\n");
    let assigned =
        |name: &str| format!("error[E0506]: cannot assign to `{name}` because it is borrowed");
    let (b, c) = (assigned("b"), assigned("c"));
    refused_as(&[
        (
            "struct P<'a, 'b: 'a> { x: &'a i32, y: &'b i32 }
fn main() { let a = 1; let mut b = 2; let p = P { x: &a, y: &b }; let r = p.x; b += 1; \
             println!(\"{} {}\", r, b); }",
            &b,
        ),
        (
            "struct Q<'a, 'b, 'c> where 'c: 'b, 'b: 'a { x: &'a i32, y: &'b i32, z: &'c i32 }
fn main() { let a = 1; let b = 2; let mut c = 3; let q = Q { x: &a, y: &b, z: &c }; \
             let r = q.x; c += 1; println!(\"{} {}\", r, c); }",
            &c,
        ),
        (
            "struct W<'a, T: 'a> { x: &'a i32, t: T }
fn main() { let a = 1; let mut b = 2; let w = W { x: &a, t: &b }; let r = w.x; b += 1; \
             println!(\"{} {}\", r, b); }",
            &b,
        ),
        (
            "struct P<'a, 'b: 'a> { x: &'a i32, y: &'b i32 }
fn longer<'a, 'b>(p: &P<'a, 'b>) -> &'b i32 { p.x }
fn main() { let a = 1; let p = P { x: &a, y: &a }; println!(\"{}\", longer(&p)); }",
            "error: lifetime may not live long enough",
        ),
    ]);
}

#[test]
fn a_slice_patterns_rest_binding_holds_its_borrow() {
    // `rest @ ..` borrows the subslice it binds, apart from the elements
    // the pattern names on either side (#116).
    let pushed =
        "error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable";
    refused_as(&[
        (
            "fn main() { let mut v = vec![1, 2, 3]; let s = v.as_slice(); \
             if let [_, rest @ ..] = s { v.push(4); println!(\"{:?}\", rest); } }",
            pushed,
        ),
        (
            "fn main() { let mut v = vec![1, 2, 3]; \
             match v.as_slice() { [first, rest @ ..] => { v.clear(); println!(\"{} {:?}\", first, rest); } _ => {} } }",
            pushed,
        ),
    ]);
    let output = printed(
        r#"
fn main() {
    let mut v = vec![1, 2, 3];
    if let [first, rest @ ..] = v.as_slice() {
        println!("{} {:?}", first, rest);
    }
    v.push(4);
    let names = [String::from("a"), String::from("b"), String::from("c")];
    let [head, tail @ ..] = names;
    println!("{} {:?}", head, tail);
    let mut a = [1, 2, 3, 4];
    let [p, mid @ .., q] = &a;
    println!("{} {:?} {}", p, mid, q);
    a[0] = 5;
    match &v[..] {
        [] => println!("empty"),
        [only] => println!("{}", only),
        [.., last] => println!("{} {:?}", last, a),
    }
}
main();
"#,
    );
    assert_eq!(
        output,
        "1 [2, 3]\na [\"b\", \"c\"]\n1 [2, 3] 4\n4 [5, 2, 3, 4]\n"
    );
}

/// Asserts that rawr refuses each program with the first line rustc gives.
fn refused_as(cases: &[(&str, &str)]) {
    for (program, expected) in cases {
        let report = refused(&format!("{program}\nmain();\n"));
        assert!(
            report.starts_with(expected),
            "{program}\nexpected: {expected}\n{report}"
        );
    }
}

#[test]
fn a_cows_to_mut_makes_it_owned_with_nothing_written() {
    // std's `to_mut` clones a borrowed value into the `Cow` at the call,
    // through a local, a `&mut` parameter and a field (#818).
    let output = printed(
        r#"
use std::borrow::Cow;
struct Holder<'a> {
    c: Cow<'a, str>,
}
fn touch(c: &mut Cow<str>) {
    let _ = c.to_mut();
}
fn main() {
    let mut c: Cow<str> = Cow::Borrowed("x");
    let _ = c.to_mut();
    let mut d: Cow<str> = Cow::Borrowed("y");
    d.to_mut().push('!');
    let mut e: Cow<str> = Cow::Borrowed("z");
    touch(&mut e);
    let mut h = Holder { c: Cow::Borrowed("w") };
    h.c.to_mut();
    let v = [1, 2];
    let mut s: Cow<[i32]> = Cow::Borrowed(&v);
    s.to_mut().push(3);
    println!(
        "{} {d} {} {} {} {} {s:?}",
        matches!(c, Cow::Owned(_)),
        matches!(d, Cow::Owned(_)),
        matches!(e, Cow::Owned(_)),
        matches!(h.c, Cow::Owned(_)),
        matches!(s, Cow::Owned(_)),
    );
}
main();
"#,
    );
    assert_eq!(output, "true y! true true true true [1, 2, 3]\n");
}

#[test]
fn std_writes_into_stdout_and_stderr_by_io_write() {
    // `write!` and `writeln!` into any writer expression are its
    // `write_fmt`, as rustc expands them, and `Stdout`, `Stderr` and their
    // locks write where `print!` and `eprint!` do (#2089).
    let output = printed(
        r#"
use std::io::Write;
fn emit<W: Write>(w: &mut W, n: i32) -> std::io::Result<()> {
    writeln!(w, "n = {n}")
}
fn main() {
    writeln!(std::io::stdout().lock(), "a {}", 1).unwrap();
    let mut out = std::io::stdout();
    write!(out, "b ").unwrap();
    writeln!(&mut out, "c").unwrap();
    out.flush().unwrap();
    let mut lock = std::io::stdout().lock();
    lock.write_all(b"d\n").unwrap();
    println!("{:?}", lock.write(b"e\n").unwrap());
    writeln!(std::io::stderr(), "f").unwrap();
    let mut buffer: Vec<u8> = Vec::new();
    emit(&mut buffer, 2).unwrap();
    print!("{}", String::from_utf8(buffer).unwrap());
    let _ = writeln!(std::io::stdout());
}
main();
"#,
    );
    assert_eq!(output, "a 1\nb c\nd\ne\n2\nf\nn = 2\n\n");
    // Without `io::Write` in scope, rustc's E0599 at the writer, which
    // suggests `io`'s `Write`.
    let report = refused("fn main() {\n    writeln!(std::io::stdout(), \"x\").unwrap();\n}\n");
    assert!(
        report.starts_with("error[E0599]: cannot write into `Stdout`"),
        "{report}"
    );
    assert!(report.contains("1 + use std::io::Write;"), "{report}");
}

#[test]
fn std_command_runs_a_process_its_engine_grants() {
    // std's `Command` builds in place, through a chain on a temporary and
    // through a local, and its `status` is the child's (#2082).
    let program = r#"
use std::process::Command;
fn main() {
    let ok = Command::new("sh").arg("-c").arg("exit 0").status().unwrap();
    let status = Command::new("sh")
        .arg("-c")
        .arg("test \"$X\" = y && test \"$(pwd)\" = / && exit 3")
        .env("X", "y")
        .current_dir("/")
        .status()
        .unwrap();
    let mut cmd = Command::new("sh");
    cmd.arg("-c");
    cmd.arg("exit 4");
    let code = cmd.status().unwrap().code();
    let program = Command::new("ls");
    println!(
        "{} {:?} {} {:?} {status} {:?}",
        ok.success(),
        ok.code(),
        status.success(),
        code,
        program.get_program()
    );
    let missing = Command::new("no-such-program-rawr").status();
    println!("{:?}", missing.map_err(|e| e.kind()));
}
main();
"#;
    assert_eq!(
        printed_with(program, crate::Access::all()),
        "true Some(0) false Some(4) exit status: 3 \"ls\"\nErr(NotFound)\n"
    );
    // A program its engine doesn't grant the process is refused as the
    // operating system would refuse it.
    let output = printed(
        "fn main() {\n    let run = std::process::Command::new(\"true\").status();\n    \
         println!(\"{:?}\", run.map_err(|e| e.kind()));\n}\nmain();\n",
    );
    assert_eq!(output, "Err(PermissionDenied)\n");
}

#[test]
fn std_command_args_and_output_run_as_stds() {
    // `args` takes any iterable of `AsRef<OsStr>` items, and `output`
    // collects the child's stdout and stderr into `Output`'s public fields
    // (#2082).
    let program = r#"
use std::process::Command;
fn main() {
    let out = Command::new("sh").args(["-c", "echo hi; echo oops >&2; exit 2"]).output().unwrap();
    println!("{} {:?}", out.status.success(), out.status.code());
    print!("{}", String::from_utf8_lossy(&out.stdout));
    println!("{:?} {}", out.stderr, out.stdout.len());
    let words = vec!["a", "b"];
    let listed = Command::new("echo").args(words.iter().map(|w| w.to_uppercase())).arg("c").output().unwrap();
    print!("{}", String::from_utf8(listed.stdout).unwrap());
    let mut cmd = Command::new("printf");
    cmd.args(&["%s-%s", "x"]);
    cmd.args(vec![String::from("y")]);
    let printed = cmd.output().unwrap().stdout;
    println!("{:?} {:?}", cmd, printed);
}
main();
"#;
    assert_eq!(
        printed_with(program, crate::Access::all()),
        "false Some(2)\nhi\n[111, 111, 112, 115, 10] 3\nA B c\n\"printf\" \"%s-%s\" \"x\" \"y\" [120, 45, 121]\n"
    );
}

#[test]
fn a_held_types_functions_take_its_borrowed_forms_by_any_path() {
    // `Path::new(&os)` and `OsString::from(&path)` select their borrowed
    // forms (#836), by an import or by the full path, and `OsStr::new` is
    // the text it is given.
    let output = printed(
        r#"
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
fn main() {
    let os = OsString::from("a/b");
    let path = PathBuf::from("x/y");
    println!("{} {}", Path::new(&os).display(), std::path::Path::new(&os).display());
    println!("{:?} {:?}", OsString::from(&path), OsStr::new("s"));
    println!("{:?} {}", std::ffi::OsStr::new(&os), std::path::Path::new("p/q").display());
}
main();
"#,
    );
    assert_eq!(output, "a/b a/b\n\"x/y\" \"s\"\n\"a/b\" p/q\n");
    // A member std declares and rawr doesn't bind is the not-yet error,
    // never an unknown member (#837).
    for (call, path) in [
        ("p.capacity()", "PathBuf::capacity"),
        ("PathBuf::with_capacity(3)", "PathBuf::with_capacity"),
        ("p.components()", "Path::components"),
    ] {
        let report = refused(&format!(
            "use std::path::PathBuf;\nfn main() {{\n    let p = PathBuf::new();\n    let _ = {call};\n}}\nmain();\n"
        ));
        assert!(
            report.contains(&format!("rawr doesn't run `{path}` yet")),
            "{report}"
        );
    }
}

#[test]
fn std_atomics_run_as_stds() {
    // Each atomic has its own impl's members (`Atomic<usize>`'s), its
    // value is shared by a static, a borrow and an `Arc` on threads, and
    // rustc names it by the type its alias names (#1549).
    let output = printed(
        r#"
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
static NEXT: AtomicUsize = AtomicUsize::new(1);
struct Counter {
    hits: AtomicI32,
}
fn main() {
    let a = AtomicUsize::new(5);
    let old = a.fetch_add(2, Ordering::SeqCst);
    println!("{old} {} {:?}", a.load(Ordering::Relaxed), a);
    println!("{:?} {:?}", a.compare_exchange(7, 9, Ordering::SeqCst, Ordering::SeqCst),
        a.compare_exchange_weak(7, 1, Ordering::SeqCst, Ordering::Relaxed));
    let b = AtomicBool::default();
    println!("{} {} {}", b.fetch_not(Ordering::SeqCst), b.swap(false, Ordering::AcqRel), b.into_inner());
    let small = AtomicU8::from(250);
    small.fetch_add(10, Ordering::SeqCst);
    let wide = AtomicU64::new(u64::MAX);
    println!("{:?} {}", small, wide.fetch_sub(1, Ordering::SeqCst));
    let counter = Counter { hits: AtomicI32::new(-4) };
    let borrowed = &counter;
    borrowed.hits.fetch_max(3, Ordering::SeqCst);
    println!("{:?}", counter.hits);
    let shared = Arc::new(AtomicUsize::new(0));
    let handles = (0..4).map(|_| {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            for _ in 0..100 {
                shared.fetch_add(1, Ordering::SeqCst);
            }
        })
    }).collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
    NEXT.fetch_add(1, Ordering::Relaxed);
    println!("{} {} {:?}", shared.load(Ordering::SeqCst), NEXT.load(Ordering::Relaxed), Ordering::SeqCst);
}
main();
"#,
    );
    assert_eq!(
        output,
        "5 7 7\nOk(7) Err(9)\nfalse true false\n4 18446744073709551615\n3\n400 2 SeqCst\n"
    );
    let report = refused(
        "use std::sync::atomic::*;\nfn main() {\n    let x: () = AtomicUsize::new(0);\n}\nmain();\n",
    );
    assert!(
        report.contains("expected `()`, found `Atomic<usize>`"),
        "{report}"
    );
    let report = refused(
        "use std::sync::atomic::*;\nfn main() {\n    AtomicUsize::new(0).nope();\n}\nmain();\n",
    );
    assert!(
        report.contains("no method named `nope` found for struct `Atomic<T>`")
            && report.contains("method not found in `Atomic<usize>`"),
        "{report}"
    );
}

#[test]
fn std_items_by_their_paths_and_impls() {
    // `std::rc::Rc::new` by its full path (#1253), `SystemTime`'s `Ord`
    // (#1471) and `SeekFrom`'s tuple variants built by their paths (#1978).
    let output = printed(
        r#"
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::time::{SystemTime, UNIX_EPOCH};
fn need(r: std::rc::Rc<i32>) -> i32 {
    *r
}
fn main() {
    let b = std::rc::Rc::clone(&std::rc::Rc::new(3));
    println!("{} {} {}", need(std::rc::Rc::new(1)), std::sync::Arc::new(2), b);
    let now = SystemTime::now();
    let ordered = (now >= UNIX_EPOCH, UNIX_EPOCH < now, now.max(UNIX_EPOCH) == now);
    println!("{:?} {:?}", ordered, UNIX_EPOCH.cmp(&now));
    let mut c = Cursor::new(vec![1u8, 2, 3, 4, 5]);
    let at = c.seek(SeekFrom::Start(2)).unwrap();
    let from = std::io::SeekFrom::End(-1);
    c.seek(from).unwrap();
    let mut rest = Vec::new();
    c.read_to_end(&mut rest).unwrap();
    println!("{at} {from:?} {rest:?}");
}
main();
"#,
    );
    assert_eq!(output, "1 2 3\n(true, true, true) Less\n2 End(-1) [5]\n");
}

#[test]
fn std_files_directories_and_metadata_run_as_stds() {
    // std's `File` and `OpenOptions` read, write, append and seek; `fs`'s
    // `metadata`, `read_dir` and `canonicalize` read the file system as far
    // as the program's access reaches (#646).
    let directory = std::env::temp_dir().join(format!("rawr-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let source = format!(
        r#"
use std::fs::{{self, File, OpenOptions}};
use std::io::{{Read, Seek, SeekFrom, Write}};
use std::path::PathBuf;
fn main() {{
    let dir = PathBuf::from({directory:?});
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.txt");
    let mut f = File::create(&p).unwrap();
    f.write_all(b"one\n").unwrap();
    writeln!(f, "two").unwrap();
    drop(f);
    let mut f = OpenOptions::new().append(true).open(&p).unwrap();
    f.write(b"three\n").unwrap();
    drop(f);
    let mut s = String::new();
    File::open(&p).unwrap().read_to_string(&mut s).unwrap();
    print!("{{s}}");
    let m = fs::metadata(&p).unwrap();
    println!("{{}} {{}} {{}} {{}}", m.len(), m.is_file(), m.is_dir(), m.file_type().is_file());
    fs::write(dir.join("b.txt"), "x").unwrap();
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for entry in fs::read_dir(&dir).unwrap() {{
        let entry = entry.unwrap();
        if entry.path().ends_with("b.txt") {{
            let kind = entry.file_type().unwrap();
            println!("{{:?}} {{}} {{}}", names, entry.metadata().unwrap().len(), kind.is_dir());
        }}
    }}
    let mut f = File::open(&p).unwrap();
    f.seek(SeekFrom::Start(4)).unwrap();
    let mut rest = Vec::new();
    f.read_to_end(&mut rest).unwrap();
    println!("{{}} {{}}", rest.len(), f.stream_position().unwrap());
    let mut options = OpenOptions::new();
    options.write(true);
    let missing = options.open(dir.join("c.txt")).map_err(|e| e.kind()).err();
    options.create(true);
    options.open(dir.join("c.txt")).unwrap().write_all(b"hi").unwrap();
    let found = fs::canonicalize(dir.join(".").join("c.txt")).unwrap();
    println!("{{:?}} {{}} {{}}", missing, fs::read_to_string(&found).unwrap(), found.ends_with("c.txt"));
    let gone = File::open(dir.join("nope")).map_err(|e| e.kind()).err();
    println!("{{:?}}", gone);
}}
main();
"#
    );
    let output = printed_with(&source, crate::Access::all());
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(
        output,
        "one\ntwo\nthree\n14 true false true\n[\"a.txt\", \"b.txt\"] 1 false\n\
         10 14\nSome(NotFound) hi true\nSome(NotFound)\n"
    );
    // A program its engine doesn't grant the files is refused as the
    // operating system would refuse it.
    let output = printed(
        "fn main() {\n    let opened = std::fs::File::open(\"/\").map_err(|e| e.kind());\n    \
         let listed = std::fs::read_dir(\"/\").map(|_| ()).map_err(|e| e.kind());\n    \
         println!(\"{:?} {:?}\", opened.err(), listed);\n}\nmain();\n",
    );
    assert_eq!(output, "Some(PermissionDenied) Err(PermissionDenied)\n");
}

#[test]
fn impls_for_two_instances_of_a_type_each_run_where_rustc_selects_them() {
    // `impl Describe for Foo<i32>` beside `Foo<String>`, and for `Vec<i64>`
    // beside `Vec<String>`, are two impls keyed by their full Self type:
    // a method call, a qualified path, a literal's fallback and an
    // associated type each select the impl rustc selects (#1178).
    let output = printed(
        r#"
trait Describe {
    type Out;
    fn describe(&self) -> String;
    fn get(&self) -> Self::Out;
}
struct Foo<T> { value: T }
impl Describe for Foo<i32> {
    type Out = i32;
    fn describe(&self) -> String { format!("i32 {}", self.value) }
    fn get(&self) -> i32 { self.value + 1 }
}
impl Describe for Foo<String> {
    type Out = String;
    fn describe(&self) -> String { format!("String {}", self.value) }
    fn get(&self) -> String { format!("{}!", self.value) }
}
trait Count { fn count(&self) -> String; }
impl Count for Vec<i64> { fn count(&self) -> String { format!("ints {}", self.len()) } }
impl Count for Vec<String> { fn count(&self) -> String { format!("texts {}", self.join("+")) } }
fn main() {
    let a = Foo { value: 1i32 };
    let b = Foo { value: "x".to_string() };
    println!("{} | {}", a.describe(), b.describe());
    println!("{} | {}", Describe::describe(&a), <Foo<String> as Describe>::describe(&b));
    println!("{}", Foo { value: 7 }.describe());
    let n: <Foo<i32> as Describe>::Out = a.get();
    let s: <Foo<String> as Describe>::Out = b.get();
    println!("{} {}", n.pow(2), s.to_uppercase());
    let v = vec![1, 2];
    println!("{} | {}", v.count(), vec!["p".to_string(), "q".to_string()].count());
}
main();
"#,
    );
    assert_eq!(
        output,
        "i32 1 | String x\ni32 1 | String x\ni32 7\n4 X!\nints 2 | texts p+q\n"
    );
}

#[test]
fn inherent_impls_for_two_instances_of_a_type_are_two_impls() {
    // `impl Foo<i32>` and `impl Foo<i64>` may each have `make` and `name`;
    // the receiver's or the written type selects one, and a literal's type
    // that fits both is rustc's E0034 (#1178).
    let output = printed(
        r#"
struct Foo<T> { value: T }
impl Foo<i32> {
    const N: i32 = 1;
    fn make() -> Self { Foo { value: 3 } }
    fn name(&self) -> String { format!("i32 {}", self.value) }
}
impl Foo<i64> {
    const N: i32 = 2;
    fn make() -> Self { Foo { value: 4 } }
    fn name(&self) -> String { format!("i64 {}", self.value) }
}
fn main() {
    let a = Foo { value: 1i32 };
    let b: Foo<i64> = Foo::<i64>::make();
    println!("{} | {} | {}", a.name(), b.name(), Foo::<i32>::make().name());
    println!("{} {}", Foo::<i32>::N + Foo::<i64>::N, Foo::<i64>::name(&b));
}
main();
"#,
    );
    assert_eq!(output, "i32 1 | i64 4 | i32 3\n3 i64 4\n");
    let report = refused(
        r#"
struct Foo<T> { value: T }
impl Foo<i32> { fn name(&self) -> i32 { self.value } }
impl Foo<i64> { fn name(&self) -> i64 { self.value } }
fn main() {
    let y = Foo { value: 7 };
    println!("{}", y.name());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0034]: multiple applicable items in scope"),
        "{report}"
    );
}

#[test]
fn std_impls_for_two_instances_of_a_type_each_run_for_their_own() {
    // `Display`, `Debug`, `Clone`, `PartialEq`, `Ord` and `Default`
    // written for `Foo<i32>` and for `Foo<i64>` run the impl of the
    // value's own instance (#1178).
    let output = printed(
        r#"
use std::cmp::Ordering;
use std::fmt;
#[derive(PartialEq, Eq)]
struct Foo<T> { value: T }
impl fmt::Display for Foo<i32> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "i32 {}", self.value) }
}
impl fmt::Display for Foo<i64> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "i64 {}", self.value) }
}
impl fmt::Debug for Foo<i32> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "I32({})", self.value) }
}
impl fmt::Debug for Foo<i64> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "I64({})", self.value) }
}
impl Clone for Foo<i32> { fn clone(&self) -> Self { Foo { value: self.value + 10 } } }
impl Clone for Foo<i64> { fn clone(&self) -> Self { Foo { value: self.value + 20 } } }
impl Default for Foo<i32> { fn default() -> Self { Foo { value: 5 } } }
impl Default for Foo<i64> { fn default() -> Self { Foo { value: 6 } } }
impl PartialOrd for Foo<i32> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for Foo<i32> { fn cmp(&self, other: &Self) -> Ordering { self.value.cmp(&other.value) } }
impl PartialOrd for Foo<i64> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for Foo<i64> { fn cmp(&self, other: &Self) -> Ordering { other.value.cmp(&self.value) } }
fn main() {
    let a = Foo { value: 1i32 };
    let b = Foo { value: 2i64 };
    println!("{} | {} | {} | {}", a, b, a.to_string(), b.to_string());
    println!("{} | {} | {:?} {:?}", a.clone(), b.clone(), a, b);
    let c: Foo<i32> = Default::default();
    println!("{:?} {:?}", c, Foo::<i64>::default());
    println!("{} {}", a < Foo { value: 7 }, b < Foo { value: 9 });
    println!("{:?} {:?}", a.cmp(&Foo { value: 0 }), b.cmp(&Foo { value: 9 }));
}
main();
"#,
    );
    assert_eq!(
        output,
        "i32 1 | i64 2 | i32 1 | i64 2\ni32 11 | i64 22 | I32(1) I64(2)\nI32(5) I64(6)\n\
         true false\nGreater Greater\n"
    );
}

#[test]
fn an_iterator_passed_returned_or_kept_runs_only_as_far_as_it_is_pulled() {
    // A runtime iterator pulls its chain item by item (#407): an unbounded
    // one passed to a function returns, and a closure's side effects come
    // when rustc's do.
    let output = printed(
        r#"
fn first<I: Iterator<Item = usize>>(i: I) -> Vec<usize> { i.take(3).collect() }
fn head<I: Iterator>(mut i: I) -> Option<I::Item> { i.next() }
fn evens() -> impl Iterator<Item = u32> { (0u32..).filter(|x| x % 2 == 0) }
fn noisy(x: u32) -> u32 { println!("made {x}"); x }
fn main() {
    println!("{:?} {:?}", first(1usize..), head(0u32..));
    println!("{:?}", head((0..4u32).map(noisy)));
    let mut m = (0..5u32).map(noisy);
    println!("{:?} {:?}", m.next(), m.next());
    let firsts: Vec<u32> = m.by_ref().take(1).collect();
    println!("{:?} {:?}", firsts, m.collect::<Vec<_>>());
    println!("{:?}", evens().skip(1).take(2).collect::<Vec<_>>());
}
main();
"#,
    );
    assert_eq!(
        output,
        "[1, 2, 3] Some(0)\nmade 0\nSome(0)\nmade 0\nmade 1\nSome(0) Some(1)\nmade 2\nmade 3\n\
         made 4\n[2] [3, 4]\n[2, 4]\n"
    );
}

#[test]
fn an_impl_over_an_iterator_parameter_iterates_its_value() {
    // `impl<I: Iterator<Item = X>>`: a field of type `I` has the iterator
    // methods, run on whatever iterator it holds at run time (#2507).
    let output = printed(
        r#"
struct Count { n: u32 }
impl Iterator for Count {
    type Item = u32;
    fn next(&mut self) -> Option<u32> { if self.n < 3 { self.n += 1; Some(self.n) } else { None } }
}
struct W<I>(I);
impl<I: Iterator<Item = i32>> W<I> {
    fn all(self) -> Vec<i32> { self.0.collect() }
}
struct U<I>(I);
impl<I: Iterator<Item = u32>> U<I> {
    fn step(&mut self) -> Option<u32> { self.0.next() }
    fn total(self) -> u32 {
        let mut t = 0;
        for x in self.0 { t += x; }
        t
    }
}
fn main() {
    println!("{:?}", W(vec![1, 2].into_iter()).all());
    let mut u = U((10u32..).map(|x| { println!("made {x}"); x }));
    println!("{:?} {:?}", u.step(), u.step());
    println!("{} {}", U(Count { n: 0 }).total(), U(0..4u32).total());
}
main();
"#,
    );
    assert_eq!(output, "[1, 2]\nmade 10\nmade 11\nSome(10) Some(11)\n6 6\n");
}

#[test]
fn an_associated_iterator_type_is_iterated_by_its_value() {
    // `type It: Iterator<Item = u32>` set to a program's iterator struct or
    // to a range: a generic body steps either by what it is (#380).
    let output = printed(
        r#"
struct Count { n: u32 }
impl Iterator for Count {
    type Item = u32;
    fn next(&mut self) -> Option<u32> { if self.n < 3 { self.n += 1; Some(self.n) } else { None } }
}
trait Source { type It: Iterator<Item = u32>; fn items(&self) -> Self::It; }
struct S;
impl Source for S { type It = Count; fn items(&self) -> Count { Count { n: 0 } } }
struct R;
impl Source for R { type It = std::ops::Range<u32>; fn items(&self) -> Self::It { 7..9 } }
fn report<T: Source>(x: &T) -> String {
    let mut it = x.items();
    let first = it.next();
    let rest: Vec<u32> = it.collect();
    let mut n = 0;
    for v in x.items() { n += v; }
    format!("{:?} {:?} {} {}", first, rest, n, x.items().sum::<u32>())
}
fn main() {
    println!("{}", report(&S));
    println!("{}", report(&R));
}
main();
"#,
    );
    assert_eq!(output, "Some(1) [2, 3] 6 6\nSome(7) [8] 15 15\n");
}

#[test]
fn a_runtime_iterator_is_cloned_peeked_and_borrowed_where_it_is() {
    // A clone part way through goes on by itself; `&mut it` and `peek`
    // step the iterator in its place.
    let output = printed(
        r#"
fn two(i: &mut impl Iterator<Item = u32>) -> (Option<u32>, Option<u32>) { (i.next(), i.next()) }
fn main() {
    let mut chars = "a1b".chars();
    let mut seen = Vec::new();
    while let Some(c) = chars.next() {
        seen.push(format!("{c}{:?}", chars.clone().next()));
    }
    println!("{:?}", seen);
    let mut part = (0u32..4).map(|x| x * 10);
    part.next();
    let copy = part.clone();
    println!("{:?} {:?}", part.collect::<Vec<_>>(), copy.collect::<Vec<_>>());
    let mut it = (1u32..6).map(|x| x * 2);
    println!("{:?}", two(&mut it));
    for x in &mut it {
        if x > 6 { break; }
        println!("x {x}");
    }
    println!("{:?}", it.next());
    let mut p = (1..4).peekable();
    println!("{:?}", p.peek());
    println!("{:?}", p.next_if(|x| *x > 1));
    println!("{:?} {:?}", p.next_if_eq(&1), p.next_if_eq(&2));
    println!("{:?}", p.next());
}
main();
"#,
    );
    assert_eq!(
        output,
        "[\"aSome('1')\", \"1Some('b')\", \"bNone\"]\n[10, 20, 30] [10, 20, 30]\n\
         (Some(2), Some(4))\nx 6\nSome(10)\nSome(1)\nNone\nSome(1) Some(2)\nSome(3)\n"
    );
}

#[test]
fn language_a_float_constant_in_a_pattern_matches_its_value() {
    // A pattern naming a float `const`, free or associated, matches its
    // value, as rustc's; an `f32` one its value as an `f32` (#2508). A
    // suffixed number ends a `match` head.
    let output = printed(
        r#"
const F: f64 = 1.5;
const G: f64 = -F * 2.0;
const LO: f64 = 0.5;
const HI: f64 = LO * 4.0;
const H: f32 = 0.1;
const N: u8 = 3u8;
struct P;
impl P {
    const K: f64 = 2.5;
}
fn main() {
    for x in [1.5, 2.0, -3.0, 0.25, 2.5] {
        match x {
            F => println!("F {x}"),
            G => println!("G {x}"),
            LO..=HI => println!("in {x}"),
            P::K => println!("K {x}"),
            _ => println!("other {x}"),
        }
    }
    let y: f32 = 0.1;
    match y {
        H => println!("H"),
        _ => println!("not H"),
    }
    match 3u8 {
        N => println!("N"),
        _ => println!("not N"),
    }
}
main();
"#,
    );
    assert_eq!(output, "F 1.5\nin 2\nG -3\nother 0.25\nK 2.5\nH\nN\n");
}

#[test]
fn language_a_parameter_literal_takes_its_suffix() {
    // `1u8` in a parameter pattern is a `u8`, as rustc reads it (#2510):
    // E0005 names the `u8` values it misses, and on an `i32` it is E0308.
    assert_eq!(
        rendered("fn f((x, 1u8): (i32, u8)) -> i32 { x }\nfn main() {}\n"),
        concat!(
            "error[E0005]: refutable pattern in function argument\n",
            " --> main.rs:1:6\n",
            "  |\n",
            "1 | fn f((x, 1u8): (i32, u8)) -> i32 { x }\n",
            "  |      ^^^^^^^^ patterns `(_, 0_u8)` and `(_, 2_u8..=u8::MAX)` not covered\n",
            "  |\n",
            "  = note: the matched value is of type `(i32, u8)`\n",
        )
    );
    assert_eq!(
        rendered("fn f((x, 1u8): (i32, i32)) -> i32 { x }\nfn main() {}\n"),
        concat!(
            "error[E0308]: mismatched types\n",
            " --> main.rs:1:10\n",
            "  |\n",
            "1 | fn f((x, 1u8): (i32, i32)) -> i32 { x }\n",
            "  |          ^^^   ---------- expected due to this\n",
            "  |          |\n",
            "  |          expected `i32`, found `u8`\n",
        )
    );
    // In a body's pattern too, a suffixed literal is its own type.
    assert_eq!(
        rendered(
            "fn main() {\n    let t: (i32, i32) = (2, 1);\n    match t {\n        \
             (x, 1u8) => println!(\"{x}\"),\n        _ => {}\n    }\n}\n"
        ),
        concat!(
            "error[E0308]: mismatched types\n",
            " --> main.rs:4:13\n",
            "  |\n",
            "3 |     match t {\n",
            "  |           - this expression has type `(i32, i32)`\n",
            "4 |         (x, 1u8) => println!(\"{x}\"),\n",
            "  |             ^^^ expected `i32`, found `u8`\n",
        )
    );
}

#[test]
fn language_a_ref_arm_borrows_the_matched_value() {
    // `match x { ref v => .. }` binds `&x`, and `ref mut` the place, on a
    // struct, a `Vec`, an `Option`, a `String` or a scalar alike (#194).
    let output = printed(
        r#"
#[derive(Debug)]
struct P {
    x: i32,
}
fn main() {
    let p = P { x: 1 };
    let a = match p { ref q => q.x };
    let v = vec![1, 2];
    let b = match v { ref w if w.len() > 5 => 0, ref w => w.len() };
    let o = Some(String::from("o"));
    let c = match o { ref r => r.is_some() };
    let mut m = P { x: 5 };
    match m { ref mut q => q.x += 1 }
    let s = String::from("abc");
    let n = match s { ref v => v.len() };
    let k = 6;
    let j = match k { ref v => *v };
    let mut t = String::from("x");
    match t { ref mut w => w.push('y') }
    println!("{a} {b} {c} {:?} {:?} {:?} {:?} {n} {j} {s} {t}", p, v, o, m);
}
main();
"#,
    );
    assert_eq!(
        output,
        "1 2 true P { x: 1 } [1, 2] Some(\"o\") P { x: 6 } 3 6 abc xy\n"
    );
    // The borrow lasts as long as the binding is used.
    let report = refused(
        "fn main() {\n    let mut s = String::from(\"abc\");\n    match s {\n        \
         ref v => {\n            s.push('d');\n            println!(\"{v}\");\n        }\n    }\n}\n",
    );
    assert!(
        report.starts_with(
            "error[E0502]: cannot borrow `s` as mutable because it is also borrowed as immutable"
        ),
        "{report}"
    );
    let report = refused(
        "struct P {\n    x: i32,\n}\nfn main() {\n    let p = P { x: 1 };\n    \
         let r = match p {\n        ref q => q,\n    };\n    drop(p);\n    println!(\"{}\", r.x);\n}\n",
    );
    assert!(
        report.starts_with("error[E0505]: cannot move out of `p` because it is borrowed"),
        "{report}"
    );
}

#[test]
fn language_an_element_borrow_checks_its_index_where_it_is_made() {
    // `&mut a[i]` past the end panics where the borrow is made, as rustc's
    // does, not where it is first used (#410).
    assert_eq!(
        panicked(
            "fn main() {\n    let mut a = [1, 2, 3];\n    let v = vec![0; 9];\n    \
             let i = v.len();\n    let r = &mut a[i];\n    println!(\"made\");\n    *r = 4;\n}\n"
        ),
        "thread 'main' panicked at main.rs:5:18:\nindex out of bounds: the len is 3 but the index is 9"
    );
    assert_eq!(
        panicked(
            "struct G {\n    cells: [i32; 2],\n}\nfn main() {\n    let mut g = G { cells: [1, 2] };\n    \
             let mut m = [[1, 2], [3, 4]];\n    let k = g.cells.len() + 1;\n    \
             let c = &mut m[1][k - 2];\n    *c = 0;\n    let d = &mut g.cells[k];\n    \
             println!(\"made\");\n    *d = 1;\n}\n"
        ),
        "thread 'main' panicked at main.rs:10:18:\nindex out of bounds: the len is 2 but the index is 3"
    );
}

#[test]
fn language_a_primitive_op_assign_through_a_ref_cell_reads_its_right_side_first() {
    // A builtin `op=` of two primitives evaluates its right side before
    // its place, as rustc does, so the right side's `borrow()` ends before
    // the place's `borrow_mut()` (#3). A trait's `op=`, of a `String`,
    // borrows its place first, and the right side's `borrow()` panics.
    let output = printed(
        r#"
use std::cell::RefCell;
fn main() {
    let n = RefCell::new(1);
    *n.borrow_mut() += {
        let v = *n.borrow();
        v
    };
    let f = RefCell::new(1.5);
    *f.borrow_mut() *= {
        let v = *f.borrow();
        v
    };
    println!("primitive {} {}", n.borrow(), f.borrow());
}
main();
"#,
    );
    assert_eq!(output, "primitive 2 2.25\n");
    assert_eq!(
        panicked(
            "use std::cell::RefCell;\nfn main() {\n    let s = RefCell::new(String::from(\"a\"));\n    \
             *s.borrow_mut() += {\n        let v = s.borrow().clone();\n        &v.clone()\n    };\n}\n"
        )
        .lines()
        .next()
        .map(str::to_owned),
        Some("thread 'main' panicked at main.rs:5:19:".to_owned())
    );
}

#[test]
fn language_a_whole_optional_borrow_binds_beside_bindings_of_its_value() {
    // A catch-all binding of a whole `Option<&mut T>`, a local's or a
    // call's, beside an arm binding its value by `&mut`, holds the
    // reference to the element's place, as rustc's (#166).
    let output = printed(
        r#"
fn main() {
    let mut v = vec![1, 2, 3];
    match v.last_mut() {
        Some(x) if *x > 10 => *x += 1,
        whole => {
            if let Some(y) = whole {
                *y += 100
            }
        }
    }
    let mut w = vec![5, 50];
    let o = w.first_mut();
    match o {
        Some(x) if *x > 10 => *x += 1,
        None => {}
        whole => {
            if let Some(y) = whole {
                *y += 7;
            }
        }
    }
    let mut u = vec![20];
    match u.first_mut() {
        Some(x) if *x > 10 => *x += 1,
        whole => println!("{:?}", whole),
    }
    println!("{:?} {:?} {:?}", v, w, u);
}
main();
"#,
    );
    assert_eq!(output, "[1, 2, 103] [12, 50] [21]\n");
}

#[test]
fn language_a_part_with_drop_glue_moves_out_below_a_field() {
    // A move out of a field below a field, or of a member below a member,
    // whose type implements `Drop` leaves the rest of the local to drop
    // where rustc drops it, the moved part with its new owner (#196).
    let output = printed(
        r#"
struct D(i32);
impl Drop for D {
    fn drop(&mut self) {
        println!("drop D{}", self.0);
    }
}
struct I {
    d: D,
    e: D,
}
struct O {
    i: I,
    f: D,
}
impl O {
    fn take(self) -> D {
        println!("taking");
        self.i.e
    }
}
fn main() {
    let t = (D(1), (D(2), D(3)));
    let a = t.1.0;
    println!("moved {}", a.0);
    let u = (D(4), D(5));
    let b = u.1;
    println!("moved {}", b.0);
    let mut o = O { i: I { d: D(6), e: D(7) }, f: D(8) };
    let c = o.i.d;
    o.i.d = D(9);
    println!("moved {}", c.0);
    let p = O { i: I { d: D(10), e: D(11) }, f: D(12) };
    let e = p.take();
    println!("took {}", e.0);
    println!("end");
}
main();
"#,
    );
    assert_eq!(
        output,
        "moved 2\nmoved 5\nmoved 6\ntaking\ndrop D10\ndrop D12\ntook 11\nend\ndrop D11\n\
         drop D6\ndrop D9\ndrop D7\ndrop D8\ndrop D5\ndrop D4\ndrop D2\ndrop D1\ndrop D3\n"
    );
}

#[test]
fn language_a_qualified_path_without_a_trait_is_the_types_path() {
    // `<T>::f(..)` means `T::f(..)`, with the turbofish a type's arguments
    // take in an expression (#378).
    let output = printed(
        r#"
struct S;
impl S {
    fn make() -> i32 { 7 }
    fn twice(&self, x: i32) -> i32 { x * 2 }
}
trait Tr { fn tr() -> i32; }
impl Tr for S { fn tr() -> i32 { 9 } }
fn main() {
    println!("{} {} {}", <S>::make(), <S>::twice(&S, 4), <S>::tr());
    println!("{} {} {}", <i32>::max(3, 5), <Vec<i32>>::new().len(), <str>::len("abc"));
}
main();
"#,
    );
    assert_eq!(output, "7 8 9\n5 0 3\n");
}

#[test]
fn language_question_mark_converts_an_error_through_std_from() {
    // `?` returns `Err(From::from(e))`, through std's impls as through a
    // program's: `&str` into `String`, `u8` into `u64` (#1034).
    let output = printed(
        r#"
fn pick(x: Option<i32>) -> Result<i32, String> {
    let v = x.ok_or("missing")?;
    Ok(v + 1)
}
fn pick2(x: Result<i32, &str>) -> Result<i32, String> { Ok(x? * 2) }
fn wide(x: Result<u8, u8>) -> Result<u8, u64> { Ok(x?) }
fn main() {
    println!("{:?} {:?} {:?} {:?}", pick(Some(1)), pick(None), pick2(Err("bad")), wide(Err(7)));
}
main();
"#,
    );
    assert_eq!(output, "Ok(2) Err(\"missing\") Err(\"bad\") Err(7)\n");
    let report = refused(
        "fn bad(x: Result<i32, bool>) -> Result<i32, String> { Ok(x?) }\nfn main() {}\nmain();\n",
    );
    assert!(
        report.starts_with("error[E0277]: `?` couldn't convert the error to `String`\n"),
        "{report}"
    );
}

#[test]
fn language_an_error_in_an_arm_where_a_boxed_error_is_expected_is_e0308() {
    // The expected `Result<_, Box<dyn Error>>` reaches each arm's
    // `Err(..)`: a bare error there is rustc's E0308, and a boxed one
    // coerces (#1430).
    let declarations = "use std::error::Error;\nuse std::fmt;\n#[derive(Debug)]\nstruct MyErr;\n\
        impl fmt::Display for MyErr {\n    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, \"my\") }\n}\n\
        impl Error for MyErr {}\n";
    for body in [
        "if c { Ok(1) } else { Err(MyErr) }",
        "match c { true => Ok(1), false => Err(MyErr) }",
    ] {
        let report = refused(&format!(
            "{declarations}fn run(c: bool) -> Result<i32, Box<dyn Error>> {{\n    {body}\n}}\n\
             fn main() {{ println!(\"{{:?}}\", run(true).is_ok()); }}\nmain();\n"
        ));
        assert!(
            report.starts_with("error[E0308]: mismatched types\n"),
            "{report}"
        );
        assert!(
            report.contains("expected `Box<dyn Error>`, found `MyErr`"),
            "{report}"
        );
    }
    let output = printed(&format!(
        "{declarations}fn run(c: bool) -> Result<i32, Box<dyn Error>> {{\n    \
         if c {{ Ok(1) }} else {{ Err(Box::new(MyErr)) }}\n}}\n\
         fn main() {{ println!(\"{{:?}} {{}}\", run(true).is_ok(), run(false).unwrap_err()); }}\nmain();\n"
    ));
    assert_eq!(output, "true my\n");
}

#[test]
fn language_a_relaxed_sized_bound_bounds_nothing() {
    // `?Sized` in a parameter's bounds and in a `where` clause (#801); no
    // other trait is relaxed.
    let output = printed(
        r#"
use std::fmt::Display;
fn show<T: Display + ?Sized>(t: &T) -> String { format!("<{}>", t) }
fn wrap<T>(t: &T) -> String where T: ?Sized + Display { format!("[{}]", t) }
fn main() {
    let s: &str = "hi";
    println!("{} {} {}", show(s), show(&5), wrap("ab"));
}
main();
"#,
    );
    assert_eq!(output, "<hi> <5> [ab]\n");
    let report = refused("fn f<T: ?Clone>(t: &T) {}\nfn main() {}\nmain();\n");
    assert!(
        report.starts_with("error: bound modifier `?` can only be applied to `Sized`\n"),
        "{report}"
    );
}

#[test]
fn language_a_method_where_self_is_sized_is_no_method_of_dyn_trait() {
    // `where Self: Sized` keeps a method out of `dyn Trait`, so it may take
    // no `self` or return `Self` (#802). Outside an impl or a trait there
    // is no `Self` (E0411).
    let output = printed(
        r#"
trait Shape {
    fn area(&self) -> f64;
    fn doubled(self) -> f64 where Self: Sized { self.area() * 2.0 }
    fn make() -> Self where Self: Sized;
}
struct Sq(f64);
impl Shape for Sq {
    fn area(&self) -> f64 { self.0 * self.0 }
    fn make() -> Self { Sq(3.0) }
}
fn main() {
    let b: Box<dyn Shape> = Box::new(Sq(2.0));
    println!("{} {} {}", b.area(), Sq(2.0).doubled(), Sq::make().area());
}
main();
"#,
    );
    assert_eq!(output, "4 8 9\n");
    let report = refused("fn f() where Self: Sized {}\nfn main() {}\nmain();\n");
    assert!(
        report.starts_with("error[E0411]: cannot find type `Self` in this scope\n"),
        "{report}"
    );
}

#[test]
fn language_a_generic_traits_associated_type_bound_names_its_parameters() {
    // `type Out: Iterator<Item = T>` of `trait Conv<T>` is the iterator of
    // the bound's or the impl's `T` (#382), and an impl whose type yields
    // another is rustc's E0271.
    let output = printed(
        r#"
trait Conv<T> {
    type Out: Iterator<Item = T>;
    fn conv(&self) -> Self::Out;
}
struct Up(u32);
impl Conv<u32> for Up {
    type Out = std::ops::Range<u32>;
    fn conv(&self) -> Self::Out { 0..self.0 }
}
fn total<C: Conv<u32>>(c: &C) -> u32 { c.conv().sum() }
fn firsts<T, C: Conv<T>>(c: &C) -> Option<T> { c.conv().next() }
fn main() {
    let s: Vec<u32> = Up(4).conv().collect();
    let f: Option<u32> = firsts(&Up(3));
    println!("{:?} {} {:?}", s, total(&Up(5)), f);
}
main();
"#,
    );
    assert_eq!(output, "[0, 1, 2, 3] 10 Some(0)\n");
    let report = refused(
        "trait Conv<T> { type Out: Iterator<Item = T>; }\nstruct Up;\n\
         impl Conv<u64> for Up { type Out = std::ops::Range<u32>; }\nfn main() {}\nmain();\n",
    );
    assert!(
        report.starts_with(
            "error[E0271]: expected `Range<u32>` to be an iterator that yields `u64`, but it \
             yields `u32`\n"
        ),
        "{report}"
    );
}

#[test]
fn language_blanket_impls_are_probed_by_the_bounds_that_hold() {
    // Two blanket impls of different traits give `shout`: the one whose
    // bounds hold for the receiver is called, and two that hold are
    // rustc's E0034. `T::Item` in a blanket impl's signature is the
    // receiver's item (#1555).
    let declarations = "trait Named { fn name(&self) -> String; }\n\
        trait Loud { fn shout(&self) -> String; }\n\
        impl<T: Named> Loud for T { fn shout(&self) -> String { self.name().to_uppercase() } }\n\
        trait Quiet { fn shout(&self) -> String; }\n\
        impl<T: Clone> Quiet for T { fn shout(&self) -> String { \"shh\".into() } }\n";
    let output = printed(&format!(
        "{declarations}struct Dog;\nimpl Named for Dog {{ fn name(&self) -> String {{ \"dog\".into() }} }}\n\
         trait Or {{ type Out; fn or(self, d: Self::Out) -> Self::Out; }}\n\
         impl<T: Iterator> Or for T {{ type Out = T::Item; fn or(self, d: T::Item) -> T::Item {{ d }} }}\n\
         fn main() {{ println!(\"{{}} {{}} {{}}\", Dog.shout(), 5.shout(), vec![4, 5].into_iter().or(9)); }}\nmain();\n"
    ));
    assert_eq!(output, "DOG shh 9\n");
    let report = refused(&format!(
        "{declarations}#[derive(Clone)]\nstruct Cat;\nimpl Named for Cat {{ fn name(&self) -> String {{ \"cat\".into() }} }}\n\
         fn main() {{ println!(\"{{}}\", Cat.shout()); }}\nmain();\n"
    ));
    assert!(
        report.starts_with("error[E0034]: multiple applicable items in scope\n"),
        "{report}"
    );
}

#[test]
fn a_prelude_type_imported_under_another_name_works_in_bodies() {
    // `use std::string::String as Text;` names `String` in a body's paths,
    // types and patterns, as in a signature (#810).
    let output = printed(
        r#"
use std::option::Option as Maybe;
use std::string::String as Text;
use std::vec::Vec as List;
fn twice(t: Text) -> Text {
    t.repeat(2)
}
fn main() {
    let t: Text = Text::from("ab");
    let mut l: List<Text> = List::new();
    l.push(twice(t));
    let m: Maybe<usize> = Maybe::Some(l.len());
    match m {
        Maybe::Some(n) => println!("{n} {:?} {}", l, Text::with_capacity(4).len()),
        Maybe::None => println!("none"),
    }
}
main();
"#,
    );
    assert_eq!(output, "1 [\"abab\"] 0\n");
}

#[test]
fn a_public_glob_re_exports_its_module_s_names() {
    // `pub use inner::*;` binds inner's names in `outer` for every module
    // `outer`'s items reach, a std module's too (#1349).
    let output = printed(
        r#"
mod outer {
    mod inner {
        pub fn f() -> i32 {
            1
        }
        pub struct S(pub i32);
        pub trait T {
            fn t(&self) -> i32 {
                9
            }
        }
        impl T for S {}
    }
    pub use inner::*;
}
mod formatting {
    pub use std::fmt::*;
}
use outer::T;
struct X;
impl formatting::Display for X {
    fn fmt(&self, f: &mut formatting::Formatter) -> formatting::Result {
        write!(f, "x")
    }
}
fn main() {
    let s = outer::S(outer::f());
    println!("{} {} {X}", s.0, s.t());
}
main();
"#,
    );
    assert_eq!(output, "1 9 x\n");
    // Each name goes no further than its item: a `pub(super)` function
    // is the parent's, rustc's E0603 where another module names it.
    let report = refused(
        r#"
mod outer {
    mod inner {
        pub(super) fn g() -> i32 {
            2
        }
    }
    pub use inner::*;
    pub fn h() -> i32 {
        g()
    }
}
fn main() {
    println!("{} {}", outer::h(), outer::g());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0603]: function import `g` is private"),
        "{report}"
    );
}

#[test]
fn a_facade_type_s_trait_method_needs_its_trait_in_scope() {
    // `Stdout`'s `flush`, `Stdin`'s `read_to_string` and a `Cursor`'s
    // `read_to_end` are `Write`'s and `Read`'s: rustc's E0599 with no
    // `use` of them (#1427).
    for (program, headline, import) in [
        (
            "fn main() {\n    std::io::stdout().flush().unwrap();\n}\n",
            "error[E0599]: no method named `flush` found for struct `Stdout` in the current scope",
            "1 + use std::io::Write;",
        ),
        (
            "fn main() {\n    let mut s = String::new();\n    \
             std::io::stdin().read_to_string(&mut s).unwrap();\n}\n",
            "error[E0599]: no method named `read_to_string` found for struct `Stdin` in the \
             current scope",
            "1 + use std::io::Read;",
        ),
        (
            "use std::io::Cursor;\nfn main() {\n    let mut c = Cursor::new(vec![1u8]);\n    \
             let mut v = Vec::new();\n    c.read_to_end(&mut v).unwrap();\n}\n",
            "error[E0599]: no method named `read_to_end` found for struct `std::io::Cursor<T>` \
             in the current scope",
            "1 + use std::io::Read;",
        ),
    ] {
        let report = refused(&format!("{program}main();\n"));
        assert!(report.starts_with(headline), "{report}");
        assert!(report.contains(import), "{report}");
    }
    // With `BufRead` in scope, a `Cursor`'s `lines` are its own.
    let output = printed(
        r#"
use std::io::{BufRead, Cursor};
fn main() {
    for line in Cursor::new("a\nb\n").lines() {
        println!("{}", line.unwrap());
    }
}
main();
"#,
    );
    assert_eq!(output, "a\nb\n");
}

#[test]
fn std_iter_s_sources_run_as_far_as_the_chain_pulls() {
    // `repeat`, `once`, `empty`, `from_fn`, `successors`, `repeat_with`,
    // `repeat_n` and `once_with`, each a chain's source; a closure runs
    // once per item pulled.
    let output = printed(
        r#"
use std::iter::{self, from_fn, successors};
fn counter() -> impl Iterator<Item = u32> {
    let mut c = 0;
    from_fn(move || {
        c += 1;
        if c <= 3 { Some(c) } else { None }
    })
}
fn main() {
    let a: Vec<i32> = iter::repeat(7).take(3).collect();
    let b: Vec<i32> = iter::once(1).chain(iter::once(2)).collect();
    let mut k = 1;
    let c: Vec<i32> = iter::repeat_with(|| { k *= 2; k }).take(4).collect();
    let d: Vec<u32> = successors(Some(1u32), |x| if *x < 100 { Some(x * 3) } else { None }).collect();
    println!("{:?} {:?} {:?} {:?} {}", a, b, c, d, iter::empty::<u8>().count());
    let mut it = counter();
    println!("{:?} {:?}", it.next(), it.collect::<Vec<_>>());
    let mut pulls = 0;
    let e: Vec<i32> = std::iter::from_fn(|| { pulls += 1; Some(pulls) }).take(2).collect();
    let s: i32 = iter::repeat_n(4, 3).sum();
    let w: Vec<String> = iter::once_with(|| "w".to_string()).collect();
    println!("{:?} {pulls} {s} {:?} {}", e, w, iter::once(1).len());
}
main();
"#,
    );
    assert_eq!(
        output,
        "[7, 7, 7] [1, 2] [2, 4, 8, 16] [1, 3, 9, 27, 81, 243] 0\nSome(1) [2, 3]\n\
         [1, 2] 2 12 [\"w\"] 1\n"
    );
    // `repeat` clones its value, which must be `Clone`.
    let report = refused(
        "struct S;\nfn main() {\n    let n = std::iter::repeat(S).take(2).count();\n}\nmain();\n",
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `S: Clone` is not satisfied"),
        "{report}"
    );
}

#[test]
fn a_buffered_reader_and_writer_over_a_file_run_as_stds() {
    // std's `BufWriter` over a `File` writes what it buffers when it is
    // flushed or dropped, and a `BufReader` reads a file's lines, as
    // std's do (#2580). `read_line` is `BufRead`'s, so it needs its `use`.
    let directory = std::env::temp_dir().join(format!("rawr-buffered-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let source = format!(
        r#"
use std::fs::{{self, File}};
use std::io::{{BufRead, BufReader, BufWriter, Cursor, Read, Write}};
use std::path::PathBuf;
fn main() {{
    let dir = PathBuf::from({directory:?});
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join("lines.txt");
    {{
        let mut out = BufWriter::new(File::create(&p).unwrap());
        writeln!(out, "one").unwrap();
        out.write_all(b"two\n").unwrap();
        println!("{{}} {{}}", out.capacity(), fs::read_to_string(&p).unwrap().len());
    }}
    let mut out = BufWriter::with_capacity(4, fs::OpenOptions::new().append(true).open(&p).unwrap());
    out.write_all(b"three\n").unwrap();
    out.flush().unwrap();
    drop(out);
    for line in BufReader::new(File::open(&p).unwrap()).lines() {{
        println!("{{}}", line.unwrap());
    }}
    let mut reader = BufReader::new(File::open(&p).unwrap());
    let mut first = String::new();
    let n = reader.read_line(&mut first).unwrap();
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    println!("{{n}} {{first:?}} {{rest:?}}");
    let words: Vec<String> = BufReader::new(Cursor::new("x y\nz")).lines().map(|l| l.unwrap()).collect();
    println!("{{:?}}", words);
    fs::remove_dir_all(&dir).unwrap();
}}
main();
"#
    );
    let output = printed_with(&source, crate::Access::all());
    assert_eq!(
        output,
        "8192 0\none\ntwo\nthree\n4 \"one\\n\" \"two\\nthree\\n\"\n[\"x y\", \"z\"]\n"
    );
    let report = refused(
        "use std::io::{BufReader, Cursor};\nfn main() {\n    let mut r = \
         BufReader::new(Cursor::new(\"a\"));\n    let mut s = String::new();\n    \
         r.read_line(&mut s).unwrap();\n}\nmain();\n",
    );
    assert!(
        report.starts_with(
            "error[E0599]: no method named `read_line` found for struct `BufReader<R>` in the \
             current scope"
        ),
        "{report}"
    );
    assert!(report.contains("1 + use std::io::BufRead;"), "{report}");
}

#[test]
fn an_operator_on_a_std_type_needs_no_trait_in_scope() {
    // `+`, `+=`, `-`, `==`, `<` and `[]` call their traits' methods with
    // no `use` of `Add`, `AddAssign`, `PartialEq` or `Index`, as in Rust;
    // only method syntax asks for its trait (#1427).
    let output = printed(
        r#"
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
fn main() {
    let mut t: SystemTime = UNIX_EPOCH + Duration::from_secs(90);
    t += Duration::from_secs(10);
    t -= Duration::from_secs(40);
    println!("{:?} {}", t.duration_since(UNIX_EPOCH).unwrap(), t == UNIX_EPOCH + Duration::from_secs(60));
    let now = Instant::now();
    let mut later = now + Duration::from_millis(5);
    later += Duration::from_millis(5);
    println!("{:?} {} {}", later - now, later > now, later == now);
    let mut d = Duration::from_secs(1) * 3;
    d += Duration::from_secs(2);
    d -= Duration::from_millis(500);
    println!("{:?} {} {:?}", d, d == Duration::from_millis(4500), d / 2);
    let mut s = String::from("ab") + "c";
    s += "d";
    println!("{} {} {}", s, s == "abcd", &s[1..3]);
}
main();
"#,
    );
    assert_eq!(
        output,
        "60s true\n10ms true false\n4.5s true 2.25s\nabcd true bc\n"
    );
}

#[test]
fn a_reference_type_written_with_a_named_lifetime_borrows_for_it() {
    // `let x: &'static T` and `let x: &'a T` of the function's `'a` borrow
    // what `x` is given for all of the body (#129).
    let short = "error[E0597]: `n` does not live long enough";
    refused_as(&[
        (
            "fn main() { let n = 5; let x: &'static i32 = &n; println!(\"{}\", x); }",
            short,
        ),
        (
            "fn main() { let x: &'static i32; { let n = 5; x = &n; } println!(\"{}\", x); }",
            short,
        ),
        (
            "fn f<'a>(r: &'a i32) -> &'a i32 { let n = 5; let x: &'a i32 = &n; println!(\"{}\", x); r }
fn main() { let k = 1; println!(\"{}\", f(&k)); }",
            short,
        ),
    ]);
    let output = printed(
        r#"
static A: i32 = 1;
const C: i32 = 3;
struct H<'a>(&'a i32);
impl<'a> H<'a> {
    fn g(&self) -> i32 { let y: &'a i32 = self.0; let z: &'a i32 = &*self.0; *y + *z }
}
fn f<'a>(r: &'a i32) -> i32 { let x: &'a i32 = r; let w: &'a i32 = &*r; *x + *w }
fn main() {
    let s: &'static str = "a";
    let x: &'static i32 = &A;
    let y: &'static i32 = &5;
    let z: &'static i32 = &C;
    let w: &'static [i32] = &[1, 2];
    let t: &'static str;
    t = s;
    let k = 4;
    println!("{} {} {} {} {:?} {} {} {}", s, x, y, z, w, t, H(&k).g(), f(&k));
}
main();
"#,
    );
    assert_eq!(output, "a 1 5 3 [1, 2] a 8 8\n");
}

#[test]
fn a_field_that_leaves_out_its_types_lifetime_is_e0106() {
    // `w: Wrapper` of `struct Wrapper<'a>`, in a field, a payload or a
    // type argument, as rustc refuses it (#160).
    let missing = "error[E0106]: missing lifetime specifier";
    refused_as(&[
        (
            "struct Wrapper<'a> { r: &'a i32 }
struct Outer { w: Wrapper }
fn main() { let a = 1; let o = Outer { w: Wrapper { r: &a } }; println!(\"{}\", o.w.r); }",
            missing,
        ),
        (
            "struct W<'a> { r: &'a i32 }
enum E { A(i32), B(Option<W>) }
fn main() {}",
            missing,
        ),
        (
            "struct W<'a, T> { r: &'a T }
struct O { w: Vec<W<i32>> }
fn main() {}",
            missing,
        ),
        (
            "struct O<'a> { w: &'a W }
struct W<'a> { r: &'a i32 }
fn main() {}",
            missing,
        ),
    ]);
    let output = printed(
        r#"
struct W<'a, T> { r: &'a T }
struct O<'a> { w: Vec<W<'a, i32>>, f: fn(i32) -> i32 }
enum E<'b> { A(W<'b, u8>), B }
fn main() {
    let a = 1;
    let b = 2u8;
    let o = O { w: vec![W { r: &a }], f: |n| n + 1 };
    let e = E::A(W { r: &b });
    if let E::A(w) = e {
        println!("{} {}", (o.f)(*o.w[0].r), w.r);
    }
    let _ = E::B;
}
main();
"#,
    );
    assert_eq!(output, "2 2\n");
}

#[test]
fn a_closure_that_moves_out_a_capture_where_fn_is_needed_is_e0507() {
    // A closure written for an `F: Fn` or `impl FnMut` parameter has that
    // kind, which can't move out a capture (#193).
    refused_as(&[
        (
            "fn call_twice<F: Fn() -> usize>(f: F) -> usize { f() + f() }
fn main() { let s = String::from(\"ab\"); println!(\"{}\", call_twice(move || { let t = s; t.len() })); }",
            "error[E0507]: cannot move out of `s`, a captured variable in an `Fn` closure",
        ),
        (
            "fn mutable(mut f: impl FnMut() -> usize) -> usize { f() }
fn main() { let u = String::from(\"abc\"); println!(\"{}\", mutable(|| { let t = u; t.len() })); }",
            "error[E0507]: cannot move out of `u`, a captured variable in an `FnMut` closure",
        ),
    ]);
    let output = printed(
        r#"
fn once<F: FnOnce() -> usize>(f: F) -> usize { f() }
fn twice<F: Fn() -> usize>(f: F) -> usize { f() + f() }
fn main() {
    let s = String::from("ab");
    let t = String::from("abc");
    println!("{} {}", once(move || { let u = s; u.len() }), twice(|| t.len()));
}
main();
"#,
    );
    assert_eq!(output, "2 6\n");
}

#[test]
fn a_cells_guard_keeps_the_cell_borrowed_until_it_drops() {
    // `c.borrow()` borrows `c` for as long as its `Ref` lives, whose drop
    // at its scope's end uses the borrow (#185).
    let assigned = "error[E0506]: cannot assign to `c` because it is borrowed";
    refused_as(&[
        (
            "use std::cell::RefCell;
fn main() { let mut c = RefCell::new(1); let b = c.borrow(); c = RefCell::new(2); println!(\"{}\", b); }",
            assigned,
        ),
        (
            "use std::cell::RefCell;
fn main() { let mut c = RefCell::new(1); let b = c.borrow_mut(); c = RefCell::new(2); }",
            assigned,
        ),
    ]);
    let output = printed(
        r#"
use std::cell::RefCell;
fn main() {
    let mut c = RefCell::new(1);
    let n = *c.borrow();
    c = RefCell::new(n + 1);
    {
        let b = c.borrow();
        println!("{}", b);
    }
    c = RefCell::new(5);
    let b = c.borrow_mut();
    drop(b);
    c = RefCell::new(6);
    *c.borrow_mut() += 1;
    let r = &c;
    let g = r.borrow();
    println!("{} {}", g, c.borrow());
}
main();
"#,
    );
    assert_eq!(output, "2\n7 7\n");
}

#[test]
fn a_field_read_of_a_drop_temporary_keeps_its_borrows_to_the_statements_end() {
    // `G(&x, 7).1` keeps the temporary `G` to the statement's end, where
    // its `Drop` uses `&x` (#185).
    refused_as(&[(
        "struct G<'a>(&'a i32, i32);
impl Drop for G<'_> { fn drop(&mut self) {} }
fn bump(x: &mut i32) -> i32 { *x += 1; *x }
fn main() { let mut x = 1; let n = G(&x, 7).1 + bump(&mut x); println!(\"{}\", n); }",
        "error[E0502]: cannot borrow `x` as mutable because it is also borrowed as immutable",
    )]);
    let output = printed(
        r#"
struct G<'a>(&'a i32, i32);
impl Drop for G<'_> { fn drop(&mut self) {} }
struct P<'a>(&'a i32, i32);
fn bump(x: &mut i32) -> i32 { *x += 1; *x }
fn main() {
    let mut x = 1;
    let a = G(&x, 7).1;
    let b = bump(&mut x);
    let c = P(&x, 2).1 + bump(&mut x);
    let d = *G(&x, 3).0;
    println!("{} {} {} {}", a, b, c, d);
}
main();
"#,
    );
    assert_eq!(output, "7 2 5 3\n");
}

#[test]
fn a_jump_out_of_a_scope_ends_its_locals_after_their_drops() {
    // `?`, `return`, `break` and `continue` end the locals of each scope
    // they leave, the last declared first, each after its drop, as the
    // scopes' ends do (#185).
    let short = "error[E0597]: `x` does not live long enough";
    let drop = "struct D<'a>(&'a i32);\nimpl Drop for D<'_> { fn drop(&mut self) {} }\n";
    let cases = [
        "fn f(o: Option<i32>) -> Option<i32> { let d; let x = 1; d = D(&x); let n = o?; drop(d); \
         Some(n) }\nfn main() { println!(\"{:?}\", f(Some(1))); }",
        "fn f(o: Result<i32, String>) -> Result<i32, String> { let d; let x = 1; d = D(&x); \
         let n = o?; drop(d); Ok(n) }\nfn main() { println!(\"{:?}\", f(Ok(1))); }",
        "fn f(c: bool) -> i32 { let d; let x = 1; d = D(&x); if c { return 1; } drop(d); 2 }\n\
         fn main() { println!(\"{}\", f(true)); }",
        "fn main() { let mut n = 0; loop { let d; let x = 1; d = D(&x); n += 1; if n > 1 { break; } \
         drop(d); } println!(\"{}\", n); }",
    ]
    .map(|case| format!("{drop}{case}"));
    refused_as(&cases.each_ref().map(|case| (case.as_str(), short)));
    let output = printed(
        r#"
struct D<'a>(&'a i32);
impl Drop for D<'_> { fn drop(&mut self) {} }
fn f(o: Option<i32>) -> Option<i32> { let x = 1; let d = D(&x); let n = o?; drop(d); Some(n) }
fn g(c: bool) -> i32 { let x = 1; let d = D(&x); if c { return 1; } drop(d); 2 }
fn main() {
    let mut n = 0;
    while n < 3 { let x = 1; let d = D(&x); n += 1; if n == 1 { continue; } drop(d); }
    println!("{:?} {:?} {} {} {}", f(Some(1)), f(None), g(true), g(false), n);
}
main();
"#,
    );
    assert_eq!(output, "Some(1) None 1 2 3\n");
}

#[test]
fn a_drop_that_uses_some_of_a_values_borrows_keeps_only_those() {
    // A value with no `Drop` of its own drops its parts: their `Drop`s use
    // the borrows they hold, and a reference beside them uses none (#185).
    let assigned = "error[E0506]: cannot assign to `x` because it is borrowed";
    let drop = "struct D<'a>(&'a i32);\nimpl Drop for D<'_> { fn drop(&mut self) {} }\n\
                struct S<'a, 'b> { d: D<'a>, r: &'b i32 }\nstruct P<T, U> { t: T, u: U }\n\
                enum E<'a, 'b> { A(D<'a>), B(&'b i32) }\n";
    let cases = [
        "let s = S { d: D(&x), r: &y }; println!(\"{}\", s.r);",
        "let s = (D(&x), &y); println!(\"{}\", s.1);",
        "let s = P { t: D(&x), u: &y }; println!(\"{}\", s.u);",
        "let s = (vec![D(&x)], vec![&y]); println!(\"{}\", s.1[0]);",
        "let s = E::A(D(&x)); let _f = E::B(&y); if let E::A(_) = &s {}",
    ]
    .map(|case| {
        format!(
            "{drop}fn main() {{ let mut x = 1; let y = 2; {case} x = 3; println!(\"{{}}\", x); }}"
        )
    });
    refused_as(&cases.each_ref().map(|case| (case.as_str(), assigned)));
    let output = printed(
        r#"
struct D<'a>(&'a i32);
impl Drop for D<'_> { fn drop(&mut self) {} }
struct S<'a, 'b> { d: D<'a>, r: &'b i32 }
struct Out<'a, 'b> { s: S<'a, 'b>, n: i32 }
struct P<T, U> { t: T, u: U }
fn take<T>(_t: T) {}
fn main() {
    let mut x = 1;
    let mut y = 2;
    let z = 5;
    {
        let s = Out { s: S { d: D(&x), r: &y }, n: 0 };
        let t = (D(&x), &y);
        let p = P { t: D(&x), u: &y };
        let v = (vec![D(&x)], Some(&y));
        println!("{} {} {} {} {:?}", s.s.r, s.n, t.1, p.u, v.1);
        y = 3;
    }
    let mut w = (D(&x), &y);
    w = (D(&z), &y);
    take((D(&x), &y));
    x = 4;
    println!("{} {} {}", x, y, w.1);
}
main();
"#,
    );
    assert_eq!(output, "2 0 2 2 Some(2)\n4 3 3\n");
}

#[test]
fn a_trait_method_and_a_mut_parameter_keep_what_their_signatures_say() {
    // A call through a generic bound or a trait object keeps what the
    // trait's signature says, and a `&mut` parameter takes another
    // argument's borrow its lifetimes tie to it (#132).
    let assigned =
        |name: &str| format!("error[E0506]: cannot assign to `{name}` because it is borrowed");
    let (t, b, a) = (assigned("t"), assigned("b"), assigned("a"));
    refused_as(&[
        (
            "trait Get { fn get(&self) -> &i32; }
struct S(i32);
impl Get for S { fn get(&self) -> &i32 { &self.0 } }
fn f<T: Get>(mut t: T, u: T) { let r = t.get(); t = u; println!(\"{}\", r); }
fn main() { f(S(1), S(2)); }",
            &t,
        ),
        (
            "trait Get { fn get(&self) -> &i32; }
struct S(i32);
impl Get for S { fn get(&self) -> &i32 { &self.0 } }
fn main() { let mut b: Box<dyn Get> = Box::new(S(1)); let r = b.get(); b = Box::new(S(2)); \
             println!(\"{} {}\", r, b.get()); }",
            &b,
        ),
        (
            "fn store<'a>(dst: &mut Vec<&'a i32>, x: &'a i32) { dst.push(x); }
fn main() { let mut a = 1; let mut v = Vec::new(); store(&mut v, &a); a = 2; println!(\"{:?} {}\", v, a); }",
            &a,
        ),
        (
            "struct Holder<'a> { r: &'a i32 }
fn put<'a>(h: &mut Holder<'a>, x: &'a i32) { h.r = x; }
fn main() { let z = 0; let mut a = 1; let mut h = Holder { r: &z }; put(&mut h, &a); a = 2; \
             println!(\"{} {}\", h.r, a); }",
            &a,
        ),
    ]);
    let output = printed(
        r#"
struct Holder<'a> { r: &'a i32 }
fn put<'a>(h: &mut Holder<'a>, x: &'a i32) { h.r = x; }
fn count(v: &mut Vec<i32>, x: &i32) { v.push(*x); }
fn store<'a>(dst: &mut Vec<&'a i32>, x: &'a i32) { dst.push(x); }
trait Get { fn get(&self) -> &i32; fn twice(&self) -> i32 { *self.get() * 2 } }
struct S(i32);
impl Get for S { fn get(&self) -> &i32 { &self.0 } }
fn show<T: Get>(t: &T) -> i32 { let r = t.get(); *r + t.twice() }
fn main() {
    let z = 0;
    let mut a = 1;
    let mut v = Vec::new();
    count(&mut v, &a);
    a = 2;
    let mut w = Vec::new();
    store(&mut w, &z);
    let mut h = Holder { r: &z };
    put(&mut h, &z);
    let b: Box<dyn Get> = Box::new(S(4));
    let r = b.get();
    println!("{:?} {} {:?} {} {} {}", v, a, w, h.r, r, show(&S(3)));
}
main();
"#,
    );
    assert_eq!(output, "[1] 2 [0] 0 4 9\n");
}

#[test]
fn a_closure_storing_a_borrow_in_a_capture_stores_it_in_the_captured_local() {
    // `|| w.push(&x)` stores the borrow of `x` in `w` itself, which a
    // later use of `w` keeps. rustc names the use of `x` in the body, as
    // it does for any capture that does not live long enough (#148).
    refused_as(&[
        (
            "fn main() { let mut w: Vec<&i32> = Vec::new(); { let x = 1; let mut f = || w.push(&x); f(); } \
             println!(\"{:?}\", w); }",
            "error[E0597]: `x` does not live long enough\n --> <script>:1:84",
        ),
        (
            "fn main() { let f; { let x = 1; f = || x + 1; } println!(\"{}\", f()); }",
            "error[E0597]: `x` does not live long enough\n --> <script>:1:40",
        ),
        (
            "fn main() { let mut x = 1; let mut w: Vec<&i32> = Vec::new(); let mut f = || w.push(&x); f(); \
             x = 2; println!(\"{:?} {}\", w, x); }",
            "error[E0506]: cannot assign to `x` because it is borrowed",
        ),
    ]);
    let output = printed(
        r#"
fn main() {
    let x = 1;
    let y = 2;
    let mut w: Vec<&i32> = Vec::new();
    {
        let mut f = || w.push(&x);
        f();
        f();
    }
    let mut g = |r: &i32| { let _ = r; w.push(&y) };
    g(&3);
    println!("{:?}", w);
}
main();
"#,
    );
    assert_eq!(output, "[1, 1, 2]\n");
}

#[test]
fn a_closure_storing_its_parameter_in_a_capture_stores_what_each_call_gives() {
    // `|r| w.push(r)`, whose parameter's type the body infers, has `r`'s
    // region be `w`'s: each call's argument is stored in `w` (#148).
    refused_as(&[
        (
            "fn main() { let mut w: Vec<&i32> = Vec::new(); { let x = 1; let mut g = |r| w.push(r); \
             g(&x); } println!(\"{:?}\", w); }",
            "error[E0597]: `x` does not live long enough\n --> <script>:1:90",
        ),
        (
            "fn main() { let mut x = 1; let mut w: Vec<&i32> = Vec::new(); let mut g = |r| w.push(r); \
             g(&x); x = 2; println!(\"{:?} {}\", w, x); }",
            "error[E0506]: cannot assign to `x` because it is borrowed",
        ),
    ]);
    let output = printed(
        r#"
fn main() {
    let mut x = 1;
    let y = 2;
    let mut w: Vec<&i32> = Vec::new();
    {
        let mut g = |r| w.push(r);
        g(&y);
        g(&y);
    }
    let mut n = 0;
    let mut h = |r: &i32| n += *r;
    h(&x);
    x = 3;
    h(&x);
    println!("{:?} {} {}", w, n, x);
}
main();
"#,
    );
    assert_eq!(output, "[2, 2] 4 3\n");
}

#[test]
fn std_from_runs_char_byte_text_and_slice_conversions() {
    // std's `From<char> for u32`, `From<u8> for char`, `From<&str> for
    // Vec<u8>`, `From<&[T]> for Vec<T>` and `From<Vec<T>> for Box<[T]>`,
    // by path, by `into` and through a generic bound (#763).
    let output = printed(
        r#"
fn wide<T: From<char>>(c: char) -> T { T::from(c) }
fn code<T: Into<u32>>(x: T) -> u32 { x.into() }
fn bytes<T: Into<Vec<u8>>>(x: T) -> Vec<u8> { x.into() }
fn main() {
    let v: Vec<u8> = "hi".into();
    let s: &[i32] = &[1, 2];
    let b: Box<[i32]> = vec![3, 4].into();
    println!("{} {} {:?} {:?} {:?}", u32::from('a'), char::from(66u8), v, Vec::from("ab"), Vec::from(s));
    let c: char = 97u8.into();
    let w: u64 = wide('b');
    println!("{c} {w} {} {:?} {:?} {:?}", code('c'), bytes("de"), Vec::<u8>::from(String::from("f")), b);
}
main();
"#,
    );
    assert_eq!(
        output,
        "97 B [104, 105] [97, 98] [1, 2]\na 98 99 [100, 101] [102] [3, 4]\n"
    );
    // std has no `From<char> for i32`.
    let report = refused("fn main() { let x: i32 = 'a'.into(); }\nmain();\n");
    assert!(
        report.starts_with("error[E0277]: the trait bound `i32: From<char>` is not satisfied"),
        "{report}"
    );
}

#[test]
fn std_trait_functions_without_self_run_by_path() {
    // `TryFrom::try_from`, `FromIterator::from_iter`, `Sum::sum` and
    // `Product::product` take `Self` from the expected type, as the std
    // methods whose bodies call them do; `Vec::from_iter` and
    // `String::from_iter` name it; an operator trait's function is its
    // operator (#759).
    let output = printed(
        r#"
use std::convert::TryFrom;
use std::iter::FromIterator;
use std::ops::{Add, Mul, Neg, Not};
#[derive(Debug, Clone, Copy)]
struct P(i32);
impl Add for P {
    type Output = P;
    fn add(self, o: P) -> P { P(self.0 + o.0) }
}
fn main() {
    let small: Result<u8, _> = TryFrom::try_from(300i32);
    let fits: Result<i8, std::num::TryFromIntError> = std::convert::TryFrom::try_from(5u64);
    let v: Vec<i32> = FromIterator::from_iter(1..4);
    let w = Vec::from_iter(v.iter().map(|x| x * 2));
    let s = String::from_iter(['a', 'b']);
    let total: i32 = std::iter::Sum::sum(v.iter());
    let product: i64 = std::iter::Product::product([2i64, 3].into_iter());
    println!("{small:?} {fits:?} {v:?} {w:?} {s} {total} {product}");
    println!("{} {} {} {} {:?}", Add::add(1, 2) * 10, Mul::mul(2.5, 2.0), Neg::neg(3), Not::not(true), Add::add(P(1), P(2)));
}
main();
"#,
    );
    assert_eq!(
        output,
        "Err(TryFromIntError(PosOverflow)) Ok(5) [1, 2, 3] [2, 4, 6] ab 6 6\n30 5 -3 false P(3)\n"
    );
}

#[test]
fn a_closure_passed_to_a_cell_method_takes_std_parameter_types() {
    // `Cell::update`'s `impl FnOnce(T) -> T` gives `x` the cell's type,
    // and `RefCell::replace_with`'s `F: FnOnce(&mut T) -> T` gives `v`
    // its: the closure reads before the method's own error (#803).
    let output = printed(
        r#"
use std::cell::Cell;
fn main() {
    let c = Cell::new(2u8);
    c.update(|x| x.pow(2) + 1);
    let held = &c;
    held.update(|x| x * 2);
    println!("{}", c.get());
}
main();
"#,
    );
    assert_eq!(output, "10\n");
    let report = refused(
        r#"
fn main() {
    let r = std::cell::RefCell::new(vec![1]);
    r.replace_with(|v| { let mut n = v.clone(); n.push(2); n });
}
main();
"#,
    );
    assert!(
        report.starts_with("error: rawr doesn't run `RefCell::replace_with` yet"),
        "{report}"
    );
}

#[test]
fn a_closure_passed_to_a_std_method_returns_its_fn_bounds_output() {
    // `map_or`'s `F: FnOnce(T) -> U` takes `U` from the default, `&str`,
    // so the closure's `&String` coerces to it, as rustc's expected
    // closure signature makes it (#804).
    let output = printed(
        r#"
fn main() {
    let name = String::from("n");
    let a: &str = Some(&name).map_or("", |s| s);
    let held = vec![1, 2];
    let b = Some(&held).map_or(&[0][..], |v| v);
    let words = vec![String::from("w")];
    let c: Vec<usize> = words.iter().map(|w| w.len()).collect();
    println!("{a} {b:?} {c:?} {}", None.map_or("none", |s: &String| s));
}
main();
"#,
    );
    assert_eq!(output, "n [1, 2] [1] none\n");
}

#[test]
fn an_iterator_has_peekable_and_double_ended_methods_only_where_std_gives_them() {
    // `peek` is `Peekable`'s own, and `next_back` and `rev` need a
    // `DoubleEndedIterator`: `v.iter()` has no `peek`, a `take_while` no
    // `next_back`, and `0..` no `rev` (#792).
    let output = printed(
        r#"
fn main() {
    let v = vec![1, 2, 3];
    let mut p = v.iter().peekable();
    println!("{:?}", p.peek());
    println!("{:?}", p.next());
    let mut q = v.iter().map(|x| x * 2).peekable();
    println!("{:?} {:?}", q.next_if(|x| *x == 2), q.next_if_eq(&5));
    let back: Vec<i32> = v.iter().map(|x| x + 1).rev().collect();
    let ends: Vec<&i32> = v.iter().peekable().rev().collect();
    println!("{back:?} {ends:?} {:?}", (0..5).rev().next());
    let mut r = (1..4).peekable();
    let first = r.peek().copied();
    println!("{first:?} {:?}", r.collect::<Vec<_>>());
}
main();
"#,
    );
    assert_eq!(
        output,
        "Some(1)\nSome(1)\nSome(2) None\n[4, 3, 2] [3, 2, 1] Some(4)\nSome(1) [1, 2, 3]\n"
    );
    // Each refusal names the std struct of the chain's outermost adapter,
    // with the struct's declared parameters, as rustc's E0599 does (#2617).
    let refusal = |body: &str| refused(&format!("fn main() {{\n{body}\n}}\nmain();\n"));
    for (body, header) in [
        (
            "let v = vec![1, 2, 3];\nlet mut it = v.iter();\nit.peek();",
            "no method named `peek` found for struct `std::slice::Iter<'a, T>`",
        ),
        (
            "let v = vec![1, 2, 3];\nlet mut it = v.iter().peekable().map(|x| x * 2);\nit.peek();",
            "no method named `peek` found for struct `Map<I, F>`",
        ),
        (
            "let v = vec![1, 2, 3];\nlet mut b = v.iter().take_while(|x| **x < 3);\nb.next_back();",
            "no method named `next_back` found for struct `TakeWhile<I, P>`",
        ),
    ] {
        let report = refusal(body);
        assert!(
            report.starts_with(&format!("error[E0599]: {header} in the current scope")),
            "{report}"
        );
    }
    let report = refusal("let c: Vec<i32> = (0..).rev().take(2).collect();");
    assert!(
        report.starts_with(
            "error[E0277]: the trait bound `std::ops::RangeFrom<{integer}>: DoubleEndedIterator` \
             is not satisfied"
        ),
        "{report}"
    );
}

#[test]
fn a_method_an_iterator_lacks_names_its_std_struct() {
    // rustc names the struct of the source or of the outermost adapter,
    // by its trimmed path and declared parameters (#2617).
    for (chain, owner) in [
        ("\"ab\".chars()", "Chars<'a>"),
        ("\"a b\".split_whitespace()", "SplitWhitespace<'a>"),
        ("\"ab\".bytes()", "std::str::Bytes<'a>"),
        ("v.clone().into_iter()", "std::vec::IntoIter<T, A>"),
        ("v.iter_mut()", "std::slice::IterMut<'a, T>"),
        ("v.iter().map(|x| x + 1)", "Map<I, F>"),
        ("v.iter().filter(|x| **x > 1)", "Filter<I, P>"),
        ("v.iter().zip(v.iter())", "Zip<A, B>"),
        ("v.iter().chain(v.iter())", "std::iter::Chain<A, B>"),
        ("v.iter().rev()", "Rev<T>"),
        ("v.iter().enumerate().skip(1)", "Skip<I>"),
        ("v.iter().peekable()", "Peekable<I>"),
        ("v.iter().cloned()", "Cloned<I>"),
        ("v.iter().flat_map(|x| vec![*x])", "FlatMap<I, U, F>"),
        ("v.iter().step_by(2).take(1)", "std::iter::Take<I>"),
    ] {
        let report = refused(&format!(
            "fn main() {{\n    let mut v = vec![1, 2, 3];\n    {chain}.nope();\n}}\nmain();\n"
        ));
        assert!(
            report.starts_with(&format!(
                "error[E0599]: no method named `nope` found for struct `{owner}` in the current \
                 scope"
            )),
            "{chain}: {report}"
        );
    }
}

#[test]
fn std_iterator_structs_are_types() {
    // `vec::IntoIter<T>` as an `IntoIterator` impl's `IntoIter` and a
    // local's type, `Peekable<I>` as a field's, which has `peek`, and
    // `slice::Iter<'a, T>`: rawr's iterator of their items (#760).
    let output = printed(
        r#"
use std::iter::Peekable;
struct Bag(Vec<i32>);
impl IntoIterator for Bag {
    type Item = i32;
    type IntoIter = std::vec::IntoIter<i32>;
    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}
struct Lex<I: Iterator<Item = char>> {
    chars: Peekable<I>,
}
impl<I: Iterator<Item = char>> Lex<I> {
    fn digits(&mut self) -> String {
        let mut out = String::new();
        while let Some(c) = self.chars.peek() {
            if !c.is_ascii_digit() { break; }
            out.push(*c);
            self.chars.next();
        }
        out
    }
}
struct Walk<'a> { rest: std::slice::Iter<'a, u8> }
fn main() {
    let total: i32 = Bag(vec![1, 2, 3]).into_iter().sum();
    let mut lex = Lex { chars: "42x".chars().peekable() };
    println!("{total} {} {:?}", lex.digits(), lex.chars.next());
    let bytes = [7u8, 8];
    let mut w = Walk { rest: bytes.iter() };
    println!("{:?} {:?}", w.rest.next(), w.rest.next());
    let mut kept: std::vec::IntoIter<u32> = vec![5u32].into_iter();
    println!("{:?} {:?}", kept.next(), kept.next());
}
main();
"#,
    );
    assert_eq!(output, "6 42 Some('x')\nSome(7) Some(8)\nSome(5) None\n");
}

#[test]
fn sum_partition_unzip_and_collect_values_keep_the_items_borrows() {
    // std's `Extend`, `Sum` and `FromIterator` impls say what the value
    // keeps: `Vec<&T>` the items' borrows, a sum and a `Vec` of copies
    // none (#793).
    let output = printed(
        r#"
fn main() {
    let mut v = vec![1, 2, 3, 4];
    let total: i32 = v.iter().sum();
    let (small, big): (Vec<i32>, Vec<i32>) = v.iter().map(|x| *x).partition(|x| *x < 3);
    let (left, right): (Vec<&i32>, Vec<char>) = v.iter().zip("ab".chars()).unzip();
    println!("{left:?} {right:?}");
    v.push(5);
    let all = v.iter().collect::<Vec<_>>();
    println!("{total} {small:?} {big:?} {all:?}");
}
main();
"#,
    );
    assert_eq!(
        output,
        "[1, 2] ['a', 'b']\n10 [1, 2] [3, 4] [1, 2, 3, 4, 5]\n"
    );
    for kept in [
        "let (small, big): (Vec<&i32>, Vec<&i32>) = v.iter().partition(|x| **x < 3);\nv.push(5);\nprintln!(\"{small:?} {big:?}\");",
        "let (left, right): (Vec<&i32>, Vec<char>) = v.iter().zip(\"ab\".chars()).unzip();\nv.push(5);\nprintln!(\"{left:?} {right:?}\");",
        "let all = v.iter().collect::<Vec<_>>();\nv.push(5);\nprintln!(\"{all:?}\");",
    ] {
        let report = refused(&format!(
            "fn main() {{\nlet mut v = vec![1, 2, 3, 4];\n{kept}\n}}\nmain();\n"
        ));
        assert!(
            report.starts_with(
                "error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable"
            ),
            "{report}"
        );
    }
}

/// The files of a Cargo package of edition `edition` whose `src/main.rs`
/// is `main`.
fn edition_package(edition: &str, main: &str) -> [(&'static str, String); 2] {
    [
        (
            "Cargo.toml",
            format!(
                "[package]\nname = \"tails\"\nversion = \"0.1.0\"\nedition = \"{edition}\"\n\n\
                 [workspace]\n"
            ),
        ),
        ("src/main.rs", format!("// rawr\n{main}")),
    ]
}

/// The package of edition `edition` whose `src/main.rs` is `main`, loaded
/// under the build directory as `name`, with its directory.
fn edition_loaded(
    name: &str,
    edition: &str,
    main: &str,
) -> (std::path::PathBuf, crate::package::Graph) {
    let files = edition_package(edition, main);
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect();
    loaded_package(&format!("tail-{name}-{edition}"), &files)
}

/// What the package of edition `edition` whose `src/main.rs` is `main`
/// prints.
fn edition_printed(name: &str, edition: &str, main: &str) -> String {
    let files = edition_package(edition, main);
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect();
    printed_package(&format!("tail-{name}-{edition}"), &files)
}

/// rustc's report of the package of edition `edition` whose `src/main.rs`
/// is `main`, which rawr refuses, but for its `-->` line, which names where
/// the build directory is.
fn edition_rendered(name: &str, edition: &str, main: &str) -> String {
    let (root, graph) = edition_loaded(name, edition, main);
    let error = Engine::new()
        .compile_package(graph)
        .map(|_| ())
        .expect_err("rawr refuses this package");
    let _ = std::fs::remove_dir_all(&root);
    let report = crate::diagnostic::render(&error.report().expect("the error is in the package"));
    let lines: Vec<&str> = report
        .lines()
        .filter(|line| !line.trim_start().starts_with("-->"))
        .collect();
    format!("{}\n", lines.join("\n"))
}

/// A type whose `Drop` uses the borrow it holds.
const GUARD: &str = "struct Guard<'a>(&'a i32);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!(\"drop {}\", self.0);
    }
}

impl Guard<'_> {
    fn get(&self) -> i32 {
        *self.0
    }
}
";

/// The help rustc gives for a tail temporary that outlives a local.
const TAIL_HELP: &str = "help: for example, you could save the expression's value in a new \
                         local variable `x` and then make `x` be the expression at the end of \
                         the block";

/// The note rustc gives for a tail temporary that outlives a local.
const TAIL_NOTE: &str = "= note: the temporary is part of an expression at the end of a block;";

#[test]
fn a_tail_temporary_that_borrows_a_local_outlives_it_before_edition_2024() {
    // Before edition 2024 a block's tail temporaries drop at the end of the
    // statement around the block, after its locals: a `Drop` temporary that
    // borrows one is rustc's E0597, with its labels, note and help. Edition
    // 2024 drops them first (#2517). Each line is one past rustc's, for
    // `// rawr`.
    let block = format!(
        "{GUARD}
fn main() {{
    let n = {{
        let x = 5;
        Guard(&x).get()
    }};
    println!(\"{{n}}\");
}}
"
    );
    assert_eq!(
        edition_rendered("block", "2021", &block),
        format!(
            "error[E0597]: `x` does not live long enough
   |
18 |         let x = 5;
   |             - binding `x` declared here
19 |         Guard(&x).get()
   |         ------^^-
   |         |     |
   |         |     borrowed value does not live long enough
   |         a temporary with access to the borrow is created here ...
20 |     }};
   |     -- ... and the borrow might be used here, when that temporary is dropped and runs the `Drop` code for type `Guard`
   |     |
   |     `x` dropped here while still borrowed
   |
   {TAIL_NOTE}
           consider forcing this temporary to be dropped sooner, before the block's local variables are dropped
{TAIL_HELP}
   |
19 |         let x = Guard(&x).get(); x
   |         +++++++                +++
"
        )
    );
    assert_eq!(edition_printed("block", "2024", &block), "drop 5\n5\n");
    let body = format!(
        "{GUARD}
fn read() -> i32 {{
    let x = 1;
    Guard(&x).get()
}}

fn main() {{
    println!(\"{{}}\", read());
}}
"
    );
    assert_eq!(
        edition_rendered("body", "2021", &body),
        format!(
            "error[E0597]: `x` does not live long enough
   |
17 |     let x = 1;
   |         - binding `x` declared here
18 |     Guard(&x).get()
   |     ------^^-
   |     |     |
   |     |     borrowed value does not live long enough
   |     a temporary with access to the borrow is created here ...
19 | }}
   | -
   | |
   | `x` dropped here while still borrowed
   | ... and the borrow might be used here, when that temporary is dropped and runs the `Drop` code for type `Guard`
   |
   {TAIL_NOTE}
           consider forcing this temporary to be dropped sooner, before the block's local variables are dropped
{TAIL_HELP}
   |
18 |     let x = Guard(&x).get(); x
   |     +++++++                +++
"
        )
    );
    assert_eq!(edition_printed("body", "2024", &body), "drop 1\n1\n");
    // A closure's body ends its own tail's temporaries after its locals.
    let closure = format!(
        "{GUARD}
fn main() {{
    let g = || {{
        let y = 2;
        Guard(&y).get()
    }};
    println!(\"{{}}\", g());
}}
"
    );
    let (root, graph) = edition_loaded("closure", "2021", &closure);
    let refused = Engine::new()
        .compile_package(graph)
        .map(|_| ())
        .expect_err("rawr refuses this package")
        .to_string();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        refused.starts_with("`y` does not live long enough (E0597)"),
        "{refused}"
    );
    // A parameter outlives its body's tail temporaries in every edition.
    let parameter = format!(
        "{GUARD}
fn read(x: i32) -> i32 {{
    Guard(&x).get()
}}

fn main() {{
    println!(\"{{}}\", read(3));
}}
"
    );
    assert_eq!(
        edition_printed("parameter", "2021", &parameter),
        "drop 3\n3\n"
    );
}

#[test]
fn a_mut_binding_of_a_slice_patterns_rest_is_a_view_of_its_elements() {
    // `[first, rest @ ..] = &mut v[..]`: `rest` is the `&mut [T]` of the
    // elements between the pattern's ends, and a write through it, its
    // methods and a call given it reach `v` itself, as rustc's build does
    // (#163). So does a catch-all binding of a whole range of elements.
    let output = printed(&format!(
        "{}{}{}fn main() {{ first(); second(); third(); }}\nmain();\n",
        r#"
fn first() {
    let mut v = vec![1, 2, 3];
    if let [first, rest @ ..] = &mut v[..] {
        *first += 10;
        rest[0] += 20;
        rest[1] = 3;
    }
    println!("{:?}", v);
    let mut a = [1, 2, 3, 4];
    match &mut a[..] {
        [x, mid @ .., y] => {
            *x = 0;
            *y = 9;
            for m in mid.iter_mut() {
                *m *= 5;
            }
            println!("{}", mid.len());
        }
        _ => {}
    }
    println!("{:?}", a);
    let s = &mut v[..];
    if let [rest @ .., last] = s {
        *last = 7;
        rest[0] = 1;
    }
    println!("{:?}", v);
}
"#,
        r#"
fn bump(rest: &mut [String]) {
    for s in rest.iter_mut() {
        s.push('!');
    }
}

fn second() {
    let mut a = [1, 2, 3, 4];
    match &mut a {
        [x, rest @ ..] => {
            *x = 7;
            rest[2] = 8;
            println!("{:?}", rest);
        }
    }
    println!("{:?}", a);
    let mut v = vec![String::from("a"), String::from("b"), String::from("c")];
    if let [head, tail @ ..] = v.as_mut_slice() {
        head.push('?');
        bump(tail);
        tail.swap(0, 1);
    }
    println!("{:?}", v);
    let mut w = vec![1, 2, 3, 4, 5];
    if let [_, middle @ .., _] = &mut w[1..] {
        middle.reverse();
        println!("{}", middle.len());
    }
    println!("{:?}", w);
}
"#,
        r#"
fn third() {
    let mut v = vec![1, 2, 3, 4];
    match &mut v[1..] {
        [] => println!("empty"),
        [x] => *x = 0,
        whole => {
            whole[0] = 20;
            whole.reverse();
            println!("{}", whole.len());
        }
    }
    println!("{:?}", v);
    match &mut v[2..] {
        [a, rest @ ..] => {
            *a += 1;
            rest[0] += 100;
        }
        all => all[0] = 9,
    }
    println!("{:?}", v);
}
"#,
    ));
    assert_eq!(
        output,
        concat!(
            "[11, 22, 3]\n",
            "2\n",
            "[0, 10, 15, 9]\n",
            "[1, 22, 7]\n",
            "[2, 3, 8]\n",
            "[7, 2, 3, 8]\n",
            "[\"a?\", \"c!\", \"b!\"]\n",
            "2\n",
            "[1, 2, 4, 3, 5]\n",
            "3\n",
            "[1, 4, 3, 20]\n",
            "[1, 4, 4, 120]\n",
        )
    );
    // The rest's borrow of `v` holds while the binding is used.
    let report = refused(
        r#"
fn main() {
    let mut v = vec![1, 2, 3];
    if let [_, rest @ ..] = &mut v[..] {
        v.push(4);
        rest[0] = 1;
    }
    println!("{:?}", v);
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0499]: cannot borrow `v` as mutable more than once at a time"),
        "{report}"
    );
}

#[test]
fn a_cell_drops_its_value_with_it_and_when_its_value_is_replaced() {
    // A `RefCell`, `Cell` or `Mutex` of a value with drop glue drops the
    // value when the cell drops, and the value `set`, an assignment
    // through a borrow or a lock, or `LocalKey::set` replaces, as rustc's
    // build does (#2643). A `thread_local!` key's cell drops as its thread
    // ends.
    let output = printed(
        r#"
use std::cell::{Cell, RefCell};
use std::sync::Mutex;

struct D(&'static str);

impl Drop for D {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

struct Holder {
    cell: RefCell<Vec<D>>,
    last: D,
}

thread_local! {
    static KEPT: RefCell<Vec<D>> = RefCell::new(vec![D("tls")]);
}

fn main() {
    {
        let c = RefCell::new(D("refcell"));
        println!("made");
        let old = c.replace(D("refcell2"));
        println!("replaced");
        drop(old);
        *c.borrow_mut() = D("refcell3");
        println!("assigned");
        c.replace(D("refcell4"));
        println!("statement");
        let e = D("refcell5");
        let back = c.replace(e);
        println!("local");
        drop(back);
    }
    println!("--");
    {
        let c = Cell::new(Some(D("cell")));
        let d = Some(D("cell2"));
        c.set(d);
        println!("set");
        let f = Some(D("cell3"));
        let back = c.replace(f);
        drop(back);
        let t = c.take();
        println!("took");
        drop(t);
    }
    println!("--");
    {
        let m = Mutex::new(D("mutex"));
        *m.lock().unwrap() = D("mutex2");
        println!("locked");
    }
    println!("--");
    {
        let h = Holder {
            cell: RefCell::new(vec![D("v1"), D("v2")]),
            last: D("last"),
        };
        h.cell.borrow_mut().push(D("v3"));
        let moved = h.cell;
        println!("moved");
        drop(moved);
        println!("dropped");
    }
    println!("--");
    KEPT.with_borrow_mut(|v| v.push(D("tls2")));
    KEPT.set(vec![D("tls3")]);
    println!("done");
}
main();
"#,
    );
    assert_eq!(
        output,
        concat!(
            "made\n",
            "replaced\n",
            "drop refcell\n",
            "drop refcell2\n",
            "assigned\n",
            "drop refcell3\n",
            "statement\n",
            "local\n",
            "drop refcell4\n",
            "drop refcell5\n",
            "--\n",
            "drop cell\n",
            "set\n",
            "drop cell2\n",
            "took\n",
            "drop cell3\n",
            "--\n",
            "drop mutex\n",
            "locked\n",
            "drop mutex2\n",
            "--\n",
            "moved\n",
            "drop v1\n",
            "drop v2\n",
            "drop v3\n",
            "dropped\n",
            "drop last\n",
            "--\n",
            "drop tls\n",
            "drop tls2\n",
            "done\n",
            "drop tls3\n",
        )
    );
}

#[test]
fn an_early_exit_drops_the_temporaries_of_each_statement_it_leaves() {
    // A `?`, `continue`, `break` or `return` inside a block inside a
    // statement drops the temporaries made so far in each statement it
    // leaves, each before the locals of the scope the statement is in,
    // as rustc's build does (#2625).
    let main = "struct D(&'static str);

impl Drop for D {
    fn drop(&mut self) {
        println!(\"drop {}\", self.0);
    }
}

impl D {
    fn get(&self) -> i32 {
        1
    }
}

fn early(fail: bool) -> Option<i32> {
    let _a = D(\"a\");
    let n = D(\"held\").get()
        + if fail {
            let _b = D(\"b\");
            D(\"inner\").get() + None::<i32>?
        } else {
            1
        };
    Some(n)
}

fn looped() -> i32 {
    let mut i = 0;
    loop {
        let _c = D(\"c\");
        let k = D(\"loop\").get() + {
            let _e = D(\"e\");
            if i == 0 {
                i += 1;
                continue;
            }
            if i == 1 {
                break D(\"break\").get() * 7;
            }
            0
        };
        println!(\"{k}\");
    }
}

fn returned() -> i32 {
    let v = D(\"return\").get() + {
        if true {
            return 5;
        }
        0
    };
    v
}

fn tail(_p: D) -> Option<i32> {
    let _l = D(\"l\");
    Some(D(\"t\").get() + None::<i32>?)
}

fn main() {
    println!(\"{:?}\", early(true));
    println!(\"{}\", looped());
    println!(\"{}\", returned());
    println!(\"{:?}\", tail(D(\"p\")));
}
";
    let rest = "drop held\ndrop a\nNone\ndrop e\ndrop loop\ndrop c\ndrop break\ndrop e\n\
                drop loop\ndrop c\n7\ndrop return\n5\n";
    // Before edition 2024 a block's tail temporaries outlive its locals,
    // and a body's drop between its locals and its parameters.
    assert_eq!(
        edition_printed("exit", "2021", main),
        format!("drop b\ndrop inner\n{rest}drop l\ndrop t\ndrop p\nNone\n")
    );
    assert_eq!(
        edition_printed("exit", "2024", main),
        format!("drop inner\ndrop b\n{rest}drop t\ndrop l\ndrop p\nNone\n")
    );
}

#[test]
fn impls_for_two_instances_reached_by_a_trait_object_run_their_own() {
    // A trait object's value keeps its concrete type's key, which tells
    // `Foo<i32>`'s impl from `Foo<i64>`'s where the runtime type can't
    // (D1115.3, #1178, #2212).
    let output = printed(
        r#"
trait Describe { fn describe(&self) -> String; }
struct Foo<T> { value: T }
impl Describe for Foo<i32> { fn describe(&self) -> String { format!("i32 {}", self.value) } }
impl Describe for Foo<i64> { fn describe(&self) -> String { format!("i64 {}", self.value) } }
fn main() {
    let a = Foo { value: 1i32 };
    let b = Foo { value: 2i64 };
    let d: Vec<&dyn Describe> = vec![&a, &b];
    let boxed: Box<dyn Describe> = Box::new(Foo { value: 3i64 });
    println!("{} {} {}", d[0].describe(), d[1].describe(), boxed.describe());
    println!("{}", Describe::describe(&b));
}
main();
"#,
    );
    assert_eq!(output, "i32 1 i64 2 i64 3\ni64 2\n");
}

#[test]
fn a_generic_impls_trait_method_runs_its_instance_through_a_trait_object() {
    // A generic impl's trait method is lowered per instance, and a trait
    // object runs the instance its key selects: through `&dyn`, `&mut dyn`,
    // a supertrait's object, a box made in a generic body and a closure,
    // as rustc's vtable does. A static call, a path call, a generic call
    // and `{}` run the instance at their types (D1115, #2622).
    let output = printed(
        r#"
use std::fmt;
trait Half { fn half(&self) -> String; }
impl Half for f32 { fn half(&self) -> String { format!("f32 {}", self / 2.0) } }
impl Half for f64 { fn half(&self) -> String { format!("f64 {}", self / 2.0) } }
trait Show { fn show(&self) -> String; fn bump(&mut self); }
trait Named: Show { fn name(&self) -> String; }
struct Wr<T>(T, u32);
impl<T: Half> Show for Wr<T> {
    fn show(&self) -> String { format!("{} #{}", self.0.half(), self.1) }
    fn bump(&mut self) { self.1 += 1; }
}
impl<T: Half> Named for Wr<T> { fn name(&self) -> String { format!("wr {}", self.0.half()) } }
impl<T: Half> fmt::Display for Wr<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "<{}>", self.0.half()) }
}
fn boxed<T: Half + 'static>(x: T) -> Box<dyn Show> { Box::new(Wr(x, 0)) }
fn via<S: Show>(s: &S) -> String { s.show() }
fn main() {
    let mut a = Wr(1.0f32, 0);
    let b = Wr(1.0f64, 0);
    let shown: Vec<&dyn Show> = vec![&a, &b];
    println!("{} | {}", shown[0].show(), shown[1].show());
    {
        let m: &mut dyn Show = &mut a;
        m.bump();
        println!("{}", m.show());
    }
    let n: &dyn Named = &a;
    println!("{} / {}", n.name(), n.show());
    let v: Vec<Box<dyn Show>> = vec![boxed(3.0f32), boxed(3.0f64)];
    let all: Vec<String> = v.iter().map(|s| s.show()).collect();
    println!("{:?}", all);
    println!("{} | {} | {}", a.show(), Show::show(&b), via(&a));
    let d: &dyn fmt::Display = &a;
    println!("{} {} {}", a, b, d);
}
main();
"#,
    );
    assert_eq!(
        output,
        "f32 0.5 #0 | f64 0.5 #0\nf32 0.5 #1\nwr f32 0.5 / f32 0.5 #1\n\
         [\"f32 1.5 #0\", \"f64 1.5 #0\"]\nf32 0.5 #1 | f64 0.5 #0 | f32 0.5 #1\n\
         <f32 0.5> <f64 0.5> <f32 0.5>\n"
    );
    // A `&mut dyn` argument and a field's borrow change the place itself.
    let output = printed(
        r#"
use std::fmt;
trait Half { fn half(&self) -> String; }
impl Half for f32 { fn half(&self) -> String { format!("f32 {}", self / 2.0) } }
impl Half for f64 { fn half(&self) -> String { format!("f64 {}", self / 2.0) } }
trait Show { fn show(&self) -> String; fn bump(&mut self); }
trait Named: Show { fn name(&self) -> String; }
struct Wr<T>(T, u32);
impl<T: Half> Show for Wr<T> {
    fn show(&self) -> String { format!("{} #{}", self.0.half(), self.1) }
    fn bump(&mut self) { self.1 += 1; }
}
fn poke(s: &mut dyn Show) { s.bump(); }
struct Holder { w: Wr<f32> }
fn main() {
    let mut a = Wr(1.0f32, 0);
    poke(&mut a);
    poke(&mut a);
    let mut h = Holder { w: Wr(2.0f32, 5) };
    poke(&mut h.w);
    let r: &mut dyn Show = &mut h.w;
    r.bump();
    println!("{} {}", a.show(), h.w.show());
}
main();
"#,
    );
    assert_eq!(output, "f32 0.5 #2 f32 1 #7\n");
}

#[test]
fn a_witnessed_impl_method_runs_through_a_trait_object() {
    // An impl method whose generics take a witness (`M: Make`, `const N`)
    // gets its witnesses from the trait object's key, and `{}` of a value
    // passes them too (#2212).
    let output = printed(
        r#"
use std::fmt;
use std::marker::PhantomData;
trait Make { fn make() -> Self; fn name(&self) -> String; }
struct A;
struct B(u8);
impl Make for A { fn make() -> Self { A } fn name(&self) -> String { "A".into() } }
impl Make for B { fn make() -> Self { B(7) } fn name(&self) -> String { format!("B{}", self.0) } }
trait Run { fn run(&self) -> String; }
struct Wrap<M>(PhantomData<M>);
impl<M: Make> Run for Wrap<M> { fn run(&self) -> String { M::make().name() } }
struct Buf<const N: usize>;
impl<const N: usize> fmt::Display for Buf<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "buf{}", N) }
}
fn main() {
    let runs: Vec<Box<dyn Run>> = vec![Box::new(Wrap::<A>(PhantomData)), Box::new(Wrap::<B>(PhantomData))];
    for r in &runs { println!("{}", r.run()); }
    let shown: Vec<Box<dyn fmt::Display>> = vec![Box::new(Buf::<5>), Box::new(Buf::<8>)];
    println!("{} {} {}", Buf::<3>, shown[0], shown[1]);
}
main();
"#,
    );
    assert_eq!(output, "A\nB7\nbuf3 buf5 buf8\n");
}

#[test]
fn a_generic_traits_object_runs_its_types_impl() {
    // `dyn Conv<i64>` keys its value too: `Foo<i32>`'s impl beside
    // `Foo<i64>`'s, a const parameter's witness and a generic impl's
    // instance, each at the trait's arguments (D1115.3, #2212, #2622).
    let output = printed(
        r#"
trait Half { fn half(&self) -> String; }
impl Half for f32 { fn half(&self) -> String { format!("f32 {}", self / 2.0) } }
impl Half for f64 { fn half(&self) -> String { format!("f64 {}", self / 2.0) } }
trait Conv<U> { fn conv(&self, by: U) -> String; }
struct Foo<T> { value: T }
impl Conv<i64> for Foo<i32> { fn conv(&self, by: i64) -> String { format!("i32 {} {}", self.value, by) } }
impl Conv<i64> for Foo<i64> { fn conv(&self, by: i64) -> String { format!("i64 {} {}", self.value, by) } }
impl Conv<String> for Foo<i64> { fn conv(&self, by: String) -> String { format!("s {} {}", self.value, by) } }
struct Buf<const N: usize>;
impl<const N: usize> Conv<i64> for Buf<N> { fn conv(&self, by: i64) -> String { format!("buf {}", N as i64 * by) } }
struct Wr<T>(T);
impl<T: Half> Conv<i64> for Wr<T> { fn conv(&self, by: i64) -> String { format!("{} {}", self.0.half(), by) } }
fn main() {
    let a = Foo { value: 1i32 };
    let b = Foo { value: 2i64 };
    let all: Vec<&dyn Conv<i64>> = vec![&a, &b, &Buf::<4>, &Wr(1.0f32), &Wr(1.0f64)];
    for c in &all { println!("{}", c.conv(10)); }
    let s: &dyn Conv<String> = &b;
    println!("{}", s.conv("x".to_string()));
}
main();
"#,
    );
    assert_eq!(
        output,
        "i32 1 10\ni64 2 10\nbuf 40\nf32 0.5 10\nf64 0.5 10\ns 2 x\n"
    );
}

#[test]
fn a_borrowed_f32_trait_object_writes_f32_digits() {
    // `&dyn Debug` of an `f32` writes it as one, as a box of it does
    // (#2622).
    let output = printed(
        r#"
use std::fmt::{Debug, Display};
fn main() {
    let f = 0.1f32;
    let g: &dyn Debug = &f;
    let h: &dyn Display = &f;
    let t = (1.5f32, u64::MAX);
    let u: &dyn Debug = &t;
    let b: Box<dyn Debug> = Box::new(0.1f32);
    println!("{:?} {} {:?} {:?}", g, h, u, b);
}
main();
"#,
    );
    assert_eq!(output, "0.1 0.1 (1.5, 18446744073709551615) 0.1\n");
}

#[test]
fn a_generic_body_runs_at_its_callers_types() {
    // A generic function whose body uses its parameters' types runs as
    // rustc's monomorphised instance does: arithmetic, formatting, parse,
    // sums and impl choice at the type each call gives it (D1115, #2569).
    let output = printed(
        r#"
use std::fmt::{Debug, Display};
use std::iter::Sum;
use std::ops::{Add, Mul};
use std::str::FromStr;
trait Scale<T> { fn scale(&self, by: T) -> String; }
impl Scale<i64> for f32 { fn scale(&self, by: i64) -> String { format!("f32 {}", *self * by as f32) } }
impl Scale<i64> for f64 { fn scale(&self, by: i64) -> String { format!("f64 {}", *self * by as f64) } }
trait Describe { fn describe(&self) -> String; }
struct Foo<T> { value: T }
impl Describe for Foo<i32> { fn describe(&self) -> String { format!("i32 {}", self.value) } }
impl Describe for Foo<i64> { fn describe(&self) -> String { format!("i64 {}", self.value) } }
fn twice<T: Add<Output = T> + Copy>(x: T) -> T { x + x }
fn square<T: Mul<Output = T> + Copy + Display>(x: T) -> String { format!("{} {:>8.2}", x * x, x) }
fn read<T: FromStr>(s: &str) -> Option<T> where T::Err: Debug { s.parse::<T>().ok() }
fn text<T: ToString>(x: T) -> String { x.to_string() }
fn total<T: Sum<T> + Copy>(v: &[T]) -> T { v.iter().copied().sum() }
fn go<T: Scale<i64>>(x: T) -> String { x.scale(2) }
fn show<T: Describe>(x: &T) -> String { x.describe() }
fn main() {
    println!("{} {}", twice(100u8), twice(1.1f32));
    println!("{} | {}", square(3.3f32), square(3.3f64));
    let a: Option<f32> = read("0.1");
    println!("{:?} {:?}", a, read::<f32>("0.1").map(|x| x * 3.0));
    println!("{} {} {}", text(0.3f32), text(0.3f64), text(7u8));
    println!("{} {} {}", total(&[1i64, 2, 3]), total(&[0.1f32, 0.2]), total(&[100u8, 100, 50]));
    println!("{} | {}", go(1.5f32), go(1.5f64));
    println!("{} {}", show(&Foo { value: 1i32 }), show(&Foo { value: 2i64 }));
}
main();
"#,
    );
    assert_eq!(
        output,
        "200 2.2\n10.889999     3.30 | 10.889999999999999     3.30\nSome(0.1) Some(0.3)\n\
         0.3 0.3 7\n6 0.3 250\nf32 3 | f64 3\ni32 1 i64 2\n"
    );
    for (program, message) in [
        (
            "use std::ops::Add;\nfn twice<T: Add<Output = T> + Copy>(x: T) -> T { x + x }\n\
             fn main() { println!(\"{}\", twice(200u8)); }\nmain();\n",
            "attempt to add with overflow",
        ),
        (
            "use std::iter::Sum;\nfn total<T: Sum<T> + Copy>(v: &[T]) -> T { v.iter().copied().sum() }\n\
             fn main() { println!(\"{}\", total(&[100u8, 100, 100])); }\nmain();\n",
            "attempt to add with overflow",
        ),
    ] {
        let error = Engine::new()
            .run(program)
            .expect_err("a `u8` sum past 255 panics");
        assert!(error.to_string().contains(message), "{error}");
    }
}

#[test]
fn an_instance_keeps_the_generic_reads_resolutions() {
    // An instance calls what its template's bounds resolve, not what its
    // type has of that name: a bound's method over an inherent one, a
    // bound's associated function over the type's, and a nested generic
    // at the instance's types (D1114, D1115).
    let output = printed(
        r#"
trait Tr { fn len(&self) -> usize; }
impl Tr for Vec<i32> { fn len(&self) -> usize { 99 } }
trait Make { fn new() -> Self; fn tag(&self) -> &'static str; }
struct A;
impl A { fn new() -> Self { A } fn tag(&self) -> &'static str { "inherent tag" } }
impl Make for A { fn new() -> Self { A } fn tag(&self) -> &'static str { "trait tag" } }
fn size<T: Tr>(x: &T) -> usize { x.len() }
fn made<T: Make>() -> &'static str { T::new().tag() }
fn outer<T: Make>(x: &T) -> String { format!("{} {}", x.tag(), made::<T>()) }
fn main() {
    println!("{} {} {}", size(&vec![1, 2]), made::<A>(), outer(&A));
}
main();
"#,
    );
    assert_eq!(output, "99 trait tag trait tag trait tag\n");
}

#[test]
fn a_generic_impls_method_runs_at_its_receivers_types() {
    // An inherent method of a generic impl whose body uses the impl's
    // parameters runs at the receiver's type arguments, as rustc's
    // instance does: an `f64`'s impl, a `u8`'s overflow, an `f32`'s digits
    // (D1115).
    let output = printed(
        r#"
use std::fmt::Display;
use std::ops::Add;
trait Half { fn half(&self) -> String; }
impl Half for f32 { fn half(&self) -> String { format!("f32 {}", self / 2.0) } }
impl Half for f64 { fn half(&self) -> String { format!("f64 {}", self / 2.0) } }
struct W<T>(T);
impl<T: Half> W<T> { fn go(&self) -> String { self.0.half() } }
struct P<T>(T);
impl<T: Add<Output = T> + Copy + Display> P<T> {
    fn new(x: T) -> Self { P(x) }
    fn dbl(&self) -> T { self.0 + self.0 }
    fn shown(&self) -> String { format!("{}", self.0) }
}
fn main() {
    println!("{} | {}", W(1.0f64).go(), W(1.0f32).go());
    println!("{} {} {}", P(100u8).dbl(), P::new(0.1f32).shown(), P(1.1f32).dbl());
}
main();
"#,
    );
    assert_eq!(output, "f64 0.5 | f32 0.5\n200 0.1 2.2\n");
    let error = Engine::new()
        .run(
            "struct P<T>(T);\nimpl<T: std::ops::Add<Output = T> + Copy> P<T> { fn dbl(&self) -> T \
             { self.0 + self.0 } }\nfn main() { println!(\"{}\", P(200u8).dbl()); }\nmain();\n",
        )
        .expect_err("a `u8` sum past 255 panics");
    assert!(
        error.to_string().contains("attempt to add with overflow"),
        "{error}"
    );
}

#[test]
fn an_array_type_owns_its_impls_apart_from_its_slice() {
    // `impl Total for [i32; 3]` beside `impl Total for [i32]`, and an impl
    // for every length of an array (#2214).
    let output = printed(
        r#"
trait Total { fn total(&self) -> i32; }
impl Total for [i32; 3] { fn total(&self) -> i32 { self.iter().sum() } }
impl Total for [i32] { fn total(&self) -> i32 { self.iter().sum::<i32>() * 10 } }
trait Len { fn size(&self) -> usize; }
impl<const N: usize> Len for [u8; N] { fn size(&self) -> usize { N } }
fn main() {
    let a = [1, 2, 3];
    println!("{} {} {} {}", a.total(), a[..].total(), [0u8; 4].size(), [1u8, 2].size());
}
main();
"#,
    );
    assert_eq!(output, "6 60 4 2\n");
}

#[test]
fn a_relaxed_sized_parameter_tells_a_slice_from_a_vec() {
    // `T: Tr + ?Sized` at `[i32]` and at `Vec<i32>` runs each one's impl,
    // which their runtime value doesn't tell apart (#801).
    let output = printed(
        r#"
trait Tr { fn name(&self) -> String; }
impl<T> Tr for [T] { fn name(&self) -> String { format!("slice {}", self.len()) } }
impl<T> Tr for Vec<T> { fn name(&self) -> String { format!("vec {}", self.len()) } }
fn f<T: Tr + ?Sized>(t: &T) -> String { t.name() }
fn main() {
    let v = vec![1, 2, 3];
    println!("{} {} {} {}", f(&v), f(&v[..]), f(&v[1..]), f(&vec!["a"]));
}
main();
"#,
    );
    assert_eq!(output, "vec 3 slice 3 slice 2 vec 1\n");
}

#[test]
fn a_tail_temporary_drops_after_its_blocks_locals_before_edition_2024() {
    // Before edition 2024 a block's tail temporaries drop at the end of the
    // temporary scope around the block, after the block's locals, and a
    // body's between its locals and its parameters. A loop's, a branch's,
    // an arm's and a closure's body is a temporary scope of its own.
    // Edition 2024 drops them with the tail, before the locals. Each is
    // what rustc's build prints.
    assert_eq!(
        edition_printed("order", "2021", TAIL_ORDER),
        "drop local\ndrop temp\nafter let 1\ndrop if local\n\
         drop if temp\nafter if 1\ndrop arm local\ndrop arm temp\n\
         after match 1\ndrop body local\ndrop body temp\ndrop param\n\
         1\ndrop closure local\ndrop closure temp\ndrop closure param\n\
         1\ndrop inner local\ndrop outer local\ndrop inner temp\n\
         after nested 1\ndrop vec local\ndrop vec temp\nafter vec 2\n\
         drop stmt local\ndrop rhs local\ndrop rhs temp\ndrop stmt temp\n\
         after sum 2\ndrop loop local\ndrop loop temp\niteration 0\n\
         drop loop local\ndrop loop temp\niteration 1\ndrop loop body local\n\
         drop loop body temp\nend\n"
    );
    assert_eq!(
        edition_printed("order", "2024", TAIL_ORDER),
        "drop temp\ndrop local\nafter let 1\ndrop if temp\n\
         drop if local\nafter if 1\ndrop arm temp\ndrop arm local\n\
         after match 1\ndrop body temp\ndrop body local\ndrop param\n\
         1\ndrop closure temp\ndrop closure local\ndrop closure param\n\
         1\ndrop inner temp\ndrop inner local\ndrop outer local\n\
         after nested 1\ndrop vec temp\ndrop vec local\nafter vec 2\n\
         drop stmt temp\ndrop stmt local\ndrop rhs temp\ndrop rhs local\n\
         after sum 2\ndrop loop temp\ndrop loop local\niteration 0\n\
         drop loop temp\ndrop loop local\niteration 1\ndrop loop body temp\n\
         drop loop body local\nend\n"
    );
}

/// An `if let` subject, a `match` scrutinee, a `vec!` element and a
/// `while` condition, each a block whose tail makes a value that prints
/// its drop.
const TAIL_SCOPES: &str = r#"struct D(&'static str);
impl Drop for D {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}
impl D {
    fn get(&self) -> i32 {
        1
    }
    fn some(&self) -> Option<i32> {
        Some(2)
    }
}
fn main() {
    if let Some(x) = { let _a = D("subject local"); D("subject temp").some() } {
        println!("matched {x}");
    }
    println!("after if let");
    match { let _a = D("scrutinee local"); D("scrutinee temp").get() } {
        1 => println!("one"),
        _ => println!("other"),
    }
    println!("after match");
    let v = vec![{ let _a = D("vec local"); D("vec temp").get() }, 2];
    println!("after vec {}", v.len());
    let mut i = 0;
    while { let _a = D("cond local"); D("cond temp").get() } > i {
        println!("loop {i}");
        i += 1;
    }
    println!("end");
}
"#;

#[test]
fn a_tail_temporary_in_a_condition_or_subject_drops_with_it_before_edition_2024() {
    // A block expression's tail temporaries that go on past its locals
    // drop with the temporary scope around the block: an `if let`'s and a
    // `match`'s after the body, a condition's before it. Each is what
    // rustc's build prints.
    assert_eq!(
        edition_printed("scopes", "2021", TAIL_SCOPES),
        "drop subject local\nmatched 2\ndrop subject temp\nafter if let\n\
         drop scrutinee local\none\ndrop scrutinee temp\nafter match\n\
         drop vec local\ndrop vec temp\nafter vec 2\ndrop cond local\n\
         drop cond temp\nloop 0\ndrop cond local\ndrop cond temp\n\
         end\n"
    );
    assert_eq!(
        edition_printed("scopes", "2024", TAIL_SCOPES),
        "drop subject temp\ndrop subject local\nmatched 2\nafter if let\n\
         drop scrutinee temp\ndrop scrutinee local\none\nafter match\n\
         drop vec temp\ndrop vec local\nafter vec 2\ndrop cond temp\n\
         drop cond local\nloop 0\ndrop cond temp\ndrop cond local\n\
         end\n"
    );
}

/// A block whose tail reads through a `RefCell`'s borrow of a local.
const GUARD_BLOCK: &str = r#"use std::cell::RefCell;

fn main() {
    let n = {
        let x = RefCell::new(1);
        *x.borrow()
    };
    println!("{n}");
}
"#;

/// A body whose tail calls a method through a `RefCell`'s mutable borrow
/// of a local.
const GUARD_BODY: &str = r#"use std::cell::RefCell;

fn total() -> usize {
    let v = RefCell::new(vec![1, 2]);
    v.borrow_mut().len()
}

fn main() {
    println!("{}", total());
}
"#;

#[test]
fn a_refcell_guard_in_a_tail_outlives_its_cell_before_edition_2024() {
    // `borrow()` gives std's `Ref<'_, T>` and `borrow_mut()` its
    // `RefMut<'_, T>`, whose drop uses the borrow of the cell. Before
    // edition 2024 one made in a block's tail drops after the block's
    // locals: a borrow of a local cell is rustc's E0597, which names the
    // guard's destructor (#2620). Edition 2024 drops it first.
    assert_eq!(
        edition_rendered("guard-block", "2021", GUARD_BLOCK),
        "error[E0597]: `x` does not live long enough\n  |\n6 |         let x = RefCell::new(1);\n  |             - binding `x` declared here\n7 |         *x.borrow()\n  |          ^---------\n  |          |\n  |          borrowed value does not live long enough\n  |          a temporary with access to the borrow is created here ...\n8 |     };\n  |     -- ... and the borrow might be used here, when that temporary is dropped and runs the destructor for type `Ref<'_, i32>`\n  |     |\n  |     `x` dropped here while still borrowed\n  |\n  = note: the temporary is part of an expression at the end of a block;\n          consider forcing this temporary to be dropped sooner, before the block's local variables are dropped\nhelp: for example, you could save the expression's value in a new local variable `x` and then make `x` be the expression at the end of the block\n  |\n7 |         let x = *x.borrow(); x\n  |         +++++++            +++\n"
    );
    assert_eq!(edition_printed("guard-block", "2024", GUARD_BLOCK), "1\n");
    assert_eq!(
        edition_rendered("guard-body", "2021", GUARD_BODY),
        "error[E0597]: `v` does not live long enough\n  |\n5 |     let v = RefCell::new(vec![1, 2]);\n  |         - binding `v` declared here\n6 |     v.borrow_mut().len()\n  |     ^-------------\n  |     |\n  |     borrowed value does not live long enough\n  |     a temporary with access to the borrow is created here ...\n7 | }\n  | -\n  | |\n  | `v` dropped here while still borrowed\n  | ... and the borrow might be used here, when that temporary is dropped and runs the destructor for type `RefMut<'_, Vec<i32>>`\n  |\n  = note: the temporary is part of an expression at the end of a block;\n          consider forcing this temporary to be dropped sooner, before the block's local variables are dropped\nhelp: for example, you could save the expression's value in a new local variable `x` and then make `x` be the expression at the end of the block\n  |\n6 |     let x = v.borrow_mut().len(); x\n  |     +++++++                     +++\n"
    );
    assert_eq!(edition_printed("guard-body", "2024", GUARD_BODY), "2\n");
}

/// Guards read, changed, printed and held through a `RefCell`, an `Rc` of
/// one and a `Mutex`, in blocks' tails whose cells outlive them.
const GUARD_USES: &str = r#"use std::cell::RefCell;
use std::rc::Rc;

fn first(v: &RefCell<Vec<i32>>) -> i32 {
    v.borrow()[0]
}

fn main() {
    let c = RefCell::new(vec![1, 2]);
    c.borrow_mut().push(3);
    *c.borrow_mut() = vec![4, 5, 6];
    let n = c.borrow().len();
    println!("{n} {:?} {}", c.borrow(), first(&c));
    let s = RefCell::new(String::from("hi"));
    println!("{} {}", s.borrow(), s.borrow().len());
    let shared = Rc::new(RefCell::new(7));
    *shared.borrow_mut() += 1;
    let total = { *shared.borrow() + 1 };
    let g = c.borrow();
    println!("{total} {} {}", g[1], g.iter().sum::<i32>());
    drop(g);
    drop(c);
    let m = std::sync::Mutex::new(2);
    let doubled = { *m.lock().unwrap() * 2 };
    println!("{doubled}");
}
"#;

#[test]
fn a_refcell_guard_derefs_to_its_value_in_every_edition() {
    // A guard derefs to the cell's value, and `Display` and `Debug` go
    // through it. A tail's guard of a cell that outlives the block is
    // accepted before edition 2024 too. Each is what rustc's build prints.
    let printed = "3 [4, 5, 6] 4\nhi 2\n9 5 15\n4\n";
    assert_eq!(edition_printed("guard-uses", "2021", GUARD_USES), printed);
    assert_eq!(edition_printed("guard-uses", "2024", GUARD_USES), printed);
}

#[test]
fn a_refcell_guard_assigned_to_a_local_drops_where_the_local_ends() {
    // `r = c.borrow();` moves the guard into `r`, whose drop at the end of
    // `main` uses the borrow of `c`, which ended with its block: rustc's
    // E0597 though `r` is never read.
    let report = refused(
        "use std::cell::RefCell;
fn main() {
    let r;
    {
        let c = RefCell::new(1);
        r = c.borrow();
    }
}
main();
",
    );
    assert!(
        report.starts_with("error[E0597]: `c` does not live long enough"),
        "{report}"
    );
}

/// A type whose drop prints its name, and a block, a branch, an arm, a
/// body, a closure, a nested block, a sum of blocks and loops whose tails
/// make one.
const TAIL_ORDER: &str = r#"struct D(&'static str);
impl Drop for D {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}
impl D {
    fn get(&self) -> i32 {
        1
    }
    fn touch(&self) {}
}
fn body(_p: D) -> i32 {
    let _a = D("body local");
    D("body temp").get()
}
fn main() {
    let n = { let _a = D("local"); D("temp").get() };
    println!("after let {n}");
    let m = if n == 1 { let _a = D("if local"); D("if temp").get() } else { 0 };
    println!("after if {m}");
    let k = match n { 1 => { let _a = D("arm local"); D("arm temp").get() } _ => 0 };
    println!("after match {k}");
    println!("{}", body(D("param")));
    let c = |_p: D| { let _a = D("closure local"); D("closure temp").get() };
    println!("{}", c(D("closure param")));
    let nested = { let _o = D("outer local"); { let _i = D("inner local"); D("inner temp").get() } };
    println!("after nested {nested}");
    let v = vec![{ let _a = D("vec local"); D("vec temp").get() }, 2];
    println!("after vec {}", v.len());
    let s = { let _a = D("stmt local"); D("stmt temp").get() } + { let _b = D("rhs local"); D("rhs temp").get() };
    println!("after sum {s}");
    for i in 0..2 {
        let _l = { let _a = D("loop local"); D("loop temp").get() + i };
        println!("iteration {i}");
    }
    for _ in 0..1 {
        let _a = D("loop body local");
        D("loop body temp").touch()
    }
    println!("end");
}
"#;

#[test]
fn a_compound_assignment_through_an_operator_impl_borrows_its_place_mut() {
    // `v += x` through `AddAssign` is `AddAssign::add_assign(&mut v, x)`:
    // a two-phase `&mut` borrow of `v`, reserved before `x` is made and
    // activated after it, and the call moves `x`, as `a + b` moves both
    // operands (#130).
    let output = printed(
        r#"
use std::ops::{Add, AddAssign};
#[derive(Debug, Clone, Copy)]
struct V(i32);
impl AddAssign for V { fn add_assign(&mut self, o: V) { self.0 += o.0; } }
impl AddAssign<i32> for V { fn add_assign(&mut self, o: i32) { self.0 += o; } }
impl Add for V { type Output = V; fn add(self, o: V) -> V { V(self.0 + o.0) } }
fn main() {
    let mut v = V(1);
    v += v;
    v += V(v.0);
    v += v.0;
    let w = v + v;
    let mut vs = vec![V(1)];
    vs[0] += 2;
    let r = &mut v;
    *r += 3;
    println!("{:?} {:?} {:?}", v, w, vs);
}
main();
"#,
    );
    assert_eq!(output, "V(11) V(16) [V(3)]\n");
    let shared = refused(
        r#"
use std::ops::AddAssign;
#[derive(Debug, Clone, Copy)]
struct V(i32);
impl AddAssign for V { fn add_assign(&mut self, o: V) { self.0 += o.0; } }
fn main() { let mut v = V(1); let r = &v; v += V(2); println!("{:?}", r); }
main();
"#,
    );
    assert!(
        shared.contains(
            "error[E0502]: cannot borrow `v` as mutable because it is also borrowed as \
             immutable\n --> <script>:6:43"
        ),
        "{shared}"
    );
    let moved = refused(
        r#"
use std::ops::AddAssign;
#[derive(Debug)]
struct V(i32);
impl AddAssign for V { fn add_assign(&mut self, o: V) { self.0 += o.0; } }
fn main() { let mut v = V(1); v += v; println!("{:?}", v); }
main();
"#,
    );
    assert!(
        moved.contains(
            "error[E0505]: cannot move out of `v` because it is borrowed\n --> <script>:6:36"
        ),
        "{moved}"
    );
    let operand = refused(
        r#"
use std::ops::AddAssign;
#[derive(Debug)]
struct V(i32);
impl AddAssign for V { fn add_assign(&mut self, o: V) { self.0 += o.0; } }
fn main() { let mut w = V(1); let v = V(2); w += v; println!("{:?} {:?}", w, v); }
main();
"#,
    );
    assert!(
        operand.contains("error[E0382]: borrow of moved value: `v`\n --> <script>:6:78"),
        "{operand}"
    );
    // An element's `IndexMut::index_mut` borrow is made before the value.
    let indexed = refused(
        r#"
use std::ops::AddAssign;
struct V(i32);
impl AddAssign<i32> for V { fn add_assign(&mut self, o: i32) { self.0 += o; } }
fn main() { let mut vs = vec![V(1)]; vs[0] += { vs = vec![]; 0 }; }
main();
"#,
    );
    assert!(
        indexed.contains(
            "error[E0506]: cannot assign to `vs` because it is borrowed\n --> <script>:5:49"
        ),
        "{indexed}"
    );
}

#[test]
fn a_function_value_from_a_method_a_field_or_a_branch_is_called_as_its_type_says() {
    // A local set from a method's `impl Fn`, a field's `fn`, or an `if` or
    // `match` of `impl Fn` values is called through a shared borrow, as
    // `Fn::call` borrows it; a `Box<dyn FnMut>` through a `&mut` one, as
    // `FnMut::call_mut` does (#188).
    let output = printed(
        r#"
struct M;
impl M { fn adder(&self) -> impl Fn(i32) -> i32 { |x| x + 3 } }
struct H { f: fn(i32) -> i32 }
fn one(x: i32) -> i32 { x + 1 }
fn make() -> impl Fn(i32) -> i32 { |x| x * 10 }
fn main() {
    let m = M;
    let mut g = m.adder();
    let r = &g;
    println!("{} {}", g(1), r(2));
    let h = H { f: one };
    let mut f = h.f;
    let r = &f;
    println!("{} {}", f(1), r(2));
    let k = 2;
    let mut b = if k > 1 { make() } else { make() };
    let r = &b;
    println!("{} {}", b(1), r(2));
    let mut a = match k { 1 => make(), _ => make() };
    let r = &a;
    println!("{} {}", a(3), r(4));
    let mut c = 0;
    let mut counter: Box<dyn FnMut() -> i32> = Box::new(move || { c += 1; c });
    println!("{} {}", counter(), counter());
}
main();
"#,
    );
    assert_eq!(output, "4 5\n2 3\n10 20\n30 40\n1 2\n");
    let written = refused(
        r#"
fn main() { let mut c = 0; let mut f: Box<dyn FnMut() -> i32> = Box::new(move || { c += 1; c }); let r = &f; f(); std::mem::drop(r); }
main();
"#,
    );
    assert!(
        written.contains(
            "error[E0502]: cannot borrow `f` as mutable because it is also borrowed as \
             immutable\n --> <script>:2:110"
        ),
        "{written}"
    );
    let returned = refused(
        r#"
fn counter() -> Box<dyn FnMut() -> i32> { let mut c = 0; Box::new(move || { c += 1; c }) }
fn main() { let mut f = counter(); let r = &f; f(); std::mem::drop(r); }
main();
"#,
    );
    assert!(
        returned.contains(
            "error[E0502]: cannot borrow `f` as mutable because it is also borrowed as \
             immutable\n --> <script>:3:48"
        ),
        "{returned}"
    );
}

#[test]
fn an_array_passed_as_impl_into_iterator_where_the_call_is_expected() {
    // A generic call whose value the caller expects types its parameters
    // from that expectation, but an `impl IntoIterator` parameter is its
    // own type parameter: the array is the iterator over its elements,
    // never coerced to the iterator type.
    let output = printed(
        r#"
struct E<M>(M);
struct W<M>(Vec<E<M>>);
fn group<M>(content: impl IntoIterator<Item = E<M>>) -> W<M> {
    W(content.into_iter().collect())
}
fn outer<M: Clone>(m: M) -> W<M> {
    group([E(m.clone()), E(m)])
}
fn main() {
    println!("{} {}", outer(1).0.len(), group([E('a')]).0.len());
}
main();
"#,
    );
    assert_eq!(output, "2 1\n");
}

#[test]
fn a_closure_local_that_shadows_a_capture_is_the_closure_s_own_borrow() {
    // `let c = c.borrow();` in a closure binds a `Ref` of the captured
    // cell, and `c` after it is that `Ref`, not the capture (#2229).
    let output = printed(
        r#"
use std::cell::RefCell;
use std::rc::Rc;
struct Counted { n: i32 }
impl Counted { fn get(&self) -> i32 { self.n } }
fn main() {
    let c = Rc::new(RefCell::new(Counted { n: 3 }));
    let f = move || { let c = c.borrow(); c.get() };
    let b = RefCell::new(Counted { n: 2 });
    let g = || { let b = b.borrow(); let n = b.get(); drop(b); n };
    let h = || { let mut b = b.borrow_mut(); b.n += 1; b.n };
    println!("{} {} {}", f(), g(), h());
    println!("{}", b.borrow().n);
}
main();
"#,
    );
    assert_eq!(output, "3 2 3\n3\n");
}

#[test]
fn a_trait_object_is_send_only_as_its_type_writes_it() {
    // `dyn Shape` is `Send` only with `+ Send` or a `Send` supertrait, and
    // only what is `Send` becomes `dyn Shape + Send` (#2485).
    let output = printed(
        r#"
use std::rc::Rc;
trait Shape { fn area(&self) -> f64; }
trait Named: Send { fn name(&self) -> String; }
struct Sq(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0 * self.0 } }
impl Named for Sq { fn name(&self) -> String { "sq".into() } }
struct Shared(Rc<f64>);
impl Shape for Shared { fn area(&self) -> f64 { *self.0 } }
fn main() {
    let b: Box<dyn Shape + Send> = Box::new(Sq(2.0));
    let n: Box<dyn Named> = Box::new(Sq(1.0));
    let h = std::thread::spawn(move || (b.area(), n.name()));
    println!("{:?}", h.join().unwrap());
    let local: Box<dyn Shape> = Box::new(Shared(Rc::new(3.0)));
    println!("{}", local.area());
}
main();
"#,
    );
    assert_eq!(output, "(4.0, \"sq\")\n3\n");
    let output = printed(
        r#"
use std::error::Error;
fn parse(text: &str) -> Result<i32, Box<dyn Error + Send + Sync>> {
    if text.is_empty() {
        return Err("empty".into());
    }
    Ok(text.parse::<i32>()?)
}
fn main() {
    let h = std::thread::spawn(|| (parse("4"), parse("")));
    let (a, b) = h.join().unwrap();
    let local: Result<i32, Box<dyn Error + Send + Sync>> = Err("local".into());
    let moved = std::thread::spawn(move || local.unwrap_err().to_string());
    println!("{:?} {} {}", a.ok(), b.unwrap_err(), moved.join().unwrap());
}
main();
"#,
    );
    assert_eq!(output, "Some(4) empty local\n");
    let report = refused(
        r#"
use std::rc::Rc;
trait Shape { fn area(&self) -> f64; }
struct Sq(Rc<f64>);
impl Shape for Sq { fn area(&self) -> f64 { *self.0 * *self.0 } }
fn main() {
    let b: Box<dyn Shape> = Box::new(Sq(Rc::new(2.0)));
    let h = std::thread::spawn(move || b.area());
    println!("{}", h.join().unwrap());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `dyn Shape` cannot be sent between threads safely"),
        "{report}"
    );
    assert!(
        report.contains("note: required because it's used within this closure"),
        "{report}"
    );
    let report = refused(
        r#"
use std::rc::Rc;
trait Shape { fn area(&self) -> f64; }
struct Shared(Rc<f64>);
impl Shape for Shared { fn area(&self) -> f64 { *self.0 } }
fn main() {
    let b: Box<dyn Shape + Send> = Box::new(Shared(Rc::new(3.0)));
    println!("{}", b.area());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Rc<f64>` cannot be sent between threads safely"),
        "{report}"
    );
    assert!(
        report.contains("help: within `Shared`, the trait `Send` is not implemented for `Rc<f64>`"),
        "{report}"
    );
    assert!(
        report.contains("note: required because it appears within the type `Shared`"),
        "{report}"
    );
}

#[test]
fn e0277_for_a_value_that_isn_t_send_names_the_type_that_isn_t() {
    // rustc names the innermost type that isn't `Send`, with a note at
    // each program type and opaque output it appears within (#2495).
    let report = refused(
        r#"
use std::rc::Rc;
struct Sq(Rc<f64>);
fn main() {
    let s = Sq(Rc::new(2.0));
    let h = std::thread::spawn(move || *s.0);
    println!("{}", h.join().unwrap());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Rc<f64>` cannot be sent between threads safely"),
        "{report}"
    );
    for note in [
        "note: required because it appears within the type `Sq`",
        "note: required because it's used within this closure",
        "required by a bound in `std::thread::spawn`",
    ] {
        assert!(report.contains(note), "{note}: {report}");
    }
    let report = refused(
        r#"
use std::rc::Rc;
trait Shape { fn area(&self) -> f64; }
struct Sq(Rc<f64>);
impl Shape for Sq { fn area(&self) -> f64 { *self.0 * *self.0 } }
fn make() -> impl Shape { Sq(Rc::new(2.0)) }
fn main() {
    let s = make();
    let h = std::thread::spawn(move || s.area());
    println!("{}", h.join().unwrap());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Rc<f64>` cannot be sent between threads safely"),
        "{report}"
    );
    let within_sq = report
        .find("note: required because it appears within the type `Sq`")
        .unwrap_or_else(|| panic!("{report}"));
    let within_opaque = report
        .find("note: required because it appears within the type `impl Shape`")
        .unwrap_or_else(|| panic!("{report}"));
    assert!(within_sq < within_opaque, "{report}");
}

#[test]
fn an_opaque_output_is_send_as_its_hidden_type_is_at_any_send_bound() {
    // A generic call's `T: Send` and spawn's `T: Send` of what the thread
    // gives see through an opaque output to its hidden type (#2496).
    let program = |field: &str, value: &str| {
        format!(
            r#"
use std::rc::Rc;
trait Shape {{ fn area(&self) -> f64; }}
struct Sq({field});
impl Shape for Sq {{ fn area(&self) -> f64 {{ 2.0 }} }}
fn make() -> impl Shape {{ Sq({value}) }}
fn require<T: Send>(t: T) -> T {{ t }}
fn main() {{
    let s = require(make());
    let h = std::thread::spawn(make);
    println!("{{}} {{}}", s.area(), h.join().unwrap().area());
}}
main();
"#
        )
    };
    assert_eq!(printed(&program("f64", "2.0")), "2 2\n");
    let report = refused(&program("Rc<f64>", "Rc::new(2.0)"));
    assert!(
        report.starts_with("error[E0277]: `Rc<f64>` cannot be sent between threads safely"),
        "{report}"
    );
    for note in [
        "help: within `impl Shape`, the trait `Send` is not implemented for `Rc<f64>`",
        "note: required because it appears within the type `Sq`",
        "note: required because it appears within the type `impl Shape`",
        "required by a bound in `require`",
    ] {
        assert!(report.contains(note), "{note}: {report}");
    }
    let report = refused(
        r#"
use std::rc::Rc;
trait Shape { fn area(&self) -> f64; }
struct Sq(Rc<f64>);
impl Shape for Sq { fn area(&self) -> f64 { 2.0 } }
fn make() -> impl Shape { Sq(Rc::new(2.0)) }
fn main() {
    let h = std::thread::spawn(make);
    println!("{}", h.join().unwrap().area());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: `Rc<f64>` cannot be sent between threads safely"),
        "{report}"
    );
    assert!(
        report.contains("required by a bound in `std::thread::spawn`"),
        "{report}"
    );
}

#[test]
fn a_thread_local_key_is_each_threads_own_value() {
    // `thread_local!` keys where an item goes, of `Cell`, `RefCell` and a
    // plain value, with `const { .. }` and lazy initializers: each thread
    // makes its own value at its first access, `set` makes it without the
    // initializer, and a key passes as `&'static LocalKey<T>` (#1765). The
    // output is rustc's.
    let output = printed(
        r#"
use std::cell::{Cell, RefCell};
use std::thread::LocalKey;
thread_local! {
    static COUNT: Cell<u32> = const { Cell::new(1) };
    static NAMES: RefCell<Vec<String>> = RefCell::new(Vec::new());
    pub static LOUD: Cell<i32> = { println!("init LOUD"); Cell::new(7) };
}
thread_local!(static PLAIN: i32 = 40 + 2);
fn bump(key: &'static LocalKey<Cell<u32>>) -> u32 {
    key.set(key.get() + 1);
    key.get()
}
fn main() {
    println!("{}", bump(&COUNT));
    println!("{}", bump(&COUNT));
    COUNT.with(|c| c.set(c.get() * 10));
    println!("{}", COUNT.get());
    NAMES.with_borrow_mut(|names| names.push("a".to_string()));
    NAMES.with_borrow_mut(|names| names.push("b".to_string()));
    let joined = NAMES.with_borrow(|names| names.join(","));
    println!("{joined}");
    LOUD.set(3);
    println!("{}", LOUD.get());
    println!("{}", LOUD.replace(4));
    println!("{}", LOUD.take());
    PLAIN.with(|p| println!("{p}"));
    let seen = std::thread::spawn(|| {
        let before = COUNT.get();
        COUNT.set(100);
        NAMES.with_borrow_mut(|names| names.push("t".into()));
        let len = NAMES.with_borrow(|names| names.len());
        (before, COUNT.get(), len, LOUD.get())
    })
    .join()
    .unwrap();
    println!("{:?}", seen);
    println!("{} {:?}", COUNT.get(), NAMES.take());
    let total = NAMES.with(|cell| cell.borrow().len());
    println!("{total}");
    println!("{:?}", COUNT);
}
main();
"#,
    );
    assert_eq!(
        output,
        "2\n3\n30\na,b\n3\n3\n4\n42\ninit LOUD\n(1, 100, 1, 7)\n30 [\"a\", \"b\"]\n0\nLocalKey { .. }\n"
    );
}

#[test]
fn a_threads_thread_local_values_drop_as_it_ends_the_last_made_first() {
    // A spawned thread's values drop as its closure returns, before its
    // join does, and the main thread's after `main`'s locals, each thread's
    // last made first, a struct's fields after it (#1765). The output is
    // rustc's.
    let output = printed(
        r#"
struct D(&'static str);
impl Drop for D { fn drop(&mut self) { println!("drop {}", self.0); } }
struct Pair { left: D, right: D }
thread_local! {
    static A: D = D("a");
    static B: D = D("b");
    static P: Pair = Pair { left: D("left"), right: D("right") };
}
fn main() {
    let _local = D("local");
    B.with(|d| println!("with {}", d.0));
    A.with(|d| println!("with {}", d.0));
    std::thread::spawn(|| {
        A.with(|d| println!("t {}", d.0));
        P.with(|p| println!("t {} {}", p.left.0, p.right.0));
        B.with(|d| println!("t {}", d.0));
        println!("end of closure");
    }).join().unwrap();
    println!("end of main");
}
main();
"#,
    );
    assert_eq!(
        output,
        "with b\nwith a\nt a\nt left right\nt b\nend of closure\ndrop b\ndrop left\n\
         drop right\ndrop a\nend of main\ndrop local\ndrop a\ndrop b\n"
    );
}

#[test]
fn a_thread_local_key_a_body_declares_is_named_in_its_block() {
    // `thread_local!` in a body declares block-scoped keys, braced or in
    // parentheses through `std::`, each thread's value its own (#1765).
    // The output is rustc's.
    let output = printed(
        r#"
use std::cell::Cell;
fn next_id() -> u32 {
    thread_local! {
        static NEXT: Cell<u32> = const { Cell::new(0) };
    }
    NEXT.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}
fn main() {
    std::thread_local!(static SEEN: Cell<bool> = Cell::new(false));
    println!("{} {} {}", next_id(), next_id(), next_id());
    let other = std::thread::spawn(|| (next_id(), next_id())).join().unwrap();
    println!("{:?} {}", other, next_id());
    SEEN.set(true);
    println!("{}", SEEN.get());
}
main();
"#,
    );
    assert_eq!(output, "0 1 2\n(0, 1) 3\ntrue\n");
}

#[test]
fn a_local_key_method_std_declares_for_another_inner_type_is_e0599() {
    // `get` is `LocalKey<Cell<T>>`'s, `frob` no `LocalKey`'s, and `take`
    // needs `T: Default`, as rustc reports them (#1765).
    let program = |inner: &str, call: &str| {
        format!(
            "use std::cell::Cell;\nstruct S;\nthread_local! {{\n    static N: {inner};\n}}\n\
             fn main() {{\n    let _ = N.{call};\n}}\nmain();\n"
        )
    };
    let report = refused(&program("i32 = 1", "get()"));
    assert!(
        report.starts_with(
            "error[E0599]: no method named `get` found for struct `LocalKey<i32>` in the current \
             scope"
        ),
        "{report}"
    );
    assert!(
        report.contains("method not found in `LocalKey<i32>`"),
        "{report}"
    );
    let report = refused(&program("i32 = 1", "frob()"));
    assert!(
        report.starts_with(
            "error[E0599]: no method named `frob` found for struct `LocalKey<T>` in the current \
             scope"
        ),
        "{report}"
    );
    assert!(
        report.contains("method not found in `LocalKey<i32>`"),
        "{report}"
    );
    let report = refused(&program("Cell<S> = Cell::new(S)", "take()"));
    assert!(
        report.starts_with("error[E0277]: the trait bound `S: Default` is not satisfied"),
        "{report}"
    );
}

#[test]
fn a_refcell_keys_set_take_and_replace_are_its_impls() {
    // std declares `set`, `take` and `replace` in `impl LocalKey<Cell<T>>`
    // and in `impl LocalKey<RefCell<T>>`; the std table keeps both (#1549).
    let output = printed(
        r#"
use std::cell::{Cell, RefCell};
thread_local! {
    static NAMES: RefCell<Vec<String>> = RefCell::new(Vec::new());
    static COUNT: Cell<u32> = Cell::new(0);
}
fn main() {
    NAMES.set(vec!["a".to_string()]);
    let old = NAMES.replace(vec!["b".to_string()]);
    let now = NAMES.take();
    COUNT.set(3);
    println!("{old:?} {now:?} {} {}", COUNT.replace(4), COUNT.take());
    NAMES.with_borrow(|names| println!("{}", names.len()));
}
main();
"#,
    );
    assert_eq!(output, "[\"a\"] [\"b\"] 3 4\n0\n");
}

#[test]
fn a_thread_local_call_std_has_no_rule_for_is_rustcs_error() {
    // What std's `thread_local!` matcher has no rule for: an item that is no
    // `static`, where an item goes and in a body, and a key with no `;`
    // before the next (#1765).
    for (source, token, line) in [
        (
            "thread_local! {\n    fn x() {}\n}\nfn main() {}\nmain();\n",
            "keyword `fn`",
            2,
        ),
        (
            "fn main() {\n    thread_local! {\n        let x = 1;\n    }\n}\nmain();\n",
            "keyword `let`",
            3,
        ),
        (
            "thread_local! {\n    static X: i32 = 1\n    static Y: i32 = 2;\n}\nfn main() {}\n\
             main();\n",
            "keyword `static`",
            3,
        ),
    ] {
        let report = refused(source);
        assert!(
            report.starts_with(&format!("error: no rules expected {token}\n")),
            "{report}"
        );
        assert!(report.contains(&format!(":{line}:")), "{report}");
        assert!(
            report.contains("no rules expected this token in macro call"),
            "{report}"
        );
    }
}

#[test]
fn language_io_write_and_fmt_write_are_two_traits() {
    // `io::Write` and `fmt::Write` are two std traits of one name: each
    // impl, bound and trait object is its own trait's (#758). rustc prints
    // `text 7`.
    let output = printed(
        r#"
use std::fmt;
use std::io::{self, Write};

struct Text(String);
struct Bytes(Vec<u8>);

impl fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.push_str(s);
        Ok(())
    }
}

impl Write for Bytes {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn fill<W: fmt::Write>(out: &mut W) {
    out.write_str("text").unwrap();
}

fn put<W: io::Write>(out: &mut W) {
    out.write_all(b"bytes").unwrap();
}

fn main() {
    let mut text = Text(String::new());
    fill(&mut text);
    let mut bytes = Bytes(Vec::new());
    put(&mut bytes);
    bytes.write_all(b"!").unwrap();
    let sink: &mut dyn io::Write = &mut bytes;
    sink.write_all(b"?").unwrap();
    println!("{} {}", text.0, bytes.0.len());
}
main();
"#,
    );
    assert_eq!(output, "text 7\n");
}

#[test]
fn language_a_bound_by_io_write_refuses_a_fmt_write_type_e0277() {
    // A type that implements `fmt::Write` alone doesn't meet `io::Write`
    // (#758): error[E0277]: the trait bound `A: std::io::Write` is not
    // satisfied.
    let report = refused(
        r#"
use std::fmt;
use std::io;

struct A(String);

impl fmt::Write for A {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.push_str(s);
        Ok(())
    }
}

fn takes<W: io::Write>(_w: W) {}

fn main() {
    takes(A(String::new()));
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `A: std::io::Write` is not satisfied"),
        "{report}"
    );
}

#[test]
fn language_a_std_trait_a_path_names_is_not_in_scope_by_its_name_e0405() {
    // An impl that names `std::fmt::Display` by its path brings no
    // `Display` into scope (#758): error[E0405]: cannot find trait
    // `Display` in this scope.
    let report = refused(
        r#"
struct P;

impl std::fmt::Display for P {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "p")
    }
}

fn show<T: Display>(t: T) -> String {
    format!("{}", t)
}

fn main() {
    println!("{}", show(P));
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0405]: cannot find trait `Display` in this scope"),
        "{report}"
    );
    assert!(report.contains(":10:12"), "{report}");
}

#[test]
fn text_into_takes_its_pointer_target_from_the_use_site() {
    let source = r#"
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
struct Named { name: Box<str> }
fn shared(text: Arc<str>) -> usize { text.len() }
fn boxed() -> Box<str> { "made".into() }
fn main() {
    let b: Box<str> = "box".into();
    let r: Rc<str> = "rc".into();
    let a: Arc<str> = String::from("arc").into();
    let s: &str = "str".into();
    let p: PathBuf = "path".into();
    println!("{b} {r} {a} {s} {} {}", p.display(), b.len());
    let named = Named { name: "field".into() };
    println!("{} {} {}", named.name, shared("arg".into()), boxed());
    let from = Rc::<str>::from("from");
    let hinted: Arc<str> = Arc::from("hinted");
    let slice = Arc::<[i32]>::from(vec![1, 2]);
    let names: Vec<Arc<str>> = vec!["x".into(), a.clone()];
    println!("{from} {hinted} {slice:?} {names:?} {}", Arc::strong_count(&a));
}
main();
"#;
    assert_eq!(
        printed(source),
        "box rc arc str path 3\nfield 3 made\nfrom hinted [1, 2] [\"x\", \"arc\"] 2\n"
    );
}

/// `&mut *w` of a program type is `DerefMut::deref_mut(&mut w)`: an
/// argument, a `let` and a slice coercion of it change `w` and call its
/// `deref_mut` once each (#816). A use of `w` while the reborrow lives is
/// rustc's E0499, an immutable `w` its E0596 naming `w`, and a type with
/// `Deref` alone its E0596. The output and the reports are rustc's for
/// the same programs.

#[test]
fn a_mut_deref_of_a_program_type_goes_through_deref_mut() {
    let head = r#"
use std::ops::{Deref, DerefMut};
struct W(Vec<i64>);
struct R(Vec<i64>);
impl Deref for R { type Target = Vec<i64>; fn deref(&self) -> &Vec<i64> { &self.0 } }
impl Deref for W { type Target = Vec<i64>; fn deref(&self) -> &Vec<i64> { &self.0 } }
impl DerefMut for W { fn deref_mut(&mut self) -> &mut Vec<i64> { println!("deref_mut"); &mut self.0 } }
fn push_through(v: &mut Vec<i64>) { v.push(9); }
"#;
    let source = format!(
        "{head}{}",
        r#"
fn main() {
    let mut w = W(vec![]);
    push_through(&mut *w);
    let r = &mut *w;
    r.push(1);
    r.push(2);
    let s: &mut [i64] = &mut *w;
    s[0] = 7;
    println!("{:?} {}", w.0, w.len());
}
main();
"#
    );
    assert_eq!(
        printed(&source),
        "deref_mut\nderef_mut\nderef_mut\n[7, 1, 2] 3\n"
    );
    for (body, report) in [
        (
            "let mut w = W(vec![]); let r = &mut *w; w.0.push(3); r.push(1);",
            "error[E0499]: cannot borrow `w.0` as mutable more than once at a time",
        ),
        (
            "let w = W(vec![]); push_through(&mut *w);",
            "error[E0596]: cannot borrow `w` as mutable, as it is not declared as mutable",
        ),
        (
            "let mut w = R(vec![]); push_through(&mut *w);",
            "error[E0596]: cannot borrow data in dereference of `R` as mutable",
        ),
    ] {
        let refused = refused(&format!("{head}fn main() {{ {body} }}\nmain();\n"));
        assert!(refused.starts_with(report), "{body}\n{refused}");
    }
}

#[test]
fn a_trait_path_call_of_a_mut_self_method_changes_the_place_it_borrows() {
    // `Iterator::next(&mut c)`, `<C as Iterator>::next(&mut c)` and
    // `fmt::Write::write_str(&mut a, ..)` are the method calls on the
    // places they borrow, as `c.next()` is. The output is rustc's.
    let output = printed(
        r#"
use std::fmt;
struct C { n: u32 }
impl Iterator for C {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.n < 3 { self.n += 1; Some(self.n) } else { None }
    }
}
struct A { s: String }
impl fmt::Write for A {
    fn write_str(&mut self, s: &str) -> fmt::Result { self.s.push_str(s); Ok(()) }
}
struct P { c: C }
fn bump(c: &mut C) -> Option<u32> { Iterator::next(c) }
fn main() {
    let mut c = C { n: 0 };
    let first: Option<u32> = Iterator::next(&mut c);
    println!("{:?} {:?}", first, <C as Iterator>::next(&mut c));
    while let Some(x) = Iterator::next(&mut c) { println!("loop {x}"); }
    let mut p = P { c: C { n: 0 } };
    println!("{:?} {:?} {}", Iterator::next(&mut p.c), bump(&mut p.c), (p.c).n);
    let mut a = A { s: String::new() };
    fmt::Write::write_str(&mut a, "hi").unwrap();
    std::fmt::Write::write_char(&mut a, '!').unwrap();
    <A as fmt::Write>::write_str(&mut a, ".").unwrap();
    println!("{} {}", a.s, c.n);
}
main();
"#,
    );
    assert_eq!(
        output,
        "Some(1) Some(2)\nloop 3\nSome(1) Some(2) 2\nhi!. 3\n"
    );
    // A local not declared `mut` is rustc's E0596 at the borrow.
    let report = rendered(
        "struct C;\nimpl Iterator for C {\n    type Item = u32;\n    fn next(&mut self) -> \
         Option<u32> { None }\n}\nfn main() {\n    let c = C;\n    let _ = <C as \
         Iterator>::next(&mut c);\n}\n",
    );
    assert_eq!(
        report,
        r#"error[E0596]: cannot borrow `c` as mutable, as it is not declared as mutable
 --> main.rs:8:35
  |
8 |     let _ = <C as Iterator>::next(&mut c);
  |                                   ^^^^^^ cannot borrow as mutable
  |
help: consider changing this to be mutable
  |
7 |     let mut c = C;
  |         +++
"#
    );
    let report = rendered(
        "struct C;\nimpl Iterator for C {\n    type Item = u32;\n    fn next(&mut self) -> \
         Option<u32> { None }\n}\nstruct P { c: C }\nfn main() {\n    let p = P { c: C \
         };\n    let _ = Iterator::next(&mut p.c);\n}\n",
    );
    assert!(
        report.starts_with(
            r#"error[E0596]: cannot borrow `p.c` as mutable, as `p` is not declared as mutable
 --> main.rs:9:28
  |
9 |     let _ = Iterator::next(&mut p.c);
  |                            ^^^^^^^^ cannot borrow as mutable
"#
        ),
        "{report}"
    );
}

#[test]
fn an_unmet_bound_a_path_writes_is_marked_whole_with_its_similarly_named_trait() {
    // `W: io::Write` of a type that implements `fmt::Write` alone: the
    // bound marked whole, the help at the type's definition, and rustc's
    // note on the similarly named trait. The report is rustc's.
    let report = rendered(
        "use std::fmt;\nuse std::io;\nstruct A;\nimpl fmt::Write for A {\n    fn write_str(&mut \
         self, _: &str) -> fmt::Result { Ok(()) }\n}\nfn f<W: io::Write>(_: W) {}\nfn main() \
         {\n    f(A);\n}\n",
    );
    assert_eq!(
        report,
        r#"error[E0277]: the trait bound `A: std::io::Write` is not satisfied
 --> main.rs:9:7
  |
9 |     f(A);
  |     - ^ unsatisfied trait bound
  |     |
  |     required by a bound introduced by this call
  |
help: the trait `std::io::Write` is not implemented for `A`
 --> main.rs:3:1
  |
3 | struct A;
  | ^^^^^^^^
  = note: `A` implements similarly named trait `std::fmt::Write`, but not `std::io::Write`
note: required by a bound in `f`
 --> main.rs:7:9
  |
7 | fn f<W: io::Write>(_: W) {}
  |         ^^^^^^^^^ required by this bound in `f`
"#
    );
    // A std type's note is the report's own.
    let report =
        rendered("fn f<W: std::io::Write>(_: W) {}\nfn main() {\n    f(String::new());\n}\n");
    assert!(
        report.contains(
            r#"  |     - ^^^^^^^^^^^^^ the trait `std::io::Write` is not implemented for `String`
  |     |
  |     required by a bound introduced by this call
  |
  = note: `String` implements similarly named trait `std::fmt::Write`, but not `std::io::Write`
note: required by a bound in `f`
"#
        ),
        "{report}"
    );
    // rustc writes a std trait by its path where its name isn't the one
    // type std's crates have of it: `std::error::Error`, `std::io::Read`.
    for (bound, written) in [
        ("std::error::Error", "std::error::Error"),
        ("std::io::Read", "std::io::Read"),
        ("AsRef<str>", "AsRef<str>"),
    ] {
        let report = rendered(&format!(
            "struct A;\nfn f<W: {bound}>(_: W) {{}}\nfn main() {{\n    f(A);\n}}\n"
        ));
        assert!(
            report.starts_with(&format!(
                "error[E0277]: the trait bound `A: {written}` is not satisfied\n"
            )),
            "{report}"
        );
        assert!(
            report.contains(&format!(
                "  |     - ^ unsatisfied trait bound\n  |     |\n  |     required by a bound \
                 introduced by this call\n  |\nhelp: the trait `{written}` is not implemented \
                 for `A`\n"
            )),
            "{report}"
        );
        let marks = "^".repeat(bound.len());
        assert!(
            report.contains(&format!(
                "  |         {marks} required by this bound in `f`"
            )),
            "{report}"
        );
    }
}

#[test]
fn a_local_keys_try_with_is_ok_until_its_value_is_destroyed() {
    // `try_with` is `with` in `Ok`, and std's `AccessError` once the
    // thread's end destroyed the key's value, the last made first: during
    // its own drop and after. A `Cell<u32>` has no drop glue, so its value
    // lasts (#2645). The output is rustc's.
    let output = printed(
        r#"
use std::cell::{Cell, RefCell};
use std::thread::AccessError;
struct D(&'static str);
impl Drop for D {
    fn drop(&mut self) {
        let seen = LOG.try_with(|log| log.borrow().len());
        let own = OWN.try_with(|own| own.0);
        let count = COUNT.try_with(|count| count.get());
        println!("drop {} {:?} {:?} {:?}", self.0, seen, own, count);
        if let Err(error) = own {
            println!("{} | {:?} | {}", error, error, error == error.clone());
        }
    }
}
thread_local! {
    static OWN: D = D("own");
    static LOG: RefCell<Vec<String>> = RefCell::new(Vec::new());
    static COUNT: Cell<u32> = Cell::new(7);
}
fn check(r: Result<u32, AccessError>) -> u32 {
    r.unwrap_or(0)
}
fn main() {
    COUNT.set(3);
    let r = COUNT.try_with(|c| c.get() + 1);
    println!("{:?} {}", r, check(COUNT.try_with(|c| c.get())));
    std::thread::spawn(|| {
        LOG.with_borrow_mut(|log| log.push("x".to_string()));
        OWN.with(|own| println!("t {}", own.0));
        COUNT.set(9);
        println!("t {}", LOG.try_with(|log| log.borrow().len()).unwrap());
    })
    .join()
    .unwrap();
    std::thread::spawn(|| {
        OWN.with(|own| println!("u {}", own.0));
        LOG.with_borrow_mut(|log| log.push("y".to_string()));
    })
    .join()
    .unwrap();
    println!("end of main");
}
main();
"#,
    );
    assert_eq!(
        output,
        "Ok(4) 3\nt own\nt 1\ndrop own Ok(1) Err(AccessError) Ok(9)\nalready destroyed | \
         AccessError | true\nu own\ndrop own Err(AccessError) Err(AccessError) Ok(7)\nalready \
         destroyed | AccessError | true\nend of main\n"
    );
}

#[test]
fn a_local_key_method_of_another_impl_is_rustcs_e0599_with_its_notes() {
    // rustc notes the impls that have the method, suggests a std method
    // of a similar name in its place, a blanket impl's too, and marks a
    // `where` bound the method's impl asks of the cell's type at the
    // method (#2644). Each report is rustc's but for a place in std's
    // source, which rawr's report notes without its source.
    let program = |inner: &str, call: &str| {
        format!(
            "use std::cell::{{Cell, RefCell}};\nstruct S;\nthread_local! {{\n    static N: \
             {inner};\n}}\nfn main() {{\n    let _ = N.{call};\n}}\n"
        )
    };
    let report = rendered(&program("i32 = 1", "get()"));
    assert!(
        report.ends_with(
            "  |               ^^^ method not found in `LocalKey<i32>`\n  |\n  = note: the method \
             was found for `LocalKey<Cell<T>>`\n"
        ),
        "{report}"
    );
    let report = rendered(&program("i32 = 1", "set(2)"));
    assert!(
        report.ends_with(
            "  = note: the method was found for\n          - `LocalKey<Cell<T>>`\n          - \
             `LocalKey<RefCell<T>>`\n"
        ),
        "{report}"
    );
    let report = rendered(&program("Cell<i32> = Cell::new(1)", "with_borrow(|_| ())"));
    assert!(
        report.starts_with(
            "error[E0599]: no method named `with_borrow` found for struct `LocalKey<Cell<i32>>` \
             in the current scope\n"
        ),
        "{report}"
    );
    assert!(
        report.ends_with(
            "  |               ^^^^^^^^^^^\n  |\n  = help: there is a method `borrow` with a \
             similar name, but with different arguments\n"
        ),
        "{report}"
    );
    let report = rendered(&program("i32 = 1", "borow()"));
    assert!(
        report.starts_with(
            "error[E0599]: no method named `borow` found for struct `LocalKey<T>` in the current \
             scope\n"
        ),
        "{report}"
    );
    assert!(
        report.ends_with(
            "help: there is a method `borrow` with a similar name\n  |\n7 |     let _ = \
             N.borrow();\n  |                  +\n"
        ),
        "{report}"
    );
    for (inner, call, trait_name, item) in [
        (
            "Cell<S> = Cell::new(S)",
            "take()",
            "Default",
            "LocalKey::<Cell<T>>::take",
        ),
        (
            "RefCell<S> = RefCell::new(S)",
            "take()",
            "Default",
            "LocalKey::<RefCell<T>>::take",
        ),
        (
            "Cell<String> = Cell::new(String::new())",
            "get()",
            "Copy",
            "LocalKey::<Cell<T>>::get",
        ),
    ] {
        let report = rendered(&program(inner, call));
        let owner = if trait_name == "Copy" { "String" } else { "S" };
        let marks = "^".repeat(call.len() - 2);
        assert!(
            report.starts_with(&format!(
                "error[E0277]: the trait bound `{owner}: {trait_name}` is not satisfied\n --> \
                 main.rs:7:15\n  |\n7 |     let _ = N.{call};\n  |               {marks} the \
                 trait `{trait_name}` is not implemented for `{owner}`\n  |\n  = note: required \
                 by a bound in `{item}`\n"
            )),
            "{report}"
        );
    }
}
