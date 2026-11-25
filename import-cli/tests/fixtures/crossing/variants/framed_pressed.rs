// rawr
// A crate helper of an opened module that returns a crate generic type no
// opened module holds, as iced's `button(..)`: its `Message` is the
// `on_press` argument's and its `Theme` the view's `impl Widget<Message>`
// default's, which rustc infers after the call.

use events::widgets::Widget;

#[derive(Default)]
struct Counter {
    count: u32,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.count += 1,
            Message::Decrement => self.count = 0,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        events::pressed("Increment").on_press(Message::Increment)
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
