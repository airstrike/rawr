// rawr
// iced's editor: `action(new_icon(), "New", Some(Message::Increment))`
// calls the program's `fn new_icon<Message>() -> impl Widget<Message>`,
// whose `Message` only `action`'s parameter fixes, from the message a
// later argument gives, as rustc infers it from the expected type.

use events::widgets::{column, span, text, tile, Widget};

#[derive(Default)]
struct Editor {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Editor {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            action(new_icon(), "New", Some(Message::Increment)),
            action(
                icon('S'),
                "Save",
                (self.value > 1).then_some(Message::Increment)
            ),
            text(self.value),
        ]
    }
}

fn action<'a, Message: Clone + 'a>(
    content: impl Widget<Message> + 'a,
    label: &'a str,
    on_press: Option<Message>,
) -> impl Widget<Message> + 'a {
    let action = tile(content);
    if let Some(on_press) = on_press {
        column![action.on_press(on_press), text(label)].boxed()
    } else {
        action.boxed()
    }
}

fn new_icon<Message>() -> impl Widget<Message> {
    glyph("Increment")
}

fn glyph<Message>(name: &'static str) -> impl Widget<Message> {
    span(name)
}

fn icon<Message>(codepoint: char) -> impl Widget<Message> {
    text(codepoint).size(12)
}

fn main() {
    events::widgets::run(Editor::update, Editor::view);
}
