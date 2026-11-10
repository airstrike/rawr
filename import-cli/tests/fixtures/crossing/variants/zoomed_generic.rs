// rawr
// As zoomed_rule.rs, but `zoomed` is generic over the message, which a call
// takes from its argument: `Message` of `impl Widget<Message>` given a
// `Rule<Message>` is the one the crate's `impl<M> Widget<M> for Rule<M>`
// gives, as rustc infers it, or the one a turbofish writes.

use events::widgets::{button, column, rule, text, Framed, Renderer, Widget};
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
        self.content.press(label)
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
            button("Increment").on_press(Message::Increment),
            zoomed(3, rule(4, Message::Decrement)),
            zoomed::<Message>(2, rule(5, Message::Increment)),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
