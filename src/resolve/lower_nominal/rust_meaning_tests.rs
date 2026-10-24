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
