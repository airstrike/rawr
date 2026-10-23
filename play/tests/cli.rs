//! The `rawr-play` CLI renders an iced file to a PNG, and runs a plain
//! program to its output as JSON.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn cli_writes_a_png() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/play/cli-test");
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("hello.rs");
    let output = root.join("hello.png");
    let _ = std::fs::remove_file(&output);
    std::fs::write(
        &input,
        "column![text(\"Hello from ?play\").size(24), button(\"go\")].spacing(12).padding(16)\n",
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .arg(&input)
        .args(["--size", "320x160", "--theme", "dark", "-o"])
        .arg(&output)
        .status()
        .unwrap();

    assert!(status.success());
    let bytes = std::fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
}

/// Render `tests/fixtures/{name}.rs`, an iced example pasted verbatim, to
/// `target/play/cli-test/{name}.png`, and return the PNG's bytes.
fn render_fixture(name: &str) -> Vec<u8> {
    render_fixture_with(name, name, &[])
}

/// [`render_fixture`] with extra CLI arguments, written to `{output}.png`.
fn render_fixture_with(name: &str, output: &str, args: &[&str]) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = root.join(format!("../target/play/cli-test/{output}.png"));
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&output);
    let result = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .arg(root.join(format!("tests/fixtures/{name}.rs")))
        .args(args)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let bytes = std::fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    bytes
}

#[test]
fn cli_renders_iced_checkbox_example() {
    render_fixture("checkbox");
}

#[test]
fn cli_renders_iced_slider_example() {
    render_fixture("slider");
}

#[test]
fn cli_renders_iced_progress_bar_example() {
    let empty = render_fixture("progress_bar");
    // A transition shows its value settled; the closure reads `self`.
    let vertical = render_fixture_with(
        "progress_bar",
        "progress_bar_vertical",
        &["--message", "Message::SliderChanged(40.0)", "--message", "Message::ToggleVertical(true)"],
    );
    assert_ne!(empty, vertical);
}

#[test]
fn cli_renders_iced_tooltip_example() {
    render_fixture("tooltip");
    // `update` matches iced's `tooltip::Position` through every variant.
    let mut args = ["--message", "Message::ChangePosition"].repeat(5);
    args.extend(["--message", "Message::ToggleDelay(true)"]);
    render_fixture_with("tooltip", "tooltip_changed", &args);
}

#[test]
fn cli_renders_iced_combo_box_example() {
    // `Language::ALL: [Language; 9]`, and a `combo_box::State` the app owns,
    // its options labelled by the script's `Display`.
    let empty = render_fixture("combo_box");
    let selected = render_fixture_with(
        "combo_box",
        "combo_box_selected",
        &["--message", "Message::Selected(Language::Other)"],
    );
    assert_ne!(empty, selected);
}

#[test]
fn cli_renders_iced_counter_example_after_messages() {
    let zero = render_fixture("counter");
    let two = render_fixture_with(
        "counter",
        "counter_two",
        &["--message", "Message::Increment", "--message", "Message::Increment"],
    );
    assert_ne!(zero, two);
}

#[test]
fn cli_renders_master_component_after_its_message() {
    // "hi" shows frames after `()`: the component appears, its sensor fires,
    // and its own `update` takes that Event.
    let show = render_fixture("component");
    let hi = render_fixture_with("component", "component_hi", &["--message", "()"]);
    assert_ne!(show, hi);
}

#[test]
fn cli_renders_a_column_built_from_a_vec() {
    // Two snippets from the iced Discord: a `Vec<Text>` pushed in a loop,
    // made a column by `Column::from_vec` and by the `column` function.
    let from_vec = render_fixture("column_from_vec");
    let function = render_fixture("column_fn");
    assert_eq!(from_vec, function);
}

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
    child.stdin.take().unwrap().write_all(source.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn text_result(source: &str, args: &[&str]) -> rawr_play::Output {
    let result = run_stdin(source, args);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
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
    assert_eq!((failed.text().as_str(), failed.status), ("oops\nError: \"no\"\n", 1));
}

#[test]
fn cli_stops_an_endless_plain_program_at_the_time_limit() {
    let looped = text_result("fn main() {\n    let mut n = 0;\n    while n >= 0 { n = n + 1 - 1; }\n}\n", &["--time-ms", "200"]);
    assert!(looped.limit && looped.status == 1, "{looped:?}");
}

#[test]
fn cli_still_writes_an_image_for_a_prelude_only_snippet() {
    let result = run_stdin("column![text(\"hi\"), button(\"go\")]", &["--format", "png"]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(result.stdout.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn cli_reports_a_plain_compile_error_on_stderr() {
    let result = run_stdin("fn main() {\n    let x: i64 = \"s\";\n}\n", &[]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("play.rs:2:"));
}

#[test]
fn cli_renders_a_snippet_using_any_widget_helper() {
    render_fixture("center");
}

#[test]
fn cli_names_an_unknown_function_and_quotes_only_what_was_written() {
    let source = include_str!("fixtures/unknown_function.rs");
    let result = run_stdin(source, &["--format", "png"]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("unknown function `centre`"), "{stderr}");
    // The quoted line is the snippet as written, nothing the wrapping added.
    let quoted = stderr
        .lines()
        .find_map(|line| line.strip_prefix(" 1 | "))
        .unwrap_or_else(|| panic!("{stderr}"));
    assert_eq!(quoted, source.trim_end());
    // A facade helper of the name says where it is.
    let result = run_stdin("use iced::widget::text;\nfn view() -> impl iced::Widget<()> { center(text(\"a\")) }\n", &["--format", "png"]);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("use iced::widget::center;"), "{stderr}");
}
