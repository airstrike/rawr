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
    // Messages animate an app's view; these check the view they end on.
    let still = args.contains(&"--message").then_some("--still");
    let result = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .arg(root.join(format!("tests/fixtures/{name}.rs")))
        .args(args)
        .args(still)
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
    let show = render_fixture("component_sensor");
    let hi = render_fixture_with("component_sensor", "component_sensor_hi", &["--message", "()"]);
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

/// The RGBA pixel at `(x, y)` of a PNG, and its width and height.
fn pixel(png: &[u8], x: u32, y: u32) -> ([u8; 4], u32, u32) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut rgba).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let at = ((y * info.width + x) * 4) as usize;
    (rgba[at..at + 4].try_into().unwrap(), info.width, info.height)
}

fn near(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 8)
}

#[test]
fn cli_renders_an_inline_svg() {
    // ?play has no files: the svg is text in the snippet.
    let png = render_fixture_with("inline_svg", "inline_svg", &["--size", "160x160"]);
    let (_, width, height) = pixel(&png, 0, 0);
    let (center, ..) = pixel(&png, width / 2, height / 2);
    assert!(near(center, [255, 105, 180, 255]), "hotpink at the center, not {center:?}");
    let (corner, ..) = pixel(&png, 2, 2);
    assert!(!near(corner, center), "the background at a corner");
}

#[test]
fn cli_renders_rgba_pixels_built_in_the_snippet() {
    let png = render_fixture_with("rgba_gradient", "rgba_gradient", &["--size", "64x64"]);
    let (_, width, height) = pixel(&png, 0, 0);
    let at = |x: u32, y: u32| pixel(&png, x * width / 64, y * height / 64).0;
    for ((x, y), expected) in [
        ((1, 1), [4, 4, 160, 255]),
        ((62, 1), [248, 4, 160, 255]),
        ((1, 62), [4, 248, 160, 255]),
        ((62, 62), [248, 248, 160, 255]),
    ] {
        assert!(near(at(x, y), expected), "({x}, {y}) is {:?}", at(x, y));
    }
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
    let cart = text_result(include_str!("../../examples/scripts/cart.rawr"), &[]);
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

/// How many distinct colors a PNG holds.
fn colors(png: &[u8]) -> usize {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut rgba).unwrap();
    let bytes = info.color_type.samples();
    let mut seen: Vec<&[u8]> = rgba[..info.buffer_size()].chunks(bytes).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

#[test]
fn cli_renders_the_playgrounds_iced_examples() {
    // Each is iced's own example, with the size and messages the
    // playground's examples.ts gives it. None draws a blank frame.
    let examples: [(&str, &[&str]); 12] = [
        ("component", &["--message", "Message::NumericInputChanged(Some(1))", "--message", "Message::NumericInputChanged(Some(2))"]),
        ("custom_widget", &["--message", "Message::RadiusChanged(70.0)", "--message", "Message::RadiusChanged(90.0)"]),
        ("modal", &["--message", "Message::ShowModal", "--message", "Message::Email(\"ferris@example.com\".to_string())"]),
        ("lazy", &["--message", "Message::AddItem(\"Rawr\".to_string())", "--message", "Message::ToggleOrder"]),
        ("vectorial_text", &["--message", "Message::AngleChanged(20.0)", "--message", "Message::SizeChanged(50.0)"]),
        ("text", &[]),
        ("scrollable", &[]),
        ("table", &[]),
        (
            "qr_code",
            &["--size", "480x740", "--message", "Message::DataChanged(\"rawr\".to_string())", "--message", "Message::DataChanged(\"https://rawr.airstrike.fyi\".to_string())"],
        ),
        (
            "styling",
            &["--size", "480x640", "--message", "Message::SliderChanged(60.0)", "--message", "Message::CheckboxToggled(true)", "--message", "Message::TogglerToggled(true)"],
        ),
        ("custom_quad", &["--size", "480x660"]),
        ("stopwatch", &[]),
    ];
    for (name, args) in examples {
        let png = render_fixture_with(name, &format!("example_{name}"), args);
        assert!(colors(&png) > 16, "{name} drew a blank frame");
    }
}
