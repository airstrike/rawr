// rawr
// iced's clock in the fixture's controls: the subscription maps each tick
// by a closure that reads the crate's clock, `|_| Message::Tick(Stamp::now())`,
// and the boot's task maps by a function that reads it too. Native code
// calls both on another thread, as iced's executor does, and each makes a
// crate value inside the program's message.

use events::controls::time::{self, milliseconds, Stamp};
use events::controls::{quote, Subscription};
use events::widgets::{column, Widget};
use events::Task;

struct Clock {
    now: Stamp,
    ticks: u32,
    shown: String,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick(Stamp),
}

impl Clock {
    fn new() -> (Self, Task<Message>) {
        let clock = Clock {
            now: Stamp::now(),
            ticks: 0,
            shown: String::from("no ticks"),
        };
        (clock, Task::done(0).map(Clock::stamped))
    }

    fn stamped(_: u32) -> Message {
        Message::Tick(Stamp::now())
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => {
                if now != self.now {
                    self.now = now;
                    self.ticks += 1;
                }
                self.shown = format!("{} ticks at {}", self.ticks, self.now.second());
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(milliseconds(500)).map(|_| Message::Tick(Stamp::now()))
    }

    fn view(&self) -> impl Widget<Message> {
        column![quote(&self.shown)]
    }
}

fn main() {
    events::controls::application(Clock::new, Clock::update, Clock::view)
        .subscription(Clock::subscription)
        .run()
}
