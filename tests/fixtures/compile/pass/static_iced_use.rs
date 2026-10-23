use iced::Center;
use iced::widget::{Column, button};
enum Message { Tick }
struct Screen { value: Column<'_, Message> }
