// rawr
// iced's loupe: program functions whose outputs name `impl Widget` inside
// another type. `zoomed` gives the program's own `Zoomed<impl Widget<M> +
// 'a>` and `centered` the crate's `Column<impl Widget<M> + 'a>`. Each
// hidden type is the crate's opaque widget, which the bodies make from
// crate calls, as rustc infers it.

use events::widgets::{button, center, column, opaque, text, Column, Widget};

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

impl<W> Zoomed<W> {
    fn padded(self) -> Column<W> {
        center(self.content).padding(self.zoom)
    }
}

fn zoomed<'a, Message: 'a>(
    zoom: u16,
    content: impl Widget<Message> + 'a,
) -> Zoomed<impl Widget<Message> + 'a> {
    Zoomed {
        zoom,
        content: opaque(content),
    }
}

fn centered<'a, Message: 'a>(content: impl Widget<Message> + 'a) -> Column<impl Widget<Message> + 'a> {
    center(opaque(content))
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            zoomed(3, button("Increment").on_press(Message::Increment)).padded(),
            text(self.value).size(50),
            centered(button("Decrement").on_press(Message::Decrement)),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
