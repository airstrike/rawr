use iced::time::{self, milliseconds};
use iced::widget::{row, text};
use iced::{Subscription, Widget};

pub fn main() -> iced::Result {
    iced::application(Dots::default, Dots::update, Dots::view)
        .subscription(Dots::subscription)
        .run()
}

#[derive(Default)]
struct Dots {
    lit: usize,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick,
}

impl Dots {
    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => self.lit = (self.lit + 1) % 4,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(milliseconds(250)).map(|_| Message::Tick)
    }

    fn view(&self) -> impl Widget<Message> {
        let dot = |at: usize| text(if at == self.lit { "●" } else { "○" }).size(32);
        row![dot(0), dot(1), dot(2), dot(3)].spacing(8)
    }
}
