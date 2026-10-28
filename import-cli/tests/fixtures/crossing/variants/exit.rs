// rawr
// iced's exit and events in the fixture's controls: `update` gives back a
// task, and `latest().and_then(..)` takes a script closure, or the
// generic crate function `close` as iced's `and_then(window::close)` does,
// on a `Task<Option<u32>>`, whose `and_then` is a method of `impl<T>
// Task<Option<T>>`, beside another of `impl<T, E> Task<Result<T, E>>`.

use events::controls::{caption, toggle};
use events::widgets::{column, Widget};
use events::{close, latest, Task};

#[derive(Default)]
struct Exit {
    show_confirm: bool,
    closed: u32,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Confirm(bool),
    Closed(u32),
}

impl Exit {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Confirm(confirm) => {
                self.show_confirm = confirm;
                if confirm {
                    latest().and_then(|id| Task::done(Message::Closed(id)))
                } else {
                    latest().and_then(close)
                }
            }
            Message::Closed(id) => {
                self.closed = id;
                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let toggle = toggle(self.show_confirm)
            .label("Exit")
            .on_toggle(Message::Confirm);
        column![toggle, caption(self.closed)]
    }
}

fn main() {
    events::controls::application(Exit::default, Exit::update, Exit::view).run()
}
