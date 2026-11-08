// rawr
use natives::pressable::{labeled, pressable, Plain, Pressable};

enum Message {
    Delete,
    Keep,
}

fn main() {
    let name = String::from("Delete");
    let delete: Pressable<'_, Message, &str, Plain> = pressable(name.as_str());
    println!("{}", delete.describe());
    let other = String::from("Keep");
    let keep: Pressable<'_, Message, &str, Plain> = labeled(&other);
    println!("{}", keep.describe());
    for message in [Message::Delete, Message::Keep] {
        match message {
            Message::Delete => println!("deleted"),
            Message::Keep => println!("kept"),
        }
    }
}
