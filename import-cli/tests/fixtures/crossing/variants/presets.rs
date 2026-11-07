// rawr
// iced's todos: `Application::presets` takes `impl IntoIterator<Item =
// Preset<P::State, P::Message>>`, whose item names the opaque program's own
// state and message. A program function gives the presets as an `impl
// IntoIterator` of them, each booting the state by a closure, and the
// crate boots each one and shows its view.

use events::controls::{quote, Preset, Subscription};
use events::widgets::{column, Widget};
use events::Task;

#[derive(Debug)]
enum Todos {
    Loading,
    Loaded(Vec<String>),
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Cleared,
}

impl Todos {
    fn new() -> (Self, Task<Message>) {
        (Todos::Loading, Task::none())
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Cleared => *self = Todos::Loaded(Vec::new()),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    fn view(&self) -> impl Widget<Message> {
        let shown = match self {
            Todos::Loading => quote("Loading..."),
            Todos::Loaded(tasks) => match tasks.first() {
                Some(task) => quote(task),
                None => quote("Nothing to do"),
            },
        };
        column![shown]
    }
}

fn presets() -> impl IntoIterator<Item = Preset<Todos, Message>> {
    [
        Preset::new("Empty", || (Todos::Loaded(Vec::new()), Task::none())),
        Preset::new("Carl Sagan", || {
            (
                Todos::Loaded(vec!["Create the universe".to_owned()]),
                Task::none(),
            )
        }),
    ]
    .into_iter()
}

fn main() {
    events::controls::application(Todos::new, Todos::update, Todos::view)
        .subscription(Todos::subscription)
        .presets(presets())
        .run()
}
