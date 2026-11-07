// rawr
// iced's progress_bar (#1083): `transition(value, |at| ..)` keeps a
// closure that makes its content again each time it shows, and the
// view's `column!` boxes it beside a button. The first closure reads the
// state, so it is a closure and not a function; the second reads none.
// Their text is invariant in its lifetime, as iced's `ProgressBar<'a>` is.

use events::controls::application;
use events::widgets::{button, column, text, transition, Widget};

#[derive(Default)]
struct App {
    value: u32,
    step: u32,
}

#[derive(Debug, Clone)]
enum Message {
    Add,
}

impl App {
    fn update(&mut self, message: Message) {
        match message {
            Message::Add => {
                self.value += 10;
                self.step += 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let bar = transition(self.value, |at| text(at).size(at + self.step));
        let label = transition(self.step, |at| text(at));

        column![bar, label, button("Add").on_press(Message::Add)]
    }
}

fn main() {
    application(App::default, App::update, App::view).run()
}
