//! The `rawr-play` CLI renders a file to a PNG.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn cli_writes_a_png() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/play/cli-test");
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("hello.rawr");
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
