//! `rawr-play` on the universe runner (#434): an iced program answers with
//! the text it printed and what it drew: the PNG of its settled frame at
//! its own scale (#528, #529), or a GIF when it animated (#527). A tokio
//! program answers with its text. Several snippets run in turn on one
//! server, prepared once (#515).
//!
//! They need the runner built first, so they are ignored by default:
//!
//! ```sh
//! RAWR_UNIVERSE_RUNNER=1 rawr-import run play/universe
//! cargo test -p rawr-play --test universe -- --ignored
//! ```

use std::path::PathBuf;
use std::process::{Command, Stdio};

const NEEDS_RUNNER: &str = "needs the runner: RAWR_UNIVERSE_RUNNER=1 rawr-import run play/universe";

/// Run `source` through the CLI on the universe runner.
fn run(source: &str) -> std::process::Output {
    use std::io::Write as _;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("universe");
    let runner = root.join("target/rawr/runner/target/debug/play");
    assert!(runner.is_file(), "{NEEDS_RUNNER}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .args(["-", "--time-ms", "30000", "-o", "-"])
        .env(rawr_play::universe::ROOT, &root)
        .env(rawr_play::universe::RUNNER, &runner)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// iced's counter, as its example writes it, with a subscription that
/// counts to five a frame at a time; [`COUNTER`] and the others add a
/// `main`.
const COUNTER_TYPES: &str = r#"use iced::widget::{button, column, text, Widget};
use iced::{window, Center, Subscription};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
        .align_x(Center)
    }

    fn scale_factor(&self) -> f32 {
        2.0
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.value < 5 {
            window::frames().map(|_| Message::Increment)
        } else {
            Subscription::none()
        }
    }
}
"#;

/// iced's counter.
fn counter() -> String {
    format!(
        "{COUNTER_TYPES}
pub fn main() -> iced::Result {{
    iced::run(Counter::update, Counter::view)
}}
"
    )
}

/// The width and height in a PNG's header.
fn png_size(png: &[u8]) -> (u32, u32) {
    let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    (size(16), size(20))
}

/// What a snippet answered: its output, with the frame it drew.
fn output(stdout: &[u8]) -> rawr_play::Output {
    rawr_play::Output::from_json(stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(stdout)))
}

#[test]
#[ignore = "needs the universe runner"]
fn an_iced_counter_answers_with_its_text_and_its_frame_as_a_png() {
    let source = counter().replace(
        "pub fn main() -> iced::Result {\n",
        "pub fn main() -> iced::Result {\n    println!(\"counting from 0\");\n",
    );
    let stdout = run(&source).stdout;
    let output = output(&stdout);
    assert_eq!(
        (output.text().as_str(), output.status),
        ("counting from 0\n", 0)
    );
    let png = output.frame.expect("the counter drew a frame");
    assert_eq!(rawr_play::plain::image_kind(&png), Some("png"));
    // iced's default window, 1024 by 768.
    assert_eq!(png_size(&png), (1024, 768));
    // Two buttons and a number on a plain background: more than an empty
    // frame, far less than a raw one.
    assert!(
        (2_000..1_000_000).contains(&png.len()),
        "{} bytes",
        png.len()
    );
    // The bot sends both.
    let reply = rawr_play::message::reply(true, &stdout, "");
    assert_eq!(reply.text, "```text\ncounting from 0\n```");
    assert!(reply.frame.is_some());
}

/// A program at `scale_factor` 2 opens a window twice its size, as
/// iced_winit opens it, and draws twice as large (#529).
#[test]
#[ignore = "needs the universe runner"]
fn an_iced_program_is_drawn_at_its_own_scale_factor() {
    let source = format!(
        "{COUNTER_TYPES}
pub fn main() -> iced::Result {{
    iced::application(Counter::default, Counter::update, Counter::view)
        .scale_factor(Counter::scale_factor)
        .run()
}}
"
    );
    let output = output(&run(&source).stdout);
    let png = output.frame.expect("the counter drew a frame");
    assert_eq!(png_size(&png), (2048, 1536));
}

/// A program that changes on its own frames answers with a GIF of each
/// change, which plays once and ends on the frame it settled on (#527).
#[test]
#[ignore = "needs the universe runner"]
fn an_animated_iced_program_answers_with_a_gif_that_ends_where_it_settled() {
    let source = format!(
        "{COUNTER_TYPES}
pub fn main() -> iced::Result {{
    iced::application(Counter::default, Counter::update, Counter::view)
        .subscription(Counter::subscription)
        .run()
}}
"
    );
    let output = output(&run(&source).stdout);
    assert_eq!(output.status, 0);
    let image = output.frame.expect("the counter drew");
    assert_eq!(rawr_play::plain::image_kind(&image), Some("gif"));
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    let mut decoder = options.read_info(image.as_slice()).unwrap();
    assert_eq!((decoder.width(), decoder.height()), (1024, 768));
    // No loop extension: it plays once.
    assert_eq!(decoder.repeat(), gif::Repeat::Finite(0));
    let mut delays = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        delays.push(frame.delay);
    }
    // The count from 0 to 5, a change a frame, and nothing after it
    // settled: at most one frame per value.
    assert!((3..=6).contains(&delays.len()), "{delays:?}");
    // The last frame, the settled one, is held.
    assert_eq!(delays.last(), Some(&300), "{delays:?}");
}

#[test]
#[ignore = "needs the universe runner"]
fn a_tokio_program_still_answers_with_its_text() {
    let source = r#"use std::time::Duration;

#[tokio::main]
async fn main() {
    tokio::time::sleep(Duration::from_millis(10)).await;
    let handle = tokio::spawn(async { 6 * 7 });
    let value = handle.await.unwrap();
    println!("joined {value}");
}
"#;
    let stdout = run(source).stdout;
    assert_eq!(
        stdout.first(),
        Some(&b'{'),
        "{}",
        String::from_utf8_lossy(&stdout)
    );
    let output = rawr_play::Output::from_json(&stdout).unwrap();
    assert_eq!((output.text().as_str(), output.status), ("joined 42\n", 0));
}

/// A server on the universe runner, prepared once.
fn server() -> rawr_play::universe::Server {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("universe");
    let runner = root.join("target/rawr/runner/target/debug/play");
    assert!(runner.is_file(), "{NEEDS_RUNNER}");
    rawr_play::universe::Universe::new(root, runner)
        .serve()
        .unwrap_or_else(|error| panic!("{error}"))
}

fn limits(millis: u64) -> rawr_play::Limits {
    rawr_play::Limits {
        time: std::time::Duration::from_millis(millis),
        ..rawr_play::Limits::default()
    }
}

fn text(output: rawr_play::Output) -> rawr_play::Output {
    assert!(output.frame.is_none(), "a frame, not text");
    output
}

/// One prepared runner answers snippets in turn (#515). Each is a program
/// of its own: a function or a value one defines is not the next one's,
/// and an iced program still draws its frame.
#[test]
#[ignore = "needs the universe runner"]
fn snippets_in_turn_on_one_server_are_independent() {
    let server = server();
    let first = r#"struct Count(u32);

fn helper(count: &mut Count) -> u32 {
    count.0 += 1;
    count.0
}

fn main() {
    let mut count = Count(0);
    helper(&mut count);
    println!("first {}", helper(&mut count));
}
"#;
    let output = text(server.run(first, &limits(30_000)).unwrap());
    assert_eq!((output.text().as_str(), output.status), ("first 2\n", 0));
    // The first snippet's function is not this one's.
    let rawr_play::PlayError::Compile(error) = server
        .run(
            "fn main() {\n    println!(\"{}\", helper());\n}\n",
            &limits(30_000),
        )
        .unwrap_err();
    assert!(error.contains("helper"), "{error}");
    // The same snippet again starts from nothing.
    let output = text(server.run(first, &limits(30_000)).unwrap());
    assert_eq!((output.text().as_str(), output.status), ("first 2\n", 0));
    let output = server.run(&counter(), &limits(30_000)).unwrap();
    let png = output.frame.expect("the counter drew no frame");
    assert_eq!(rawr_play::plain::image_kind(&png), Some("png"));
    let output = text(server.run("1 + 2", &limits(30_000)).unwrap());
    assert_eq!((output.text().as_str(), output.status), ("3\n", 0));
}

/// A snippet past its time is stopped, and the server answers the next.
#[test]
#[ignore = "needs the universe runner"]
fn a_snippet_past_its_time_is_stopped_and_the_server_goes_on() {
    let server = server();
    let source =
        "fn main() {\n    println!(\"start\");\n    let mut n: u64 = 0;\n    loop { n += 1; }\n}\n";
    let output = text(server.run(source, &limits(1_000)).unwrap());
    assert_eq!((output.status, output.limit), (1, true));
    assert_eq!(output.text(), "start\ntime limit exceeded\n");
    let output = text(server.run("1 + 2", &limits(30_000)).unwrap());
    assert_eq!((output.text().as_str(), output.status), ("3\n", 0));
}

/// A snippet past its memory is stopped (`RLIMIT_AS`, which only Linux
/// enforces), and the server answers the next.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs the universe runner"]
fn a_snippet_past_its_memory_is_stopped_and_the_server_goes_on() {
    let server = server();
    let limits = rawr_play::Limits {
        memory: Some(1024 * 1024 * 1024),
        ..limits(30_000)
    };
    let source =
        "fn main() {\n    let text = \"x\".repeat(1 << 32);\n    println!(\"{}\", text.len());\n}\n";
    let output = text(server.run(source, &limits).unwrap());
    assert!(
        output.status != 0 && !output.text().contains("4294967296"),
        "{output:?}"
    );
    let output = text(server.run("1 + 2", &limits).unwrap());
    assert_eq!((output.text().as_str(), output.status), ("3\n", 0));
}
