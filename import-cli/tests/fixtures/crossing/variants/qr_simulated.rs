// rawr
// iced's qr_code in a view the program calls itself, out of any slot, as
// a test drives iced_test's simulator: the view borrows the state's code
// data, the simulator drops with its block, and `update` then changes the
// data in place, as rustc lets it once no view borrows it.

use events::controls::{qr_code, toggle};
use events::simulator::{self, simulator, Simulator};
use events::widgets::{column, Renderer, Widget};
use events::Theme;

struct Generator {
    data: qr_code::Data,
}

#[derive(Debug, Clone)]
enum Message {
    Grow(bool),
}

impl Generator {
    fn update(&mut self, message: Message) {
        match message {
            Message::Grow(_) => self.data.grow(),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            toggle(false).on_toggle(Message::Grow).label("Grow"),
            qr_code(&self.data).cell_size(10.0),
        ]
    }
}

fn main() {
    let mut generator = Generator {
        data: qr_code::Data::new("rawr").unwrap(),
    };
    for _ in 0..2 {
        let messages: Vec<_> = {
            let mut ui: Simulator<'_, Message, Theme, Renderer> = simulator(generator.view());
            ui.point_at("Grow");
            println!("simulated {:?}", ui.simulate(simulator::click()));
            ui.into_messages().collect()
        };
        for message in messages {
            generator.update(message);
        }
    }
    let mut ui: Simulator<'_, Message, Theme, Renderer> = simulator(generator.view());
    println!("grown {}", ui.find("qr 13@10 draw 1").is_ok());
}
