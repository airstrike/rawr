// rawr
//! A marked widget module generic over the message, as inboard's
//! `widget/sidebar.rs`: native code calls it at its own message types,
//! directly and from a generic native helper.
use crate::Element;

/// A sidebar row: a label that gives `message` when pressed.
pub fn row_group<'a, Message: Clone + 'a>(
    label: &'a str,
    message: Message,
) -> Element<'a, Message> {
    ui::label(&format!("{label} row")).on_press(message)
}
