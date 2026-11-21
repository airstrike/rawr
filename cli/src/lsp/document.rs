//! A file the client has open, and how its positions translate: the
//! protocol counts lines from 0 and characters in the encoding the client
//! and the server agreed on, and rawr counts both from 1, in characters.

use std::path::{Path, PathBuf};

/// How the protocol counts the characters of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Encoding {
    /// UTF-16 code units, the protocol's default.
    Utf16,
    /// Characters, as rawr counts them, when the client offers it.
    Utf32,
}

impl Encoding {
    /// The encoding among `offered`, the client's `positionEncodings`:
    /// characters when it offers them.
    pub(super) fn chosen(offered: &serde_json::Value) -> Self {
        let offers = |name: &str| {
            offered
                .as_array()
                .is_some_and(|offered| offered.iter().any(|offer| offer == name))
        };
        if offers("utf-32") {
            Self::Utf32
        } else {
            Self::Utf16
        }
    }

    /// The encoding's name in the protocol.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Utf16 => "utf-16",
            Self::Utf32 => "utf-32",
        }
    }

    /// The character, from 1, that the protocol's `character` on `line`
    /// names.
    pub(super) fn column(self, line: &str, character: u64) -> usize {
        let character = usize::try_from(character).unwrap_or(usize::MAX);
        match self {
            Self::Utf32 => character + 1,
            Self::Utf16 => {
                let mut units = 0;
                for (index, each) in line.chars().enumerate() {
                    units += each.len_utf16();
                    if units > character {
                        return index + 1;
                    }
                }
                line.chars().count() + 1
            }
        }
    }

    /// The protocol's `character` for character `column` (from 1) of
    /// `line`.
    pub(super) fn character(self, line: &str, column: usize) -> u64 {
        let before = column.saturating_sub(1);
        let units = match self {
            Self::Utf32 => before,
            Self::Utf16 => line.chars().take(before).map(char::len_utf16).sum(),
        };
        u64::try_from(units).unwrap_or(u64::MAX)
    }
}

/// A file the client has open: where it is, and its text as the client
/// holds it, saved or not.
#[derive(Debug)]
pub(super) struct Document {
    pub(super) path: PathBuf,
    text: String,
}

impl Document {
    /// The document the client opened at `uri` with `text`: `None` when
    /// `uri` names no file.
    pub(super) fn new(uri: &str, text: String) -> Option<Self> {
        Some(Self {
            path: path(uri)?,
            text,
        })
    }

    /// The client's whole new text.
    pub(super) fn replace(&mut self, text: String) {
        self.text = text;
    }

    /// The document's text.
    pub(super) fn text(&self) -> &str {
        &self.text
    }
}

/// Line `line` of `text`, from 1, without its line ending.
pub(super) fn line(text: &str, line: usize) -> &str {
    text.split('\n')
        .nth(line.saturating_sub(1))
        .unwrap_or_default()
        .trim_end_matches('\r')
}

/// The file a `file:` URI names, as the run names it: absolute, through
/// its symlinks when it is there.
pub(super) fn path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // A host before the path, as `file://localhost/p`, is this machine.
    let rest = &rest[rest.find('/')?..];
    let mut bytes = Vec::with_capacity(rest.len());
    let mut at = 0;
    let raw = rest.as_bytes();
    while at < raw.len() {
        let hex = |offset: usize| {
            raw.get(at + offset)
                .and_then(|digit| char::from(*digit).to_digit(16))
        };
        match (raw[at], hex(1), hex(2)) {
            (b'%', Some(high), Some(low)) => {
                bytes.push(u8::try_from(high * 16 + low).ok()?);
                at += 3;
            }
            (byte, ..) => {
                bytes.push(byte);
                at += 1;
            }
        }
    }
    let path = PathBuf::from(String::from_utf8(bytes).ok()?);
    Some(std::fs::canonicalize(&path).unwrap_or(path))
}

/// The `file:` URI of `path`.
pub(super) fn uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(char::from(byte));
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{path, uri, Encoding};

    #[test]
    fn a_utf16_character_is_the_character_it_falls_in() {
        let line = "let é = \"🦖\";";
        // `🦖` is two UTF-16 units, at units 9 and 10.
        assert_eq!(Encoding::Utf16.column(line, 9), 10);
        assert_eq!(Encoding::Utf16.column(line, 10), 10);
        assert_eq!(Encoding::Utf16.column(line, 11), 11);
        assert_eq!(Encoding::Utf16.character(line, 11), 11);
        assert_eq!(Encoding::Utf32.column(line, 10), 11);
        assert_eq!(Encoding::Utf32.character(line, 11), 10);
    }

    #[test]
    fn a_uri_and_its_path_round_trip_with_spaces() {
        let spaced = Path::new("/rawr-lsp-test/a crate/src/main.rs");
        let named = uri(spaced);
        assert_eq!(named, "file:///rawr-lsp-test/a%20crate/src/main.rs");
        assert_eq!(path(&named).as_deref(), Some(spaced));
        assert_eq!(path("untitled:Untitled-1"), None);
    }
}
