// rawr
//! A marked leaf under a native root: it builds the root's own types.
use crate::{Item, Kind, Message};

/// A native enum, by value.
pub fn message(count: u32) -> Message {
    if count == 0 {
        Message::Closed
    } else {
        Message::Pressed(count)
    }
}

/// A native struct inside an `Option`.
pub fn maybe(some: bool) -> Option<Item> {
    if some {
        Some(Item::new("one", 1))
    } else {
        None
    }
}

/// Native structs inside a `Vec`.
pub fn items(count: u32) -> Vec<Item> {
    let mut items = Vec::new();
    for index in 0..count {
        items.push(Item::new("row", index));
    }
    items
}

/// A native enum and a number inside a tuple.
pub fn pair(count: u32) -> (Message, u32) {
    (Message::Pressed(count), count * 2)
}

/// A native enum taken by value.
pub fn label(kind: Kind) -> String {
    match kind {
        Kind::Info => "info".to_owned(),
        Kind::Warn => "warn".to_owned(),
    }
}

/// A native struct taken by value and given back.
pub fn grow(mut item: Item) -> Item {
    item.count += 10;
    item
}

/// A native struct changed through `&mut`.
pub fn bump(item: &mut Item) {
    item.count += 1;
    item.name.push('!');
}

/// A native enum replaced through `&mut`.
pub fn raise(kind: &mut Kind) {
    *kind = Kind::Warn;
}

/// A generic type with a lifetime, from another crate, as iced's
/// `Element<'a, Message>`.
pub fn view<'a>(count: u32) -> ui::Element<'a, Message> {
    ui::button(format!("press {count}")).on_press(Message::Pressed(count))
}
