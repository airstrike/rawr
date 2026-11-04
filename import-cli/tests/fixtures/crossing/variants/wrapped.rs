// rawr
// iced's loupe: a program function whose output names `impl Widget` inside
// another type. `zoomed` gives the program's own `Zoomed<impl Widget<M> +
// 'a>`, whose hidden type is the crate's `Column` of a `Button`, which the
// body makes from crate calls, as rustc infers it. The view passes it on
// to `column!` and reads its zoom.

use events::widgets::{button, center, column, text, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

/// A program's own wrapper of a widget.
struct Zoomed<W> {
    zoom: u16,
    content: W,
}

fn zoomed<'a, Message: Clone + 'a>(
    zoom: u16,
    label: &'a str,
    message: Message,
) -> Zoomed<impl Widget<Message> + 'a> {
    Zoomed {
        zoom,
        content: center(button(label).on_press(message)),
    }
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let zoomed = zoomed(3, "Increment", Message::Increment);
        let padding = zoomed.zoom;
        column![
            zoomed.content,
            text(self.value).size(50),
            center(button("Decrement").on_press(Message::Decrement)).padding(padding),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
