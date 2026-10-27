// rawr
// The program's own state, message and methods cross into a native
// generic `application` through traits over `Fn`, as an iced program's
// `iced::run(Counter::update, Counter::view)` do (D594).

use events::Column;

#[derive(Default)]
struct Counter {
    value: i64,
    label: String,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
                self.label = format!("count {}", self.value);
            }
        }
    }

    fn view(&self) -> Column<'_, Message> {
        events::column(&self.label).on_press(Message::Increment)
    }
}

fn main() {
    events::application(Counter::update, Counter::view);
}
