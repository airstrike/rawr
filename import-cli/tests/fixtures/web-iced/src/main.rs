// rawr
use iced::widget::{button, column, text, Widget};
use iced::Center;

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            text(format!("Count: {}", self.value)).size(40),
            button(text("Add one").size(24)).on_press(Message::Increment),
        ]
        .spacing(20)
        .padding(40)
        .align_x(Center)
    }
}

fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}
