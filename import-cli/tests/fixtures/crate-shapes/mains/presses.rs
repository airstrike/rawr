// rawr
// A match on `press.key.as_ref()` borrows the press while its arms are
// tested, and an arm that binds nothing of it moves the press, as iced's
// editor's `key_binding` closure does: rustc's borrow ends before that
// arm (#2005). A `let` whose value borrows nothing of the press ends its
// borrow with the statement.
use natives::presses::{Binding, Editor, Key, KeyPress, Modifiers};

#[derive(Debug, Clone, PartialEq)]
enum Message {
    Save,
}

fn bind(press: KeyPress) -> Option<Binding<Message>> {
    match press.key.as_ref() {
        Key::Character("s") if press.modifiers.command() => Some(Binding::Custom(Message::Save)),
        _ => Binding::from_key_press(press),
    }
}

fn measured(press: KeyPress) -> Option<Binding<Message>> {
    let width = press.key.width();
    println!("{width}");
    Binding::from_key_press(press)
}

fn pressed(key: &str, modifiers: u8) -> KeyPress {
    KeyPress {
        key: Key::Character(key.to_owned()),
        modifiers: Modifiers(modifiers),
    }
}

fn main() {
    println!("{:?}", bind(pressed("x", 0)));
    println!("{:?}", bind(pressed("s", 1)));
    println!("{:?}", measured(pressed("ab", 0)));
    let mut editor = Editor::new().key_binding(|press| match press.key.as_ref() {
        Key::Character("s") if press.modifiers.command() => Some(Binding::Custom(Message::Save)),
        _ => Binding::from_key_press(press),
    });
    println!("{:?}", editor.press(Key::Named(3), Modifiers(0)));
    println!(
        "{:?}",
        editor.press(Key::Character("s".to_owned()), Modifiers(1))
    );
    println!(
        "{:?}",
        editor.press(Key::Character("q".to_owned()), Modifiers(1))
    );
}
