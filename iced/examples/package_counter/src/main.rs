// rawr
pub mod counter;

use crate::counter::Counter;

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}
