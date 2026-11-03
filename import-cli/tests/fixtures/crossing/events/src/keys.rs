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
