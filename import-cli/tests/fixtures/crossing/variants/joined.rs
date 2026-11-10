// rawr
// Two `let`s of an invariant text that borrows a string, as iced's
// `Text<'a>` is through its `Theme::Class<'a>`, passed to one call, and two
// `let`s of what `explain` gives on it, an opaque `impl Widget`, passed to
// another: Rust infers one lifetime for both borrows, in a view and out of
// one. The element the first call boxes is lent to a crate function by `&`
// and to its own `labels(&mut Vec<String>)` (#1366, #1369), and two values
// of one opaque type go to one call (#1368).

use events::widgets::{
    button, column, hover, rule, shown, shown_by_ref, span, text, Column, Element, Widget,
};
use events::Framed;

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let title = span("count");
        let unit = span("presses");
        let first = span("first").explain(0x0000ff_u32);
        let second = rule(2, Message::Decrement).explain(0x00ff00_u32);
        column![
            button("Increment").on_press(Message::Increment),
            Column::with_children([title, unit]),
            text(self.value).size(50),
            column![first, second],
            button("Decrement").on_press(Message::Decrement),
        ]
    }
}

fn main() {
    // Out of any view, the first `let` of a value that borrows what it
    // takes opens a scope frame, and each later one joins its region.
    let name = String::from("left");
    let left = span(&name);
    let right = span("right");
    let column: Element<'_, Message> = Column::with_children([left, right]).boxed();
    let mut labels = Vec::new();
    column.labels(&mut labels);
    println!("main {} {labels:?}", shown_by_ref(&column));
    println!("main {}", shown(column));
    let below = rule(1, Message::Increment).explain(0x0000ff_u32);
    let above = rule(2, Message::Decrement).explain(0x00ff00_u32);
    println!("main {}", shown(hover(below, above).boxed()));
    let left = span(&name).explain(0x0000ff_u32);
    let right = span("right").explain(0x00ff00_u32);
    let column: Element<'_, Message> = column![left, right].boxed();
    println!("main {}", shown(column));
    events::widgets::run(Counter::update, Counter::view);
}
