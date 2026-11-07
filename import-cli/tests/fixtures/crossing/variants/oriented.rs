// rawr
// iced's progress_bar (#1129): a transition made before an `if` moves into
// a `row!` in one branch and a `column!` in the other, each spaced or
// padded, boxed and centered, and the view's `column!` boxes the branch's
// value beside a button that flips it.

use events::controls::application;
use events::widgets::{button, center, column, row, text, transition, Widget};

#[derive(Default)]
struct App {
    value: u32,
    vertical: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Add,
    Flip,
}

impl App {
    fn update(&mut self, message: Message) {
        match message {
            Message::Add => self.value += 10,
            Message::Flip => self.vertical = !self.vertical,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let bar = transition(self.value, |at| {
            text(at).size(at + u32::from(self.vertical))
        });

        column![
            if self.vertical {
                center(
                    row![bar, button("Flip").on_press(Message::Flip)]
                        .spacing(2)
                        .boxed(),
                )
            } else {
                center(
                    column![bar, button("Flip").on_press(Message::Flip)]
                        .padding(3)
                        .boxed(),
                )
            },
            button("Add").on_press(Message::Add)
        ]
    }
}

fn main() {
    application(App::default, App::update, App::view).run()
}
