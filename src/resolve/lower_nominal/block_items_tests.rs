//! Items declared in a block, as rustc scopes them (#1631, #1429): each
//! is named throughout its block, before its declaration too, and by the
//! items nested there, and nowhere outside it. Two blocks' items of one
//! name are two items.

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
fn language_two_bodies_declare_items_of_one_name() {
    // Each body's `MAX` and `Point` are its own, of their own shapes, and
    // `{:?}` prints the struct's own name. rustc prints
    // `Point { x: 3 } 3 | Point(1, 2) max`.
    let output = printed(
        r#"
fn first() -> String {
    const MAX: u32 = 3;
    #[derive(Debug)]
    struct Point {
        x: u32,
    }
    format!("{:?} {}", Point { x: MAX }, MAX)
}

fn second() -> String {
    const MAX: &str = "max";
    #[derive(Debug)]
    struct Point(i64, i64);
    format!("{:?} {}", Point(1, 2), MAX)
}

fn main() {
    println!("{} | {}", first(), second());
}
main();
"#,
    );
    assert_eq!(output, "Point { x: 3 } 3 | Point(1, 2) max\n");
}

#[test]
fn language_nested_items_read_their_block_s_const() {
    // A nested `fn` and a nested `impl` name the block's constants, one of
    // them declared after the `fn`, and the body names the `fn` and a
    // constant before their declarations. rustc prints `18 2`.
    let output = printed(
        r#"
struct Kind;

fn main() {
    let total = add(LIMIT);
    const LIMIT: i32 = 4;
    fn add(n: i32) -> i32 {
        n + LIMIT + offset()
    }
    fn offset() -> i32 {
        OFFSET
    }
    const OFFSET: i32 = 10;
    const VARIANTS: &[&str] = &["a", "b"];
    impl Kind {
        fn count(&self) -> usize {
            VARIANTS.len()
        }
    }
    println!("{total} {}", Kind.count());
}
main();
"#,
    );
    assert_eq!(output, "18 2\n");
}

#[test]
fn language_block_items_shadow_outer_ones() {
    // An inner block's `N` shadows its body's, which shadows the file's;
    // a file item still names the file's `N`, and two blocks' `use`s bind
    // one name to two functions. rustc prints `3 2 1 | 2 1`.
    let output = printed(
        r#"
const N: i32 = 1;

fn outer() -> i32 {
    N
}

fn main() {
    const N: i32 = 2;
    let inner = {
        const N: i32 = 3;
        N
    };
    let a = {
        use std::cmp::max as pick;
        pick(1, 2)
    };
    let b = {
        use std::cmp::min as pick;
        pick(1, 2)
    };
    println!("{inner} {N} {} | {a} {b}", outer());
}
main();
"#,
    );
    assert_eq!(output, "3 2 1 | 2 1\n");
}

#[test]
fn language_block_items_are_unseen_outside_their_block() {
    // rustc: error[E0425]: cannot find function `helper` in this scope.
    let report = refused(
        r#"
fn a() -> i32 {
    fn helper() -> i32 {
        1
    }
    helper()
}

fn main() {
    println!("{}", a() + helper());
}
main();
"#,
    );
    assert!(
        report.starts_with("error[E0425]: cannot find function `helper` in this scope"),
        "{report}"
    );
}

#[test]
fn language_two_bodies_declare_types_with_methods_of_one_name() {
    // Each body's `Counter` has its own `new` and `step`, and each body's
    // `helper` its own closure. rustc prints `2 4 | 30 6`.
    let output = printed(
        r#"
fn first() -> String {
    struct Counter {
        n: i32,
    }
    impl Counter {
        fn new() -> Self {
            Counter { n: 1 }
        }
        fn step(&self) -> i32 {
            self.n + 1
        }
    }
    fn helper(v: i32) -> i32 {
        let f = |x: i32| x * 2;
        f(v)
    }
    format!("{} {}", Counter::new().step(), helper(2))
}

fn second() -> String {
    struct Counter(i32);
    impl Counter {
        fn new() -> Self {
            Counter(10)
        }
        fn step(&self) -> i32 {
            self.0 * 3
        }
    }
    fn helper(v: i32) -> i32 {
        let f = |x: i32| x + 1;
        f(v)
    }
    format!("{} {}", Counter::new().step(), helper(5))
}

fn main() {
    println!("{} | {}", first(), second());
}
main();
"#,
    );
    assert_eq!(output, "2 4 | 30 6\n");
}

#[test]
fn language_block_items_of_a_unit_compiled_alone() {
    // A unit `eval` compiles alone resolves its blocks' items as a
    // package's: a nested `fn`'s signature names its block's struct and
    // alias, and its body a constant declared after it. rustc prints
    // `12 7`.
    let value = Engine::new()
        .eval::<String>(
            r#"
fn area() -> i64 {
    struct Rect {
        w: i64,
        h: i64,
    }
    fn of(r: &Rect) -> i64 {
        r.w * r.h * SCALE
    }
    const SCALE: i64 = 2;
    of(&Rect { w: 2, h: 3 })
}

fn size() -> i64 {
    struct Rect(i64);
    type Side = i64;
    fn of(r: Rect) -> Side {
        r.0
    }
    of(Rect(7))
}

pub fn run() -> String {
    format!("{} {}", area(), size())
}
run()
"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value, "12 7");
}
