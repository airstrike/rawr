//! A key whose character type has a default, as iced's `keyboard::Key<C =
//! SmolStr>`, inside an event that names it without arguments.

/// A key's text, as smol_str's `SmolStr`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text(String);

impl Text {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Tab,
    Escape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key<C = Text> {
    Named(Named),
    Character(C),
    Unidentified,
}

#[derive(Clone, Debug)]
pub enum Event {
    KeyPressed { key: Key, shift: bool },
    Other,
}

pub fn press(key: Key, shift: bool) -> Event {
    Event::KeyPressed { key, shift }
}

pub fn typed(text: &str) -> Event {
    press(Key::Character(Text(text.to_owned())), false)
}

pub fn name(key: &Key) -> String {
    format!("{key:?}")
}

/// A key press as iced's `keyboard::Event::KeyPressed` gives it: its keys
/// beside the text it types, an `Option` of the defaulted parameter's type.
#[derive(Clone, Debug)]
pub enum Typed {
    KeyPressed {
        key: Key,
        modified_key: Key,
        text: Option<Text>,
        repeat: bool,
    },
    KeyReleased {
        key: Key,
    },
}

/// What `filter` keeps of a few presses, as iced's
/// `keyboard::listen().filter_map(..)` hands its closure each event.
pub fn listen<T>(filter: impl Fn(Typed) -> Option<T>) -> Vec<T> {
    let character = |text: &str| Key::Character(Text(text.to_owned()));
    [
        Typed::KeyPressed {
            key: character("a"),
            modified_key: character("A"),
            text: Some(Text("A".to_owned())),
            repeat: false,
        },
        Typed::KeyPressed {
            key: Key::Named(Named::Tab),
            modified_key: Key::Named(Named::Tab),
            text: None,
            repeat: true,
        },
        Typed::KeyReleased {
            key: character("a"),
        },
        Typed::KeyPressed {
            key: Key::Named(Named::Escape),
            modified_key: Key::Named(Named::Escape),
            text: None,
            repeat: false,
        },
    ]
    .into_iter()
    .filter_map(filter)
    .collect()
}
