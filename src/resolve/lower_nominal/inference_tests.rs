//! Programs whose types rustc infers from their uses and expectations:
//! literals, arms and tails, a generic call's parameters and an unsettled
//! `Vec::new()`. Each prints what rustc's build of it prints, or is refused
//! with rustc's code and message.

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
fn an_integer_literal_takes_its_type_from_its_use() {
    // `rounds` is a `usize` from the division, `big` a `u64` and `x` a `u8`
    // from the `let`s they reach, so `y` wraps at 256 (#1048).
    let output = printed(
        r#"
fn main() {
    let v = vec![1, 2, 3, 4, 5, 6, 7];
    let count = v.len();
    let rounds = 2;
    println!("{}", count / rounds);
    let big = 4_000_000_000;
    let n: u64 = big;
    println!("{}", n * 3);
    let x = 200;
    let y: u8 = x;
    println!("{}", y.wrapping_add(100));
    let idx = 3;
    println!("{}", v[idx]);
    let s = 250;
    let t: u8 = 10;
    println!("{}", t.wrapping_add(s));
}
main();
"#,
    );
    assert_eq!(output, "3\n12000000000\n44\n4\n4\n");
}

#[test]
fn a_usize_inferred_from_len_overflows_as_a_usize() {
    // `151` is a `usize` from `len`, so the subtraction overflows and
    // panics, where an `i32` would give -1.
    let error = Engine::new()
        .run("fn main() {\n    let len = vec![0; 300].len();\n    let mid = len / 2 - 151;\n    println!(\"{}\", mid);\n}\nmain();\n")
        .expect_err("the subtraction overflows");
    assert!(
        error
            .to_string()
            .contains("attempt to subtract with overflow"),
        "{error}"
    );
}

#[test]
fn a_literal_in_an_arm_a_tail_or_a_closure_takes_the_expectation() {
    // Each arm, block tail and closure body takes the type its `let`, its
    // function's output or its closure's return type expects (#1293).
    let output = printed(
        r#"
fn pick(c: bool) -> u64 {
    if c { 5_000_000_000 } else { 1 }
}
fn pick2(c: u8) -> u8 {
    match c { 0 => 255, _ => { 1 } }
}
fn main() {
    let f = |_x: u8| -> u8 { 250 };
    let a: u8 = if true { 200 } else { 100 };
    println!("{}", a.wrapping_add(100));
    println!("{} {} {}", pick(true), pick2(0).wrapping_add(1), f(0).wrapping_add(10));
    let b: u8 = { let t = 1; t + 254 };
    println!("{}", b);
    let d: u64 = match 1 { 1 => 5_000_000_000, _ => 0 };
    println!("{}", d);
    let k: Box<dyn Fn() -> u8> = Box::new(|| 255);
    println!("{}", k().wrapping_add(1));
    let hour = 13;
    let greetings: &[&str] = match hour {
        5..12 => &["a", "b"],
        12..17 => &["a", "b", "c"],
        _ => &[],
    };
    println!("{:?}", greetings);
}
main();
"#,
    );
    assert_eq!(
        output,
        "44\n5000000000 0 4\n255\n5000000000\n0\n[\"a\", \"b\", \"c\"]\n"
    );
}

#[test]
fn a_literal_in_an_if_arm_coerces_its_elements_to_the_output() {
    // Each arm's `vec![..]` is expected to be the `Vec<Box<dyn Named>>`, so
    // its elements coerce to `Box<dyn Named>` (#1293).
    let output = printed(
        r#"
trait Named {
    fn name(&self) -> String;
}
struct A;
struct B;
impl Named for A {
    fn name(&self) -> String {
        "a".to_string()
    }
}
impl Named for B {
    fn name(&self) -> String {
        "b".to_string()
    }
}
fn f(c: bool) -> Vec<Box<dyn Named>> {
    if c { vec![Box::new(A), Box::new(B)] } else { Vec::new() }
}
fn g(c: u8) -> Vec<Box<dyn Named>> {
    match c {
        0 => vec![Box::new(B)],
        _ => {
            let n = c;
            if n > 1 { vec![Box::new(B), Box::new(A)] } else { vec![] }
        }
    }
}
fn main() {
    let names: Vec<String> = f(true).iter().map(|n| n.name()).collect();
    let more: Vec<String> = g(2).iter().map(|n| n.name()).collect();
    println!("{:?} {:?} {}", names, more, f(false).len() + g(1).len());
}
main();
"#,
    );
    assert_eq!(output, "[\"a\", \"b\"] [\"b\", \"a\"] 0\n");
}

#[test]
fn a_generic_call_takes_its_parameters_from_the_let_it_is_assigned_to() {
    // The `let`'s type fixes `E`, `T` and the literals' kinds, and `&name`
    // coerces to the `&str` that `P<&str>` gives `T` (#1037, #1419).
    let output = printed(
        r#"
fn bad<E: From<String>>(n: i32) -> Result<(), E> {
    Err(E::from(format!("bad {n}")))
}
fn make<T: Default>() -> T {
    T::default()
}
#[derive(Debug)]
struct P<T>(T);
fn f<T>(t: T) -> P<T> {
    P(t)
}
fn pair<T>(a: T, b: T) -> (T, T) {
    (a, b)
}
fn wrap<T>(t: T) -> Option<T> {
    Some(t)
}
fn main() {
    let r: Result<(), String> = bad(3);
    println!("{:?}", r);
    let v: Vec<u8> = make();
    println!("{:?}", v);
    let name = String::from("hi");
    let b: P<&str> = f(&name);
    println!("{:?}", b);
    let c: (u8, u8) = pair(200, 100);
    println!("{}", c.0.wrapping_add(c.1));
    let w: Option<u8> = wrap(200);
    println!("{:?}", w.map(|x| x.wrapping_add(100)));
}
main();
"#,
    );
    assert_eq!(output, "Err(\"bad 3\")\n[]\nP(\"hi\")\n44\nSome(44)\n");
}

#[test]
fn a_vec_new_nothing_settles_is_e0282() {
    // rustc needs the element type of every local's `Vec<_>` (#1257).
    let report = refused(
        "fn main() {\n    let v = Vec::new();\n    println!(\"{}\", v.len());\n}\nmain();\n",
    );
    assert!(
        report.starts_with("error[E0282]: type annotations needed for `Vec<_>`"),
        "{report}"
    );
    // A use that fixes it.
    let output = printed("fn main() {\n    let mut v = Vec::new();\n    v.push(1u8);\n    println!(\"{}\", v.len());\n}\nmain();\n");
    assert_eq!(output, "1\n");
}

#[test]
fn float_literals_converted_into_f32s_are_f32s() {
    // std's `From` for `(f32, f32)` and `Vec<f32>` names the literals'
    // kind, so each sum rounds to an `f32`'s, where an `f64`'s 0.1 + 0.7
    // prints 0.7999999999999999 (#1048).
    let output = printed(
        r#"
fn point(p: impl Into<(f32, f32)>) -> (f32, f32) {
    p.into()
}
fn main() {
    let p = point((0.1, 0.2));
    println!("{:?} {}", p, p.0 + p.1);
    let q: (f32, f32) = (0.1, 0.7).into();
    println!("{}", q.0 + q.1);
    let v: Vec<f32> = [0.1, 0.2].into();
    println!("{}", v[0] + v[1]);
}
main();
"#,
    );
    assert_eq!(output, "(0.1, 0.2) 0.3\n0.8\n0.3\n");
}
