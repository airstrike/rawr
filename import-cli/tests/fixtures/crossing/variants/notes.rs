// rawr
// iced's changelog: `markdown::view<'a>(items: &'a [Item], settings,
// theme) -> impl Widget<Uri, ..> + 'a` borrows the state's items for the
// view's lifetime. The items aren't `Clone` and count their draws, so each
// view draws the one value of each item the state holds, which `update`
// then adds to once no view borrows them.

use events::controls::markdown;
use events::widgets::{button, column, Framed, Widget};
use events::Theme;

struct Notes {
    items: Vec<markdown::Item>,
    clicked: Option<markdown::Uri>,
}

impl Default for Notes {
    fn default() -> Self {
        Self {
            items: markdown::parse("first\n\nsecond").collect(),
            clicked: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Add,
    LinkClicked(markdown::Uri),
}

impl Notes {
    fn update(&mut self, message: Message) {
        match message {
            Message::Add => self.items.extend(markdown::parse("third")),
            Message::LinkClicked(uri) => self.clicked = Some(uri),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let size = if self.clicked.is_some() { 12.0 } else { 16.0 };
        column![
            button("Add").on_press(Message::Add),
            markdown::view(
                &self.items,
                markdown::Settings::with_text_size(size),
                Theme,
            )
            .map(Message::LinkClicked),
        ]
    }
}

fn main() {
    events::controls::application(Notes::default, Notes::update, Notes::view).run()
}
