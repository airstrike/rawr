// rawr
// iced's stopwatch in the fixture's controls: the crate's `time` re-exports
// std's `Duration` and `Instant`, which the script names through it, and
// `time::every(milliseconds(10))` gives a `Subscription<Instant>` that a
// variant's constructor maps. Each tick's instant is std's, and the
// stopwatch measures from the first.

use events::controls::time::{self, milliseconds, Duration, Instant};
use events::controls::{quote, toggle, Subscription};
use events::widgets::{column, Widget};

struct Stopwatch {
    duration: Duration,
    state: State,
    ticks: u32,
    shown: String,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Ticking {
        first: Option<Instant>,
        started: Instant,
    },
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggle(bool),
    Tick(Instant),
}

impl Stopwatch {
    fn new() -> Self {
        Stopwatch {
            duration: Duration::default(),
            state: State::Ticking {
                first: None,
                started: Instant::now(),
            },
            ticks: 0,
            shown: String::from("0 ms in 0 ticks"),
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Toggle(on) => {
                self.state = if on {
                    State::Ticking {
                        first: None,
                        started: Instant::now(),
                    }
                } else {
                    State::Idle
                };
            }
            Message::Tick(now) => {
                if let State::Ticking { first, started } = &mut self.state {
                    let since: time::Duration = now - *started;
                    if since < Duration::from_secs(60) {
                        self.ticks += 1;
                    }
                    if let Some(earlier) = *first {
                        self.duration = now - earlier;
                    } else {
                        *first = Some(now);
                    }
                }
                self.shown = format!("{} ms in {} ticks", self.duration.as_millis(), self.ticks);
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        match self.state {
            State::Idle => Subscription::none(),
            State::Ticking { .. } => time::every(milliseconds(10)).map(Message::Tick),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let ticking = matches!(self.state, State::Ticking { .. });
        let toggle = toggle(ticking).label("Stop").on_toggle(Message::Toggle);
        column![toggle, quote(&self.shown)]
    }
}

fn main() {
    events::controls::application(Stopwatch::new, Stopwatch::update, Stopwatch::view)
        .subscription(Stopwatch::subscription)
        .run()
}
