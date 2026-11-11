// rawr
// iced_test's `ui.click("Increment")` (#1678): `click` is generic over a
// `Selector` the crate implements for `&str` and for `FnMut(Candidate<'_>)`
// closures, and a string literal fills it as itself, which no closure form
// is needed for.

use events::simulator::{simulator, Simulator};
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
    println!("click {:?}", ui.click("Increment"));
    println!("click {:?}", ui.click("Increment"));
    println!("click {:?}", ui.click("Decrement"));
    println!("click {:?}", ui.click("Reset"));
    for message in ui.into_messages() {
        println!("message {message:?}");
        counter.update(message);
    }
    println!("value {}", counter.value);
}
