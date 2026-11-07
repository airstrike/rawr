// rawr
// iced's progress_bar with a view closure that captures nothing (#1130):
// `animate(start, value, |spring, at| ..)` lends the closure a crate value,
// as `transition`'s view takes `&Animation<f32>`, and the closure gives a
// text invariant in its lifetime, as iced's `ProgressBar<'a>` is. A closure
// that captures nothing is a function, which native code calls with the
// spring it lends.

use events::controls::application;
use events::widgets::{animate, button, column, text, Widget};

#[derive(Default)]
struct App {
    value: u32,
}

#[derive(Debug, Clone)]
enum Message {
    Add,
}

impl App {
    fn update(&mut self, message: Message) {
        match message {
            Message::Add => self.value += 10,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let bar = animate(2, self.value, |spring, at| text(spring.at(at)).size(at));

        column![bar, button("Add").on_press(Message::Add)]
    }
}

fn main() {
    application(App::default, App::update, App::view).run()
}
