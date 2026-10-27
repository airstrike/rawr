// rawr
// iced's counter in the fixture's widgets: `view`'s `impl Widget<Message>`
// leaves the renderer to the trait's defaults and the crate's impls (D604),
// and the buttons' string literals fill a type parameter as `&'static str`.

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
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
