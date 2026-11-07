// rawr
// iced's markdown (#1098): `titled(title, on_press)`, as `code_block`,
// takes two `impl Trait` inputs, one a callable a script closure fills, and
// gives an opaque widget. `explain`, a method the blanket impl over
// `Widget` gives that opaque type, names it as its `Self` with the same
// arguments the function's output has.

use events::widgets::{button, column, text, titled, Framed, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Titled(u16),
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Titled(width) => self.value += i64::from(width),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            titled("code", Message::Titled).explain(0x00ff00_u32),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
