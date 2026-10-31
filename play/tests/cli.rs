//! The `rawr-play` CLI runs a plain program to its output as JSON, and
//! answers an iced or async snippet with its message.

use std::process::Command;

/// Run `source` through the CLI with `args`, the result on stdout.
fn run_stdin(source: &str, args: &[&str]) -> std::process::Output {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .arg("-")
        .args(args)
        .args(["-o", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn text_result(source: &str, args: &[&str]) -> rawr_play::Output {
    let result = run_stdin(source, args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout.first(), Some(&b'{'), "a text result is JSON");
    rawr_play::Output::from_json(&result.stdout).unwrap()
}

#[test]
fn cli_writes_a_plain_programs_output_as_json() {
    let cart = text_result(include_str!("../../examples/scripts/cart.rs"), &[]);
    assert_eq!((cart.text().as_str(), cart.status), ("total: 27\n", 0));
    let expression = text_result("vec![1, 2, 3].iter().sum::<i64>()", &[]);
    assert_eq!(expression.text(), "6\n");
    let failed = text_result("fn main() -> Result<(), String> {\n    eprintln!(\"oops\");\n    Err(\"no\".to_string())\n}\n", &[]);
    assert_eq!(
        (failed.text().as_str(), failed.status),
        ("oops\nError: \"no\"\n", 1)
    );
}

#[test]
fn cli_stops_an_endless_plain_program_at_the_time_limit() {
    let looped = text_result(
        "fn main() {\n    let mut n = 0;\n    while n >= 0 { n = n + 1 - 1; }\n}\n",
        &["--time-ms", "200"],
    );
    assert!(looped.limit && looped.status == 1, "{looped:?}");
}

#[test]
fn cli_reports_a_plain_compile_error_on_stderr() {
    let result = run_stdin("fn main() {\n    let x: i64 = \"s\";\n}\n", &[]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("play.rs:2:"));
}

#[test]
fn cli_answers_iced_and_async_snippets_with_their_message() {
    for (source, reason) in [
        ("column![text(\"hi\"), button(\"go\")]", rawr_play::Unavailable::Iced),
        (
            "use tokio::time::{sleep, Duration};\n\n#[tokio::main]\nasync fn main() {\n    sleep(Duration::from_millis(100)).await;\n    println!(\"done\");\n}\n",
            rawr_play::Unavailable::Async,
        ),
    ] {
        let result = run_stdin(source, &[]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert_eq!(stdout, rawr_play::unavailable_json(reason));
        let text = rawr_play::message::reply(true, stdout.as_bytes(), "");
        assert_eq!(text, reason.message());
    }
}
