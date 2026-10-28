// rawr
// iced's styling and delineate in the fixture's controls: `Look::ALL` is a
// crate constant `&'static [Look]` and `Look::MONOSPACE` a crate constant
// of a struct, neither of them a scalar, as iced's `Theme::ALL` and
// `Font::MONOSPACE` are. `caption!` reaches `Caption::new` through the
// only name of its type, as iced's `text!` reaches `Text::new`.

use events::controls::{caption, Look};
use events::widgets::{column, Widget};

#[derive(Debug, Clone)]
enum Message {
    Shown,
}

#[derive(Default)]
struct Styling {
    shown: u8,
}

impl Styling {
    fn update(&mut self, message: Message) {
        match message {
            Message::Shown => {
                self.shown += 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            caption!("{} looks", Look::ALL.len()).look(Look::MONOSPACE),
            caption(Look::ALL[1].name()),
            caption(self.shown).look(Look::DEFAULT),
        ]
    }
}

fn main() {
    events::controls::application(Styling::default, Styling::update, Styling::view).run()
}
