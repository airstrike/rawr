// rawr
// iced's solar_system in the fixture's controls: `timed(..)` gives an
// opaque `impl Program<Message = (Message, Instant)>`, whose message is a
// tuple, and `.theme(..)` gives another of the same message, read from the
// first's binding as `P::Message`.

use events::controls::time::Instant;
use events::controls::{toggle, Subscription, Theme};
use events::widgets::{column, Widget};

#[derive(Debug, Default)]
struct Solar {
    ticks: u32,
    dark: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggled(bool),
}

impl Solar {
    fn update(&mut self, message: Message, _now: Instant) {
        match message {
            Message::Toggled(dark) => {
                self.dark = dark;
                self.ticks += 1;
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    fn view(&self) -> impl Widget<Message> {
        column![toggle(self.dark).label("Dark").on_toggle(Message::Toggled)]
    }

    fn theme(&self) -> Theme {
        if self.dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }
}

fn main() {
    events::controls::timed(
        Solar::default,
        Solar::update,
        Solar::subscription,
        Solar::view,
    )
    .theme(Solar::theme)
    .run()
}
