//! Native, below the private `mod tiles`: a message type no path from the
//! crate root names, at which native code calls the marked generic
//! `row_group`, as inboard's `spreadsheet::sidebar::Message`.
use crate::Element;

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Strip(u32),
}

pub fn strip<'a>(label: &'a str) -> Element<'a, Message> {
    crate::widget::sidebar::row_group(label, Message::Strip(13))
}
