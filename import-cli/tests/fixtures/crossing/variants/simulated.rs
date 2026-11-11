// rawr
// iced_test's `ui.simulate(click())` (#1679): `simulate` takes `impl
// IntoIterator<Item = Event>`, and the opaque `impl Iterator<Item =
// Event>` that `click()` gives fills it as itself, as every iterator is
// its own `IntoIterator`.

use events::simulator::{self, simulator, Simulator};
use events::widgets::{button, column, text, Renderer, Widget};
use events::Theme;

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    let mut counter = Counter::default();
    let mut ui: Simulator<'_, Message, Theme, Renderer> = simulator(counter.view());
    ui.point_at("Increment");
    println!("simulated {:?}", ui.simulate(simulator::click()));
    ui.point_at("Decrement");
    let events = simulator::click();
    println!("simulated {:?}", ui.simulate(events));
    println!("simulated {:?}", ui.simulate(simulator::click()));
    for message in ui.into_messages() {
        println!("message {message:?}");
        counter.update(message);
    }
    println!("value {}", counter.value);
}
