// rawr
// iced's loupe: the program's own `Zoomed<W>` implements the crate's
// `Widget` for any `W: Widget`, through `W`'s methods. `zoomed` takes an
// `impl Widget` and wraps what its `explain` gives, the opaque type the
// trait's own default body gives, so the program's impl runs at that type.

use events::widgets::{button, column, text, Framed, Renderer, Widget};
use events::Theme;

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

impl<W, Message> Widget<Message> for Zoomed<W>
where
    W: Widget<Message>,
{
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!("zoom {}", self.zoom));
        self.content.labels(labels);
    }

    fn press(&self, label: &str) -> Option<Message> {
        Widget::press(&self.content, label)
    }
}

fn zoomed<'a, Message: 'a>(
    zoom: u16,
    content: impl Widget<Message, Theme, Renderer> + 'a,
) -> Zoomed<impl Widget<Message> + 'a> {
    Zoomed {
        zoom,
        content: content.explain(0x000000_u32),
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
        column![
            zoomed(3, button("Increment").on_press(Message::Increment)),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
