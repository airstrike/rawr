//! A game built on `events`, as elevato is on iced: its `view` gives a
//! widget known only as `events`'s extension trait `Framed`, whose
//! defaulted `Renderer` this crate can't name (its backends are private to
//! `events`, as iced's renderer is to iced), and its `Message` is another
//! module's, re-exported, as elevato's is elevato_api's.

use events::widgets::{Framed, Widget};

pub use events::elevated::{Building, Message};

/// The floors of a building, as a label.
struct Floors(u8);

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Floors {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!("game floors {}", self.0));
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

/// `floors` floors drawn, as elevato's `view<'a, Message: 'a>(building:
/// &Building) -> impl Widget<Message> + 'a`: only the output names
/// `Message`. It takes the floors by value, so its output borrows nothing
/// (#965 is one that borrows a field of a slot's lent input).
pub fn view<'a, Message: 'a>(floors: u8) -> impl Framed<Message> + 'a {
    Floors(floors)
}
