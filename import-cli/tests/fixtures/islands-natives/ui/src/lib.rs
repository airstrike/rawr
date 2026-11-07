//! A widget tree with a lifetime and a message type, as iced's `Element`.

use std::borrow::Cow;
use std::fmt::Debug;

/// A labelled button that may send a message.
pub struct Element<'a, Message> {
    label: Cow<'a, str>,
    on_press: Option<Message>,
}

/// A button labelled `label`, which sends nothing yet.
pub fn button<'a, Message>(label: String) -> Element<'a, Message> {
    Element {
        label: Cow::Owned(label),
        on_press: None,
    }
}

impl<'a, Message: Debug> Element<'a, Message> {
    /// The button, sending `message` when pressed.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// What the button shows and sends.
    pub fn describe(&self) -> String {
        format!("[{}] -> {:?}", self.label, self.on_press)
    }
}
