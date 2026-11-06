// rawr
// iced's table and arc in the fixture: the rows a crate takes are script
// values whose fields hold std's `Duration` and `Instant`, which cross as
// Rust's own types.

use std::time::{Duration, Instant};

use events::conversions::{self, table};
use events::widgets::{column, Widget};

struct Event {
    name: String,
    duration: Duration,
    seen: Instant,
}

struct Table {
    events: Vec<Event>,
}

impl Default for Table {
    fn default() -> Self {
        let seen = Instant::now();
        Table {
            events: vec![
                Event {
                    name: String::from("Talk"),
                    duration: Duration::from_secs(45 * 60),
                    seen,
                },
                Event {
                    name: String::from("Workshop"),
                    duration: Duration::from_secs(120 * 60),
                    seen,
                },
            ],
        }
    }
}

#[derive(Debug, Clone)]
enum Message {}

impl Table {
    fn update(&mut self, message: Message) {
        match message {}
    }

    fn view(&self) -> impl Widget<Message> {
        let columns = [
            conversions::column("Name", |event: &Event| event.name.clone()),
            conversions::column("Time", |event: &Event| {
                let fresh = event.seen.elapsed() < Duration::from_secs(60);
                format!("{} min {fresh}", event.duration.as_secs() / 60)
            }),
        ];
        column![table(columns, &self.events)]
    }
}

fn main() {
    events::controls::application(Table::default, Table::update, Table::view).run()
}
