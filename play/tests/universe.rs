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

/// An iced program that never settles, run with a far ceiling and no
/// deadline, picks up the deadline the bot sets as it runs: its shell ends
/// the GIF before it, sooner than the shell's own 100 frames would,
/// and the run is not killed.
#[test]
#[ignore = "needs the universe runner"]
fn an_iced_program_finishes_its_gif_before_a_deadline_set_as_it_runs() {
    let server = server();
    let endless = COUNTER_TYPES.replace("if self.value < 5 {", "if self.value >= 0 {");
    let source = format!(
        "{endless}
pub fn main() -> iced::Result {{
    iced::application(Counter::default, Counter::update, Counter::view)
        .subscription(Counter::subscription)
        .run()
}}
"
    );
    let file = rawr::play::deadline::File::new().unwrap();
    let limits = rawr_play::Limits {
        deadline: Some(file.path().to_owned()),
        ..limits(600_000)
    };
    let started = std::time::Instant::now();
    let output = std::thread::scope(|scope| {
        let run = scope.spawn(|| server.run(&source, &limits));
        std::thread::sleep(std::time::Duration::from_secs(2));
        let at = std::time::SystemTime::now() + std::time::Duration::from_millis(1_500);
        file.set(at).unwrap();
        run.join().unwrap().unwrap()
    });
    let elapsed = started.elapsed();
    assert_eq!((output.status, output.limit), (0, false), "{output:?}");
    let image = output.frame.expect("the program drew");
    assert_eq!(rawr_play::plain::image_kind(&image), Some("gif"));
    assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
}

/// A program each of whose frames takes longer to draw than the shell's quiet second and
/// than the 5 s a fast machine needs for 100 frames, as a frame does on
/// ?play's fraction of a CPU, run as the bot runs one with nothing queued:
/// it still answers with a GIF of every frame it changed.
#[test]
#[ignore = "needs the universe runner"]
fn an_iced_program_slow_to_draw_answers_with_every_frame_it_changed() {
    let source = r#"use iced::widget::{column, text, Widget};
use iced::{window, Center, Subscription};
use std::time::Duration;

#[derive(Default)]
struct Slow {
    t: i64,
}

#[derive(Debug, Clone, Copy)]
enum Msg {
    Tick,
}

impl Slow {
    fn update(&mut self, _: Msg) {
        self.t += 1;
    }

    fn view(&self) -> impl Widget<Msg> {
        std::thread::sleep(Duration::from_millis(1800));
        column![text(self.t).size(40)].padding(20).align_x(Center)
    }

    fn subscription(&self) -> Subscription<Msg> {
        if self.t < 2 {
            window::frames().map(|_| Msg::Tick)
        } else {
            Subscription::none()
        }
    }
}

fn main() -> iced::Result {
    iced::application(Slow::default, Slow::update, Slow::view)
        .subscription(Slow::subscription)
        .run()
}
"#;
    let server = server();
    let file = rawr::play::deadline::File::new().unwrap();
    let limits = rawr_play::Limits {
        deadline: Some(file.path().to_owned()),
        ..limits(600_000)
    };
    let output = server.run(source, &limits).unwrap();
    assert_eq!((output.status, output.limit), (0, false), "{output:?}");
    let image = output.frame.expect("the program drew");
    assert_eq!(rawr_play::plain::image_kind(&image), Some("gif"));
    let mut decoder = gif::DecodeOptions::new()
        .read_info(image.as_slice())
        .unwrap();
    let mut frames = 0;
    while decoder.read_next_frame().unwrap().is_some() {
        frames += 1;
    }
    // 0, 1 and 2, a change a frame.
    assert_eq!(frames, 3);
}

/// A snippet's state, message and enum, named otherwise than the
/// universe program's, as iced's gradient, pick_list and stopwatch write
/// theirs (#863); [`CONTROLS`] and [`KEYS`] add a view and a `main`.
const APP_TYPES: &str = r#"use iced::keyboard::{self, key};
use iced::widget::{button, column, pick_list, row, slider, text, Widget};
use iced::{window, Center, Radians, Subscription};

#[derive(Default)]
struct App {
    count: i64,
    volume: f32,
    fruit: Option<Fruit>,
}

#[derive(Debug, Clone, Copy)]
enum Msg {
    Up,
    Down,
    Volume(f32),
    Angle(Radians),
    Pick(Fruit),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Fruit {
    #[default]
    Apple,
    Pear,
}

impl Fruit {
    const ALL: &[Fruit] = &[Fruit::Apple, Fruit::Pear];
}

impl std::fmt::Display for Fruit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl App {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Up => self.count += 1,
            Msg::Down => self.count -= 1,
            Msg::Volume(volume) => self.volume = volume,
            Msg::Angle(angle) => self.volume = angle.0,
            Msg::Pick(fruit) => self.fruit = Some(fruit),
        }
    }
"#;

/// Text of a `String`, a row, spacing, sliders of an `f32` and of
/// `Radians`, and a pick list.
const CONTROLS: &str = r#"
    fn view(&self) -> impl Widget<Msg> {
        let angle = Radians(self.volume);
        column![
            text(format!("{} and {:.2} rad", self.count, angle.0)),
            text("Pick a fruit").size(24),
            row![
                button("-").on_press(Msg::Down),
                text(self.count),
                button("+").on_press(Msg::Up),
            ]
            .spacing(8)
            .align_y(Center),
            slider(0.0..=100.0, self.volume, Msg::Volume),
            slider(Radians::RANGE, angle, Msg::Angle).step(0.01),
            pick_list(self.fruit, Fruit::ALL, Fruit::to_string)
                .on_select(Msg::Pick)
                .placeholder("Fruit..."),
        ]
        .spacing(16)
        .padding(20)
        .align_x(Center)
    }
}

fn main() -> iced::Result {
    iced::run(App::update, App::view)
}
"#;

/// Keys matched from `keyboard::listen`, batched with frames, in an
/// application with a subscription and a scale factor.
const KEYS: &str = r#"
    fn view(&self) -> impl Widget<Msg> {
        column![
            text(format!("{} (press space)", self.count)),
            row![button("-").on_press(Msg::Down), button("+").on_press(Msg::Up)].spacing(8),
        ]
        .spacing(16)
        .padding(20)
        .align_x(Center)
    }

    fn subscription(&self) -> Subscription<Msg> {
        let frames = if self.count < 3 {
            window::frames().map(|_| Msg::Up)
        } else {
            Subscription::none()
        };
        let keys = keyboard::listen().filter_map(|event| {
            let keyboard::Event::KeyPressed { key, .. } = event else {
                return None;
            };
            match key.as_ref() {
                keyboard::Key::Named(key::Named::Space) => Some(Msg::Up),
                keyboard::Key::Character("d") => Some(Msg::Down),
                _ => None,
            }
        });
        Subscription::batch(vec![frames, keys])
    }

    fn scale_factor(&self) -> f32 {
        1.5
    }
}

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .subscription(App::subscription)
        .scale_factor(App::scale_factor)
        .run()
}
"#;

/// The calls iced's recent examples make draw on the universe (#863).
#[test]
#[ignore = "needs the universe runner"]
fn text_rows_sliders_and_a_pick_list_answer_with_their_frame() {
    let output = output(&run(&format!("{APP_TYPES}{CONTROLS}")).stdout);
    assert_eq!(output.status, 0, "{}", output.text());
    let png = output.frame.expect("the controls drew a frame");
    assert_eq!(rawr_play::plain::image_kind(&png), Some("png"));
    assert_eq!(png_size(&png), (1024, 768));
}

/// A subscription of keys and frames, with a scale factor after it (#863).
#[test]
#[ignore = "needs the universe runner"]
fn keys_and_frames_with_a_scale_factor_answer_with_a_gif() {
    let output = output(&run(&format!("{APP_TYPES}{KEYS}")).stdout);
    assert_eq!(output.status, 0, "{}", output.text());
    let image = output.frame.expect("the counter drew");
    assert_eq!(rawr_play::plain::image_kind(&image), Some("gif"));
    let decoder = gif::DecodeOptions::new()
        .read_info(image.as_slice())
        .unwrap();
    assert_eq!((decoder.width(), decoder.height()), (1536, 1152));
}

/// A call the universe program doesn't make, at the types it makes it or
/// at all, fails to compile where the snippet makes it, and never reaches
/// the runner's bindings (#863).
#[test]
#[ignore = "needs the universe runner"]
fn a_call_the_universe_does_not_make_does_not_compile() {
    let server = server();
    for (view, message) in [
        // `text` of an `f64`: the universe takes text of an `i64`, a
        // `&str` and a `String`.
        (
            "column![text(self.volume as f64), button(\"Up\").on_press(Msg::Up)]",
            "error[RAWR-RESOLVE-HOST-OPERATION]: this use of a crate isn't among the universe's bindings",
        ),
        // A widget it doesn't use is not in its bindings at all, as rustc
        // says of a missing item.
        (
            "column![iced::widget::checkbox(self.count > 0)]",
            "error[E0425]: cannot find function `checkbox` in module `iced::widget`",
        ),
        // A method it doesn't call.
        (
            "column![text(self.count)].max_width(300)",
            "error[E0599]: no method named `max_width` found for `Column`",
        ),
        // Two functions where the universe's one adapter holds one: a
        // second slider of an `f32` with another message.
        (
            "column![slider(0.0..=1.0, self.volume, Msg::Volume), \
             slider(0.0..=1.0, self.volume, |volume| Msg::Volume(volume / 2.0))]",
            "error[RAWR-RESOLVE-HOST-OPERATION]: this use of a crate isn't among the universe's bindings",
        ),
    ] {
        let source = format!(
            "{APP_TYPES}
    fn view(&self) -> impl Widget<Msg> {{
        {view}
    }}
}}

fn main() -> iced::Result {{
    iced::run(App::update, App::view)
}}
"
        );
        let line = source.lines().position(|line| line.contains(view)).unwrap() + 1;
        let rawr_play::PlayError::Compile(error) = server
            .run(&source, &limits(30_000))
            .expect_err("a call outside the universe compiles");
        assert!(
            error.starts_with(message) && error.contains(&format!(" --> play.rs:{line}:")),
            "{error}"
        );
    }
}
