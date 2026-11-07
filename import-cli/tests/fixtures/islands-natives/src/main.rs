//! Native `main`, as an iced app's: it defines its own `Message`, `Kind`
//! and `Item`, and the marked leaf `badge` gives them back and takes them,
//! by value, inside an `Option`, a `Vec` and a tuple, and by `&mut`, and
//! builds a `ui::Element<'a, Message>`.
mod badge;

/// What the app's widgets send.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Pressed(u32),
    Closed,
}

/// How loud a badge is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Info,
    Warn,
}

/// One row of the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub count: u32,
}

impl Item {
    pub fn new(name: &str, count: u32) -> Item {
        Item {
            name: name.to_owned(),
            count,
        }
    }
}

fn main() {
    println!("message {:?}", badge::message(3));
    println!("message {:?}", badge::message(0));
    println!("maybe {:?}", badge::maybe(true));
    println!("maybe {:?}", badge::maybe(false));
    println!("items {:?}", badge::items(2));
    println!("pair {:?}", badge::pair(4));
    println!("label {}", badge::label(Kind::Warn));
    println!("label {}", badge::label(Kind::Info));
    let grown = badge::grow(Item::new("b", 5));
    println!("grown {grown:?}");
    let mut item = Item::new("a", 1);
    badge::bump(&mut item);
    badge::bump(&mut item);
    println!("bumped {item:?}");
    let mut kind = Kind::Info;
    badge::raise(&mut kind);
    println!("raised {kind:?}");
    let element: ui::Element<'_, Message> = badge::view(7);
    println!("view {}", element.describe());
}
