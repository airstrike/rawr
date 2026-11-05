//! `rawr test`: a file's `#[test]` functions built with `cfg(test)` and
//! reported as `cargo test` reports them (#898).

use std::fs;

use super::{assert_success, stderr, stdout, Sandbox};

const TESTED: &str = r#"fn main() {
    println!("{}", add(1, 2));
}

fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        println!("hidden");
        assert_eq!(add(2, 2), 4);
    }

    #[test]
    fn fails() {
        println!("shown");
        assert_eq!(add(1, 1), 3);
    }

    #[test]
    #[should_panic]
    fn panics() {
        panic!("boom");
    }

    #[test]
    #[should_panic(expected = "zero")]
    fn wrong_message() {
        panic!("boom");
    }

    #[test]
    #[should_panic]
    fn no_panic() {}

    #[test]
    #[ignore = "slow"]
    fn later() {}

    #[test]
    fn errs() -> Result<(), String> {
        Err("bad".to_string())
    }
}
"#;

/// libtest's report of `TESTED`, run with `--test-threads=1`, with the
/// time and the panic's thread id taken out.
const REPORT: &str = r#"
running 7 tests
test tests::adds ... ok
test tests::errs ... FAILED
test tests::fails ... FAILED
test tests::later ... ignored, slow
test tests::no_panic - should panic ... FAILED
test tests::panics - should panic ... ok
test tests::wrong_message - should panic ... FAILED

failures:

---- tests::errs stdout ----
Error: "bad"

---- tests::fails stdout ----
shown

thread 'tests::fails' panicked at tested.rs:22:9:
assertion `left == right` failed
  left: 2
 right: 3
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- tests::no_panic stdout ----
note: test did not panic as expected at tested.rs:39:8
---- tests::wrong_message stdout ----

thread 'tests::wrong_message' panicked at tested.rs:34:9:
boom
note: panic did not contain expected string
      panic message: "boom"
 expected substring: "zero"

failures:
    tests::errs
    tests::fails
    tests::no_panic
    tests::wrong_message

test result: FAILED. 2 passed; 4 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

"#;

/// `report` with its time, `finished in N.NNs`, as `0.00s`.
fn timeless(report: &str) -> String {
    match report.find("finished in ") {
        Some(at) => {
            let start = at + "finished in ".len();
            let end = start + report[start..].find('s').unwrap();
            format!("{}0.00{}", &report[..start], &report[end..])
        }
        None => report.to_owned(),
    }
}

#[test]
fn rawr_test_runs_each_test_and_reports_as_cargo_test() {
    let sandbox = Sandbox::new("test-report");
    fs::write(sandbox.work().join("tested.rs"), TESTED).unwrap();

    let tested = sandbox.rawr(&["test", "tested.rs"]);
    let run = sandbox.rawr(&["run", "tested.rs"]);

    assert_eq!(timeless(&stdout(&tested)), REPORT, "{}", stderr(&tested));
    assert_eq!(tested.status.code(), Some(101));
    // `cargo run` builds without `cfg(test)`: the tests are not there.
    assert_success(&run);
    assert_eq!(stdout(&run), "3\n");
}

#[test]
fn rawr_test_takes_cargo_tests_filters() {
    let sandbox = Sandbox::new("test-filters");
    fs::write(sandbox.work().join("tested.rs"), TESTED).unwrap();

    let named = sandbox.rawr(&["test", "tested.rs", "add"]);
    let skipped = sandbox.rawr(&[
        "test",
        "tested.rs",
        "--",
        "l",
        "--skip",
        "fails",
        "--include-ignored",
    ]);
    let exact = sandbox.rawr(&["test", "tested.rs", "--", "--exact", "add"]);
    let ignored = sandbox.rawr(&["test", "tested.rs", "--", "--ignored"]);

    assert_success(&named);
    assert_eq!(
        timeless(&stdout(&named)),
        "\nrunning 1 test\ntest tests::adds ... ok\n\ntest result: ok. 1 passed; 0 failed; \
         0 ignored; 0 measured; 6 filtered out; finished in 0.00s\n\n"
    );
    assert_success(&skipped);
    assert!(stdout(&skipped).contains("test tests::later ... ok\n"));
    assert!(stdout(&skipped).contains("1 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out"));
    assert_success(&exact);
    assert!(stdout(&exact).contains("running 0 tests\n"));
    assert_success(&ignored);
    assert!(stdout(&ignored).contains("running 1 test\ntest tests::later ... ok\n"));
}

/// rustc parses the items a `#[cfg]` leaves out, so `cargo run` and
/// `cargo check` stop at a syntax error in a test module (rustlings
/// options2).
#[test]
fn a_syntax_error_in_a_test_module_stops_run_and_check() {
    let sandbox = Sandbox::new("test-syntax");
    fs::write(
        sandbox.work().join("broken.rs"),
        "fn main() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn simple_option() {\n        let target = \"rustlings\";\n        let optional_target = Some(target);\n        word = optional_target {\n            assert_eq!(word, target);\n        }\n    }\n}\n",
    )
    .unwrap();

    let run = sandbox.rawr(&["run", "broken.rs"]);
    let checked = sandbox.rawr(&["check", "broken.rs"]);

    for output in [&run, &checked] {
        assert!(!output.status.success());
        assert!(
            stderr(output).contains("error: expected `,`"),
            "{}",
            stderr(output)
        );
        assert!(
            stderr(output).contains("broken.rs:10:22"),
            "{}",
            stderr(output)
        );
    }
}
