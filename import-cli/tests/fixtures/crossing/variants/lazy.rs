// rawr
// iced's lazy (#328): `lazy(self.version, |_| ..)` keeps a closure that
// makes its widgets again, and is a widget only where what the closure
// gives is `'static`, as `W: Widget + 'static` of its impl says. A
// container takes it, and `column!` boxes that.

use events::controls::application;
use events::widgets::{button, column, container, lazy, Column, Widget};

#[derive(Default)]
struct App {
    version: u8,
    items: Vec<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Add,
    Delete(String),
}

impl App {
    fn update(&mut self, message: Message) {
        self.version = self.version.wrapping_add(1);
        match message {
            Message::Add => self.items.push(format!("item{}", 3 - self.items.len())),
            Message::Delete(item) => self.items.retain(|kept| *kept != item),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let options = lazy(self.version, |_| {
            let mut items = self.items.clone();
            items.sort();
            Column::with_children(
                items
                    .into_iter()
                    .map(|item| button(item.clone()).on_press(Message::Delete(item)).boxed()),
            )
        });

        column![
            container(options).padding(1),
            button("Add").on_press(Message::Add)
        ]
    }
}

fn main() {
    application(App::default, App::update, App::view).run()
}
