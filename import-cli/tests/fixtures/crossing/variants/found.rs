// rawr
// iced's delineate and scrollable: `selector::find(OUTER)` takes the
// widget `Id` through `Selector`, a trait with an associated output that
// the crate also implements for `FnMut` closures, and its task maps the
// `Option<Target>` it finds into a message. A palette field's `Color`,
// whose type the program names by no path, converts into a `Background`
// by `.into()`, through `Background`'s `From<Color>`.

use events::controls::{caption, toggle};
use events::conversions::{Background, Id};
use events::selector::{self, palette, Target};
use events::widgets::{column, Widget};
use events::Task;

struct Found {
    width: Option<f32>,
    fill: Background,
}

#[derive(Debug, Clone)]
enum Message {
    Found(Option<Target>),
    Refresh(bool),
}

impl Found {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                width: None,
                fill: palette().strong.color.into(),
            },
            selector::find(Id::from("outer")).map(Message::Found),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Found(target) => {
                self.width = target.as_ref().and_then(Target::width);
                Task::none()
            }
            Message::Refresh(_) => {
                self.width = None;
                selector::find(Id::from("outer")).map(Message::Found)
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let toggle = toggle(self.width.is_some())
            .label("Refresh")
            .on_toggle(Message::Refresh);
        column![toggle, caption(format!("{:?} {:?}", self.width, self.fill))]
    }
}

fn main() {
    events::controls::application(Found::new, Found::update, Found::view).run()
}
