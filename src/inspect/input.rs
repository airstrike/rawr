//! Input a tool sends a running program's window (#989): a pointer move,
//! a click, a drag, a scroll, a key press and typed text, asked for on the
//! program's socket as `{"op": "input", "action": .., ..}`.
//!
//! The input goes to the program itself, in its own process: each event is
//! handed to the window's views as the window system would hand it, through
//! their own event methods. Nothing reaches the system's event stream, so
//! the person's own pointer, keyboard and focus stay as they are, and the
//! window is never brought forward.
//!
//! Positions are in the window's logical points from its top-left corner,
//! title bar included, as a capture of the window shows it. The window
//! also shows an agent cursor where the last pointer event was, drawn
//! above the program's own drawing: the program's layout and events don't
//! change for it.
//!
//! An answer comes once the program has taken each event and drawn again:
//! `{"window": {"title": .., "width": .., "height": ..}, "cursor": {"x": ..,
//! "y": ..} or null}`, or `{"error": ..}`.

#[cfg(target_os = "macos")]
mod appkit;

use std::fmt;

use serde_json::{json, Value};

/// A position in a window: logical points from its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Point {
    x: f64,
    y: f64,
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Button {
    Left,
    Right,
    Middle,
}

/// How a scroll's amounts count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unit {
    /// Lines, as a mouse wheel's notches.
    Line,
    /// Logical points, as a trackpad's.
    Pixel,
}

/// The modifier keys held during a key press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Modifiers {
    shift: bool,
    control: bool,
    alt: bool,
    command: bool,
}

/// A key the window takes: its virtual key code on a US layout and the
/// characters it types, `None` for a key that types none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Key {
    code: u16,
    characters: Option<String>,
}

/// One input, checked: every position, button and key is one the window
/// can take.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Gesture {
    /// The pointer moves to a point.
    Move(Point),
    /// A button goes down and up there, `count` times, as a double click
    /// for 2.
    Click {
        at: Point,
        button: Button,
        count: u32,
    },
    /// A button goes down at `from`, the pointer moves to `to` with it
    /// held, and it goes up there.
    Drag {
        from: Point,
        to: Point,
        button: Button,
    },
    /// The content under a point scrolls by `x` and `y`: positive `y`
    /// toward the end, positive `x` to the right.
    Scroll {
        at: Point,
        x: f64,
        y: f64,
        unit: Unit,
    },
    /// A key goes down and up with modifiers held.
    Press { key: Key, modifiers: Modifiers },
    /// Text is typed, one key for each character.
    Type(String),
}

/// Why an input wasn't sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Error {
    /// The request names no action this socket knows.
    Action(Option<String>),
    /// A required argument is missing or isn't a number or a string.
    Missing(&'static str),
    /// A button that is not `left`, `right` or `middle`.
    Button(String),
    /// A key name this socket doesn't know.
    Key(String),
    /// A modifier this socket doesn't know.
    Modifier(String),
    /// A click count of zero.
    Count,
    /// The program shows no window, or none with that title.
    Window { title: Option<String> },
    /// This system can't hand a window input.
    #[cfg(not(target_os = "macos"))]
    Unsupported,
    /// The wait for the window was given up: the program paused, or the
    /// tool that asked went away.
    Interrupted,
    /// The window's event loop ended before it took the input.
    Ended,
}

/// What a sent input reached: the window and where the agent cursor is.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sent {
    title: String,
    width: f64,
    height: f64,
    cursor: Option<Point>,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Action(Some(action)) => write!(
                formatter,
                "unknown input `{action}`: use move, click, drag, scroll, key or type"
            ),
            Self::Action(None) => {
                formatter.write_str("an input needs an `action`: move, click, drag, scroll, key or type")
            }
            Self::Missing(name) => write!(formatter, "this input needs `{name}`"),
            Self::Button(button) => write!(
                formatter,
                "unknown button `{button}`: use left, right or middle"
            ),
            Self::Key(key) => write!(
                formatter,
                "unknown key `{key}`: use one character, or Enter, Tab, Space, Backspace, Delete, Escape, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown or F1 to F12"
            ),
            Self::Modifier(modifier) => write!(
                formatter,
                "unknown modifier `{modifier}`: use shift, control, alt or command"
            ),
            Self::Count => formatter.write_str("a click's `count` is at least 1"),
            Self::Window { title: Some(title) } => {
                write!(formatter, "the program shows no window titled `{title}`")
            }
            Self::Window { title: None } => formatter.write_str("the program shows no window"),
            #[cfg(not(target_os = "macos"))]
            Self::Unsupported => formatter.write_str("sending a window input needs macOS here"),
            Self::Interrupted => {
                formatter.write_str("the program paused before its window took the input")
            }
            Self::Ended => {
                formatter.write_str("the program's window ended before it took the input")
            }
        }
    }
}

impl Point {
    fn from_request(request: &Value, x: &'static str, y: &'static str) -> Result<Self, Error> {
        Ok(Self {
            x: number(request, x)?,
            y: number(request, y)?,
        })
    }

    fn to_json(self) -> Value {
        json!({ "x": self.x, "y": self.y })
    }
}

impl Button {
    fn from_request(request: &Value) -> Result<Self, Error> {
        match request.get("button").and_then(Value::as_str) {
            None | Some("left") => Ok(Self::Left),
            Some("right") => Ok(Self::Right),
            Some("middle") => Ok(Self::Middle),
            Some(other) => Err(Error::Button(other.to_owned())),
        }
    }
}

impl Modifiers {
    fn from_request(request: &Value) -> Result<Self, Error> {
        let mut modifiers = Self::default();
        let names = request
            .get("modifiers")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for name in names {
            let name = name.as_str().unwrap_or_default();
            match name.to_ascii_lowercase().as_str() {
                "shift" => modifiers.shift = true,
                "control" | "ctrl" => modifiers.control = true,
                "alt" | "option" => modifiers.alt = true,
                "command" | "cmd" | "logo" | "super" | "meta" => modifiers.command = true,
                _ => return Err(Error::Modifier(name.to_owned())),
            }
        }
        Ok(modifiers)
    }
}

impl Key {
    /// The key `name` names: a W3C key name, as iced's `key::Named` has
    /// them, or one character.
    fn named(name: &str) -> Result<Self, Error> {
        let named = |code: u16, character: Option<char>| Self {
            code,
            characters: character.map(String::from),
        };
        let key = match name {
            "Enter" | "Return" => named(36, Some('\r')),
            "Tab" => named(48, Some('\t')),
            "Space" => named(49, Some(' ')),
            "Backspace" => named(51, Some('\u{7f}')),
            "Escape" => named(53, Some('\u{1b}')),
            "Delete" => named(117, Some('\u{f728}')),
            "Home" => named(115, Some('\u{f729}')),
            "End" => named(119, Some('\u{f72b}')),
            "PageUp" => named(116, Some('\u{f72c}')),
            "PageDown" => named(121, Some('\u{f72d}')),
            "ArrowLeft" => named(123, Some('\u{f702}')),
            "ArrowRight" => named(124, Some('\u{f703}')),
            "ArrowDown" => named(125, Some('\u{f701}')),
            "ArrowUp" => named(126, Some('\u{f700}')),
            _ => {
                if let Some(number) = name.strip_prefix('F').and_then(|n| n.parse::<u16>().ok())
                    && (1..=12).contains(&number)
                {
                    const CODES: [u16; 12] =
                        [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
                    let character = char::from_u32(0xf703 + u32::from(number));
                    named(CODES[usize::from(number - 1)], character)
                } else {
                    let mut characters = name.chars();
                    match (characters.next(), characters.next()) {
                        (Some(character), None) => Self::typing(character),
                        _ => return Err(Error::Key(name.to_owned())),
                    }
                }
            }
        };
        Ok(key)
    }

    /// The key that types `character`: its key on a US layout, or a key no
    /// layout names for one that has none there, which still types it.
    fn typing(character: char) -> Self {
        match character {
            '\n' | '\r' => return Self::named("Enter").expect("Enter is a key"),
            '\t' => return Self::named("Tab").expect("Tab is a key"),
            _ => {}
        }
        const LAYOUT: [(char, char, u16); 47] = [
            ('a', 'A', 0),
            ('s', 'S', 1),
            ('d', 'D', 2),
            ('f', 'F', 3),
            ('h', 'H', 4),
            ('g', 'G', 5),
            ('z', 'Z', 6),
            ('x', 'X', 7),
            ('c', 'C', 8),
            ('v', 'V', 9),
            ('b', 'B', 11),
            ('q', 'Q', 12),
            ('w', 'W', 13),
            ('e', 'E', 14),
            ('r', 'R', 15),
            ('y', 'Y', 16),
            ('t', 'T', 17),
            ('1', '!', 18),
            ('2', '@', 19),
            ('3', '#', 20),
            ('4', '$', 21),
            ('6', '^', 22),
            ('5', '%', 23),
            ('=', '+', 24),
            ('9', '(', 25),
            ('7', '&', 26),
            ('-', '_', 27),
            ('8', '*', 28),
            ('0', ')', 29),
            (']', '}', 30),
            ('o', 'O', 31),
            ('u', 'U', 32),
            ('[', '{', 33),
            ('i', 'I', 34),
            ('p', 'P', 35),
            ('l', 'L', 37),
            ('j', 'J', 38),
            ('\'', '"', 39),
            ('k', 'K', 40),
            (';', ':', 41),
            ('\\', '|', 42),
            (',', '<', 43),
            ('/', '?', 44),
            ('n', 'N', 45),
            ('m', 'M', 46),
            ('.', '>', 47),
            ('`', '~', 50),
        ];
        // 0x34 is a key code no layout gives a key: the character alone
        // says what it types.
        let code = match character {
            ' ' => 49,
            _ => LAYOUT
                .iter()
                .find(|(plain, shifted, _)| *plain == character || *shifted == character)
                .map_or(0x34, |(_, _, code)| *code),
        };
        Self {
            code,
            characters: Some(character.to_string()),
        }
    }
}

impl Gesture {
    /// The input `request` asks for.
    pub(super) fn from_request(request: &Value) -> Result<Self, Error> {
        let action = request.get("action").and_then(Value::as_str);
        let gesture = match action {
            Some("move") => Self::Move(Point::from_request(request, "x", "y")?),
            Some("click") => {
                let count = request.get("count").and_then(Value::as_u64).unwrap_or(1);
                if count == 0 {
                    return Err(Error::Count);
                }
                Self::Click {
                    at: Point::from_request(request, "x", "y")?,
                    button: Button::from_request(request)?,
                    count: u32::try_from(count.min(3)).unwrap_or(3),
                }
            }
            Some("drag") => Self::Drag {
                from: Point::from_request(request, "x", "y")?,
                to: Point::from_request(request, "to_x", "to_y")?,
                button: Button::from_request(request)?,
            },
            Some("scroll") => Self::Scroll {
                at: Point::from_request(request, "x", "y")?,
                x: request.get("dx").and_then(Value::as_f64).unwrap_or(0.0),
                y: request.get("dy").and_then(Value::as_f64).unwrap_or(0.0),
                unit: match request.get("unit").and_then(Value::as_str) {
                    Some("pixel") => Unit::Pixel,
                    _ => Unit::Line,
                },
            },
            Some("key") => Self::Press {
                key: Key::named(
                    request
                        .get("key")
                        .and_then(Value::as_str)
                        .ok_or(Error::Missing("key"))?,
                )?,
                modifiers: Modifiers::from_request(request)?,
            },
            Some("type") => Self::Type(
                request
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(Error::Missing("text"))?
                    .to_owned(),
            ),
            other => return Err(Error::Action(other.map(str::to_owned))),
        };
        Ok(gesture)
    }

    /// Send the input to the program's window titled `title`, or its front
    /// window, waiting for it to take each event and draw, until
    /// `interrupted` says to give up.
    pub(super) fn send(
        &self,
        title: Option<&str>,
        interrupted: &dyn Fn() -> bool,
    ) -> Result<Sent, Error> {
        #[cfg(target_os = "macos")]
        {
            appkit::send(self, title, interrupted)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (title, interrupted);
            Err(Error::Unsupported)
        }
    }
}

impl Sent {
    pub(super) fn to_json(&self) -> Value {
        json!({
            "window": { "title": self.title, "width": self.width, "height": self.height },
            "cursor": self.cursor.map(Point::to_json),
        })
    }
}

/// Answer one input request, or `None` once `interrupted` gave it up.
pub(super) fn answer(request: &Value, interrupted: &dyn Fn() -> bool) -> Option<Value> {
    let title = request.get("title").and_then(Value::as_str);
    match Gesture::from_request(request).and_then(|gesture| gesture.send(title, interrupted)) {
        Ok(sent) => Some(sent.to_json()),
        Err(Error::Interrupted) => None,
        Err(error) => Some(json!({ "error": error.to_string() })),
    }
}

fn number(request: &Value, name: &'static str) -> Result<f64, Error> {
    request
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or(Error::Missing(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_reads_its_point_button_and_count() {
        let gesture = Gesture::from_request(
            &json!({ "action": "click", "x": 10.5, "y": 20, "button": "right", "count": 2 }),
        );
        assert_eq!(
            gesture,
            Ok(Gesture::Click {
                at: Point { x: 10.5, y: 20.0 },
                button: Button::Right,
                count: 2,
            })
        );
    }

    #[test]
    fn a_click_without_a_point_or_with_no_count_is_refused() {
        assert_eq!(
            Gesture::from_request(&json!({ "action": "click", "x": 1 })),
            Err(Error::Missing("y"))
        );
        assert_eq!(
            Gesture::from_request(&json!({ "action": "click", "x": 1, "y": 1, "count": 0 })),
            Err(Error::Count)
        );
        assert_eq!(
            Gesture::from_request(
                &json!({ "action": "click", "x": 1, "y": 1, "button": "fourth" })
            ),
            Err(Error::Button("fourth".to_owned()))
        );
    }

    #[test]
    fn named_keys_and_characters_have_their_us_key_codes() {
        assert_eq!(
            Key::named("Enter"),
            Ok(Key {
                code: 36,
                characters: Some("\r".to_owned())
            })
        );
        assert_eq!(
            Key::named("F5"),
            Ok(Key {
                code: 96,
                characters: Some("\u{f708}".to_owned())
            })
        );
        assert_eq!(Key::named("a").map(|key| key.code), Ok(0));
        assert_eq!(Key::named("A").map(|key| key.code), Ok(0));
        assert_eq!(Key::named("?").map(|key| key.code), Ok(44));
        // A character no US key types still types itself.
        assert_eq!(
            Key::named("é"),
            Ok(Key {
                code: 0x34,
                characters: Some("é".to_owned())
            })
        );
        assert_eq!(Key::named("Hyper"), Err(Error::Key("Hyper".to_owned())));
        assert_eq!(Key::named("F13"), Err(Error::Key("F13".to_owned())));
    }

    #[test]
    fn a_key_press_reads_its_modifiers() {
        let gesture = Gesture::from_request(
            &json!({ "action": "key", "key": "a", "modifiers": ["cmd", "Shift"] }),
        );
        let Ok(Gesture::Press { modifiers, .. }) = gesture else {
            panic!("{gesture:?}");
        };
        assert_eq!(
            modifiers,
            Modifiers {
                shift: true,
                command: true,
                ..Modifiers::default()
            }
        );
        assert_eq!(
            Gesture::from_request(&json!({ "action": "key", "key": "a", "modifiers": ["hyper"] })),
            Err(Error::Modifier("hyper".to_owned()))
        );
    }

    #[test]
    fn an_unknown_action_says_which_there_are() {
        let answer = answer(&json!({ "action": "wave" }), &|| false).unwrap();
        assert_eq!(
            answer["error"],
            "unknown input `wave`: use move, click, drag, scroll, key or type"
        );
    }
}
