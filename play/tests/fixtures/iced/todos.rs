use iced::widget::{button, column, row, text, Widget};
use iced::Center;

pub fn main() -> iced::Result {
    iced::run(Todos::update, Todos::view)
}

#[derive(Default)]
struct Todos {
    done: [bool; 3],
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggle(usize),
}

const ITEMS: [&str; 3] = ["Buy milk", "Walk the dog", "Try rawr"];

impl Todos {
    fn update(&mut self, message: Message) {
        match message {
            Message::Toggle(i) => self.done[i] = !self.done[i],
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let left = self.done.iter().filter(|done| !**done).count();
        let item = |i: usize| {
            let (action, item) = if self.done[i] {
                ("Undo", format!("{} (done)", ITEMS[i]))
            } else {
                ("Done", ITEMS[i].to_string())
            };
            row![button(action).on_press(Message::Toggle(i)), text(item)]
                .spacing(10)
                .align_y(Center)
        };
        column![
            text(format!("To do: {left} left")).size(20),
            item(0),
            item(1),
            item(2),
        ]
        .spacing(10)
        .padding(20)
    }
}
