//! `rawr-play` on the universe runner (#434): an iced program answers with
//! the PNG of its first settled frame, and a tokio program with its text.
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

/// iced's counter, as its example writes it.
const COUNTER: &str = r#"use iced::widget::{button, column, text, Widget};
use iced::Center;

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
}

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}
"#;

#[test]
#[ignore = "needs the universe runner"]
fn an_iced_counter_answers_with_its_first_frame_as_a_png() {
    let png = run(COUNTER).stdout;
    assert!(
        rawr_play::message::is_png(&png),
        "{}",
        String::from_utf8_lossy(&png)
    );
    // The header's width and height: iced's default window, 1024 by 768.
    let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    assert_eq!((size(16), size(20)), (1024, 768));
    // Two buttons and a number on a plain background: more than an empty
    // frame, far less than a raw one.
    assert!(
        (2_000..1_000_000).contains(&png.len()),
        "{} bytes",
        png.len()
    );
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
