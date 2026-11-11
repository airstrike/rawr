// rawr
// iced_test's `ui.click(|candidate| ..)` (#1694): `click` is generic over a
// `Selector` the crate implements for any `FnMut(Candidate<'_>) ->
// Option<T>` closure, and a closure over the lifetime-carrying candidate
// fills it, one capturing what it looks for.

use events::simulator::{simulator, Candidate, Simulator};
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

fn labelled(candidate: Candidate<'_>, wanted: &str) -> Option<String> {
    let label = candidate
        .label()
        .trim_start_matches('[')
        .trim_end_matches(']');
    (label == wanted).then(|| label.to_owned())
}

fn main() {
    let mut counter = Counter::default();
    let mut ui: Simulator<'_, Message, Theme, Renderer> = simulator(counter.view());
    println!(
        "click {:?}",
        ui.click(|candidate: Candidate<'_>| labelled(candidate, "Increment"))
    );
    let wanted = String::from("Decrement");
    println!(
        "click {:?}",
        ui.click(move |candidate: Candidate<'_>| labelled(candidate, &wanted))
    );
    println!(
        "click {:?}",
        ui.click(|candidate: Candidate<'_>| labelled(candidate, "Reset"))
    );
    for message in ui.into_messages() {
        println!("message {message:?}");
        counter.update(message);
    }
    println!("value {}", counter.value);
}
