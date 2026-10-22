// rawr
mod theme;
mod ui;

use crate::ui::view;

#[derive(Default)]
pub struct Counter {
    pub value: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }
}

pub fn main() -> iced::Result {
    iced::run(Counter::update, view)
}
