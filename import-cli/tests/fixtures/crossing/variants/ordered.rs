// rawr
use std::collections::BTreeMap;

use events::ordered::{self, Id, Key, Named};

fn hotkey(key: &Key) -> Option<&'static str> {
    match key.as_ref() {
        Key::Named(Named::Space) => Some("toggle"),
        Key::Character("r") => Some("reset"),
        Key::Character(other) => {
            println!("other {other}");
            None
        }
        _ => None,
    }
}

fn main() {
    let mut windows = BTreeMap::new();
    for name in ["first", "second", "third"] {
        windows.insert(Id::unique(), name.to_string());
    }
    let names: Vec<&String> = windows.values().collect();
    println!("windows {names:?}");
    let first = *windows.keys().next().unwrap();
    println!("first {first:?} {}", windows[&first]);
    for key in [
        ordered::named(Named::Space),
        ordered::character("r"),
        ordered::character("q"),
        ordered::named(Named::Escape),
        ordered::unidentified(),
    ] {
        println!("{:?}", hotkey(&key));
    }
}
