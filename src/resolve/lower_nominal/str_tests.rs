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
    // A formatting argument is sized, as this toolchain's rustc says.
    let report = refused(
        r#"
fn main() {
    let s: &str = "a";
    println!("{}", *s);
}
main();
"#,
    );
    assert!(
        report.contains("the size for values of type `str` cannot be known at compilation time"),
        "{report}"
    );
    // A `&str` parameter takes `&*s`, not `*s` (E0308).
    let report = refused(
        r#"
fn count(s: &str) -> usize {
    s.len()
}
fn main() {
    let s = String::new();
    count(*s);
}
main();
"#,
    );
    assert!(report.contains("expected `&str`, found `str`"), "{report}");
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

#[test]
fn language_cow_converts_compares_and_derefs() {
    let source = r#"
use std::borrow::Cow;

fn count(text: &str) -> usize {
    text.len()
}

fn shout<'a>(text: &'a str) -> Cow<'a, str> {
    if text.ends_with('!') {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(format!("{}!", text))
    }
}

fn describe(c: &Cow<str>) -> String {
    match c {
        Cow::Borrowed(s) => format!("borrowed {}", s),
        Cow::Owned(s) => format!("owned {}", s),
    }
}

fn main() {
    let a: Cow<str> = "hi".into();
    let b: Cow<str> = String::from("hi").into();
    let c = Cow::from("x");
    println!("{} {} {} {}", a == b, count(&a), c, describe(&b));
    let words: Vec<Cow<str>> = vec![shout("a"), shout("b!")];
    println!("{:?}", words);
    for w in &words {
        println!("{} {}", describe(w), w.to_uppercase());
    }
    let n = match shout("z") {
        Cow::Borrowed(s) => s.len(),
        Cow::Owned(s) => s.len() * 10,
    };
    println!("{}", n);
    let mut v: Cow<[i32]> = Cow::Borrowed(&[1, 2, 3]);
    v.to_mut()[0] = 9;
    println!("{:?} {}", v, v.iter().sum::<i32>());
    let s: &str = &*a;
    let t = String::from("tt");
    println!("{} {}", s, (*t).len() + (*a).len());
    println!("{:>5}|{:?}", a, Cow::Borrowed("q"));
}
main();
"#;
    assert_eq!(
        printed(source),
        "true 2 x owned hi\n[\"a!\", \"b!\"]\nowned a! A!\nborrowed b! B!\n20\n[9, 2, 3] 14\nhi 4\n   hi|\"q\"\n"
    );
}

#[test]
fn language_cow_refusals_are_rustcs() {
    // `is_owned` is an unstable associated function, no method (E0599).
    let report = refused(
        r#"
use std::borrow::Cow;
fn main() {
    let c: Cow<str> = Cow::Borrowed("a");
    let _ = c.is_owned();
}
main();
"#,
    );
    assert!(report.contains("is_owned"), "{report}");
    assert!(report.contains("Cow<'_, str>"), "{report}");
    let report = refused(
        r#"
use std::borrow::Cow;
fn main() {
    let c: Cow<str> = Cow::Borrowed("a");
    match c {
        Cow::Borrowed(s) => println!("{}", s),
    }
}
main();
"#,
    );
    // rawr's match-coverage error, as for every enum: rustc's E0004.
    assert!(report.contains("non-exhaustive patterns"), "{report}");
    assert!(report.contains("Cow::Owned(_)"), "{report}");
    let report = refused(
        r#"
use std::borrow::Cow;
fn main() {
    let c: Cow<str> = Cow::Borrowed("a");
    let x = *c;
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
use std::borrow::Cow;
fn f() -> Cow<str> {
    Cow::Borrowed("a")
}
fn main() {
    f();
}
main();
"#,
    );
    assert!(report.contains("E0106"), "{report}");
}

#[test]
fn language_cow_orders_and_clones_by_stds_impls() {
    let source = r#"
use std::borrow::Cow;

fn take(c: Cow<'_, str>) -> usize {
    c.len()
}

fn main() {
    let s = String::from("ab");
    let r = &s;
    println!("{} {}", *s == *"ab", **r == *s);
    let c: Cow<str> = Cow::Borrowed("a");
    let d = c.clone();
    let e: String = d.to_string();
    let o: Cow<str> = Cow::Owned(String::from("b"));
    println!("{} {} {} {}", e, c < o, c == d, o > d);
    println!("{}", take(Cow::Borrowed(&s)));
}
main();
"#;
    assert_eq!(printed(source), "true true\na true true true\n2\n");
}

#[test]
fn language_a_string_literal_spans_lines_as_rustc_lexes_it() {
    // A raw line break is part of the string, `\` before one skips the
    // next line's leading whitespace (blank lines too), and raw and byte
    // strings span lines. A test module skipped as rustc skips it holds
    // one too (#905).
    let source = r##"
const TABLE: &str = "a,1
b,2";

fn main() {
    let continued = "one \
                     two \

        three";
    let raw = r#"x
"y" \n"#;
    let bytes = b"p
q";
    let escaped = "tab\there
next";
    println!("{:?}", TABLE.lines().collect::<Vec<_>>());
    println!("{continued:?}");
    println!("{raw:?}");
    println!("{bytes:?}");
    println!("{escaped:?}");
    let message = "a\
\n  b";
    println!("{message:?}");
}

#[cfg(test)]
mod tests {
    const RESULTS: &str = "England,France,4,2
France,Italy,3,1";

    #[test]
    fn reads() {
        let team = RESULTS.lines().count();
        assert_eq!(team, 2);
    }
}
main();
"##;
    assert_eq!(
        printed(source),
        "[\"a,1\", \"b,2\"]\n\"one two three\"\n\"x\\n\\\"y\\\" \\\\n\"\n[112, 10, 113]\n\
         \"tab\\there\\nnext\"\n\"a\\n  b\"\n"
    );
    // A string still open at the end of the file is rustc's unterminated
    // string.
    let report = refused("fn main() { let s = \"open\nstill open; }\nmain();\n");
    assert!(report.contains("not terminated"), "{report}");
}
