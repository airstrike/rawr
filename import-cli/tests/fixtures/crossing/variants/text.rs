// rawr
// iced's `text!` in the fixture's widgets: the macro's `$crate::widgets::
// Label::new(format!(..))` reaches `Text::new` through an alias that passes
// its parameters on, as iced's `$crate::Text::new` does.

use events::widgets::{button, column, text, Widget};

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
            text!("count {} of {}", self.value, 3).size(50),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
