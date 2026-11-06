// rawr
// iced's bezier_tool in the fixture: the view is a container of a hover
// whose base is a widget with no lifetime and whose top is `None` until
// there is something to clear, so no value the view passes the hover
// borrows anything. The container, invariant in its lifetime as iced's
// is, is still made at the view's (#352).

use events::widgets::{button, container, hover, rule, Widget};

#[derive(Default)]
struct Board {
    count: u16,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Board {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.count += 1,
            Message::Decrement => self.count -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        container(hover(
            rule(self.count, Message::Increment),
            if self.count == 0 {
                None
            } else {
                Some(button("Decrement").on_press(Message::Decrement))
            },
        ))
        .padding(20)
    }
}

fn main() {
    events::widgets::run(Board::update, Board::view);
}
