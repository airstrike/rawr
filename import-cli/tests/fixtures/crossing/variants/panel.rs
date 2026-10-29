// rawr
// iced's component: the program's own generic type implements a crate
// trait whose `view` gives `impl Widget<Message>`, over the message its
// callback makes, and a function of the impl gives an `Option` of that
// message. rawr compiles the impl for the use the crate's bound makes of
// it (D629), so the runner's impl names the view's widget and the
// program's own message.

use events::custom;
use events::widgets::{button, column, text, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Changed(i64),
}

struct Stepper<Message> {
    value: i64,
    on_change: Box<dyn Fn(i64) -> Message>,
}

fn stepper<Message>(value: i64, on_change: impl Fn(i64) -> Message + 'static) -> Stepper<Message> {
    Stepper {
        value,
        on_change: Box::new(on_change),
    }
}

impl<'a, Message: Clone + 'a> custom::Panel<'a, Message> for Stepper<Message> {
    fn view(&self) -> impl Widget<Message> + 'a {
        column![
            button("Increment").on_press((self.on_change)(self.value + 1)),
            text(self.value),
        ]
    }

    fn fallback(&self, label: &str) -> Option<Message> {
        if label == "Decrement" {
            Some((self.on_change)(0))
        } else {
            None
        }
    }
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Changed(value) => {
                self.value = value;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        custom::panel(stepper(self.value, Message::Changed))
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
