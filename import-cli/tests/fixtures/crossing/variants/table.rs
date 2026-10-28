// rawr
// iced's table in the fixture: `table(columns, &self.events)` takes the
// borrowed `Vec` as its rows, whose items are `&Event`, as the columns'
// closures `|event: &Event| ..` take them. The script's `Event` is a
// name the runner's own code used too.

use events::controls::caption;
use events::conversions::{self, table};
use events::widgets::{column, Widget};

struct Event {
    name: String,
    minutes: u32,
}

struct Table {
    events: Vec<Event>,
}

impl Default for Table {
    fn default() -> Self {
        Table {
            events: vec![
                Event {
                    name: String::from("Talk"),
                    minutes: 45,
                },
                Event {
                    name: String::from("Workshop"),
                    minutes: 120,
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
            conversions::column("Time", |event: &Event| format!("{} min", event.minutes)),
        ];
        column![caption("Events"), table(columns, &self.events)]
    }
}

fn main() {
    events::controls::application(Table::default, Table::update, Table::view).run()
}
