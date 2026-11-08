// rawr
//! `?play`'s universe (#434, #436). The prebuilt runner covers the native
//! calls this program makes, and a snippet runs on it when its own native
//! calls are among them. Script types are named by their index in the
//! program's definition order, so a snippet's first three types meet this
//! program's `Counter`, `Message` and `Language` whatever their names.
//!
//! The calls are iced's examples' own: the counter, a row and a column
//! with spacing, text of a number, a `&str` and a `String`, sliders of an
//! `f32` and of `Radians` (gradient), a pick list (pick_list), keyboard
//! keys matched in a subscription (stopwatch, layout), a clock's ticks
//! (stopwatch, clock: #889), and an application with a subscription and a
//! scale factor (#863). A script function a call
//! takes fills it by its place, whatever it is named, so a snippet's
//! `view` or message meets this program's.
//!
//! Its `examples` are programs of their own (#1000): iced's examples that
//! `examples.txt` names and ?play's own in `programs/`, which
//! `examples.sh` copies here. Each has its own `main`, so the runner counts
//! its types from zero, as a snippet's are, and a snippet that makes the
//! native calls one of them makes runs as it does.

mod examples;

use iced::keyboard::{self, key};
use iced::widget::{button, column, pick_list, row, slider, text, Widget};
use iced::{time, window, Center, Radians, Subscription};
use std::time::Duration;

/// Its `Default` is derived, as the counter's is: the `default` an
/// application takes is the derived one. `Radians` has none, so the angle
/// is made from the level.
#[derive(Default)]
struct Counter {
    value: i64,
    level: f32,
    language: Option<Language>,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
    LevelChanged(f32),
    AngleChanged(Radians),
    LanguageSelected(Language),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Language {
    #[default]
    Rust,
    Elm,
}

impl Language {
    const ALL: &[Language] = &[Language::Rust, Language::Elm];
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Language::Rust => "Rust",
            Language::Elm => "Elm",
        })
    }
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
            Message::LevelChanged(level) => {
                self.level = level;
            }
            Message::AngleChanged(angle) => {
                self.level = angle.0;
            }
            Message::LanguageSelected(language) => {
                self.language = Some(language);
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

    /// Text of each kind, a row, spacing, sliders and a pick list.
    fn controls(&self) -> impl Widget<Message> {
        let languages = pick_list(self.language, Language::ALL, Language::to_string)
            .on_select(Message::LanguageSelected)
            .placeholder("Choose a language...");
        let angle = Radians(self.level);
        column![
            text(format!("Count: {} at {:.2} rad", self.value, angle.0)),
            text("Controls").size(20),
            row![
                button("-").on_press(Message::Decrement),
                text(self.value),
                button("+").on_press(Message::Increment),
            ]
            .spacing(10)
            .align_y(Center),
            slider(0.0..=100.0, self.level, Message::LevelChanged),
            slider(Radians::RANGE, angle, Message::AngleChanged).step(0.01),
            languages,
        ]
        .spacing(20)
        .padding(20)
        .align_x(Center)
    }

    fn scale_factor(&self) -> f32 {
        2.0
    }

    /// Frames up to a count of five, a tick a second down to it,
    /// and keys matched as stopwatch and layout match them.
    fn subscription(&self) -> Subscription<Message> {
        // A key handler that names the event's type, as stopwatch's does.
        fn hotkey(event: keyboard::Event) -> Option<Message> {
            let keyboard::Event::KeyPressed { modified_key, .. } = event else {
                return None;
            };
            match modified_key.as_ref() {
                keyboard::Key::Named(key::Named::ArrowUp) => Some(Message::Increment),
                keyboard::Key::Character("-") => Some(Message::Decrement),
                _ => None,
            }
        }

        let frames = if self.value < 5 {
            window::frames().map(|_| Message::Increment)
        } else {
            Subscription::none()
        };
        let ticks = if self.value > 5 {
            time::every(Duration::from_secs(1)).map(|_| Message::Decrement)
        } else {
            Subscription::none()
        };
        let keys = keyboard::listen().filter_map(|event| {
            let keyboard::Event::KeyPressed {
                key, modified_key, ..
            } = event
            else {
                return None;
            };
            match key.as_ref() {
                keyboard::Key::Named(
                    key::Named::Space
                    | key::Named::Enter
                    | key::Named::ArrowUp
                    | key::Named::ArrowRight,
                ) => Some(Message::Increment),
                keyboard::Key::Named(
                    key::Named::Backspace | key::Named::ArrowDown | key::Named::ArrowLeft,
                ) => Some(Message::Decrement),
                keyboard::Key::Named(key::Named::Escape | key::Named::Tab) => None,
                _ => match modified_key.as_ref() {
                    keyboard::Key::Character("+") => Some(Message::Increment),
                    keyboard::Key::Character("-") => Some(Message::Decrement),
                    _ => None,
                },
            }
        });
        Subscription::batch(vec![
            frames,
            ticks,
            keys,
            keyboard::listen().filter_map(hotkey),
        ])
    }
}

/// An iced program's `main`, as iced's counter writes it.
pub fn run() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}

/// An application as iced's examples build one: alone, at the scale it
/// asks for (#529), counting to five a frame at a time (#527), or both, in
/// either order (#863).
pub fn application(choice: u8) -> iced::Result {
    let application = iced::application(Counter::default, Counter::update, Counter::view);
    match choice {
        0 => application.run(),
        1 => application.scale_factor(Counter::scale_factor).run(),
        2 => application.subscription(Counter::subscription).run(),
        3 => application
            .subscription(Counter::subscription)
            .scale_factor(Counter::scale_factor)
            .run(),
        _ => application
            .scale_factor(Counter::scale_factor)
            .subscription(Counter::subscription)
            .run(),
    }
}

/// tokio's own calls, on the server. A page has one thread and no
/// blocking pool, so the browser's universe makes none of them.
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() {
    tokio::spawn(async { println!("ran") });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let handle = tokio::spawn(async { 6 * 7 });
    let value = handle.await.unwrap();
    println!("joined {value}");
    let _blocking = tokio::task::spawn_blocking(|| println!("blocking ran"));
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[cfg(target_arch = "wasm32")]
fn main() {}
