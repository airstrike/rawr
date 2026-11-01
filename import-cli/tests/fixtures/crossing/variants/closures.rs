// rawr
// iced's `.theme(|state| ..)`, `.subscription(|state| ..)`,
// `.scale_factor(|_: &State| 2.0)` and an update closure: closures whose
// inputs borrow the program's own state, `&State` and `&mut State`, cross
// as its methods do.

use events::controls::{ticks, toggle, Subscription, Theme};
use events::widgets::{column, Widget};

#[derive(Debug, Default)]
struct Closures {
    heard: u32,
    enabled: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Ticked(u32),
    Toggled(bool),
}

impl Closures {
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
        Closures::default,
        |state: &mut Closures, message| {
            println!("update {message:?} after {}", state.heard);
            state.update(message)
        },
        Closures::view,
    )
    .subscription(|state: &Closures| {
        if state.enabled {
            Subscription::none()
        } else {
            ticks(2).map(Message::Ticked)
        }
    })
    .theme(|state: &Closures| {
        if state.enabled {
            Theme::Dark
        } else {
            Theme::Light
        }
    })
    .scale_factor(|_: &Closures| 2.0)
    .run()
}
