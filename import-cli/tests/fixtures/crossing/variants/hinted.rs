// rawr
// iced's `tooltip(button(..), label, ..)` in a view the program calls
// itself, out of any slot, as a test drives iced_test's simulator: the
// button carries a lifetime, the `&str` hint fills a type parameter the
// output keeps, and both join at `'static`, as rustc types a literal.

use events::controls::hinted;
use events::simulator::{self, simulator, Simulator};
use events::widgets::{button, Renderer, Widget};
use events::Theme;

#[derive(Default)]
struct Hints {
    presses: u32,
}

#[derive(Debug, Clone)]
enum Message {
    Pressed,
}

impl Hints {
    fn update(&mut self, message: Message) {
        match message {
            Message::Pressed => self.presses += 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        hinted(
            button("Press").on_press(Message::Pressed),
            hint(self.presses),
        )
    }
}

fn hint<'a>(presses: u32) -> &'a str {
    if presses == 0 {
        "never"
    } else {
        "pressed"
    }
}

fn main() {
    let mut hints = Hints::default();
    for _ in 0..2 {
        let messages: Vec<_> = {
            let mut ui: Simulator<'_, Message, Theme, Renderer> = simulator(hints.view());
            println!("never {}", ui.find("hint never").is_ok());
            ui.point_at("Press");
            println!("simulated {:?}", ui.simulate(simulator::click()));
            ui.into_messages().collect()
        };
        for message in messages {
            hints.update(message);
        }
    }
    println!("presses {}", hints.presses);
}
