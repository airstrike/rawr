// rawr
// iced's tooltip in the fixture's controls: the state derives `Default`
// over a field of a crate enum whose `Default` the crate derives, a match
// reads it, and a box of a string hint takes a crate function
// `fn(&Theme) -> Style` as its style.

use events::controls::{boxed, rounded_box, toggle, Position};
use events::widgets::{column, Widget};

#[derive(Default)]
struct Tooltip {
    position: Position,
    delay: bool,
}

#[derive(Debug, Clone)]
enum Message {
    ToggleDelay(bool),
}

impl Tooltip {
    fn update(&mut self, message: Message) {
        match message {
            Message::ToggleDelay(delay) => {
                self.delay = delay;
                self.position = match self.position {
                    Position::Top => Position::Bottom,
                    Position::Bottom => Position::Top,
                };
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let hint = boxed(position_to_text(self.position)).style(rounded_box);
        let delay = toggle(self.delay)
            .label("Delay")
            .on_toggle(Message::ToggleDelay);
        column![hint, delay]
    }
}

fn position_to_text(position: Position) -> &'static str {
    match position {
        Position::Top => "Top",
        Position::Bottom => "Bottom",
    }
}

fn main() {
    events::controls::application(Tooltip::default, Tooltip::update, Tooltip::view).run()
}
