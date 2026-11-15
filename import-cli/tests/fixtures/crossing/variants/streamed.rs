// rawr
// iced's markdown stream: the view lends a crate method's slice of the
// state's field (`self.plan.parts()`) to a crate call whose widget keeps
// it, with a gallery that borrows the state's other fields and lines the
// slice it is lent, as markdown's `view_with(self.content.items(), ..,
// &CustomViewer { .. })`. The next update grows the field through `&mut`
// while a `&mut` of another field lives (`match &mut self.mode`). The
// view's widget is gone by then, so nothing borrows the field.

use events::controls::time::Instant;
use events::controls::{toggle, Subscription};
use events::framing::{self, Gallery, Shape};
use events::widgets::{button, column, Widget};

struct Streamed {
    plan: framing::Plan,
    label: String,
    mode: Mode,
}

enum Mode {
    Preview,
    Stream { content: String, offset: usize },
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Toggled(bool),
    Next,
}

struct Wall<'a> {
    label: &'a str,
}

impl<'a> Gallery<'a> for Wall<'a> {
    fn shape(&self, side: u32) -> impl Shape<u32> + 'a {
        let shape = framing::square(side);
        framing::framed(shape, framing::caption(self.label))
    }

    fn outline(&self, label: &'a str) -> impl Shape<u32> + 'a {
        let shape = framing::traced(label);
        framing::framed(shape, framing::caption(label))
    }

    fn lines(&self, parts: &'a [framing::Part]) -> impl Shape<u32> + 'a {
        let shape = framing::lined(self, parts, tagged);
        framing::framed(shape, framing::caption(self.label))
    }
}

fn tagged(name: String) -> String {
    format!("#{name}")
}

impl Streamed {
    fn new() -> Self {
        Self {
            plan: framing::Plan::new(vec!["beam".to_owned()]),
            label: "oak".to_owned(),
            mode: Mode::Preview,
        }
    }

    fn update(&mut self, message: Message, _now: Instant) {
        match message {
            Message::Toggled(true) => {
                self.plan = framing::Plan::new(Vec::new());
                self.mode = Mode::Stream {
                    content: "post".to_owned(),
                    offset: 0,
                };
            }
            Message::Toggled(false) => self.mode = Mode::Preview,
            Message::Next => match &mut self.mode {
                Mode::Preview => {}
                Mode::Stream { content, offset } => {
                    if *offset >= content.len() {
                        self.mode = Mode::Preview;
                    } else if let Some(token) = content[*offset..].chars().next() {
                        self.plan.push(&token.to_string());
                        *offset += token.len_utf8();
                    }
                }
            },
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    fn view(&self) -> impl Widget<Message> {
        let board = framing::board(self.plan.parts(), &Wall { label: &self.label });
        column![
            toggle(matches!(self.mode, Mode::Stream { .. }))
                .label("Stream")
                .on_toggle(Message::Toggled),
            button("Next").on_press(Message::Next),
            board,
        ]
    }
}

fn main() {
    events::controls::timed(
        Streamed::new,
        Streamed::update,
        Streamed::subscription,
        Streamed::view,
    )
    .run()
}
