// rawr
// iced's checkbox in the fixture's controls: `application(State::default,
// ..)` gives an opaque `impl Program` its `fonts` and `run` take, a toggle
// takes an `Option` of a callable and a crate function as its style, and
// its label is a string literal it borrows.

use events::controls::{primary, toggle};
use events::widgets::{column, Widget};

#[derive(Default)]
struct Example {
    default: bool,
    styled: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    DefaultToggled(bool),
    StyledToggled(bool),
}

impl Example {
    fn update(&mut self, message: Message) {
        match message {
            Message::DefaultToggled(default) => {
                self.default = default;
            }
            Message::StyledToggled(styled) => {
                self.styled = styled;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let default_toggle = toggle(self.default)
            .label("Default")
            .on_toggle(Message::DefaultToggled);
        let styled_toggle = toggle(self.styled)
            .label("Styled")
            .on_toggle_maybe(self.default.then_some(Message::StyledToggled))
            .style(primary);
        column![default_toggle, styled_toggle]
    }
}

fn main() {
    events::controls::application(Example::default, Example::update, Example::view)
        .fonts([include_bytes!("../Cargo.toml").as_slice()])
        .run()
}
