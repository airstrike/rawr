// rawr
// iced's `.subscription(|state| ..)` and `.scale_factor(|_| 2.0)` without
// annotations: each closure's input is `&P::State` of the opaque program
// `application(..)` gives, which its `State = Inferred` binding names, as
// rustc reads it.

use events::controls::{ticks, toggle, Subscription, Theme};
use events::widgets::{column, Widget};

#[derive(Debug, Default)]
struct Inferred {
    heard: u32,
    enabled: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Ticked(u32),
    Toggled(bool),
}

impl Inferred {
    fn update(&mut self, message: Message) {
        match message {
            Message::Ticked(tick) => self.heard += tick,
            Message::Toggled(enabled) => self.enabled = enabled,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![toggle(self.enabled)
            .label("Dark")
            .on_toggle(Message::Toggled)]
    }
}

fn main() {
    events::controls::application(
        Inferred::default,
        |state: &mut Inferred, message| {
            println!("update {message:?} after {}", state.heard);
            state.update(message)
        },
        Inferred::view,
    )
    .subscription(|state| {
        if state.enabled {
            Subscription::none()
        } else {
            ticks(2).map(Message::Ticked)
        }
    })
    // `ThemeFn` is implemented over `Fn`, and rustc deduces no closure
    // signature through it, so this input keeps its annotation.
    .theme(|state: &Inferred| {
        if state.enabled {
            Theme::Dark
        } else {
            Theme::Light
        }
    })
    .scale_factor(|_| 2.0)
    .run()
}
