// rawr
// iced's tour: `padded_button("Back")` calls a generic function of the
// program's own whose signature names a crate type, so it is compiled per
// instantiation (D629). Its `Message` is fixed by `.on_press(..)`, a later
// use of the call's value, and not by its arguments, as rustc infers it
// (#1074).

use events::widgets::{button, column, Button, Widget};

#[derive(Default)]
struct Tour {
    step: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    BackPressed,
    NextPressed,
}

impl Tour {
    fn update(&mut self, message: Message) {
        match message {
            Message::BackPressed => self.step -= 1,
            Message::NextPressed => self.step += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let back = padded_button("Decrement").on_press(Message::BackPressed);
        column![back, padded_button("Increment").on_press(Message::NextPressed)]
    }
}

fn padded_button<Message: Clone>(label: &str) -> Button<'_, Message, &str> {
    button(label)
}

fn main() {
    events::widgets::run(Tour::update, Tour::view);
}
