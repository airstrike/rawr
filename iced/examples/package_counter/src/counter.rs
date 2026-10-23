// rawr
use iced::Center;
use iced::widget::{Widget, button, column, text};

#[derive(Default)]
pub struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Increment,
    Decrement,
}

impl Counter {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }

    pub fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement),
        ]
        .padding(20)
        .align_x(Center)
    }
}
