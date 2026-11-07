//! The JSON a reload line is: one object whose fields an outcome reads are
//! strings and whole numbers. Nested values are checked and skipped.

use std::iter::Peekable;
use std::str::Chars;

/// A JSON object's top-level fields.
pub(crate) struct Object(Vec<(String, Value)>);

enum Value {
    Text(String),
    Number(String),
    /// An object, an array, `true`, `false` or `null`.
    Other,
}

impl Object {
    /// `line` as one JSON object, or `None` when it is anything else.
    pub(crate) fn parse(line: &str) -> Option<Self> {
        let mut chars = line.chars().peekable();
        skip_space(&mut chars);
        let mut fields = Vec::new();
        object(&mut chars, Some(&mut fields))?;
        skip_space(&mut chars);
        chars.next().is_none().then_some(Self(fields))
    }

    /// The string field `name`.
    pub(crate) fn text(&self, name: &str) -> Option<&str> {
        match self.field(name)? {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The whole-number field `name`.
    pub(crate) fn number(&self, name: &str) -> Option<u32> {
        match self.field(name)? {
            Value::Number(number) => number.parse().ok(),
            _ => None,
        }
    }

    fn field(&self, name: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    }
}

type Input<'a> = Peekable<Chars<'a>>;

fn skip_space(chars: &mut Input<'_>) {
    while chars
        .next_if(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
        .is_some()
    {}
}

/// An object, its fields kept in `fields` when given.
fn object(chars: &mut Input<'_>, mut fields: Option<&mut Vec<(String, Value)>>) -> Option<()> {
    chars.next_if_eq(&'{')?;
    skip_space(chars);
    if chars.next_if_eq(&'}').is_some() {
        return Some(());
    }
    loop {
        skip_space(chars);
        let name = string(chars)?;
        skip_space(chars);
        chars.next_if_eq(&':')?;
        skip_space(chars);
        let value = value(chars)?;
        if let Some(fields) = fields.as_deref_mut() {
            fields.push((name, value));
        }
        skip_space(chars);
        match chars.next()? {
            ',' => {}
            '}' => return Some(()),
            _ => return None,
        }
    }
}

fn array(chars: &mut Input<'_>) -> Option<()> {
    chars.next_if_eq(&'[')?;
    skip_space(chars);
    if chars.next_if_eq(&']').is_some() {
        return Some(());
    }
    loop {
        skip_space(chars);
        value(chars)?;
        skip_space(chars);
        match chars.next()? {
            ',' => {}
            ']' => return Some(()),
            _ => return None,
        }
    }
}

fn value(chars: &mut Input<'_>) -> Option<Value> {
    Some(match *chars.peek()? {
        '"' => Value::Text(string(chars)?),
        '{' => {
            object(chars, None)?;
            Value::Other
        }
        '[' => {
            array(chars)?;
            Value::Other
        }
        't' => word(chars, "true")?,
        'f' => word(chars, "false")?,
        'n' => word(chars, "null")?,
        '-' | '0'..='9' => {
            let mut number = String::new();
            while let Some(c) =
                chars.next_if(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
            {
                number.push(c);
            }
            Value::Number(number)
        }
        _ => return None,
    })
}

fn word(chars: &mut Input<'_>, word: &str) -> Option<Value> {
    for expected in word.chars() {
        chars.next_if_eq(&expected)?;
    }
    Some(Value::Other)
}

fn string(chars: &mut Input<'_>) -> Option<String> {
    chars.next_if_eq(&'"')?;
    let mut text = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(text),
            '\\' => text.push(match chars.next()? {
                '"' => '"',
                '\\' => '\\',
                '/' => '/',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'u' => escaped(chars)?,
                _ => return None,
            }),
            c => text.push(c),
        }
    }
}

/// The character after `\u`: four hex digits, or a surrogate pair of them.
fn escaped(chars: &mut Input<'_>) -> Option<char> {
    let high = hex(chars)?;
    if !(0xd800..0xdc00).contains(&high) {
        return char::from_u32(high);
    }
    chars.next_if_eq(&'\\')?;
    chars.next_if_eq(&'u')?;
    let low = hex(chars)?;
    if !(0xdc00..0xe000).contains(&low) {
        return None;
    }
    char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00))
}

fn hex(chars: &mut Input<'_>) -> Option<u32> {
    let mut value = 0;
    for _ in 0..4 {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::Object;

    #[test]
    fn escapes_decode_as_json_writes_them() {
        // `\u` escapes, spelled with the backslash apart.
        let u = |hex: &str| format!("{}u{hex}", '\\');
        let line = format!(
            r#"{{"a":"q\"b\\s\/n\nt\t{}p{}{}"}}"#,
            u("00e9"),
            u("d83e"),
            u("dd80")
        );
        let object = Object::parse(&line).unwrap();
        assert_eq!(object.text("a"), Some("q\"b\\s/n\nt\t\u{e9}p\u{1f980}"));
    }

    #[test]
    fn a_lone_surrogate_or_trailing_text_is_no_object() {
        assert!(Object::parse(r#"{"a":"\ud83e"}"#).is_none());
        assert!(Object::parse(r#"{"a":1} x"#).is_none());
        assert!(Object::parse(r#"{"a":1,}"#).is_none());
        assert!(Object::parse(r#"{}"#).is_some());
    }

    #[test]
    fn a_number_reads_only_when_whole() {
        let object = Object::parse(r#"{"a":12,"b":-1,"c":2.5,"d":"3"}"#).unwrap();
        assert_eq!(object.number("a"), Some(12));
        assert_eq!(object.number("b"), None);
        assert_eq!(object.number("c"), None);
        assert_eq!(object.number("d"), None);
    }
}
