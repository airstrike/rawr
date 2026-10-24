use iced::time::{self, Duration};
use iced::widget::{column, text};
use iced::{Subscription, Widget};

pub fn main() -> iced::Result {
    iced::application(Stopwatch::default, Stopwatch::update, Stopwatch::view)
        .subscription(Stopwatch::subscription)
        .run()
}

#[derive(Default)]
struct Stopwatch {
    tenths: u64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick,
}

impl Stopwatch {
    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => self.tenths += 1,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            text("Stopwatch"),
            text(format!("{}.{} s", self.tenths / 10, self.tenths % 10)).size(32),
        ]
        .spacing(4)
    }
}
