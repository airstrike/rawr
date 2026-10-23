// rawr
use baseline_native::alternate;
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    #[preview(name = "Phase 9 Baseline", size = (96, 64))]
    fn view(&self) -> Element<Message> { text(alternate()) }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
