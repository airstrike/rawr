// rawr
// iced's text, system_information and multi_window: `new` gives the state
// and its first task, `(Self, Task<Message>)`, which `application(..)`
// takes through `BootFn` and the tuple's `IntoBoot`, and `update` calls
// `Self::new()` again and keeps its task.

use events::controls::{caption, toggle};
use events::widgets::{column, Widget};
use events::Task;

struct Boot {
    value: u32,
    loaded: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Loaded(u32),
    Refresh(bool),
}

impl Boot {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                value: 0,
                loaded: false,
            },
            Task::done(Message::Loaded(7)),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(value) => {
                self.value += value;
                self.loaded = true;
                Task::none()
            }
            Message::Refresh(_) => {
                let (state, refresh) = Self::new();
                *self = state;
                refresh
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let toggle = toggle(self.loaded)
            .label("Refresh")
            .on_toggle(Message::Refresh);
        column![toggle, caption(self.value)]
    }
}

fn main() {
    events::controls::application(Boot::new, Boot::update, Boot::view).run()
}
