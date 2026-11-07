//! One generic trait implemented more than once for one type: each trait
//! instance (`From<&str>`, `From<String>`) or Self type (`Half for f32`,
//! `Half for f64`) is its own impl, which a call selects by rustc's rules.

use crate::diagnostic::{Report, Source};
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

/// Two impls whose bodies tell apart which one ran.
const BAR: &str = r#"
struct Bar(String);
impl From<&str> for Bar { fn from(s: &str) -> Self { Bar(format!("&{s}")) } }
impl From<String> for Bar { fn from(s: String) -> Self { Bar(s) } }
"#;

/// rustc's code and message for `source`, which rawr refuses.
fn refused(source: &str) -> (String, String) {
    let error = Engine::new()
        .compile(source)
        .map(|_| ())
        .expect_err("rawr refuses this program");
    let source = Source::new("main.rs", source).expect("a test source has an identity");
    let report = Report::from_parse(source, &error).expect("the error is in the source");
    (
        report.code().as_str().to_string(),
        report.message().to_string(),
    )
}

#[test]
fn from_impls_at_two_type_arguments_are_two_impls() {
    let output = printed(
        r#"
struct Bar(String);
impl From<&str> for Bar { fn from(s: &str) -> Self { Bar(s.to_string()) } }
impl From<String> for Bar { fn from(s: String) -> Self { Bar(s) } }
fn show(foo: impl Into<Bar>) -> String { let bar: Bar = foo.into(); bar.0 }
fn main() {
    println!("{}", show("a"));
    println!("{}", show(String::from("b")));
}
main();
"#,
    );
    assert_eq!(output, "a\nb\n");
}

#[test]
fn an_into_bound_takes_either_from_impl() {
    // A generic body's value is erased at runtime, where a `&str` and a
    // `String` are one text, so these impls convert alike.
    let output = printed(
        r#"
struct Bar(String);
impl From<&str> for Bar { fn from(s: &str) -> Self { Bar(s.to_string()) } }
impl From<String> for Bar { fn from(s: String) -> Self { Bar(s) } }
fn bound<T: Into<Bar>>(foo: T) -> String { foo.into().0 }
fn clause<T>(foo: T) -> String where T: Into<Bar> { let bar: Bar = foo.into(); bar.0 }
fn made<T: From<String>>(s: &str) -> T { T::from(s.to_string()) }
fn main() {
    println!("{} {}", bound("c"), bound(String::from("d")));
    println!("{} {}", clause("e"), clause(String::from("f")));
    let g: Bar = made("g");
    let h = made::<Bar>("h");
    println!("{} {}", g.0, h.0);
}
main();
"#,
    );
    assert_eq!(output, "c d\ne f\ng h\n");
}

#[test]
fn from_and_into_select_the_impl_by_the_argument() {
    let output = printed(&format!(
        r#"{BAR}
fn main() {{
    let x: Bar = "g".into();
    let y: Bar = String::from("h").into();
    let z = Bar::from("i");
    let w = Bar::from(String::from("j"));
    let f: fn(&str) -> Bar = Bar::from;
    println!("{{}} {{}} {{}} {{}} {{}}", x.0, y.0, z.0, w.0, f("k").0);
}}
main();
"#
    ));
    assert_eq!(output, "&g h &i j &k\n");
}

#[test]
fn a_trait_for_f32_and_f64_is_two_impls() {
    // #988: the Self type selects the impl, and a bare literal is f64.
    let output = printed(
        r#"
use std::ops::Mul;
#[derive(Clone, Copy)]
struct Angle(f64);
impl Mul<Angle> for f32 { type Output = f32; fn mul(self, a: Angle) -> f32 { self * a.0 as f32 + 0.5 } }
impl Mul<Angle> for f64 { type Output = f64; fn mul(self, a: Angle) -> f64 { self * a.0 } }
trait Half { fn half(self) -> Self; }
impl Half for f32 { fn half(self) -> f32 { self / 2.0 + 100.0 } }
impl Half for f64 { fn half(self) -> f64 { self / 2.0 } }
fn main() {
    let a = Angle(3.0);
    println!("{} {} {}", 2.0f32 * a, 2.0f64 * a, 2.0 * a);
    println!("{} {} {} {}", 4.0f32.half(), 4.0f64.half(), Half::half(4.0f32), 4.0.half());
}
main();
"#,
    );
    assert_eq!(output, "6.5 6 6\n102 2 102 2\n");
}

#[test]
fn from_for_f32_and_f64_converts_into_each() {
    let output = printed(
        r#"
#[derive(Clone, Copy)]
enum Roar { Growl }
impl From<Roar> for f32 { fn from(_: Roar) -> f32 { 1.5 } }
impl From<Roar> for f64 { fn from(_: Roar) -> f64 { 2.5 } }
fn wide<T: Into<f64>>(x: T) -> f64 { x.into() }
fn main() {
    let a: f32 = Roar::Growl.into();
    let b: f64 = Roar::Growl.into();
    println!("{a} {b} {} {}", wide(Roar::Growl), f32::from(Roar::Growl));
}
main();
"#,
    );
    assert_eq!(output, "1.5 2.5 2.5 1.5\n");
}

#[test]
fn a_trait_for_str_and_string_is_two_impls() {
    let output = printed(
        r#"
trait Shout { fn shout(&self) -> String; }
impl Shout for &str { fn shout(&self) -> String { format!("&{}", self.to_uppercase()) } }
impl Shout for String { fn shout(&self) -> String { self.to_uppercase() } }
fn main() {
    let s = String::from("b");
    println!("{} {}", "a".shout(), s.shout());
}
main();
"#,
    );
    assert_eq!(output, "&A B\n");
}

#[test]
fn a_function_without_self_for_f32_and_f64_is_selected_by_its_type() {
    let output = printed(
        r#"
trait Unit { fn unit() -> Self; }
impl Unit for f32 { fn unit() -> f32 { 0.5 } }
impl Unit for f64 { fn unit() -> f64 { 0.25 } }
fn main() {
    let a = <f32 as Unit>::unit();
    let b = <f64 as Unit>::unit();
    println!("{} {} {} {}", f32::unit(), f64::unit(), a, b);
}
main();
"#,
    );
    assert_eq!(output, "0.5 0.25 0.5 0.25\n");
}

#[test]
fn a_program_trait_at_two_arguments_is_selected_by_the_let() {
    let output = printed(
        r#"
trait Convert<T> { fn convert(&self) -> T; }
struct C(i64);
impl Convert<i64> for C { fn convert(&self) -> i64 { self.0 * 2 } }
impl Convert<String> for C { fn convert(&self) -> String { format!("c{}", self.0) } }
fn main() {
    let c = C(3);
    let n: i64 = c.convert();
    let s: String = c.convert();
    println!("{n} {s} {}", <C as Convert<i64>>::convert(&c));
}
main();
"#,
    );
    assert_eq!(output, "6 c3 6\n");
}

#[test]
fn an_operator_at_two_right_operands_is_two_impls() {
    let output = printed(
        r#"
use std::ops::Add;
struct C(i64);
impl Add<i64> for C { type Output = i64; fn add(self, n: i64) -> Self::Output { self.0 + n } }
impl Add<&str> for C { type Output = String; fn add(self, s: &str) -> Self::Output { format!("{}{s}", self.0) } }
fn main() {
    let n: i64 = C(1) + 2;
    let s: String = C(1) + "x";
    println!("{n} {s} {}", (C(3) + "y").len());
}
main();
"#,
    );
    assert_eq!(output, "3 1x 2\n");
}

#[test]
fn a_call_nothing_selects_an_impl_for_is_e0283() {
    let expected = ("E0283".to_string(), "type annotations needed".to_string());
    assert_eq!(
        refused(
            r#"
trait Convert<T> { fn convert(&self) -> T; }
struct C(i64);
impl Convert<i64> for C { fn convert(&self) -> i64 { self.0 * 2 } }
impl Convert<String> for C { fn convert(&self) -> String { format!("c{}", self.0) } }
fn main() {
    let c = C(3);
    let x = c.convert();
}
main();
"#
        ),
        expected
    );
    assert_eq!(
        refused(
            r#"
struct Bar(String);
impl From<Bar> for String { fn from(b: Bar) -> Self { b.0 } }
impl From<Bar> for Vec<u8> { fn from(b: Bar) -> Self { b.0.into_bytes() } }
fn main() {
    let x = Bar(String::new()).into();
}
main();
"#
        ),
        expected
    );
}
