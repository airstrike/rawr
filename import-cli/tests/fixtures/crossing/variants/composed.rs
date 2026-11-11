// rawr
// iced's component (#965, #1075): `component(numeric_input(..))` gives an
// opaque widget made of script values, which no region holds, and
// `center(..)` wraps it in a container whose type declares a lifetime. The
// view's output is that container, `Container<'_, impl Widget, Theme>`,
// which Rust code can't name.

use events::widgets::{button, container, opaque, Widget};

#[derive(Default)]
struct Counter {
    value: u16,
}

#[derive(Debug, Clone)]
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
        container(opaque(button("Increment").on_press(Message::Increment))).padding(self.value)
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
