//! rustc's `str` and `std::borrow::Cow`: `*` of a `&str` or a `String` is
//! the unsized `str`, usable where rustc allows it, and `Cow<'_, str>` and
//! `Cow<'_, [T]>` are std's, with their methods and impls from std's
//! table. Each program's expected output is what rustc's build of it
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
fn language_deref_of_text_is_str() {
    let source = r#"
fn count(text: &str) -> usize {
    text.len()
}

fn main() {
    let s: &str = "ab";
    let t = String::from("abc");
    let a: &str = &*s;
    let b: &str = &*t;
    println!("{} {}", a, b);
    println!("{} {}", *s == *t, *s == *"ab");
    println!("{} {}", (*s).len(), (*t).to_uppercase());
    println!("{}", count(&*t));
    let r = &t;
    let c: &str = &**r;
    println!("{}", c);
}
main();
"#;
    assert_eq!(printed(source), "ab abc\nfalse true\n2 ABC\n3\nabc\n");
}

#[test]
fn language_str_by_value_is_unsized() {
    let report = refused(
        r#"
fn main() {
    let s: &str = "a";
    let x = *s;
}
main();
"#,
    );
    assert!(report.contains("E0277"), "{report}");
    assert!(
        report.contains("the size for values of type `str` cannot be known at compilation time"),
        "{report}"
    );
    let report = refused(
        r#"
fn main() {
    let t = String::new();
    let y = *t;
}
main();
"#,
    );
    assert!(report.contains("E0277"), "{report}");
    let report = refused(
        r#"
fn main() {
    let s: &str = "a";
    let _ = *s == "a";
}
main();
"#,
    );
    assert!(report.contains("E0277"), "{report}");
    assert!(
        report.contains("can't compare `str` with `&str`"),
        "{report}"
    );
}

#[test]
fn language_cow_of_str() {
    let source = r#"
use std::borrow::Cow;

fn label(n: i64) -> Cow<'static, str> {
    if n == 0 {
        Cow::Borrowed("zero")
    } else {
        Cow::Owned(format!("n{}", n))
    }
}

fn main() {
    let c: Cow<'_, str> = Cow::Borrowed("x");
    let mut d: Cow<str> = Cow::Owned(String::from("y"));
    d.to_mut().push_str("z");
    let e: &str = &*c;
    println!("{} {} {}", e, c.len(), d);
    match &d {
        Cow::Borrowed(s) => println!("b {}", s),
        Cow::Owned(s) => println!("o {}", s),
    }
    let o: String = c.into_owned();
    println!("{} {} {}", o, label(0), label(3));
    println!("{:?} {}", label(1), d == "yz");
    let mut e2 = label(0);
    e2.to_mut().push('!');
    println!("{} {}", e2, matches!(e2, Cow::Owned(_)));
}
main();
"#;
    assert_eq!(
        printed(source),
        "x 1 yz\no yz\nx zero n3\n\"n1\" true\nzero! true\n"
    );
}

#[test]
fn language_cow_of_slice() {
    let source = r#"
use std::borrow::Cow;

fn main() {
    let v = vec![1, 2];
    let cs: Cow<'_, [i32]> = Cow::Borrowed(&v[..]);
    let mut cs2 = cs.clone();
    cs2.to_mut().push(3);
    println!("{:?} {:?} {}", cs, cs2, cs2.len());
    let owned: Vec<i32> = cs2.into_owned();
    println!("{:?}", owned);
}
main();
"#;
    assert_eq!(printed(source), "[1, 2] [1, 2, 3] 3\n[1, 2, 3]\n");
}
