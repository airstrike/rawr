// rawr
// A `for` loops over a crate's value of the program's own type by its
// `IntoIterator` impl, as integration's `for message in messages` loops
// over iced's `shell::Bus<controls::Message>`.
use natives::bags::Bag;

#[derive(Debug)]
enum Message {
    Renamed(String),
    Cleared,
}

fn main() {
    let mut bag = Bag::new();
    bag.push(Message::Renamed("sea".to_owned()));
    bag.push(Message::Cleared);
    for message in bag {
        println!("{message:?}");
    }
    let mut again = Bag::new();
    again.push(Message::Cleared);
    for message in again.into_iter() {
        println!("{message:?}");
    }
}
