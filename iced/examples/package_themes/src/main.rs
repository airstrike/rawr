// rawr
mod midnight;

use crate::midnight::midnight;
use iced::Theme;
use iced::{Element, Widget};
use iced::widget::{button, column, text};

struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Counter {
    fn new() -> Counter {
        Counter { value: 0 }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let count = text(self.value).style(text::primary);
        let increment = button("Increment")
            .style(button::primary)
            .on_press(Message::Increment);
        column![
            count,
            increment,
        ]
        .boxed()
    }

    fn theme(&self) -> Theme {
        midnight()
    }
}

pub fn main() -> iced::Result {
    iced::application(Counter::new, Counter::update, Counter::view)
        .theme(Counter::theme)
        .run()
}
