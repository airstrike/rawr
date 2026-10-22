use std::io::{self, Write};
use std::ops::Range;

// The transport has no parser or dependency of its own. Non-ASCII characters
// remain UTF-8; JSON controls, quotation marks, and backslashes are escaped.
pub fn string(output: &mut impl Write, text: &str) -> io::Result<()> {
    output.write_all(b"\"")?;
    for character in text.chars() {
        match character {
            '"' => output.write_all(b"\\\"")?,
            '\\' => output.write_all(b"\\\\")?,
            '\n' => output.write_all(b"\\n")?,
            '\r' => output.write_all(b"\\r")?,
            '\t' => output.write_all(b"\\t")?,
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04x}", u32::from(character))?;
            }
            character => {
                let mut bytes = [0; 4];
                output.write_all(character.encode_utf8(&mut bytes).as_bytes())?;
            }
        }
    }
    output.write_all(b"\"")
}

pub fn span(output: &mut impl Write, span: Option<Range<usize>>) -> io::Result<()> {
    match span {
        Some(span) => write!(output, "[{},{}]", span.start, span.end),
        None => output.write_all(b"null"),
    }
}

pub fn optional_string(output: &mut impl Write, text: Option<&str>) -> io::Result<()> {
    match text {
        Some(text) => string(output, text),
        None => output.write_all(b"null"),
    }
}
