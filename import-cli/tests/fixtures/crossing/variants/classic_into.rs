// rawr
// A view of text alone in iced's earlier widgets, made into its element by
// `.into()`: only the view's return type names its message, theme and
// renderer, as rustc infers them from it.

use events::classic::{center, text, Element};

#[derive(Default)]
struct Hello;

#[derive(Debug, Clone, Copy)]
enum Message {}

impl Hello {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> Element<'_, Message> {
        center(text("hi")).into()
    }
}

fn main() {
    events::widgets::run(Hello::update, Hello::view);
}
