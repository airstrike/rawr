// rawr
use events::keys::{self, Event, Key, Named};

fn describe(event: Event) -> String {
    match event {
        Event::KeyPressed {
            key: Key::Named(Named::Tab),
            shift,
        } => {
            if shift {
                "back".to_string()
            } else {
                "forward".to_string()
            }
        }
        Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        } => "close".to_string(),
        Event::KeyPressed {
            key: Key::Character(text),
            ..
        } => format!("typed {}", text.as_str()),
        _ => "other".to_string(),
    }
}

fn main() {
    println!("{}", describe(keys::press(Key::Named(Named::Tab), true)));
    println!("{}", describe(keys::press(Key::Named(Named::Tab), false)));
    println!(
        "{}",
        describe(keys::press(Key::Named(Named::Escape), false))
    );
    println!("{}", describe(keys::typed("a")));
    println!("{}", describe(keys::press(Key::Unidentified, false)));
    println!("{}", describe(Event::Other));
    let key = Key::Named(Named::Escape);
    println!("{}", keys::name(&key));
}
