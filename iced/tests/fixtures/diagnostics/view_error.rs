// rawr
use iced::Element;

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}

#[derive(Default)]
struct Counter { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Tick }

impl Counter {
    fn update(&mut self, message: Message) {
        match message { Message::Tick => self.value += 1 }
    }

    fn view(&self) -> Element<'_, Message> {
        panic!("initial view failed")
    }
}
