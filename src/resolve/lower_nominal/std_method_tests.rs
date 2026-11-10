//! std's inherent methods come from the std table (D711 step 4): which a
//! native type has, the probe step each is found at, how it takes its
//! receiver and what a call keeps. Each program's expected output is what
//! rustc's build of it prints, and each refused one is refused by rustc
//! with the same error, except the not-yet error, which rustc never gives.

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
fn language_std_inherent_methods_of_each_native_type() {
    let source = r#"
use std::collections::{BTreeMap, HashSet};

fn main() {
    let some: Option<i32> = Some(4);
    let none: Option<i32> = None;
    println!(
        "{} {} {:?} {} {:?} {}",
        some.is_some(),
        none.is_none(),
        some.map(|x| x * 2),
        none.unwrap_or(7),
        some.filter(|x| *x > 9),
        some.map_or(0, |x| x + 1)
    );
    let mut slot = Some(String::from("a"));
    let taken = slot.take();
    println!("{:?} {:?}", taken, slot);
    let ok: Result<i32, String> = Ok(3);
    let err: Result<i32, String> = Err(String::from("bad"));
    println!(
        "{} {} {:?} {:?} {} {:?}",
        ok.is_ok(),
        err.is_err(),
        ok.as_ref().map(|x| x + 1),
        err.as_ref().map_err(|e| e.len()),
        err.clone().unwrap_or_default(),
        ok.clone().ok()
    );
    let c = 'q';
    println!(
        "{} {} {:?} {} {}",
        c.is_alphabetic(),
        c.to_ascii_uppercase(),
        '7'.to_digit(10),
        c.len_utf8(),
        c.is_whitespace()
    );
    let f = 2.5f64;
    println!("{} {} {} {} {}", f.floor(), f.powi(2), f.sqrt() > 1.5, f.max(3.0), (-f).abs());
    let n = -12i64;
    println!("{} {} {} {} {:?}", n.abs(), n.pow(2), n.signum(), n.rem_euclid(5), 200u8.checked_add(100));
    let text = String::from("  Hello, World  ");
    let trimmed = text.trim();
    println!(
        "{} {} {} {} {:?} {}",
        trimmed,
        trimmed.len(),
        trimmed.to_uppercase(),
        trimmed.starts_with("Hell"),
        trimmed.split(", ").collect::<Vec<_>>(),
        trimmed.replace("World", "rawr")
    );
    let mut owned = String::from("ab");
    owned.push_str("cd");
    owned.push('e');
    println!("{} {:?} {}", owned, owned.find('c'), owned.chars().rev().collect::<String>());
    let mut v = vec![3, 1, 2];
    v.push(5);
    v.sort();
    println!(
        "{:?} {} {:?} {:?} {} {:?}",
        v,
        v.len(),
        v.first(),
        v.iter().map(|x| x * 10).collect::<Vec<_>>(),
        v.contains(&2),
        v.get(1)
    );
    v.retain(|x| x % 2 == 1);
    v.reverse();
    let last = v.pop();
    println!("{:?} {:?}", v, last);
    let mut map = BTreeMap::new();
    map.insert("b", 2);
    map.insert("a", 1);
    let removed = map.remove("b");
    println!(
        "{:?} {} {:?} {:?}",
        map.get("a"),
        map.contains_key("z"),
        map.keys().collect::<Vec<_>>(),
        removed
    );
    let set: HashSet<i32> = [1, 2, 3].into_iter().collect();
    println!("{} {}", set.contains(&2), set.len());
}
main();
"#;
    assert_eq!(
        printed(source),
        "true true Some(8) 7 None 5\n\
         Some(\"a\") None\n\
         true true Ok(4) Err(3) 0 Some(3)\n\
         true Q Some(7) 1 false\n\
         2 6.25 true 3 2.5\n\
         12 144 -1 3 None\n\
         Hello, World 12 HELLO, WORLD true [\"Hello\", \"World\"] Hello, rawr\n\
         abcde Some(2) edcba\n\
         [1, 2, 3, 5] 4 Some(1) [10, 20, 30, 50] true Some(2)\n\
         [5, 3] Some(1)\n\
         Some(1) false [\"a\"] Some(2)\n\
         true 3\n"
    );
}

#[test]
fn language_a_trait_method_comes_before_an_inherent_one_a_deref_step_away() {
    // `sort` is `[T]`'s, a deref step past `&Vec<i64>`, where `Sorted::sort`
    // is found; `contains` is `str`'s, past `&String`. A `&str` finds
    // `str::contains` at its first step.
    let source = r#"
trait Sorted {
    fn sort(&self) -> i64;
}

impl Sorted for Vec<i64> {
    fn sort(&self) -> i64 {
        99
    }
}

trait Has {
    fn contains(&self, c: char) -> bool;
}

impl Has for String {
    fn contains(&self, _c: char) -> bool {
        true
    }
}

fn main() {
    let v: Vec<i64> = vec![3, 1];
    let s = String::from("abc");
    println!("{} {:?} {} {}", v.sort(), v, s.contains('z'), s.as_str().contains('z'));
}
main();
"#;
    assert_eq!(printed(source), "99 [3, 1] true false\n");
}

#[test]
fn language_receiver_modes_are_stds() {
    // `String::into_bytes` takes `self`: the string is moved.
    let report = refused(
        r#"
fn main() {
    let s = String::from("hi");
    let b = s.into_bytes();
    println!("{:?} {}", b, s);
}
main();
"#,
    );
    assert!(report.starts_with("error[E0382]"), "{report}");
    // `sort`, `take` and `push_str` take `&mut self`.
    for (source, name) in [
        (
            "let v = vec![3, 1, 2]; v.sort(); println!(\"{:?}\", v);",
            "v",
        ),
        (
            "let o = Some(1); let t = o.take(); println!(\"{:?}\", t);",
            "o",
        ),
        (
            "let s = String::new(); s.push_str(\"a\"); println!(\"{}\", s);",
            "s",
        ),
    ] {
        let report = refused(&format!("fn main() {{ {source} }}\nmain();\n"));
        assert!(
            report.starts_with(&format!(
                "error[E0596]: cannot borrow `{name}` as mutable, as it is not declared as \
                 mutable\n"
            )),
            "{report}"
        );
    }
}

#[test]
fn language_a_std_method_rawr_does_not_run_is_the_not_yet_error() {
    // rustc runs this; `Vec::dedup_by_key` is std's, which rawr has no
    // operation for yet (D712.1), never an unknown member.
    let report = refused(
        r#"
fn main() {
    let mut v = vec![1, 1, 2, 3, 3];
    v.dedup_by_key(|x: &mut i64| *x / 2);
    println!("{:?}", v);
}
main();
"#,
    );
    assert!(
        report.contains("rawr doesn't run `Vec::dedup_by_key` yet"),
        "{report}"
    );
    // A method std doesn't declare is rustc's unknown member.
    let report = refused("fn main() { let v = vec![1]; v.dedup_by_kye(); }\nmain();\n");
    assert!(!report.contains("doesn't run"), "{report}");
}

#[test]
fn language_an_iterators_methods_are_iterators() {
    // Iterator's own, `DoubleEndedIterator`'s and `Peekable`'s, each read
    // from std's declaration.
    let source = r#"
fn main() {
    let v = vec![3, 1, 4, 1, 5];
    let mut it = v.iter().peekable();
    let first = it.peek().copied();
    let rest: Vec<i64> = it.map(|x| x * 2).collect();
    let pairs: Vec<(usize, &i64)> = v.iter().enumerate().filter(|&(_, x)| *x > 1).collect();
    let both: Vec<&i64> = v.iter().chain(v.iter().rev()).take(6).collect();
    println!("{:?} {:?} {:?} {:?}", first, rest, pairs, both);
}
main();
"#;
    assert_eq!(
        printed(source),
        "Some(3) [6, 2, 8, 2, 10] [(0, 3), (2, 4), (4, 5)] [3, 1, 4, 1, 5, 5]\n"
    );
    // What an adapter's items borrow, as its declaration says: rustc's
    // E0502.
    let report = refused(
        r#"
fn main() {
    let mut v = vec![1, 2];
    let w = vec![3];
    let items: Vec<&i64> = w.iter().chain(v.iter()).collect();
    v.push(4);
    println!("{:?}", items);
}
main();
"#,
    );
    assert!(report.starts_with("error[E0502]"), "{report}");
    // `next` takes `&mut self`.
    let report = refused("fn main() { let v = vec![1]; let it = v.iter(); it.next(); }\nmain();\n");
    assert!(report.starts_with("error[E0596]"), "{report}");
    // A method std declares that rawr doesn't run yet, never an unknown
    // member.
    for (call, path) in [
        ("rposition(|x: &i64| *x > 1)", "Iterator::rposition"),
        ("next_back()", "DoubleEndedIterator::next_back"),
    ] {
        let report = refused(&format!(
            "fn main() {{ let v = vec![1, 2]; let mut it = v.iter(); let _ = it.{call}; }}\nmain();\n"
        ));
        assert!(
            report.contains(&format!("rawr doesn't run `{path}` yet")),
            "{report}"
        );
    }
    let report = refused("fn main() { let v = vec![1]; let _ = v.iter().mapp(|x| x); }\nmain();\n");
    assert!(!report.contains("doesn't run"), "{report}");
}

/// `o.as_mut().unwrap()` is the place of `o`'s payload, which `*..` and
/// a `&mut self` call change, on a local, a field and an element (#1001).
#[test]
fn an_unwrapped_as_mut_is_the_payload_s_place() {
    let source = r#"
struct S {
    n: Option<i32>,
    v: Option<Vec<i32>>,
}
fn main() {
    let mut o: Option<i32> = Some(4);
    *o.as_mut().unwrap() += 1;
    *o.as_mut().expect("some") *= 10;
    let mut s = S { n: Some(1), v: Some(vec![]) };
    *s.n.as_mut().unwrap() += 1;
    s.v.as_mut().unwrap().push(7);
    let mut all = vec![Some(1), Some(2)];
    *all[1].as_mut().unwrap() = 20;
    let r = o.as_mut().unwrap();
    *r += 1;
    println!("{:?} {:?} {:?} {:?}", o, s.n, s.v, all);
}
main();
"#;
    assert_eq!(
        printed(source),
        "Some(51) Some(2) Some([7]) [Some(1), Some(20)]\n"
    );
}

/// A `Mutex`'s `get_mut()` and `into_inner()`, unwrapped as `lock()` is,
/// reach its value (#949).
#[test]
fn a_mutex_s_get_mut_and_into_inner_reach_its_value() {
    let source = r#"
use std::sync::Mutex;
fn main() {
    let mut m = Mutex::new(vec![1]);
    m.get_mut().unwrap().push(2);
    *m.lock().unwrap() = vec![3, 4, 5];
    m.get_mut().expect("not poisoned").push(6);
    println!("{:?}", m.into_inner().unwrap());
}
main();
"#;
    assert_eq!(printed(source), "[3, 4, 5, 6]\n");
}

#[test]
fn language_clone_from_and_try_into_go_through_method_lookup() {
    // `clone_from` is `Clone`'s provided method, on a std type and a
    // program's (#759); `try_into` takes the target's `TryFrom` (#757).
    let source = r#"
use std::convert::TryFrom;

#[derive(Debug, Clone)]
struct Even(u32);

impl TryFrom<u32> for Even {
    type Error = String;
    fn try_from(n: u32) -> Result<Self, Self::Error> {
        if n % 2 == 0 { Ok(Even(n)) } else { Err(format!("{n} is odd")) }
    }
}

fn main() {
    let mut a = String::from("old");
    let b = String::from("new");
    a.clone_from(&b);
    let mut v = vec![1, 2];
    Clone::clone_from(&mut v, &vec![7, 8, 9]);
    let mut e = Even(0);
    e.clone_from(&Even(4));
    println!("{a} {b} {:?} {:?}", v, e);
    let ok: Result<Even, String> = 8u32.try_into();
    let bad: Result<Even, String> = 7u32.try_into();
    println!("{:?} {:?}", ok, bad);
}
main();
"#;
    assert_eq!(
        printed(source),
        "new new [7, 8, 9] Even(4)\nOk(Even(8)) Err(\"7 is odd\")\n"
    );
}

#[test]
fn language_a_vec_derefs_to_its_slice_which_is_its_own_type() {
    // `&Vec<T>` and `&[T; N]` coerce to `&[T]` (#769); a range of a `Vec`
    // is a `[T]`, which a slice pattern matches.
    let source = r#"
fn total(xs: &[i64]) -> i64 {
    xs.iter().sum()
}

fn head(xs: &[i64]) -> &[i64] {
    &xs[..2]
}

fn main() {
    let v = vec![3, 1, 2];
    let a = [4, 5, 6];
    let s: &[i64] = &v;
    println!("{} {} {:?} {:?} {}", total(&v), total(&a), head(&v), s.to_vec(), s.len());
    match &v[..] {
        [first, .., last] => println!("{first} {last}"),
        _ => {}
    }
    match v.as_slice() {
        [] => println!("empty"),
        [one, rest @ ..] => println!("{one} {:?}", rest),
    }
}
main();
"#;
    assert_eq!(printed(source), "6 15 [3, 1] [3, 1, 2] 3\n3 2\n3 [1, 2]\n");
}

#[test]
fn language_a_slice_has_only_the_slices_methods() {
    // rustc's E0599: `push` is `Vec<T>`'s, which a `&mut [T]` doesn't have.
    let report = refused(
        r#"
fn add(xs: &mut [i64]) {
    xs.push(1);
}
fn main() {
    let mut v = vec![1];
    add(&mut v);
}
main();
"#,
    );
    assert!(
        report.contains("no method named `push` found for `&mut [i64]`"),
        "{report}"
    );
    // A `Vec` is no slice to a pattern (E0529).
    let report = refused(
        r#"
fn main() {
    let v: Vec<i64> = vec![1, 2];
    match &v {
        [a, b] => println!("{a} {b}"),
        _ => {}
    }
}
main();
"#,
    );
    assert!(report.contains("E0529"), "{report}");
}

#[test]
fn language_a_vec_deque_has_its_own_methods() {
    let source = r#"
use std::collections::VecDeque;

fn main() {
    let mut d: VecDeque<i64> = VecDeque::from(vec![2, 3]);
    d.push_front(1);
    d.push_back(4);
    let front = d.front().copied();
    if let Some(back) = d.back_mut() {
        *back += 10;
    }
    println!("{:?} {:?} {} {:?}", d, front, d.len(), d.get(1));
    let v: Vec<i64> = d.iter().map(|x| x * 2).collect();
    let e: VecDeque<i64> = v.iter().copied().collect();
    println!("{:?} {:?} {}", v, e.back(), d == VecDeque::from([1, 2, 3, 14]));
}
main();
"#;
    assert_eq!(
        printed(source),
        "[1, 2, 3, 14] Some(1) 4 Some(2)\n[2, 4, 6, 28] Some(28) true\n"
    );
}

#[test]
fn language_vec_and_vec_deque_methods_are_each_their_own() {
    // rustc's E0599 for another sequence's method.
    let deque =
        "use std::collections::VecDeque;\nfn main() { let mut d: VecDeque<i64> = VecDeque::new(); ";
    for (source, method, owner) in [
        (
            "fn main() { let mut v: Vec<i64> = Vec::new(); v.push_back(1); }\nmain();\n"
                .to_string(),
            "push_back",
            "Vec<i64>",
        ),
        (
            format!("{deque}d.push(1); }}\nmain();\n"),
            "push",
            "VecDeque<i64>",
        ),
        (
            format!("{deque}d.sort(); }}\nmain();\n"),
            "sort",
            "VecDeque<i64>",
        ),
    ] {
        let report = refused(&source);
        assert!(
            report.contains(&format!("no method named `{method}` found for `{owner}`")),
            "{report}"
        );
    }
    // std's own `VecDeque` method rawr doesn't run yet (D712.1).
    let report = refused(&format!("{deque}d.binary_search(&1); }}\nmain();\n"));
    assert!(
        report.contains("rawr doesn't run `VecDeque::binary_search` yet"),
        "{report}"
    );
}

#[test]
fn language_a_closure_takes_its_parameter_types_from_std() {
    // Each closure's parameters have the types its std method's `Fn` bound
    // gives them (#795), with no annotation and no hand list.
    let source = r#"
use std::cmp::Ordering;

fn main() {
    let mut v = vec![3, 1, 2];
    v.sort_by_key(|x| -*x);
    v.retain(|x| *x != 2);
    let mut s = String::from("banana");
    s.retain(|c| c != 'a');
    let ok: Result<i64, String> = Err(String::from("no"));
    let order = Ordering::Equal.then_with(|| 2.cmp(&1));
    let picked: Vec<i64> = v.iter().filter(|x| **x > 1).map(|x| x * 10).collect();
    println!(
        "{:?} {} {:?} {} {:?} {:?}",
        v,
        s,
        ok.map_err(|e| e.len()),
        order == Ordering::Greater,
        true.then(|| 5),
        picked,
    );
}
main();
"#;
    assert_eq!(printed(source), "[3, 1] bnn Err(2) true Some(5) [30]\n");
    // A method rawr doesn't run takes its closure's types from std too, so
    // the not-yet error is what the program gets (#770).
    for (call, path) in [
        ("v.dedup_by_key(|x| *x / 2)", "Vec::dedup_by_key"),
        ("let _ = v.iter().rposition(|x| *x > 1)", "Iterator::rposition"),
        (
            "let _: Vec<i64> = v.iter().scan(0, |total, x| { *total += *x; Some(*total) }).collect()",
            "Iterator::scan",
        ),
        ("let _ = v.chunk_by(|a, b| *a <= *b)", "[T]::chunk_by"),
        (
            "let _ = v.first().map_or_else(|| 0, |x| *x * 2)",
            "Option::map_or_else",
        ),
    ] {
        let report = refused(&format!(
            "fn main() {{ let mut v: Vec<i64> = vec![1, 2, 3]; {call}; }}\nmain();\n"
        ));
        assert!(
            report.contains(&format!("rawr doesn't run `{path}` yet")),
            "{call}: {report}"
        );
    }
}

#[test]
fn language_an_options_clone_borrows_its_receiver() {
    // std writes `const impl<T> Clone for Option<T> where T: [const] Clone
    // + [const] Destruct`; the table keeps that impl, so `clone` takes
    // `&self` through a field of `&self` and through a `&Option`.
    let source = r#"
struct App {
    t: Option<String>,
}

impl App {
    fn get(&self) -> Option<String> {
        self.t.clone()
    }
}

fn get(o: &Option<String>) -> Option<String> {
    o.clone()
}

fn main() {
    let app = App { t: Some(String::from("a")) };
    let o = Some(String::from("b"));
    println!("{:?} {:?} {:?} {:?}", app.get(), get(&o), app.t, o);
}
main();
"#;
    assert_eq!(
        printed(source),
        "Some(\"a\") Some(\"b\") Some(\"a\") Some(\"b\")\n"
    );
}

#[test]
fn language_std_members_rustlings_reaches_for_run() {
    // `ParseIntError::kind` gives std's own `IntErrorKind`,
    // `Result::as_deref` borrows the `Ok` value through `Deref`, and a
    // `char`'s case mapping is std's own iterator (#902).
    let source = r#"
use std::num::IntErrorKind;

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + chars.as_str(),
    }
}

fn main() {
    let empty = "".parse::<u8>().unwrap_err();
    let digit = "beep".parse::<i32>().unwrap_err();
    let large = "300".parse::<u8>().unwrap_err();
    let small = "-300".parse::<i8>().unwrap_err();
    println!("{:?} {:?} {:?} {:?}", empty.kind(), digit.kind(), large.kind(), small.kind());
    println!("{}", digit.kind() == &IntErrorKind::InvalidDigit);
    match large.kind() {
        IntErrorKind::PosOverflow => println!("too large"),
        _ => println!("other"),
    }
    let name: Result<String, String> = Ok("Beyoncé".to_string());
    let failed: Result<String, String> = Err("empty".to_string());
    println!("{:?} {:?}", name.as_deref(), failed.as_deref());
    println!("{}", name.as_deref() == Ok("Beyoncé"));
    println!("{} {}", capitalize("hello"), capitalize("ß"));
    let mut upper = 'ß'.to_uppercase();
    println!("{:?} {:?} {:?}", upper.next(), upper.next(), upper.next());
    println!("{} {}", 'Q'.to_lowercase(), 'q'.to_uppercase());
}
main();
"#;
    assert_eq!(
        printed(source),
        "Empty InvalidDigit PosOverflow NegOverflow\ntrue\ntoo large\n\
         Ok(\"Beyoncé\") Err(\"empty\")\ntrue\nHello SS\nSome('S') Some('S') None\nq Q\n"
    );
}

#[test]
fn language_system_time_is_stds_own() {
    let source = r#"
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    let now = SystemTime::now();
    let since = now.duration_since(UNIX_EPOCH).unwrap();
    println!("{}", since.as_secs() > 1_700_000_000);
    let later = UNIX_EPOCH.checked_add(Duration::from_secs(90)).unwrap();
    println!("{:?}", later.duration_since(SystemTime::UNIX_EPOCH));
    let error = UNIX_EPOCH.duration_since(later).unwrap_err();
    println!("{:?} {}", error.duration(), error);
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => println!("{}", n.as_secs() > 0),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    }
}
main();
"#;
    assert_eq!(
        printed(source),
        "true\nOk(90s)\n90s second time provided was later than self\ntrue\n"
    );
}

#[test]
fn language_only_a_parse_int_error_has_a_kind() {
    // `ParseFloatError` has no `kind`: rustc's E0599.
    let report =
        refused("fn main() { let e = \"x\".parse::<f64>().unwrap_err(); e.kind(); }\nmain();\n");
    assert!(report.starts_with("error[E0599]"), "{report}");
}

#[test]
fn language_chars_as_str_is_the_text_left() {
    // `Chars::as_str` is std's own method of the struct `chars` gives
    // (#923), not of every iterator of chars.
    let source = r#"
fn main() {
    let text = String::from("rawr");
    let mut it = text.chars();
    it.next();
    println!("{} {}", it.as_str(), "abc".chars().as_str());
    let mut word = "héllo".chars();
    word.next();
    word.next();
    println!("{} {:?}", word.as_str(), word.next());
}
main();
"#;
    assert_eq!(printed(source), "awr abc\nllo Some('l')\n");
    let report =
        refused("fn main() { let s = \"ab\"; let it = s.chars().rev(); it.as_str(); }\nmain();\n");
    assert!(report.starts_with("error[E0599]"), "{report}");
}

#[test]
fn language_slice_binary_search_family_matches_std() {
    // std's `binary_search_by` loop (#1456): the `Ok` it picks among equal
    // elements and each `Err`'s insertion point are what rustc's build
    // prints, as is an inconsistent comparator's answer.
    let source = r#"
#[derive(Debug)]
struct Stop {
    floor: usize,
    name: &'static str,
}

struct Queue {
    calls: Vec<usize>,
}

impl Queue {
    fn call(&mut self, floor: usize) {
        if let Err(index) = self.calls.binary_search(&floor) {
            self.calls.insert(index, floor);
        }
    }
}

fn main() {
    let mut calls: Vec<usize> = vec![1, 3, 5, 7];
    for floor in [5, 4, 0, 9] {
        match calls.binary_search(&floor) {
            Ok(index) => println!("{floor} at {index}"),
            Err(index) => {
                calls.insert(index, floor);
                println!("{floor} in at {index}");
            }
        }
    }
    println!("{:?}", calls);
    let same = [1, 1, 1, 1, 1, 2, 2, 3];
    println!("{:?} {:?} {:?}", same.binary_search(&1), same.binary_search(&2), same.binary_search(&4));
    let empty: Vec<i32> = Vec::new();
    println!("{:?}", empty.binary_search(&3));
    let ascending = vec![10i64, 20, 30, 40, 50];
    let by = ascending.binary_search_by(|probe| probe.cmp(&35));
    let reversed = ascending.binary_search_by(|probe| 30.cmp(probe));
    println!("{:?} {:?} {:?}", by, reversed, ascending.binary_search_by(|p| p.cmp(&40)));
    let pairs = [(0, 0), (2, 1), (4, 1), (5, 1), (3, 1), (1, 2), (2, 3), (4, 5), (5, 8), (3, 13)];
    println!(
        "{:?} {:?} {:?} {:?}",
        pairs.binary_search_by_key(&13, |&(_, b)| b),
        pairs.binary_search_by_key(&4, |&(_, b)| b),
        pairs.binary_search_by_key(&1, |&(_, b)| b),
        pairs.binary_search_by_key(&100, |&(_, b)| b)
    );
    let v = [1, 2, 3, 3, 5, 6, 7];
    println!("{} {} {}", v.partition_point(|&x| x < 5), v.partition_point(|x| *x < 0), v.partition_point(|x| *x < 100));
    let stops = vec![
        Stop { floor: 2, name: "lobby" },
        Stop { floor: 4, name: "gym" },
        Stop { floor: 4, name: "pool" },
        Stop { floor: 8, name: "roof" },
    ];
    let at = stops.binary_search_by_key(&4, |stop| stop.floor);
    let missing = stops.binary_search_by(|stop| stop.floor.cmp(&6));
    let split = stops.partition_point(|stop| stop.floor < 5);
    println!("{:?} {:?} {} {}", at, missing, split, stops[split].name);
    let window = &v[1..5];
    println!("{:?} {:?}", window.binary_search(&3), window.binary_search(&4));
    let mut queue = Queue { calls: Vec::new() };
    for floor in [6, 2, 6, 9, 0] {
        queue.call(floor);
    }
    println!("{:?}", queue.calls);
}
main();
"#;
    assert_eq!(
        printed(source),
        "5 at 2\n4 in at 2\n0 in at 0\n9 in at 6\n[0, 1, 3, 4, 5, 7, 9]\n\
         Ok(4) Ok(6) Err(8)\nErr(0)\nErr(3) Err(5) Ok(3)\nOk(9) Err(7) Ok(4) Err(10)\n\
         4 0 7\nOk(2) Err(3) 3 roof\nOk(2) Err(3)\n[0, 2, 6, 9]\n"
    );
    // `binary_search` needs `T: Ord`, as its declaration says: rustc's
    // E0277 for an `f64`.
    let report = refused(
        "fn main() { let v = vec![1.0f64, 2.0]; let _ = v.binary_search(&1.0); }\nmain();\n",
    );
    assert!(report.starts_with("error[E0277]"), "{report}");
}

#[test]
fn language_std_paths_rustlings_writes_run() {
    // A parse error written in full in a `let` (#922), std's time types
    // imported into a `let` and an associated function named as a value
    // (#1002), `Formatter::write_str` through `use std::fmt` (#978), std's
    // trait functions with a turbofish or a written `Self` (#977), and
    // `SystemTime`'s `Add` and `Sub` of a `Duration` (#924).
    let source = r#"
use std::fmt;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Name;

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Formatter::write_str(f, "name")?;
        std::fmt::Formatter::write_str(f, "!")
    }
}

fn main() {
    let r: Result<i64, std::num::ParseIntError> = "3".parse();
    let bad: Result<u8, std::num::ParseIntError> = "x".parse();
    let d: Duration = Duration::from_secs(2);
    let start: Instant = Instant::now();
    let now = Instant::now;
    let later: Option<Instant> = Some(now());
    let secs = Duration::from_secs;
    let sum = Iterator::sum::<i64>(vec![1i64, 2, 3].into_iter());
    let parsed = <i64 as FromStr>::from_str("4").unwrap();
    let wide = <i64 as TryFrom<i32>>::try_from(5).unwrap();
    let narrow = <u8 as TryFrom<i32>>::try_from(300).is_err();
    let t: SystemTime = UNIX_EPOCH + Duration::from_secs(90);
    let mut u = t - Duration::from_secs(30);
    u += Duration::from_secs(1);
    u -= Duration::from_secs(2);
    println!("{r:?} {bad:?} {d:?} {} {:?} {}", later.unwrap() >= start, secs(3), Name);
    println!("{sum} {parsed} {wide} {narrow} {:?}", u.duration_since(UNIX_EPOCH).unwrap());
    println!("{} {} {:?}", t > u, t <= u, t.cmp(&u));
}
main();
"#;
    assert_eq!(
        printed(source),
        "Ok(3) Err(ParseIntError { kind: InvalidDigit }) 2s true 3s name!\n6 4 5 true 59s\ntrue false Greater\n"
    );
}
