// rawr
// iced's table, whose view's `column!` holds only the table: a widget
// with no lifetime, made in the view's region from what the view borrows
// (#352).

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
        column![table(columns, &self.events)]
    }
}

fn main() {
    events::controls::application(Table::default, Table::update, Table::view).run()
}
