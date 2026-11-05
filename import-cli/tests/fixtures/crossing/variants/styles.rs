// rawr
// iced's styling: `primary as fn(&Theme, _) -> _` casts a crate function
// to a function pointer whose parts the cast leaves to inference, so the
// array's other element, a crate function of the same signature, coerces
// to that pointer, and each toggle takes its style from the array.

use events::controls::{primary, secondary, toggle};
use events::widgets::{Column, Widget};
use events::Theme;

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggled(bool),
}

#[derive(Default)]
struct Styling {
    toggled: bool,
}

impl Styling {
    fn update(&mut self, message: Message) {
        match message {
            Message::Toggled(toggled) => {
                self.toggled = toggled;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let styles = [
            ("Primary", primary as fn(&Theme, _) -> _),
            ("Secondary", secondary),
        ];
        Column::with_children(styles.into_iter().map(|(name, style)| {
            toggle(self.toggled)
                .label(name)
                .on_toggle(Message::Toggled)
                .style(style)
        }))
    }
}

fn main() {
    events::controls::application(Styling::default, Styling::update, Styling::view).run()
}
