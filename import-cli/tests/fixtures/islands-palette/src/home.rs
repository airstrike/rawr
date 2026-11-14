// rawr
//! The home screen, marked, as inboard's `home.rs`: a title made by the
//! crate's `From` impl of its text into an element, beside one made by an
//! extension trait's `.boxed()` (#1896).
use ui::{text, WidgetExt};

use crate::Element;

#[derive(Debug, Clone)]
pub enum Message {
    Opened,
}

/// The title, converted by `.into()` through the crate's `From<Text>`.
pub fn title<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(20).into()
}

/// The subtitle, boxed by the crate's extension trait.
pub fn subtitle<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(12).boxed()
}
