//! std's impls on rawr's native types come from the std table (D711 step
//! 3): one `Deref` rule for coercion, the method probe and `as_deref`,
//! `PartialEq` and `PartialOrd` from std's impls, and `From`/`Into` among
//! std's types. Each program's expected output is what rustc's build of it
//! prints, and each refused one is refused by rustc with the same error.

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

/// The report `rawr` prints for `source`, which rustc refuses.
fn refused(source: &str) -> String {
    Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rustc refuses this program")
        .to_string()
}

#[test]
fn language_deref_coerces_and_probes_through_a_programs_impl() {
    let source = r#"
use std::ops::Deref;

struct Name(String);

impl Deref for Name {
    type Target = String;
    fn deref(&self) -> &String {
        &self.0
    }
}

struct Wrapper {
    inner: Vec<i64>,
    label: &'static str,
}

impl Deref for Wrapper {
    type Target = Vec<i64>;
    fn deref(&self) -> &Vec<i64> {
        &self.inner
    }
}

fn shout(text: &str) -> String {
    text.to_uppercase()
}

fn total(values: &[i64]) -> i64 {
    values.iter().sum()
}

fn main() {
    let name = Name(String::from("ferris"));
    println!("{} {}", name.len(), name.starts_with("fe"));
    println!("{}", shout(&name));
    let w = Wrapper { inner: vec![1, 2, 3], label: "w" };
    println!("{} {} {}", w.len(), w.label, total(&w));
    println!("{:?}", w.first());
    let boxed: Box<String> = Box::new(String::from("box"));
    println!("{}", shout(&boxed));
    let nested = &&String::from("deep");
    println!("{}", shout(nested));
}
main();
"#;
    assert_eq!(
        printed(source),
        "6 true\nFERRIS\n3 w 6\nSome(1)\nBOX\nDEEP\n"
    );
}

#[test]
fn language_as_deref_borrows_through_the_targets_deref() {
    let source = r#"
fn main() {
    let data: Option<Vec<u8>> = Some(vec![1, 2]);
    let view: Option<&[u8]> = data.as_deref();
    println!("{:?}", view);
    let text: Option<String> = Some(String::from("hi"));
    let borrowed: Option<&str> = text.as_deref();
    println!("{:?}", borrowed);
    let mut owned: Option<Vec<u8>> = Some(vec![3]);
    if let Some(bytes) = owned.as_deref_mut() {
        bytes[0] = 9;
    }
    println!("{:?}", owned);
}
main();
"#;
    assert_eq!(printed(source), "Some([1, 2])\nSome(\"hi\")\nSome([9])\n");
}

#[test]
fn language_sequences_text_maps_and_cells_compare_by_stds_impls() {
    let source = r#"
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

fn main() {
    let a = [1, 2];
    let v = vec![1, 2];
    let s: &[i32] = &v;
    println!("{} {} {}", v == a, s == v, v == s);
    println!("{} {}", v < vec![1, 3], a < [1, 1]);
    let words = vec![String::from("a")];
    println!("{} {}", words == ["a"], &words[..] == ["a"]);
    let name = String::from("b");
    println!("{} {} {}", name.as_str() < "c", name > String::from("a"), "a" < "b");
    let mut m = BTreeMap::new();
    m.insert(1, "x");
    let mut n = m.clone();
    n.insert(2, "y");
    println!("{} {}", m < n, m == n);
    let set: BTreeSet<i32> = [1, 2].into_iter().collect();
    let other: BTreeSet<i32> = [1, 3].into_iter().collect();
    println!("{}", set < other);
    let ok: Result<i32, String> = Ok(1);
    let err: Result<i32, String> = Err(String::from("e"));
    println!("{} {}", ok < err, ok < Ok(2));
    println!("{} {}", Cell::new(1) < Cell::new(2), RefCell::new(3) > RefCell::new(2));
    println!("{}", (1, "a") < (1, "b"));
}
main();
"#;
    assert_eq!(
        printed(source),
        "true true true\ntrue false\ntrue true\ntrue true true\ntrue false\ntrue\ntrue true\ntrue true\ntrue\n"
    );
}

#[test]
fn language_from_and_into_between_std_types() {
    let source = r#"
fn widen<T: Into<i64>>(x: T) -> i64 {
    x.into()
}

fn text<T: Into<String>>(x: T) -> String {
    x.into()
}

fn listed<T: Into<Vec<i32>>>(x: T) -> Vec<i32> {
    x.into()
}

fn boxed<T: Into<Box<i32>>>(x: T) -> Box<i32> {
    x.into()
}

fn main() {
    println!("{} {} {}", widen(7u8), widen(true), widen(-3i32));
    let owned = String::from("own");
    println!("{} {} {}", text("lit"), text(&owned), text('c'));
    println!("{:?} {}", listed([1, 2]), boxed(5));
    let s: String = "into".into();
    let n: i64 = 9u16.into();
    let w = i64::from(2u32);
    let v: Vec<i32> = [3, 4].into();
    let o: Option<i32> = 6.into();
    let f = f64::from(1.5f32);
    println!("{} {} {} {:?} {:?} {}", s, n, w, v, o, f);
}
main();
"#;
    assert_eq!(
        printed(source),
        "7 1 -3\nlit own c\n[1, 2] 5\ninto 9 2 [3, 4] Some(6) 1.5\n"
    );
}

#[test]
fn language_a_missing_std_impl_is_rustcs_error() {
    // `Vec<String>: PartialEq<[U; N]>` needs `String: PartialEq<U>`.
    let report = refused(
        r#"
fn main() {
    let words = vec![String::from("a")];
    let _ = words == [1];
}
"#,
    );
    assert!(
        report.starts_with("error[E0277]: can't compare `String` with `{integer}`\n"),
        "{report}"
    );
    assert!(
        report
            .contains("note: required for `Vec<String>` to implement `PartialEq<[{integer}; 1]>`"),
        "{report}"
    );
    // A scalar compares with its own type only.
    let report = refused(
        r#"
fn main() {
    let x: u16 = 4;
    let _ = x == "4";
}
"#,
    );
    assert!(report.contains("expected `u16`, found `&str`"), "{report}");
    // `Option<T>` has one impl, which takes its own type.
    let report = refused(
        r#"
fn main() {
    let a: Option<String> = None;
    let _ = a == Some("x");
}
"#,
    );
    assert!(
        report.contains("expected `Option<String>`, found `Option<&str>`"),
        "{report}"
    );
    // std has no `impl PartialOrd for HashMap<K, V>`.
    let report = refused(
        r#"
use std::collections::HashMap;
fn main() {
    let m: HashMap<i32, i32> = HashMap::new();
    let _ = m < m.clone();
}
"#,
    );
    assert!(
        report.contains("binary operation `<` cannot be applied to type `HashMap<i32, i32>`"),
        "{report}"
    );
    // Nor `impl From<i64> for u8`.
    let report = refused(
        r#"
fn widen<T: Into<u8>>(x: T) -> u8 {
    x.into()
}
fn main() {
    let _ = widen(1i64);
}
"#,
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `u8: From<i64>` is not satisfied\n"),
        "{report}"
    );
}

/// A method found through a program's `Deref` that takes `&mut self`
/// takes the steps through its `DerefMut`, as rustc's probe does, through
/// two impls and through a `&mut` parameter; `as_deref` borrows through a
/// program's `Deref`.
#[test]
fn language_a_mut_self_method_probes_through_a_programs_deref_mut() {
    let source = r#"
use std::ops::{Deref, DerefMut};

struct Stack {
    items: Vec<i64>,
}

impl Deref for Stack {
    type Target = Vec<i64>;
    fn deref(&self) -> &Vec<i64> {
        &self.items
    }
}

impl DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut Vec<i64> {
        &mut self.items
    }
}

struct Named {
    stack: Stack,
    name: &'static str,
}

impl Deref for Named {
    type Target = Stack;
    fn deref(&self) -> &Stack {
        &self.stack
    }
}

impl DerefMut for Named {
    fn deref_mut(&mut self) -> &mut Stack {
        &mut self.stack
    }
}

fn fill(stack: &mut Stack) {
    stack.push(5);
    stack.push(6);
}

fn main() {
    let mut s = Stack { items: vec![1] };
    s.push(2);
    fill(&mut s);
    s.retain(|x| *x != 5);
    println!("{:?} {}", s.items, s.len());
    let mut n = Named { stack: Stack { items: Vec::new() }, name: "n" };
    n.push(7);
    n.stack.push(8);
    println!("{} {:?} {:?}", n.name, n.items, n.last());
    let some = Some(Stack { items: vec![3, 4] });
    let view: Option<&Vec<i64>> = some.as_deref();
    println!("{:?} {:?}", view.map(|v| v.len()), some.as_deref().map(|v| v[0]));
}
main();
"#;
    assert_eq!(
        printed(source),
        "[1, 2, 6] 3\nn [7, 8] Some(8)\nSome(2) Some(3)\n"
    );
}

/// A `&mut self` method through a `Deref` with no `DerefMut` is rustc's
/// E0596.
#[test]
fn language_a_mut_self_method_without_deref_mut_is_refused() {
    let report = refused(
        r#"
use std::ops::Deref;

struct Ro {
    items: Vec<i64>,
}

impl Deref for Ro {
    type Target = Vec<i64>;
    fn deref(&self) -> &Vec<i64> {
        &self.items
    }
}

fn main() {
    let mut r = Ro { items: vec![1] };
    r.push(3);
}
"#,
    );
    assert!(
        report.contains("cannot borrow data in dereference of `Ro` as mutable"),
        "{report}"
    );
    assert!(report.contains("E0596"), "{report}");
}

#[test]
fn language_a_derive_holds_where_std_implements_the_trait_for_a_field() {
    // std's error types and `Duration` have the traits std's impls give
    // them, so a type holding them derives those (#904).
    let source = r#"
use std::num::{ParseFloatError, ParseIntError, TryFromIntError};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
enum ParseError {
    Int(ParseIntError),
    Float(ParseFloatError),
    Range(TryFromIntError),
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
struct Timeout {
    after: Duration,
}

fn main() {
    let int = ParseError::Int("x".parse::<i32>().unwrap_err());
    let float = ParseError::Float("y".parse::<f64>().unwrap_err());
    let range = ParseError::Range(u8::try_from(300_i32).unwrap_err());
    println!("{int:?}");
    println!("{float:?}");
    println!("{range:?}");
    println!("{:?} {}", ParseError::Empty, int == int.clone());
    println!("{:?}", Timeout::default());
}
main();
"#;
    assert_eq!(
        printed(source),
        "Int(ParseIntError { kind: InvalidDigit })\nFloat(ParseFloatError { kind: Invalid })\n\
         Range(TryFromIntError(PosOverflow))\nEmpty true\nTimeout { after: 0ns }\n"
    );
    // std's `ParseIntError` is not `Default` or `Copy`: rustc's E0277.
    let report = refused(
        "#[derive(Default)]\nstruct S { e: std::num::ParseIntError }\nfn main() {}\nmain();\n",
    );
    assert!(report.contains("Default"), "{report}");
}
