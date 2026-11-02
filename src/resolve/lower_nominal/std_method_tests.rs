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
    let report = refused(&format!("{deque}d.rotate_left(1); }}\nmain();\n"));
    assert!(
        report.contains("rawr doesn't run `VecDeque::rotate_left` yet"),
        "{report}"
    );
}
