// rawr
// iced's events and url_handler in the fixture's controls: the opaque
// `impl Program` that `application(..)` gives takes a subscription of its
// state, whose input is `&P::State` and whose output is a
// `Subscription<P::Message>`, and gives another opaque program of the same
// state and message that `fonts` and `run` take.

use events::controls::{caption, ticks, toggle, Subscription};
use events::widgets::{column, Widget};

#[derive(Debug, Default)]
struct Events {
    heard: u32,
    enabled: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Ticked(u32),
    Toggled(bool),
}

impl Events {
    fn update(&mut self, message: Message) {
        match message {
            Message::Ticked(tick) => {
                self.heard += tick;
            }
            Message::Toggled(enabled) => {
                self.enabled = enabled;
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.enabled {
            Subscription::none()
        } else {
            ticks(3).map(Message::Ticked)
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let toggle = toggle(self.enabled)
            .label("Listen")
            .on_toggle(Message::Toggled);
        column![toggle, caption(self.heard)]
    }
}

fn main() {
    events::controls::application(Events::default, Events::update, Events::view)
        .subscription(Events::subscription)
        .fonts([include_bytes!("../Cargo.toml").as_slice()])
        .run()
}
