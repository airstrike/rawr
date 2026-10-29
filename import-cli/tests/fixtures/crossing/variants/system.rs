// rawr
// iced's system_information: `new` gives `Loading` and the task of
// reading the system's information, whose crate value
// `.map(Message::InformationReceived)` takes on another thread, as iced's
// executor runs it. The message is made where it is delivered, and a
// variant's field holds the crate value, whose fields the view reads.

use events::controls::{caption, toggle};
use events::widgets::{column, Widget};
use events::{information, Info, Task};

#[derive(Default)]
enum Example {
    #[default]
    Loading,
    Loaded {
        information: Info,
    },
}

#[derive(Debug, Clone)]
enum Message {
    InformationReceived(Info),
    Refresh(bool),
}

impl Example {
    fn new() -> (Self, Task<Message>) {
        (
            Self::Loading,
            information().map(Message::InformationReceived),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh(_) => {
                let (state, refresh) = Self::new();
                *self = state;
                refresh
            }
            Message::InformationReceived(information) => {
                *self = Self::Loaded { information };
                Task::none()
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let shown = match self {
            Example::Loading => caption("Loading..."),
            Example::Loaded { information } => caption(format!(
                "{} with {} cores",
                information.name,
                information
                    .cores
                    .map_or("unknown".to_string(), |cores| cores.to_string())
            )),
        };
        let refresh = toggle(false).label("Refresh").on_toggle(Message::Refresh);
        column![refresh, shown]
    }
}

fn main() {
    events::controls::application(Example::new, Example::update, Example::view).run()
}
