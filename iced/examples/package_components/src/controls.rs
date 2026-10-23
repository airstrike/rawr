// rawr
use iced::{Element, Widget};
use iced::widget::button;

#[derive(Debug, Clone, Copy)]
pub enum ControlsMessage {
    Increment,
}

pub fn controls() -> Element<'static, ControlsMessage> {
    button("Increment child").on_press(ControlsMessage::Increment).boxed()
}
