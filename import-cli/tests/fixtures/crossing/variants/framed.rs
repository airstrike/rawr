// rawr
// iced's `.boxed()` (scrollable, system_information, multi_window): a
// method of an extension trait that no widget implements itself, reached
// through its blanket impl over each widget's own `Widget` impl, which
// fixes the trait's arguments. Both arms of the `if` are one element type.

use events::widgets::{button, column, text, Framed, Widget};

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
        let shown = if self.value > 1 {
            text(self.value).size(50).framed()
        } else {
            button("Decrement").on_press(Message::Decrement).framed()
        };
        column![
            button("Increment").on_press(Message::Increment).framed(),
            shown,
        ]
        .padding(20)
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
