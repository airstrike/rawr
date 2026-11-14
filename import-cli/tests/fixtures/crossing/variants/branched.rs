// rawr
// iced's table and toast: an `if`, an `else if` or a `match` picks one of
// a crate's functions of one signature, which meet at a `fn` pointer, and
// each toggle takes the style its branch picked. Beside them, two toggles
// take a crate function each, which are two instances of `style`.

use events::controls::{primary, secondary, toggle};
use events::widgets::{column, Widget};

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggled(bool),
}

#[derive(Default)]
struct Branched {
    toggled: bool,
    presses: u32,
}

impl Branched {
    fn update(&mut self, message: Message) {
        match message {
            Message::Toggled(toggled) => {
                self.toggled = toggled;
                self.presses += 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            toggle(self.toggled)
                .label("If")
                .on_toggle(Message::Toggled)
                .style(if self.toggled { primary } else { secondary }),
            toggle(self.toggled)
                .label("Match")
                .on_toggle(Message::Toggled)
                .style(match self.toggled {
                    true => secondary,
                    false => primary,
                }),
            toggle(self.toggled)
                .label("Else")
                .on_toggle(Message::Toggled)
                .style(if self.presses > 1 {
                    primary
                } else if self.toggled {
                    secondary
                } else {
                    primary
                }),
            toggle(self.toggled).label("Primary").style(primary),
            toggle(self.toggled).label("Secondary").style(secondary),
        ]
    }
}

fn main() {
    events::controls::application(Branched::default, Branched::update, Branched::view).run()
}
