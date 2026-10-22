//! The `// rawr` marker. A `.rs` file whose first line is exactly `// rawr`
//! is a Rawr file: Rawr runs it, and Cargo builds the same file.
//!
//! The rule is strict. Trailing whitespace (including the `\r` of a CRLF
//! line end) and a leading UTF-8 byte order mark are allowed. Nothing else
//! is: `//rawr`, `// rawr: note`, `// Rawr` and a marker on a later line do
//! not count. An unmarked `.rs` file is native Rust.

use std::fs;
use std::io::{self, Read as _};
use std::path::Path;

/// The first line that makes a `.rs` file a Rawr file.
pub const MARKER: &str = "// rawr";

/// Whether `text` starts with the marker line.
#[must_use]
pub fn is_marked(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let line = text.split('\n').next().unwrap_or_default();
    line.trim_end() == MARKER
}

/// Whether the file at `path` starts with the marker line, reading only
/// its first bytes.
pub(crate) fn file_is_marked(path: &Path) -> io::Result<bool> {
    let mut head = Vec::with_capacity(256);
    fs::File::open(path)?.take(256).read_to_end(&mut head)?;
    let end = head
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(head.len());
    Ok(std::str::from_utf8(&head[..end]).is_ok_and(is_marked))
}

#[cfg(test)]
mod tests {
    use super::is_marked;

    #[test]
    fn only_an_exact_first_line_marks_a_file() {
        for marked in [
            "// rawr",
            "// rawr\n",
            "// rawr  \t\r\nfn main() {}",
            "\u{feff}// rawr\n",
        ] {
            assert!(is_marked(marked), "{marked:?}");
        }
        for unmarked in [
            "",
            "//rawr\n",
            "// rawr: fast\n",
            "// Rawr\n",
            " // rawr\n",
            "/// rawr\n",
            "fn main() {}\n// rawr\n",
            "\n// rawr\n",
        ] {
            assert!(!is_marked(unmarked), "{unmarked:?}");
        }
    }
}
