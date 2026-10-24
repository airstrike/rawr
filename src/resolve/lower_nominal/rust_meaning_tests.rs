//! Code Rust accepts runs with Rust's meaning (D494.6): each test's
//! expected text is what the same program prints when rustc builds it in
//! debug, where overflow panics.

use crate::Engine;

/// The `String` a program evaluates to.
fn text(source: &str) -> String {
    Engine::new()
        .eval::<String>(source)
        .unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn language_rust_meaning_float_comparison_is_ieee() {
    let value = text(
        r#"
        let nan = 0.0_f64 / 0.0;
        format!(
            "{} {} {} {} {} {} {} {} {} {:?} {} {}",
            0.0 < 1e-16,
            1e-300 > 0.0,
            0.1 + 0.2 == 0.3,
            nan == nan,
            nan != nan,
            nan < 1.0,
            nan >= 1.0,
            -0.0 == 0.0,
            5e-324 > 0.0,
            1.0_f64.partial_cmp(&nan),
            1.0_f64.max(nan),
            (0.1 + 0.2) != 0.3,
        )
        "#,
    );
    assert_eq!(value, "true true false false true false false true true None 1 true");
}

#[test]
fn language_rust_meaning_f32_holds_f32_values() {
    // Literals and arithmetic whose `{float}` becomes an `f32` hold the
    // nearest `f32`, so exact comparison agrees with Rust's.
    let value = text(
        r#"
        let a = 0.1_f32 + 0.2_f32;
        let b: f32 = 1.0 / 3.0;
        let x: f32 = 0.1;
        let v: Vec<f32> = vec![0.1, 0.2];
        let mut acc = 0.0_f32;
        for _ in 0..10 { acc += 0.1; }
        format!(
            "{} {} {} {} {} {} {}",
            a == 0.3_f32,
            b * 3.0 == 1.0,
            (b as f64) == 1.0 / 3.0,
            x as f64,
            v[0] + v[1] == 0.3,
            acc,
            acc == 1.0,
        )
        "#,
    );
    assert_eq!(value, "true true false 0.10000000149011612 true 1.0000001 false");
}

#[test]
fn language_rust_meaning_method_calls_never_reach_a_same_named_fn() {
    // rustc resolves `x.abs()` to `i64::abs` and `s.len()` to `str::len`;
    // a free `fn abs` or `fn len` is reached only by a free call.
    let value = text(
        r#"
        fn abs(x: i64) -> i64 { x + 1000 }
        fn len() -> usize { 99 }
        fn sqrt(x: f64) -> f64 { x + 1000.0 }
        fn sign(x: i64) -> i64 { 55 }
        fn to_string() -> String { String::from("mine") }
        fn push(x: i64) {}
        fn contains(x: i64) -> bool { true }
        let a: i64 = -5;
        let s = String::from("hey");
        let mut v = vec![1];
        v.push(2);
        format!(
            "{} {} {} {} {} {} {:?} {} {} {}",
            a.abs(),
            abs(a),
            s.len(),
            len(),
            16.0_f64.sqrt(),
            (-3_i64).signum(),
            v,
            v.contains(&7),
            5.to_string(),
            to_string(),
        )
        "#,
    );
    assert_eq!(value, "5 995 3 99 4 -1 [1, 2] false 5 mine");
}

#[test]
fn language_rust_meaning_step_by_steps() {
    // Walking every item of these ranges takes millions of operations;
    // stepping takes a few, as Rust's `StepBy` of a range does.
    let mut engine = Engine::new();
    engine.set_max_operations(20_000);
    let value = engine
        .eval::<String>(
            r#"
            let a = vec![10, 20, 30, 40, 50];
            let b: Vec<i64> = a.iter().step_by(2).map(|x| *x + 1).collect();
            let c: Vec<i64> = (5..).step_by(10).take(3).collect();
            let mut s = 0;
            for i in (0..1_000_000_000_000i64).step_by(100_000_000_000) { s += i; }
            format!(
                "{:?} {:?} {} {:?} {} {:?} {:?} {} {} {:?}",
                (0..10).step_by(3).collect::<Vec<i64>>(),
                (1..=10).step_by(3).collect::<Vec<i64>>(),
                (0..i64::MAX).step_by(1 << 61).count(),
                (i64::MAX - 5..=i64::MAX).step_by(2).collect::<Vec<i64>>(),
                (0..u64::MAX).step_by(1 << 62).count(),
                (u64::MAX - 4..=u64::MAX).step_by(2).collect::<Vec<u64>>(),
                b,
                (0..10_000_000).step_by(1_000_000).len(),
                s,
                c,
            )
            "#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        value,
        "[0, 3, 6, 9] [1, 4, 7, 10] 4 [9223372036854775802, 9223372036854775804, \
         9223372036854775806] 4 [18446744073709551611, 18446744073709551613, \
         18446744073709551615] [11, 31, 51] 10 4500000000000 [5, 15, 25]"
    );
    for source in ["(0..3).step_by(0).count()", "vec![1, 2].iter().step_by(0).count()"] {
        let error = Engine::new().eval::<usize>(source).expect_err("a zero step panics");
        assert!(error.to_string().contains("assertion failed: step != 0"), "{error}");
    }
}

#[test]
fn language_rust_meaning_host_reads_integer_kinds() {
    // A `u64` result is Rust's `u64`, not the `-1` its bits read as.
    let engine = Engine::new();
    assert_eq!(engine.eval::<u64>("-1_i64 as u64").unwrap(), u64::MAX);
    assert_eq!(engine.eval::<u64>("(1_u64 << 63) + 5").unwrap(), (1 << 63) + 5);
    assert_eq!(engine.eval::<usize>("vec![1, 2].len()").unwrap(), 2);
    assert_eq!(engine.eval::<u8>("200_u8").unwrap(), 200);
    assert_eq!(engine.eval::<i32>("-7_i32").unwrap(), -7);
    let ast = engine
        .compile("pub fn big(x: u64) -> u64 { x * 2 } pub fn count(v: Vec<i64>) -> usize { v.len() }")
        .unwrap();
    let mut scope = crate::Scope::new();
    assert_eq!(
        engine.call_fn::<u64>(&mut scope, &ast, "big", (1_i64 << 62,)).unwrap(),
        1 << 63
    );
    assert_eq!(
        engine
            .call_fn::<usize>(&mut scope, &ast, "count", (vec![crate::Dynamic::from(1_i64)],))
            .unwrap(),
        1
    );
}

#[test]
fn language_rust_meaning_byte_literals() {
    let value = text(
        r#"
        let c = 'd';
        let s = b"hey";
        let t: u8 = b'z';
        let kind = match t { b'a'..=b'y' => "early", b'z' => "last", _ => "other" };
        format!("{} {:?} {} {} {} {} {}", c as u8 - b'a', s, b'\n', b'\x7f', b'\\', t, kind)
        "#,
    );
    assert_eq!(value, "3 [104, 101, 121] 10 127 92 122 last");
}

#[test]
fn language_rust_meaning_collect_into_a_tuple_struct_pattern() {
    let value = text(
        r#"
        struct Histogram([usize; 26]);
        impl FromIterator<char> for Histogram {
            fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> Self {
                let mut counts = [0; 26];
                for c in iter {
                    if c.is_ascii_lowercase() {
                        counts[(c as u8 - b'a') as usize] += 1;
                    }
                }
                Histogram(counts)
            }
        }
        let text = "hello world";
        let Histogram(counts) = text.chars().collect();
        let h: Histogram = text.chars().collect();
        let Histogram(again) = h;
        format!("{} {:?}", counts[11], &again[..5])
        "#,
    );
    assert_eq!(value, "3 [0, 0, 0, 1, 1]");
}

#[test]
fn language_rust_meaning_runtime_errors_point_at_the_script() {
    // Line 4, column 13 is `v[3]` in the script; the lowered program the
    // evaluator runs has other lines and columns.
    let script = "fn main_part() {}\nlet v: Vec<i64> = vec![1, 2, 3];\nlet n = 3;\nlet x = v[n] + 1;\nprintln!(\"{x}\");\n";
    let error = Engine::new().run(script).expect_err("the index is out of bounds");
    let position = deepest(&error).position();
    assert_eq!((position.line(), position.position()), (Some(4), Some(9)), "{error}");
    let ast = Engine::new().compile("pub fn get(v: Vec<i64>) -> i64 {\n    v[5]\n}").unwrap();
    let error = Engine::new()
        .call_fn::<i64>(&mut crate::Scope::new(), &ast, "get", (crate::Array::new(),))
        .expect_err("the index is out of bounds");
    assert_eq!(deepest(&error).position().line(), Some(2), "{error}");
}

fn deepest(error: &crate::EvalAltResult) -> &crate::EvalAltResult {
    match error {
        crate::EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => deepest(inner),
        leaf => leaf,
    }
}
