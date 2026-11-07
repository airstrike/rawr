// rawr
// iced's lazy (#328): `lazy(self.version, |_| ..)` keeps a closure that
// makes its widgets again, and is a widget only where what the closure
// gives is `'static`, as `W: Widget + 'static` of its impl says. Each row
// the closure makes has a `button("Delete")`, whose literal is a `&'static
// str`, as in iced's lazy. A container takes the lazy widget, and
// `column!` boxes that.

use events::controls::application;
use events::widgets::{button, column, container, lazy, row, text, Column, Widget};

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
            Message::Delete(item) => {
                if let Some(at) = self.items.iter().position(|kept| *kept == item) {
                    self.items.remove(at);
                }
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let options = lazy(self.version, |_| {
            let mut items = self.items.clone();
            items.sort();
            Column::with_children(items.into_iter().map(|item| {
                let delete = button("Delete").on_press(Message::Delete(item.clone()));
                row![text(item), delete].boxed()
            }))
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
