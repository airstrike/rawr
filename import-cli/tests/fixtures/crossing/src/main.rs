// rawr
// The program's own state, message, `update` and `view` cross into a
// native generic `run`, as an iced program's do (D594).

use events::Widget;

#[derive(Default)]
struct Counter {
    value: i64,
    label: String,
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
}

fn update(counter: &mut Counter, message: Message) {
    match message {
        Message::Increment => {
            counter.value += 1;
            counter.label = format!("count {}", counter.value);
        }
    }
}

fn view(counter: &Counter) -> Widget<'_, Message> {
    events::label(&counter.label).on_press(Message::Increment)
}

fn main() {
    events::run(update, view);
}
