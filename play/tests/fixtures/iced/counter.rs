// From iced (https://github.com/iced-rs/iced), examples/counter/src/main.rs.
// Copyright 2019 Héctor Ramón, Iced contributors. MIT licence:
// https://github.com/iced-rs/iced/blob/master/LICENSE
// Changed: the tests module is dropped.

use iced::widget::{button, column, text, Widget};
use iced::Center;

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}

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
