//! `iced::application::timed`: an update that also takes the `Instant` of
//! each message, and `Instant` and `Duration` carried as bytes.

use std::time::Duration;

use rawr::host::value::Carry as _;

use super::Adapter;
use crate::snapshot::clock;

/// A timed application whose view shows how long after boot its last
/// message came, by the `Instant` its update was given.
const TIMED: &str = r#"
use iced::time::Instant;
use iced::widget::{button, column, text};
use iced::{Element, Subscription, Task, Theme};

struct Clock {
    start: Instant,
    now: Instant,
    ticks: u32,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
}

fn tick() -> Message {
    Message::Tick
}

impl Clock {
    fn new() -> Self {
        let now = Instant::now();
        Clock { start: now, now, ticks: 0 }
    }

    fn update(&mut self, message: Message, now: Instant) -> Task<Message> {
        match message {
            Message::Tick => {
                self.now = now;
                self.ticks += 1;
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            button(text("Tick")).on_press(Message::Tick),
            text(format!("{} ticks at {} ms", self.ticks, (self.now - self.start).as_millis())),
        ]
    }

    fn title(&self) -> String {
        format!("Clock {}", self.ticks)
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

pub fn main() -> iced::Result {
    iced::application::timed(Clock::new, Clock::update, Clock::subscription, Clock::view)
        .title(Clock::title)
        .theme(Clock::theme)
        .window_size((400.0, 300.0))
        .run()
}
"#;

fn boot(source: &str) -> Adapter {
    super::carry_tests::boot(source)
}

fn shows(adapter: &Adapter, line: &str) -> bool {
    iced_test::simulator(adapter.view()).find(line).is_ok()
}

fn tick(adapter: &mut Adapter) {
    let message = adapter.message_of("tick").unwrap().unwrap();
    adapter.deliver([message]);
}

#[test]
fn a_timed_application_checks_and_runs() {
    let _frozen = clock::freeze();
    let mut adapter = boot(TIMED);
    assert!(shows(&adapter, "0 ticks at 0 ms"));
    assert_eq!(adapter.title(), "Clock 0");
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Tick").unwrap();
    let message = simulator.into_messages().next().unwrap();
    adapter.update(message).unwrap();
    assert!(shows(&adapter, "1 ticks at 0 ms"));
    assert_eq!(adapter.title(), "Clock 1");
}

#[test]
fn update_gets_the_instant_of_its_message_on_the_frozen_clock() {
    let _frozen = clock::freeze();
    let start = clock::now().unwrap();
    let mut adapter = boot(TIMED);
    clock::set(start + Duration::from_millis(250));
    tick(&mut adapter);
    assert!(shows(&adapter, "1 ticks at 250 ms"));
    clock::set(start + Duration::from_millis(1_000));
    tick(&mut adapter);
    assert!(shows(&adapter, "2 ticks at 1000 ms"));
}

#[test]
fn a_timed_snapshot_is_the_same_on_every_run() {
    let render = || {
        let _frozen = clock::freeze();
        let start = clock::now().unwrap();
        let mut adapter = boot(TIMED);
        clock::set(start + Duration::from_millis(1_500));
        let message = adapter.message_of("tick").unwrap().unwrap();
        let canvas = crate::snapshot::Canvas {
            fit: true,
            fallback: (200, 100),
            margin: 0,
            scale: 1.0,
            max_pixels: 1024,
        };
        crate::snapshot::render_application(&mut adapter, vec![message], canvas, (200, 100), None)
            .unwrap()
            .bytes
    };
    let first = render();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(first, render());
}

#[test]
fn a_timed_update_must_take_an_instant() {
    let source = TIMED.replace(
        "fn update(&mut self, message: Message, now: Instant)",
        "fn update(&mut self, message: Message, now: u32)",
    );
    let mut engine = rawr::Engine::new();
    crate::install(&mut engine).unwrap();
    let Err(error) = rawr::Shared::new(engine).compile_application_descriptor(&source) else {
        panic!("an update whose third parameter is not an Instant is refused");
    };
    assert!(error.to_string().contains("iced::time::Instant"), "{error}");
}

#[test]
fn a_timed_application_takes_its_subscription_as_its_third_argument() {
    let source = TIMED.replace(
        ".title(Clock::title)",
        ".subscription(Clock::subscription).title(Clock::title)",
    );
    let mut engine = rawr::Engine::new();
    crate::install(&mut engine).unwrap();
    let Err(error) = rawr::Shared::new(engine).compile_application_descriptor(&source) else {
        panic!("a second subscription is refused");
    };
    assert!(error.to_string().contains("third argument"), "{error}");
}

#[test]
fn a_duration_carries_exactly() {
    for duration in [
        Duration::ZERO,
        Duration::new(3, 141_592_653),
        Duration::new(u64::MAX, 999_999_999),
    ] {
        let mut bytes = Vec::new();
        crate::marker::CarryDuration::encode(&duration, &mut bytes);
        assert_eq!(crate::marker::CarryDuration::decode(&bytes), Some(duration));
    }
    assert_eq!(crate::marker::CarryDuration::decode(&[0; 11]), None);
    let mut invalid = vec![0; 8];
    invalid.extend_from_slice(&1_000_000_000_u32.to_le_bytes());
    assert_eq!(crate::marker::CarryDuration::decode(&invalid), None);
}

#[test]
fn an_instant_carries_as_its_distance_from_the_present() {
    let now = iced::time::Instant::now();
    for (instant, ago) in [
        (now - Duration::from_millis(1_500), true),
        (now + Duration::from_millis(700), false),
    ] {
        let mut bytes = Vec::new();
        crate::marker::CarryInstant::encode(&crate::marker::Instant(instant), &mut bytes);
        let decoded = crate::marker::CarryInstant::decode(&bytes).unwrap().0;
        let expected = if ago {
            now.elapsed() + Duration::from_millis(1_500)
        } else {
            Duration::from_millis(700).saturating_sub(now.elapsed())
        };
        let actual = if ago {
            decoded.elapsed()
        } else {
            decoded.duration_since(iced::time::Instant::now())
        };
        let error = expected.abs_diff(actual);
        assert!(error < Duration::from_millis(5), "off by {error:?}");
    }
    assert!(crate::marker::CarryInstant::decode(&[0; 15]).is_none());
}

const STOPWATCH: &str = include_str!("../../../play/tests/fixtures/stopwatch.rs");

#[test]
fn a_running_stopwatch_keeps_its_elapsed_time_across_a_runner_restart() {
    let _frozen = clock::freeze();
    let start = clock::now().unwrap();
    let mut old = boot(STOPWATCH);
    let mut simulator = iced_test::simulator(old.view());
    simulator.click("Start").unwrap();
    let message = simulator.into_messages().next().unwrap();
    old.update(message).unwrap();
    assert!(shows(&old, "Stop"));
    let launch = old.launch.static_authority().unwrap().clone();
    let bytes = old
        .export_state_authority(&launch)
        .expect("a Duration and an Instant are carried as bytes");

    // The next runner process boots on its own present.
    clock::set(start + Duration::from_secs(2));
    let mut new = boot(&STOPWATCH.replace("\"Reset\"", "\"Clear\""));
    assert!(shows(&new, "Start"));
    let launch = new.launch.static_authority().unwrap().clone();
    assert!(new.seed_state_authority(&launch, &bytes, None));
    assert!(shows(&new, "Stop") && shows(&new, "Clear"));
}
