// rawr
use iced::Theme;
use iced::Element;
use iced::widget::{button, column, text};

#[derive(Default)]
struct Counter { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Increment }

impl Counter {
    fn forty_two() -> Counter { Counter { value: 42 } }
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.value += 1 }
    }
    fn light(&self) -> Theme { Theme::Light }
    fn dark() -> Theme { Theme::Dark }

    #[preview(name = "default", group = "counter", themes = [Counter::light, Counter::dark], size = (640, 480))]
    #[preview(name = "forty two", group = "counter", seed = Counter::forty_two, snapshot = false, size = (320, 240))]
    fn view(&self) -> Element<'_, Message> {
        let increment = button("Increment").on_press(Message::Increment);
        let value = text(self.value);
        column![value, increment]
    }
}

pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
