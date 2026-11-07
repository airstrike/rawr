// rawr
// iced's pane_grid (#1190): a pane's body is an opaque widget made of what
// the view borrows, so the pane, `Pane<'a, T, W>` with `W` that opaque
// type, is made in the view's region, and its title bar, another value of
// the region, joins it.

use events::widgets::{button, column, container, field, opaque, pane, text, Widget};

#[derive(Default)]
struct Panes {
    value: i64,
    title: String,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Panes {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
                self.title = format!("pane {}", self.value);
            }
            Message::Decrement => self.value -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let title = container(field("Title").value(&self.title)).padding(1);
        let body = opaque(column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement),
        ]);
        pane(body).title_bar(title).boxed()
    }
}

fn main() {
    events::widgets::run(Panes::update, Panes::view);
}
