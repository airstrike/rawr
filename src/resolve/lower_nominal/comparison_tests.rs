//! `==` and `!=` resolve through the `PartialEq<Rhs>` impl of the left
//! operand's type that takes the right operand's, as rustc selects it:
//! std's impls between its own types and the blanket `impl PartialEq<&B>
//! for &A where A: PartialEq<B>`. Each program's expected output is what
//! rustc's build of it prints, and each refused one is refused by rustc
//! with the same E0277.

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
fn language_text_compares_across_string_str_and_references() {
    let source = r#"
fn main() {
    let s = String::from("Serif");
    let r: &str = "Serif";
    let rs: &String = &s;
    let rr: &&str = &r;
    println!("{} {} {}", rs == r, s == r, r == s);
    println!("{} {}", *rs == r, rr == &r);
    println!("{} {}", rs != "Mono", &s == &r);
    let names = vec![String::from("Mono"), String::from("Serif")];
    println!("{:?}", names.iter().position(|name| name == "Serif"));
    println!("{:?}", names.iter().position(|name| "Mono" == name));
}
main();
"#;
    assert_eq!(
        printed(source),
        "true true true\ntrue true\ntrue true\nSome(1)\nSome(0)\n"
    );
}

#[test]
fn language_sequences_and_options_compare_through_their_elements() {
    let source = r#"
fn main() {
    let v: Vec<u8> = vec![1, 2, 3];
    println!("{} {}", v == [1u8, 2, 3], v == [1, 2, 4]);
    println!("{} {}", v == &[1u8, 2, 3][..], &v[..] == v);
    let words = vec![String::from("a"), String::from("b")];
    println!("{}", words == ["a", "b"]);
    let o: Option<&str> = Some("x");
    println!("{} {}", o == Some("x"), o != None);
}
main();
"#;
    assert_eq!(printed(source), "true false\ntrue true\ntrue\ntrue true\n");
}

#[test]
fn language_a_comparison_no_impl_takes_is_e0277() {
    let report = refused(
        r#"
fn main() {
    let s = String::from("x");
    println!("{}", s == 1);
}
"#,
    );
    assert!(
        report.starts_with("error[E0277]: can't compare `String` with `{integer}`\n"),
        "{report}"
    );
    // At the operator, as rustc reports it.
    assert!(report.contains("<script>:4:22-4:24"), "{report}");
    assert!(
        report.contains("no implementation for `String == {integer}`"),
        "{report}"
    );
    assert!(
        report.contains("help: the trait `PartialEq<{integer}>` is not implemented for `String`"),
        "{report}"
    );

    let report = refused(
        r#"
fn main() {
    let r: &str = "x";
    println!("{}", r == 1u8);
}
"#,
    );
    assert!(
        report.starts_with("error[E0277]: can't compare `&str` with `u8`\n"),
        "{report}"
    );
    assert!(
        report.contains("help: the trait `PartialEq<u8>` is not implemented for `&str`"),
        "{report}"
    );

    // `&&str == &str` needs `&str: PartialEq<str>`, which std doesn't have.
    let report = refused(
        r#"
fn main() {
    let r: &str = "x";
    let rr: &&str = &r;
    println!("{}", rr == r);
}
"#,
    );
    assert!(
        report.starts_with("error[E0277]: can't compare `&str` with `str`\n"),
        "{report}"
    );
    assert!(
        report.contains("note: required for `&&str` to implement `PartialEq<&str>`"),
        "{report}"
    );

    // A sequence impl needs its elements' impl.
    let report = refused(
        r#"
fn main() {
    let v: Vec<u8> = vec![1, 2, 3];
    println!("{}", v == [1u16, 2, 3]);
}
"#,
    );
    assert!(
        report.starts_with("error[E0277]: can't compare `u8` with `u16`\n"),
        "{report}"
    );
    assert!(
        report.contains("note: required for `Vec<u8>` to implement `PartialEq<[u16; 3]>`"),
        "{report}"
    );

    let report = refused(
        r#"
fn main() {
    println!("{}", &1i64 == 1i64);
}
"#,
    );
    assert!(
        report.starts_with("error[E0277]: can't compare `&i64` with `i64`\n"),
        "{report}"
    );
}

/// `&a < &b` compares the referents through core's `impl PartialOrd<&B>
/// for &A`, a `u64`'s as unsigned.
#[test]
fn language_references_order_through_their_referents() {
    let source = r#"
fn main() {
    let a = 1i64;
    let b = 2i64;
    let s = String::from("a");
    let t = String::from("b");
    let r: &str = "b";
    println!("{} {} {} {}", &a < &b, &s >= &t, r > "a", &&r <= &&"b");
    let big: u64 = u64::MAX;
    let one: u64 = 1;
    println!("{}", &big > &one);
    let names = vec![String::from("Mono"), String::from("Serif")];
    println!("{}", names.iter().filter(|name| name.as_str() < "N").count());
    let mut sorted = vec![3, 1, 2];
    sorted.sort_by(|x, y| if x < y { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater });
    println!("{:?}", sorted);
}
main();
"#;
    assert_eq!(
        printed(source),
        "true false true true\ntrue\n1\n[1, 2, 3]\n"
    );
}
