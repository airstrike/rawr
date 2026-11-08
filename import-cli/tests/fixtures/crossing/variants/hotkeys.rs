// rawr
// iced's pane_grid: `keyboard::listen().filter_map(..)` runs its closure on
// iced's executor thread, which gives `Some(Message::SplitFocused(Axis::
// Vertical))`. The crate values the program's message holds are made
// there and move with it to the program's thread, while `filter_map` reads
// `Some` or `None` at once.

use events::controls::{quote, signals, Axis, Signal, Subscription};
use events::widgets::{column, Widget};

#[derive(Debug, Default)]
struct Panes {
    splits: Vec<Axis>,
}

#[derive(Debug, Clone)]
enum Message {
    SplitFocused(Axis),
    Both(Vec<Axis>),
}

fn handle_hotkey(signal: Signal) -> Option<Message> {
    match signal {
        Signal::Moved => Some(Message::SplitFocused(Axis::Vertical)),
        Signal::Rescaled(scale) if scale > 1.0 => {
            Some(Message::Both(vec![Axis::Horizontal, Axis::Vertical]))
        }
        Signal::Rescaled(_) => None,
    }
}

impl Panes {
    fn update(&mut self, message: Message) {
        match message {
            Message::SplitFocused(axis) => {
                println!("split {axis:?} {}", axis == Axis::Vertical);
                self.splits.push(axis);
            }
            Message::Both(axes) => self.splits.extend(axes),
        }
        println!("splits {:?}", self.splits);
    }

    fn subscription(&self) -> Subscription<Message> {
        signals().filter_map(|signal| handle_hotkey(signal))
    }

    fn view(&self) -> impl Widget<Message> {
        column![quote("Panes")]
    }
}

fn main() {
    events::controls::application(Panes::default, Panes::update, Panes::view)
        .subscription(Panes::subscription)
        .run()
}
