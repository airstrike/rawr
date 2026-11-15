// rawr
// A simulator of the program's own widget, whose message is a crate type,
// as iced_test's `Simulator<'_, Never>` of a custom widget is: the
// simulator's type names no script type, yet each `find` and `simulate`
// calls the program's `labels` and `press` back through the widget it
// holds erased.

use events::simulator::{self, simulator, Event, Simulator};
use events::widgets::{Renderer, Widget};
use events::Theme;

struct Rainbow {
    bands: u32,
}

impl<T, R> Widget<Event, T, R> for Rainbow {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!("bands {}", self.bands));
    }

    fn press(&self, label: &str) -> Option<Event> {
        (label == "bands 7").then_some(Event::Pressed)
    }
}

fn main() {
    let mut ui: Simulator<'_, Event, Theme, Renderer> = simulator(Rainbow { bands: 7 });
    println!("found {:?}", ui.find("bands 7").is_ok());
    ui.point_at("bands 7");
    println!("simulated {:?}", ui.simulate(simulator::click()));
    for message in ui.into_messages() {
        println!("message {message:?}");
    }
}
