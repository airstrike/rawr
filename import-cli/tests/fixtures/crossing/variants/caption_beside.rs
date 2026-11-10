// rawr
// A caption, whose only name is the `Caption` alias, sits in one `column!`
// beside a quote, another widget of the same shape (`<'a, Theme>`), as
// iced's events puts `text(..)` beside other widgets.

use events::controls::{caption, quote, Look};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {}

#[derive(Default)]
struct Events {
    seen: u8,
}

impl Events {
    fn update(&mut self, message: Message) {
        match message {}
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            caption(self.seen).look(Look::MONOSPACE),
            quote("beside").size(12),
            caption("last"),
        ]
    }
}

fn main() {
    events::controls::application(Events::default, Events::update, Events::view).run()
}
