// rawr
// iced's layout: `container(pin("• (50, 50)").x(50))`. A string literal
// fills `pin`'s content, so the container takes a `Pin<&str>`, a crate
// value whose type has a lifetime only its `&str` names.

use events::widgets::{button, column, container, pin, text, Widget};

#[derive(Default)]
struct Pinning {
    x: u16,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
}

impl Pinning {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.x += 10,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            container(pin("• (50, 50)").x(self.x)),
            button("Increment").on_press(Message::Increment),
            text(format!("{}", self.x)),
        ]
    }
}

fn main() {
    events::widgets::run(Pinning::update, Pinning::view);
}
