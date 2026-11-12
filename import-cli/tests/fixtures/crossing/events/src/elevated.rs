//! elevato's `view`, in each of the shapes that set it apart from
//! `widgets::plaque`: a lifetime the output outlives, an output known only
//! as the crate's extension trait `Framed`, and a `Message` of the crate's
//! own that a program's method imports. Each takes its floors by value, so
//! its output borrows nothing (#361 is a borrowing one moved into a
//! widget). The `game` crate beside this one gives all of them at once,
//! as elevato does over iced.

use crate::widgets::{Framed, Widget};

/// What a view shows.
#[derive(Debug, Default)]
pub struct Building {
    /// Its floors.
    pub floors: u8,
}

/// The crate's message, as elevato's, which a program imports.
#[derive(Debug, Clone, Copy)]
pub enum Message {
    /// A press of the call button.
    Call,
}

/// The floors of a building, as a label.
struct Floors(u8);

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Floors {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!("floors {}", self.0));
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

/// `floors` floors, outliving `'a`: only the output names `Message`.
pub fn floors<'a, Message: 'a>(floors: u8) -> impl Widget<Message> + 'a {
    Floors(floors)
}

/// `floors` floors, known only as the extension trait `Framed`, whose
/// defaulted parameters the caller's bound leaves out.
pub fn shaft<Message>(floors: u8) -> impl Framed<Message> {
    Floors(floors)
}

/// `floors` floors, as a plain `Widget` with no lifetime.
pub fn lobby<Message>(floors: u8) -> impl Widget<Message> {
    Floors(floors)
}
