// rawr
use iced::Center;
use iced::widget::{Widget, button, column, text};

use crate::{Counter, Message};

pub fn view(counter: &Counter) -> impl Widget<Message> {
    column![
        button("Increment").on_press(Message::Increment),
        text(counter.value).size(50),
        button("Decrement").on_press(Message::Decrement),
    ]
    .padding(20)
    .align_x(Center)
}
