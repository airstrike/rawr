// rawr
// iced's layout in the fixture: a crate closure takes a key event whose
// press holds an `Option` of the key's text type, and the program
// destructures it by value with a literal field and `..`, as layout's
// `subscription` does.

use events::keys::{self, Key, Named, Typed};

fn main() {
    let kept = keys::listen(|event| {
        let Typed::KeyPressed {
            modified_key,
            repeat: false,
            ..
        } = event
        else {
            return None;
        };
        match modified_key {
            Key::Named(Named::Escape) => Some("escape".to_string()),
            Key::Character(text) => Some(format!("character {}", text.as_str())),
            _ => None,
        }
    });
    println!("{kept:?}");
    let texts = keys::listen(|event| match event {
        Typed::KeyPressed { text, .. } => Some(text.map(|text| text.as_str().to_owned())),
        Typed::KeyReleased { .. } => None,
    });
    println!("{texts:?}");
}
