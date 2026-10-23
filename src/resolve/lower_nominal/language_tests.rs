//! Rust-shaped core language in checked Rawr functions: each snippet
//! type-checks as Rust would and evaluates to the Rust result.

use crate::Engine;

fn run(source: &str) -> i64 {
    Engine::new()
        .eval::<i64>(source)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn rejected(source: &str) -> String {
    match Engine::new().eval::<i64>(source) {
        Ok(value) => panic!("Rust rejects this program, but it evaluated to {value}: {source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn language_comparison_and_logic() {
    let value = run(r#"
        pub fn run(n: i64) -> i64 {
            let a = n > 2 && n < 10;
            let b = n >= 3 || !(n <= 4);
            let c = n != 2;
            if a && b && c { 1 } else { 0 }
        }
        run(5)
    "#);
    assert_eq!(value, 1);
    rejected("pub fn run(n: i64) -> bool { n > 2.0 } run(1)");
    rejected("pub fn run(n: i64) -> bool { 1 < n < 3 } run(1)");
}

#[test]
fn language_integer_arithmetic() {
    let value = run(r#"
        pub fn run(n: i64) -> i64 {
            let mut x = n * 3 / 2 % 5 - -n;
            x *= 4;
            x /= 3;
            x %= 7;
            x
        }
        run(7)
    "#);
    assert_eq!(value, 7 * 4 / 3 % 7);
    rejected("pub fn run(n: i64) -> i64 { n * 2.0 } run(1)");
    let float = Engine::new()
        .eval::<f64>("pub fn run(x: f64) -> f64 { x * 3.0 / 2.0 % 4.0 - -x } run(5.0)")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(float, 5.0 * 3.0 / 2.0 % 4.0 + 5.0);
}

#[test]
fn language_private_free_functions_are_typed() {
    let value = run(r#"
        fn double(n: i64) -> i64 { n * 2 }
        fn seven() -> i64 { 7 }
        pub fn run() -> i64 { double(seven()) + 1 }
        run()
    "#);
    assert_eq!(value, 15);
    rejected(
        r#"
        fn name(n: i64) -> String { "x" }
        pub fn run() -> i64 { name(1) }
        run()
    "#,
    );
}

struct UiElement;
struct UiText;
struct UiButton;

/// An iced-shaped facade: `Text` and `Button<M>` project to `Element<M>`.
fn ui_facade() -> crate::host::facade::Definition {
    use crate::host::{facade, item, signature, type_def, type_ref};
    let mut builder = facade::Builder::new("ui", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<UiElement>("Element", &["Message"], &[])
        .unwrap();
    builder.type_::<UiText>("Text", &[], &[]).unwrap();
    builder
        .type_::<UiButton>("Button", &["Message"], &[])
        .unwrap();
    builder
        .project_with("Text", "Element", &[type_def::Argument::fresh()])
        .unwrap();
    builder.project("Button", "Element", &["Message"]).unwrap();
    let message = || type_ref::Type::parameter("Message").unwrap();
    let button = || type_ref::Type::applied("Button", [message()]).unwrap();
    let mut text = signature::Builder::new();
    text.output(type_ref::Type::registered("Text").unwrap());
    builder
        .item(item::Definition::function("text", text.finish()).unwrap())
        .unwrap();
    let mut new = signature::Builder::new();
    new.type_parameter("Message", &[]).unwrap();
    new.output(button());
    builder
        .item(item::Definition::function("button", new.finish()).unwrap())
        .unwrap();
    let mut on_press = signature::Builder::new();
    on_press.type_parameter("Message", &[]).unwrap();
    on_press.parameter(message()).output(button());
    builder
        .item(
            item::Definition::method(
                "Button::on_press",
                crate::receiver::Mode::Owned,
                on_press.finish(),
            )
            .unwrap(),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn language_if_else_widget_arms_meet_at_element() {
    let mut engine = Engine::new();
    engine.register_host_facade(ui_facade()).unwrap();
    engine
        .compile_executable(
            r#"
use ui::{Element, button, text};
#[derive(Clone)]
enum Message { Go }
pub fn same(flag: bool) -> Element<'static, Message> {
    let b = if flag { button() } else { button().on_press(Message::Go) };
    b
}
pub fn mixed(flag: bool) -> Element<'static, Message> {
    let e: Element<'_, _> = if flag { text() } else { button().on_press(Message::Go) };
    e
}
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn language_loops() {
    let value = run(r#"
        pub fn run(n: i64) -> i64 {
            let mut total = 0;
            for i in 0..n {
                if i % 2 == 0 {
                    continue;
                }
                total += i;
            }
            for _ in 1..=2 { total += 100; }
            let mut k = 0;
            while k < 3 { k += 1; }
            loop {
                k += 1;
                if k >= 10 { break; }
            }
            total + k
        }
        run(6)
    "#);
    assert_eq!(value, (1 + 3 + 5) + 200 + 10);
    rejected("pub fn run() -> i64 { break; 1 } run()");
}

#[test]
fn language_integer_match() {
    let value = run(r#"
        pub fn classify(n: i64) -> i64 {
            match n {
                0 => 10,
                1 | 2 => 20,
                3..=5 => 30,
                -9..-1 => 40,
                m if m > 100 => m,
                _ => 50,
            }
        }
        pub fn run() -> i64 {
            classify(0) + classify(2) + classify(4) + classify(-3) + classify(101) + classify(7)
        }
        run()
    "#);
    assert_eq!(value, 10 + 20 + 30 + 40 + 101 + 50);
    rejected("pub fn run(n: i64) -> i64 { match n { 0 => 1, 1 => 2 } } run(0)");
}

#[test]
fn language_match_self_in_enum_impl() {
    let value = run(r#"
        enum Section { First, Second }
        impl Section {
            fn number(&self) -> i64 { match self { Section::First => 1, Section::Second => 2 } }
            fn deref(&self) -> i64 { match *self { Section::First => 3, Section::Second => 4 } }
        }
        pub fn run() -> i64 { let s = Section::Second; s.number() * 10 + s.deref() }
        run()
    "#);
    assert_eq!(value, 24);
}

#[test]
fn language_string_building() {
    let value = Engine::new()
        .eval::<String>(
            r#"
            pub fn run(n: i64) -> String {
                let unit = "px".to_string();
                let mut s = format!("{}{unit} of {total}, {{{0}}}", n, total = n * 2);
                s.push_str(" and ");
                s.push('!');
                let tail = String::new() + "x" + &unit;
                s + &tail + &format!("{}", s.len()) + &n.to_string().to_uppercase()
            }
            run(5)
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let s = format!("{}{unit} of {total}, {{{0}}}", 5, unit = "px", total = 10) + " and !";
    assert_eq!(value, format!("{s}xpx{}5", s.len()));
    rejected(r#"pub fn run() -> i64 { let s = format!("{}", 1, 2); 0 } run()"#);
}

#[test]
fn language_print_macros_format_like_format() {
    use std::sync::{Arc, Mutex};
    let written = Arc::new(Mutex::new(Vec::<(crate::Output, String)>::new()));
    let mut engine = Engine::new();
    let log = written.clone();
    engine.on_output(move |output, text| log.lock().unwrap().push((output, text.to_owned())));
    engine
        .eval::<()>(
            r#"
            fn main() {
                let n = 7;
                print!("{n:>3}|");
                println!("{}{unit}", n * 2, unit = "px");
                println!();
                eprint!("{:?}", Some("x"));
                eprintln!("{{{}}}", 1.5);
            }
            main()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let (out, err) = (crate::Output::Stdout, crate::Output::Stderr);
    assert_eq!(
        *written.lock().unwrap(),
        [
            (out, "  7|".to_owned()),
            (out, "14px\n".to_owned()),
            (out, "\n".to_owned()),
            (err, "Some(\"x\")".to_owned()),
            (err, "{1.5}\n".to_owned()),
        ]
    );
    // Rust's text for floats: `{}` drops a zero fraction, `{:?}` keeps it.
    let floats = Engine::new()
        .eval::<String>(r#"format!("{} {} {:?} {:?} {} {:?}", 27.0, 27.5, 27.0, 27.5, 27, "s")"#)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        floats,
        format!("{} {} {:?} {:?} {} {:?}", 27.0, 27.5, 27.0, 27.5, 27, "s")
    );
    rejected(r#"pub fn run() -> i64 { println!("{}", 1, 2); 0 } run()"#);
    rejected(r#"pub fn run() -> i64 { print!("{}"); 0 } run()"#);
    rejected(r#"pub fn run() -> i64 { eprintln!("{:x}", "x"); 0 } run()"#);
    rejected(r#"pub fn run() -> i64 { print!(); 0 } run()"#);
    rejected(r#"pub fn run() -> i64 { let x: i64 = println!("a"); x } run()"#);
}

#[test]
fn language_constants() {
    let value = run(r#"
        pub const GAP: i64 = 4 * 2;
        const LIMIT: i64 = GAP + 1;
        pub fn run(n: i64) -> i64 { if n < LIMIT { GAP } else { 0 } }
        run(3)
    "#);
    assert_eq!(value, 8);
    rejected(r#"pub const NAME: i64 = "x"; pub fn run() -> i64 { NAME } run()"#);
}

#[test]
fn language_format_specs() {
    let value = Engine::new()
        .eval::<String>(
            r#"
            pub fn run(n: i64) -> String {
                let s = "abc";
                format!("{:02}|{:05}|{:+04}|{:>4}|{:<4}|{:^5}|{:*^6}|{:.2}|{s:>6.1}|{:+}|{:05}|{:3}|{:x<3}", n, -n, n, n, n, n, s, s, n, s, 'c', true)
            }
            run(7)
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let (n, s) = (7, "abc");
    assert_eq!(
        value,
        format!(
            "{:02}|{:05}|{:+04}|{:>4}|{:<4}|{:^5}|{:*^6}|{:.2}|{s:>6.1}|{:+}|{:05}|{:3}|{:x<3}",
            n, -n, n, n, n, n, s, s, n, s, 'c', true
        )
    );
    rejected(r#"pub fn run() -> i64 { let s = format!("{:e}", "x"); 0 } run()"#);
}

#[test]
fn language_break_values_and_labels() {
    let value = run(r#"
        pub fn run() -> i64 {
            let mut i = 0;
            let doubled = loop {
                i += 1;
                if i == 3 { break i * 2; }
            };
            let mut t = 0;
            'outer: for a in 0..5 {
                let mut b = 0;
                while b < 5 {
                    b += 1;
                    if b == 3 { continue 'outer; }
                    if a == 3 { break 'outer; }
                    t += 1;
                }
            }
            let found = 'search: loop {
                for k in 1..10 {
                    if k * k > 30 { break 'search k; }
                }
                break 0;
            };
            doubled * 1000 + t * 10 + found
        }
        run()
    "#);
    let found = (1..10).find(|k| k * k > 30).unwrap_or(0);
    assert_eq!(value, 6 * 1000 + 3 * 2 * 10 + found);
    rejected("pub fn run() -> i64 { let mut i = 0; while i < 3 { i += 1; break 5; } i } run()");
    rejected("pub fn run() -> i64 { loop { break 'nowhere; } 1 } run()");
    rejected("pub fn run() -> i64 { loop { if true { break 1; } break true; } } run()");
}

#[test]
fn language_wildcard_arm_with_guard() {
    let value = run(r#"
        enum Mode { Plain, Strong, Soft }
        fn score(mode: Mode, mains: bool) -> i64 {
            match mode {
                Mode::Plain => 1,
                _ if !mains => 2,
                Mode::Strong => 3,
                _ => 4,
            }
        }
        pub fn run() -> i64 {
            score(Mode::Plain, false) * 1000 + score(Mode::Strong, false) * 100
                + score(Mode::Strong, true) * 10 + score(Mode::Soft, true)
        }
        run()
    "#);
    assert_eq!(value, 1234);
    rejected(
        r#"
        enum Mode { Plain, Strong }
        pub fn run() -> i64 { let m = Mode::Plain; match m { Mode::Plain => 1, _ if true => 2 } }
        run()
    "#,
    );
}

#[test]
fn language_local_shadows_function() {
    let value = run(r#"
        fn lamp() -> i64 { 1 }
        const GAP: i64 = 10;
        pub fn run() -> i64 {
            let x = lamp();
            let lamp = 5;
            let GAP = 7;
            lamp * 100 + GAP * 10 + x
        }
        run()
    "#);
    assert_eq!(value, 571);
    rejected("fn lamp() -> i64 { 1 } pub fn run() -> i64 { let lamp = 5; lamp() } run()");
}

#[test]
fn language_struct_update_syntax() {
    let value = run(r#"
        struct Style { width: i64, height: i64, depth: i64 }
        fn base() -> Style { Style { width: 1, height: 2, depth: 3 } }
        pub fn run() -> i64 {
            let height = 20;
            let s = Style { depth: 30, height, ..base() };
            let t = Style { width: 400, ..s };
            t.width * 1000 + t.height * 10 + t.depth
        }
        run()
    "#);
    assert_eq!(value, 400 * 1000 + 20 * 10 + 30);
    rejected(
        r#"
        struct Style { width: i64, height: i64 }
        struct Other { width: i64, height: i64 }
        pub fn run() -> i64 { let o = Other { width: 1, height: 2 }; let s = Style { width: 3, ..o }; s.width }
        run()
    "#,
    );
    rejected("struct P { x: i64, y: i64 } pub fn run() -> i64 { let p = P { x: 1 }; p.x } run()");
}

#[test]
fn language_str_parameters() {
    let value = run(r#"
        fn width(label: &str) -> i64 { label.len() as i64 }
        fn pick(label: &'static str) -> &'static str { label }
        pub fn run() -> i64 {
            let owned = String::from("four");
            let s: &str = "ab";
            width("abc") * 100 + width(&owned) * 10 + width(pick(s)) + width(owned.as_str()) * 1000
        }
        run()
    "#);
    assert_eq!(value, 3 * 100 + 4 * 10 + 2 + 4 * 1000);
    rejected("fn width(label: &str) -> i64 { label.len() as i64 } pub fn run() -> i64 { width(5) } run()");
}

#[test]
fn language_owned_self_receivers() {
    let value = run(r#"
        #[derive(Clone, Copy)]
        struct Counter { n: i64 }
        impl Counter {
            fn bump(mut self) -> Counter { self.n += 1; self }
            fn take(self) -> i64 { self.n }
        }
        pub fn run() -> i64 {
            let c = Counter { n: 1 };
            let d = c.bump().bump();
            d.take() * 10 + c.n
        }
        run()
    "#);
    assert_eq!(value, 31);
    rejected("struct C { n: i64 } impl C { fn set(self) { self.n = 2; } } pub fn run() -> i64 { 0 } run()");
}

#[test]
fn language_closures_and_function_values() {
    let value = run(r#"
        fn add(a: i64, b: i64) -> i64 { a + b }
        fn twice(n: i64) -> i64 { n * 2 }
        pub fn run(flag: bool) -> i64 {
            let k = 10;
            let plus = |a: i64, b: i64| a + b + k;
            let mut count = 0;
            let mut bump = || count += 1;
            bump();
            bump();
            let mut base = 1;
            let frozen = move |x| x + base;
            base = 100;
            let f: fn(i64, i64) -> i64 = add;
            let pick = if flag { twice } else { |n: i64| n + 1 };
            let block = |n: i64| -> i64 { let m = n * 3; m + 1 };
            plus(1, 2) * 1000 + f(3, 4) * 100 + count * 10 + frozen(0) + pick(base) + block(1)
        }
        run(true)
    "#);
    let k = 10;
    let plus = |a: i64, b: i64| a + b + k;
    let frozen = move |x: i64| x + 1;
    assert_eq!(
        value,
        plus(1, 2) * 1000 + 7 * 100 + 2 * 10 + frozen(0) + 200 + 4
    );
    rejected("pub fn run() -> i64 { let f = |a: i64| a; f(1, 2) } run()");
    rejected("pub fn run() -> i64 { let f = |a: i64| a; f(true) } run()");
}

#[test]
fn language_closures_in_variables_are_values() {
    // A closure in a local is a value: passed to a function or an adaptor,
    // copied to another local, and called from inside another closure.
    let value = run(r#"
        struct Db(i64);
        impl From<i64> for Db {
            fn from(r: i64) -> Db { Db(r * 2) }
        }
        fn apply(f: impl Fn(i64) -> i64, x: i64) -> i64 { f(x) }
        pub fn run() -> i64 {
            let sq = |x: i64| x * x;
            let g = sq;
            let loud = |r: i64| Db::from(r);
            let v: Vec<i64> = vec![1, 2, 3].into_iter().map(sq).collect();
            let w: Vec<i64> = v.iter().map(|x| loud(*x).0).collect();
            apply(sq, 4) + g(5) * 100 + w[2] * 10000
        }
        run()
    "#);
    assert_eq!(value, 16 + 25 * 100 + 18 * 10000);
}

#[test]
fn language_associated_functions_are_values() {
    // `Db::from` of a user `From` impl and an inherent `Db::new` are
    // function values: passed to `map`, bound to a local, and coerced to
    // a `fn` pointer.
    let value = run(r#"
        #[derive(Clone, Copy)]
        enum Roar { Squeak = 3, Growl = 7 }
        struct Db(i64);
        impl From<Roar> for Db {
            fn from(r: Roar) -> Db { Db(r as i64 * 2) }
        }
        impl Db { fn new(n: i64) -> Db { Db(n) } }
        pub fn run() -> i64 {
            let v: Vec<i64> = [Roar::Squeak, Roar::Growl].iter().copied().map(Db::from).map(|d| d.0).collect();
            let loud = Db::from;
            let f: fn(Roar) -> Db = Db::from;
            let n = Db::new;
            v[0] + v[1] * 100 + loud(Roar::Growl).0 * 10000 + f(Roar::Squeak).0 * 1000000 + n(5).0 * 100000000
        }
        run()
    "#);
    assert_eq!(value, 6 + 14 * 100 + 14 * 10000 + 6 * 1000000 + 5 * 100000000);
}

#[test]
fn language_vec_repeat_takes_any_count() {
    // `vec![x; n]` takes any `usize` expression; the value comes first.
    let value = run(r#"
        pub fn run() -> i64 {
            let code = vec![1, 2, 3];
            let a = 4;
            let jump = vec![0usize; code.len()];
            let more = vec![a; code.len() * 2];
            let fixed = [7; 2];
            (jump.len() + more.len() * 10) as i64 + more[5] * 100 + fixed[1] * 1000
        }
        run()
    "#);
    assert_eq!(value, 3 + 60 + 400 + 7000);
    rejected("pub fn run() -> i64 { let n = 2; let a = [0; n]; a[0] } run()");
}

#[test]
fn language_from_impls_for_sized_numbers() {
    // `impl From<Local> for u32`, also through an alias, is a trait impl
    // with a local type argument, which Rust's orphan rule allows.
    let value = run(r#"
        type Decibels = u32;
        #[derive(Clone, Copy)]
        enum Roar { Squeak = 3, Growl = 7 }
        impl From<Roar> for Decibels {
            fn from(r: Roar) -> Decibels { r as u32 * 10 }
        }
        impl From<Roar> for f32 {
            fn from(r: Roar) -> f32 { r as u8 as f32 / 2.0 }
        }
        pub fn run() -> i64 {
            let d: Decibels = Roar::Growl.into();
            let e = u32::from(Roar::Squeak);
            let h: f32 = Roar::Growl.into();
            (d + e * 1000) as i64 + (h * 2.0) as i64 * 1000000
        }
        run()
    "#);
    assert_eq!(value, 70 + 30 * 1000 + 7 * 1000000);
}

#[test]
fn language_fixed_array_map() {
    // `[T; N]::map` is `[U; N]`; a `Vec` has no `map`.
    let value = run(r#"
        pub fn run() -> i64 {
            let a = [1, 2, 3];
            let b: [i64; 3] = a.map(|x| x * 10);
            let c = ["a", "bc"].map(|s| s.len());
            b[2] + (c[1] * 100) as i64 + b.len() as i64 * 1000
        }
        run()
    "#);
    assert_eq!(value, 30 + 200 + 3000);
    rejected("pub fn run() -> i64 { let v = vec![1]; let w = v.map(|x| x); w[0] } run()");
}

#[test]
fn language_float_trigonometry() {
    // `f32` and `f64` have Rust's trig, exponential and log methods.
    let value = run(r#"
        pub fn run() -> i64 {
            let angle: f32 = std::f32::consts::PI / 2.0;
            let x = 3.0_f64;
            let total = angle.sin() * 1000.0
                + (angle.cos().abs() < 0.001) as i64 as f32 * 100.0
                + (1.0_f32).atan2(1.0).to_degrees();
            (total as f64 + x.hypot(4.0) * 10000.0 + x.ln().exp() * 100000.0 + 100.0_f64.log10() * 1000000.0) as i64
        }
        run()
    "#);
    assert_eq!(value, 1000 + 100 + 45 + 50000 + 300000 + 2000000);
}

#[test]
fn language_iter_once_and_empty() {
    // `std::iter::once(x)` is an iterator of one item, `empty()` of none.
    let value = run(r#"
        use std::iter;
        pub fn run() -> i64 {
            let all: Vec<i64> = std::iter::once(1).chain(vec![2, 3]).collect();
            let none: Vec<i64> = iter::empty().collect();
            let one = iter::once(7).count() as i64;
            all.iter().sum::<i64>() * 100 + none.len() as i64 * 10 + one
        }
        run()
    "#);
    assert_eq!(value, 600 + 1);
}

#[test]
fn language_std_time_in_plain_programs() {
    // `std::time::Duration` and `Instant` without iced: Rust's values, with
    // their constructors, readers and operators.
    let value = run(r#"
        use std::time::{Duration, Instant};
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        struct Lap { best: Duration }
        fn total(laps: &[Duration]) -> Duration {
            let mut sum = Duration::ZERO;
            for lap in laps {
                sum += *lap;
            }
            sum
        }
        pub fn run() -> i64 {
            let start = Instant::now();
            let laps = vec![Duration::from_millis(1500), Duration::from_secs(2), Duration::from_secs_f64(0.25)];
            let sum = total(&laps);
            let half = sum / 2;
            let later = start + Duration::from_millis(10);
            let lap = Lap::default();
            let shown = format!("{:?} {:?} {:?}", sum, half * 3, lap);
            assert_eq!(shown, "3.75s 5.625s Lap { best: 0ns }");
            assert!(sum > half && later > start && start.elapsed() < Duration::from_secs(5));
            assert_eq!(Duration::from_secs(1).checked_sub(Duration::from_secs(2)), None);
            (sum.as_millis() + (later - start).as_millis()) as i64 + sum.subsec_millis() as i64 * 10000
        }
        run()
    "#);
    assert_eq!(value, 3750 + 10 + 750 * 10000);
}

/// `Button::style` takes a two-parameter host callable, as iced's
/// `fn(&Theme, Status) -> Style`.
fn style_facade() -> crate::host::facade::Definition {
    use crate::host::{facade, item, signature, type_ref};
    let mut builder = facade::Builder::new("ui", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<UiButton>("Button", &[], &[]).unwrap();
    let mut new = signature::Builder::new();
    new.output(type_ref::Type::registered("Button").unwrap());
    builder
        .item(item::Definition::function("button", new.finish()).unwrap())
        .unwrap();
    let int = || type_ref::Type::builtin(type_ref::Builtin::I64);
    let mut style = signature::Builder::new();
    style
        .parameter(type_ref::Type::callable(
            type_ref::Type::tuple([int(), type_ref::Type::builtin(type_ref::Builtin::Bool)])
                .unwrap(),
            int(),
        ))
        .output(type_ref::Type::registered("Button").unwrap());
    builder
        .item(
            item::Definition::method(
                "Button::style",
                crate::receiver::Mode::Owned,
                style.finish(),
            )
            .unwrap(),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn language_multi_parameter_function_values() {
    let compile = |source: &str| {
        let mut engine = Engine::new();
        engine.register_host_facade(style_facade()).unwrap();
        engine.compile_executable(source).map(|_| ())
    };
    compile(
        r#"
use ui::{Button, button};
fn shade(base: i64, pressed: bool) -> i64 { if pressed { base * 2 } else { base } }
pub fn view() -> Button { button().style(shade) }
"#,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let _ = compile(
        r#"
use ui::{Button, button};
fn shade(base: i64) -> i64 { base }
pub fn view() -> Button { button().style(shade) }
"#,
    )
    .expect_err("Rust rejects a one-parameter function for a two-parameter callable");
}

#[test]
fn language_closures_passed_to_the_host() {
    let mut engine = Engine::new();
    engine.register_host_facade(style_facade()).unwrap();
    let engine = crate::Shared::new(engine);
    let executable = engine
        .compile_executable(
            r#"
use ui::{Button, button};
fn shade(base: i64, pressed: bool) -> i64 { if pressed { base * 2 } else { base } }
pub fn view() -> Button { button().style(|base, pressed| shade(base, pressed) + base.to_string().len() as i64) }
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let authority = engine.authorize(executable).unwrap();
    let closure = authority
        .items()
        .find(|value| value.key().name().contains("{closure#"))
        .expect("the closure is an item the host can invoke")
        .clone();
    let result = authority
        .invoke(
            &closure,
            &mut [crate::Dynamic::from(5_i64), crate::Dynamic::from(true)],
        )
        .unwrap();
    assert_eq!(result.as_int().unwrap(), 10 + 1);

    let mut engine = Engine::new();
    engine.register_host_facade(style_facade()).unwrap();
    let _ = engine
        .compile_executable(
            r#"
use ui::{Button, button};
pub fn view() -> Button { button().style(|base: i64| base) }
"#,
        )
        .expect_err("Rust rejects a one-parameter closure for a two-parameter callable");
}

struct UiStyle;

/// A facade `Style` with iced-shaped field builders and `default()`.
fn struct_facade() -> crate::host::facade::Definition {
    use crate::host::{facade, item, signature, type_ref};
    let mut builder = facade::Builder::new("ui", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<UiStyle>("Style", &[], &[]).unwrap();
    let style = || type_ref::Type::registered("Style").unwrap();
    let mut default = signature::Builder::new();
    default.output(style());
    builder
        .item(item::Definition::associated_function("Style::default", default.finish()).unwrap())
        .unwrap();
    for field in ["width", "border"] {
        let mut set = signature::Builder::new();
        set.parameter(type_ref::Type::builtin(type_ref::Builtin::I64))
            .output(style());
        builder
            .item(
                item::Definition::method(
                    format!("Style::{field}"),
                    crate::receiver::Mode::Owned,
                    set.finish(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    builder.finish().unwrap()
}

#[test]
fn language_facade_struct_literals() {
    let compile = |source: &str| {
        let mut engine = Engine::new();
        engine.register_host_facade(struct_facade()).unwrap();
        engine.compile_executable(source).map(|_| ())
    };
    compile(
        r#"
use ui::Style;
pub fn update(base: Style) -> Style { let border = 3; Style { border, width: 4, ..base } }
"#,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let _ = compile("use ui::Style;\npub fn bad() -> Style { Style { width: 2 } }\n")
        .expect_err("Rust rejects missing fields without `..base`");
    let _ = compile("use ui::Style;\npub fn bad() -> Style { Style { height: 2 } }\n")
        .expect_err("Rust rejects an unknown field");
    let _ = compile("use ui::Style;\npub fn bad() -> Style { Style { width: true } }\n")
        .expect_err("Rust rejects a field of the wrong type");
}

#[test]
fn language_arrays_vectors_and_for() {
    let value = run(r#"
        pub fn run() -> i64 {
            let gaps = [8, 16];
            let mut ticks = Vec::new();
            for i in 0..3 { ticks.push(i * 10); }
            let mut more = vec![1, 2, 3];
            more[1] = 20;
            let zeros: [i64; 3] = [0; 3];
            let mut total = 0;
            for g in gaps { total += g; }
            for t in &ticks { total += t; }
            for m in more.iter() { total += *m; }
            total * 100 + ticks.len() as i64 * 10 + zeros.len() as i64 + gaps[1] * 1000
        }
        run()
    "#);
    let gaps = [8, 16];
    let ticks: Vec<i64> = (0..3).map(|i| i * 10).collect();
    let more = [1, 20, 3];
    let total: i64 =
        gaps.iter().sum::<i64>() + ticks.iter().sum::<i64>() + more.iter().sum::<i64>();
    assert_eq!(value, total * 100 + 3 * 10 + 3 + 16 * 1000);
    rejected("pub fn run() -> i64 { let a = [1, true]; 0 } run()");
    rejected("pub fn run() -> i64 { let a = vec![1]; a.push(2); 0 } run()");
    rejected("pub fn run() -> i64 { let a: [i64; 2] = [1, 2, 3]; 0 } run()");
}

#[test]
fn language_default_and_switch_are_names() {
    let value = run(r#"
        fn default(n: i64) -> i64 { n + 1 }
        fn switch(n: i64) -> i64 { n * 2 }
        pub fn run() -> i64 {
            let total = default(3) + switch(5);
            let default = total;
            let switch = default * 10;
            switch
        }
        run()
    "#);
    assert_eq!(value, 140);
}

#[test]
fn language_tuple_destructuring() {
    let value = run(r#"
        fn split(n: i64) -> (i64, bool) { (n / 10, n % 2 == 0) }
        pub fn run(n: i64) -> i64 {
            let (edge, wide) = match n { 0 => (1, true), _ if n > 5 => (n, false), _ => (2, true) };
            let (mut tens, even): (i64, bool) = split(n * 7);
            let (_, last) = split(4);
            tens += 1;
            let pair = (edge, tens);
            let score = if !wide && even && last { 100 } else { 0 };
            pair.0 * 1000 + pair.1 + score
        }
        run(6)
    "#);
    assert_eq!(value, 6 * 1000 + 5 + 100);
    rejected("pub fn run() -> i64 { let (a, b) = (1, 2, 3); a } run()");
    rejected("pub fn run() -> i64 { let (a, b) = (1, true); a + b } run()");
}

fn run_string(source: &str) -> String {
    Engine::new()
        .eval::<String>(source)
        .unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn language_option_types_and_inference() {
    let value = run(r#"
        struct Slot { value: Option<i64>, label: Option<String> }
        enum Event { Picked(Option<i64>), Cleared }
        fn find(n: i64) -> Option<i64> { if n > 0 { Some(n * 2) } else { None } }
        fn or_minus(x: Option<i64>) -> i64 { match x { Some(v) => v, None => -1 } }
        fn event(e: Event) -> i64 {
            match e { Event::Picked(Some(v)) => v, Event::Picked(None) => 0, Event::Cleared => -5 }
        }
        pub fn run() -> i64 {
            let none: Option<i64> = None;
            let mut later = None;
            later = Some(5);
            let nested: Option<Option<i64>> = Some(None);
            let deeper: Option<Option<i64>> = Some(Some(2));
            let slot = Slot { value: find(4), label: None };
            let words = Option::Some(String::from("w"));
            or_minus(none) + or_minus(later) * 10 + or_minus(slot.value) * 100
                + event(Event::Picked(Some(7))) * 1000 + event(Event::Cleared)
                + if nested.is_some() && deeper.is_some() && slot.label.is_none() && words.is_some() { 0 } else { 1 }
        }
        run()
    "#);
    assert_eq!(value, -1 + 50 + 800 + 7000 - 5);
    rejected("pub fn run() -> i64 { let x: Option<i64> = Some(true); 0 } run()");
    rejected("pub fn run() -> i64 { let x: i64 = Some(1); x } run()");
    rejected("fn f() -> Option<i64> { Some(\"s\") } pub fn run() -> i64 { 0 } run()");
    rejected("struct S { v: Option<i64, bool> } pub fn run() -> i64 { 0 } run()");
}

#[test]
fn language_option_patterns_and_exhaustiveness() {
    let value = run(r#"
        #[derive(Clone, Copy)]
        enum Shape { Dot, Circle(Option<i64>) }
        fn area(s: Shape) -> i64 {
            match s { Shape::Dot => 0, Shape::Circle(Some(r)) => r * r, Shape::Circle(None) => -1 }
        }
        fn depth(x: Option<Option<i64>>) -> i64 {
            match x { Some(Some(v)) => v, Some(None) => 1, None => 0 }
        }
        fn kind(x: Option<Shape>) -> i64 {
            match x { Some(Shape::Dot) => 1, Some(_) => 2, Option::None => 3 }
        }
        fn guarded(x: Option<i64>) -> i64 {
            match x { Some(v) if v > 10 => 2, Some(_) => 1, None => 0 }
        }
        pub fn run() -> i64 {
            area(Shape::Circle(Some(3))) + area(Shape::Circle(None)) * 10
                + depth(Some(Some(4))) * 100 + depth(Some(None)) * 1000 + depth(None)
                + kind(Some(Shape::Dot)) * 10000 + kind(Some(Shape::Circle(None))) * 100000
                + kind(None) * 1000000 + guarded(Some(11)) * 10000000 + guarded(Some(1))
        }
        run()
    "#);
    assert_eq!(
        value,
        9 - 10 + 400 + 1000 + 10000 + 200000 + 3000000 + 20000000 + 1
    );
    let missing = rejected(
        "fn f(x: Option<i64>) -> i64 { match x { Some(v) => v } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing.contains("missing None"), "{missing}");
    let nested = rejected(
        "fn f(x: Option<Option<i64>>) -> i64 { match x { Some(Some(v)) => v, None => 0 } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(nested.contains("missing Some(None)"), "{nested}");
    let unreachable = rejected(
        "fn f(x: Option<i64>) -> i64 { match x { Some(_) => 1, None => 0, Some(v) => v } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(unreachable.contains("unreachable"), "{unreachable}");
    rejected(
        "#[derive(Clone)] enum E { A } \
         fn f(x: i64) -> i64 { match Some(x) { Some(E::A) => 1, _ => 0 } } \
         pub fn run() -> i64 { 0 } run()",
    );
}

#[test]
fn language_option_if_let_and_while_let() {
    let value = run(r#"
        enum Shape { Dot, Circle(Option<i64>) }
        fn find(n: i64) -> Option<i64> { if n > 0 { Some(n) } else { None } }
        fn radius(s: Shape) -> i64 {
            if let Shape::Circle(Some(r)) = s { r } else { 0 }
        }
        pub fn run() -> i64 {
            let mut total = 0;
            if let Some(v) = find(4) { total += v; } else { total -= 100; }
            if let Some(v) = find(0) { total += v; } else if let None = find(-1) { total += 10; }
            let mut next = Some(3);
            while let Some(k) = next {
                total += k * 100;
                next = if k > 1 { Some(k - 1) } else { None };
            }
            total + radius(Shape::Circle(Some(7))) * 10000 + radius(Shape::Dot)
        }
        run()
    "#);
    assert_eq!(value, 4 + 10 + 600 + 70000);
    rejected("pub fn run() -> i64 { if let Some(v) = Some(1) { v } else { 0 }; v } run()");
}

#[test]
fn language_option_methods() {
    let value = run(r#"
        struct Picker { selected: Option<i64> }
        impl Picker {
            fn clear(&mut self) -> Option<i64> { self.selected.take() }
        }
        fn big(x: i64) -> Option<i64> { if x > 3 { Some(x) } else { None } }
        pub fn run() -> i64 {
            let a = Some(4);
            let n: Option<i64> = None;
            let mut t = Some(9);
            let got = t.take();
            let mut r = Some(1);
            let old = r.replace(5);
            let mut picker = Picker { selected: Some(8) };
            let cleared = picker.clear();
            let checks = a.is_some() && n.is_none() && a.filter(|x| *x > 10).is_none()
                && t.is_none() && picker.selected.is_none() && a.is_some_and(|x| x == 4)
                && Some(4) == a && n == None && a != n && Some(Some(1)) != Some(None);
            a.unwrap() + n.unwrap_or(10) + n.unwrap_or_else(|| 20) + n.unwrap_or_default()
                + a.map(|x| x * 3).unwrap() * 10 + a.and_then(big).unwrap_or(0) * 100
                + n.or(Some(6)).unwrap() * 1000 + n.or_else(|| Some(7)).unwrap() * 10000
                + a.as_ref().copied().unwrap() * 100000 + a.expect("present")
                + got.unwrap() + old.unwrap() + r.unwrap() + cleared.unwrap()
                + a.map_or(0, |x| x + 1) + n.map_or(50, |x| x)
                + if checks { 1000000 } else { 0 }
        }
        run()
    "#);
    assert_eq!(
        value,
        4 + 10 + 20 + 120 + 400 + 6000 + 70000 + 400000 + 4 + 9 + 1 + 5 + 8 + 5 + 50 + 1000000
    );
    let shown = run_string(
        r#"
        #[derive(Debug)]
        enum Tone { Warm }
        pub fn run() -> String {
            let a = Some(4);
            let n: Option<String> = None;
            format!("{:?} {:?} {:?} {:?} {a:?}", Some("x"), n, Some(Some(Tone::Warm)), Some((1, 'c')))
        }
        run()
    "#,
    );
    // Rawr's derived enum `Debug` keeps the enum name (`Tone::Warm`).
    assert_eq!(
        shown,
        r#"Some("x") None Some(Some(Warm)) Some((1, 'c')) Some(4)"#
    );
    let unwrapped = rejected("pub fn run() -> i64 { let n: Option<i64> = None; n.unwrap() } run()");
    assert!(
        unwrapped.contains("called `Option::unwrap()` on a `None` value"),
        "{unwrapped}"
    );
    let expected =
        rejected("pub fn run() -> i64 { let n: Option<i64> = None; n.expect(\"gone\") } run()");
    assert!(expected.contains("gone"), "{expected}");
    rejected("pub fn run() -> i64 { let a = Some(1); a.take(); 0 } run()");
    rejected("pub fn run() -> i64 { Some(1).unwrap_or(true) } run()");
    rejected("pub fn run() -> i64 { Some(1).ok_or(); 0 } run()");
}

#[test]
fn language_option_question_mark() {
    let value = run(r#"
        fn half(n: i64) -> Option<i64> { if n % 2 == 0 { Some(n / 2) } else { None } }
        fn quarter(n: i64) -> Option<i64> {
            let h = half(n)?;
            Some(half(h)?)
        }
        pub fn run() -> i64 {
            let plus = |n: i64| -> Option<i64> { Some(half(n)? + 1) };
            quarter(12).unwrap_or(-1) * 10 + quarter(6).unwrap_or(-1)
                + plus(8).unwrap() * 100 + plus(3).unwrap_or(0)
        }
        run()
    "#);
    assert_eq!(value, 30 - 1 + 500);
    rejected(
        "fn half(n: i64) -> Option<i64> { Some(n) } fn f() -> i64 { half(2)? } \
         pub fn run() -> i64 { f() } run()",
    );
    rejected("fn f(n: i64) -> Option<i64> { Some(n?) } pub fn run() -> i64 { 0 } run()");
}

#[derive(Clone)]
struct OptShade(i64);

/// A facade taking and returning Rust's `Option<T>`.
fn option_facade() -> crate::host::facade::Definition {
    use crate::host::{facade, item, signature, type_ref};
    let mut builder = facade::Builder::new("opt", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<OptShade>("Shade", &[], &[]).unwrap();
    let int = || type_ref::Type::option(type_ref::Type::builtin(type_ref::Builtin::I64));
    let shade = || type_ref::Type::option(type_ref::Type::registered("Shade").unwrap());
    let mut bump = signature::Builder::new();
    bump.parameter(int()).output(int());
    builder
        .item(item::Definition::function("bump", bump.finish()).unwrap())
        .unwrap();
    let mut darker = signature::Builder::new();
    darker.parameter(shade()).output(shade());
    builder
        .item(item::Definition::function("darker", darker.finish()).unwrap())
        .unwrap();
    let mut make = signature::Builder::new();
    make.output(type_ref::Type::registered("Shade").unwrap());
    builder
        .item(item::Definition::function("make", make.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn language_option_round_trips_through_the_host() {
    let mut engine = Engine::new();
    engine.register_host_facade(option_facade()).unwrap();
    let engine = crate::Shared::new(engine);
    let authority = engine
        .compile_authority(
            r#"
use opt::{bump, darker, make, Shade};
pub fn run() -> i64 {
    let none: Option<i64> = None;
    let shade: Option<Shade> = darker(Some(make()));
    bump(Some(4)).unwrap() * 10 + if bump(none).is_none() { 1 } else { 0 }
        + if shade.is_some() && darker(None).is_none() { 100 } else { 0 }
}
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let mut runtime = crate::host::runtime::Builder::new(option_facade());
    runtime
        .bind("opt::bump", |args| {
            let value = crate::option::cast::<i64>(args[0].take()).expect("an i64 option");
            Ok(crate::option::from(value.map(|value| value + 1)))
        })
        .unwrap();
    runtime
        .bind("opt::darker", |args| {
            let value = crate::option::cast::<OptShade>(args[0].take()).expect("a shade option");
            Ok(crate::option::from(
                value.map(|shade| OptShade(shade.0 - 1)),
            ))
        })
        .unwrap();
    runtime
        .bind("opt::make", |_| Ok(crate::Dynamic::from(OptShade(9))))
        .unwrap();
    let mut registry = crate::host::runtime::Registry::new();
    registry.install(runtime.finish()).unwrap();
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let output = authority
        .invoke_bound(&run, &mut [], crate::Shared::new(registry))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(output.as_int().unwrap(), 50 + 1 + 100);

    let mut engine = Engine::new();
    engine.register_host_facade(option_facade()).unwrap();
    engine
        .compile_executable("use opt::bump; pub fn run() -> i64 { bump(Some(true)).unwrap() }")
        .expect_err("Rust rejects `Option<bool>` where `Option<i64>` is expected");
}

#[test]
fn language_result_types_and_inference() {
    let value = run(r#"
        struct Slot { value: Result<i64, String> }
        enum Event { Parsed(Result<i64, bool>), Cleared }
        fn parse(n: i64) -> Result<i64, String> {
            if n > 0 { Ok(n * 2) } else { Err(String::from("negative")) }
        }
        fn or_minus(x: Result<i64, String>) -> i64 { match x { Ok(v) => v, Err(_) => -1 } }
        fn event(e: Event) -> i64 {
            match e {
                Event::Parsed(Ok(v)) => v,
                Event::Parsed(Err(_)) => 0,
                Event::Cleared => -5,
            }
        }
        pub fn run() -> i64 {
            let failed: Result<i64, String> = Err(String::from("x"));
            let mut later = Err(String::from("y"));
            later = Ok(5);
            let nested: Result<Option<i64>, String> = Ok(None);
            let slot = Slot { value: parse(4) };
            let qualified: Result<i64, String> = Result::Ok(3);
            or_minus(failed) + or_minus(later) * 10 + or_minus(slot.value) * 100
                + event(Event::Parsed(Ok(7))) * 1000 + event(Event::Cleared)
                + or_minus(qualified) * 10000
                + if nested.is_ok() { 0 } else { 1 }
        }
        run()
    "#);
    assert_eq!(value, -1 + 50 + 800 + 7000 - 5 + 30000);
    rejected("pub fn run() -> i64 { let x: Result<i64, String> = Ok(true); 0 } run()");
    rejected("pub fn run() -> i64 { let x: Result<i64, String> = Err(1); 0 } run()");
    rejected("pub fn run() -> i64 { let x: i64 = Ok(1); x } run()");
    rejected("struct S { v: Result<i64> } pub fn run() -> i64 { 0 } run()");
}

#[test]
fn language_result_patterns_and_exhaustiveness() {
    let value = run(r#"
        #[derive(Clone, Copy)]
        enum Shape { Dot, Circle(i64) }
        fn depth(x: Result<Option<i64>, i64>) -> i64 {
            match x { Ok(Some(v)) => v, Ok(None) => 1, Err(e) => e * 10 }
        }
        fn kind(x: Result<Shape, Shape>) -> i64 {
            match x { Ok(Shape::Dot) => 1, Ok(_) => 2, Result::Err(_) => 3 }
        }
        fn guarded(x: Result<i64, i64>) -> i64 {
            match x { Ok(v) if v > 10 => 2, Ok(_) => 1, Err(_) => 0 }
        }
        pub fn run() -> i64 {
            let mut total = depth(Ok(Some(4))) + depth(Ok(None)) * 100 + depth(Err(5)) * 1000
                + kind(Ok(Shape::Dot)) * 100000 + kind(Ok(Shape::Circle(1))) * 1000000
                + kind(Err(Shape::Dot)) * 10000000 + guarded(Ok(11)) * 100000000 + guarded(Ok(1));
            let parsed: Result<i64, String> = Ok(2);
            if let Ok(v) = parsed { total += v * 1000000000; } else { total -= 1; }
            let mut next: Result<i64, String> = Ok(3);
            while let Ok(k) = next {
                total += k * 10000000000;
                next = if k > 1 { Ok(k - 1) } else { Err(String::from("done")) };
            }
            total
        }
        run()
    "#);
    assert_eq!(
        value,
        4 + 100 + 50000 + 100000 + 2000000 + 30000000 + 200000000 + 1 + 2000000000 + 60000000000
    );
    let missing = rejected(
        "fn f(x: Result<i64, i64>) -> i64 { match x { Ok(v) => v } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing.contains("missing Err(_)"), "{missing}");
    let nested = rejected(
        "fn f(x: Result<Option<i64>, i64>) -> i64 { match x { Ok(Some(v)) => v, Err(e) => e } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(nested.contains("missing Ok(None)"), "{nested}");
    let unreachable = rejected(
        "fn f(x: Result<i64, i64>) -> i64 { match x { Ok(_) => 1, Err(_) => 0, Ok(v) => v } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(unreachable.contains("unreachable"), "{unreachable}");
    rejected(
        "fn f(x: Option<i64>) -> i64 { match x { Ok(v) => v, _ => 0 } } \
         pub fn run() -> i64 { 0 } run()",
    );
    rejected("pub fn run() -> i64 { if let Ok(v) = Ok::<i64, i64>(1) { v } else { 0 }; v } run()");
}

#[test]
fn language_result_methods() {
    let value = run(r#"
        fn big(x: i64) -> Result<i64, String> {
            if x > 3 { Ok(x) } else { Err(String::from("small")) }
        }
        pub fn run() -> i64 {
            let a: Result<i64, String> = Ok(4);
            let e: Result<i64, String> = Err(String::from("bad"));
            let none: Option<i64> = None;
            let checks = a.is_ok() && e.is_err() && a.is_ok_and(|x| x == 4)
                && e.is_err_and(|m| m.len() == 3) && !e.is_ok_and(|x| x == 4)
                && a.ok() == Some(4) && e.ok().is_none() && a.err().is_none()
                && e.err().unwrap() == "bad" && a.clone() == Ok(4) && a != e
                && e == Err(String::from("bad"));
            a.unwrap() + e.unwrap_or(10) + e.unwrap_or_else(|m| m.len() as i64 * 10)
                + e.unwrap_or_default() + a.map(|x| x * 3).unwrap() * 10
                + a.and_then(big).unwrap_or(0) * 100
                + e.and_then(big).unwrap_or(5) * 1000
                + e.map_err(|m| m.len() as i64).unwrap_err() * 10000
                + e.or_else(|_m| -> Result<i64, i64> { Ok(6) }).unwrap() * 100000
                + if a.as_ref().is_ok() { 1 } else { 0 } + a.expect("present")
                + e.unwrap_err().len() as i64
                + Some(9).ok_or(String::from("none")).unwrap()
                + none.ok_or_else(|| 7).unwrap_err()
                + if checks { 1000000 } else { 0 }
        }
        run()
    "#);
    assert_eq!(
        value,
        4 + 10 + 30 + 0 + 120 + 400 + 5000 + 30000 + 600000 + 1 + 4 + 3 + 9 + 7 + 1000000
    );
    let shown = run_string(
        r#"
        #[derive(Debug)]
        enum Tone { Warm }
        pub fn run() -> String {
            let a: Result<i64, String> = Ok(4);
            let e: Result<i64, String> = Err(String::from("x"));
            let n: Result<Option<Tone>, (i64, char)> = Err((1, 'c'));
            let t: Result<Option<Tone>, (i64, char)> = Ok(Some(Tone::Warm));
            format!("{:?} {:?} {:?} {t:?}", a, e, n)
        }
        run()
    "#,
    );
    assert_eq!(shown, r#"Ok(4) Err("x") Err((1, 'c')) Ok(Some(Warm))"#);
    let unwrapped = rejected(
        "pub fn run() -> i64 { let e: Result<i64, String> = Err(String::from(\"boom\")); e.unwrap() } run()",
    );
    assert!(
        unwrapped.contains(r#"called `Result::unwrap()` on an `Err` value: "boom""#),
        "{unwrapped}"
    );
    let expected = rejected(
        "pub fn run() -> i64 { let e: Result<i64, i64> = Err(3); e.expect(\"gone\") } run()",
    );
    assert!(expected.contains("gone: 3"), "{expected}");
    let unwrapped_err =
        rejected("pub fn run() -> i64 { let a: Result<i64, i64> = Ok(3); a.unwrap_err() } run()");
    assert!(
        unwrapped_err.contains("called `Result::unwrap_err()` on an `Ok` value: 3"),
        "{unwrapped_err}"
    );
    rejected("pub fn run() -> i64 { let a: Result<i64, i64> = Ok(1); a.unwrap_or(true) } run()");
    rejected(
        "pub fn run() -> i64 { let a: Result<i64, i64> = Ok(1); a.map_err(|e: bool| e); 0 } run()",
    );
    rejected("pub fn run() -> i64 { let a: Result<i64, i64> = Ok(1); a.missing() } run()");
}

#[test]
fn language_result_question_mark() {
    let value = run(r#"
        fn half(n: i64) -> Result<i64, String> {
            if n % 2 == 0 { Ok(n / 2) } else { Err(format!("{n} is odd")) }
        }
        fn quarter(n: i64) -> Result<i64, String> {
            let h = half(n)?;
            Ok(half(h)?)
        }
        fn length(n: i64) -> i64 {
            match quarter(n) { Ok(v) => v, Err(message) => -(message.len() as i64) }
        }
        pub fn run() -> i64 {
            let plus = |n: i64| -> Result<i64, String> { Ok(half(n)? + 1) };
            let inferred = |n: i64| { let h = half(n)?; Ok(h * 2) };
            length(12) * 10 + length(6)
                + plus(8).unwrap() * 100 + plus(3).unwrap_or(0)
                + inferred(4).unwrap() * 1000
        }
        run()
    "#);
    assert_eq!(value, 30 - 8 + 500 + 4000);
    // `?` on a `Result` needs a body returning `Result` with the same error:
    // Rawr has no `From` conversion.
    let mixed = rejected(
        "fn half(n: i64) -> Result<i64, String> { Ok(n) } \
         fn f() -> Option<i64> { Some(half(2)?) } pub fn run() -> i64 { 0 } run()",
    );
    assert!(mixed.contains("Result<_, String>"), "{mixed}");
    rejected(
        "fn half(n: i64) -> Option<i64> { Some(n) } \
         fn f() -> Result<i64, String> { Ok(half(2)?) } pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "fn half(n: i64) -> Result<i64, i64> { Ok(n) } \
         fn f() -> Result<i64, String> { Ok(half(2)?) } pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "fn half(n: i64) -> Result<i64, i64> { Ok(n) } fn f() -> i64 { half(2)? } \
         pub fn run() -> i64 { f() } run()",
    );
}

#[derive(Clone)]
struct ResShade(i64);

/// A facade taking and returning Rust's `Result<T, E>`.
fn result_facade() -> crate::host::facade::Definition {
    use crate::host::{facade, item, signature, type_ref};
    let mut builder = facade::Builder::new("res", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<ResShade>("Shade", &[], &[]).unwrap();
    let parsed = || {
        type_ref::Type::result(
            type_ref::Type::builtin(type_ref::Builtin::I64),
            type_ref::Type::builtin(type_ref::Builtin::String),
        )
    };
    let shade = || {
        type_ref::Type::result(
            type_ref::Type::registered("Shade").unwrap(),
            type_ref::Type::builtin(type_ref::Builtin::Bool),
        )
    };
    let mut bump = signature::Builder::new();
    bump.parameter(parsed()).output(parsed());
    builder
        .item(item::Definition::function("bump", bump.finish()).unwrap())
        .unwrap();
    let mut darker = signature::Builder::new();
    darker.parameter(shade()).output(shade());
    builder
        .item(item::Definition::function("darker", darker.finish()).unwrap())
        .unwrap();
    let mut make = signature::Builder::new();
    make.output(type_ref::Type::registered("Shade").unwrap());
    builder
        .item(item::Definition::function("make", make.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn language_result_round_trips_through_the_host() {
    let mut engine = Engine::new();
    engine.register_host_facade(result_facade()).unwrap();
    let engine = crate::Shared::new(engine);
    let authority = engine
        .compile_authority(
            r#"
use res::{bump, darker, make, Shade};
pub fn run() -> i64 {
    let failed: Result<i64, String> = Err(String::from("abc"));
    let shade: Result<Shade, bool> = darker(Ok(make()));
    bump(Ok(4)).unwrap() * 10 + bump(failed).unwrap_err().len() as i64
        + if shade.is_ok() && darker(Err(true)).err() == Some(false) { 100 } else { 0 }
}
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let mut runtime = crate::host::runtime::Builder::new(result_facade());
    runtime
        .bind("res::bump", |args| {
            let value = crate::result::cast::<i64, crate::ImmutableString>(args[0].take())
                .expect("an i64 result");
            Ok(crate::result::from(
                value
                    .map(|value| value + 1)
                    .map_err(|error| format!("{error}!")),
            ))
        })
        .unwrap();
    runtime
        .bind("res::darker", |args| {
            let value =
                crate::result::cast::<ResShade, bool>(args[0].take()).expect("a shade result");
            Ok(crate::result::from(
                value
                    .map(|shade| ResShade(shade.0 - 1))
                    .map_err(|flag| !flag),
            ))
        })
        .unwrap();
    runtime
        .bind("res::make", |_| Ok(crate::Dynamic::from(ResShade(9))))
        .unwrap();
    let mut registry = crate::host::runtime::Registry::new();
    registry.install(runtime.finish()).unwrap();
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let output = authority
        .invoke_bound(&run, &mut [], crate::Shared::new(registry))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(output.as_int().unwrap(), 50 + 4 + 100);

    let mut engine = Engine::new();
    engine.register_host_facade(result_facade()).unwrap();
    engine
        .compile_executable("use res::bump; pub fn run() -> i64 { bump(Ok(true)).unwrap() }")
        .expect_err("Rust rejects `Result<bool, _>` where `Result<i64, String>` is expected");
}

#[test]
fn language_match_catch_all_binding_arm() {
    let value = run(r#"
        #[derive(Clone, Copy, PartialEq)]
        enum Shape { Dot, Circle(i64), Square(i64) }
        fn area(s: Shape) -> i64 {
            match s {
                Shape::Dot => 0,
                other if other == Shape::Circle(2) => 12,
                other => match other { Shape::Circle(r) => r * r * 3, Shape::Square(w) => w * w, Shape::Dot => -1 },
            }
        }
        fn first(x: Option<i64>) -> i64 {
            match x { Some(v) if v > 10 => v, rest => rest.unwrap_or(0) + 1 }
        }
        fn shown(x: Result<i64, String>) -> String {
            match x { Err(_) => String::from("failed"), whole => format!("{whole:?}") }
        }
        pub fn run() -> i64 {
            area(Shape::Dot) + area(Shape::Circle(2)) * 10 + area(Shape::Circle(1)) * 100
                + area(Shape::Square(3)) * 1000 + first(Some(11)) * 100000 + first(Some(4))
                + first(None) + shown(Ok(5)).len() as i64 * 10000000
        }
        run()
    "#);
    assert_eq!(value, 120 + 300 + 9000 + 1100000 + 5 + 1 + 50000000);
    let after = rejected(
        "#[derive(Clone)] enum E { A, B } \
         fn f(e: E) -> i64 { match e { other => 1, E::A => 2 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(after.contains("unreachable"), "{after}");
    rejected(
        "#[derive(Clone)] enum E { A, B } \
         fn f(e: E) -> i64 { match e { E::A => 1, other => 2 }; other } pub fn run() -> i64 { 0 } run()",
    );
}

#[test]
fn language_patterns_literals_ranges_tuples_and_bindings() {
    let value = run(r#"
        #[derive(Clone, Copy)]
        enum Shape { Dot, Circle(f64), Square(i64) }
        fn code(x: Result<i64, i64>) -> i64 {
            match x { Ok(0) => 1, Ok(1 | 2) => 2, Ok(n @ 3..=9) => n * 10, Ok(_) => 3, Err(-1) => 4, Err(_) => 5 }
        }
        fn letter(c: Option<char>) -> i64 {
            match c { Some('a') => 1, Some('b'..='z') => 2, Some(_) => 3, None => 0 }
        }
        fn word(w: Option<&str>) -> i64 {
            match w { Some("x") => 1, Some(other) => other.len() as i64, None => 0 }
        }
        fn owned(w: Option<String>) -> i64 {
            if let Some("hi") = w { 1 } else { 0 }
        }
        fn shape(s: Shape) -> i64 {
            match s { Shape::Circle(1.0) => 1, Shape::Circle(_) => 2, Shape::Square(n @ (4 | 9)) => n, Shape::Square(..=0) => -1, Shape::Square(_) | Shape::Dot => 0 }
        }
        fn pair(p: (i64, Option<i64>)) -> i64 {
            match p { (0, _) => 1, (n, Some(m)) if n == m => 2, (n, Some(m)) => n + m, (_, None) => 3 }
        }
        fn nested(p: ((i64, bool), char)) -> i64 {
            match p { ((a, true), 'y') => a, ((a, false), _) => -a, ((_, true), _) => 0 }
        }
        fn swap((x, y): (i64, i64)) -> (i64, i64) { (y, x) }
        fn digit(n: i64) -> i64 {
            match n { d @ 0..=9 => d, 10..100 => 2, 100.. => 3, _ => -1 }
        }
        pub fn run() -> i64 {
            let ((a, b), mut c) = ((1, 2), 3);
            c += a + b;
            let pairs = vec![(1, 10), (2, 20)];
            let mut total = 0;
            for (i, x) in pairs { total += i * x; }
            let add = |(l, r): (i64, i64)| l + r;
            let firsts = [(5, 'a'), (6, 'b')];
            let mut sum = 0;
            for &(n, _) in firsts.iter() { sum += n; }
            let whole @ (p, q) = (7, 8);
            let (s0, s1) = swap((1, 2));
            code(Ok(0)) + code(Ok(2)) * 10 + code(Ok(5)) * 100 + code(Ok(50)) + code(Err(-1)) + code(Err(7))
                + letter(Some('a')) + letter(Some('q')) * 10 + letter(Some('!')) * 100 + letter(None)
                + word(Some("x")) + word(Some("abcd")) * 10 + word(None) + owned(Some(String::from("hi")))
                + shape(Shape::Circle(1.0)) + shape(Shape::Circle(2.5)) * 10 + shape(Shape::Square(9)) * 100
                + shape(Shape::Square(-3)) + shape(Shape::Dot)
                + pair((0, None)) + pair((4, Some(4))) * 10 + pair((1, Some(5))) * 100 + pair((1, None))
                + nested(((5, true), 'y')) + nested(((6, false), 'n')) + nested(((7, true), 'n'))
                + c + total + add((3, 4)) + sum + whole.0 + p + q + s0 * 10 + s1
                + digit(7) + digit(42) * 10 + digit(500) * 100 + digit(-4)
        }
        run()
    "#);
    let expected = (1 + 2 * 10 + 50 * 100 + 3 + 4 + 5)
        + (1 + 2 * 10 + 3 * 100)
        + (1 + 4 * 10 + 1)
        + (1 + 2 * 10 + 9 * 100 - 1)
        + (1 + 2 * 10 + 6 * 100 + 3)
        + (5 - 6)
        + 6
        + 50
        + 7
        + 11
        + 7
        + 7
        + 8
        + 21
        + (7 + 2 * 10 + 3 * 100 - 1);
    assert_eq!(value, expected);
    // Rust's rules: exhaustiveness, refutability, one type per pattern.
    let missing = rejected(
        "fn f(x: Option<i64>) -> i64 { match x { Some(0) => 0, None => 1 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing.contains("missing Some(_)"), "{missing}");
    let tuple = rejected(
        "fn f(p: (bool, bool)) -> i64 { match p { (true, _) => 0, (false, true) => 1 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(tuple.contains("missing (false, false)"), "{tuple}");
    rejected("pub fn run() -> i64 { let Some(x) = Some(1); x } run()");
    rejected("fn f(x: Option<i64>) -> i64 { match x { Some('a') => 0, _ => 1 } } pub fn run() -> i64 { 0 } run()");
    rejected("fn f(x: Option<i64>) -> i64 { match x { Some(a) | None => 0 } } pub fn run() -> i64 { 0 } run()");
    rejected(
        "fn f(x: i64) -> i64 { match x { 9..=1 => 0, _ => 1 } } pub fn run() -> i64 { 0 } run()",
    );
    rejected("pub fn run() -> i64 { let (a, b) = (1, 2, 3); a } run()");
    rejected("pub fn run() -> i64 { let &x = 1; x } run()");
}

#[test]
fn language_constructors_as_values_and_turbofish() {
    let value = run(r#"
        #[derive(Clone, Copy)]
        enum Shape { Dot, Circle(i64), Rect(i64, i64) }
        fn area(s: Shape) -> i64 {
            match s { Shape::Dot => 0, Shape::Circle(r) => r * r, Shape::Rect(w, h) => w * h }
        }
        fn double(n: i64) -> i64 { n * 2 }
        pub fn run() -> i64 {
            let some = Some(3).map(Some);
            let circle = Some(4).map(Shape::Circle);
            let rect = Shape::Rect;
            let ok: Result<i64, String> = Some(5).ok_or(String::from("none")).map(double);
            let wrapped = Ok::<i64, String>(6);
            let none = None::<i64>;
            let err = Err::<i64, i64>(7);
            let wrap: fn(i64) -> Option<i64> = Some;
            let made = wrap(8);
            let e = Some(9).map(Err).unwrap();
            let failed: Result<i64, i64> = e;
            let shapes = vec![Shape::Dot, Shape::Circle(2)];
            some.unwrap().unwrap() + area(circle.unwrap()) * 10 + area(rect(2, 3)) * 100
                + ok.unwrap() * 1000 + wrapped.unwrap() * 10000 + none.unwrap_or(1)
                + err.unwrap_err() + made.unwrap() + failed.unwrap_err() + area(shapes[1])
        }
        run()
    "#);
    assert_eq!(value, 3 + 160 + 600 + 10000 + 60000 + 1 + 7 + 8 + 9 + 4);
    rejected("pub fn run() -> i64 { let x = Ok::<i64, String>(true); 0 } run()");
    rejected("pub fn run() -> i64 { let x = None::<i64, i64>; 0 } run()");
    rejected("pub fn run() -> i64 { Some(1).map(Some).unwrap() } run()");
}

#[test]
fn language_from_into_and_question_mark() {
    let value = run(r#"
        #[derive(Debug, PartialEq)]
        enum AppError { Parse(String), Range(i64), Empty }
        impl From<String> for AppError {
            fn from(message: String) -> Self { AppError::Parse(message) }
        }
        impl From<i64> for AppError {
            fn from(n: i64) -> AppError { AppError::Range(n) }
        }
        struct Meters { value: i64 }
        impl From<i64> for Meters {
            fn from(value: i64) -> Self { Meters { value } }
        }
        impl Meters {
            fn double(&self) -> i64 { self.value * 2 }
        }
        fn parse(text: &str) -> Result<i64, String> {
            if text.len() > 2 { Err(String::from("long")) } else { Ok(text.len() as i64) }
        }
        fn check(n: i64) -> Result<i64, i64> { if n > 5 { Err(n) } else { Ok(n) } }
        fn run_all(text: &str) -> Result<i64, AppError> {
            let n = parse(text)?;
            let m = check(n * 3)?;
            if m == 0 { Err(AppError::Empty) } else { Ok(m) }
        }
        fn code(r: Result<i64, AppError>) -> i64 {
            match r {
                Ok(v) => v,
                Err(AppError::Parse(m)) => 100 + m.len() as i64,
                Err(AppError::Range(n)) => 200 + n,
                Err(AppError::Empty) => 300,
            }
        }
        fn length(m: Meters) -> i64 { m.value }
        pub fn run() -> i64 {
            let meters: Meters = 5.into();
            let direct = Meters::from(7);
            let error: AppError = String::from("x").into();
            let wrapped: Result<i64, AppError> = Err(4.into());
            let same: i64 = 3.into();
            code(run_all("a")) + code(run_all("abc")) * 10 + code(run_all("ab")) * 1000
                + meters.double() * 100000 + direct.value * 1000000 + length(9.into()) * 10000000
                + if error == AppError::Parse(String::from("x")) { 100000000 } else { 0 }
                + code(wrapped) + same
        }
        run()
    "#);
    assert_eq!(
        value,
        3 + 1040 + 206000 + 1000000 + 7000000 + 90000000 + 100000000 + 204 + 3
    );
    // Rust's rules: a conversion needs its `impl From`.
    rejected("struct M { v: i64 } pub fn run() -> i64 { let m: M = 5.into(); m.v } run()");
    rejected(
        "struct M { v: i64 } impl From<i64> for M { fn from(v: i64) -> M { M { v } } } \
         pub fn run() -> i64 { let m: M = true.into(); m.v } run()",
    );
    rejected(
        "struct M { v: i64 } impl From<i64> for M { fn from(v: bool) -> M { M { v: 1 } } } \
         pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "struct M { v: i64 } impl Hash for M { fn hash(&self) -> i64 { 1 } } \
         pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "fn f() -> Result<i64, bool> { Ok(1) } fn g() -> Result<i64, String> { Ok(f()?) } \
         pub fn run() -> i64 { 0 } run()",
    );
}

#[test]
fn language_vec_in_signatures_fields_and_payloads() {
    let value = run(r#"
        #[derive(Clone, Debug, Default, PartialEq)]
        struct Bag { items: Vec<i64>, names: Vec<String> }
        #[derive(Clone)]
        enum Event { Batch(Vec<i64>), Nothing }
        fn total(v: &Vec<i64>) -> i64 {
            let mut t = 0;
            for x in v.iter() { t += *x; }
            t
        }
        fn first(v: Vec<i64>) -> Option<i64> { if v.is_empty() { None } else { Some(v[0]) } }
        fn evens(n: i64) -> Vec<i64> {
            let mut out = Vec::new();
            for i in 0..n { if i % 2 == 0 { out.push(i); } }
            out
        }
        fn size(e: Event) -> i64 { match e { Event::Batch(v) => v.len() as i64, Event::Nothing => 0 } }
        pub fn run() -> i64 {
            let bag = Bag { items: vec![1, 2, 3], names: vec![String::from("a")] };
            let empty = Bag::default();
            let copy = bag.clone();
            total(&bag.items) + first(evens(5)).unwrap() * 10 + evens(7).len() as i64 * 100
                + size(Event::Batch(vec![4, 5])) * 1000 + size(Event::Nothing)
                + empty.items.len() as i64 * 10000 + if copy == bag { 100000 } else { 0 }
                + bag.names.len() as i64 * 1000000
        }
        run()
    "#);
    assert_eq!(value, 6 + 0 + 400 + 2000 + 0 + 0 + 100000 + 1000000);
    rejected("fn f(v: Vec<i64>) -> i64 { v.len() } pub fn run() -> i64 { f(vec![true]) } run()");
    rejected("struct S { v: Vec<i64, i64> } pub fn run() -> i64 { 0 } run()");
}

#[test]
fn language_generic_functions() {
    let value = run(r#"
        #[derive(Clone, Debug, PartialEq)]
        enum Tone { Warm, Cool }
        fn first<T: Clone>(v: &Vec<T>) -> Option<T> {
            if v.is_empty() { None } else { Some(v[0].clone()) }
        }
        fn same<T: PartialEq>(a: T, b: T) -> bool { a == b }
        fn show<T: std::fmt::Debug>(x: T) -> String { format!("<{:?}>", x) }
        fn swap<A, B>((a, b): (A, B)) -> (B, A) { (b, a) }
        fn id<T>(x: T) -> T { x }
        fn count<T>(v: Vec<T>) -> i64 where T: Clone { v.len() as i64 }
        fn wrap<T>(x: T) -> Vec<T> { vec![x] }
        pub fn run() -> i64 {
            let a = first(&vec![3, 4]).unwrap();
            let s = first(&vec![String::from("xy")]).unwrap();
            let none: Option<i64> = first(&Vec::new());
            let (n, t) = swap((true, 5));
            let f = id::<i64>(7);
            let tone = first(&vec![Tone::Cool]).unwrap();
            let shown = show(Some(Tone::Warm));
            a + s.len() as i64 * 10 + n * 100 + f * 1000 + count(vec![1, 2, 3]) * 10000
                + wrap(4)[0] * 2 * 100000
                + if same(1, 1) && !same(String::from("a"), String::from("b")) && t
                    && same(tone, Tone::Cool) && none.is_none() { 1000000 } else { 0 }
                + shown.len() as i64 * 10000000
        }
        run()
    "#);
    assert_eq!(
        value,
        3 + 20 + 500 + 7000 + 30000 + 800000 + 1000000 + "<Some(Warm)>".len() as i64 * 10000000
    );
    // Rust's rules: bounds hold at each use and limit the body.
    let unbounded =
        rejected("fn eq<T>(a: T, b: T) -> bool { a == b } pub fn run() -> i64 { 0 } run()");
    assert!(unbounded.contains("PartialEq"), "{unbounded}");
    rejected("fn dup<T>(a: &T) -> T { a.clone() } pub fn run() -> i64 { 0 } run()");
    let missing = rejected(
        "struct Plain { v: i64 } fn dup<T: Clone>(a: &T) -> T { a.clone() } \
         pub fn run() -> i64 { dup(&Plain { v: 1 }).v } run()",
    );
    assert!(missing.contains("Clone"), "{missing}");
    rejected("fn id<T>(x: T) -> i64 { x } pub fn run() -> i64 { id(1) } run()");
    rejected("fn id<T>(x: T) -> T { x } pub fn run() -> i64 { let s: String = id(1); 0 } run()");
    rejected("fn id<T>(x: T) -> T { x } pub fn run() -> i64 { id::<i64, i64>(1) } run()");
    rejected("fn f<T: Hasher>(x: T) -> i64 { 0 } pub fn run() -> i64 { 0 } run()");
}

#[test]
fn language_generic_structs_enums_and_impls() {
    let value = run(r#"
        #[derive(Clone, Debug, PartialEq)]
        struct Pair<T> { a: T, b: T }
        #[derive(Clone, Debug)]
        enum Tree<T> { Leaf(T), Node(Vec<Tree<T>>), Empty }
        impl<T: Clone> Pair<T> {
            fn new(a: T, b: T) -> Self { Pair { a, b } }
            fn first(&self) -> T { self.a.clone() }
            fn swap(self) -> Pair<T> { Pair { a: self.b, b: self.a } }
            fn paired<U>(&self, other: U) -> (T, U) { (self.b.clone(), other) }
        }
        fn sum(t: &Tree<i64>) -> i64 {
            match t {
                Tree::Leaf(v) => *v,
                Tree::Node(children) => { let mut s = 0; for c in children.iter() { s += sum(c); } s }
                Tree::Empty => 0,
            }
        }
        fn leaves<T>(t: Tree<T>) -> i64 { match t { Tree::Leaf(_) => 1, Tree::Node(c) => c.len() as i64, Tree::Empty => 0 } }
        fn opt(o: &Option<i64>) -> i64 { match o { Some(v) => *v, None => 0 } }
        pub fn run() -> i64 {
            let p = Pair { a: 1, b: 2 };
            let q = Pair::new(String::from("x"), String::from("yy"));
            let t = Tree::Node(vec![Tree::Leaf(3), Tree::Leaf(4), Tree::Empty]);
            let r: Pair<i64> = p.clone().swap();
            let (_, lengths) = q.paired(2);
            let shown = format!("{:?}", p);
            let leaf = Some(5).map(Tree::Leaf);
            p.a + p.first() * 10 + q.swap().a.len() as i64 * 100 + sum(&t) * 1000 + leaves(t) * 10000
                + r.a * 100000 + if p == (Pair { a: 1, b: 2 }) { 1000000 } else { 0 }
                + shown.len() as i64 * 10000000 + lengths * 1000000000 + leaves(leaf.unwrap())
                + opt(&Some(6)) * 10000000000
        }
        run()
    "#);
    assert_eq!(
        value,
        1 + 10 + 200 + 7000 + 30000 + 200000 + 1000000 + 190000000 + 2000000000 + 1 + 60000000000
    );
    // Rust's rules: arity, one type per parameter, bounds on derives.
    let arity = rejected(
        "struct Pair<T> { a: T } struct S { p: Pair<i64, i64> } pub fn run() -> i64 { 0 } run()",
    );
    assert!(arity.contains("Pair"), "{arity}");
    rejected("struct Pair<T> { a: T } fn f(p: Pair) -> i64 { 0 } pub fn run() -> i64 { 0 } run()");
    rejected("struct Pair<T> { a: T, b: T } pub fn run() -> i64 { let p = Pair { a: 1, b: true }; 0 } run()");
    rejected("struct Pair<T> { a: T } pub fn run() -> i64 { let p: Pair<i64> = Pair { a: true }; 0 } run()");
    let derive = rejected(
        "#[derive(PartialEq)] struct Pair<T> { a: T } struct Plain { v: i64 } \
         pub fn run() -> i64 { if Pair { a: Plain { v: 1 } } == Pair { a: Plain { v: 1 } } { 1 } else { 0 } } run()",
    );
    let _ = derive;
    let bound = rejected(
        "struct Pair<T> { a: T } impl<T: Clone> Pair<T> { fn get(&self) -> T { self.a.clone() } } \
         struct Plain { v: i64 } pub fn run() -> i64 { let p = Pair { a: Plain { v: 1 } }; p.get().v } run()",
    );
    assert!(bound.contains("Clone"), "{bound}");
}

#[test]
fn language_explicit_return() {
    let value = run(r#"
        struct Counter { n: i64 }
        impl Counter {
            fn capped(&self, cap: i64) -> i64 {
                if self.n > cap { return cap; }
                self.n
            }
        }
        fn first_even(values: Vec<i64>) -> Option<i64> {
            for v in values {
                if v % 2 == 0 { return Some(v); }
            }
            None
        }
        fn sign(n: i64) -> i64 {
            match n {
                0 => return 0,
                n if n < 0 => { return -1; }
                _ => {}
            }
            loop { return 1; }
        }
        fn nothing(n: i64) {
            if n > 0 { return; }
            return
        }
        pub fn run() -> i64 {
            let early = |x: i64| { if x > 10 { return 10; } x * 2 };
            let annotated = |x: i64| -> i64 { return x + 1; };
            nothing(3);
            let c = Counter { n: 50 };
            c.capped(20) + first_even(vec![3, 5, 8, 9]).unwrap() + sign(-4) + sign(0)
                + sign(9) + early(3) + early(30) + annotated(1)
        }
        run()
    "#);
    assert_eq!(value, 20 + 8 - 1 + 0 + 1 + 6 + 10 + 2);
    rejected("pub fn run() -> i64 { return \"no\"; } run()");
    rejected("pub fn run() -> i64 { if true { return; } 1 } run()");
    rejected("fn f(n: i64) { return n; } pub fn run() -> i64 { f(1); 0 } run()");
    rejected("pub fn run() -> i64 { let f = |x: i64| -> i64 { return true; }; f(1) } run()");
}

#[test]
fn language_fn_types_in_item_headers() {
    let value = run(r#"
        struct Op { name: String, f: fn(i64) -> i64 }
        enum Step { Map(fn(i64) -> i64), Stop }
        fn apply<T, U>(x: T, f: fn(T) -> U) -> U { f(x) }
        fn twice(f: impl Fn(i64) -> i64, x: i64) -> i64 { f(f(x)) }
        fn compose<F>(f: F, x: i64) -> i64 where F: Fn(i64) -> i64 { f(x) + 1 }
        fn by_ref(f: &dyn Fn(i64) -> i64, x: i64) -> i64 { f(x) }
        fn adder() -> fn(i64) -> i64 { |x| x + 100 }
        fn inc(x: i64) -> i64 { x + 1 }
        impl Op {
            fn run(&self, x: i64) -> i64 { (self.f)(x) }
        }
        pub fn run() -> i64 {
            let op = Op { name: "inc".to_string(), f: inc };
            let step = Step::Map(|x| x * 3);
            let mapped = match step { Step::Map(g) => g(2), Step::Stop => 0 };
            let len = apply(true, |b: bool| if b { 3 } else { 0 });
            let k = 5;
            op.run(1) + mapped + len + apply(4, inc) + twice(|x| x * k, 1)
                + compose(inc, 1) + by_ref(&inc, 10) + adder()(1)
        }
        run()
    "#);
    assert_eq!(value, 2 + 6 + 3 + 5 + 25 + 3 + 11 + 101);
    rejected(
        "fn apply(f: fn(i64) -> i64) -> i64 { f(true) } pub fn run() -> i64 { apply(|x| x) } run()",
    );
    rejected("fn apply(f: fn(i64) -> i64) -> i64 { f(1) } pub fn run() -> i64 { apply(|x: bool| 1) } run()");
    rejected("#[derive(PartialEq)] struct S { f: fn() } pub fn run() -> i64 { 0 } run()");
    // `impl Clone` is an anonymous generic parameter, as in Rust; its
    // bound still holds at each call.
    assert_eq!(run("fn apply(f: impl Clone) -> i64 { 0 } pub fn run() -> i64 { apply(1) } run()"), 0);
    rejected(
        "struct S; fn apply(f: impl Clone) -> i64 { 0 } pub fn run() -> i64 { apply(S) } run()",
    );
}

#[test]
fn language_script_only_reserved_words_are_names() {
    let value = run(r#"
        struct Config { with: i64, module: i64, public: bool }
        fn with(n: i64) -> i64 { n + 1 }
        fn module(n: i64) -> i64 { n * 2 }
        fn spawn(case: i64, var: i64) -> i64 { case - var }
        pub fn run() -> i64 {
            let config = Config { with: 3, module: 4, public: true };
            let is = with(config.with) + module(config.module);
            let null = spawn(10, 4);
            let (shared, sync, thread, void, nil, package, protected) = (1, 2, 3, 4, 5, 6, 7);
            let total = is + null + shared + sync + thread + void + nil + package + protected;
            if config.public { total } else { 0 }
        }
        run()
    "#);
    assert_eq!(value, 4 + 8 + 6 + 28);
}

#[test]
fn language_traits_declarations_and_impls() {
    let value = run(r#"
        trait Shape {
            fn area(&self) -> i64;
            fn name(&self) -> String { "shape".to_string() }
            fn describe(&self) -> String { format!("{} {}", self.name(), self.area()) }
        }
        trait Grow { fn grow(&mut self, by: i64); fn unit() -> Self; }
        struct Circle { r: i64 }
        struct Square { side: i64 }
        enum Blob { Small, Big(i64) }
        impl Circle {
            fn name(&self) -> String { "inherent".to_string() }
        }
        impl Shape for Circle {
            fn area(&self) -> i64 { 3 * self.r * self.r }
        }
        impl Shape for Square {
            fn area(&self) -> i64 { self.side * self.side }
            fn name(&self) -> String { "square".to_string() }
        }
        impl Shape for Blob {
            fn area(&self) -> i64 { match self { Blob::Small => 1, Blob::Big(a) => *a } }
        }
        impl Grow for Square {
            fn grow(&mut self, by: i64) { self.side += by; }
            fn unit() -> Self { Square { side: 1 } }
        }
        pub fn run() -> i64 {
            let c = Circle { r: 2 };
            let mut s = Square { side: 3 };
            s.grow(1);
            let u = Square::unit();
            let shown = c.describe();
            (c.area()) + (s.area()) * 100 + (u.area()) * 10000
                + (Blob::Big(7).area()) * 100000
                + c.name().len() as i64 * 1000000 + s.name().len() as i64 * 100000000
                + Blob::Small.name().len() as i64 * 10000000000
                + shown.len() as i64 * 1000000000000
        }
        run()
    "#);
    assert_eq!(
        value,
        12 + 1600
            + 10000
            + 700000
            + 8000000
            + 600000000
            + 50000000000
            + "shape 12".len() as i64 * 1000000000000
    );
    // Rust's rules: every required item, nothing extra, the trait's signatures.
    let missing = rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } impl Shape for C {} \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing.contains("area"), "{missing}");
    let extra = rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } \
         impl Shape for C { fn area(&self) -> i64 { 1 } fn more(&self) -> i64 { 2 } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(extra.contains("not a member"), "{extra}");
    let signature = rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } \
         impl Shape for C { fn area(&self) -> bool { true } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(signature.contains("incompatible"), "{signature}");
    rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } \
         impl Shape for C { fn area(self) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    rejected("struct C { r: i64 } impl Missing for C {} pub fn run() -> i64 { 0 } run()");
    rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } struct D { r: i64 } \
         impl Shape for C { fn area(&self) -> i64 { 1 } } \
         pub fn run() -> i64 { let d = D { r: 1 }; d.area() } run()",
    );
}

#[test]
fn language_traits_as_generic_bounds() {
    let value = run(r#"
        trait Shape {
            fn area(&self) -> i64;
            fn name(&self) -> String { "shape".to_string() }
            fn scale(&mut self, by: i64);
        }
        trait Label { fn label(&self) -> String; }
        #[derive(Clone)]
        struct Circle { r: i64 }
        #[derive(Clone)]
        struct Square { side: i64 }
        impl Shape for Circle {
            fn area(&self) -> i64 { 3 * self.r * self.r }
            fn scale(&mut self, by: i64) { self.r *= by; }
        }
        impl Shape for Square {
            fn area(&self) -> i64 { self.side * self.side }
            fn name(&self) -> String { "square".to_string() }
            fn scale(&mut self, by: i64) { self.side *= by; }
        }
        impl Label for Square { fn label(&self) -> String { format!("[{}]", self.name()) } }
        fn total<T: Shape>(xs: &Vec<T>) -> i64 {
            let mut sum = 0;
            for x in xs.iter() { sum += x.area(); }
            sum
        }
        fn names<T>(xs: &Vec<T>) -> i64 where T: Shape {
            let mut n = 0;
            for x in xs.iter() { n += x.name().len() as i64; }
            n
        }
        fn grown<T: Shape + Clone>(x: &T, by: i64) -> T { let mut y = x.clone(); y.scale(by); y }
        fn both<T: Shape + Label>(x: T) -> String { format!("{}{}", x.label(), x.area()) }
        fn twice<T: Shape>(x: &T) -> i64 { inner(x) * 2 }
        fn inner<T: Shape>(x: &T) -> i64 { x.area() }
        struct Holder<T: Shape> { item: T }
        impl<T: Shape> Holder<T> { fn area(&self) -> i64 { self.item.area() + 1 } }
        pub fn run() -> i64 {
            let circles = vec![Circle { r: 1 }, Circle { r: 2 }];
            let squares = vec![Square { side: 3 }];
            let big = grown(&Square { side: 2 }, 5);
            let held = Holder { item: Circle { r: 1 } };
            total(&circles) + total(&squares) * 100 + names(&circles) * 10000
                + names(&squares) * 1000000 + big.side * 100000000
                + both(Square { side: 4 }).len() as i64 * 10000000000
                + twice(&Circle { r: 1 }) * 1000000000000
                + held.area() * 100000000000000
        }
        run()
    "#);
    assert_eq!(
        value,
        15 + 900
            + 100000
            + 6000000
            + 1000000000
            + 10 * 10000000000
            + 6 * 1000000000000
            + 4 * 100000000000000
    );
    // Rust's rules: the bound holds at each use, and limits the body.
    let unbound = rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } \
         fn total<T: Shape>(x: &T) -> i64 { x.area() } \
         pub fn run() -> i64 { total(&C { r: 1 }) } run()",
    );
    assert!(unbound.contains("Shape"), "{unbound}");
    rejected(
        "trait Shape { fn area(&self) -> i64; } fn total<T: Shape>(x: &T) -> i64 { x.area() } \
         pub fn run() -> i64 { total(&1) } run()",
    );
    rejected(
        "trait Shape { fn area(&self) -> i64; } fn f<T>(x: &T) -> i64 { x.area() } \
         pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "trait Shape { fn area(&self) -> i64; } fn f<T: Shape>(x: &T) -> String { x.area() } \
         pub fn run() -> i64 { 0 } run()",
    );
    rejected("fn f<T: Missing>(x: T) -> i64 { 0 } pub fn run() -> i64 { 0 } run()");
}

#[test]
fn language_hand_written_std_trait_impls() {
    let value = run_string(
        r#"
        use std::fmt;
        struct Point { x: i64, y: i64 }
        impl Clone for Point {
            fn clone(&self) -> Self { Point { x: self.x + 100, y: self.y } }
        }
        impl PartialEq for Point {
            fn eq(&self, other: &Self) -> bool { self.x == other.x }
        }
        impl Default for Point {
            fn default() -> Self { Point { x: 7, y: 8 } }
        }
        impl fmt::Debug for Point {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "P<{}", self.x)?;
                write!(f, ",{}>", self.y)
            }
        }
        impl std::fmt::Display for Point {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "({}, {})", self.x, self.y)
            }
        }
        #[derive(Clone, Debug, PartialEq)]
        enum Tag { A, B }
        impl fmt::Display for Tag {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                match self { Tag::A => write!(f, "a"), Tag::B => write!(f, "b") }
            }
        }
        fn show<T: fmt::Display>(x: &T) -> String { format!("[{}]", x) }
        fn same<T: PartialEq>(a: &T, b: &T) -> bool { a == b }
        fn dup<T: Clone>(a: &T) -> T { a.clone() }
        fn dbg<T: fmt::Debug>(a: &T) -> String { format!("{:?}", a) }
        fn shout<T: fmt::Display>(x: T) -> String { x.to_string() }
        pub fn run() -> String {
            let p = Point { x: 1, y: 2 };
            let q = p.clone();
            let d = Point::default();
            let equal = p == Point { x: 1, y: 99 };
            let unequal = p != Point { x: 2, y: 2 };
            format!(
                "{} {} {:?} {} {} {} {} {} {} {} {} {} {} {}",
                p, q, d, equal, unequal, Tag::B, show(&p), show(&Tag::A), show(&5),
                same(&p, &Point { x: 1, y: 0 }), dup(&p).x, dbg(&p), dbg(&Tag::A),
                shout(p) + &d.to_string()
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "(1, 2) (101, 2) P<7,8> true true b [(1, 2)] [a] [5] true 101 P<1,2> A (1, 2)(7, 8)"
    );
    // Rust's rules: one impl per trait and type, the trait's signature.
    let conflict = rejected(
        "#[derive(Clone)] struct P { x: i64 } impl Clone for P { fn clone(&self) -> Self { P { x: 1 } } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(conflict.contains("conflicting"), "{conflict}");
    rejected(
        "struct P { x: i64 } impl PartialEq for P { fn eq(&self, other: &Self) -> i64 { 1 } } \
         pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "struct P { x: i64 } pub fn run() -> i64 { let s = format!(\"{}\", P { x: 1 }); 0 } run()",
    );
    rejected(
        "fn show<T: std::fmt::Display>(x: T) -> String { format!(\"{}\", x) } struct P { x: i64 } \
         pub fn run() -> i64 { show(P { x: 1 }).len() } run()",
    );
}

#[test]
fn language_generic_default() {
    let value = run_string(
        r#"
        #[derive(Debug, Default)]
        struct Config { size: i64, name: String }
        #[derive(Debug)]
        struct Seed { n: i64 }
        impl Default for Seed { fn default() -> Self { Seed { n: 42 } } }
        fn fresh<T: Default>() -> T { T::default() }
        fn pair<T: Default + Clone>(x: &T) -> (T, T) { (x.clone(), T::default()) }
        fn forward<U: Default>() -> U { fresh::<U>() }
        fn or_default<T: Default>(o: Option<T>) -> T { match o { Some(v) => v, None => T::default() } }
        struct Stack<T> { items: Vec<T> }
        impl<T: Default + Clone> Stack<T> {
            fn top(&self) -> T { if self.items.is_empty() { T::default() } else { self.items[0].clone() } }
        }
        pub fn run() -> String {
            let n: i64 = fresh();
            let s: String = fresh();
            let c: Config = fresh();
            let seed = forward::<Seed>();
            let (a, b) = pair(&7);
            let v: Vec<i64> = fresh();
            let o: Option<bool> = fresh();
            let made: fn() -> Seed = fresh;
            let empty: Stack<i64> = Stack { items: Vec::new() };
            format!("{} [{}] {:?} {:?} {} {} {} {:?} {} {} {}",
                n, s, c, seed, a, b, v.len(), o, made().n, or_default::<i64>(None), empty.top())
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "0 [] Config { size: 0, name: \"\" } Seed { n: 42 } 7 0 0 None 42 0 0"
    );
    let unbounded = rejected("fn f<T>() -> T { T::default() } pub fn run() -> i64 { 0 } run()");
    assert!(unbounded.contains("Default"), "{unbounded}");
    rejected(
        "struct P { x: i64 } fn f<T: Default>() -> T { T::default() } \
         pub fn run() -> i64 { let p: P = f(); p.x } run()",
    );
}

#[test]
fn language_box() {
    let value = run(r#"
        #[derive(Debug, Clone, PartialEq)]
        struct Point { x: i64, y: i64 }
        impl Point {
            fn sum(&self) -> i64 { self.x + self.y }
            fn bump(&mut self) { self.x += 1; }
        }
        enum List { Cons(i64, Box<List>), Nil }
        fn total(list: &List) -> i64 {
            match list { List::Cons(v, rest) => *v + total(rest), List::Nil => 0 }
        }
        fn make(n: i64) -> Box<Point> { Box::new(Point { x: n, y: 1 }) }
        struct Holder { inner: Box<Point> }
        pub fn run() -> i64 {
            let b = Box::new(5);
            let n = *b + 1;
            let mut p = make(2);
            p.bump();
            let list = List::Cons(1, Box::new(List::Cons(2, Box::new(List::Nil))));
            let h = Holder { inner: Box::new(Point { x: 7, y: 20 }) };
            let q: Box<Point> = p.clone();
            let same = q == p && format!("{:?}", q) == "Point { x: 3, y: 1 }";
            let f: Box<dyn Fn(i64) -> i64> = Box::new(|x| x * 2);
            let unboxed: Point = *h.inner;
            n + p.sum() * 10 + total(&list) * 100 + h.inner.x * 1000
                + (if same { 10000 } else { 0 }) + f(4) * 100000 + unboxed.y * 1000000
        }
        run()
    "#);
    assert_eq!(value, 6 + 40 + 300 + 7000 + 10000 + 800000 + 20000000);
    // Rust's rules: `Box<T>` is its own type.
    rejected("pub fn run() -> i64 { let b: Box<i64> = 5; 0 } run()");
    rejected("pub fn run() -> i64 { let x: i64 = Box::new(5); x } run()");
    rejected("fn f(b: Box<i64>) -> i64 { b + 1 } pub fn run() -> i64 { f(Box::new(1)) } run()");
}

#[test]
fn language_trait_objects() {
    let value = run_string(
        r#"
        trait Shape {
            fn area(&self) -> i64;
            fn name(&self) -> String { "shape".to_string() }
            fn scale(&mut self, by: i64);
        }
        struct Circle { r: i64 }
        struct Square { side: i64 }
        impl Shape for Circle {
            fn area(&self) -> i64 { 3 * self.r * self.r }
            fn scale(&mut self, by: i64) { self.r *= by; }
        }
        impl Shape for Square {
            fn area(&self) -> i64 { self.side * self.side }
            fn name(&self) -> String { "square".to_string() }
            fn scale(&mut self, by: i64) { self.side *= by; }
        }
        fn describe(s: &dyn Shape) -> String { format!("{}:{}", s.name(), s.area()) }
        fn total(shapes: &Vec<Box<dyn Shape>>) -> i64 {
            let mut sum = 0;
            for s in shapes.iter() { sum += s.area(); }
            sum
        }
        fn biggest(a: Box<dyn Shape>, b: Box<dyn Shape>) -> Box<dyn Shape> {
            if a.area() > b.area() { a } else { b }
        }
        fn pick(square: bool) -> Box<dyn Shape> {
            if square { return Box::new(Square { side: 5 }); }
            Box::new(Circle { r: 1 })
        }
        struct Scene { shapes: Vec<Box<dyn Shape>> }
        pub fn run() -> String {
            let c = Circle { r: 2 };
            let r: &dyn Shape = &c;
            let shapes: Vec<Box<dyn Shape>> = vec![Box::new(Circle { r: 1 }), Box::new(Square { side: 3 })];
            let mut b: Box<dyn Shape> = Box::new(Square { side: 2 });
            b.scale(3);
            let scene = Scene { shapes: vec![Box::new(Square { side: 2 }), pick(false)] };
            format!("{} {} {} {} {} {} {}", describe(r), describe(&Square { side: 1 }), total(&shapes),
                b.area(), biggest(Box::new(Circle { r: 3 }), pick(true)).name(),
                total(&vec![pick(false), pick(true)]), total(&scene.shapes))
        }
        run()
    "#,
    );
    assert_eq!(value, "shape:12 square:1 12 36 shape 28 7");
    // Rust's rules: the trait must be implemented and dyn compatible.
    let missing = rejected(
        "trait Shape { fn area(&self) -> i64; } struct D { x: i64 } \
         pub fn run() -> i64 { let s: &dyn Shape = &D { x: 1 }; 0 } run()",
    );
    assert!(missing.contains("Shape"), "{missing}");
    let generic = rejected(
        "trait T { fn f<U>(&self, u: U) -> i64; } fn g(x: &dyn T) -> i64 { 0 } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(generic.contains("dyn compatible"), "{generic}");
    let returns_self = rejected(
        "trait T { fn make(&self) -> Self; } \
         pub fn run() -> i64 { let v: Vec<Box<dyn T>> = Vec::new(); 0 } run()",
    );
    assert!(returns_self.contains("dyn compatible"), "{returns_self}");
    let no_self = rejected(
        "trait T { fn new() -> i64; } struct S { b: Box<dyn T> } pub fn run() -> i64 { 0 } run()",
    );
    assert!(no_self.contains("dyn compatible"), "{no_self}");
    rejected("pub fn run() -> i64 { let s: &dyn Missing = &1; 0 } run()");
    rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } \
         impl Shape for C { fn area(&self) -> i64 { 1 } } \
         fn f(b: Box<dyn Shape>) -> i64 { let c = b.clone(); 0 } pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "trait Shape { fn area(&self) -> i64; } struct C { r: i64 } \
         impl Shape for C { fn area(&self) -> i64 { 1 } } \
         fn f(b: &dyn Shape) -> i64 { b.r } pub fn run() -> i64 { f(&C { r: 1 }) } run()",
    );
}

#[test]
fn language_generic_trait_functions_without_self() {
    let value = run_string(
        r#"
        trait Shape {
            fn new(size: i64) -> Self;
            fn label() -> String;
            fn area(&self) -> i64;
        }
        struct Circle { r: i64 }
        struct Square { side: i64 }
        impl Shape for Circle {
            fn new(size: i64) -> Self { Circle { r: size } }
            fn label() -> String { "circle".to_string() }
            fn area(&self) -> i64 { 3 * self.r * self.r }
        }
        impl Shape for Square {
            fn new(size: i64) -> Self { Square { side: size } }
            fn label() -> String { "square".to_string() }
            fn area(&self) -> i64 { self.side * self.side }
        }
        fn make<T: Shape>(size: i64) -> T { T::new(size) }
        fn area_of<T: Shape>(size: i64) -> i64 { let x = T::new(size); x.area() }
        fn labels<T: Shape, U>() -> String where U: Shape { format!("{}+{}", T::label(), U::label()) }
        fn forward<T: Shape>(size: i64) -> i64 { area_of::<T>(size) }
        fn in_closure<T: Shape>(sizes: &Vec<i64>) -> i64 {
            let f = |n: i64| T::new(n).area();
            let mut sum = 0;
            for s in sizes.iter() { let n = *s; sum += f(n); }
            sum
        }
        fn defaults<T: Default>() -> Vec<T> {
            let make = || T::default();
            vec![make(), make()]
        }
        pub fn run() -> String {
            let c: Circle = make(2);
            let s = make::<Square>(3);
            let zeros: Vec<i64> = defaults();
            format!("{} {} {} {} {} {} {} {}", c.r, s.side, area_of::<Square>(4),
                labels::<Circle, Square>(), forward::<Circle>(1), in_closure::<Square>(&vec![1, 2]),
                zeros.len(), zeros[1])
        }
        run()
    "#,
    );
    assert_eq!(value, "2 3 16 circle+square 3 5 2 0");
    let unbounded = rejected(
        "trait Shape { fn new(size: i64) -> Self; } fn f<T>() -> T { T::new(1) } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(unbounded.contains("new"), "{unbounded}");
    rejected(
        "trait Shape { fn new(size: i64) -> Self; } fn f<T: Shape>() -> T { T::new() } \
         pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "trait Shape { fn new(size: i64) -> Self; } struct P { x: i64 } \
         fn f<T: Shape>() -> T { T::new(1) } pub fn run() -> i64 { let p: P = f(); p.x } run()",
    );
}

#[test]
fn language_associated_consts() {
    let value = run(r#"
        trait Shape {
            const SIDES: i64;
            const NAME_LEN: i64 = 5;
            fn sides(&self) -> i64 { Self::SIDES }
        }
        struct Triangle { a: i64 }
        struct Square { side: i64 }
        impl Shape for Triangle { const SIDES: i64 = 3; }
        impl Shape for Square { const SIDES: i64 = 4; const NAME_LEN: i64 = 6; }
        impl Square { const ORIGIN: i64 = 100; fn unit() -> Square { Square { side: Square::ORIGIN } } }
        fn corners<T: Shape>(x: &T) -> i64 { T::SIDES * 10 + x.sides() }
        fn closure_sides<T: Shape>() -> i64 { let f = || T::SIDES + T::NAME_LEN; f() }
        pub fn run() -> i64 {
            Triangle::SIDES + Square::SIDES * 10 + corners(&Triangle { a: 1 }) * 100
                + corners(&Square { side: 1 }) * 10000 + Square::unit().side * 1000000
                + closure_sides::<Triangle>() * 1000000000 + Triangle::NAME_LEN * 100000000000
        }
        run()
    "#);
    assert_eq!(
        value,
        3 + 40 + 33 * 100 + 44 * 10000 + 100 * 1000000 + 8 * 1000000000 + 5 * 100000000000
    );
    // Rust's rules: a required const must be given, with the trait's type.
    let missing = rejected(
        "trait Shape { const SIDES: i64; } struct S { x: i64 } impl Shape for S {} \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing.contains("SIDES"), "{missing}");
    rejected(
        "trait Shape { const SIDES: i64; } struct S { x: i64 } \
         impl Shape for S { const SIDES: bool = true; } pub fn run() -> i64 { 0 } run()",
    );
    let object = rejected(
        "trait Shape { const SIDES: i64; } fn f(s: &dyn Shape) -> i64 { 0 } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(object.contains("dyn compatible"), "{object}");
}

#[test]
fn language_operators_lex_as_rust() {
    // `(*`, `*)`, `**`, `--` and `<-` are Rust tokens, not Pascal comments or untyped-script operators.
    let value = run(r#"
        fn f(x: i64) -> i64 { x }
        fn g(x: Option<Option<i64>>) -> Option<i64> { let v = x??; Some(v) }
        # [derive(Clone)]
        struct P { x: i64 }
        pub fn run() -> i64 {
            let a = 3;
            let b = 4;
            let (x, y) = (&a, &b);
            let r = &x;
            let boxed = Box::new(5);
            let neg = if a<-1 { 100 } else { 0 };
            f(*x) + (*x + *y) + (*boxed) + **r + a--b + --a + neg + g(Some(Some(2))).unwrap() + P { x: 1 }.x
        }
        run()
    "#);
    assert_eq!(value, 3 + 7 + 5 + 3 + 7 + 3 + 0 + 2 + 1);
}

#[test]
fn language_associated_types() {
    let shapes = r#"
        trait Shape {
            type Output;
            fn make(&self) -> Self::Output;
        }
        #[derive(Clone)]
        struct Square { side: i64 }
        #[derive(Clone)]
        struct Label { text: String }
        impl Shape for Square {
            type Output = i64;
            fn make(&self) -> Self::Output { self.side * self.side }
        }
        impl Shape for Label {
            type Output = String;
            fn make(&self) -> String { self.text.clone() }
        }
        fn made<T: Shape>(shape: &T) -> T::Output { shape.make() }
        fn qualified<T: Shape>(shape: &T) -> <T as Shape>::Output { made(shape) }
        fn total<T: Shape<Output = i64>>(shape: &T) -> i64 { shape.make() + 1 }
        fn area(square: &Square) -> <Square as Shape>::Output { square.make() }
    "#;
    let value = run(&format!(
        "{shapes}
        pub fn run() -> i64 {{
            let square = Square {{ side: 3 }};
            let label = Label {{ text: String::from(\"abcd\") }};
            let n: i64 = made(&square);
            let text: String = qualified(&label);
            let local: Square::Output = area(&square);
            let m = made(&square) * 1;
            n + total(&square) + text.len() as i64 + local + m
        }}
        run()"
    ));
    assert_eq!(value, 9 + 10 + 4 + 9 + 9);
    // `T::Output` is opaque in a generic body: it is not an i64.
    rejected(&format!(
        "{shapes}
        fn bad<T: Shape>(shape: &T) -> i64 {{ shape.make() }}
        pub fn run() -> i64 {{ bad(&Square {{ side: 1 }}) }} run()"
    ));
    // A bound's equality holds at its use.
    rejected(&format!(
        "{shapes}
        pub fn run() -> i64 {{ total(&Label {{ text: String::new() }}) }} run()"
    ));
    // An impl gives every associated type, with the trait's signature.
    rejected(
        "trait Shape { type Output; fn make(&self) -> Self::Output; }
        struct S;
        impl Shape for S { fn make(&self) -> i64 { 1 } }
        pub fn run() -> i64 { 1 } run()",
    );
    rejected(
        "trait Shape { type Output; fn make(&self) -> Self::Output; }
        struct S;
        impl Shape for S { type Output = String; fn make(&self) -> i64 { 1 } }
        pub fn run() -> i64 { 1 } run()",
    );
}

#[test]
fn language_associated_types_through_generics() {
    let shapes = r#"
        trait Shape {
            type Output;
            fn make(&self) -> Self::Output;
            fn twice(&self) -> (Self::Output, Self::Output) { (self.make(), self.make()) }
        }
        #[derive(Clone)]
        struct Square { side: i64 }
        impl Shape for Square { type Output = i64; fn make(&self) -> i64 { self.side } }
        #[derive(Clone)]
        struct Wrapper<T> { inner: T }
        impl<T: Clone> Shape for Wrapper<T> {
            type Output = T;
            fn make(&self) -> T { self.inner.clone() }
        }
        trait Named { type Name; }
        impl Named for Square { type Name = String; }
        fn made<T: Shape>(shape: &T) -> T::Output { shape.make() }
        fn total<T: Shape<Output = i64>>(shape: &T) -> i64 { shape.make() + 1 }
        fn outer<U: Shape<Output = i64>>(u: &U) -> i64 { total(u) + made(u) }
        fn outer2<U: Shape>(u: &U) -> U::Output { made(u) }
        fn pair<T: Shape>(shape: &T) -> (T::Output, T::Output) { shape.twice() }
        fn keep<T: Shape>(shape: &T) -> Vec<T::Output> { let x: T::Output = shape.make(); vec![x] }
        fn bounded<T>(shape: &T) -> i64 where T: Shape<Output = i64> { shape.make() }
    "#;
    let value = run(&format!(
        "{shapes}
        pub fn run() -> i64 {{
            let square = Square {{ side: 2 }};
            let (a, b) = pair(&square);
            let (c, d) = square.twice();
            let name: <Square as Named>::Name = String::from(\"abc\");
            let nested = Wrapper {{ inner: Square {{ side: 3 }} }};
            made(&Wrapper {{ inner: 5 }}) + outer(&square) + outer2(&square) + a + b + c + d
                + keep(&square)[0] + name.len() as i64 + bounded(&square) + made(&made(&nested))
        }}
        run()"
    ));
    assert_eq!(value, 5 + 5 + 2 + 2 + 2 + 2 + 2 + 2 + 3 + 2 + 3);
    rejected(&format!(
        "{shapes} pub fn run() -> i64 {{ total(&Wrapper {{ inner: String::new() }}) }} run()"
    ));
    // `T::Output` names an associated type of a trait bounding `T`.
    rejected(&format!(
        "{shapes} fn f<T: Clone>(x: &T) -> T::Output {{ x.clone() }} pub fn run() -> i64 {{ 1 }} run()"
    ));
    // An opaque associated type has no capabilities without bounds.
    rejected(&format!(
        "{shapes} fn f<T: Shape>(a: &T) -> bool {{ a.make() == a.make() }} pub fn run() -> i64 {{ 1 }} run()"
    ));
    // A trait with an associated type is not a `dyn` type in Rawr.
    rejected(&format!(
        "{shapes} fn f(x: &dyn Shape) -> i64 {{ 1 }} pub fn run() -> i64 {{ 1 }} run()"
    ));
}

#[test]
fn language_binary_star_star_is_multiply_deref() {
    // Rust has no power operator: `a ** b` is `a * *b`, and powers are methods.
    let value = run(r#"
        pub fn run(a: i64, b: &i64) -> i64 {
            let c = 2;
            let r = &c;
            a ** b + a**r + a.pow(3)
        }
        run(5, &3)
    "#);
    assert_eq!(value, 15 + 10 + 125);
    let float = Engine::new()
        .eval::<f64>("pub fn run(x: f64, y: &f64) -> f64 { x.powi(3) + y.powf(0.5) + x ** y } run(2.0, &4.0)")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(float, 8.0 + 2.0 + 8.0);
    rejected("pub fn run(a: i64, b: i64) -> i64 { a ** b } run(2, 3)");
    rejected("pub fn run(a: i64) -> i64 { a.pow(2.0) } run(2)");
    rejected("pub fn run(a: f64) -> f64 { a.pow(2) } run(2.0)");
    // Untyped scripts keep their power operator.
    assert_eq!(run("let x = 2 ** 3; x"), 8);
}

#[test]
fn language_inner_attributes() {
    // Lint attributes are accepted and ignored, as rustc ignores lints it does not know.
    let value = run(r#"#![allow(dead_code, clippy::too_many_lines)]
        #![deny(warnings)]
        #![warn(unused)]
        pub fn run() -> i64 { 7 }
        run()
    "#);
    assert_eq!(value, 7);
    // A shebang on line 1 is skipped; `#![` on line 1 is an attribute.
    assert_eq!(
        run("#!/usr/bin/env rawr\npub fn run() -> i64 { 8 } run()"),
        8
    );
    let error = rejected("#![no_std]\npub fn run() -> i64 { 1 } run()");
    assert!(
        error.contains("unsupported inner attribute `#![no_std]`"),
        "{error}"
    );
    let error = rejected("pub fn run() -> i64 { 1 }\n#![allow(unused)]\nrun()");
    assert!(
        error.contains("only permitted at the top of a file"),
        "{error}"
    );
}

#[test]
fn language_associated_types_keyed_by_trait() {
    // Two traits each give `Square` an `Output`; each projection names its trait.
    let traits = r#"
        trait Shape { type Output; fn make(&self) -> Self::Output; }
        trait Size { type Output; fn size(&self) -> Self::Output; }
        #[derive(Clone)]
        struct Square { side: i64 }
        impl Shape for Square { type Output = i64; fn make(&self) -> Self::Output { self.side } }
        impl Size for Square {
            type Output = String;
            fn size(&self) -> Self::Output { String::from("big") }
        }
        fn made<T: Shape>(shape: &T) -> T::Output { shape.make() }
        fn sized<T: Size>(shape: &T) -> <T as Size>::Output { shape.size() }
        fn both<T: Shape<Output = i64> + Size>(shape: &T) -> i64 { shape.make() + 1 }
    "#;
    let value = run(&format!(
        "{traits}
        pub fn run() -> i64 {{
            let square = Square {{ side: 4 }};
            let text: <Square as Size>::Output = sized(&square);
            let n: <Square as Shape>::Output = made(&square);
            n + text.len() as i64 + both(&square) + square.make()
        }}
        run()"
    ));
    assert_eq!(value, 4 + 3 + 5 + 4);
    // Both bounds declare `Output`: `T::Output` is ambiguous, as in Rust.
    let error = rejected(&format!(
        "{traits} fn f<T: Shape + Size>(x: &T) -> T::Output {{ x.make() }}
        pub fn run() -> i64 {{ 1 }} run()"
    ));
    assert!(
        error.contains("ambiguous") && error.contains("`Shape`") && error.contains("`Size`"),
        "{error}"
    );
    let error = rejected(&format!(
        "{traits} pub fn run() -> i64 {{ let x: Square::Output = 1; x }} run()"
    ));
    assert!(error.contains("ambiguous"), "{error}");
    // `<T as Trait>::Name` names a trait bounding `T`.
    rejected(&format!(
        "{traits} fn f<T: Shape>(x: &T) -> <T as Size>::Output {{ x.make() }}
        pub fn run() -> i64 {{ 1 }} run()"
    ));
}

#[test]
fn language_associated_type_bounds() {
    let shapes = r#"
        trait Area { fn area(&self) -> i64; }
        #[derive(Clone, PartialEq)]
        struct Square { side: i64 }
        impl Area for Square { fn area(&self) -> i64 { self.side * self.side } }
        trait Maker {
            type Output: Clone + PartialEq + Area;
            fn make(&self) -> Self::Output;
        }
        struct Factory { side: i64 }
        impl Maker for Factory {
            type Output = Square;
            fn make(&self) -> Square { Square { side: self.side } }
        }
        fn twice<M: Maker>(maker: &M) -> i64 {
            let made = maker.make();
            let copy = made.clone();
            if copy == made { made.area() + copy.area() } else { 0 }
        }
    "#;
    let value = run(&format!(
        "{shapes} pub fn run() -> i64 {{ twice(&Factory {{ side: 3 }}) }} run()"
    ));
    assert_eq!(value, 18);
    // The impl's type must meet the bounds.
    let error = rejected(
        "trait Maker { type Output: Clone; fn make(&self) -> Self::Output; }
        struct Plain;
        struct Factory;
        impl Maker for Factory { type Output = Plain; fn make(&self) -> Plain { Plain } }
        pub fn run() -> i64 { 1 } run()",
    );
    assert!(error.contains("Clone"), "{error}");
    rejected(&format!(
        "{shapes} struct Bare {{ n: i64 }}
        impl Maker for Bare {{ type Output = i64; fn make(&self) -> i64 {{ 1 }} }}
        pub fn run() -> i64 {{ 1 }} run()"
    ));
    // Without a bound, an associated type has no methods.
    rejected(
        "trait Maker { type Output; fn make(&self) -> Self::Output; }
        fn f<M: Maker>(m: &M) -> bool { let a = m.make(); a.clone() == a }
        pub fn run() -> i64 { 1 } run()",
    );
}

#[test]
fn language_trait_objects_with_auto_traits() {
    // `dyn Shape + Send + Sync + 'static`: auto traits and lifetimes are
    // accepted after the trait, as in Rust.
    let shapes = r#"
        trait Shape { fn area(&self) -> i64; }
        trait Named { fn name(&self) -> String; }
        #[derive(Clone)]
        struct Square { side: i64 }
        impl Shape for Square { fn area(&self) -> i64 { self.side * self.side } }
        impl Named for Square { fn name(&self) -> String { String::from("square") } }
        fn total(shapes: &Vec<Box<dyn Shape + Send + Sync>>) -> i64 {
            let mut sum = 0;
            for shape in shapes { sum += shape.area(); }
            sum
        }
        fn one(shape: &(dyn Shape + Send + 'static)) -> i64 { shape.area() }
    "#;
    let value = run(&format!(
        "{shapes}
        pub fn run() -> i64 {{
            let square = Square {{ side: 3 }};
            let shapes: Vec<Box<dyn Shape + Send + Sync>> = vec![Box::new(square.clone())];
            let local: &(dyn Shape + Sync) = &square;
            let boxed: Box<dyn Shape + std::marker::Send> = Box::new(square.clone());
            total(&shapes) + one(&square) + local.area() + boxed.area()
        }}
        run()"
    ));
    assert_eq!(value, 9 * 4);
    // Two non-auto traits make no trait object in Rust (E0225).
    let error = rejected(&format!(
        "{shapes} fn both(x: &(dyn Shape + Named)) -> i64 {{ 1 }} pub fn run() -> i64 {{ 1 }} run()"
    ));
    assert!(error.contains("E0225"), "{error}");
    let error = rejected(&format!(
        "{shapes} pub fn run() -> i64 {{
            let s = Square {{ side: 1 }};
            let x: &(dyn Shape + Named) = &s;
            1
        }} run()"
    ));
    assert!(error.contains("E0225"), "{error}");
}

#[test]
fn language_trait_objects_with_associated_types() {
    let shapes = r#"
        trait Shape { type Output; fn make(&self) -> Self::Output; }
        #[derive(Clone)]
        struct Square { side: i64 }
        #[derive(Clone)]
        struct Label { text: String }
        #[derive(Clone)]
        struct Circle { r: i64 }
        impl Shape for Square { type Output = i64; fn make(&self) -> i64 { self.side * self.side } }
        impl Shape for Circle { type Output = i64; fn make(&self) -> i64 { 3 * self.r * self.r } }
        impl Shape for Label { type Output = String; fn make(&self) -> String { self.text.clone() } }
        fn total(shapes: &Vec<Box<dyn Shape<Output = i64>>>) -> i64 {
            let mut sum = 0;
            for shape in shapes { sum += shape.make(); }
            sum
        }
        fn text(shape: &dyn Shape<Output = String>) -> String { shape.make() }
    "#;
    let value = run(&format!(
        "{shapes}
        pub fn run() -> i64 {{
            let shapes: Vec<Box<dyn Shape<Output = i64>>> =
                vec![Box::new(Square {{ side: 2 }}), Box::new(Circle {{ r: 1 }})];
            let label = Label {{ text: String::from(\"abc\") }};
            let one: &dyn Shape<Output = i64> = &Square {{ side: 5 }};
            total(&shapes) + text(&label).len() as i64 + one.make()
        }}
        run()"
    ));
    assert_eq!(value, 4 + 3 + 3 + 25);
    // The type's associated type must be the binding's.
    rejected(&format!(
        "{shapes} pub fn run() -> i64 {{ let l = Label {{ text: String::new() }}; let x: &dyn Shape<Output = i64> = &l; 1 }} run()"
    ));
    // Every associated type is given (E0191), and no other.
    let error = rejected(&format!(
        "{shapes} fn f(x: &dyn Shape) -> i64 {{ 1 }} pub fn run() -> i64 {{ 1 }} run()"
    ));
    assert!(error.contains("E0191"), "{error}");
    rejected(&format!(
        "{shapes} fn f(x: &dyn Shape<Output = i64, Extra = i64>) -> i64 {{ 1 }} pub fn run() -> i64 {{ 1 }} run()"
    ));
    // A binding's value is the call's type.
    rejected(&format!(
        "{shapes} fn f(x: &dyn Shape<Output = String>) -> i64 {{ x.make() }} pub fn run() -> i64 {{ 1 }} run()"
    ));
}

#[test]
fn language_supertraits() {
    let value = run_string(
        r#"
        trait Shape { fn area(&self) -> i64; }
        trait Named: Shape + Clone {
            fn name(&self) -> String;
            fn describe(&self) -> String { format!("{}={}", self.name(), self.area()) }
        }
        trait Labeled: Shape { fn label(&self) -> String; }
        trait A { fn a(&self) -> i64; }
        trait B { fn b(&self) -> i64; }
        trait AB: A + B {}
        trait Container { type Item; const K: i64; fn first(&self) -> Self::Item; fn make(k: i64) -> Self; }
        trait Bag: Container { fn count(&self) -> i64; }
        #[derive(Clone)]
        struct Sq { side: i64 }
        impl Shape for Sq { fn area(&self) -> i64 { self.side * self.side } }
        impl Named for Sq { fn name(&self) -> String { "sq".to_string() } }
        impl Labeled for Sq { fn label(&self) -> String { format!("L{}", self.side) } }
        struct P { x: i64 }
        impl A for P { fn a(&self) -> i64 { self.x } }
        impl B for P { fn b(&self) -> i64 { self.x * 10 } }
        impl AB for P {}
        struct V { items: Vec<i64> }
        impl Container for V {
            type Item = i64;
            const K: i64 = 3;
            fn first(&self) -> i64 { self.items[0] }
            fn make(k: i64) -> V { V { items: vec![k, k] } }
        }
        impl Bag for V { fn count(&self) -> i64 { self.items.len() as i64 } }
        fn twice<T: Named>(t: &T) -> String { format!("{} {}", t.describe(), t.clone().area()) }
        fn both<T: AB>(t: &T) -> i64 { t.a() + t.b() }
        fn head<T: Bag>(t: &T) -> T::Item { t.first() }
        fn build<T: Bag>() -> T { T::make(T::K) }
        fn object(l: &dyn Labeled) -> String {
            let s: &dyn Shape = l;
            format!("{} {} {}", l.label(), l.area(), s.area())
        }
        fn boxed(l: Box<dyn Labeled>) -> Box<dyn Shape> { l }
        pub fn run() -> String {
            let sq = Sq { side: 3 };
            let v: V = build();
            format!("{} | {} | {} | {} {} | {} | {}", twice(&sq), both(&P { x: 2 }), head(&V { items: vec![7] }),
                v.count(), head(&v), object(&sq), boxed(Box::new(Sq { side: 2 })).area())
        }
        run()
    "#,
    );
    assert_eq!(value, "sq=9 9 | 22 | 7 | 2 3 | L3 9 9 | 4");
    // An impl of the subtrait needs each supertrait (E0277).
    let missing = rejected(
        "trait Shape { fn area(&self) -> i64; } trait Named: Shape { fn name(&self) -> i64; } \
         struct C { r: i64 } impl Named for C { fn name(&self) -> i64 { 1 } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing.contains("Shape"), "{missing}");
    let not_clone = rejected(
        "trait Shape: Clone { fn area(&self) -> i64; } struct C { r: i64 } \
         impl Shape for C { fn area(&self) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(not_clone.contains("Clone"), "{not_clone}");
    let cycle = rejected(
        "trait A: B { fn a(&self) -> i64; } trait B: A { fn b(&self) -> i64; } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(cycle.contains("E0391"), "{cycle}");
    // `Clone` needs `Self: Sized`: such a trait is not dyn compatible.
    let sized = rejected(
        "trait Shape: Clone { fn area(&self) -> i64; } fn f(s: &dyn Shape) -> i64 { 0 } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(sized.contains("dyn compatible"), "{sized}");
    // Upcasting goes to a supertrait only.
    rejected(
        "trait Shape { fn area(&self) -> i64; } trait Labeled { fn label(&self) -> i64; } \
         fn f(l: &dyn Labeled) -> i64 { let s: &dyn Shape = l; 0 } pub fn run() -> i64 { 0 } run()",
    );
    rejected(
        "trait Shape { fn area(&self) -> i64; } trait Named: Shape { fn name(&self) -> i64; } \
         fn f<T: Shape>(t: &T) -> i64 { t.name() } pub fn run() -> i64 { 0 } run()",
    );
}

#[test]
fn language_generic_traits() {
    let value = run_string(
        r#"
        trait Convert<T> {
            fn convert(&self) -> T;
            fn pair(&self) -> Vec<T> { let x: T = self.convert(); vec![x, self.convert()] }
        }
        struct C { n: i64 }
        struct D { n: i64 }
        struct Holder<T> { v: T }
        impl Convert<i64> for C { fn convert(&self) -> i64 { self.n * 2 } }
        impl Convert<String> for C { fn convert(&self) -> String { format!("c{}", self.n) } }
        impl Convert<i64> for D { fn convert(&self) -> i64 { self.n + 100 } }
        impl<T: Clone> Convert<T> for Holder<T> { fn convert(&self) -> T { self.v.clone() } }
        fn total<U: Convert<i64>>(u: &U) -> i64 { u.convert() + 1 }
        fn label<U>(u: &U) -> String where U: Convert<String> { u.convert() }
        fn show(x: &dyn Convert<String>) -> String { x.convert() }
        fn sum(items: &Vec<Box<dyn Convert<i64>>>) -> i64 {
            let mut s = 0;
            for item in items.iter() { s += item.convert(); }
            s
        }
        pub fn run() -> String {
            let c = C { n: 3 };
            let a: i64 = c.convert();
            let b: String = c.convert();
            let d = D { n: 1 };
            let z = d.convert();
            let h = Holder { v: 7 };
            let items: Vec<Box<dyn Convert<i64>>> = vec![Box::new(C { n: 1 }), Box::new(D { n: 2 }), Box::new(Holder { v: 5 })];
            let p: Vec<String> = c.pair();
            format!("{} {} {} {} {} {} {} {} {} {}", a, b, z + 1, total(&c), total(&d), total(&h),
                label(&c), show(&c), sum(&items), p.len())
        }
        run()
    "#,
    );
    assert_eq!(value, "6 c3 102 7 102 8 c3 c3 109 2");
    let traits = "trait Convert<T> { fn convert(&self) -> T; } struct C { n: i64 } \
                  impl Convert<i64> for C { fn convert(&self) -> i64 { 1 } } \
                  impl Convert<String> for C { fn convert(&self) -> String { \"a\".to_string() } }";
    // Two impls and nothing to choose between them (E0283).
    let ambiguous = rejected(&format!(
        "{traits} pub fn run() -> i64 {{ let c = C {{ n: 1 }}; let x = c.convert(); 0 }} run()"
    ));
    assert!(ambiguous.contains("E0283"), "{ambiguous}");
    // No `impl Convert<bool> for C`.
    let missing = rejected(&format!(
        "{traits} fn f<U: Convert<bool>>(u: &U) -> bool {{ u.convert() }} \
         pub fn run() -> i64 {{ f(&C {{ n: 1 }}); 0 }} run()"
    ));
    assert!(missing.contains("Convert"), "{missing}");
    let object = rejected(&format!(
        "{traits} pub fn run() -> i64 {{ let x: &dyn Convert<bool> = &C {{ n: 1 }}; 0 }} run()"
    ));
    assert!(object.contains("Convert"), "{object}");
    // One argument per trait parameter (E0107).
    let arity = rejected(
        "trait Convert<T> { fn convert(&self) -> T; } struct C { n: i64 } \
         impl Convert for C { fn convert(&self) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(arity.contains("Convert"), "{arity}");
    rejected(
        "trait Convert<T> { fn convert(&self) -> T; } fn f<U: Convert>(u: &U) -> i64 { 0 } \
         pub fn run() -> i64 { 0 } run()",
    );
    // The impl's signature is the trait's with its arguments.
    rejected(
        "trait Convert<T> { fn convert(&self) -> T; } struct C { n: i64 } \
         impl Convert<i64> for C { fn convert(&self) -> String { \"\".to_string() } } \
         pub fn run() -> i64 { 0 } run()",
    );
}

#[test]
fn language_builtin_runtime_type_keys() {
    use super::builtin_type_key;
    use crate::Dynamic;
    let keys = [
        builtin_type_key(&Dynamic::from(5_i64)),
        builtin_type_key(&Dynamic::from(1.5_f64)),
        builtin_type_key(&Dynamic::from("text")),
        builtin_type_key(&Dynamic::from(true)),
        builtin_type_key(&Dynamic::from('c')),
        builtin_type_key(&crate::option::none()),
        builtin_type_key(&Dynamic::from(crate::Array::new())),
        builtin_type_key(&Dynamic::UNIT),
        builtin_type_key(&crate::result::ok(Dynamic::UNIT)),
    ];
    assert_eq!(
        keys,
        [-2, -3, -4, -5, -6, -7, -8, -9, -10].map(Some),
        "each builtin type has its witness code as its runtime key"
    );
    // `T::default()` through the same codes, `Box<T>` as `T`.
    let value = run(r#"
        fn make<T: Default>() -> T { T::default() }
        pub fn run() -> i64 {
            let b: Box<i64> = make();
            let s: String = make();
            let n: i64 = *b;
            if s == "" { n + 3 } else { 0 }
        }
        run()
    "#);
    assert_eq!(value, 3);
}

#[test]
fn language_trait_impls_on_builtin_types() {
    let value = run(r#"
        trait Shape {
            fn area(&self) -> i64;
            fn twice(&self) -> i64 { self.area() * 2 }
        }
        trait Named: Shape { fn name(&self) -> String; }
        impl Shape for i64 { fn area(&self) -> i64 { *self * *self } }
        impl Named for i64 { fn name(&self) -> String { "int".to_string() } }
        impl Shape for String { fn area(&self) -> i64 { self.len() as i64 } }
        impl<T: Shape> Shape for Vec<T> {
            fn area(&self) -> i64 {
                let mut total = 0;
                for x in self { total += x.area(); }
                total
            }
        }
        impl<T: Shape> Shape for Option<T> {
            fn area(&self) -> i64 { match self { Some(x) => x.area(), None => 0 } }
        }
        struct Sq { side: i64 }
        impl Shape for Sq { fn area(&self) -> i64 { self.side * self.side } }
        fn total<T: Shape>(x: &T) -> i64 { x.area() }
        fn named<T: Named>(x: &T) -> i64 { x.name().len() as i64 + x.area() }
        pub fn run() -> i64 {
            let a = 5.area();
            let b = total(&3);
            let s = "abcd".to_string();
            let c = s.area();
            let d = vec![2, 3].area();
            let e = Some(4).area() + total(&None::<i64>);
            let shapes: Vec<Box<dyn Shape>> =
                vec![Box::new(2), Box::new(Sq { side: 3 }), Box::new("xy".to_string())];
            let mut f = 0;
            for x in &shapes { f += x.area(); }
            let g = named(&2);
            let h = 3.twice();
            let o: &dyn Named = &6;
            let i = o.area() + o.name().len() as i64;
            a + b + c + d + e + f + g + h + i
        }
        run()
    "#);
    assert_eq!(value, 25 + 9 + 4 + 13 + 16 + 15 + 7 + 18 + 39);
    // A std trait for a std type is E0117; a type without the impl has none.
    let foreign = rejected(
        "impl Default for i64 { fn default() -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(foreign.contains("E0117"), "{foreign}");
    rejected(
        "trait Shape { fn area(&self) -> i64; } impl Shape for i64 { fn area(&self) -> i64 { 1 } } \
         pub fn run() -> i64 { true.area() } run()",
    );
    // A supertrait must hold for the builtin type too (E0277).
    rejected(
        "trait Shape { fn area(&self) -> i64; } trait Named: Shape { fn n(&self) -> i64; } \
         impl Named for bool { fn n(&self) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    // Functions without `self` reach a builtin type through its witness.
    let made = run(r#"
        trait Make { fn make() -> Self; }
        impl Make for i64 { fn make() -> i64 { 40 } }
        impl Make for String { fn make() -> String { "ab".to_string() } }
        fn build<T: Make>() -> T { T::make() }
        pub fn run() -> i64 { let n: i64 = build(); let s: String = build(); n + s.len() as i64 }
        run()
    "#);
    assert_eq!(made, 42);
    rejected(
        "trait Shape { fn area(&self) -> i64; } impl Shape for i64 { fn area(&self) -> i64 { 1 } } \
         fn total<T: Shape>(x: &T) -> i64 { x.area() } pub fn run() -> i64 { total(&1.5) } run()",
    );
}

#[test]
fn language_generic_trait_functions_by_instance() {
    let value = run(r#"
        trait Make<T> { fn make(value: T) -> Self; }
        struct C { n: i64 }
        impl Make<i64> for C { fn make(value: i64) -> C { C { n: value } } }
        impl Make<String> for C { fn make(value: String) -> C { C { n: value.len() as i64 } } }
        impl Make<bool> for i64 { fn make(value: bool) -> i64 { if value { 1 } else { 0 } } }
        fn build<T: Make<String>>(s: String) -> T { T::make(s) }
        fn build_int<T: Make<i64>>(n: i64) -> T { T::make(n) }
        fn build_flag<T: Make<bool>>(b: bool) -> T { T::make(b) }
        pub fn run() -> i64 {
            let a = C::make(5);
            let b = C::make("abc".to_string());
            let c: C = build("ab".to_string());
            let d: C = build_int(7);
            let e: i64 = build_flag(true);
            a.n + b.n * 10 + c.n * 100 + d.n * 1000 + e * 10000
        }
        run()
    "#);
    assert_eq!(value, 5 + 30 + 200 + 7000 + 10000);
    // No impl for the instance the arguments name.
    rejected(
        "trait Make<T> { fn make(value: T) -> Self; } struct C { n: i64 } \
         impl Make<i64> for C { fn make(value: i64) -> C { C { n: value } } } \
         impl Make<String> for C { fn make(value: String) -> C { C { n: 0 } } } \
         pub fn run() -> i64 { C::make(true).n } run()",
    );
}

#[test]
fn language_from_and_into_are_prelude_traits() {
    let value = run(r#"
        struct Celsius { degrees: i64 }
        impl From<Celsius> for i64 { fn from(c: Celsius) -> i64 { c.degrees } }
        impl From<Celsius> for String { fn from(c: Celsius) -> String { "hot".to_string() } }
        struct Wrapper { n: i64 }
        impl From<i64> for Wrapper { fn from(n: i64) -> Wrapper { Wrapper { n } } }
        impl From<bool> for Wrapper {
            fn from(b: bool) -> Wrapper { Wrapper { n: if b { 10 } else { 20 } } }
        }
        fn make<T: From<i64>>(n: i64) -> T { T::from(n) }
        fn flag<T: From<bool>>(b: bool) -> T { T::from(b) }
        fn convert<T: Into<Wrapper>>(value: T) -> i64 { let w: Wrapper = value.into(); w.n }
        fn describe<T: Into<String>>(value: T) -> String { value.into() }
        fn fails(n: i64) -> Result<i64, Celsius> {
            if n > 0 { Err(Celsius { degrees: n }) } else { Ok(0) }
        }
        fn message(n: i64) -> Result<i64, String> { let v = fails(n)?; Ok(v) }
        pub fn run() -> i64 {
            let a = i64::from(Celsius { degrees: 3 });
            let b: i64 = Celsius { degrees: 4 }.into();
            let c = String::from(Celsius { degrees: 0 }).len() as i64;
            let d = i64::from(true) + String::from('x').len() as i64 + String::from("ab").len() as i64;
            let e: Wrapper = make(5);
            let f = convert(7) + convert(true);
            let g = (describe(Celsius { degrees: 1 }).len() + describe('x').len()
                + describe("ab".to_string()).len()) as i64;
            let h = match message(2) { Err(m) => m.len() as i64, Ok(_) => 0 };
            let i: i64 = true.into();
            let w = Wrapper::from(false);
            let j: i64 = flag(true);
            let k: Wrapper = flag(true);
            a + b + c + d + e.n + f + g + h + i + w.n + j + k.n
        }
        run()
    "#);
    assert_eq!(value, 3 + 4 + 3 + 4 + 5 + 17 + 6 + 3 + 1 + 20 + 1 + 10);
    let float = Engine::new()
        .eval::<f64>("pub fn run() -> f64 { f64::from(true) + f64::from(false) } run()")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(float, 1.0);
    // Rust's rules: a std trait for a std type is E0117; no `From<f64>` for `i64`.
    let foreign = rejected(
        "impl From<f64> for i64 { fn from(x: f64) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(foreign.contains("E0117"), "{foreign}");
    rejected("pub fn run() -> i64 { i64::from(1.5) } run()");
    rejected("fn f<T: Into<i64>>(x: T) -> i64 { x.into() } pub fn run() -> i64 { f(1.5) } run()");
}

#[test]
fn language_default_trait_parameters_and_repeated_generic_bounds() {
    let value = run(r#"
        trait Add<Rhs = Self> { fn add(&self, other: Rhs) -> i64; }
        struct M { v: i64 }
        impl Add for M { fn add(&self, other: M) -> i64 { self.v + other.v } }
        impl Add<i64> for M { fn add(&self, other: i64) -> i64 { self.v * other } }
        fn sum<T: Add>(a: &T, b: T) -> i64 { a.add(b) }
        trait Convert<T> { fn convert(&self) -> T; }
        struct C { n: i64 }
        impl Convert<i64> for C { fn convert(&self) -> i64 { self.n } }
        impl Convert<String> for C { fn convert(&self) -> String { "abc".to_string() } }
        fn both<U: Convert<i64> + Convert<String>>(u: &U) -> i64 {
            let n: i64 = u.convert();
            let s: String = u.convert();
            n + s.len() as i64
        }
        pub fn run() -> i64 {
            let a = M { v: 2 };
            let x = a.add(M { v: 3 });
            let y: i64 = a.add(10);
            let z = sum(&M { v: 4 }, M { v: 5 });
            x + y * 10 + z * 1000 + both(&C { n: 7 }) * 100000
        }
        run()
    "#);
    assert_eq!(value, 5 + 200 + 9000 + 1000000);
    // Without a default, the argument is required (E0107); defaults are
    // only on a trait's own parameters.
    rejected(
        "trait Convert<T> { fn convert(&self) -> T; } struct C { n: i64 } \
         impl Convert for C { fn convert(&self) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    rejected("struct P<T = i64> { v: T } pub fn run() -> i64 { 0 } run()");
    // Both bounds hold for the type.
    rejected(
        "trait Convert<T> { fn convert(&self) -> T; } struct C { n: i64 } \
         impl Convert<i64> for C { fn convert(&self) -> i64 { self.n } } \
         fn both<U: Convert<i64> + Convert<String>>(u: &U) -> i64 { 0 } \
         pub fn run() -> i64 { both(&C { n: 1 }) } run()",
    );
}

#[test]
fn language_builtin_inherent_methods_before_trait_methods() {
    // Rust finds a builtin's inherent method before a trait's function of
    // the same name; a bound or a trait object still reaches the trait's.
    let value = run(r#"
        trait Len { fn len(&self) -> i64; fn is_some(&self) -> bool; }
        impl Len for String { fn len(&self) -> i64 { 100 } fn is_some(&self) -> bool { true } }
        impl Len for Option<i64> { fn len(&self) -> i64 { 200 } fn is_some(&self) -> bool { false } }
        impl Len for Vec<i64> { fn len(&self) -> i64 { 300 } fn is_some(&self) -> bool { false } }
        fn through<T: Len>(x: &T) -> i64 { x.len() }
        pub fn run() -> i64 {
            let s = "abc".to_string();
            let v = vec![1, 2];
            let o = Some(1);
            let own = s.len() + v.len() + if o.is_some() { 1 } else { 0 };
            let object: &dyn Len = &s;
            let traits = through(&s) + through(&v) + object.len() + o.len();
            own as i64 * 10000 + traits
        }
        run()
    "#);
    assert_eq!(value, (3 + 2 + 1) * 10000 + 100 + 300 + 100 + 200);
}

#[test]
fn language_fully_qualified_trait_calls() {
    let value = run(r#"
        trait Shape { fn area(&self) -> i64; fn sides(self) -> i64; }
        trait Make { fn make() -> Self; const N: i64; }
        trait Len { fn len(&self) -> i64; }
        trait Convert<T> { fn convert(&self) -> T; }
        struct Sq { side: i64 }
        impl Sq { fn area(&self) -> i64 { 0 } }
        impl Shape for Sq { fn area(&self) -> i64 { self.side * self.side } fn sides(self) -> i64 { 4 } }
        impl Make for Sq { fn make() -> Sq { Sq { side: 6 } } const N: i64 = 9; }
        impl Make for i64 { fn make() -> i64 { 40 } const N: i64 = 1; }
        impl Len for String { fn len(&self) -> i64 { 100 } }
        impl Convert<i64> for Sq { fn convert(&self) -> i64 { self.side } }
        impl Convert<String> for Sq { fn convert(&self) -> String { "abc".to_string() } }
        fn through<T: Shape>(x: &T) -> i64 { Shape::area(x) + <T as Shape>::area(x) }
        fn build<T: Make>() -> T { <T as Make>::make() }
        trait Named where Self: Shape { fn name(&self) -> i64 { self.area() + 1 } }
        impl Named for Sq {}
        pub fn run() -> i64 {
            let s = Sq { side: 3 };
            let text = "ab".to_string();
            let a = Shape::area(&s) + <Sq as Shape>::area(&s) + s.area();
            let b = Len::len(&text) + text.len() as i64;
            let c = through(&s);
            let d = <Sq as Make>::make().side + <i64 as Make>::make() + <Sq as Make>::N;
            let e: Sq = build();
            let f = <Sq as Convert<i64>>::convert(&s) + Convert::<String>::convert(&s).len() as i64;
            let g = Shape::sides(Sq { side: 1 }) + s.name();
            a * 1000000 + b * 10000 + c * 1000 + d + e.side + f * 100 + g * 10
        }
        run()
    "#);
    assert_eq!(
        value,
        18 * 1000000 + 102 * 10000 + 18 * 1000 + (6 + 40 + 9) + 6 + 6 * 100 + 14 * 10
    );
    // No impl of the trait for the receiver's type; `&self` needs a reference.
    rejected(
        "trait Shape { fn area(&self) -> i64; } struct Sq { side: i64 } \
         pub fn run() -> i64 { Shape::area(&Sq { side: 1 }) } run()",
    );
    rejected(
        "trait Shape { fn area(&self) -> i64; } struct Sq { side: i64 } \
         impl Shape for Sq { fn area(&self) -> i64 { 1 } } \
         pub fn run() -> i64 { Shape::area(Sq { side: 1 }) } run()",
    );
    // `where Self: Shape` is a supertrait: the impl needs `Shape` (E0277).
    rejected(
        "trait Shape { fn area(&self) -> i64; } trait Named where Self: Shape { fn n(&self) -> i64; } \
         struct Sq { side: i64 } impl Named for Sq { fn n(&self) -> i64 { 1 } } \
         pub fn run() -> i64 { 0 } run()",
    );
}

#[test]
fn language_receivers_take_named_lifetimes() {
    let value = run(r#"
        struct Counter { n: i64 }
        impl Counter {
            fn bump<'a>(&'a mut self) { self.n += 1; }
            fn total<'a>(&'a self, extra: &'a i64) -> i64 { self.n + *extra }
        }
        pub fn run() -> i64 {
            let mut counter = Counter { n: 40 };
            counter.bump();
            counter.total(&1)
        }
        run()
    "#);
    assert_eq!(value, 42);
}

#[test]
fn language_into_string_bounds_take_str_string_and_crate_impls() {
    let value = Engine::new()
        .eval::<String>(r#"
        struct Celsius { c: i64 }
        impl From<Celsius> for String { fn from(c: Celsius) -> String { format!("{}C", c.c) } }
        fn label<T: Into<String>>(content: T) -> String { let s: String = content.into(); s }
        fn borrowed(name: &str) -> String { label(name) }
        pub fn run() -> String {
            let owned = String::from("owned");
            format!("{} {} {} {} {}", label("literal"), label(owned.clone()), label(&owned), borrowed("slice"), label(Celsius { c: 21 }))
        }
        run()
    "#)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value, "literal owned owned slice 21C");
}

#[test]
fn language_conversions_to_generic_targets_and_prelude_containers() {
    let value = run(r#"
        struct Wrapper { n: i64 }
        impl From<i64> for Wrapper { fn from(n: i64) -> Wrapper { Wrapper { n } } }
        impl From<bool> for Wrapper { fn from(b: bool) -> Wrapper { Wrapper { n: if b { 7 } else { 8 } } } }
        struct Meters { m: i64 }
        impl From<Wrapper> for Meters { fn from(w: Wrapper) -> Meters { Meters { m: w.n * 10 } } }
        fn make<T: From<i64>>(n: i64) -> T { let t: T = n.into(); t }
        fn flag<T: From<bool>>(b: bool) -> T { b.into() }
        fn convert<S, T: From<S>>(s: S) -> T { T::from(s) }
        fn convert_into<S, T: From<S>>(s: S) -> T { s.into() }
        pub fn run() -> i64 {
            let a: Wrapper = make(5);
            let b: Wrapper = flag(true);
            let c: Wrapper = convert(3);
            let d: Meters = convert(Wrapper { n: 2 });
            let e: Wrapper = convert(false);
            let f: Meters = convert_into(Wrapper { n: 4 });
            let o = Option::from(9);
            let p: Option<i64> = 6.into();
            let v = Vec::from([1, 2, 3]);
            let w: Vec<i64> = [4, 5].into();
            a.n + b.n * 10 + c.n * 100 + d.m * 1000 + e.n * 100000 + f.m * 1000000
                + o.unwrap() * 100000000 + p.unwrap() + v.len() as i64 * 1000000000 + w.len() as i64
        }
        run()
    "#);
    assert_eq!(
        value,
        5 + 70 + 300 + 20000 + 800000 + 40000000 + 900000000 + 6 + 3000000000 + 2
    );
    // No `From<f64>` for the target the bound names.
    rejected(
        "struct W { n: i64 } impl From<i64> for W { fn from(n: i64) -> W { W { n } } } \
         fn make<T: From<i64>>(n: i64) -> T { let t: T = 1.5.into(); t } \
         pub fn run() -> i64 { let w: W = make(1); w.n } run()",
    );
}

#[test]
fn language_runtime_errors_in_builtin_impls_point_at_source() {
    // A runtime error inside the function of a trait impl for a builtin
    // type translates to the failing expression in the user's source, as
    // one in the invoked function's own body does.
    let source = "trait Shape { fn area(&self) -> i64; }
impl Shape for i64 {
    fn area(&self) -> i64 {
        let v: Vec<i64> = Vec::new();
        v[3]
    }
}
pub fn run() -> i64 { 5.area() }
pub fn own() -> i64 { let v: Vec<i64> = Vec::new(); v[2] }
";
    let authority = crate::Shared::new(Engine::new())
        .compile_authority(source)
        .unwrap_or_else(|error| panic!("{error}"));
    let translated = |name: &str| {
        let item = authority
            .items()
            .find(|item| item.key().name() == name)
            .unwrap()
            .clone();
        let error = authority
            .invoke(&item, &mut [])
            .expect_err("indexing past the end fails at runtime");
        authority
            .item_evidence(&item)
            .expect("a compiled function retains its source")
            .translate(&error)
            .unwrap_or_else(|| panic!("`{name}` failure has no source location: {error}"))
    };
    let own = translated("own");
    assert_eq!(
        (own.start().line(), own.start().position()),
        (Some(9), Some(53))
    );
    let nested = translated("run");
    assert_eq!(
        (nested.start().line(), nested.start().position()),
        (Some(5), Some(9)),
        "the error is `v[3]` in the impl for `i64`"
    );
}

#[test]
fn language_generic_supertraits() {
    // `trait Both<T>: Convert<T>` and `trait Ord2: PartialOrd2<Self>`: a
    // supertrait takes arguments, substituted through elaboration, impl
    // checks, bounds and `dyn` lookup, as Rust elaborates them.
    let value = run_string(
        r#"
        trait Convert<T> { fn convert(&self) -> T; }
        trait Both<T>: Convert<T> {
            fn twice(&self) -> T;
            fn pair(&self) -> (T, T) { (self.convert(), self.twice()) }
        }
        trait PartialOrd2<Rhs> { fn lt2(&self, other: &Rhs) -> bool; }
        trait Ord2: PartialOrd2<Self> {
            fn cmp2(&self, other: &Self) -> i64 {
                if self.lt2(other) { -1 } else if other.lt2(self) { 1 } else { 0 }
            }
        }
        struct C { x: i64 }
        impl Convert<i64> for C { fn convert(&self) -> i64 { self.x } }
        impl Convert<String> for C { fn convert(&self) -> String { format!("c{}", self.x) } }
        impl Both<i64> for C { fn twice(&self) -> i64 { self.x * 2 } }
        impl PartialOrd2<C> for C { fn lt2(&self, other: &C) -> bool { self.x < other.x } }
        impl Ord2 for C {}
        fn sum<T: Both<i64>>(t: &T) -> i64 { let n: i64 = t.convert(); n + t.twice() }
        fn order<T: Ord2>(a: &T, b: &T) -> i64 { if a.lt2(b) { a.cmp2(b) } else { b.cmp2(a) * 10 } }
        fn object(b: &dyn Both<i64>) -> i64 { b.convert() + b.twice() }
        pub fn run() -> String {
            let c = C { x: 3 };
            let d = C { x: 5 };
            let (a, b) = c.pair();
            let s: String = c.convert();
            format!("{} {} {} {} {} {} {}", sum(&c), order(&c, &d), order(&d, &c), object(&c), a, b, s)
        }
        run()
    "#,
    );
    assert_eq!(value, "9 -1 -10 9 3 6 c3");
    // The impl of `Both<i64>` needs `Convert<i64>`, not another instance
    // (E0277).
    let missing = rejected(
        "trait Convert<T> { fn convert(&self) -> T; } trait Both<T>: Convert<T> { fn twice(&self) -> T; } \
         struct C { x: i64 } impl Convert<String> for C { fn convert(&self) -> String { String::new() } } \
         impl Both<i64> for C { fn twice(&self) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing.contains("Convert<i64>"), "{missing}");
    let missing_self = rejected(
        "trait PartialOrd2<Rhs> { fn lt2(&self, other: &Rhs) -> bool; } trait Ord2: PartialOrd2<Self> {} \
         struct C { x: i64 } struct D { x: i64 } \
         impl PartialOrd2<D> for C { fn lt2(&self, other: &D) -> bool { true } } \
         impl Ord2 for C {} pub fn run() -> i64 { 0 } run()",
    );
    assert!(missing_self.contains("PartialOrd2<C>"), "{missing_self}");
}

#[test]
fn language_context_typed_trait_calls() {
    // `Default::default()`, `From::from(x)` and `Into::into(x)` take the
    // type the context expects, as Rust infers it.
    let value = run_string(
        r#"
        #[derive(Default)]
        struct Config { width: i64, name: String }
        struct Meters { n: i64 }
        impl From<i64> for Meters { fn from(n: i64) -> Meters { Meters { n } } }
        struct Pair { a: Meters, b: Meters }
        fn total(m: Meters) -> i64 { m.n }
        fn zero<T: Default>() -> T { let t: T = Default::default(); t }
        pub fn run() -> String {
            let c: Config = Default::default();
            let n: i64 = Default::default();
            let s: String = std::default::Default::default();
            let v: Vec<i64> = Default::default();
            let o: Option<i64> = Default::default();
            let z: i64 = zero();
            let m: Meters = From::from(4);
            let p = Pair { a: From::from(1), b: Into::into(2) };
            let k: Meters = std::convert::Into::into(7);
            format!("{} {:?} {} {:?} {} {:?} {} {} {} {} {} {}", c.width, c.name, n, s, v.len(), o, z,
                m.n, p.a.n, p.b.n, k.n, total(From::from(9)))
        }
        run()
    "#,
    );
    assert_eq!(value, r#"0 "" 0 "" 0 None 0 4 1 2 7 9"#);
    // Nothing fixes the type (E0790).
    let unknown = rejected("pub fn run() -> i64 { let d = Default::default(); 0 } run()");
    assert!(unknown.contains("E0790"), "{unknown}");
    rejected(
        "struct S { x: i64 } pub fn run() -> i64 { let s: S = Default::default(); s.x } run()",
    );
}

#[test]
fn language_qualified_calls_through_std_traits() {
    // `Clone::clone(&x)`, `PartialEq::eq(&a, &b)`, `ToString::to_string(&x)`
    // and `fmt::Display::fmt(&self.inner, f)` name the std trait's function, as
    // Rust resolves a fully qualified call.
    let value = run_string(
        r#"
        use std::fmt;
        #[derive(Clone, PartialEq, Debug)]
        struct P { x: i64 }
        struct Celsius { c: i64 }
        impl fmt::Display for Celsius {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "{}C", self.c) }
        }
        struct Wrapper { inner: Celsius }
        impl fmt::Display for Wrapper {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { fmt::Display::fmt(&self.inner, f) }
        }
        struct Shown { p: P }
        impl fmt::Debug for Shown {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { std::fmt::Debug::fmt(&self.p, f) }
        }
        pub fn run() -> String {
            let p = P { x: 3 };
            let q = Clone::clone(&p);
            let v = vec![1, 2];
            let w: Vec<i64> = Clone::clone(&v);
            let same = PartialEq::eq(&p, &q);
            let differ = PartialEq::ne(&p, &P { x: 4 });
            let less = PartialOrd::lt(&1, &2);
            let ge = std::cmp::PartialOrd::ge(&2.5, &3.0);
            let text = ToString::to_string(&5);
            let shown = ToString::to_string(&Wrapper { inner: Celsius { c: 21 } });
            format!("{} {:?} {} {} {} {} {} {} {:?}", q.x, w, same, differ, less, ge, text, shown, Shown { p })
        }
        run()
    "#,
    );
    assert_eq!(value, "3 [1, 2] true true true false 5 21C P { x: 3 }");
    // Rust's arity: `PartialEq::eq` takes two arguments.
    rejected("pub fn run() -> i64 { if PartialEq::eq(&1) { 1 } else { 0 } } run()");
    // `Clone` is a capability the type must have.
    rejected(
        "struct S { x: i64 } pub fn run() -> i64 { let s = S { x: 1 }; Clone::clone(&s).x } run()",
    );
}

#[test]
fn language_by_value_trait_methods_before_autoref_inherent_methods() {
    // Rust probes a receiver by value before it autorefs it: on a `String`
    // value, a trait's `len(self)` comes before `String::len(&self)`, and
    // on a `Vec` a trait's `push(&self, ..)` before `Vec::push(&mut self,
    // ..)`; through a reference, or against an inherent method that also
    // takes `self` (`Option::unwrap`), the inherent method stays first.
    let value = run_string(
        r#"
        trait Size { fn len(self) -> i64; }
        impl Size for String { fn len(self) -> i64 { 100 } }
        trait Peek { fn push(&self, x: i64) -> i64; }
        impl Peek for Vec<i64> { fn push(&self, x: i64) -> i64 { x * 10 } }
        trait Open { fn unwrap(self) -> i64; }
        impl Open for Option<i64> { fn unwrap(self) -> i64 { -1 } }
        trait Shared { fn is_empty(&self) -> bool; }
        impl Shared for String { fn is_empty(&self) -> bool { true } }
        pub fn run() -> String {
            let s = String::from("abc");
            let r = &s;
            let by_reference = r.len();
            let empty = s.is_empty();
            let by_value = s.len();
            let v = vec![1];
            let pushed = v.push(4);
            let o = Some(7);
            format!("{} {} {} {} {}", by_value, by_reference, empty, pushed, o.unwrap())
        }
        run()
    "#,
    );
    assert_eq!(value, "100 3 false 40 7");
}

#[test]
fn language_qualified_default_calls() {
    // `<T as Default>::default()` is `T::default()` for a generic, a crate
    // type or a builtin, as Rust resolves a fully qualified call.
    let value = run_string(
        r#"
        #[derive(Default, Debug)]
        struct P { x: i64 }
        fn make<T: Default>() -> T { <T as Default>::default() }
        fn make2<T: Default>() -> T { T::default() }
        pub fn run() -> String {
            let a: i64 = make();
            let b: P = make2();
            let c = <P as Default>::default();
            let d = <i64 as Default>::default() + 1;
            let e = <String as std::default::Default>::default();
            format!("{} {:?} {:?} {} {:?}", a, b, c, d, e)
        }
        run()
    "#,
    );
    assert_eq!(value, "0 P { x: 0 } P { x: 0 } 1 \"\"");
    // The written type must be `Default`.
    rejected("struct S { x: i64 } pub fn run() -> i64 { <S as Default>::default().x } run()");
    rejected(
        "fn make<T>() -> T { <T as Default>::default() } pub fn run() -> i64 { make() } run()",
    );
}

#[test]
fn language_ordering() {
    // `std::cmp::Ordering` from `cmp`/`partial_cmp` on builtins, matched,
    // printed, reversed and chained as Rust has it.
    let value = run_string(
        r#"
        use std::cmp::Ordering;
        fn name(o: Ordering) -> String {
            match o { Ordering::Less => "lt".to_string(), Ordering::Equal => "eq".to_string(), Ordering::Greater => "gt".to_string() }
        }
        pub fn run() -> String {
            let (a, b) = (1, 2);
            let s = String::from("b");
            let by_str = s.cmp(&String::from("a"));
            let nan = 0.0 / 0.0;
            let floats = 1.5.partial_cmp(&0.5);
            let unordered = nan.partial_cmp(&1.0);
            let chained = a.cmp(&a).then(b.cmp(&a)).reverse();
            let q = Ord::cmp(&'a', &'b');
            let p = std::cmp::PartialOrd::partial_cmp(&true, &false);
            let o: Option<Ordering> = Some(Ordering::Equal);
            format!(
                "{} {} {:?} {:?} {:?} {:?} {:?} {:?} {} {} {}",
                name(a.cmp(&b)), name(by_str), floats, unordered, chained, q, p, o,
                Ordering::Less < Ordering::Greater, a.cmp(&b).is_lt(), Some(1) < Some(2)
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "lt gt Some(Greater) None Less Less Some(Greater) Some(Equal) true true true"
    );
    // `f64` is only `PartialOrd`; a match on `Ordering` must be exhaustive.
    rejected("pub fn run() -> i64 { let o = 1.0.cmp(&2.0); 0 } run()");
    rejected(
        "use std::cmp::Ordering; pub fn run() -> i64 { match 1.cmp(&2) { Ordering::Less => 1, Ordering::Equal => 0 } } run()",
    );
}

#[test]
fn language_ord_impls() {
    // Hand-written `PartialOrd` and `Ord` are the type's comparison: its
    // methods, `<` and the qualified calls, and builtins satisfy `T: Ord`.
    let value = run_string(
        r#"
        use std::cmp::Ordering;
        #[derive(PartialEq, Eq, Debug)]
        struct Version { major: i64, minor: i64 }
        impl PartialOrd for Version {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
        }
        impl Ord for Version {
            fn cmp(&self, other: &Self) -> Ordering {
                self.major.cmp(&other.major).then(self.minor.cmp(&other.minor))
            }
        }
        fn largest<T: PartialOrd>(a: T, b: T) -> T { if a > b { a } else { b } }
        fn order<T: Ord>(a: &T, b: &T) -> Ordering { a.cmp(b) }
        pub fn run() -> String {
            let old = Version { major: 1, minor: 4 };
            let new = Version { major: 1, minor: 10 };
            format!(
                "{:?} {} {:?} {:?} {} {:?} {:?}",
                old.cmp(&new), old < new, Ord::cmp(&new, &old), largest(3, 8),
                largest(String::from("a"), String::from("b")), order(&2, &2),
                largest(Version { major: 2, minor: 0 }, Version { major: 1, minor: 9 })
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Less true Greater 8 b Equal Version { major: 2, minor: 0 }"
    );
    // `Ord` needs `PartialOrd` and `Eq`, as Rust's supertraits have it.
    let error = rejected(
        "use std::cmp::Ordering; #[derive(PartialEq, Eq)] struct S { x: i64 } \
         impl Ord for S { fn cmp(&self, other: &Self) -> Ordering { self.x.cmp(&other.x) } } \
         pub fn run() -> i64 { 0 } run()",
    );
    assert!(error.contains("PartialOrd"), "{error}");
    // A type without `PartialOrd` cannot be compared.
    rejected("struct S { x: i64 } pub fn run() -> i64 { if S { x: 1 } < S { x: 2 } { 1 } else { 0 } } run()");
}

#[test]
fn language_by_value_trait_methods_before_autoref_on_crate_types() {
    // Rust's probe on a crate type: on a value, a trait's `get(self)` comes
    // before the inherent `get(&self)`, and a trait's `bump(&self)` before
    // an inherent `bump(&mut self)`; through `&S` the `&self` method is
    // found first, so the inherent one stays.
    let value = run(r#"
        #[derive(Clone, Copy)]
        struct S { x: i64 }
        impl S {
            fn get(&self) -> i64 { 1 }
            fn bump(&mut self) -> i64 { 10 }
            fn same(&self) -> i64 { 100 }
        }
        trait G { fn get(self) -> i64; fn bump(&self) -> i64; fn same(&self) -> i64; }
        impl G for S {
            fn get(self) -> i64 { 2 }
            fn bump(&self) -> i64 { 20 }
            fn same(&self) -> i64 { 200 }
        }
        pub fn run() -> i64 {
            let mut s = S { x: 0 };
            let by_value = s.get() + s.bump() + s.same();
            let r = &s;
            let by_reference = r.get() + r.same();
            by_value * 1000 + by_reference
        }
        run()
    "#);
    assert_eq!(value, 122 * 1000 + 101);
}

#[test]
fn language_associated_type_bounds_naming_generic_traits() {
    // `type Item: Into<String>;` and `type Item: Convert<i64>;` bound an
    // associated type by a generic trait: each impl's type is checked
    // against the instance, and a generic body calls through it, as does
    // `T: Source<Item = U>` with `U: Convert<i64>`.
    let value = run_string(
        r#"
        trait Convert<T> { fn convert(&self) -> T; }
        struct N { n: i64 }
        impl From<N> for String { fn from(n: N) -> String { format!("n{}", n.n) } }
        impl Convert<i64> for N { fn convert(&self) -> i64 { self.n * 10 } }
        trait Source { type Item: Into<String> + Convert<i64>; fn get(&self) -> Self::Item; }
        struct S { k: i64 }
        impl Source for S { type Item = N; fn get(&self) -> N { N { n: 3 } } }
        fn show<T: Source>(t: &T) -> String { t.get().into() }
        fn number<T: Source>(t: &T) -> i64 { t.get().convert() }
        fn through<T: Source<Item = U>, U: Convert<i64>>(t: &T) -> i64 { t.get().convert() + 1 }
        pub fn run() -> String { let s = S { k: 0 }; format!("{} {} {}", show(&s), number(&s), through(&s)) }
        run()
    "#,
    );
    assert_eq!(value, "n3 30 31");
    // The impl's type must have the bound's instance (E0277).
    let error = rejected(
        r#"
        trait Convert<T> { fn convert(&self) -> T; }
        struct N { n: i64 }
        impl Convert<String> for N { fn convert(&self) -> String { String::new() } }
        trait Source { type Item: Convert<i64>; fn get(&self) -> Self::Item; }
        struct S { k: i64 }
        impl Source for S { type Item = N; fn get(&self) -> N { N { n: 3 } } }
        pub fn run() -> i64 { 0 }
        run()
    "#,
    );
    assert!(error.contains("Convert<i64>"), "{error}");
}

#[test]
fn language_turbofish_and_qualified_conversions() {
    // `Into::<T>::into(x)`, `From::<S>::from(x)`, `<T as From<S>>::from(x)`
    // and `<S as Into<T>>::into(x)` take their written types as Rust does:
    // the target, and the type the argument coerces to.
    let value = run_string(
        r#"
        struct N { n: i64 }
        impl From<N> for String { fn from(n: N) -> String { format!("n{}", n.n) } }
        pub fn run() -> String {
            let a = Into::<String>::into(N { n: 1 });
            let b = <String as From<N>>::from(N { n: 2 });
            let c: String = From::<N>::from(N { n: 3 });
            let d = <N as Into<String>>::into(N { n: 4 });
            let e = Into::<i64>::into(true) + i64::from(false);
            format!("{} {} {} {} {} {}", a, b, c, d, e, Into::<String>::into(N { n: 5 }).len())
        }
        run()
    "#,
    );
    assert_eq!(value, "n1 n2 n3 n4 1 2");
    // The written source type must be the argument's.
    rejected(
        "struct N { n: i64 } impl From<N> for String { fn from(n: N) -> String { String::new() } } \
         pub fn run() -> i64 { let s = <String as From<N>>::from(5); 0 } run()",
    );
    // `Into` takes one type argument (E0107).
    rejected("pub fn run() -> i64 { Into::<i64, i64>::into(1) } run()");
}

#[test]
fn language_unit_structs_and_constructor_values() {
    // `S` of `struct S;` is a value and a pattern; a variant constructor
    // bound to a local is called as the function it is in Rust.
    let value = run_string(
        r#"
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        struct Marker;
        impl Marker { fn n(&self) -> i64 { 7 } }
        #[derive(Debug, Default)]
        struct Holder { m: Marker, o: Option<Marker> }
        #[derive(Debug)]
        enum E { A(i64), P(i64, i64) }
        fn take(m: &Marker) -> i64 { match m { Marker => 5 } }
        pub fn run() -> String {
            let s = Marker;
            let t: Marker = Marker;
            let Marker = t;
            let k = match s { Marker => 1 };
            let h = Holder { m: Marker, o: Some(Marker) };
            let (f, g) = (E::P, E::A);
            format!(
                "{:?} {} {} {} {} {:?} {:?} {:?} {:?} {:?}",
                s, t.n(), k, s == Marker, take(&h.m), h, Holder::default(), f(1, 2), g(3),
                Some(4).map(E::A)
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Marker 7 1 true 5 Holder { m: Marker, o: Some(Marker) } Holder { m: Marker, o: None } \
         P(1, 2) A(3) Some(A(4))"
    );
    // The unit value has its struct's type, in expressions and patterns.
    rejected("struct S; pub fn run() -> i64 { let x: i64 = S; x } run()");
    rejected("struct S; pub fn run() -> i64 { match 1 { S => 1 } } run()");
    rejected("struct S; struct T; pub fn run() -> i64 { match T { S => 1 } } run()");
}

#[test]
fn language_derived_ord() {
    // `#[derive(PartialOrd, Ord)]` compares fields in declaration order,
    // and an enum's variants by declaration order, then their payloads.
    let value = run_string(
        r#"
        use std::cmp::Ordering;
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
        struct Version { major: i64, minor: i64 }
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
        enum Level { Low(i64), Mid, High(Version) }
        #[derive(PartialEq, PartialOrd)]
        struct Wrap<T> { x: T }
        fn largest<T: PartialOrd>(a: T, b: T) -> T { if a > b { a } else { b } }
        fn order<T: Ord>(a: &T, b: &T) -> Ordering { a.cmp(b) }
        pub fn run() -> String {
            let a = Version { major: 1, minor: 4 };
            let b = Version { major: 1, minor: 10 };
            format!(
                "{} {:?} {:?} {} {} {:?} {:?} {:?} {:?} {} {} {}",
                a < b, a.cmp(&b), b.partial_cmp(&a), Level::Low(9) < Level::Mid,
                Level::High(a.clone()) > Level::Mid, Level::Low(1).cmp(&Level::Low(2)),
                largest(a.clone(), b.clone()), order(&Level::Mid, &Level::Mid),
                Ord::cmp(&a, &b), Some(a.clone()) < Some(b.clone()),
                Wrap { x: 2.5 } > Wrap { x: 1.0 }, (a.clone(), 1) <= (a, 0)
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "true Less Some(Greater) true true Less Version { major: 1, minor: 10 } Equal Less true \
         true false"
    );
    // `Ord` needs `Eq` and `PartialOrd`; `f64` is not `Ord`.
    let error =
        rejected("#[derive(PartialEq, Ord)] struct S { x: i64 } pub fn run() -> i64 { 0 } run()");
    assert!(error.contains("Ord"), "{error}");
    rejected(
        "#[derive(PartialEq, Eq, PartialOrd, Ord)] struct S { x: f64 } pub fn run() -> i64 { 0 } run()",
    );
    // Only a derive or an impl orders a type.
    rejected(
        "#[derive(PartialEq)] struct S { x: i64 } \
         pub fn run() -> i64 { if S { x: 1 } < S { x: 2 } { 1 } else { 0 } } run()",
    );
    rejected(
        "#[derive(PartialEq, PartialOrd)] struct S { x: f64 } \
         pub fn run() -> i64 { let o = S { x: 1.0 }.cmp(&S { x: 2.0 }); 0 } run()",
    );
}

#[test]
fn language_ordering_debug_in_derived_debug() {
    // A derived `Debug` prints an `Ordering` field or payload by its
    // variant, also inside `Vec`, `Option` and tuples; tuple fields hold.
    let value = run_string(
        r#"
        use std::cmp::Ordering;
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        struct Step { o: Ordering, v: Vec<Ordering>, p: Option<Ordering>, t: (Ordering, i64), n: i64 }
        #[derive(Debug)]
        enum Event { Ranked(Ordering), Maybe(Option<Ordering>) }
        pub fn run() -> String {
            let s = Step {
                o: 1.cmp(&2), v: vec![Ordering::Equal], p: Some(Ordering::Greater),
                t: (Ordering::Less, 3), n: -1,
            };
            format!(
                "{:?} {:?} {:?} {:?} {:?} {}",
                s, Event::Ranked(Ordering::Greater), Event::Maybe(None), vec![Ordering::Less],
                s.t, s.clone() < s
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Step { o: Less, v: [Equal], p: Some(Greater), t: (Less, 3), n: -1 } \
         Ranked(Greater) Maybe(None) [Less] (Less, 3) false"
    );
    // `Ordering` has no `Default`.
    rejected(
        "use std::cmp::Ordering; #[derive(Default)] struct S { o: Ordering } \
         pub fn run() -> i64 { 0 } run()",
    );
}

#[test]
fn language_ord_helpers() {
    // `min`, `max` and `clamp` through `Ord` (and `f64`'s own), sorting,
    // `iter().min()` and its kin, `then_with` and `std::cmp::{min, max}`,
    // on builtins, derived and hand-written `Ord` types and generics.
    let value = run_string(
        r#"
        use std::cmp::{max, Ordering};
        use std::cmp;
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
        struct Score { points: i64 }
        #[derive(Debug, PartialEq, Eq)]
        struct Rank { n: i64 }
        impl PartialOrd for Rank {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
        }
        impl Ord for Rank {
            fn cmp(&self, other: &Self) -> Ordering { other.n.cmp(&self.n) }
        }
        fn smaller<T: Ord>(a: T, b: T) -> T { a.min(b) }
        pub fn run() -> String {
            let mut ranks = vec![Rank { n: 1 }, Rank { n: 3 }, Rank { n: 2 }];
            ranks.sort();
            let mut scores = vec![Score { points: 2 }, Score { points: 1 }];
            scores.sort_unstable();
            let mut down = vec![3, 1, 2];
            down.sort_by(|a, b| b.cmp(a));
            let mut pairs = vec![(1, 'b'), (0, 'a'), (1, 'a')];
            pairs.sort_by_key(|p| p.0);
            let n = vec![3, 1, 2];
            let none: Vec<i64> = Vec::new();
            let o = 1.cmp(&1).then_with(|| 2.cmp(&1));
            format!(
                "{} {} {} {:?} {} {:?} {:?} {:?} {} | {:?} {:?} {:?} {:?} | {:?} {:?} {:?} {:?} {:?} {:?} {:?} | {:?} {} {} {} {} {} {} {}",
                3.min(5), 3.max(5), 9.clamp(1, 4), 2.5.min(1.0), (1, 2).min((1, 1)).1,
                Score { points: 3 }.min(Score { points: 1 }), Rank { n: 3 }.max(Rank { n: 1 }),
                smaller(Rank { n: 1 }, Rank { n: 2 }), smaller(4, 2),
                ranks, scores, down, pairs,
                n.iter().min(), n.iter().max(), n.iter().min_by_key(|x| (*x - 2) * (*x - 2)),
                n.iter().max_by_key(|x| -*x), scores.iter().max(), none.iter().min(),
                n.iter().max_by(|a, b| b.cmp(a)),
                o, std::cmp::min(2, 1), std::cmp::max(2, 1), Ord::max(4, 6), Ord::clamp(9, 1, 4),
                max(3, 8), cmp::min(5, 4), core::cmp::max('a', 'b')
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "3 5 4 1.0 1 Score { points: 1 } Rank { n: 1 } Rank { n: 2 } 2 | \
         [Rank { n: 3 }, Rank { n: 2 }, Rank { n: 1 }] [Score { points: 1 }, Score { points: 2 }] \
         [3, 2, 1] [(0, 'a'), (1, 'b'), (1, 'a')] | Some(1) Some(3) Some(2) Some(1) \
         Some(Score { points: 2 }) None Some(1) | Greater 1 2 6 4 8 4 b"
    );
    // `f64` is not `Ord`; sorting needs a mutable local; `clamp` asserts
    // its bounds are ordered.
    rejected("pub fn run() -> i64 { let mut v = vec![1.0]; v.sort(); 0 } run()");
    rejected("pub fn run() -> i64 { let v = vec![2, 1]; v.sort(); v[0] } run()");
    let error = rejected("pub fn run() -> i64 { 5.clamp(4, 1) } run()");
    assert!(error.contains("min <= max"), "{error}");
}

#[test]
fn language_ord_supertrait_and_qualified_std_calls() {
    // `T: Ord` has `PartialOrd`'s `partial_cmp`; a qualified std call
    // checks its written `Self`; turbofish on a full `std::convert` path.
    let value = run_string(
        r#"
        use std::cmp::Ordering;
        use std::fmt;
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        struct P { x: i64 }
        struct N { n: i64 }
        impl From<N> for String { fn from(n: N) -> String { format!("n{}", n.n) } }
        struct Shown { n: i64 }
        impl fmt::Display for Shown {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { <i64 as fmt::Display>::fmt(&self.n, f) }
        }
        fn partial<T: Ord>(a: &T, b: &T) -> Option<Ordering> { a.partial_cmp(b) }
        fn copy<T: Clone>(t: &T) -> T { <T as Clone>::clone(t) }
        pub fn run() -> String {
            let p = P { x: 1 };
            let a = std::convert::Into::<String>::into(N { n: 1 });
            let b: String = core::convert::From::<N>::from(N { n: 2 });
            format!(
                "{:?} {:?} {:?} {} {:?} {:?} {} {} {} {:?} {} {} {}",
                partial(&1, &2), partial(&P { x: 3 }, &p), <P as Clone>::clone(&p),
                <P as PartialEq>::eq(&p, &p), <i64 as Ord>::cmp(&1, &2),
                <i64 as PartialOrd>::partial_cmp(&2, &1), <char as Ord>::max('a', 'z'),
                <bool as ToString>::to_string(&true), 3.clone(), copy(&p), a, b, Shown { n: 7 }
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Some(Less) Some(Greater) P { x: 1 } true Less Some(Greater) z true 3 P { x: 1 } n1 n2 7"
    );
    // The written `Self` is the first argument's type.
    rejected("pub fn run() -> i64 { <i64 as Clone>::clone(&String::new()); 0 } run()");
    rejected(
        "pub fn run() -> i64 { if <i64 as PartialEq>::eq(&true, &true) { 1 } else { 0 } } run()",
    );
    rejected("pub fn run() -> i64 { let o = <char as Ord>::cmp(&1, &2); 0 } run()");
}

#[test]
fn language_format_floats_radixes_and_pretty_debug() {
    // `f64` in `{}` and `{:?}` with precision, width, sign and zero
    // padding; radixes, `#` prefixes and `{:#?}`; inline arguments.
    let value = run_string(
        r#"
        #[derive(Debug)]
        struct Point { x: i64, label: String }
        #[derive(Debug)]
        enum Shape { Dot(Point), Empty }
        pub fn run() -> String {
            let x = 1.5;
            let y = 2.0;
            let z = -0.25;
            let big = 1e20;
            let n = 255;
            let s = "ab";
            let v: Vec<f64> = vec![0.5, 1.0];
            let p = Point { x: 1, label: "{a}, (b)".to_string() };
            let pretty = format!("{:#?}", Shape::Dot(p));
            format!(
                "{} {} {:?} {:.2} [{:8.3}] [{:<6}] [{:>6}] [{:^7}] {:+} {:08.2} {x} {y:?} {big} {:.0} {:.1?} \
                 {:5}| {:x} {:X} {:#x} {:#06x} {:b} {:o} {:02x}{:02x} {:?} {:#?} {:#?}|{}",
                x, y, y, x, z, n, s, s, y, z, 2.5, 0.25, x, n, n, n, n, 5, 8, 1, 171, v, (1,), Shape::Empty,
                pretty
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "1.5 2 2.0 1.50 [  -0.250] [255   ] [    ab] [  ab   ] +2 -0000.25 1.5 2.0 \
         100000000000000000000 2 0.2   1.5| ff FF 0xff 0x00ff 101 10 01ab [0.5, 1.0] (\n    1,\n) Empty|\
         Dot(\n    Point {\n        x: 1,\n        label: \"{a}, (b)\",\n    },\n)"
    );
    rejected(r#"pub fn run() -> i64 { let s = format!("{:x}", 1.5); 0 } run()"#);
    rejected(r#"pub fn run() -> i64 { let s = format!("{:z}", 1); 0 } run()"#);
}

#[test]
fn language_tuple_structs() {
    // Tuple structs: construction, `.0`, patterns in `let`, `match` and
    // `if let`, generics, derives, the constructor as a
    // function value, and `pub` fields.
    let value = run_string(
        r#"
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Meters(pub f64);
        #[derive(Debug, PartialEq)]
        pub struct Origin;
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        struct W(i64);
        #[derive(Debug, Clone, PartialEq)]
        struct Pair<T>(T, String);
        struct Empty {}
        fn first<T: Clone>(p: &Pair<T>) -> T { p.0.clone() }
        fn unwrap(w: W) -> i64 { let W(n) = w; n }
        pub fn run() -> String {
            let w = W(3);
            let mut m = W(1);
            m.0 += 4;
            let W(inner) = w.clone();
            let p = Pair(vec![1, 2], "x".to_string());
            let Pair(v, s) = p.clone();
            let kind = match &w { W(0) => "zero", W(n) if *n > 2 => "big", W(_) => "small" };
            let found = if let Pair(xs, _) = &p { xs.len() } else { 0 };
            let make = W;
            let ws: Vec<W> = vec![make(2), W(1), Some(3).map(W).unwrap()];
            let mut sorted = ws.clone();
            sorted.sort();
            let made = Meters(2.5);
            let Meters(raw) = made;
            let origin = Origin;
            let at_origin = match origin { Origin => true };
            let _ = Empty {};
            format!(
                "{:?} {} {} {:?} {:?} {} {} {} {:?} {:?} {} {} {:?} {:?} {} {}",
                w, m.0, inner, first(&p), v, s, kind, found, ws, sorted,
                w < m, w == W(3), made, origin, raw, at_origin && unwrap(W(9)) == 9
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "W(3) 5 3 [1, 2] [1, 2] x big 2 [W(2), W(1), W(3)] [W(1), W(2), W(3)] \
         true true Meters(2.5) Origin 2.5 true"
    );
    // Rust's rules: arity, field types, and `struct S {}` is no value.
    rejected("struct W(i64); pub fn run() -> i64 { let w = W(1, 2); 0 } run()");
    rejected("struct W(i64); pub fn run() -> i64 { let w = W(true); 0 } run()");
    rejected("struct W(i64); pub fn run() -> i64 { let W(a, b) = W(1); a } run()");
    rejected("struct S {} pub fn run() -> i64 { let s = S; 0 } run()");
    rejected("struct W(i64); pub fn run() -> i64 { W(1).1 } run()");
}

#[test]
fn language_hand_written_reverse() {
    // A crate's own `Reverse`: a generic tuple struct whose derived
    // `PartialEq` holds under `T: PartialOrd` (`PartialOrd: PartialEq`).
    let value = run_string(
        r#"
        use std::cmp::Ordering;
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct Rev<T>(pub T);
        impl<T: PartialOrd> PartialOrd for Rev<T> {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> { other.0.partial_cmp(&self.0) }
        }
        impl<T: Ord> Ord for Rev<T> {
            fn cmp(&self, other: &Self) -> Ordering { other.0.cmp(&self.0) }
        }
        pub fn run() -> String {
            let mut v = vec![1, 3, 2];
            v.sort_by_key(|x| Rev(*x));
            let mut w = vec![Rev(1), Rev(3)];
            w.sort();
            format!("{:?} {:?} {}", v, w, Rev(1) < Rev(2))
        }
        run()
    "#,
    );
    assert_eq!(value, "[3, 2, 1] [Rev(3), Rev(1)] false");
}

#[test]
fn language_iterators() {
    // Rust's iterators: sources, adaptors fused into one loop, consumers,
    // `for` over a chain, `collect` by its target, and an iterator kept in
    // a local.
    let value = run_string(
        r#"
        fn evens(n: i64) -> Vec<i64> { (0..n).filter(|x| x % 2 == 0).collect() }
        fn reversed(s: &str) -> String { s.chars().rev().collect() }
        pub fn run() -> String {
            let v = vec![3, 1, 4, 1, 5, 9, 2, 6];
            let doubled: Vec<i64> = v.iter().map(|x| x * 2).collect();
            let big = v.iter().filter(|x| **x > 3).count();
            let total: i64 = v.iter().sum();
            let product = (1..=5).product::<i64>();
            let firsts = v.iter().take(3).cloned().collect::<Vec<i64>>();
            let rest: Vec<i64> = v.iter().skip(5).copied().collect();
            let pairs: Vec<(usize, i64)> = v.iter().enumerate().filter(|(i, _)| i % 3 == 0).map(|(i, x)| (i, *x)).collect();
            let zipped: Vec<(i64, char)> = (1..).zip("abc".chars()).collect();
            let chained: Vec<i64> = (0..2).chain(v.iter().rev().take(2).copied()).collect();
            let flat: Vec<i64> = (1..4).flat_map(|n| (0..n).map(move |k| n * 10 + k)).collect();
            let options = vec![Some(1), None, Some(3)];
            let parsed: Vec<i64> = options.into_iter().flatten().collect();
            let halves: Vec<i64> = v.iter().filter_map(|x| if x % 2 == 0 { Some(x / 2) } else { None }).collect();
            let folded = v.iter().fold(String::new(), |acc, x| format!("{acc}{x}"));
            let found = v.iter().find(|x| **x > 4);
            let at = v.iter().position(|x| *x == 9);
            let any = v.iter().any(|x| *x == 2);
            let all = v.iter().all(|x| *x > 0);
            let last = v.iter().map(|x| x + 1).last();
            let nth = (10..20).step_by(3).nth(2);
            let while_small: Vec<i64> = v.iter().copied().take_while(|x| *x < 5).collect();
            let after: Vec<i64> = v.iter().copied().skip_while(|x| *x < 5).collect();
            let (low, high): (Vec<i64>, Vec<i64>) = v.iter().copied().partition(|x| *x < 4);
            let longest = vec!["a", "abc", "ab"].into_iter().max_by_key(|s| s.len());
            let smallest = v.iter().min();
            let mut it = v.iter().map(|x| x * 10);
            let first = it.next();
            let second = it.next();
            let mut sum = 0;
            for (i, x) in v.iter().enumerate().skip(1) {
                if i > 4 { break; }
                if *x == 1 { continue; }
                sum += x;
            }
            for n in (0..3).rev() { sum = sum * 10 + n; }
            let mut seen = Vec::new();
            v.iter().for_each(|x| seen.push(*x));
            let infinite: i64 = (1..).map(|x| x * x).take_while(|x| *x < 50).sum();
            format!(
                "{:?} {} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {} {:?} {:?} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {} {} {:?} {}",
                doubled, big, total, product, firsts, rest, pairs, zipped, chained, flat, parsed, halves,
                folded, found, at, any, all, last, nth, while_small, after, low, high, longest, smallest,
                first, second, sum, seen.len(), evens(7), reversed("abc")
            ) + &format!(" {}", infinite)
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "[6, 2, 8, 2, 10, 18, 4, 12] 4 31 120 [3, 1, 4] [9, 2, 6] [(0, 3), (3, 1), (6, 2)] \
         [(1, 'a'), (2, 'b'), (3, 'c')] [0, 1, 6, 2] [10, 20, 21, 30, 31, 32] [1, 3] [2, 1, 3] \
         31415926 Some(5) Some(5) true true Some(7) Some(16) [3, 1, 4, 1] [5, 9, 2, 6] [3, 1, 1, 2] \
         [4, 5, 9, 6] Some(\"abc\") Some(1) Some(30) Some(10) 9210 8 [0, 2, 4, 6] cba 140"
    );
    // Rust's rules: adaptors are not `Vec` methods; `sum` needs numbers;
    // `next` needs a mutable iterator.
    rejected("pub fn run() -> i64 { let v = vec![1]; v.map(|x| x).len() } run()");
    rejected(r#"pub fn run() -> i64 { let n: i64 = vec!["a"].into_iter().sum(); n } run()"#);
    rejected("pub fn run() -> i64 { let it = vec![1].into_iter(); it.next().unwrap() } run()");
}

#[test]
fn language_user_iterators() {
    // A crate type implementing `Iterator` gets the adaptors and `for`.
    let value = run_string(
        r#"
        struct Countdown { n: i64 }
        impl Iterator for Countdown {
            type Item = i64;
            fn next(&mut self) -> Option<i64> {
                if self.n == 0 { None } else { self.n -= 1; Some(self.n + 1) }
            }
        }
        pub fn run() -> String {
            let mut c = Countdown { n: 2 };
            let first = c.next();
            let doubled: Vec<i64> = Countdown { n: 4 }.map(|x| x * 2).collect();
            let mut seen = 0;
            let three = Countdown { n: 3 };
            for x in three { seen = seen * 10 + x; }
            let total: i64 = Countdown { n: 100 }.take(3).sum();
            format!("{:?} {:?} {} {}", first, doubled, seen, total)
        }
        run()
    "#,
    );
    assert_eq!(value, "Some(2) [8, 6, 4, 2] 321 297");
}

#[test]
fn language_maps_and_sets() {
    // Rust's `HashMap`, `HashSet`, `BTreeMap` and `BTreeSet`: entries kept
    // sorted by key, so every kind iterates in key order.
    let value = run_string(
        r#"
        use std::collections::{HashMap, HashSet, BTreeMap};
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        enum Tone { Warm, Cool }
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        struct Key { id: i64, name: String }
        #[derive(Debug, Clone, Default)]
        struct Index { by_name: HashMap<String, i64> }
        fn count(words: Vec<&str>) -> HashMap<String, i64> {
            let mut counts = HashMap::new();
            for word in words { *counts.entry(word.to_string()).or_insert(0) += 1; }
            counts
        }
        fn total(map: &HashMap<String, i64>) -> i64 {
            let mut sum = 0;
            for (_, v) in map { sum += v; }
            sum
        }
        pub fn run() -> String {
            let mut m: HashMap<i64, &str> = HashMap::new();
            let first = m.insert(3, "c");
            m.insert(1, "a");
            let replaced = m.insert(3, "C");
            m.insert(2, "b");
            let got = m.get(&2).cloned();
            let missing = m.get(&9);
            let has = m.contains_key(&1);
            let removed = m.remove(&1);
            let indexed = m[&3];
            let keys: Vec<i64> = m.keys().copied().collect();
            let values: Vec<&str> = m.values().cloned().collect();
            let counts = count(vec!["b", "a", "b", "c", "b"]);
            let mut groups: BTreeMap<bool, Vec<i64>> = BTreeMap::new();
            for n in 1..7 { groups.entry(n % 2 == 0).or_default().push(n); }
            let mut tones = HashMap::new();
            tones.entry(Tone::Cool).or_insert_with(|| 10);
            tones.entry(Tone::Cool).and_modify(|v| *v += 1).or_insert(0);
            tones.entry(Tone::Warm).and_modify(|v| *v += 1).or_insert(5);
            let mut seen = HashSet::new();
            let fresh = seen.insert(4);
            let again = seen.insert(4);
            seen.insert(2);
            let set: HashSet<i64> = vec![3, 1, 3, 2].into_iter().collect();
            let squares: HashMap<i64, i64> = (1..4).map(|x| (x, x * x)).collect();
            let from = BTreeMap::from([(2, 'b'), (1, 'a')]);
            let mut keyed = HashMap::new();
            keyed.insert(Key { id: 2, name: "two".to_string() }, true);
            keyed.insert(Key { id: 1, name: "one".to_string() }, false);
            let mut index = Index::default();
            index.by_name.insert("x".to_string(), 7);
            let copy = index.clone();
            let mut pairs = Vec::new();
            for (k, v) in &squares { pairs.push(k * 10 + v); }
            let mut extended = set.clone();
            extended.extend(vec![9, 0]);
            format!(
                "{:?} {:?} {:?} {:?} {:?} {} {:?} {} {:?} {:?} {:?} {} {:?} {:?} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {} {} {} {:?} {:?} {}",
                m, first, replaced, got, missing, has, removed, indexed, keys, values,
                counts, total(&counts), groups, tones, fresh, again, seen, set, squares,
                from, keyed.len(), copy, set.contains(&2), m.len(), m.is_empty(),
                pairs, extended, squares == (1..4).map(|x| (x, x * x)).collect::<HashMap<i64, i64>>()
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "{2: \"b\", 3: \"C\"} None Some(\"c\") Some(\"b\") None true Some(\"a\") C [2, 3] [\"b\", \"C\"] \
         {\"a\": 1, \"b\": 3, \"c\": 1} 5 {false: [1, 3, 5], true: [2, 4, 6]} {Warm: 5, Cool: 11} \
         true false {2, 4} {1, 2, 3} {1: 1, 2: 4, 3: 9} {1: 'a', 2: 'b'} 2 \
         Index { by_name: {\"x\": 7} } true 2 false [11, 24, 39] {0, 1, 2, 3, 9} true"
    );
    // Fields changed through `&mut self`, generic keys, full paths, nested
    // maps and B-tree sets.
    let value = run_string(
        r#"
        use std::collections::BTreeSet;
        use std::hash::Hash;
        #[derive(Debug, Default, PartialEq)]
        struct Board { cells: std::collections::HashMap<(i64, i64), char> }
        impl Board {
            fn place(&mut self, x: i64, y: i64, c: char) -> Option<char> { self.cells.insert((x, y), c) }
            fn at(&self, x: i64, y: i64) -> Option<char> { self.cells.get(&(x, y)).copied() }
        }
        fn distinct<T: Hash + Eq + Clone>(items: Vec<T>) -> usize {
            let mut seen = std::collections::HashSet::new();
            for item in items { seen.insert(item); }
            seen.len()
        }
        pub fn run() -> String {
            let mut board = Board::default();
            board.place(1, 0, 'x');
            let old = board.place(1, 0, 'o');
            board.place(0, 2, 'x');
            let mut nested: std::collections::HashMap<String, std::collections::HashMap<i64, i64>> =
                std::collections::HashMap::new();
            *nested.entry("a".to_string()).or_default().entry(1).or_insert(0) += 5;
            *nested.entry("a".to_string()).or_default().entry(1).or_insert(0) += 1;
            let mut letters = BTreeSet::new();
            for c in "hello".chars() { letters.insert(c); }
            let removed = letters.remove(&'l');
            let doubled: Vec<String> = board.cells.iter().map(|((x, y), c)| format!("{x}{y}{c}")).collect();
            let owned: Vec<((i64, i64), char)> = board.cells.clone().into_iter().collect();
            let key = "a".to_string();
            format!(
                "{:?} {:?} {:?} {} {:?} {:?} {} {:?} {:?} {:?} {} {:#?}",
                old, board.at(1, 0), board.at(5, 5), distinct(vec![1, 2, 1, 3]), nested,
                letters, removed, doubled, owned, nested.get(&key).map(|inner| inner.len()),
                board == Board::default(), letters
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Some('x') Some('o') None 3 {\"a\": {1: 6}} {'e', 'h', 'o'} true [\"02x\", \"10o\"] \
         [((0, 2), 'x'), ((1, 0), 'o')] Some(1) false {\n    'e',\n    'h',\n    'o',\n}"
    );
    // Rust's rules: keys need `Hash` and `Eq`; `get` takes a reference;
    // changing a map needs it mutable; `f64` is not `Hash`.
    rejected("use std::collections::HashMap; pub fn run() -> i64 { let mut m = HashMap::new(); m.insert(1.5, 1); 0 } run()");
    rejected("use std::collections::HashMap; pub fn run() -> i64 { let mut m = HashMap::new(); m.insert(1, 2); *m.get(1).unwrap() } run()");
    rejected("use std::collections::HashMap; pub fn run() -> i64 { let m: HashMap<i64, i64> = HashMap::new(); m.insert(1, 2); 0 } run()");
    rejected("struct P { x: i64 } use std::collections::HashSet; pub fn run() -> i64 { let mut s = HashSet::new(); s.insert(P { x: 1 }); 0 } run()");
}

#[test]
fn language_iterator_signatures() {
    // `impl Iterator<Item = T>` and `impl IntoIterator<Item = T>` in
    // argument and return position, and `I: Iterator<Item = T>` bounds.
    let value = run_string(
        r#"
        fn evens(n: i64) -> impl Iterator<Item = i64> { (0..n).filter(|x| x % 2 == 0) }
        fn total(items: impl Iterator<Item = i64>) -> i64 { items.sum() }
        fn doubled<I: Iterator<Item = i64>>(items: I) -> Vec<I::Item> { items.map(|x| x * 2).collect() }
        fn longest<'a, I>(words: I) -> usize where I: IntoIterator<Item = &'a str> {
            let mut best = 0;
            for word in words { if word.len() > best { best = word.len(); } }
            best
        }
        fn count_all(items: impl IntoIterator<Item = i64>) -> usize { items.into_iter().count() }
        fn pairs(v: &Vec<i64>) -> impl Iterator<Item = (usize, i64)> + '_ { v.iter().copied().enumerate() }
        pub fn run() -> String {
            let v = vec![1, 2, 3];
            format!(
                "{} {} {:?} {} {} {} {:?} {}",
                total(evens(7)), total(v.iter().copied()), doubled(evens(5)),
                longest(vec!["a", "abc", "ab"]), count_all(v.clone()), count_all(0..4),
                pairs(&v).collect::<Vec<(usize, i64)>>(), evens(10).map(|x| x + 1).sum::<i64>()
            )
        }
        run()
    "#,
    );
    assert_eq!(value, "12 6 [0, 4, 8] 3 3 4 [(0, 1), (1, 2), (2, 3)] 25");
    // Rust's rules: a `Vec` is not an `Iterator`, and the items must match.
    rejected("fn total(items: impl Iterator<Item = i64>) -> i64 { items.sum() } pub fn run() -> i64 { total(vec![1]) } run()");
    rejected("fn total(items: impl Iterator<Item = i64>) -> i64 { items.sum() } pub fn run() -> i64 { total(\"ab\".chars()) } run()");
}

#[test]
fn language_lazy_iterators() {
    // An iterator bound to an immutable local is its chain: binding an
    // infinite one finishes, its sources are read where it is bound, and
    // `rev` after `map`/`filter` runs their closures from the last item.
    let value = run_string(
        r#"
        pub fn run() -> String {
            let squares = (1..).map(|x| x * x);
            let firsts: Vec<i64> = squares.take(3).collect();
            let naturals = 1..;
            let big = naturals.map(|x| x * 3).find(|x| *x > 10);
            let mut v = vec![1, 2];
            let kept = v.clone().into_iter();
            v.push(3);
            let counted = kept.count();
            let mut log = Vec::new();
            let last: Vec<i64> = v.iter().map(|x| { log.push(*x); x * 10 }).rev().take(1).collect();
            let odd: Vec<i64> = v.iter().filter(|x| **x % 2 == 1).rev().copied().collect();
            let back: String = "abc".chars().filter(|c| *c != 'b').rev().collect();
            let evens = (0..10).filter(|x| x % 2 == 0);
            let sum_evens = move || evens.sum::<i64>();
            let mut it = v.iter().map(|x| x + 1);
            let first = it.next();
            format!(
                "{:?} {:?} {} {:?} {:?} {:?} {} {} {:?}",
                firsts, big, counted, last, log, odd, back, sum_evens(), first
            )
        }
        run()
    "#,
    );
    assert_eq!(value, "[1, 4, 9] Some(12) 2 [30] [3] [3, 1] ca 20 Some(2)");
}

#[test]
fn language_discarded_iterators_run_no_stage() {
    // As in Rust, a discarded iterator evaluates its source and closures
    // and runs none of its stages.
    let value = run_string(
        r#"
        pub fn run() -> String {
            let mut made = 0;
            let mut source = || { made += 1; vec![1, 2] };
            source().into_iter().map(|x| x * 2);
            let mut ran = 0;
            [3, 4].iter().map(|x| { ran += 1; x + 1 });
            let _ = [3, 4].iter().map(|x| { ran += 1; x + 1 });
            let total: i64 = [5, 6].iter().map(|x| { ran += 1; x * 1 }).sum();
            format!("{} {} {}", made, ran, total)
        }
        run()
    "#,
    );
    assert_eq!(value, "1 2 11");
}

#[test]
fn language_std_reverse() {
    // `std::cmp::Reverse`: Rust's tuple struct ordering its value backwards,
    // as a sort key, alone, in tuples, as a map key and in a derived `Ord`.
    let value = run_string(
        r#"
        use std::cmp::Reverse;
        use std::collections::BTreeSet;
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        struct Rank { score: Reverse<i64>, name: String }
        fn top(v: &Vec<(String, i64)>) -> Vec<String> {
            let mut sorted = v.clone();
            sorted.sort_by_key(|p| (Reverse(p.1), p.0.clone()));
            sorted.iter().map(|(name, _)| name.clone()).collect()
        }
        pub fn run() -> String {
            let mut v = vec![3, 1, 4, 1, 5];
            v.sort_by_key(|x| Reverse(*x));
            let words = vec![("b".to_string(), 2), ("a".to_string(), 2), ("c".to_string(), 9)];
            let r = Reverse(3);
            let less = Reverse(5) < Reverse(2);
            let ordering = Reverse("a").cmp(&Reverse("b"));
            let set: BTreeSet<Reverse<i64>> = vec![Reverse(1), Reverse(3), Reverse(2)].into_iter().collect();
            let mut ranks = vec![
                Rank { score: Reverse(1), name: "x".to_string() },
                Rank { score: Reverse(7), name: "y".to_string() },
            ];
            ranks.sort();
            let best = vec![2, 8, 5].into_iter().max_by_key(|x| Reverse(*x));
            let typed: std::cmp::Reverse<i64> = std::cmp::Reverse(4);
            format!(
                "{:?} {:?} {} {:?} {} {:?} {:?} {:?} {:?} {} {}",
                v, top(&words), r.0, r, less, ordering, set, ranks[0], best,
                typed.0, Reverse(1) == Reverse(1)
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "[5, 4, 3, 1, 1] [\"c\", \"a\", \"b\"] 3 Reverse(3) true Greater \
         {Reverse(3), Reverse(2), Reverse(1)} Rank { score: Reverse(7), name: \"y\" } Some(2) 4 true"
    );
    // Rust's rules: `Reverse<f64>` is not `Ord`.
    rejected("use std::cmp::Reverse; pub fn run() -> i64 { let r = Reverse(1.5); let o = r.cmp(&r); 0 } run()");
}

#[test]
fn language_mutable_iteration_and_string_pieces() {
    // Tuple-struct parameter patterns, `pub (A, B)` fields and `t.0.1`,
    // mutable iteration, `peekable`, `unzip`, string pieces, `len` of an
    // exact-size chain, and an array literal followed by a chain.
    let value = run_string(
        r#"
        use std::collections::HashMap;
        struct Meters(i64);
        struct Span(pub (i64, i64));
        fn double(Meters(m): Meters) -> i64 { m * 2 }
        fn width(Span((a, b)): Span) -> i64 { b - a }
        pub fn run() -> String {
            let span = Span((2, 9));
            let mut v = vec![1, 2, 3, 4];
            for x in v.iter_mut() { if *x == 2 { continue; } *x *= 10; }
            for x in &mut v { *x += 1; }
            for (i, x) in v.iter_mut().enumerate() { *x += i * 100; }
            v.iter_mut().for_each(|x| *x -= 1);
            let mut groups = vec![vec![1], vec![]];
            for g in groups.iter_mut() { g.push(7); }
            let mut scores: HashMap<&str, i64> = HashMap::new();
            scores.insert("a", 1);
            scores.insert("b", 2);
            for s in scores.values_mut() { *s *= 3; }
            for (k, s) in scores.iter_mut() { if *k == "b" { *s += 1; } }
            let mut tokens = Vec::new();
            let mut chars = "12+3".chars().peekable();
            while let Some(c) = chars.next() {
                if c == '+' { tokens.push("plus".to_string()); continue; }
                let mut number = c.to_string();
                while let Some(d) = chars.next_if(|d| *d != '+') { number.push(d); }
                tokens.push(number);
            }
            let first = chars.peek().is_none();
            let (names, ages): (Vec<&str>, Vec<i64>) = vec![("ann", 3), ("bo", 5)].into_iter().unzip();
            let text = "one two\nthree  four\n";
            let lines: Vec<&str> = text.lines().collect();
            let words: Vec<&str> = text.split_whitespace().collect();
            let fields: Vec<&str> = "a,b,,c".split(',').collect();
            let parts: Vec<String> = "x--y".split("--").map(|s| s.to_uppercase()).collect();
            let mut calls = 0;
            let n = v.iter().map(|x| { calls += 1; x + 1 }).len();
            let m = (0..5).len() + (1..=3).len() + v.iter().enumerate().skip(1).len();
            let flat: Vec<i64> = vec![Some(1), None, Some(3)].into_iter().flatten().collect();
            format!(
                "{} {} {} {:?} {:?} {:?} {:?} {} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {} {} {} {:?}",
                double(Meters(4)), width(Span((1, 5))), span.0.1, v, groups, scores,
                tokens, first, names, ages, lines, words, fields, parts, n, calls, m, flat.len(), flat
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "8 4 9 [10, 102, 230, 340] [[1, 7], [7]] {\"a\": 3, \"b\": 7} [\"12\", \"plus\", \"3\"] true \
         [\"ann\", \"bo\"] [3, 5] [\"one two\", \"three  four\"] [\"one\", \"two\", \"three\", \"four\"] \
         [\"a\", \"b\", \"\", \"c\"] [\"X\", \"Y\"] 4 0 11 2 [1, 3]"
    );
    // Rust's rules: `Chars` and `Filter` have no `len`; `iter_mut` needs a
    // mutable binding; `peek` needs a mutable iterator.
    rejected(r#"pub fn run() -> i64 { "ab".chars().len() } run()"#);
    rejected("pub fn run() -> i64 { let v = vec![1]; v.iter().filter(|x| **x > 0).len() } run()");
    rejected("pub fn run() -> i64 { let v = vec![1]; for x in v.iter_mut() { *x += 1; } 0 } run()");
    rejected("pub fn run() -> i64 { let it = vec![1].into_iter().peekable(); *it.peek().unwrap() } run()");
}

#[test]
fn language_map_get_mut() {
    // `if let Some(v) = map.get_mut(&k)`: `v` is the value's place.
    let value = run_string(
        r#"
        use std::collections::HashMap;
        pub fn run() -> String {
            let mut stock: HashMap<String, Vec<i64>> = HashMap::new();
            stock.insert("a".to_string(), vec![1]);
            let key = "a".to_string();
            if let Some(items) = stock.get_mut(&key) { items.push(2); }
            let missing = if let Some(items) = stock.get_mut("b") { items.len() } else { 0 };
            let mut counts = HashMap::new();
            counts.insert(1, 10);
            if let Some(n) = counts.get_mut(&1) { *n += 5; } else if counts.is_empty() { }
            format!("{:?} {} {:?}", stock, missing, counts)
        }
        run()
    "#,
    );
    assert_eq!(value, "{\"a\": [1, 2]} 0 {1: 15}");
}

#[test]
fn language_numeric_kinds() {
    // Rust's integer and float kinds: suffixes, literal inference, `as`
    // with wrapping, truncation and saturation, `usize` lengths and indexes,
    // and debug Rust's overflow panics.
    let value = run_string(
        r#"
        struct Pixel { r: u8, g: u8, b: u8 }
        fn mean(values: &Vec<u32>) -> f32 {
            let mut total = 0;
            for v in values { total += v; }
            total as f32 / values.len() as f32
        }
        fn at(v: &Vec<i64>, i: usize) -> i64 { v[i] }
        pub fn run() -> String {
            let p = Pixel { r: 200, g: 100, b: 3u8 };
            let wide = p.r as u32 + p.g as u32 + 1u32;
            let v = vec![10, 20, 30];
            let mut i = 0;
            let mut sum = 0;
            while i < v.len() { sum += v[i]; i += 1; }
            let last = v[v.len() - 1];
            let n: usize = v.len();
            let signed = -1i32;
            let float = 2.9f64;
            let big = 300;
            format!(
                "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
                wide, sum, last, n, at(&v, 1), mean(&vec![1, 2, 4]),
                big as u8, -1i64 as u8, signed as u32, 257u16 as i8, float as i64,
                -float as u8, 1e10 as i32, 'a' as u32, 97u8 as char
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "301 60 30 3 20 2.3333333 44 255 4294967295 1 2 0 2147483647 97 a"
    );
    // `MIN`/`MAX`, Rust's lossless `From` impls, the sign and rounding
    // methods, `pow` with its `u32` exponent.
    let value = run_string(
        r#"
        pub fn run() -> String {
            let small: u8 = 7;
            let wide = i64::from(small) + i64::from(u8::MAX) + u32::from(small) as i64;
            let f = f64::from(1.5f32) + f64::from(2i32);
            let n: i32 = -5;
            let x = 2.5f64;
            format!(
                "{} {} {} {} {} {} {} {} {} {}",
                wide, f, i8::MIN, u16::MAX, n.abs(), n.signum(), x.floor(), x.round(),
                2u64.pow(10), (n.abs() as u8).pow(2)
            )
        }
        run()
    "#,
    );
    assert_eq!(value, "269 3.5 -128 65535 5 -1 2 3 1024 25");
    rejected("pub fn run() -> i64 { let x: u8 = 16; x.pow(2) as i64 } run()");
    // Debug Rust panics on overflow of the operation's kind.
    let overflow =
        rejected("pub fn run() -> i64 { let x: u8 = 250; let y = x + 10; y as i64 } run()");
    assert!(
        overflow.contains("attempt to add with overflow"),
        "{overflow}"
    );
    let underflow = rejected(
        "pub fn run() -> i64 { let v: Vec<i64> = Vec::new(); (v.len() - 1) as i64 } run()",
    );
    assert!(
        underflow.contains("attempt to subtract with overflow"),
        "{underflow}"
    );
    rejected("pub fn run() -> i64 { let mut x: i8 = 100; x *= 2; 0 } run()");
    // Kinds do not mix; an index is a `usize`; `-` needs a signed kind.
    rejected("pub fn run() -> i64 { let a: i32 = 1; let b: i64 = 2; (a + b) as i64 } run()");
    rejected("pub fn run() -> i64 { let v = vec![1]; let i: i64 = 0; v[i] } run()");
    rejected("pub fn run() -> i64 { let v = vec![1]; v.len() } run()");
    rejected("pub fn run() -> i64 { let x: u32 = 1; (-x) as i64 } run()");
    rejected("pub fn run() -> i64 { let x = 256u8; 0 } run()");
    rejected("pub fn run() -> i64 { (300 as u8) as i64 } run()");
    rejected(r#"pub fn run() -> i64 { "1" as i64 } run()"#);
}

#[test]
fn language_casts_after_postfix_expressions() {
    // `as` binds tighter than binary operators and looser than unary ones,
    // after method calls, fields, indexes, calls and other casts.
    let value = run_string(
        r#"
        struct P { x: i32 }
        fn half(n: u32) -> u32 { n / 2 }
        pub fn run() -> String {
            let v = vec![4, -9, 16];
            let p = P { x: -3 };
            let r = &v[1];
            let len = v.iter().len() as i64;
            let a = v[1].abs() as u8;
            let b = -p.x as u8 + 1;
            let c = 2 * p.x as i64 * 3;
            let d = half(v[2] as u32) as f64 / 4.0;
            let e = *r as u8 as char as u32;
            let f = v.iter().map(|x| *x as f64 / 2.0).sum::<f64>();
            let g = (v.len() as i64 - 1) as usize;
            let h = v[v.len() as i64 as usize - 1];
            format!("{} {} {} {} {} {} {} {} {}", len, a, b, c, d, e, f, g, h)
        }
        run()
    "#,
    );
    assert_eq!(value, "3 9 4 -18 2 247 5.5 2 16");
    // Rust rejects a comparison right after a cast's type.
    rejected(
        "pub fn run() -> i64 { let v = vec![1]; if v.len() as i64 < 2 { 1 } else { 0 } } run()",
    );
}

#[test]
fn language_slices() {
    // `&[T]` in signatures, range indexes of arrays,
    // `Vec`s, slices and strings (by bytes), and the slice methods.
    let value = run_string(
        r#"
        fn total(values: &[i64]) -> i64 { values.iter().sum() }
        fn widest(rows: &[Vec<i64>]) -> usize { rows.iter().map(|r| r.len()).max().unwrap_or(0) }
        pub fn run() -> String {
            let v = vec![1, 2, 3, 4, 5];
            let fixed = [7, 8, 9];
            let middle = &v[1..3];
            let head: &[i64] = &v[..2];
            let tail = &v[3..];
            let all = &v[..];
            let closed = &v[1..=3];
            let (left, right) = v.split_at(2);
            let text = "héllo";
            let (a, b) = text.split_at(3);
            format!(
                "{:?} {:?} {:?} {} {:?} {} {} {} {:?} {:?} {:?} {:?} {:?} {} {:?} {:?} {} {} {} {}",
                middle, head, tail, all.len(), closed, total(&v[2..]), total(&fixed), total(&v),
                middle.to_vec(), v.first(), v.last(), middle.get(1), middle.get(5),
                closed.contains(&3), left, right, &text[0..1], &text[3..], a, b
            ) + &format!(" {} {:?} {}", widest(&[vec![1], vec![1, 2]]), v[1..4].iter().rev().collect::<Vec<_>>(), middle.iter().map(|x| x * 10).sum::<i64>())
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "[2, 3] [1, 2] [4, 5] 5 [2, 3, 4] 12 24 15 [2, 3] Some(1) Some(5) Some(3) None true [1, 2] [3, 4, 5] h llo hé llo 2 [4, 3, 2] 50"
    );
    // Rust's panics: a range past the end, a reversed range, a string index
    // inside a `char`.
    let past = rejected("pub fn run() -> i64 { let v = vec![1, 2]; v[1..3].len() as i64 } run()");
    assert!(
        past.contains("range end index 3 out of range for slice of length 2"),
        "{past}"
    );
    rejected("pub fn run() -> i64 { let v = vec![1, 2]; let (a, b) = (2, 1); v[a..b].len() as i64 } run()");
    let boundary = rejected(r#"pub fn run() -> i64 { let s = "é"; s[0..1].len() as i64 } run()"#);
    assert!(boundary.contains("not a char boundary"), "{boundary}");
    rejected("pub fn run() -> i64 { let v = vec![1]; let i: i64 = 0; v[i..].len() as i64 } run()");
}

#[test]
fn language_default_binding_modes() {
    // RFC 2005: a non-reference pattern matched through `&T` binds by
    // reference, in `match`, `if let`, `let`, `for` and closure parameters.
    let value = run_string(
        r#"
        use std::collections::HashMap;
        enum Shape { Circle(i64), Square(i64) }
        fn area(shape: &Shape) -> i64 {
            match shape { Shape::Circle(r) => *r * 3, Shape::Square(side) => side * side }
        }
        pub fn run() -> String {
            let pairs = vec![("a".to_string(), 3), ("b".to_string(), 5)];
            let total: i64 = pairs.iter().map(|(_, n)| *n).sum();
            let names: Vec<String> = pairs.iter().map(|(s, _)| s.clone()).collect();
            let firsts: Vec<i64> = pairs.iter().filter(|(_, n)| *n > 3).map(|(_, n)| *n).collect();
            let opt = Some(7);
            let seven = if let Some(x) = &opt { *x } else { 0 };
            let pair = (1, "z".to_string());
            let (one, z) = &pair;
            let mut map = HashMap::new();
            map.insert(1, 10);
            map.insert(2, 20);
            let mut sum = 0;
            for (k, v) in &map { sum += *k * *v; }
            let mut seen = 0;
            for (i, (s, n)) in pairs.iter().enumerate() { seen += i as i64 + *n + s.len() as i64; }
            for &(_, n) in &pairs { seen += n; }
            let shapes = vec![Shape::Circle(2), Shape::Square(3)];
            let areas: i64 = shapes.iter().map(area).sum();
            let nested = Some((4, Some(5)));
            let deep = match &nested { Some((a, Some(b))) => *a + *b, _ => 0 };
            format!("{} {:?} {:?} {} {} {} {} {} {} {}", total, names, firsts, seven, *one, z, sum, seen, areas, deep)
        }
        run()
    "#,
    );
    assert_eq!(value, "8 [\"a\", \"b\"] [5] 7 1 z 50 19 15 9");
    // The bindings are references: Rust rejects them where values are due.
    for source in [
        "pub fn run() -> i64 { let pair = (1, 2); let (a, _) = &pair; let x: i64 = a; x } run()",
        "pub fn run() -> i64 { let v = vec![(1, 2)]; v.iter().map(|(a, _)| -> i64 { a }).sum() } run()",
    ] {
        let error = rejected(source);
        assert!(error.contains("expected 'i64', found '&"), "{error}");
    }
}

#[test]
fn language_char_vec_and_set_methods() {
    // `char`'s classification, case and digit methods; a `Vec` changed in
    // place; set algebra; `BTreeMap::range`; `entry` on a field.
    let value = run_string(
        r#"
        use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
        #[derive(Default)]
        struct Tally { counts: HashMap<String, i64>, seen: Vec<String> }
        impl Tally {
            fn add(&mut self, word: &str) {
                *self.counts.entry(word.to_string()).or_insert(0) += 1;
                self.counts.entry("all".to_string()).or_default();
                self.seen.push(word.to_string());
            }
        }
        pub fn run() -> String {
            let c = 'q';
            let chars = format!(
                "{}{} {} {} {} {} {} {:?} {:?} {:?} {:?} {}",
                c.to_ascii_uppercase(), 'Q'.to_ascii_lowercase(), c.is_alphabetic(), '7'.is_numeric(),
                '_'.is_alphanumeric(), ' '.is_whitespace(), 'x'.is_ascii_digit(), '7'.to_digit(10),
                'f'.to_digit(16), char::from_digit(4, 10), char::from_u32(65), 'é'.len_utf8()
            );
            let mut v = vec![3, 1, 1, 4, 1, 5, 9, 2, 6];
            v.insert(1, 7);
            let removed = v.remove(0);
            v.swap(0, 1);
            v.dedup();
            v.retain(|x| *x != 9);
            v.extend(vec![8, 8]);
            v.extend(&[0]);
            let drained: Vec<i64> = v.drain(1..3).collect();
            v.reverse();
            let mut w = v.clone();
            w.truncate(3);
            let back: Vec<i64> = v.iter().rev().copied().collect();
            if let Some(first) = w.get_mut(0) { *first *= 10; }
            if let Some(x) = w.get_mut(10) { *x = 0; } else { v.clear(); v.push(5); }
            let a: HashSet<i64> = [1, 2, 3, 4].into_iter().collect();
            let b: HashSet<i64> = [3, 4, 5].into_iter().collect();
            let union: BTreeSet<i64> = a.union(&b).copied().collect();
            let common: Vec<i64> = a.intersection(&b).cloned().collect();
            let only: Vec<&i64> = a.difference(&b).collect();
            let either = a.symmetric_difference(&b).count();
            let small: HashSet<i64> = [3].into_iter().collect();
            let mut m = BTreeMap::new();
            for k in 1..8 { m.insert(k, k * k); }
            let mid: Vec<(i64, i64)> = m.range(2..5).map(|(k, v)| (*k, *v)).collect();
            let tail: Vec<&i64> = m.range(6..).map(|(k, _)| k).collect();
            let head = m.range(..=2).count();
            let mut tally = Tally::default();
            tally.add("x");
            tally.add("x");
            tally.add("y");
            let mut counts: Vec<(String, i64)> = tally.counts.into_iter().collect();
            counts.sort();
            format!(
                "{} | {} {:?} {:?} {:?} {:?} | {:?} {:?} {:?} {} {} {} {} | {:?} {:?} {} | {:?} {}",
                chars, removed, v, drained, w, back, union, common, only, either,
                small.is_subset(&a), a.is_superset(&small), a.is_disjoint(&b), mid, tail, head,
                counts, tally.seen.len()
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Qq true true false true false Some(7) Some(15) Some('4') Some('A') 2 | 3 \
         [5] [7, 1] [0, 8, 8] [1, 4, 1, 5, 2, 6, 8, 8, 0] | {1, 2, 3, 4, 5} [3, 4] [1, 2] 3 \
         true true false | [(2, 4), (3, 9), (4, 16)] [6, 7] 2 | [(\"all\", 0), (\"x\", 2), (\"y\", 1)] 3"
    );
    // Rust's panics.
    let insert = rejected("pub fn run() -> i64 { let mut v = vec![1]; v.insert(3, 2); 0 } run()");
    assert!(
        insert.contains("insertion index (is 3) should be <= len (is 1)"),
        "{insert}"
    );
    let remove =
        rejected("pub fn run() -> i64 { let mut v: Vec<i64> = Vec::new(); v.remove(0) } run()");
    assert!(
        remove.contains("removal index (is 0) should be < len (is 0)"),
        "{remove}"
    );
    rejected("pub fn run() -> i64 { let v = vec![1]; v.reverse(); 0 } run()");
}

#[test]
fn language_mutable_reference_parameters() {
    // A free function's `&mut` parameter (`&mut [T]`, `&mut Vec<T>`, `&mut
    // i64`) changes the caller's place: a local, an element, a field, or
    // the caller's own `&mut` parameter; `and_modify` takes such a function.
    let value = run_string(
        r#"
        use std::collections::HashMap;
        struct Bag { items: Vec<i64> }
        fn double_all(values: &mut [i64]) { for v in values.iter_mut() { *v *= 2; } }
        fn fill(v: &mut Vec<i64>, n: i64) -> usize {
            if n > 0 { v.push(n); fill(v, n - 1); }
            v.len()
        }
        fn bump(n: &mut i64) { *n += 1; }
        fn reset(first: &mut [i64]) { first[0] = 0; }
        pub fn run() -> String {
            let mut v = vec![1, 2, 3];
            double_all(&mut v);
            let mut w = Vec::new();
            let filled = fill(&mut w, 3);
            let mut n = 5;
            bump(&mut n);
            let mut grid = vec![vec![1, 1], vec![2, 2]];
            reset(&mut grid[1]);
            let mut bag = Bag { items: vec![4] };
            fill(&mut bag.items, 2);
            let mut counts = HashMap::new();
            counts.insert("a", 1);
            counts.entry("a").and_modify(bump).or_insert(0);
            format!("{:?} {:?} {} {} {:?} {:?} {:?}", v, w, filled, n, grid, bag.items, counts)
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "[2, 4, 6] [3, 2, 1] 3 6 [[1, 1], [0, 2]] [4, 2, 1] {\"a\": 2}"
    );
    // Rust's rules: the place is mutable, borrowed with `&mut`; a `&mut`
    // borrow kept in a local is the place.
    rejected("fn bump(n: &mut i64) { *n += 1; } pub fn run() -> i64 { let n = 1; bump(&mut n); n } run()");
    rejected(
        "fn bump(n: &mut i64) { *n += 1; } pub fn run() -> i64 { let mut n = 1; bump(n); n } run()",
    );
    assert_eq!(
        run("pub fn run() -> i64 { let mut n = 1; let r = &mut n; *r += 1; n } run()"),
        2
    );
}

#[test]
fn language_struct_variants_and_struct_patterns() {
    // Enum struct variants built with `{ .. }` (shorthand too), matched by
    // field name, derived `Debug`/`PartialEq`/`Ord`/`Clone`/`Hash`; struct
    // patterns in `match`, `let`, `for` and closures; `matches!`.
    let value = run_string(
        r#"
        use std::collections::HashSet;
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        enum Shape { Dot, Rect { w: i64, h: i64 }, Named { name: String, sides: i64 }, Pair(i64, i64) }
        #[derive(Debug, Clone, PartialEq)]
        struct Point { x: i64, y: i64 }
        fn area(shape: &Shape) -> i64 {
            match shape {
                Shape::Rect { w: 0, .. } | Shape::Dot => 0,
                Shape::Rect { w, h } => w * h,
                Shape::Named { sides, .. } if *sides > 4 => 99,
                Shape::Named { name, sides: n } => name.len() as i64 + n,
                Shape::Pair { .. } => -1,
            }
        }
        pub fn run() -> String {
            let h = 3;
            let shapes = vec![
                Shape::Rect { h, w: 2 },
                Shape::Named { name: "tri".to_string(), sides: 3 },
                Shape::Rect { w: 0, h: 9 },
                Shape::Named { sides: 6, name: String::new() },
                Shape::Dot,
                Shape::Pair(1, 2),
            ];
            let areas: Vec<i64> = shapes.iter().map(area).collect();
            let mut sorted = shapes.clone();
            sorted.sort();
            let set: HashSet<Shape> = shapes.iter().cloned().collect();
            let p = Point { x: 1, y: 2 };
            let Point { x, .. } = p.clone();
            let q = Point { y: 5, ..p.clone() };
            let moved = match q { Point { x: 1, y } => y, Point { .. } => 0 };
            let points = vec![p.clone(), Point { x: 3, y: 4 }];
            let mut ys = 0;
            for Point { y, .. } in &points { ys += *y; }
            let xs: Vec<i64> = points.iter().map(|Point { x, .. }| *x).collect();
            let nested = Some(Shape::Rect { w: 4, h: 5 });
            let inner = if let Some(Shape::Rect { w, h: 5 }) = nested { w } else { 0 };
            format!(
                "{:?} {:?} {:?} {} {} {} {} {} {:?} {} {} {} {}",
                shapes[0], shapes[3], areas, shapes[0] == Shape::Rect { w: 2, h: 3 },
                shapes[0] < shapes[1], set.len(), x, moved, xs, ys, inner,
                matches!(nested, Some(Shape::Rect { w: 4, .. })),
                matches!(shapes[1], Shape::Named { sides, .. } if sides > 3)
            ) + &format!(" {:?}", sorted.last())
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Rect { w: 2, h: 3 } Named { name: \"\", sides: 6 } [6, 6, 0, 99, 0, -1] true true 6 1 5 \
         [1, 3] 6 4 true false Some(Pair(1, 2))"
    );
    // A `match` after other bindings reads its scrutinee, not a neighbour
    // (the optimizer kept a `let` the parser's variable indexes count).
    let value = run_string(
        r#"pub fn run() -> String {
            let n = Some(4);
            let i = 1;
            let j = match n { Some(w) => w, _ => 0 };
            format!("{} {}", i, j)
        } run()"#,
    );
    assert_eq!(value, "1 4");
    // Rust's rules: struct variants are built and matched by name, every
    // field named once unless `..` ends the pattern.
    for source in [
        "enum E { V { x: i64 } } pub fn run() -> i64 { let e = E::V(1); 0 } run()",
        "enum E { V { x: i64 } } pub fn run() -> i64 { let e = E::V { y: 1 }; 0 } run()",
        "enum E { V { x: i64, y: i64 } } pub fn run() -> i64 { let e = E::V { x: 1 }; 0 } run()",
        "enum E { V { x: i64, y: i64 } } pub fn run() -> i64 { match (E::V { x: 1, y: 2 }) { E::V { x } => x } } run()",
        "enum E { V { x: i64 } } pub fn run() -> i64 { match (E::V { x: 1 }) { E::V(x) => x } } run()",
        "struct P { x: i64 } pub fn run() -> i64 { let P { z } = P { x: 1 }; z } run()",
    ] {
        rejected(source);
    }
}

#[test]
fn language_bit_operators_and_integer_methods() {
    // `& | ^ ! << >>` and their assignments at Rust's precedence, on every
    // kind and on `bool`; `checked_*`/`wrapping_*`/`saturating_*`/
    // `overflowing_*`, bit counts, rotations, Euclidean division;
    // `TryFrom`/`TryInto` among integers.
    let value = run_string(
        r#"
        use std::convert::TryInto;
        pub fn run() -> String {
            let (a, b) = (12, 10);
            let mut c = 12i32;
            c &= 10; c |= 1; c ^= 3; c <<= 2; c >>= 1;
            let byte: u8 = 200;
            let small: i8 = -128;
            let flags = true & !false | false ^ true;
            let ops = format!(
                "{} {} {} {} {} {} {} {} {} {} {} {} {}",
                a & b, a | b, a ^ b, !a, a << 2, -a >> 1, c, !byte, byte << 1, byte >> 3,
                1 + 2 << 3, 6 & 3 == 2, flags
            );
            let checked = format!(
                "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
                5i64.checked_add(1), i64::MAX.checked_add(1), 3u8.checked_sub(4), 7.checked_div(0),
                small.checked_neg(), 2u32.checked_pow(31), 2u32.checked_pow(32)
            );
            let wrapping = format!(
                "{} {} {} {} {} {}",
                250u8.wrapping_add(10), 0u8.wrapping_sub(1), small.wrapping_neg(), 100i8.wrapping_mul(3),
                small.wrapping_div(-1), 3u8.wrapping_pow(5)
            );
            let saturating = format!(
                "{} {} {} {} {}",
                250u8.saturating_add(10), 3u8.saturating_sub(4), 100i8.saturating_mul(-2),
                small.saturating_div(-1), i64::MAX.saturating_add(1)
            );
            let overflowing = format!(
                "{:?} {:?} {:?}",
                250u8.overflowing_add(10), 5i32.overflowing_sub(2), small.overflowing_neg()
            );
            let bits = format!(
                "{} {} {} {} {} {} {} {} {} {} {} {} {}",
                12i64.count_ones(), 12u8.count_zeros(), 1u8.leading_zeros(), 0i32.trailing_zeros(),
                0b1001_0000u8.rotate_left(2), 1u8.rotate_right(1), (-7).rem_euclid(3),
                (-7).div_euclid(3), 3u8.abs_diff(10), (-5i32).abs_diff(5), 64u32.is_power_of_two(),
                1000u32.ilog10(), 9u64.ilog2()
            );
            let fits: Result<u8, _> = u8::try_from(255i64);
            let wide = i8::try_from(200u8);
            let into: u16 = 70000i64.try_into().unwrap_or(7);
            let back: Result<i32, _> = (-1i64).try_into();
            format!(
                "{} | {} | {} | {} | {} | {} | {:?} {} {} {:?}",
                ops, checked, wrapping, saturating, overflowing, bits, fits, wide.is_err(), into, back
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "8 14 6 -13 48 -6 20 55 144 25 24 true true | Some(6) None None None None \
         Some(2147483648) None | 4 255 -128 44 -128 243 | 255 0 -128 127 \
         9223372036854775807 | (4, true) (3, false) (-128, true) | 2 6 7 32 66 128 2 -3 7 \
         10 true 3 3 | Ok(255) true 7 Ok(-1)"
    );
    // Rust's panics and rules.
    for (source, panic) in [
        (
            "pub fn run() -> i64 { let s = 64; 1 << s } run()",
            "attempt to shift left with overflow",
        ),
        (
            "pub fn run() -> i64 { let s = -1; 8 >> s } run()",
            "attempt to shift right with overflow",
        ),
        (
            "pub fn run() -> i64 { let n: u8 = 1; (n << 8u32) as i64 } run()",
            "attempt to shift left with overflow",
        ),
        (
            "pub fn run() -> i64 { let z = 0; 5i64.wrapping_div(z) } run()",
            "attempt to divide by zero",
        ),
        (
            "pub fn run() -> i64 { let z = 0; 5i64.rem_euclid(z) } run()",
            "attempt to calculate the remainder with a divisor of zero",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(panic), "{error}");
    }
    rejected("pub fn run() -> i64 { let a: i32 = 1; let b: i64 = 2; (a & b) as i64 } run()");
    rejected("pub fn run() -> i64 { let x = 1.5; !x; 0 } run()");
    rejected("pub fn run() -> i64 { let x = true; (x << 1) as i64 } run()");
    rejected("pub fn run() -> i64 { let x: u8 = 1; -x as i64 } run()");
    rejected("pub fn run() -> i64 { 5i64.is_power_of_two(); 0 } run()");
}

#[test]
fn language_full_unsigned_range() {
    // `u64` and `usize` hold their full range: the runtime keeps their
    // bits in an `i64`, and their arithmetic, ordering, text and casts are
    // unsigned.
    let value = run_string(
        r#"
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct Hash { bits: u64, size: usize }
        fn smallest(values: &[usize]) -> usize {
            let mut best = usize::MAX;
            for v in values { if *v < best { best = *v; } }
            best
        }
        pub fn run() -> String {
            let max = u64::MAX;
            let wrapped = -1i64 as usize;
            let half = max / 2 + 1;
            let big: u64 = 1 << 63;
            let h = Hash { bits: big + 5, size: usize::MAX - 1 };
            let mut n: usize = 0;
            n = n.wrapping_sub(1);
            format!(
                "{} {} {} {} {} {} {} {:?} {} {} {} {} {:?} {} {} {:?} {} {} {} {:x} {}",
                max, usize::MAX, wrapped, half, big > 5, 5 < big, max.to_string(), h,
                smallest(&[7, 3, 9]), smallest(&[]), big.max(3),
                3u64.cmp(&big) == std::cmp::Ordering::Less, (max as f64, max as u32, max as i64),
                3.0e19 as u64, -5.0 as u64, big.checked_add(big), u64::try_from(-1i64).is_err(),
                n, max % 10, max, max >> 60
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "18446744073709551615 18446744073709551615 18446744073709551615 9223372036854775808 \
         true true 18446744073709551615 Hash { bits: 9223372036854775813, size: 18446744073709551614 } \
         3 18446744073709551615 9223372036854775808 true (1.8446744073709552e19, 4294967295, -1) \
         18446744073709551615 0 None true 18446744073709551615 5 ffffffffffffffff 15"
    );
    for (source, panic) in [
        (
            "pub fn run() -> i64 { let a = u64::MAX; (a + 1) as i64 } run()",
            "attempt to add with overflow",
        ),
        (
            "pub fn run() -> i64 { let a: usize = 0; (a - 1) as i64 } run()",
            "attempt to subtract with overflow",
        ),
        (
            "pub fn run() -> i64 { let a = u64::MAX; (a * 2) as i64 } run()",
            "attempt to multiply with overflow",
        ),
        (
            "pub fn run() -> i64 { let a: u64 = 2; a.pow(64) as i64 } run()",
            "attempt to multiply with overflow",
        ),
        (
            "pub fn run() -> i64 { let mut a: usize = 3; a -= 4; a as i64 } run()",
            "attempt to subtract with overflow",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(panic), "{error}");
    }
    // No lossless `From<u64>` for `i64`.
    rejected("pub fn run() -> i64 { let a: u64 = 1; i64::from(a) } run()");
}

#[test]
fn language_references_in_locals_patterns_and_parameters() {
    // `ref`/`ref mut` bindings; `let r = &mut place;` used in straight-line
    // code; several `&mut` parameters on free functions, `&mut` parameters
    // on methods and associated functions, and `&mut T` closure
    // parameters, each changing the caller's place (copied in and back
    // out, also on `return` and `?`); `Vec::pop` and `append`.
    let value = run_string(
        r#"
        struct Counter { n: i64, log: Vec<i64> }
        impl Counter {
            fn drain_into(&mut self, out: &mut Vec<i64>) -> bool {
                if self.n == 0 { return false; }
                out.push(self.n);
                self.n -= 1;
                true
            }
            fn peek(&self, out: &mut i64) { *out = self.n; }
            fn reset(v: &mut i64) { *v = 0; }
        }
        fn swap2(a: &mut i64, b: &mut i64) { let t = *a; *a = *b; *b = t; }
        fn split(src: &mut Vec<i64>, evens: &mut Vec<i64>, odds: &mut Vec<i64>) -> usize {
            while let Some(v) = src.pop() {
                if v % 2 == 0 { evens.push(v); } else { odds.push(v); }
            }
            evens.len()
        }
        fn take(total: &mut i64, from: &mut Vec<i64>) -> Option<i64> {
            let x = from.pop()?;
            *total += x;
            Some(x)
        }
        fn apply(f: impl Fn(&mut i64), x: &mut i64) { f(x); f(x); }
        pub fn run() -> String {
            let mut v = vec![1];
            let r = &mut v;
            r.push(2);
            let first = &mut v[0];
            *first += 10;
            let mut k = 5;
            let m = &mut k;
            *m *= 3;
            let mut t = (1, String::from("a"));
            let (ref mut a, ref b) = t;
            *a += 1;
            let ref c = t.1;
            let ref mut d = t.0;
            *d *= 10;
            let o = Some(3);
            let shown = match o { Some(ref x) => *x, None => 0 };
            let (mut x, mut y) = (1, 2);
            swap2(&mut x, &mut y);
            let mut src = vec![1, 2, 3, 4, 5];
            let (mut evens, mut odds) = (Vec::new(), Vec::new());
            let count = split(&mut src, &mut evens, &mut odds);
            let mut counter = Counter { n: 3, log: Vec::new() };
            let mut out = Vec::new();
            while counter.drain_into(&mut out) {}
            let mut seen = 9;
            counter.peek(&mut seen);
            Counter::reset(&mut k);
            let mut total = 0;
            let mut from = vec![4, 5];
            take(&mut total, &mut from);
            take(&mut total, &mut from);
            let none = take(&mut total, &mut from);
            let mut add = |w: &mut Vec<i64>, x: i64| w.push(x);
            add(&mut counter.log, 7);
            add(&mut counter.log, 8);
            let mut n = 1;
            apply(|n: &mut i64| *n *= 2, &mut n);
            let mut moved = vec![9];
            out.append(&mut moved);
            let last = out.pop();
            format!(
                "{:?} {} {:?} {} {} {} {} {} | {} {:?} {:?} {:?} | {:?} {} {} {} | {} {:?} {:?} | {:?} {} {:?} {:?}",
                v, k, t, b, c, shown, x, y, count, src, evens, odds, out, counter.n, seen,
                k, total, from, none, counter.log, n, moved, last
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "[11, 2] 0 (20, \"a\") a a 3 2 1 | 2 [] [4, 2] [5, 3, 1] | [3, 2, 1] 0 0 0 | 9 [] None \
         | [7, 8] 4 [] Some(9)"
    );
    // Rust's borrow rules where Rawr checks them.
    for source in [
        "fn f(a: &mut i64, b: &mut i64) { *a += *b; } pub fn run() -> i64 { let mut x = 1; f(&mut x, &mut x); x } run()",
        "pub fn run() -> i64 { let mut v = vec![1]; let r = &mut v; v.push(2); r.push(3); 0 } run()",
        "pub fn run() -> i64 { let v = vec![1]; let r = &mut v; r.push(3); 0 } run()",
        "fn f(a: &mut i64, b: &mut i64) {} pub fn run() -> i64 { let mut x = 1; let mut y = 2; f(&mut x, y); 0 } run()",
    ] {
        rejected(source);
    }
    // A moved borrow is still the place.
    let moved = run("pub fn run() -> i64 { let mut v = vec![1]; let r = &mut v; let s = r; s.push(3); v.len() as i64 } run()");
    assert_eq!(moved, 2);
    // Not provided: `ref mut` in `match`/`if let` arms.
    let arm = rejected("pub fn run() -> i64 { let mut o = Some(1); match o { Some(ref mut x) => *x += 1, None => {} } 0 } run()");
    assert!(arm.contains("`ref mut x` in a `match`"), "{arm}");
}

#[test]
fn language_mutable_sub_slices() {
    // `v[a..b]` of a local changed in place: sorted, reversed, swapped,
    // filled, copied into, walked with `&mut`, and passed as `&mut [T]`.
    let value = run_string(
        r#"
        fn double(values: &mut [i64]) { for v in values.iter_mut() { *v *= 2; } }
        fn bump(a: &mut [i64], b: &mut i64) { a[0] += *b; *b += 1; }
        pub fn run() -> String {
            let mut v = vec![5, 4, 3, 2, 1, 0];
            v[1..4].sort();
            let mut w = v.clone();
            w[..3].reverse();
            w[3..].swap(0, 2);
            let mut z = vec![0; 6];
            z[2..4].fill(7);
            z[4..].copy_from_slice(&v[..2]);
            for x in &mut z[..2] { *x += 1; }
            let mut y = vec![1, 2, 3, 4];
            double(&mut y[1..3]);
            let mut n = 10;
            bump(&mut y[2..], &mut n);
            y[..].iter_mut().for_each(|x| *x += 100);
            let mut t = vec![3, 1, 2];
            t[..].sort_by(|a, b| b.cmp(a));
            format!("{:?} {:?} {:?} {:?} {} {:?}", v, w, z, y, n, t)
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "[5, 2, 3, 4, 1, 0] [3, 2, 5, 0, 1, 4] [1, 1, 7, 7, 5, 2] [101, 104, 116, 104] 11 [3, 2, 1]"
    );
    for (source, panic) in [
        (
            "pub fn run() -> i64 { let mut v = vec![1, 2, 3]; v[1..].copy_from_slice(&[1]); 0 } run()",
            "source slice length (1) does not match destination slice length (2)",
        ),
        (
            "pub fn run() -> i64 { let mut v = vec![1, 2, 3]; for x in &mut v[1..5] { *x += 1; } 0 } run()",
            "range end index 5 out of range for slice of length 3",
        ),
        (
            "pub fn run() -> i64 { let mut v = vec![1, 2, 3]; v[2..1].sort(); 0 } run()",
            "slice index starts at 2 but ends at 1",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(panic), "{error}");
    }
    rejected("pub fn run() -> i64 { let v = vec![2, 1]; v[..].sort(); 0 } run()");
}

#[test]
fn language_self_paths_and_unsigned_derives() {
    // `Self` names the impl's owner in bodies and patterns: `Self { x }`,
    // `Self(x)`, `Self::V { x }`, `Self::V(x)`; a derived `Ord` compares
    // `u64` fields as unsigned.
    let value = run_string(
        r#"
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
        enum E { V { x: u64 }, W(i64) }
        impl E {
            fn make(x: u64) -> Self { Self::V { x } }
            fn other(n: i64) -> Self { Self::W(n) }
            fn get(&self) -> u64 { match self { Self::V { x } => *x, Self::W(_) => 0 } }
        }
        #[derive(Debug)]
        struct S { x: i64 }
        impl S {
            fn make(x: i64) -> Self { Self { x } }
            fn x(&self) -> i64 { let Self { x } = self; *x }
        }
        #[derive(Debug)]
        struct T(i64);
        impl T { fn make(n: i64) -> Self { Self(n) } }
        pub fn run() -> String {
            let mut all = vec![E::make(u64::MAX), E::make(1), E::other(2)];
            all.sort();
            format!("{:?} {} {:?} {} {:?}", all, E::make(4).get(), S::make(3), S::make(5).x(), T::make(6))
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "[V { x: 1 }, V { x: 18446744073709551615 }, W(2)] 4 S { x: 3 } 5 T(6)"
    );
}

#[test]
fn language_bool_then_some_and_then() {
    let value = run(r#"
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Message { Toggled(bool) }
        pub fn run(n: i64) -> i64 {
            let on = n > 2;
            let make = on.then_some(Message::Toggled);
            let a = match make { Some(f) => if f(true) == Message::Toggled(true) { 1 } else { 0 }, None => 0 };
            let b = (n > 9).then_some(5).unwrap_or(10);
            let c = on.then(|| n * 100).unwrap_or(0);
            let d = (!on).then(|| 1).is_none();
            a + b + c + if d { 1000 } else { 0 }
        }
        run(3)
    "#);
    assert_eq!(value, 1 + 10 + 300 + 1000);
    rejected("pub fn run() -> i64 { true.then_some(1, 2).unwrap() } run()");
}

#[test]
fn language_closures_capturing_self() {
    let value = run(r#"
        struct Counter { step: i64, base: i64 }
        impl Counter {
            fn scaled(&self, n: i64) -> i64 { n * self.step }
            fn total(&self) -> i64 {
                let add = |n: i64| self.base + self.scaled(n);
                let nested = |n: i64| { let inner = |m: i64| m + self.step; inner(n) * 10 };
                let moved = move |n: i64| n + self.base;
                let sum: i64 = (1..=3).map(|n| n * self.step).sum();
                add(2) * 1000 + nested(1) * 100 + moved(5) + sum
            }
            fn grow(&mut self) -> i64 {
                self.base += 1;
                let read = || self.base * 2;
                read()
            }
        }
        pub fn run() -> i64 {
            let mut c = Counter { step: 3, base: 4 };
            c.total() * 10 + c.grow()
        }
        run()
    "#);
    assert_eq!(value, ((4 + 6) * 1000 + 40 * 100 + 9 + 18) * 10 + 10);
    rejected(
        r#"
        struct Counter { n: i64 }
        impl Counter {
            fn bump(&mut self) { let mut add = || self.n += 1; add(); }
        }
        pub fn run() -> i64 { let mut c = Counter { n: 0 }; c.bump(); c.n }
        run()
    "#,
    );
}

#[test]
fn language_enum_default_variants() {
    let value = run(r#"
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub enum Language {
            Elm,
            #[default]
            Rust,
            Other(i64),
        }
        #[derive(Default)]
        struct State { language: Language, chosen: Option<Language> }
        fn pick<T: Default>() -> T { T::default() }
        pub fn run() -> i64 {
            let state = State::default();
            let a = if state.language == Language::Rust { 1 } else { 0 };
            let b = if Language::default() == Language::Rust { 10 } else { 0 };
            let c = if pick::<Language>() == Language::Rust { 100 } else { 0 };
            let d = if state.chosen.unwrap_or_default() == Language::Rust { 1000 } else { 0 };
            let e = if format!("{:?}", Language::default()) == "Rust" { 10000 } else { 0 };
            a + b + c + d + e
        }
        run()
    "#);
    assert_eq!(value, 11111);
    for source in [
        "#[derive(Default)] enum E { A, B } pub fn run() -> i64 { 0 } run()",
        "#[derive(Default)] enum E { #[default] A, #[default] B } pub fn run() -> i64 { 0 } run()",
        "#[derive(Default)] enum E { #[default] A(i64), B } pub fn run() -> i64 { 0 } run()",
    ] {
        rejected(source);
    }
}

#[test]
fn language_methods_as_values_and_reference_consts() {
    let value = run(r#"
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Language { Rust, Elm }
        impl Language {
            const ALL: &[Language] = &[Language::Elm, Language::Rust];
            fn weight(&self) -> i64 { match self { Language::Rust => 1, Language::Elm => 2 } }
            fn scaled(self, by: i64) -> i64 { self.weight() * by }
        }
        impl std::fmt::Display for Language {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", match self { Language::Rust => "Rust", Language::Elm => "Elm" })
            }
        }
        fn names(options: &[Language], label: fn(&Language) -> String) -> String {
            let mut out = String::new();
            for option in options { out.push_str(&label(option)); }
            out
        }
        pub fn run() -> i64 {
            let weights: i64 = Language::ALL.iter().map(Language::weight).sum();
            let scale = Language::scaled;
            let text = names(Language::ALL, Language::to_string);
            let a = if text == "ElmRust" { 1000 } else { 0 };
            a + weights * 10 + scale(Language::Elm, 3)
        }
        run()
    "#);
    assert_eq!(value, 1000 + 30 + 6);
}

#[test]
fn language_array_types_in_item_headers() {
    // `[T; N]` in consts, impl consts, fields, parameters and returns, as
    // iced's combo_box example writes `Language::ALL`.
    let value = run_string(
        r#"
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        enum Language { #[default] Danish, English, French }
        impl Language {
            const ALL: [Language; 3] = [Language::Danish, Language::English, Language::French];
        }
        const GRID: [[i64; 2]; 2] = [[1, 2], [3, 4]];
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        struct Pair { xs: [i64; 2] }
        fn swap(a: [i64; 2]) -> [i64; 2] { [a[1], a[0]] }
        fn count(languages: &[Language]) -> usize { languages.len() }
        pub fn run() -> String {
            let mut seen = Vec::new();
            for language in Language::ALL { seen.push(language); }
            let english = Language::ALL.iter().position(|l| *l == Language::English);
            let pair = Pair { xs: swap([1, 2]) };
            let total: i64 = GRID.iter().map(|row| row.iter().sum::<i64>()).sum();
            format!(
                "{} {:?} {:?} {} {} {} {:?} {:?} {} {}",
                Language::ALL.len(), Language::ALL.to_vec(), english, count(&Language::ALL),
                count(&Language::ALL[1..]), seen.len(), pair, Pair::default(), total, GRID[1][0]
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "3 [Danish, English, French] Some(1) 3 2 3 Pair { xs: [2, 1] } Pair { xs: [0, 0] } 10 3"
    );
    // A length that differs is rustc's E0308, in a const, a field and a call.
    for source in [
        "const ALL: [i64; 3] = [1, 2]; pub fn run() -> i64 { ALL[0] } run()",
        "struct S { xs: [i64; 2] } pub fn run() -> i64 { let s = S { xs: [1, 2, 3] }; s.xs[0] } run()",
        "fn f(a: [i64; 2]) -> i64 { a[0] } pub fn run() -> i64 { f([1]) } run()",
    ] {
        let error = rejected(source);
        assert!(error.contains("expected an array with a size of"), "{error}");
    }
    let parsed = rejected("const ALL: [i64; n] = [1]; pub fn run() -> i64 { 0 } run()");
    assert!(parsed.contains("array length `n`"), "{parsed}");
}

#[test]
fn language_slice_and_rest_patterns() {
    let value = run_string(
        r#"
        #[derive(Debug)]
        enum Cmd { Quit, Say(String) }
        fn parse(words: &[&str]) -> Option<Cmd> {
            match words {
                ["quit"] => Some(Cmd::Quit),
                ["say", rest @ ..] if !rest.is_empty() => Some(Cmd::Say(rest.join(" "))),
                _ => None,
            }
        }
        fn sum(v: &[i64]) -> i64 { match v { [] => 0, [x, rest @ ..] => x + sum(rest) } }
        fn ends(v: &[i64]) -> i64 { match v { [] | [_] => 0, [a, .., b] => a * b } }
        fn bits(v: [bool; 2]) -> i64 { match v { [true, ..] => 1, [false, true] => 2, [.., false] => 3 } }
        fn tens(n: i64) -> i64 { match Some(n) { Some(k @ 10..=19) => k.clone(), _ => 0 } }
        struct W(i64, i64, i64);
        enum E { V(i64, String, i64) }
        pub fn run() -> String {
            let v = vec![1, 2, 3, 4];
            let [a, mid @ .., d] = [5, 6, 7, 8];
            let ws: Vec<&str> = "say hi there".split_whitespace().collect();
            let (first, .., last) = (1, 2, 3, 4);
            let W(x, ..) = W(5, 6, 7);
            let k = match E::V(8, String::from("s"), 9) { E::V(n, ..) => n };
            let m = match (1, 2, 3) { (1, ..) => 100, (.., 3) => 200, _ => 0 };
            let r = if let [p, q, ..] = v.as_slice() { p + q } else { 0 };
            format!(
                "{:?} {:?} {} {} {} {:?} {} {} {} {} {} {} {} {} {}",
                parse(&ws), parse(&["x"]), sum(&v), ends(&v), a + d, mid, sum(&mid),
                bits([false, false]), first + last, x, k, m, r, tens(12),
                ["a", "b"].concat()
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        r#"Some(Say("hi there")) None 10 4 13 [6, 7] 13 3 5 5 8 100 3 12 ab"#
    );
    // Rust's errors: coverage by length, E0527/E0528, refutable `let`.
    for (source, expected) in [
        (
            "fn f(v: &[i64]) -> i64 { match v { [] => 0, [x] => *x } }",
            "missing [_, _, ..]",
        ),
        (
            "fn f(v: [bool; 2]) -> i64 { match v { [true, _] => 1, [_, true] => 2 } }",
            "missing [false, false]",
        ),
        (
            "fn f(v: [i64; 3]) -> i64 { match v { [a, b] => a } }",
            "pattern requires 2 elements but array has 3",
        ),
        (
            "fn f(v: [i64; 1]) -> i64 { match v { [a, b, ..] => a } }",
            "pattern requires at least 2 elements but array has 1",
        ),
        (
            "fn f(v: &[i64]) -> i64 { match v { [] | [_, ..] => 0, [..] => 1 } }",
            "unreachable",
        ),
        (
            "fn f(v: &[i64]) -> i64 { let [a, b] = v; *a }",
            "refutable pattern",
        ),
        (
            "fn f(v: &[i64]) -> i64 { match v { [a, .., b, ..] => 0, _ => 1 } }",
            "only be used once per slice",
        ),
        (
            "fn f() -> i64 { let (a, rest @ ..) = (1, 2, 3); a }",
            "not allowed in a tuple",
        ),
    ] {
        let error = rejected(&format!("{source} pub fn run() -> i64 {{ 0 }} run()"));
        assert!(error.contains(expected), "{source}: {error}");
    }
}

#[test]
fn language_generic_from_impls_and_type_path_turbofish() {
    let value = run_string(
        r#"
        #[derive(Debug, Clone, PartialEq)]
        struct Wrapper<T> { v: T }
        impl<T> From<T> for Wrapper<T> { fn from(v: T) -> Self { Wrapper { v } } }
        #[derive(Debug)]
        enum Error<E> { Inner(E), Other }
        impl<E> From<E> for Error<E> { fn from(e: E) -> Self { Error::Inner(e) } }
        struct Stack<T> { items: Vec<T> }
        impl<T> Stack<T> { fn new() -> Self { Stack { items: Vec::new() } } }
        fn step(n: i64) -> Result<i64, String> { if n < 0 { Err(String::from("neg")) } else { Ok(n) } }
        fn twice(n: i64) -> Result<i64, Error<String>> { let a = step(n)?; Ok(a * 2) }
        fn wrap_into<T>(x: T) -> Wrapper<T> { x.into() }
        pub fn run() -> String {
            let w: Wrapper<i64> = Wrapper::from(5);
            let x: Wrapper<i64> = 6.into();
            let s: Wrapper<String> = String::from("s").into();
            let y = Wrapper::from(true);
            let mut v = Vec::<u8>::new();
            v.push(7);
            let mut stack = Stack::<String>::new();
            stack.items.push(String::from("t"));
            let m = std::collections::BTreeMap::<String, Vec<i64>>::new();
            format!(
                "{} {} {} {:?} {:?} {:?} {:?} {:?} {} {}",
                w.v + x.v, s.v, y.v, twice(2), twice(-1), wrap_into(1.5),
                Wrapper::<i64>::from(3), v, stack.items[0], m.len()
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        r#"11 s true Ok(4) Err(Inner("neg")) Wrapper { v: 1.5 } Wrapper { v: 3 } [7] t 0"#
    );
    // The written arguments bind, as Rust's turbofish does.
    for source in [
        "struct W<T> { v: T } impl<T> From<T> for W<T> { fn from(v: T) -> Self { W { v } } } \
         fn f() -> i64 { let w = W::<i64>::from(true); 0 }",
        "fn f() -> i64 { let mut v = Vec::<i64>::new(); v.push(String::new()); 0 }",
        "struct W<T> { v: T } impl<T> From<T> for W<T> { fn from(v: T) -> Self { W { v } } } \
         fn f() -> i64 { let w: W<String> = 1.into(); 0 }",
    ] {
        let error = rejected(&format!("{source} pub fn run() -> i64 {{ 0 }} run()"));
        assert!(
            error.contains("mismatch") || error.contains("does not provide"),
            "{source}: {error}"
        );
    }
}

#[test]
fn language_impl_trait_parameters() {
    let value = run_string(
        r#"
        use std::fmt::Display;
        trait Animal { fn name(&self) -> String; }
        struct Dog;
        struct Cat;
        impl Animal for Dog { fn name(&self) -> String { String::from("dog") } }
        impl Animal for Cat { fn name(&self) -> String { String::from("cat") } }
        fn show(a: &impl Animal) -> String { a.name() }
        fn both(a: impl Animal, b: impl Animal) -> String { format!("{}+{}", a.name(), b.name()) }
        fn label(s: impl Into<String>) -> String { let s: String = s.into(); s }
        fn shout(x: impl Display + Clone) -> String { format!("{}!", x) }
        fn total(v: &[impl Animal]) -> usize { v.len() }
        pub fn run() -> String {
            format!("{} {} {} {} {}", show(&Cat), both(Dog, Cat), label("x"), shout(3), total(&[Dog, Dog]))
        }
        run()
    "#,
    );
    assert_eq!(value, "cat dog+cat x 3! 2");
    let unbounded = rejected(
        "trait Animal { fn name(&self) -> String; } struct Rock; \
         fn show(a: &impl Animal) -> String { a.name() } pub fn run() -> i64 { show(&Rock); 0 } run()",
    );
    assert!(unbounded.contains("Animal"), "{unbounded}");
    let returned = rejected(
        "fn f() -> impl std::fmt::Display { 3 } pub fn run() -> i64 { 0 } run()",
    );
    assert!(returned.contains("return type"), "{returned}");
}

#[test]
fn language_const_array_lengths() {
    let value = run_string(
        r#"
        const N: usize = 3;
        const ROWS: usize = 2;
        #[derive(Debug, Clone, Default)]
        struct Grid { cells: [[i64; N]; ROWS] }
        fn sum(a: [i64; N]) -> i64 { let mut s = 0; for x in a { s += x; } s }
        fn fresh() -> [i64; N] { [7; N] }
        pub fn run() -> String {
            let g = Grid { cells: [[1, 2, 3], [4, 5, 6]] };
            let z = [0i64; N];
            let local: [bool; ROWS] = [true; ROWS];
            format!(
                "{} {} {} {} {:?} {:?}",
                g.cells.len(), sum(g.cells[1]), z.len(), local.len(), fresh(), Grid::default()
            )
        }
        run()
    "#,
    );
    assert_eq!(value, "2 15 3 2 [7, 7, 7] Grid { cells: [[0, 0, 0], [0, 0, 0]] }");
    let mismatch = rejected(
        "const N: usize = 3; fn f() -> [i64; N] { [1, 2] } pub fn run() -> i64 { 0 } run()",
    );
    assert!(mismatch.contains("expected an array with a size of 3"), "{mismatch}");
    let unknown = rejected("struct G { c: [i64; M] } pub fn run() -> i64 { 0 } run()");
    assert!(unknown.contains("array length `M`"), "{unknown}");
}

#[test]
fn language_operator_traits() {
    let value = run_string(
        r#"
        use std::ops::{Add, AddAssign, Mul, Neg, Not, Sub};
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        struct V2 { x: f32, y: f32 }
        impl Add for V2 { type Output = V2; fn add(self, o: V2) -> V2 { V2 { x: self.x + o.x, y: self.y + o.y } } }
        impl Sub for V2 { type Output = Self; fn sub(self, o: Self) -> Self { V2 { x: self.x - o.x, y: self.y - o.y } } }
        impl Mul<f32> for V2 { type Output = V2; fn mul(self, k: f32) -> V2 { V2 { x: self.x * k, y: self.y * k } } }
        impl Mul for V2 { type Output = f32; fn mul(self, o: V2) -> f32 { self.x * o.x + self.y * o.y } }
        impl Neg for V2 { type Output = V2; fn neg(self) -> V2 { V2 { x: -self.x, y: -self.y } } }
        impl AddAssign for V2 { fn add_assign(&mut self, o: V2) { self.x += o.x; self.y += o.y; } }
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct Flag(bool);
        impl Not for Flag { type Output = Flag; fn not(self) -> Flag { Flag(!self.0) } }
        struct Body { pos: V2 }
        fn sum<T: Add<Output = T> + Copy>(a: T, b: T) -> T { a + b }
        fn double<T: std::ops::Mul<Output = T> + Copy>(a: T, k: T) -> T { a * k }
        pub fn run() -> String {
            let a = V2 { x: 1.0, y: 2.0 };
            let b = a + a * 2.0 - V2 { x: 1.0, y: 1.0 };
            let mut c = -b;
            c += a;
            let mut body = Body { pos: a };
            body.pos += V2 { x: 10.0, y: 0.0 };
            let mut list = vec![a, a];
            list[0] += a;
            format!(
                "{:?} {:?} {} {:?} {:?} {:?} {:?} {:?} {} {} {}",
                b, c, a * a, !Flag(true), body.pos, list[0], a, sum(a, a), sum(3, 4),
                sum(1.5, 2.0), double(3u8, 2)
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "V2 { x: 2.0, y: 5.0 } V2 { x: -1.0, y: -3.0 } 5 Flag(false) V2 { x: 11.0, y: 2.0 } \
         V2 { x: 2.0, y: 4.0 } V2 { x: 1.0, y: 2.0 } V2 { x: 2.0, y: 4.0 } 7 3.5 6"
    );
    // Rust's E0369, E0600 and E0368 without the impl; an unmet bound.
    for (source, expected) in [
        ("#[derive(Clone, Copy)] struct V(i64); fn f() -> i64 { let v = V(1) + V(2); 0 }", "E0369"),
        ("#[derive(Clone, Copy)] struct V(i64); fn f() -> i64 { let v = -V(1); 0 }", "E0600"),
        (
            "use std::ops::Add; #[derive(Clone, Copy)] struct V(i64); \
             impl Add for V { type Output = V; fn add(self, o: V) -> V { V(self.0 + o.0) } } \
             fn f() -> i64 { let mut v = V(1); v += V(2); 0 }",
            "E0368",
        ),
        (
            "use std::ops::Add; #[derive(Clone, Copy)] struct V(i64); \
             impl Add for V { type Output = V; fn add(self, o: V) -> V { V(self.0 + o.0) } } \
             fn f() -> i64 { let v = V(1) + 2; 0 }",
            "E0369",
        ),
        (
            "use std::ops::Add; fn sum<T: Add<Output = T>>(a: T, b: T) -> T { a + b } \
             fn f() -> bool { sum(true, false) }",
            "Add",
        ),
    ] {
        let error = rejected(&format!("{source} pub fn run() -> i64 {{ 0 }} run()"));
        assert!(error.contains(expected), "{source}: {error}");
    }
}

#[test]
fn language_field_element_assignment() {
    let value = run_string(
        r#"
        #[derive(Debug, Clone)]
        struct G { cells: Vec<i64>, grid: Vec<Vec<i64>>, fixed: [bool; 3] }
        impl G {
            fn set(&mut self, r: usize, c: usize, v: i64) { self.grid[r][c] = v; self.grid[r][c] += 1; }
        }
        pub fn run() -> String {
            let mut g = G { cells: vec![0, 0], grid: vec![vec![0, 0], vec![0, 0]], fixed: [false; 3] };
            g.cells[0] = 3;
            g.cells[1] += 4;
            g.grid[0][1] = 2;
            g.grid[1][0] -= 5;
            g.fixed[2] = true;
            let i = 1;
            g.cells[i] *= 10;
            g.set(1, 1, 6);
            format!("{:?}", g)
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "G { cells: [3, 40], grid: [[0, 2], [-5, 7]], fixed: [false, false, true] }"
    );
    let immutable = rejected(
        "struct G { c: Vec<i64> } pub fn run() -> i64 { let g = G { c: vec![1] }; g.c[0] = 2; 0 } run()",
    );
    assert!(immutable.contains("immutable"), "{immutable}");
}

#[test]
fn language_str_parse_and_methods() {
    let value = run_string(
        r#"
        use std::num::ParseIntError;
        fn read(s: &str) -> Result<i64, ParseIntError> { let n: i64 = s.trim().parse()?; Ok(n * 2) }
        fn sum(a: &str, b: &str) -> Result<u8, std::num::ParseIntError> { Ok(a.parse::<u8>()? + b.parse::<u8>()?) }
        pub fn run() -> String {
            let a = "42".parse::<i32>();
            let b: Result<f32, _> = "2.5".parse();
            let c = "abc".parse::<i64>();
            let d: u32 = "7".parse().unwrap_or(0);
            let e = "300".parse::<u8>();
            let f = "true".parse::<bool>().unwrap();
            let g = "xy".parse::<char>();
            let h = match "x1".parse::<i64>() { Ok(n) => n.to_string(), Err(e) => e.to_string() };
            let i = "-5".parse::<usize>().is_err();
            let j = "".parse::<f64>().map_err(|e| format!("{e}"));
            let s = "  a=b=c  ".trim();
            format!(
                "{:?} {:?} {:?} {} {:?} {} {:?} {} {} {:?} {:?} {:?} {:?} | {} {:?} {:?} {:?} {:?} {} {} {}",
                a, b, c, d, e, f, g, h, i, j, read(" 21 "), read("q"), sum("100", "20"),
                s.replace("=", "+"), s.find('='), s.split_once('='), s.strip_prefix("a"),
                s.rsplit_once("="), "ab".repeat(2), " x ".trim_start().len(),
                "Hi".eq_ignore_ascii_case("hI")
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Ok(42) Ok(2.5) Err(ParseIntError { kind: InvalidDigit }) 7 \
         Err(ParseIntError { kind: PosOverflow }) true Err(ParseCharError { kind: TooManyChars }) \
         invalid digit found in string true Err(\"cannot parse float from empty string\") Ok(42) \
         Err(ParseIntError { kind: InvalidDigit }) Ok(120) | a+b+c Some(1) Some((\"a\", \"b=c\")) \
         Some(\"=b=c\") Some((\"a=b\", \"c\")) abab 2 true"
    );
    for (source, expected) in [
        ("fn f() -> i64 { let x = \"1\".parse().unwrap(); 0 }", "type annotations needed"),
        ("struct S; fn f() -> i64 { let x = \"1\".parse::<S>(); 0 }", "FromStr"),
        ("fn f() -> Result<i64, std::num::ParseFloatError> { let x: i64 = \"1\".parse()?; Ok(x) }", "mismatch"),
    ] {
        let error = rejected(&format!("{source} pub fn run() -> i64 {{ 0 }} run()"));
        assert!(error.contains(expected), "{source}: {error}");
    }
}

#[test]
fn language_rust_names_the_legacy_lexer_reserves() {
    let value = run_string(
        r#"
        #[derive(Debug, Clone, PartialEq)]
        struct Config { debug: bool, call: i64, go: i64, this: String }
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Mode { print, eval, Goto }
        #[derive(Debug)]
        struct curry { import: i64 }
        impl Config {
            fn debug(&self) -> bool { self.debug }
            fn call(&self, eval: i64) -> i64 { self.call + eval }
        }
        fn print(x: i64) -> i64 { x * 2 }
        fn type_of(x: i64) -> i64 { x }
        pub fn run() -> String {
            let this = 4; let goto = 5; let until = 1; let catch = 2; let switch = 3;
            let c = Config { debug: true, call: 2, go: 3, this: String::from("t") };
            let Config { go, .. } = c.clone();
            let m = Mode::print;
            let k = curry { import: 9 };
            let s = match m { Mode::print => 1, Mode::eval => 2, Mode::Goto => 3 };
            format!(
                "{} {} {} {} {:?} {:?} {:?} {} {}",
                print(this) + goto + until - catch + switch, c.call(go), c.debug(), type_of(s), c,
                m, k, k.import, this - 1
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "15 5 true 1 Config { debug: true, call: 2, go: 3, this: \"t\" } print curry { import: 9 } 9 3"
    );
    let error = rejected("struct S { debug: bool } pub fn run() -> i64 { let s = S { debug: 1 }; let x = s.debug + 1; 0 } run()");
    assert!(!error.contains("__rawr_"), "{error}");
}

#[test]
fn language_methods_on_element_and_nested_places() {
    let value = run_string(
        r#"
        #[derive(Debug, Clone, Default)]
        struct Cell { v: i64 }
        impl Cell { fn set(&mut self, v: i64) { self.v = v; } }
        #[derive(Debug, Clone, Default)]
        struct Row { cells: Vec<Cell> }
        #[derive(Debug, Clone, Default)]
        struct Pos { x: i64 }
        #[derive(Debug, Clone, Default)]
        struct G { items: Vec<Vec<i64>>, rows: Vec<Row>, pos: Pos }
        impl G {
            fn poke(&mut self, i: usize, j: usize) {
                self.rows[i].cells[j].set(7);
                self.items[0].push(9);
                self.pos.x += 1;
            }
        }
        pub fn run() -> String {
            let mut g = G { items: vec![vec![1]], rows: vec![Row { cells: vec![Cell::default(); 2] }], pos: Pos::default() };
            g.items[0].push(2);
            g.rows[0].cells[1].set(5);
            g.poke(0, 0);
            g.pos.x = 10;
            let mut v = vec![Row::default()];
            v[0].cells.push(Cell { v: 3 });
            v[0].cells[0].set(4);
            v[0].cells[0].v += 1;
            let mut cells = vec![Cell::default()];
            cells[0].set(8);
            format!("{:?} {:?} {:?}", g, v, cells)
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "G { items: [[1, 2, 9]], rows: [Row { cells: [Cell { v: 7 }, Cell { v: 5 }] }], pos: Pos { x: 10 } } \
         [Row { cells: [Cell { v: 5 }] }] [Cell { v: 8 }]"
    );
    let immutable = rejected(
        "struct C { v: i64 } impl C { fn set(&mut self) { self.v = 1; } } \
         pub fn run() -> i64 { let v = vec![C { v: 0 }]; v[0].set(); 0 } run()",
    );
    assert!(immutable.contains("immutable") || immutable.contains("mutable"), "{immutable}");
}

#[test]
fn language_bit_reference_and_index_operator_traits() {
    let value = run_string(
        r#"
        use std::ops::{Add, BitAnd, BitOr, BitOrAssign, Index, Mul, Not, Shl};
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct Flags(u8);
        impl BitOr for Flags { type Output = Flags; fn bitor(self, o: Flags) -> Flags { Flags(self.0 | o.0) } }
        impl BitAnd for Flags { type Output = Flags; fn bitand(self, o: Flags) -> Flags { Flags(self.0 & o.0) } }
        impl BitOrAssign for Flags { fn bitor_assign(&mut self, o: Flags) { self.0 |= o.0; } }
        impl Shl<u8> for Flags { type Output = Flags; fn shl(self, n: u8) -> Flags { Flags(self.0 << n) } }
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct V(f64);
        impl Add for V { type Output = V; fn add(self, o: V) -> V { V(self.0 + o.0) } }
        impl<'a> Add<&'a V> for &'a V { type Output = V; fn add(self, o: &V) -> V { V(self.0 + o.0 + 100.0) } }
        impl Mul<V> for f64 { type Output = V; fn mul(self, v: V) -> V { V(self * v.0) } }
        struct Grid { cells: Vec<i64>, width: usize }
        impl Index<(usize, usize)> for Grid {
            type Output = i64;
            fn index(&self, (x, y): (usize, usize)) -> &i64 { &self.cells[y * self.width + x] }
        }
        impl Index<usize> for Grid { type Output = i64; fn index(&self, i: usize) -> &i64 { &self.cells[i] } }
        fn mask<T: BitAnd<Output = T>>(a: T, b: T) -> T { a & b }
        pub fn run() -> String {
            let mut f = Flags(1) | Flags(4);
            f |= Flags(8);
            let a = V(1.0);
            let b = V(2.0);
            let g = Grid { cells: vec![1, 2, 3, 4], width: 2 };
            format!(
                "{:?} {:?} {:?} {:?} {:?} {:?} {} {} {} {}",
                f, f & Flags(5), Flags(1) << 3, a + b, &a + &b, 2.0 * b, g[(1, 1)], g[2], mask(6, 3), mask(true, false)
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Flags(13) Flags(5) Flags(8) V(3.0) V(103.0) V(4.0) 4 3 2 false"
    );
    for (source, expected) in [
        ("#[derive(Clone, Copy)] struct F(u8); fn f() -> i64 { let x = F(1) | F(2); 0 }", "E0369"),
        (
            "use std::ops::Index; struct G { c: Vec<i64> } \
             impl Index<usize> for G { type Output = i64; fn index(&self, i: usize) -> &i64 { &self.c[i] } } \
             fn f() -> i64 { let g = G { c: vec![1] }; g[true] }",
            "Index<bool>",
        ),
    ] {
        let error = rejected(&format!("{source} pub fn run() -> i64 {{ 0 }} run()"));
        assert!(error.contains(expected), "{source}: {error}");
    }
}

#[test]
fn language_ref_bindings_in_slice_patterns() {
    let value = run_string(
        r#"
        fn first(v: &[String]) -> usize { match v { [ref a, ..] => a.len(), [] => 0 } }
        pub fn run() -> String {
            let mut arr = [1, 2, 3];
            let [ref mut head, .., ref tail] = arr;
            *head = 9;
            let t = *tail;
            format!("{:?} {} {}", arr, t, first(&[String::from("abc")]))
        }
        run()
    "#,
    );
    assert_eq!(value, "[9, 2, 3] 3 3");
}

#[test]
fn language_slice_patterns_on_a_vec_value_are_e0529() {
    let error = rejected(
        "fn f(v: Vec<i64>) -> i64 { match v { [a, ..] => a, _ => 0 } } pub fn run() -> i64 { f(vec![1]) } run()",
    );
    assert!(error.contains("E0529"), "{error}");
    assert_eq!(
        run("fn f(v: Vec<i64>) -> i64 { match v.as_slice() { [a, ..] => *a, _ => 0 } } pub fn run() -> i64 { f(vec![4]) } run()"),
        4
    );
}

#[test]
fn language_associated_const_array_lengths() {
    let value = run_string(
        r#"
        #[derive(Debug, Clone)]
        struct Board { cells: [u8; Board::SIZE] }
        impl Board {
            const SIZE: usize = 3;
            fn new() -> Self { Board { cells: [0; Self::SIZE] } }
            fn row(&self) -> [u8; Self::SIZE] { self.cells }
            fn blank() -> [bool; Board::SIZE] { [false; Board::SIZE] }
        }
        pub fn run() -> String {
            let mut b = Board::new();
            b.cells[1] = 7;
            let local: [u8; Board::SIZE] = b.row();
            format!("{:?} {:?} {:?}", b, local, Board::blank())
        }
        run()
    "#,
    );
    assert_eq!(value, "Board { cells: [0, 7, 0] } [0, 7, 0] [false, false, false]");
}

#[test]
fn language_parse_into_string_is_infallible() {
    let value = run_string(
        r#"
        use std::convert::Infallible;
        fn copy(s: &str) -> Result<String, Infallible> { s.parse() }
        pub fn run() -> String {
            let a = "abc".parse::<String>();
            let b: String = "x y".parse().unwrap();
            format!("{:?} {} {:?}", a, b, copy("z"))
        }
        run()
    "#,
    );
    assert_eq!(value, r#"Ok("abc") x y Ok("z")"#);
}

#[test]
fn language_mutation_through_traits() {
    let value = run_string(
        r#"
        trait Add { fn add_to(&self, total: &mut i64); fn twice(&self, total: &mut i64) { self.add_to(total); self.add_to(total); } }
        struct One;
        struct Two;
        impl Add for One { fn add_to(&self, total: &mut i64) { *total += 1; } }
        impl Add for Two { fn add_to(&self, total: &mut i64) { *total += 2; } }
        fn via<T: Add>(b: &T, t: &mut i64) { b.add_to(t); }
        fn dynamic(b: &dyn Add, t: &mut i64) { b.add_to(t); }

        trait Bump { fn bump(&mut self); fn get(&self) -> i64; }
        struct C { n: i64 }
        impl Bump for C { fn bump(&mut self) { self.n += 1; } fn get(&self) -> i64 { self.n } }
        fn go(b: &mut dyn Bump) { b.bump(); }
        fn twice<T: Bump>(b: &mut T) { b.bump(); b.bump(); }
        fn imp(b: &mut impl Bump) { b.bump(); }
        struct Holder { items: Vec<Box<dyn Bump>>, one: Box<dyn Bump>, c: C }
        impl Holder {
            fn update(&mut self, i: usize) {
                self.items[i].bump();
                self.one.bump();
                self.c.bump();
                for b in self.items.iter_mut() { b.bump(); }
            }
        }
        pub fn run() -> String {
            let mut t = 0;
            One.add_to(&mut t);
            Two.twice(&mut t);
            via(&Two, &mut t);
            dynamic(&One, &mut t);
            let adders: Vec<Box<dyn Add>> = vec![Box::new(One), Box::new(Two)];
            for b in &adders { b.add_to(&mut t); }

            let mut c = C { n: 0 };
            go(&mut c);
            twice(&mut c);
            imp(&mut c);
            let mut v: Vec<Box<dyn Bump>> = vec![Box::new(C { n: 10 }), Box::new(C { n: 20 })];
            for b in v.iter_mut() { b.bump(); }
            for b in &mut v { b.bump(); }
            v[0].bump();
            let mut h = Holder { items: vec![Box::new(C { n: 0 }), Box::new(C { n: 5 })], one: Box::new(C { n: 7 }), c: C { n: 1 } };
            h.update(1);
            h.update(0);
            format!(
                "{} {} {} {} {} {} {} {}",
                t, c.get(), v[0].get(), v[1].get(), h.items[0].get(), h.items[1].get(), h.one.get(), h.c.n
            )
        }
        run()
    "#,
    );
    assert_eq!(value, "11 4 13 22 3 8 9 3");
}

#[test]
fn language_index_mut_and_mutable_trait_object_borrows() {
    let value = run_string(
        r#"
        use std::ops::{Index, IndexMut};
        #[derive(Debug, Clone)]
        struct Grid { cells: Vec<Vec<i64>>, width: usize }
        impl Index<usize> for Grid { type Output = Vec<i64>; fn index(&self, i: usize) -> &Vec<i64> { &self.cells[i] } }
        impl IndexMut<usize> for Grid { fn index_mut(&mut self, i: usize) -> &mut Vec<i64> { &mut self.cells[i] } }
        struct Board { grid: Grid }
        impl Board {
            fn mark(&mut self) { self.grid.cells[0][0] = 5; }
        }
        impl Grid {
            fn reset(&mut self) { self[1] = vec![0, 0]; self[0][1] += 10; }
        }
        trait Bump { fn bump(&mut self); }
        struct C { n: i64 }
        impl Bump for C { fn bump(&mut self) { self.n += 1; } }
        pub fn run() -> String {
            let mut g = Grid { cells: vec![vec![1, 2], vec![3, 4]], width: 2 };
            g[0] = vec![7, 8];
            g[1].push(9);
            g[1][0] += 100;
            let read = g[1][2];
            g.reset();
            let mut b = Board { grid: g.clone() };
            b.mark();
            let mut c = C { n: 0 };
            let r: &mut dyn Bump = &mut c;
            r.bump();
            r.bump();
            let mut d = C { n: 5 };
            let e = &mut d as &mut dyn Bump;
            e.bump();
            format!("{:?} {} {:?} {} {}", g, read, b.grid.cells, c.n, d.n)
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Grid { cells: [[7, 18], [0, 0]], width: 2 } 9 [[5, 18], [0, 0]] 2 6"
    );
    let immutable = rejected(
        "use std::ops::{Index, IndexMut}; struct G { c: Vec<i64> } \
         impl Index<usize> for G { type Output = i64; fn index(&self, i: usize) -> &i64 { &self.c[i] } } \
         impl IndexMut<usize> for G { fn index_mut(&mut self, i: usize) -> &mut i64 { &mut self.c[i] } } \
         pub fn run() -> i64 { let g = G { c: vec![1] }; g[0] = 2; 0 } run()",
    );
    assert!(immutable.contains("immutable"), "{immutable}");
}

#[test]
fn language_block_expressions() {
    let value = run_string(
        r#"
        fn tail(x: i64) -> i64 {
            {
                let y = x * 2;
                { y + 1 }
            }
        }
        fn early(x: i64) -> i64 {
            let v = {
                if x > 5 {
                    return 0;
                }
                x + 10
            };
            v
        }
        pub fn run() -> String {
            let y = { let a = 2; a * 3 };
            let x = 1;
            {
                let x = "shadow";
                let _ = x;
            }
            let mut total = 0;
            for i in 0..3 {
                { total += i; }
            }
            let unit = {};
            let nested = { let a = { 1 + 1 }; let b = { let c = a * 3; c }; format!("{}-{}", a, b) };
            format!("{} {} {} {} {} {} {:?} {}", y, x, tail(4), early(2), early(9), total, unit, nested)
        }
        run()
    "#,
    );
    assert_eq!(value, "6 1 9 12 0 3 () 2-6");
    let unit_tail = rejected("pub fn run() -> i64 { let n: i64 = { let t = 1; t; }; n } run()");
    assert!(unit_tail.contains("expected 'i64', found '()'"), "{unit_tail}");
    rejected("pub fn run() -> i64 { { let t = 1; } t } run()");
}

#[test]
fn language_panic_macros() {
    let value = run_string(
        r#"
        fn later() -> i64 { todo!() }
        fn never() -> i64 { unimplemented!() }
        pub fn run() -> String {
            let x = 5;
            assert!(x > 1);
            assert!(x > 1, "x was {}", x);
            assert_eq!(x, 5);
            assert_eq!(x, 5, "x was {}", x);
            assert_ne!(x, 4);
            debug_assert!(true);
            debug_assert_eq!(x + 1, 6);
            debug_assert_ne!(String::from("a"), "b".to_string());
            assert_eq!(vec![1, 2], vec![1, 2]);
            let y = match x { 5 => 10, _ => unreachable!("bad {}", x) };
            let z = if y > 3 { y } else { panic!("small") };
            format!("ok {} {}", y, z)
        }
        run()
    "#,
    );
    assert_eq!(value, "ok 10 10");
    for (body, message) in [
        ("panic!()", "explicit panic"),
        ("panic!(\"boom {}\", x)", "boom 5"),
        ("assert!(x < 1)", "assertion failed: x < 1"),
        ("assert!(s.len()==9 && !s.is_empty())", "assertion failed: s.len() == 9 && !s.is_empty()"),
        ("assert!(x < 1, \"x was {x}\")", "x was 5"),
        ("assert_eq!(x, 4)", "assertion `left == right` failed\n  left: 5\n right: 4"),
        (
            "assert_eq!(s, \"q\", \"with {} args\", 2)",
            "assertion `left == right` failed: with 2 args\n  left: \"ab\"\n right: \"q\"",
        ),
        ("assert_ne!(x, 5)", "assertion `left != right` failed\n  left: 5\n right: 5"),
        ("debug_assert!(-x > 0)", "assertion failed: -x > 0"),
        ("todo!()", "not yet implemented"),
        ("todo!(\"later\")", "not yet implemented: later"),
        ("unimplemented!()", "not implemented"),
        ("unimplemented!(\"not {}\", \"here\")", "not implemented: not here"),
        ("unreachable!()", "internal error: entered unreachable code"),
        ("unreachable!(\"x is {}\", x)", "internal error: entered unreachable code: x is 5"),
    ] {
        let error = rejected(&format!(
            "pub fn run() -> i64 {{ let x = 5; let s = String::from(\"ab\"); {body}; 0 }} run()"
        ));
        assert!(error.contains(message), "{body}: {error}");
    }
    let mismatch = rejected("pub fn run() -> i64 { assert!(1); 0 } run()");
    assert!(mismatch.contains("expected 'bool'"), "{mismatch}");
}

#[test]
fn language_to_string_on_references_and_literals() {
    let value = run_string(
        r#"
        fn show(n: &i64, f: &f64, u: &u64, c: &char, b: &bool, s: &String) -> String {
            format!("{} {} {} {} {} {}", n.to_string(), f.to_string(), u.to_string(), c.to_string(), b.to_string(), s.to_string())
        }
        pub fn run() -> String {
            let n = 3;
            let r = &n;
            let v = vec![1, 2, 3];
            let big: Vec<String> = v.iter().filter(|x| **x > 1).map(|x| x.to_string()).collect();
            let t: f32 = 0.1;
            format!(
                "{} {} {} {} {} {} {} {:?} {}",
                r.to_string(), 1.5.to_string(), 'c'.to_string(), true.to_string(), 1.0.to_string(),
                (-2.5f64).to_string(), t.to_string(), big,
                show(&4, &2.0, &u64::MAX, &'z', &false, &String::from("s"))
            )
        }
        run()
    "#,
    );
    assert_eq!(value, "3 1.5 c true 1 -2.5 0.1 [\"2\", \"3\"] 4 2 18446744073709551615 z false s");
}

#[test]
fn language_string_and_vec_methods() {
    let value = run_string(
        r#"
        fn shrink(s: &mut String) { s.truncate(2); }
        pub fn run() -> String {
            let mut s = String::from("héllo");
            let last = s.pop();
            s.insert(0, '>');
            s.insert_str(1, "> ");
            let r = s.remove(0);
            let a = format!("{:?} {} {:?}", last, s, r);
            shrink(&mut s);
            let mut t = String::from("abcdef");
            let tail = t.split_off(4);
            t.clear();
            let b: Vec<u8> = "hé".bytes().collect();
            let n = "hé".as_bytes().len();
            let i: Vec<(usize, char)> = "hé!".char_indices().collect();
            let parts: Vec<&str> = "a,b,,c".splitn(3, ',').collect();
            let utf8 = format!("{:?} {}", String::from_utf8(vec![104, 105]), String::from_utf8(vec![0x41, 0xff]).unwrap_err());
            let mut v: Vec<i32> = Vec::with_capacity(10);
            v.push(1); v.push(2); v.push(3); v.push(4); v.push(5);
            let w: Vec<i32> = v.windows(2).map(|p| p[0] + p[1]).collect();
            let c: Vec<Vec<i32>> = v.chunks(2).map(|p| p.to_vec()).collect();
            let rest = v.split_off(3);
            let name = String::with_capacity(4);
            format!("{a}|{s}|{t}{tail}|{b:?} {n}|{i:?}|{parts:?}|{utf8}|{w:?} {c:?} {v:?} {rest:?} [{name}]")
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        "Some('o') > héll '>'|> |ef|[104, 195, 169] 3|[(0, 'h'), (1, 'é'), (3, '!')]|[\"a\", \"b\", \",c\"]|\
         Ok(\"hi\") invalid utf-8 sequence of 1 bytes from index 1|[3, 5, 7, 9] [[1, 2], [3, 4], [5]] [1, 2, 3] [4, 5] []"
    );
    for (body, message) in [
        ("let mut s = String::from(\"é\"); s.insert(1, 'x')", "assertion failed: self.is_char_boundary(idx)"),
        ("let mut s = String::from(\"ab\"); s.remove(2)", "cannot remove a char from the end of a string"),
        ("let mut s = String::from(\"ab\"); s.remove(5)", "start byte index 5 is out of bounds for string of length 2"),
        (
            "let mut s = String::from(\"éa\"); s.remove(1)",
            "start byte index 1 is not a char boundary; it is inside 'é' (bytes 0..2 of string)",
        ),
        ("let mut s = String::from(\"é\"); s.truncate(1)", "assertion failed: self.is_char_boundary(new_len)"),
        ("let v = vec![1]; let n = v.windows(0).count()", "window size must be non-zero"),
        ("let v = vec![1]; let n = v.chunks(0).count()", "chunk size must be non-zero"),
        ("let mut v = vec![1]; let w = v.split_off(3)", "`at` split index (is 3) should be <= len (is 1)"),
    ] {
        let error = rejected(&format!("pub fn run() -> i64 {{ {body}; 0 }} run()"));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn language_byte_and_raw_strings() {
    let value = run_string(
        r##"
        pub fn run() -> String {
            let a = b"hi\n\x00\xff";
            let s: &[u8] = b"ok";
            let raw = r#"say "hi" #now"#;
            let plain = r"back\slash";
            let svg = br#"<a "b">"#;
            let owned = String::from("yo").into_bytes();
            let mut pixels = vec![0u8; 4];
            pixels[3] = 255;
            let r = 1;
            let b = 2;
            format!("{a:?} {s:?}|{raw}|{plain}|{svg:?}|{owned:?}|{pixels:?}|{}", r + b)
        }
        run()
    "##,
    );
    assert_eq!(
        value,
        "[104, 105, 10, 0, 255] [111, 107]|say \"hi\" #now|back\\slash|\
         [60, 97, 32, 34, 98, 34, 62]|[121, 111]|[0, 0, 0, 255]|3"
    );
    rejected("pub fn run() -> i64 { let a = b\"é\"; 0 } run()");
}

#[test]
fn language_std_mem_swap_replace_take() {
    let value = run_string(
        r#"
        use std::mem::{replace, take};
        use std::mem;
        #[derive(Debug, Default)]
        struct P { a: Vec<i64>, b: Vec<i64>, name: String }
        fn rotate(x: &mut i64, y: &mut i64) { mem::swap(x, y); }
        pub fn run() -> String {
            let mut a = 1;
            let mut b = 2;
            std::mem::swap(&mut a, &mut b);
            let old = std::mem::replace(&mut a, 0);
            let mut v = vec![1];
            let t = std::mem::take(&mut v);
            let mut p = P { a: vec![1], b: vec![2, 3], name: String::from("n") };
            mem::swap(&mut p.a, &mut p.b);
            let n = take(&mut p.name);
            let first = replace(&mut p.a[0], 9);
            let (mut x, mut y) = (5, 6);
            rotate(&mut x, &mut y);
            format!("{} {} {} {:?} {:?} {:?} {} {} {} {}", a, b, old, t, v, p, n, first, x, y)
        }
        run()
    "#,
    );
    assert_eq!(value, "0 1 2 [1] [] P { a: [9, 3], b: [1], name: \"\" } n 2 6 5");
    // A crate's own `swap` is called as written.
    let own = run("fn swap(a: i64, b: i64) -> i64 { a - b } pub fn run() -> i64 { swap(5, 2) } run()");
    assert_eq!(own, 3);
    rejected("pub fn run() -> i64 { let mut a = 1; std::mem::swap(&mut a, &mut a); a } run()");
}

#[test]
fn language_let_else() {
    let value = run_string(
        r#"
        enum Shape { Circle(f64), Square(f64) }
        fn area(s: &Shape) -> f64 {
            let Shape::Circle(r) = s else { return 0.0; };
            3.0 * r * r
        }
        fn parse(t: &str) -> i64 {
            let Ok(n) = t.parse::<i64>() else { panic!("bad {}", t) };
            n
        }
        pub fn run() -> String {
            let Some(q) = Some(3) else { return String::new(); };
            let mut letters = String::new();
            for name in vec!["ab", "", "cd"] {
                let Some(mut c) = name.chars().next() else { continue; };
                c = c.to_ascii_uppercase();
                letters.push(c);
            }
            let x = if q > 2 { 1 } else { 2 };
            let None = Some(1) else { return format!("{} {} {} {} {}", q, letters, area(&Shape::Circle(1.0)), area(&Shape::Square(2.0)), parse("42") + x); };
            String::new()
        }
        run()
    "#,
    );
    assert_eq!(value, "3 AC 3 0 43");
    let error = rejected("pub fn run() -> i64 { let Some(x) = Some(1) else { 0 }; x } run()");
    assert!(error.contains("`!`"), "{error}");
    let error = rejected("pub fn run() -> i64 { let Some(x) = Some(1) else { return 0; }; } run()");
    assert!(!error.is_empty());
    rejected("pub fn run() -> i64 { let Some(x) = Some(1) else { let y = x; return 0; }; x } run()");
}

#[test]
fn language_nested_fn_items() {
    let value = run_string(
        r#"
        struct C { n: i64 }
        impl C {
            fn get(&self) -> i64 { fn add(a: i64, b: i64) -> i64 { a + b } add(self.n, 1) }
        }
        pub fn run() -> String {
            let first = twice(sq(3));
            fn sq(x: i64) -> i64 { x * x }
            fn twice(x: i64) -> i64 { fact(2) * x / 2 }
            fn fact(n: i64) -> i64 { if n <= 1 { 1 } else { n * fact(n - 1) } }
            let f: fn(i64) -> i64 = sq;
            let label = if true {
                fn inner() -> String { String::from("inner") }
                inner()
            } else {
                String::new()
            };
            format!("{} {} {} {}", first, f(5), label, C { n: 1 }.get())
        }
        run()
    "#,
    );
    assert_eq!(value, "9 25 inner 2");
}

#[test]
fn language_untyped_integers_are_i32() {
    let error = rejected("pub fn run() -> i64 { let f = (1..=20).fold(1, |a, b| a * b); println!(\"{}\", f); 0 } run()");
    assert!(error.contains("attempt to multiply with overflow"), "{error}");
    let error = rejected("pub fn run() -> i64 { let mut x = 2147483600; for _ in 0..100 { x += 1; } println!(\"{}\", x); 0 } run()");
    assert!(error.contains("attempt to add with overflow"), "{error}");
    // A literal an `i64` binds is an `i64`, before or after the arithmetic.
    let value = run(r#"
        fn widen(x: i64) -> i64 { x }
        pub fn run() -> i64 {
            let mut x = 2147483600;
            x += 100;
            let y: i64 = 3_000_000_000 * 2;
            widen(x) + y
        }
        run()
    "#);
    assert_eq!(value, 2_147_483_700 + 6_000_000_000);
    let value = run_string(
        "pub fn run() -> String { let n = 7; let m = n * 6; format!(\"{} {}\", m, (1..=10).fold(1, |a, b| a * b)) } run()",
    );
    assert_eq!(value, "42 3628800");
}

#[test]
fn language_debug_escapes_strings_and_chars() {
    let value = run_string(
        r#"
        #[derive(Debug)]
        struct S { name: String, c: char }
        pub fn run() -> String {
            let s = "a\"b\n\t\\ é \u{7f} \u{301}x '";
            let v = vec![String::from("x\"y"), String::from("__rawr_z")];
            format!(
                "{:?}|{:?} {:?} {:?} {:?}|{:?} {:?}|{:?}|{:?}",
                s, '\'', '"', '\n', '\u{301}', v, Some("q\""),
                S { name: String::from("n\"m"), c: '\'' }, ("t\"", 'x')
            )
        }
        run()
    "#,
    );
    assert_eq!(
        value,
        r#""a\"b\n\t\\ é \u{7f} \u{301}x '"|'\'' '"' '\n' '\u{301}'|["x\"y", "__rawr_z"] Some("q\"")|S { name: "n\"m", c: '\'' }|("t\"", 'x')"#
    );
}

/// Every float method the checker accepts runs, and gives Rust's value.
#[test]
fn language_every_accepted_float_method_runs() {
    let unary = |name: &str, x: f64| -> String {
        match name {
            "abs" => format!("{:?}", x.abs()),
            "sqrt" => format!("{:?}", x.sqrt()),
            "floor" => format!("{:?}", x.floor()),
            "ceil" => format!("{:?}", x.ceil()),
            "round" => format!("{:?}", x.round()),
            "trunc" => format!("{:?}", x.trunc()),
            "is_nan" => format!("{:?}", x.is_nan()),
            "fract" => format!("{:?}", x.fract()),
            "is_finite" => format!("{:?}", x.is_finite()),
            "is_infinite" => format!("{:?}", x.is_infinite()),
            "sin" => format!("{:?}", x.sin()),
            "cos" => format!("{:?}", x.cos()),
            "tan" => format!("{:?}", x.tan()),
            "asin" => format!("{:?}", x.asin()),
            "acos" => format!("{:?}", x.acos()),
            "atan" => format!("{:?}", x.atan()),
            "sinh" => format!("{:?}", x.sinh()),
            "cosh" => format!("{:?}", x.cosh()),
            "tanh" => format!("{:?}", x.tanh()),
            "asinh" => format!("{:?}", x.asinh()),
            "acosh" => format!("{:?}", x.acosh()),
            "atanh" => format!("{:?}", x.atanh()),
            "exp" => format!("{:?}", x.exp()),
            "ln" => format!("{:?}", x.ln()),
            "log10" => format!("{:?}", x.log10()),
            "to_degrees" => format!("{:?}", x.to_degrees()),
            "to_radians" => format!("{:?}", x.to_radians()),
            other => panic!("`{other}` is accepted on floats but this test does not know it"),
        }
    };
    let mut calls: Vec<(String, Box<dyn Fn(f64) -> String>)> = super::FLOAT_UNARY_METHODS
        .iter()
        .map(|(name, _)| {
            let name = name.to_string();
            let expected: Box<dyn Fn(f64) -> String> = Box::new({
                let name = name.clone();
                move |x| unary(&name, x)
            });
            (format!("{name}()"), expected)
        })
        .collect();
    calls.push(("powi(3)".into(), Box::new(|x: f64| format!("{:?}", x.powi(3)))));
    calls.push(("atan2(1.5)".into(), Box::new(|x: f64| format!("{:?}", x.atan2(1.5)))));
    calls.push(("hypot(1.5)".into(), Box::new(|x: f64| format!("{:?}", x.hypot(1.5)))));
    calls.push(("log(2.0)".into(), Box::new(|x: f64| format!("{:?}", x.log(2.0)))));
    calls.push(("powf(0.5)".into(), Box::new(|x: f64| format!("{:?}", x.powf(0.5)))));
    calls.push(("min(1.0)".into(), Box::new(|x: f64| format!("{:?}", x.min(1.0)))));
    calls.push(("max(1.0)".into(), Box::new(|x: f64| format!("{:?}", x.max(1.0)))));
    calls.push(("clamp(-1.0, 1.0)".into(), Box::new(|x: f64| format!("{:?}", x.clamp(-1.0, 1.0)))));
    calls.push(("partial_cmp(&1.0)".into(), Box::new(|x: f64| format!("{:?}", x.partial_cmp(&1.0)))));
    calls.push(("to_string()".into(), Box::new(|x: f64| format!("{:?}", x.to_string()))));
    calls.push(("clone()".into(), Box::new(|x: f64| format!("{:?}", x))));
    for (call, expected) in &calls {
        for x in [2.25, -2.5, 0.5, -0.5, 3.0] {
            for ty in ["f64", "f32"] {
                let source = format!(
                    "pub fn run() -> String {{ let x: {ty} = {x:?}; format!(\"{{:?}}\", x.{call}) }} run()"
                );
                let value = Engine::new()
                    .eval::<String>(&source)
                    .unwrap_or_else(|error| panic!("{ty}::{call} of {x}: {error}"));
                // An `f32` result prints as an `f32`.
                let mut expected = expected(x);
                if ty == "f32" {
                    if let Ok(number) = expected.parse::<f64>() {
                        expected = format!("{:?}", number as f32);
                    } else if let Some(number) = expected
                        .strip_prefix('"')
                        .and_then(|text| text.strip_suffix('"'))
                        .and_then(|text| text.parse::<f64>().ok())
                    {
                        expected = format!("{:?}", (number as f32).to_string());
                    }
                }
                assert_eq!(value, expected, "{ty}::{call} of {x}");
            }
        }
    }
}

/// Every placeholder of std::fmt's grammar prints Rust's text: the table
/// is checked against the same program built by cargo.
#[test]
fn language_format_specs_match_rust() {
    let value = Engine::new()
        .eval::<String>(
            r#"
pub fn run() -> String {
    let digits: usize = 5;
    let width: usize = 8;
    let x: f64 = 3.14159;
    let n: i32 = 255;
    let neg: i32 = -42;
    let big: u64 = u64::MAX;
    let s = "hello";
    let c = 'z';
    let b = true;
    let v = vec![1, 2, 3];
    let f: f32 = 1.5;
    let name = "Rawr";
    let mut out = String::new();
    out = out + &format!("{:0digits$}", 1, digits = 5) + "\n";
    out = out + &format!("{:*^10}", "mid") + "\n";
    out = out + &format!("{:<8}|", "ab") + "\n";
    out = out + &format!("{:>8}|", "ab") + "\n";
    out = out + &format!("{:^9}|", "ab") + "\n";
    out = out + &format!("{:+}", 5) + "\n";
    out = out + &format!("{:+}", -5) + "\n";
    out = out + &format!("{:+.2}", x) + "\n";
    out = out + &format!("{:#x}", n) + "\n";
    out = out + &format!("{:#X}", n) + "\n";
    out = out + &format!("{:#o}", n) + "\n";
    out = out + &format!("{:#b}", 5u8) + "\n";
    out = out + &format!("{:x}", neg) + "\n";
    out = out + &format!("{:08.3}", x) + "\n";
    out = out + &format!("{:#010x}", n) + "\n";
    out = out + &format!("{:1$}|", 7, 4) + "\n";
    out = out + &format!("{:width$}|", 7) + "\n";
    out = out + &format!("{:>width$.prec$}", x, prec = 2) + "\n";
    out = out + &format!("{:.*}", 3, x) + "\n";
    out = out + &format!("{:.1$}", x, 1) + "\n";
    out = out + &format!("{0} {1} {0}", "a", "b") + "\n";
    out = out + &format!("{1:?} {0}", "a", "b") + "\n";
    out = out + &format!("{name} {digits}") + "\n";
    out = out + &format!("{x:>10.2}") + "\n";
    out = out + &format!("{:e}", 1234.5) + "\n";
    out = out + &format!("{:E}", 0.00012) + "\n";
    out = out + &format!("{:.2e}", 1234.5) + "\n";
    out = out + &format!("{:e}", 1500) + "\n";
    out = out + &format!("{:x?}", 255) + "\n";
    out = out + &format!("{:X?}", 255) + "\n";
    out = out + &format!("{:#?}", v) + "\n";
    out = out + &format!("{:?}", s) + "\n";
    out = out + &format!("{:>8?}|", s) + "\n";
    out = out + &format!("{:5}|", c) + "\n";
    out = out + &format!("{:>6}|", b) + "\n";
    out = out + &format!("{:.3}", "abcdef") + "\n";
    out = out + &format!("{:>8.3}|", "abcdef") + "\n";
    out = out + &format!("{:?}", x) + "\n";
    out = out + &format!("{:?}", f) + "\n";
    out = out + &format!("{}", big) + "\n";
    out = out + &format!("{:x}", big) + "\n";
    out = out + &format!("{:05}", -42) + "\n";
    out = out + &format!("{:<05}", 7) + "\n";
    out = out + &format!("{:^+9.1}|", x) + "\n";
    out = out + &format!("{:#b}", 0) + "\n";
    out = out + &format!("{{}} {}", 1) + "\n";
    out = out + &format!("{:-^7}", "") + "\n";
    out = out + &format!("{:.0}", 2.5) + "\n";
    out = out + &format!("{:10.3e}|", 123456.0) + "\n";
    out = out + &format!("{:05}|", "ab") + "\n";
    out = out + &format!("{:#x?}", 255) + "\n";
    out = out + &format!("{:+e}", 1234.5) + "\n";
    out = out + &format!("{:?} {}", s, s) + "\n";
    out
}
run()
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let expected = r#"00001
***mid****
ab      |
      ab|
   ab    |
+5
-5
+3.14
0xff
0xFF
0o377
0b101
ffffffd6
0003.142
0x000000ff
   7|
       7|
    3.14
3.142
3.1
a b a
"b" a
Rawr 5
      3.14
1.2345e3
1.2E-4
1.23e3
1.5e3
ff
FF
[
    1,
    2,
    3,
]
"hello"
"hello"|
z    |
  true|
abc
     abc|
3.14159
1.5
18446744073709551615
ffffffffffffffff
-0042
00007
  +3.1   |
0b0
{} 1
-------
2
   1.235e5|
ab   |
0xff
+1.2345e3
"hello" hello
"#;
    for (line, (actual, expected)) in value.lines().zip(expected.lines()).enumerate() {
        assert_eq!(actual, expected, "line {}", line + 1);
    }
    assert_eq!(value, expected);
}

#[test]
fn language_format_strings_are_checked_as_rustc_does() {
    let value = Engine::new()
        .eval::<String>(
            r#"
            pub fn run() -> String {
                println!("{:0digits$}", 1, digits = 5);
                let width: usize = 6;
                let y = 2.5;
                format!("{:0digits$}|{y:>width$}|{:>1$}|{:.*}", 1, 7, 3, y, digits = 5)
            }
            run()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let (width, y) = (6usize, 2.5);
    assert_eq!(value, format!("{:0digits$}|{y:>width$}|{:>1$}|{:.*}", 1, 7, 3, y, digits = 5));
    for source in [
        r#"format!("{} {}", 1)"#,
        r#"format!("{}", 1, 2)"#,
        r#"format!("{a}", b = 1)"#,
        r#"format!("{missing}")"#,
        r#"format!("{:1$}", 7)"#,
        r#"format!("{:.*}", 2.5)"#,
        r#"format!("{:x}", 2.5)"#,
        r#"format!("{:o}", "s")"#,
        r#"format!("{:e}", true)"#,
        r#"format!("{:width$}", 1, width = 2.5)"#,
        r#"format!("{:q}", 1)"#,
        r#"format!("{")"#,
        r#"format!("}")"#,
    ] {
        rejected(&format!("pub fn run() -> i64 {{ let s = {source}; 0 }} run()"));
    }
}

#[test]
fn language_builtin_functions_as_values() {
    let value = Engine::new()
        .eval::<String>(
            r#"
            #[derive(Debug)]
            enum Word { Named(String) }
            pub fn run() -> String {
                let words: Vec<String> = vec!["a", "b"].into_iter().map(String::from).collect();
                let roots: Vec<f64> = vec![4.0, 9.0].into_iter().map(f64::sqrt).collect();
                let ones: Vec<i64> = vec![true, false].into_iter().map(i64::from).collect();
                let named: Vec<Word> = words.clone().into_iter().map(Word::Named).collect();
                let upper: Vec<char> = "ab".chars().map(char::to_ascii_uppercase).collect();
                format!("{:?} {:?} {:?} {:?} {:?}", words, roots, ones, named, upper)
            }
            run()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value, r#"["a", "b"] [2.0, 3.0] [1, 0] [Named("a"), Named("b")] ['A', 'B']"#);
}

#[test]
fn language_fn_traits_with_lifetimes_and_auto_traits() {
    let value = Engine::new()
        .eval::<String>(
            r#"
            fn apply<'a>(f: impl Fn(i64) -> i64 + 'a, x: i64) -> i64 {
                f(x)
            }
            fn twice(f: &dyn Fn(i64) -> i64, x: i64) -> i64 {
                f(f(x))
            }
            fn count(mut f: impl FnMut() -> i64 + Send) -> i64 {
                f() + f()
            }
            fn adder<'a>(n: &'a i64) -> impl Fn(i64) -> i64 + 'a {
                move |x| x + *n
            }
            fn boxed(n: i64) -> Box<dyn Fn(i64) -> i64 + Send + 'static> {
                Box::new(move |x| x * n)
            }
            pub fn run() -> String {
                let n = 5;
                let mut calls = 0;
                let a = apply(|x| x + 1, 1);
                let b = twice(&|x| x * 3, 2);
                let c = count(|| { calls += 1; calls });
                let d = adder(&n)(10);
                let e = boxed(3)(4);
                format!("{a} {b} {c} {d} {e}")
            }
            run()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    // The same program built by rustc prints `2 18 3 15 12`.
    assert_eq!(value, "2 18 3 15 12");
    rejected("fn f(g: impl Fn(i64) -> i64 + Clone) -> i64 { g(1) } pub fn run() -> i64 { f(|x| x) } run()");
}

#[test]
fn language_ranges_as_types_and_hand_written_hash() {
    let value = Engine::new()
        .eval::<String>(
            r#"
use std::collections::HashSet;
use std::hash::Hash;
use std::ops::{Range, RangeInclusive};

struct Bounds {
    span: RangeInclusive<i64>,
    half: Range<usize>,
    unit: RangeInclusive<f64>,
}

fn width(range: &RangeInclusive<i64>) -> i64 {
    range.end() - range.start()
}

fn make(n: i64) -> RangeInclusive<i64> {
    0..=n
}

#[derive(Debug, Eq, PartialEq)]
struct Tag {
    name: String,
}

impl Hash for Tag {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct Pair(i64, i64);

pub fn run() -> String {
    let b = Bounds { span: 2..=9, half: 0..4, unit: 0.0..=1.0 };
    let copy = b.span.clone();
    let total: i64 = make(4).sum();
    let mut tags = HashSet::new();
    tags.insert(Tag { name: String::from("a") });
    tags.insert(Tag { name: String::from("a") });
    tags.insert(Tag { name: String::from("b") });
    let mut pairs = HashSet::new();
    pairs.insert(Pair(1, 2));
    pairs.insert(Pair(1, 2));
    format!(
        "{} {} {} {} {} {} {} {} {}",
        width(&b.span),
        b.half.len(),
        copy.contains(&5),
        total,
        *b.span.start(),
        b.unit.contains(&0.5),
        b.unit.end(),
        tags.len(),
        pairs.len()
    )
}
run()
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    // The same program built by rustc prints this.
    assert_eq!(value, "7 4 true 10 2 true 1 2 1");
}

#[test]
fn language_float_constants_and_compile_time_text() {
    let value = Engine::new()
        .eval::<String>(
            r#"
            use std::f32::consts::TAU;
            use std::f64::consts;
            pub fn run() -> String {
                let text = concat!("ra", "wr", '-', 1, 2.5, true, stringify!(a + b));
                format!("{:.5} {:.5} {:.5} {}", std::f64::consts::PI, consts::E, TAU, text)
            }
            run()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        value,
        format!(
            "{:.5} {:.5} {:.5} {}",
            std::f64::consts::PI,
            std::f64::consts::E,
            std::f32::consts::TAU,
            concat!("ra", "wr", '-', 1, 2.5, true, stringify!(a + b))
        )
    );
    // Without a file on disk there is no project for `env!` or a path for
    // `include_str!` to name.
    let error = rejected(r#"pub fn run() -> i64 { let p = env!("CARGO_MANIFEST_DIR"); 0 } run()"#);
    assert!(error.contains("env!"), "{error}");
    let error = rejected(r#"pub fn run() -> i64 { let p = include_str!("x.txt"); 0 } run()"#);
    assert!(error.contains("not in one"), "{error}");
}

#[test]
fn language_impl_trait_with_associated_type_equalities() {
    let value = run(r#"
        trait Shape {
            type Output;
            fn area(&self) -> Self::Output;
        }
        struct Square(i64);
        impl Shape for Square {
            type Output = i64;
            fn area(&self) -> i64 { self.0 * self.0 }
        }
        fn total(shape: impl Shape<Output = i64>, extra: i64) -> i64 {
            shape.area() + extra
        }
        fn evens() -> impl Iterator<Item = i64> {
            vec![2, 4].into_iter()
        }
        pub fn run() -> i64 {
            total(Square(3), evens().sum::<i64>())
        }
        run()
    "#);
    assert_eq!(value, 15);
}

#[test]
fn language_statics_and_unsafe_blocks() {
    let value = Engine::new()
        .eval::<String>(
            r#"
            static LIMIT: i64 = 40;
            pub static NAMES: [&str; 2] = ["a", "b"];
            pub fn run() -> String {
                let doubled = unsafe { LIMIT * 2 };
                format!("{} {} {}", LIMIT + 2, NAMES[1], doubled)
            }
            run()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    // The same program built by rustc prints `42 b 80`.
    assert_eq!(value, "42 b 80");
    let error = rejected("static mut COUNT: i64 = 0; pub fn run() -> i64 { 0 } run()");
    assert!(error.contains("static mut"), "{error}");
    let error = rejected("unsafe fn f() {} pub fn run() -> i64 { 0 } run()");
    assert!(error.contains("not supported in Rawr yet"), "{error}");
}

#[test]
fn language_range_bounds_take_the_element_kind() {
    let value = Engine::new()
        .eval::<f64>(
            r#"
            use std::ops::RangeInclusive;
            const SIZE: RangeInclusive<f32> = 200.0..=400.0;
            pub fn run() -> f32 {
                let middle: Option<f32> = true.then_some(SIZE.start() + (SIZE.end() - SIZE.start()) / 2.0);
                middle.unwrap()
            }
            run()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value, 300.0);
}

#[test]
fn language_mut_methods_through_field_and_element_places() {
    let value = run(r#"
        struct Counter { n: i64 }
        impl Counter { fn bump(&mut self) { self.n += 1; } }
        struct Inner { log: Vec<i64>, name: String, items: Vec<Counter> }
        struct State { text: String, inner: Inner, rows: Vec<String> }
        impl State {
            fn change(&mut self, x: &str) {
                self.text.push_str(&x);
                self.text.push('!');
                self.inner.name.push_str(x);
                self.inner.log.push(3);
                self.inner.log.sort();
                self.inner.log.sort_by_key(|n| -*n);
                self.rows[0].push_str("z");
                self.inner.items[0].bump();
                self.inner.items.iter_mut().for_each(|c| c.n *= 10);
            }
        }
        pub fn run() -> i64 {
            let mut s = State {
                text: String::new(),
                inner: Inner { log: vec![5, 1], name: String::from("n"), items: vec![Counter { n: 1 }] },
                rows: vec![String::from("a")],
            };
            s.change("ab");
            let mut v = vec![String::from("q")];
            v[0].push_str("rs");
            let ok = s.text == "ab!" && s.inner.name == "nab" && s.rows[0] == "az" && v[0] == "qrs";
            if ok { s.inner.log[0] * 100 + s.inner.log[2] * 10 + s.inner.items[0].n } else { -1 }
        }
        run()
    "#);
    assert_eq!(value, 5 * 100 + 1 * 10 + 20);
    let error = rejected(
        r#"
        struct S { text: String }
        impl S { fn f(&self) { self.text.push_str("x"); } }
        pub fn run() -> i64 { 0 }
        run()
    "#,
    );
    assert!(error.contains("self"), "{error}");
}

#[test]
fn language_patterns_on_mut_places_write_their_bindings_back() {
    let value = run(r#"
        enum Mode { Preview, Stream { content: String, offset: usize } }
        enum State { Idle, Ticking { last: i64, ticks: i64 } }
        struct App { mode: Mode, out: String, state: State, total: i64 }
        impl App {
            fn next(&mut self) {
                match &mut self.mode {
                    Mode::Preview => {}
                    Mode::Stream { content, offset } => {
                        if *offset >= content.len() {
                            self.mode = Mode::Preview;
                        } else if let Some(token) = content[*offset..].chars().next() {
                            self.out.push(token);
                            *offset += token.len_utf8();
                        }
                    }
                }
            }
            fn tick(&mut self, now: i64) -> i64 {
                if let State::Ticking { last, ticks } = &mut self.state {
                    self.total += now - *last;
                    *last = now;
                    *ticks += 1;
                    if *ticks == 2 { return 1; }
                }
                0
            }
        }
        fn grow(slots: &mut Vec<Option<i64>>) -> i64 {
            let mut total = 0;
            for slot in slots.iter_mut() {
                if let Some(n) = slot { *n *= 2; total += *n; }
            }
            match &mut slots[0] { Some(n) if *n > 100 => *n = 0, Some(n) => *n += 1, None => {} }
            total
        }
        pub fn run() -> i64 {
            let mut app = App {
                mode: Mode::Stream { content: String::from("héy"), offset: 0 },
                out: String::new(),
                state: State::Ticking { last: 1, ticks: 0 },
                total: 0,
            };
            for _ in 0..5 { app.next(); }
            let early = app.tick(5) * 10 + app.tick(9);
            let ticks = match &app.state { State::Ticking { ticks, .. } => *ticks, State::Idle => -1 };
            let mut slots = vec![Some(1), None, Some(5)];
            let doubled = grow(&mut slots);
            let mut opt = Some(0);
            while let Some(n) = &mut opt {
                *n += 1;
                if *n > 3 { break; }
            }
            let mut x = Some(1);
            match &mut x { Some(v) => *v += 1, None => {} }
            let preview = matches!(app.mode, Mode::Preview) && app.out == "héy";
            if !preview { return -1; }
            app.total * 1000000 + early * 10000 + ticks * 1000 + doubled * 10
                + slots[0].unwrap() + opt.unwrap() * 100 + x.unwrap() * 100000
        }
        run()
    "#);
    assert_eq!(value, 8 * 1000000 + 1 * 10000 + 2 * 1000 + 12 * 10 + 3 + 4 * 100 + 2 * 100000);
}

#[test]
fn language_mut_pattern_write_back_keeps_a_nan_neighbour() {
    // The place is unchanged when the body leaves it alone, even though a
    // NaN field is never `==` to itself.
    let value = run(r#"
        enum State { Idle, Running { progress: f32, rate: f32 } }
        struct App { state: State }
        impl App {
            fn step(&mut self) {
                if let State::Running { progress, .. } = &mut self.state { *progress = 42.0; }
            }
        }
        pub fn run() -> i64 {
            let zero: f32 = 0.0;
            let mut app = App { state: State::Running { progress: 0.0, rate: zero / zero } };
            app.step();
            match app.state { State::Running { progress, .. } => progress as i64, State::Idle => -1 }
        }
        run()
    "#);
    assert_eq!(value, 42);
}

#[test]
fn language_a_call_of_no_function_names_it() {
    let error = rejected("fn f() -> i64 { centre(1) } pub fn run() -> i64 { f() } run()");
    assert!(error.contains("nknown function") && error.contains("centre"), "{error}");
}

#[test]
fn language_precise_capture_bounds_and_async_functions() {
    let value = run(r#"
        fn nums(limit: i64) -> impl Iterator<Item = i64> + use<> { (1..limit).map(|n| n * 2) }
        pub fn run() -> i64 { nums(4).sum::<i64>() }
        run()
    "#);
    assert_eq!(value, 12);
    for source in [
        "async fn go() -> i64 { 1 } pub fn run() -> i64 { 0 } run()",
        "struct S; impl S { async fn go(&self) -> i64 { 1 } } pub fn run() -> i64 { 0 } run()",
    ] {
        let error = rejected(source);
        assert!(error.contains("not supported in Rawr yet"), "{error}");
    }
}

#[test]
fn language_restricted_item_visibility() {
    let value = run(r#"
        pub(crate) struct Point { pub(crate) x: i64, pub(super) y: i64 }
        pub(crate) fn sum(p: &Point) -> i64 { p.x + p.y }
        pub(in crate) fn twice(n: i64) -> i64 { n * 2 }
        pub(self) fn one() -> i64 { 1 }
        pub fn run() -> i64 { twice(sum(&Point { x: 2, y: 3 })) + one() }
        run()
    "#);
    assert_eq!(value, 11);
}

#[test]
fn language_fieldless_enum_casts() {
    let value = run(r#"
        #[derive(Clone, Copy)]
        enum Level { Low, Mid = 10, High, Negative = -3, After }
        pub fn run() -> i64 {
            let high = Level::High;
            (Level::Low as i64) + (Level::Mid as i32) as i64 * 100 + high as i64 * 10000
                + (Level::Negative as i64) * 1000000 + (Level::After as u8) as i64 * 100000000
        }
        run()
    "#);
    assert_eq!(value, 0 + 10 * 100 + 11 * 10000 + -3 * 1000000 + 254 * 100000000);
    rejected("enum E { A(i64) } pub fn run() -> i64 { E::A(1) as i64 } run()");
}

#[test]
fn language_fn_pointer_casts_and_trait_function_values() {
    let value = run(r#"
        struct Item(String);
        impl From<&str> for Item { fn from(s: &str) -> Self { Item(s.to_string()) } }
        fn double(x: i64) -> i64 { x * 2 }
        pub fn run() -> i64 {
            let f = double as fn(i64) -> i64;
            let g: fn(i64) -> i64 = From::from;
            let items: Vec<Item> = ["a", "bb"].into_iter().map(From::from).collect();
            let s: Vec<String> = vec![1, 2].iter().map(ToString::to_string).collect();
            let t: Vec<String> = ["a", "b"].into_iter().map(Into::into).collect();
            f(4) + g(3) + items[1].0.len() as i64 + s.len() as i64 + t.len() as i64
        }
        run()
    "#);
    assert_eq!(value, 17);
}

#[test]
fn language_std_glob_imports_bind_the_module_names() {
    let value = run(r#"
        use std::collections::{self, *};
        use std::cmp::{self, *};
        pub fn run() -> i64 {
            let mut set: HashSet<i64> = HashSet::new();
            set.insert(3);
            let mut map = BTreeMap::new();
            map.insert(1, 2);
            let order = if max(1, 2) == 2 { Ordering::Less } else { Ordering::Greater };
            let less = if order == Ordering::Less { 10 } else { 0 };
            set.len() as i64 + map[&1] + less + cmp::min(4, 5) + min(0, 1)
        }
        run()
    "#);
    assert_eq!(value, 17);
}

#[test]
fn language_registered_rust_functions_are_typed() {
    let mut engine = Engine::new();
    engine.register_fn("double", |n: i64| n * 2);
    engine.register_fn("shout", |s: &str| s.to_uppercase());
    engine.register_fn("half", |x: f64| x / 2.0);
    let value = engine
        .eval::<i64>(
            r#"
            pub fn run() -> i64 {
                let n: i64 = double(20);
                let s: String = shout("hi");
                let h: f64 = half(5.0);
                n + s.len() as i64 + h as i64
            }
            run()
        "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value, 44);
}

#[test]
fn language_type_aliases() {
    let value = run(r#"
        use std::collections::HashMap;
        type Pair = (i64, i64);
        type Grid<T> = Vec<Vec<T>>;
        type Totals = HashMap<String, Total>;
        type Total = i64;
        type Res<T> = Result<T, String>;
        type At = Point;
        struct Point { x: i64, y: i64 }
        fn sum(pair: Pair) -> Total { pair.0 + pair.1 }
        fn corner(grid: &Grid<i64>) -> i64 { grid[0][0] }
        fn half(n: i64) -> Res<i64> { if n % 2 == 0 { Ok(n / 2) } else { Err("odd".to_string()) } }
        pub fn run() -> i64 {
            let pair: Pair = (2, 3);
            let mut totals: Totals = HashMap::new();
            totals.insert("a".to_string(), sum(pair));
            let grid: Grid<i64> = vec![vec![10]];
            let at: At = At { x: 100, y: 0 };
            totals["a"] + corner(&grid) + half(8).unwrap() + at.x + at.y
        }
        run()
    "#);
    assert_eq!(value, 5 + 10 + 4 + 100);
    rejected("type Pair = (i64, i64); pub fn run() -> i64 { let p: Pair = (1, true); p.0 } run()");
    rejected("type Grid<T> = Vec<T>; pub fn run() -> i64 { let g: Grid = vec![1]; g[0] } run()");
}

#[test]
fn language_field_and_variant_attributes_are_inert() {
    // Derive helpers such as `#[serde(..)]` mean nothing to Rawr, as rustc
    // leaves them inert. `#[cfg(..)]` still leaves a field out.
    let value = run(r#"
        #[derive(Debug, Clone, Default)]
        struct Task {
            #[serde(default = "Uuid::new_v4")]
            id: i64,
            #[serde(skip)]
            #[allow(dead_code)]
            pub weight: i64,
            #[cfg(test)]
            missing: String,
        }
        struct Pair(#[allow(unused)] i64, i64);
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        enum Filter {
            #[serde(rename = "all")]
            #[default]
            All,
            Done { #[allow(unused)] count: i64 },
        }
        pub fn run() -> i64 {
            let task = Task { id: 1, weight: 20 };
            let pair = Pair(300, 4000);
            let done = match (Filter::Done { count: 50000 }) { Filter::Done { count } => count, Filter::All => 0 };
            let all = if Filter::default() == Filter::All { 600000 } else { 0 };
            task.id + task.weight + pair.0 + pair.1 + done + all
        }
        run()
    "#);
    assert_eq!(value, 654321);
}

#[test]
fn language_use_with_a_leading_path_separator() {
    let value = run(r#"
        use ::std::cmp::Ordering;
        use ::std::time::{Duration};
        pub fn run() -> i64 {
            let a = match 1.cmp(&2) { Ordering::Less => 1, _ => 0 };
            a + Duration::from_millis(20).as_millis() as i64
        }
        run()
    "#);
    assert_eq!(value, 21);
}

#[test]
fn language_builtin_default_calls() {
    let value = run(r#"
        pub fn run() -> i64 {
            let mut text = String::default();
            text.push_str("abc");
            let n = u32::default() + 4;
            let on = if bool::default() { 0 } else { 50 };
            let x = f32::default();
            text.len() as i64 + n as i64 + on + x as i64
        }
        run()
    "#);
    assert_eq!(value, 57);
}

#[test]
fn language_block_like_statement_as_a_method_receiver() {
    let value = run_string(
        r#"
        use std::fmt;
        enum Plan { Basic, Pro }
        impl fmt::Display for Plan {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    Plan::Basic => "Basic",
                    Plan::Pro => "Pro",
                }
                .fmt(f)
            }
        }
        enum Color { Red, Blue }
        impl fmt::Display for Color {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self {
                    Color::Red => "red",
                    Color::Blue => "blue",
                })
            }
        }
        struct Name(String);
        impl fmt::Debug for Name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        fn size(plan: Plan) -> usize {
            if true { plan.to_string() } else { String::new() }.len()
        }
        pub fn run() -> String {
            format!("{} {} {} {:?} {}{}", Plan::Basic, Plan::Pro, size(Plan::Pro), Name("a".into()), Color::Red, Color::Blue)
        }
        run()
    "#,
    );
    assert_eq!(value, "Basic Pro 3 \"a\" redblue");
}

#[test]
fn language_boxed_closure_field_as_a_function_value() {
    let value = run(r#"
        struct Input { on_change: Box<dyn Fn(Option<i64>) -> i64> }
        impl Input {
            fn parse(&self, text: &str) -> Option<i64> {
                text.parse().ok().map(Some).map(self.on_change.as_ref())
            }
        }
        pub fn run() -> i64 {
            let input = Input { on_change: Box::new(|value: Option<i64>| value.unwrap_or(0) * 2) };
            let f: &dyn Fn(Option<i64>) -> i64 = input.on_change.as_ref();
            input.parse("21").unwrap() + f(Some(100))
        }
        run()
    "#);
    assert_eq!(value, 242);
}

#[test]
fn language_closure_parameter_typed_by_a_later_call() {
    let value = run_string(
        r#"
        fn scaled(value: f32, by: f32) -> f32 { value * by }
        fn count(n: u64) -> u64 { n + 1 }
        pub fn run() -> String {
            let label = |name, value, n| {
                let text = format!("{name}: {:.2} {} {:?}", value, n, value);
                scaled(value, 2.0);
                count(n);
                text
            };
            let name: &str = "size";
            label(name, 1.5, 7)
        }
        run()
    "#,
    );
    assert_eq!(value, "size: 1.50 7 1.5");
    let value = run_string(
        r#"
        pub fn run() -> String {
            let show = |v| format!("<{}>", v);
            show(5u8)
        }
        run()
    "#,
    );
    assert_eq!(value, "<5>");
    rejected("pub fn run() -> i64 { let show = |v| format!(\"{}\", v); 0 } run()");
}

#[test]
fn language_closure_mutates_through_self() {
    let value = run(r#"
        struct Grid { level: i64 }
        impl Grid {
            fn topple(&mut self) -> bool { self.level -= 1; self.level > 0 }
        }
        struct App { grid: Grid, speed: i64, ticks: i64 }
        impl App {
            fn tick(&mut self) -> Option<i64> {
                let topples = (0..self.speed).find(|_| !self.grid.topple());
                (0..2).for_each(|_| self.ticks += 1);
                topples
            }
            fn count(&mut self) -> i64 {
                (0..3).filter(|_| { self.ticks += 10; true }).count() as i64
            }
        }
        pub fn run() -> i64 {
            let mut app = App { grid: Grid { level: 3 }, speed: 10, ticks: 0 };
            let found = app.tick().unwrap();
            let counted = app.count();
            found * 1000 + app.grid.level * 100 + app.ticks + counted * 10000
        }
        run()
    "#);
    assert_eq!(value, 30000 + 2000 + 32);
}

#[test]
fn language_items_declared_in_a_function_body() {
    let value = run_string(
        r#"
        use std::fmt;
        fn square(size: i64) -> String {
            #[derive(Debug, Clone, Copy)]
            struct Square { size: i64 }

            impl Square {
                fn area(&self) -> i64 { self.size * self.size }
            }

            impl fmt::Display for Square {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, "square {}", self.area())
                }
            }

            enum Shade { Light, Dark }

            let shade = if size > 2 { Shade::Dark } else { Shade::Light };
            let name = match shade { Shade::Light => "light", Shade::Dark => "dark" };
            format!("{} {}", Square { size }, name)
        }
        pub fn run() -> String {
            square(3)
        }
        run()
    "#,
    );
    assert_eq!(value, "square 9 dark");
}

#[test]
fn language_matches_inside_an_if_inside_some() {
    let value = run(r#"
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Message { Next, Previous }
        fn pick(message: Message) -> Option<i64> {
            let mut turns = 0;
            while matches!(message, Message::Next) && turns < 3 { turns += 1; }
            Some(if matches!(message, Message::Next) { 1 } else { 2 } + turns * 100)
        }
        pub fn run() -> i64 {
            pick(Message::Next).unwrap() * 10 + pick(Message::Previous).unwrap()
        }
        run()
    "#);
    assert_eq!(value, 3012);
}

#[test]
fn language_derives_over_function_pointer_fields() {
    let value = run(r#"
        fn seven() -> i64 { 7 }
        #[derive(Debug, Clone, Copy, Eq)]
        struct Example { title: &'static str, view: fn() -> i64 }
        impl PartialEq for Example {
            fn eq(&self, other: &Self) -> bool { self.title == other.title }
        }
        #[derive(Debug)]
        struct Layout { example: Example }
        pub fn run() -> i64 {
            let example = Example { title: "a", view: seven };
            let copied = example;
            let layout = Layout { example };
            let _ = copied.title;
            let text = format!("{:?}", layout);
            let same = if layout.example == layout.example { 100 } else { 0 };
            (layout.example.view)() + same + if text.starts_with("Layout") { 1000 } else { 0 }
        }
        run()
    "#);
    assert_eq!(value, 1107);
    rejected("#[derive(Clone, Copy)] struct S { s: String } pub fn run() -> i64 { 0 } run()");
}

#[test]
fn language_builtin_to_string_as_a_value_takes_a_reference() {
    let value = run_string(
        r#"
        pub fn run() -> String {
            let value: Option<i32> = Some(42);
            let shown = value.as_ref().map(i32::to_string).unwrap_or_default();
            let all: Vec<String> = vec![1i64, 2].iter().map(i64::to_string).collect();
            format!("{shown} {}", all.join(","))
        }
        run()
    "#,
    );
    assert_eq!(value, "42 1,2");
}

#[test]
fn language_eq_and_ne_methods_on_builtins() {
    let value = run(r#"
        struct Item { name: String, size: i64 }
        impl PartialEq for Item {
            fn eq(&self, other: &Self) -> bool { self.name.eq(&other.name) }
        }
        pub fn run() -> i64 {
            let a = Item { name: "a".to_string(), size: 1 };
            let b = Item { name: "a".to_string(), size: 2 };
            let same = if a == b { 1 } else { 0 };
            let ne = if a.size.ne(&b.size) { 10 } else { 0 };
            let eq = if 'x'.eq(&'x') { 100 } else { 0 };
            same + ne + eq
        }
        run()
    "#);
    assert_eq!(value, 111);
}

#[test]
fn language_cfg_on_statements_and_cfg_macro() {
    let value = run(r#"
        pub fn run() -> i64 {
            let mut n = 0;
            #[cfg(target_arch = "wasm32")]
            {
                n += 100;
            }
            #[cfg(not(target_arch = "wasm32"))]
            n += 1;
            #[cfg(all(unix, not(windows)))]
            n += 10;
            #[cfg(feature = "anything")]
            if n > 0 { n += 1000; }
            if cfg!(target_arch = "wasm32") { n += 10000; }
            if cfg!(any(unix, windows)) { n += 20; }
            n
        }
        run()
    "#);
    assert_eq!(value, 31);
}

#[test]
fn language_phantom_data_is_a_field_of_no_value() {
    let value = run(r#"
        use std::marker::PhantomData;
        #[derive(Default, Clone, Copy)]
        struct Picker<C> { space: PhantomData<C> }
        #[derive(Clone, Copy)]
        struct Rgb;
        pub fn run() -> i64 {
            let a: Picker<Rgb> = Picker::default();
            let b: Picker<Rgb> = Picker { space: PhantomData };
            let c: std::marker::PhantomData<i64> = PhantomData;
            let _ = (a, b.space, c);
            7
        }
        run()
    "#);
    assert_eq!(value, 7);
}


#[test]
fn language_a_map_entry_held_in_a_local_writes_the_map() {
    let value = run(r#"
        use std::collections::HashMap;
        struct Grid { sand: HashMap<i64, u32> }
        impl Grid {
            fn add(&mut self, cell: i64, amount: u32) {
                let grains = self.sand.entry(cell).or_default();
                *grains += amount;
            }
            fn topple(&mut self, cell: i64) -> u32 {
                let grains = self.sand.entry(cell).or_default();
                let amount = *grains / 4;
                *grains %= 4;
                amount
            }
        }
        pub fn run() -> i64 {
            let mut grid = Grid { sand: HashMap::new() };
            grid.add(1, 9);
            grid.add(1, 2);
            let moved = grid.topple(1);
            let mut local: HashMap<i64, i64> = HashMap::new();
            let slot = local.entry(5).or_insert(10);
            *slot += 1;
            (grid.sand[&1] as i64) * 1000 + (moved as i64) * 100 + local[&5]
        }
        run()
    "#);
    assert_eq!(value, 3211);
}

#[test]
fn language_ref_cell_borrows_and_changes_through_a_shared_reference() {
    let value = run(r#"
        use std::cell::RefCell;
        struct Counter { hits: RefCell<Vec<i64>> }
        impl Counter {
            fn hit(&self, n: i64) {
                let mut hits = self.hits.borrow_mut();
                hits.push(n);
                if hits.len() > 2 {
                    hits.remove(0);
                }
            }
            fn total(&self) -> i64 {
                self.hits.borrow().iter().sum()
            }
        }
        pub fn run() -> i64 {
            let counter = Counter { hits: RefCell::new(Vec::new()) };
            counter.hit(1);
            counter.hit(20);
            counter.hit(300);
            counter.hits.borrow_mut().push(4000);
            let cell = RefCell::new(5);
            *cell.borrow_mut() += 1;
            let old = cell.replace(7);
            counter.total() * 100 + old * 10 + cell.into_inner()
        }
        run()
    "#);
    assert_eq!(value, 432000 + 60 + 7);
}

#[test]
fn language_ref_cell_panics_on_a_conflicting_borrow() {
    let error = Engine::new()
        .eval::<i64>(
            r#"
            use std::cell::RefCell;
            pub fn run() -> i64 {
                let cell = RefCell::new(1);
                let first = cell.borrow_mut();
                let second = cell.borrow();
                *second
            }
            run()
        "#,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("already mutably borrowed"), "{error}");
    // A borrow ends with its block, as Rust's does.
    let value = run(r#"
        use std::cell::RefCell;
        pub fn run() -> i64 {
            let cell = RefCell::new(1);
            {
                let mut first = cell.borrow_mut();
                *first += 1;
            }
            let second = cell.borrow();
            *second
        }
        run()
    "#);
    assert_eq!(value, 2);
}
