// rawr
// iced's layout: each example is a `fn() -> Element<'static, Message>` in a
// constant table, and the view calls the current one through its pointer.
// `column_` makes a `column![..]` of boxed children, sets its padding and
// boxes it, as layout's `column![..].spacing(40).boxed()`.

use events::widgets::{button, column, rule, text, Element, Widget};

#[derive(Debug, Clone, Copy)]
struct Example {
    title: &'static str,
    view: fn() -> Element<'static, Message>,
}

impl Example {
    const LIST: &'static [Self] = &[
        Self {
            title: "Ruled",
            view: ruled,
        },
        Self {
            title: "Column",
            view: column_,
        },
    ];

    fn next(self, step: usize) -> Self {
        Self::LIST[step % Self::LIST.len()]
    }

    fn view(&self) -> Element<'_, Message> {
        (self.view)()
    }
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

struct Layout {
    step: usize,
    example: Example,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            step: 0,
            example: Example::LIST[0],
        }
    }
}

impl Layout {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.step += 1,
            Message::Decrement => self.step = 0,
        }
        self.example = self.example.next(self.step);
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            text(self.example.title),
            button("Increment").on_press(Message::Increment),
            self.example.view(),
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn ruled() -> Element<'static, Message> {
    rule(1, Message::Increment).boxed()
}

fn column_<'a>() -> Element<'a, Message> {
    column![
        text("A column can be used to"),
        rule(2, Message::Increment),
        rule(3, Message::Increment),
    ]
    .padding(40)
    .boxed()
}

fn main() {
    events::widgets::run(Layout::update, Layout::view);
}
