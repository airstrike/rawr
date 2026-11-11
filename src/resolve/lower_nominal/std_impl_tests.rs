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

/// An `Option<PathBuf>`'s `as_deref` is an `Option<&Path>`, by std's
/// `impl Deref for PathBuf`, and `Path::extension` and `OsStr::to_str` go
/// on as function values, as iced's editor picks its highlighting (#1705).
#[test]
fn language_as_deref_of_an_optional_path_buf_lends_a_path() {
    let source = r#"
use std::ffi;
use std::path::{Path, PathBuf};

fn main() {
    let file: Option<PathBuf> = Some(PathBuf::from("src/main.rs"));
    let none: Option<PathBuf> = None;
    let path: Option<&Path> = file.as_deref();
    println!("{:?}", path);
    let kind = file
        .as_deref()
        .and_then(Path::extension)
        .and_then(ffi::OsStr::to_str)
        .unwrap_or("txt");
    let other = none.as_deref().and_then(Path::extension).and_then(ffi::OsStr::to_str);
    println!("{kind} {:?}", other);
}
main();
"#;
    assert_eq!(printed(source), "Some(\"src/main.rs\")\nrs None\n");
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
    // `Option<T>` has one impl, which takes its own type: rustc checks
    // `Some`'s argument against it.
    let report = refused(
        r#"
fn main() {
    let a: Option<String> = None;
    let _ = a == Some("x");
}
"#,
    );
    assert!(
        report.contains("error[E0308]: mismatched types")
            && report.contains("expected `String`, found `&str`"),
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

#[test]
fn language_a_vec_of_a_type_without_default_is_default() {
    // `Vec<T>: Default` and `VecDeque<T>: Default` hold for any `T`, so
    // `mem::take` empties a `Vec` of a struct that is not `Default`, as
    // iced's todos takes its tasks (#1711). `[T; 1]` asks it of `T`.
    let source = r#"
use std::collections::VecDeque;

#[derive(Debug)]
struct Task {
    name: String,
}

fn main() {
    let mut tasks = vec![Task { name: String::from("write") }];
    let taken = std::mem::take(&mut tasks);
    println!("{} {:?}", tasks.len(), taken);
    let mut queue = VecDeque::from(vec![Task { name: String::from("read") }]);
    let drained = std::mem::take(&mut queue);
    println!("{} {}", queue.len(), drained.len());
}
main();
"#;
    assert_eq!(printed(source), "0 [Task { name: \"write\" }]\n0 1\n");
    let report = refused(
        "struct Task;\nfn main() {\n    let mut one = [Task];\n    \
         let _ = std::mem::take(&mut one);\n}\nmain();\n",
    );
    assert!(report.contains("Default"), "{report}");
}

#[test]
fn language_try_into_takes_its_error_type_from_the_impl_its_target_selects() {
    // `let c: Result<Color, _> = x.try_into();` is a `Result<Color,
    // IntoColorError>` from that statement on, as rustc infers it once
    // the target selects the `TryFrom` impl (#927).
    let source = r#"
#[derive(Debug, PartialEq)]
struct Color {
    red: u8,
    green: u8,
    blue: u8,
}

#[derive(Debug, PartialEq)]
enum IntoColorError {
    BadLen,
    IntConversion,
}

impl TryFrom<(i16, i16, i16)> for Color {
    type Error = IntoColorError;

    fn try_from(tuple: (i16, i16, i16)) -> Result<Self, Self::Error> {
        let (Ok(red), Ok(green), Ok(blue)) = (
            u8::try_from(tuple.0),
            u8::try_from(tuple.1),
            u8::try_from(tuple.2),
        ) else {
            return Err(IntoColorError::IntConversion);
        };
        Ok(Self { red, green, blue })
    }
}

impl TryFrom<[i16; 3]> for Color {
    type Error = IntoColorError;

    fn try_from(arr: [i16; 3]) -> Result<Self, Self::Error> {
        Self::try_from((arr[0], arr[1], arr[2]))
    }
}

impl TryFrom<&[i16]> for Color {
    type Error = IntoColorError;

    fn try_from(slice: &[i16]) -> Result<Self, Self::Error> {
        if let &[red, green, blue] = slice {
            Self::try_from((red, green, blue))
        } else {
            Err(IntoColorError::BadLen)
        }
    }
}

fn main() {
    let c2: Result<Color, _> = [183, 65, 14].try_into();
    println!("{c2:?}");
    let v = vec![183, 65, 14];
    let c4: Result<Color, _> = (&v[..]).try_into();
    println!("{c4:?}");
    let c5: Result<Color, _> = (183, 65, 1000).try_into();
    println!("{c5:?}");
    let short = [1, 2];
    let c6: Result<Color, _> = (&short[..]).try_into();
    println!("{:?}", c6);
    // A literal's `{integer}` inside the argument selects the one impl
    // it fits, `&[i16]`'s, not the array's.
    let long = vec![0, 0, 0, 0];
    println!("{:?} {:?}", Color::try_from(&long[..]), Color::try_from(&v[..2]));
}
main();
"#;
    assert_eq!(
        printed(source),
        "Ok(Color { red: 183, green: 65, blue: 14 })\nOk(Color { red: 183, green: 65, blue: 14 })\n\
         Err(IntConversion)\nErr(BadLen)\nErr(BadLen) Err(BadLen)\n"
    );
}

#[test]
fn language_a_pointer_is_display_where_its_value_is() {
    // std's `impl<T: Display> Display for Rc<T>` and its kin meet a
    // `T: Display` bound, and a generic `{}` writes what they point to
    // (#928).
    let source = r#"
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

struct Point {
    x: i32,
    y: i32,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

struct Holder<T> {
    v: T,
}

impl<T: fmt::Display> Holder<T> {
    fn print(&self) {
        println!("[{}]", self.v);
    }
}

fn show<T: fmt::Display>(value: T) -> String {
    format!("<{value}>")
}

fn main() {
    Holder { v: Rc::new(3) }.print();
    Holder { v: Arc::new(String::from("arc")) }.print();
    Holder { v: Box::new(1.5) }.print();
    let shared = Rc::new(Point { x: 1, y: 2 });
    println!("{} {}", show(shared.clone()), show(&shared));
    println!("{} {}", show(Arc::new(Rc::new(5u8))), show(Rc::new('x')));
    println!("{} {}", shared, Rc::strong_count(&shared));
}
main();
"#;
    assert_eq!(
        printed(source),
        "[3]\n[arc]\n[1.5]\n<(1, 2)> <(1, 2)>\n<5> <x>\n(1, 2) 1\n"
    );
    // rustc names the type the pointer's impl needed `Display` of.
    let report = refused(
        "use std::rc::Rc;\nfn show<T: std::fmt::Display>(v: T) -> String { format!(\"{v}\") }\n\
         fn main() { show(Rc::new(vec![1])); }\nmain();\n",
    );
    assert!(
        report.starts_with("error[E0277]: `Vec<{integer}>` doesn't implement `std::fmt::Display`"),
        "{report}"
    );
}

/// `AsRef` and `AsMut` bounds of std's types hold by std's impls, and
/// `as_ref` and `as_mut` through them run: `as_mut` lends the receiver's
/// own place (rustlings' as_ref_mut).
#[test]
fn language_std_as_ref_and_as_mut_bounds_hold_for_std_types() {
    let source = r#"
fn bytes<T: AsRef<str>>(arg: T) -> usize {
    arg.as_ref().len()
}
fn chars<T: AsRef<str>>(arg: T) -> usize {
    arg.as_ref().chars().count()
}
fn first<T: AsRef<[i32]>>(arg: T) -> i32 {
    arg.as_ref()[0]
}
fn square<T: AsMut<u32>>(arg: &mut T) {
    let arg = arg.as_mut();
    *arg *= *arg;
}
fn push<T: std::borrow::BorrowMut<Vec<i32>>>(arg: &mut T) {
    arg.borrow_mut().push(4);
}
fn zero<T: AsMut<[i32]>>(arg: &mut T) {
    arg.as_mut()[0] = 0;
}
fn main() {
    let s = String::from("Café");
    println!("{} {}", bytes(s.clone()), chars(s));
    println!("{} {}", bytes("Café"), chars("Café"));
    println!("{}", first(vec![7, 8]));
    let mut n: Box<u32> = Box::new(3);
    square(&mut n);
    println!("{}", *n);
    let mut v = vec![1, 2];
    push(&mut v);
    zero(&mut v);
    println!("{v:?}");
}
main();
"#;
    assert_eq!(printed(source), "5 4\n5 4\n7\n9\n[0, 2, 4]\n");
    // A bound std's impls don't cover is rustc's E0277, with the trait's
    // arguments.
    let report = refused(
        "fn bytes<T: AsRef<str>>(arg: T) -> usize { arg.as_ref().len() }\n\
         fn main() { bytes(1u8); }\nmain();\n",
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `u8: AsRef<str>` is not satisfied"),
        "{report}"
    );
}

/// A `match` or `if` whose arms give `&str` and `&String` is a `&str`: rustc
/// deref-coerces the `&String` arm to the type of the others, before it or
/// after it, as pokedex's `title` matches `"Loading"` and
/// `&pokemon.name` (#1018).
#[test]
fn language_deref_coerces_an_arm_to_the_type_another_arm_gives() {
    let source = r#"
enum P {
    A,
    B { name: String },
}

fn title(p: &P) -> String {
    let subtitle = match p {
        P::A => "Loading",
        P::B { name } => &name,
    };
    format!("{subtitle} - x")
}

fn main() {
    let name = String::from("y");
    let n = 1;
    let s = if n == 0 { "a" } else { &name };
    let t = match n {
        0 => &name,
        _ => "b",
    };
    println!("{} {} {s} {t}", title(&P::B { name: String::from("z") }), title(&P::A));
}
main();
"#;
    assert_eq!(printed(source), "z - x Loading - x y b\n");
    // Arms no coercion joins: rustc's E0308.
    let report =
        refused("fn main() { let n = 1; let t = match n { 0 => 1, _ => \"b\" }; }\nmain();\n");
    assert!(report.starts_with("error[E0308]"), "{report}");
}

/// A native value's `as_ref()` and `borrow()` lend the view the use
/// expects, from std's `AsRef` and `Borrow` impls for its type: a `let`
/// annotation, a parameter or a return fixes the target, as rustc infers
/// the trait's argument (#1632).
#[test]
fn language_std_as_ref_and_borrow_of_a_native_value_lend_the_expected_view() {
    let source = r#"
use std::borrow::Borrow;

fn count(text: &str) -> usize {
    text.len()
}

fn first(bytes: &[u8]) -> u8 {
    bytes[0]
}

fn view(s: &String) -> &str {
    s.as_ref()
}

fn main() {
    let s = String::from("hey");
    let literal = "abc";
    let v = vec![1, 2, 3];
    let a = [4, 5];
    let r: &str = s.as_ref();
    let bytes: &[u8] = s.as_ref();
    let borrowed: &str = s.borrow();
    let whole: &Vec<i32> = v.borrow();
    let part: &[i32] = v.borrow();
    let same: &String = s.borrow();
    let held: &[i32] = a.as_ref();
    let n = 7;
    let m: &i32 = n.borrow();
    println!("{r} {bytes:?} {borrowed}");
    println!("{} {} {}", count(s.as_ref()), first(literal.as_ref()), view(&s));
    println!("{whole:?} {part:?} {same} {held:?} {m}");
    let t: &str = (&s).as_ref();
    println!("{t}");
}
main();
"#;
    assert_eq!(
        printed(source),
        "hey [104, 101, 121] hey\n3 97 hey\n[1, 2, 3] [1, 2, 3] hey [4, 5] 7\nhey\n"
    );
    // No impl lends the target: rustc's E0277 at the method.
    let report = refused(
        "fn main() { let s = String::from(\"x\"); let r: &i32 = s.as_ref(); println!(\"{r}\"); }\n\
         main();\n",
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `String: AsRef<i32>` is not satisfied"),
        "{report}"
    );
    // No use fixes it: rustc's E0282.
    let report = refused(
        "fn main() { let s = String::from(\"x\"); let r = s.as_ref(); println!(\"{}\", r.len()); }\n\
         main();\n",
    );
    assert!(
        report.starts_with("error[E0282]: type annotations needed"),
        "{report}"
    );
    // The view borrows the value: rustc's E0505.
    let report = refused(
        "fn main() { let s = String::from(\"x\"); let r: &str = s.as_ref(); drop(s); println!(\"{r}\"); }\n\
         main();\n",
    );
    assert!(
        report.starts_with("error[E0505]: cannot move out of `s` because it is borrowed"),
        "{report}"
    );
}

/// A text's `as_ref()` lends a `&Path` or `&OsStr`, std's facade-held
/// types, as rustc's `impl AsRef<Path> for String` and `str` do, and a
/// type std's impls don't lend one to is rustc's E0277, naming `Path`
/// (#1675).
#[test]
fn language_std_as_ref_of_a_text_lends_a_path_or_os_str() {
    let source = r#"
use std::ffi::OsStr;
use std::path::Path;

fn stem(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().to_string()
}

fn main() {
    let s = String::from("dir/a.txt");
    let p: &Path = s.as_ref();
    let o: &OsStr = s.as_ref();
    let q: &Path = "b.rs".as_ref();
    println!("{} {:?} {:?}", p.display(), o, q.extension());
    println!("{}", stem(s.as_ref()));
}
main();
"#;
    assert_eq!(printed(source), "dir/a.txt \"dir/a.txt\" Some(\"rs\")\na\n");
    let report = refused(
        "use std::path::Path;\n\
         fn main() { let v = vec![1u8]; let p: &Path = v.as_ref(); println!(\"{}\", p.display()); }\n\
         main();\n",
    );
    assert!(
        report.starts_with("error[E0277]: the trait bound `Vec<u8>: AsRef<Path>` is not satisfied"),
        "{report}"
    );
}

/// A generic `T: AsRef<[u8]>` given a `String` lends its bytes, as std's
/// `impl AsRef<[u8]> for String` does, and `T: AsRef<str>` its text
/// (#1676).
#[test]
fn language_std_as_ref_of_a_bounded_parameter_lends_the_bound_view() {
    let source = r#"
fn first<T: AsRef<[u8]>>(t: T) -> u8 {
    t.as_ref()[0]
}

fn size<T: AsRef<str>>(t: T) -> usize {
    t.as_ref().len()
}

fn main() {
    println!("{} {}", first(String::from("abc")), first("xyz"));
    println!("{} {}", first(vec![7u8, 8]), size(String::from("four")));
}
main();
"#;
    assert_eq!(printed(source), "97 120\n7 4\n");
}
