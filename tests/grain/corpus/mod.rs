//! Scripts the VM must agree with rawr on.
//!
//! Weighted towards the places a bytecode VM is most likely to drift from a
//! tree walker rather than towards breadth: scope discipline, early exits
//! (`return`, `break`, `continue` and a panic), and the places a value is
//! written through an index, a field or a reference.
//!
//! Every script is typed Rust. Integer locals are annotated `i64` wherever the
//! case is about the encoding rather than about inference, because an
//! integer whose type is only inferred is still lowered through compiler-owned
//! support, and a program holding that support cannot be written to an
//! artifact.

use rawr::INT;

// Only `tests/fuzz.rs` and the `generated` fuzz target use this; the other
// harnesses take the module for its cases.
#[allow(dead_code)]
pub mod generate;

pub struct Case {
    pub name: &'static str,
    pub source: &'static str,
}

/// A host type a script reaches only through functions the host registered.
///
/// A script has no syntax for the type itself, so every use goes through a
/// registered function called by name. A `&mut` first parameter takes the
/// script's variable by reference, so the mutation has to land in it.
/// Held as `INT` rather than `i64` throughout: `only_i32` narrows the script
/// integer, and a host type registered against the wider one would take a type
/// no script under that build can produce.
#[derive(Debug, Clone, Default)]
pub struct Widget {
    pub level: INT,
}

/// The engine both sides of the differential run against.
pub fn engine() -> rawr::Engine {
    let mut engine = rawr::Engine::new();

    engine
        .register_type_with_name::<Widget>("Widget")
        .register_fn("widget", |level: INT| Widget { level })
        // Takes the receiver by reference, so the call writes back.
        .register_fn("bump", |w: &mut Widget| w.level += 1)
        .register_fn("level_of", |w: &mut Widget| w.level)
        // The receiver and an argument, so a named receiver is resolved after
        // the argument and has to be moved back under it.
        .register_fn("raise", |w: &mut Widget, by: INT| w.level += by)
        // Reads only.
        .register_fn("doubled", |w: &mut Widget| w.level * 2)
        // Mutates and *then* fails, returning an error rather than panicking,
        // because a panic in a registered function takes the test process with
        // it.
        .register_fn("bump_then_fail", |w: &mut Widget| -> Result<(), Box<rawr::EvalAltResult>> {
            w.level += 1;
            Err("bump_then_fail".into())
        });

    engine
}

const fn case(name: &'static str, source: &'static str) -> Case {
    Case { name, source }
}

/// Whether a corpus case exercises anything on this build.
///
/// A restriction feature removes the syntax outright — rawr will not parse a
/// closure under `no_closure`, or a float literal under `no_float` — so the
/// case tests nothing here, and both sides agreeing on the parse failure would
/// be an empty agreement rather than a passing one.
///
/// Lives here rather than in one harness because every harness that walks
/// [`CASES`] needs the same answer.
#[must_use]
pub fn applies_to_this_build(name: &str) -> bool {
    #[cfg(feature = "no_closure")]
    if name.starts_with("closure_") {
        return false;
    }
    // `unchecked` removes the arithmetic guards, so `1 / 0` panics inside
    // rawr's own built-in rather than raising — there is no behaviour left for
    // the two sides to agree on, and the case would take the process with it.
    #[cfg(feature = "unchecked")]
    if matches!(name, "error_divide_by_zero" | "error_overflow" | "error_temp_root_index_runs_first") {
        return false;
    }
    // No shared prefix to key on: a float literal is incidental to most of
    // these.
    #[cfg(feature = "no_float")]
    if matches!(name, "float_arithmetic" | "format_of_every_type" | "casts") {
        return false;
    }
    let _ = name;
    true
}

pub const CASES: &[Case] = &[
    // --- values and operators -------------------------------------------
    case("int_arithmetic", "let a: i64 = 7; let b: i64 = 3; a * b - a / b + a % b"),
    case("float_arithmetic", "let a: f64 = 7.5; let b: f64 = 0.5; a * b + a / b"),
    case("comparison_chain", "let a: i64 = 5; a > 1 && a < 10 || a == 5"),
    case("bitwise", "let a: i64 = 0b1010; (a & 0b0110) | (a ^ 0b1111) << 2"),
    case("integer_methods", "let x: i64 = -5; x.abs() + x.max(3) + x.min(3) + x.pow(2)"),
    case("casts", "let a: i64 = 7; let f = a as f64 / 2.0; f as i64 + 1"),
    case("string_ops", r#"let s = "hello"; s.to_string() + " " + "world" + &s.len().to_string()"#),
    case("string_format", r#"let n: i64 = 42; format!("answer is {} and {}", n, n * 2)"#),
    // Every argument type goes through a different arm of the rendering.
    case("format_of_every_type", r#"let s = "x"; let n: i64 = 1; let f: f64 = 1.5; let b = true; let c = 'c'; format!("{}|{}|{}|{}|{}|", s, n, f, b, c)"#),
    case("format_inline_arguments", r#"let a = "0123456789"; format!("{a}{a}{a}")"#),
    case("format_debug_of_containers", r#"let a = vec![1, 2]; let t = (1, "a"); format!("{:?}|{:?}", a, t)"#),
    case("char_ops", "let c = 'a'; c.to_ascii_uppercase()"),
    case("unit_value", "()"),
    // --- containers -------------------------------------------------------
    case("array_literal", "let a: [i64; 3] = [1, 2, 3]; a[0] + a[1] + a[2]"),
    case("vec_literal", "let a: Vec<i64> = vec![1, 2, 3]; a[0] + a[1] + a[2]"),
    case("vec_methods", "let mut a = vec![3, 1, 2]; a.sort(); a.reverse(); a"),
    case("vec_push_pop", "let mut a: Vec<i64> = Vec::new(); a.push(1); a.push(2); a.pop(); a.len()"),
    case("vec_repeat", "let a = vec![0; 3]; a"),
    // All-constant literals are folded before the compiler sees them. These
    // are built at run time.
    case("vec_computed", "let v: i64 = 7; vec![v, 2, v * 2]"),
    case("vec_nested", "let v: i64 = 7; let m = vec![vec![1, 2], vec![v]]; m[1][0]"),
    // An empty literal inside one that is not. It contributes no size check of
    // its own, so it must not consume the enclosing literal's running total.
    case("vec_empty_nested_in_computed", "let v: i64 = 7; let e: Vec<i64> = Vec::new(); vec![e.clone(), vec![v], e.clone()]"),
    case("tuple_fields", r#"let t = (1, "a", true); t.1"#),
    case("tuple_destructure", "let (a, b) = (3, 4); a * b"),
    case("hashmap_insert_get", r#"use std::collections::HashMap; let mut m = HashMap::new(); m.insert("a", 1); m.insert("b", 2); m["a"] + m["b"]"#),
    case("hashmap_get_absent", r#"use std::collections::HashMap; let mut m: HashMap<String, i64> = HashMap::new(); m.insert("a".to_string(), 1); m.get("b").is_none()"#),
    case("struct_fields", "struct P { x: i64, y: i64 } let p = P { x: 1, y: 2 }; p.x + p.y"),
    case("struct_field_assign", "struct P { x: i64 } let mut p = P { x: 1 }; p.x = 7; p.x"),
    case("struct_field_deep", "struct C { c: i64 } struct B { b: C } let mut a = B { b: C { c: 1 } }; a.b.c = 7; a.b.c"),
    // --- control flow -----------------------------------------------------
    case("if_else", r#"let a: i64 = 5; if a > 3 { "big" } else { "small" }"#),
    case("while_loop", "let mut i: i64 = 0; let mut s: i64 = 0; while i < 5 { s += i; i += 1; } s"),
    case("loop_break_value", "let mut i: i64 = 0; loop { i += 1; if i > 4 { break i * 10; } }"),
    case("continue_skips", "let mut s: i64 = 0; for i in 0..10 { if i % 2 == 0 { continue; } s += i; } s"),
    case("for_range", "let mut s: i64 = 0; for i in 0..5 { s += i; } s"),
    case("for_range_inclusive_reversed", "let mut s: i64 = 0; for i in (1..=5).rev() { s = s * 10 + i; } s"),
    case("for_range_step", "let mut s: i64 = 0; for i in (0..10).step_by(3) { s += i; } s"),
    case("for_vec", "let mut s: i64 = 0; for x in vec![10, 20, 30] { s += x; } s"),
    case("for_with_counter", "let mut s: i64 = 0; for (i, x) in vec![10, 20, 30].into_iter().enumerate() { s += x * i as i64; } s"),
    // The loop variable is its own binding, so the outer one of the same name
    // must come back untouched.
    case("for_loop_var_not_leaked", "let x: i64 = 99; for x in 0..3 { } x"),
    case("nested_loops_break", "let mut s: i64 = 0; for i in 0..3 { for j in 0..3 { if j == 2 { break; } s += 1; } } s"),
    case("for_empty_body", "let s: i64 = 0; for i in 0..5 { } s"),
    // `return` out of a `for` skips the exhaustion path, so the iterator and
    // the loop variable have to go with the frame.
    case("for_return_from_body", "fn find(xs: Vec<i64>) -> i64 { for (i, x) in xs.into_iter().enumerate() { if x > 1 { return i as i64; } } -1 } find(vec![1, 2, 3])"),
    // A `break` out of a `while` nested in a `for` must drop nothing, and out
    // of the `for` must drop one — the two are easy to get the wrong way round.
    case("for_around_while_break", "let mut s: i64 = 0; for i in 0..3 { let mut j: i64 = 0; loop { j += 1; if j > 2 { break; } s += 1; } } s"),
    // --- match ------------------------------------------------------------
    case("match_literal", r#"let x: i64 = 2; match x { 1 => "one", 2 => "two", _ => "other" }"#),
    case("match_range", r#"let x: i64 = 42; match x { 0..=9 => "small", 10..=99 => "medium", _ => "large" }"#),
    // A failing guard falls through to the next arm, so both single-digit arms
    // are needed to tell that apart from falling to the default.
    case("match_guard", r#"let x: i64 = 5; match x { 0..=9 if x % 2 == 1 => "odd digit", 0..=9 => "even digit", _ => "big" }"#),
    case("match_declined_guard_reaches_a_later_range", r#"let f = false; let x: i64 = 1; match x { 1 if f => "guarded", 0..=5 => "range", _ => "default" }"#),
    case("match_default_only", r#"let x: i64 = 999; match x { 1 => "a", _ => "fallback" }"#),
    // Two values, one arm.
    case("match_shared_body", r#"let x: i64 = 2; match x { 1 | 2 => "low", 3 => "three", _ => "other" }"#),
    case("match_string", r#"let s = "b"; match s { "a" => 1, "b" => 2, _ => 0 }"#),
    // An arm body is a block: it declares, and it has to leave the scope the
    // depth it found it.
    case("match_block_body_scope", "let x: i64 = 1; let mut y: i64 = 0; match x { 1 => { let z: i64 = 5; y = z * 2 } _ => () } y"),
    // A jump out of an arm and out of the match.
    case("match_break_from_loop", "let mut s: i64 = 0; let mut i: i64 = 0; while i < 10 { match i { 3 => break, _ => () } s += 1; i += 1; } s"),
    case("match_enum", "enum S { A, B(i64), C { v: i64 } } fn f(s: S) -> i64 { match s { S::A => 0, S::B(n) => n, S::C { v } => v * 2 } } f(S::A) + f(S::B(3)) + f(S::C { v: 5 })"),
    case("match_option", "let o: Option<i64> = Some(3); match o { Some(x) => x, None => 0 }"),
    case("if_let", "let o = Some(3); if let Some(x) = o { x } else { 0 }"),
    // --- blocks used for their value ---------------------------------------
    // Each declares inside the block, so a lowering that forgot to rewind
    // would leave the scope a different depth.
    case("let_from_match", r#"let x: i64 = 2; let y = match x { 1 => "one", 2 => "two", _ => "other" }; y"#),
    case("let_from_if", "let c = true; let y = if c { let a: i64 = 1; a } else { let b: i64 = 2; b }; y + 10"),
    case("let_from_block", "let a: i64 = 3; let y = { let z = a; z * 2 }; y"),
    // A block among a call's arguments, where the scope grows while operands
    // are already on the stack.
    case("block_as_argument", "fn add(a: i64, b: i64) -> i64 { a + b } let n: i64 = 2; add({ let t = n; t + 1 }, 10)"),
    // --- scoping ----------------------------------------------------------
    case("shadowing_nested", "let x: i64 = 1; { let x: i64 = 2; { let x: i64 = 3; } } x"),
    case("block_scope_discarded", "let mut x: i64 = 1; { let y: i64 = 2; x += y; } x"),
    case("const_read", "const K: i64 = 10; K * 2"),
    // --- functions --------------------------------------------------------
    case("fn_call", "fn add(a: i64, b: i64) -> i64 { a + b } add(2, 3)"),
    // Kept shallow deliberately: rawr's default call-depth limit is far lower
    // in debug builds than in release, and this case is about recursion working
    // at all, not about the limit.
    case("fn_recursion", "fn fib(n: i64) -> i64 { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } } fib(6)"),
    case("fn_early_return", r#"fn f(x: i64) -> String { if x > 0 { return "pos".to_string(); } "nonpos".to_string() } f(1) + &f(-1)"#),
    case("fn_mutable_reference", "fn double(x: &mut i64) { *x *= 2; } let mut v: i64 = 21; double(&mut v); v"),
    case("fn_shared_reference", "fn total(xs: &Vec<i64>) -> i64 { let mut s: i64 = 0; for x in xs { s += x; } s } let v = vec![1, 2, 3]; total(&v) + v.len() as i64"),
    case("fn_returning_a_tuple", "fn split(n: i64) -> (i64, i64) { (n / 10, n % 10) } let (a, b) = split(42); a * 100 + b"),
    case("fn_generic", "fn id<T>(x: T) -> T { x } id(4)"),
    case("method_mutates_its_receiver", "struct C { n: i64 } impl C { fn bump(&mut self) { self.n += 1; } fn get(&self) -> i64 { self.n } } let mut c = C { n: 1 }; c.bump(); c.bump(); c.get()"),
    case("trait_method", "trait Area { fn area(&self) -> f64; } struct Sq(f64); impl Area for Sq { fn area(&self) -> f64 { self.0 * self.0 } } Sq(3.0).area()"),
    case("trait_default_method", "trait Twice { fn one(&self) -> i64; fn two(&self) -> i64 { self.one() * 2 } } struct N(i64); impl Twice for N { fn one(&self) -> i64 { self.0 } } N(21).two()"),
    case("question_mark", "fn half(n: i64) -> Option<i64> { if n % 2 == 0 { Some(n / 2) } else { None } } fn q(n: i64) -> Option<i64> { let h = half(n)?; Some(h + 1) } q(8).unwrap_or(0) + q(7).unwrap_or(100)"),
    // --- functions the host registered -------------------------------------
    // A registered function is called by name, and a `&mut` first parameter
    // mutates the variable handed to it.
    case("host_call_mutates", "let mut w = widget(4); bump(w); level_of(w)"),
    case("host_call_pure", "let w = widget(4); doubled(w)"),
    case("host_temporary", "level_of(widget(4))"),
    // --- closures ---------------------------------------------------------
    // The closure is kept inside a block in most of these. Not incidental: the
    // pointer the VM builds is late-bound where rawr's is early-bound, so rawr
    // renders it `Fn*+("anon$..")` and the VM renders it `Fn("anon$..")`. That
    // difference is pinned by `a_closure_pointer_is_late_bound`, so these cases
    // test the capture rather than re-testing the rendering.
    case("closure_capture_read", "let n: i64 = 10; let mut r: i64 = 0; { let f = |x: i64| x + n; r = f(5); } r"),
    // Capture is by shared cell, so the mutation must be visible outside.
    case("closure_capture_mutate", "let mut n: i64 = 0; { let mut f = || n += 1; f(); f(); } n"),
    // `move` copies what it captures, so a later write is not seen.
    case("closure_move_copies", "let mut n: i64 = 1; let mut r: i64 = 0; { let f = move |x: i64| x * n; n = 10; r = f(2); } r + n"),
    case("closure_fn_pointer", "fn triple(x: i64) -> i64 { x * 3 } let mut r: i64 = 0; { let f = triple; r = f(4); } r"),
    case("closure_fn_pointer_argument", "fn triple(x: i64) -> i64 { x * 3 } fn apply(f: fn(i64) -> i64, v: i64) -> i64 { f(v) } apply(triple, 4)"),
    case("closure_argument", "fn apply(f: impl Fn(i64) -> i64, v: i64) -> i64 { f(v) } apply(|x| x + 1, 4)"),
    case("closure_in_map", "let a = vec![1, 2, 3]; a.iter().map(|x| x * 2).collect::<Vec<i64>>()"),
    case("closure_in_filter", "let a = vec![1, 2, 3, 4]; a.into_iter().filter(|x| x % 2 == 0).collect::<Vec<i64>>()"),
    // A capture reaching a native that calls the closure back. Subtraction,
    // because only arithmetic that cares about order can tell the captured
    // value and the element apart.
    case("closure_capture_in_map", "let n: i64 = 10; let a = vec![1, 2, 3]; a.iter().map(|x| n - x).collect::<Vec<i64>>()"),
    case("closure_nested_in_map", "let a = vec![vec![1, 2], vec![3, 4]]; a.iter().map(|row| row.iter().map(|x| x + 1).collect::<Vec<i64>>()).collect::<Vec<Vec<i64>>>()"),
    case("closure_named_function_in_map", "fn double(x: i64) -> i64 { x * 2 } let a = vec![1, 2, 3]; a.into_iter().map(double).collect::<Vec<i64>>()"),
    // --- written through an index, a field or a reference -------------------
    case("index_assign_array", "let mut a: Vec<i64> = vec![1, 2, 3]; a[1] = 99; a"),
    case("index_assign_nested", "let mut m = vec![vec![1, 2, 3], vec![4]]; m[0][2] = 42; m"),
    case("op_assign_indexed", "let mut a: Vec<i64> = vec![1, 2, 3]; a[0] += 10; a"),
    case("string_push", "let mut s = String::from(\"hell\"); s.push('o'); s.push_str(\"!\"); s"),
    case("string_slice_read", r#"let s = "hello world"; s[0..5].to_string()"#),
    case("string_slice_inclusive", r#"let s = "hello world"; s[6..=9].to_string()"#),
    // --- chains rooted at something that is not a variable ------------------
    // Rawr evaluates the root into a temporary and walks that, so there is no
    // scope entry behind it and nothing is written back.
    case("temp_root_array_method", "vec![3, 1, 2].len()"),
    case("temp_root_array_index", "vec![10, 20, 30][1]"),
    case("temp_root_string", r#""hello".to_uppercase()"#),
    case("temp_root_call", "fn make() -> Vec<i64> { vec![1, 2, 3] } make().len()"),
    case("temp_root_parenthesised", "let a: i64 = 1; let b: i64 = 2; (a + b).to_string()"),
    case("temp_root_nested", "vec![vec![1, 2], vec![3, 4]][1][0]"),
    // --- errors -----------------------------------------------------------
    // Compared by variant and position, so a VM that reports the right failure
    // at the wrong place still fails the test.
    case("error_divide_by_zero", "let a: i64 = 1; let b: i64 = 0; a / b"),
    case("error_overflow", "let a: i64 = i64::MAX; a + 1"),
    case("error_array_bounds", "let a = vec![1, 2]; a[10]"),
    case("error_temp_root_out_of_bounds", "vec![1, 2, 3][99]"),
    // Rust evaluates what is indexed before the index, so the first division
    // is the one that fails.
    case("error_temp_root_index_runs_first", "let z: i64 = 0; vec![1 / z][(9 / z) as usize]"),
    case("error_string_slice_out_of_bounds", r#"let s = "abc"; s[0..9].to_string()"#),
    case("error_unwrap_none", "let o: Option<i64> = None; o.unwrap()"),
    case("error_unwrap_of_a_parse_failure", r#"let s = String::from("zz"); s.parse::<i64>().unwrap()"#),
    case("error_panic", r#"panic!("boom")"#),
    case("error_panic_in_fn", r#"fn f() -> i64 { panic!("boom") } f()"#),
    case("error_returned_by_a_native", "let mut w = widget(1); bump_then_fail(w)"),
    case("error_in_a_callback", r#"let a = vec![1, 2, 3]; a.iter().map(|x| -> i64 { panic!("boom {}", x) }).collect::<Vec<i64>>()"#),
    // --- what an escaping error leaves in the scope -------------------------
    // Rawr rewinds a block whether it is left normally or by a panic, and
    // rewinds nothing at the top level. The comparison that matters in all of
    // these is the leftover scope rather than the error.
    case("error_panic_from_a_block_rewinds_it", r#"let a: i64 = 1; { let b: i64 = 2; panic!("3"); }"#),
    case("error_panic_from_a_for_body_drops_the_loop_var", r#"let a: i64 = 1; for i in 0..3 { panic!("{}", i); }"#),
    case("error_panic_from_a_while_body_drops_its_locals", r#"let a: i64 = 1; let n: i64 = 0; while n < 3 { let b = n; panic!("{}", b); }"#),
    case("error_panic_from_a_nested_block_drops_every_level", r#"let a: i64 = 1; { let b: i64 = 2; { let c: i64 = 3; for i in 0..2 { panic!("{}", i); } } }"#),
    // The frame boundary: a function's own locals go with its scope, and the
    // caller's top-level ones stay.
    case("error_panic_from_a_function_leaves_the_caller_top_level_alone", r#"fn boom() { let inner: i64 = 9; panic!("{}", inner); } let a: i64 = 1; { let b: i64 = 2; boom(); }"#),
    // Nothing to rewind, which is the case a floor set too low would break.
    case("error_panic_at_the_top_level_keeps_what_ran", r#"let a: i64 = 1; let b: i64 = 2; panic!("3");"#),
];
