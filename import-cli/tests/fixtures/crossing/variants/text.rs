// rawr
// iced's `text!` in the fixture's widgets: the macro's `$crate::widgets::
// Label::new(format!(..))` reaches `Text::new` through an alias that passes
// its parameters on, as iced's `$crate::Text::new` does, and a column's
// `impl IntoIterator` takes a lazy iterator, as iced's text example's
// `column(sizes.map(..))` does. The text is invariant in its lifetime, as
// iced's is, and a column of it is made from a lazy iterator over the
// state, as iced's events example's `column(self.last.iter().map(|event|
// text!("{event:?}").size(40)))` is.

use events::widgets::{button, column, text, Column, Widget};

#[derive(Default)]
struct Counter {
    value: i64,
    pressed: Vec<Message>,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        self.pressed.push(message);
        match message {
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text!("count {} of {}", self.value, 3).size(50),
            Column::with_children((1..=2).map(|step| text!("{}+{step}", self.value))),
            Column::with_children(
                self.pressed
                    .iter()
                    .map(|message| text!("{message:?}").size(40))
            ),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
    }
}

fn main() {
    events::widgets::run(Counter::update, Counter::view);
}
