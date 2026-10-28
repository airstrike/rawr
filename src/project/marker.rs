//! The `// rawr` marker. A `.rs` file whose first line of Rust source is
//! exactly `// rawr` is a Rawr file: Rawr runs it, and Cargo builds the
//! same file.
//!
//! The first line of Rust source is the file's first line after an
//! optional UTF-8 byte order mark, an optional shebang line and an optional
//! cargo-script frontmatter block (D608 point 9). The rule is strict.
//! Trailing whitespace (including the `\r` of a CRLF line end) is allowed.
//! Nothing else is: `//rawr`, `// rawr: note`, `// Rawr`, a blank line
//! before the marker and a marker on a later line do not count. A file
//! whose frontmatter cargo refuses is not marked. An unmarked `.rs` file is
//! native Rust.

use std::fs;
use std::io::{self, Read as _};
use std::path::Path;

use super::frontmatter;

/// The first line that makes a `.rs` file a Rawr file.
pub const MARKER: &str = "// rawr";

/// Whether the first line of the Rust source in `text` is the marker.
#[must_use]
pub fn is_marked(text: &str) -> bool {
    let Ok(lead) = frontmatter::read(text) else {
        return false;
    };
    let line = text[lead.source..].split('\n').next().unwrap_or_default();
    line.trim_end() == MARKER
}

/// What a file's start says about it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Marking {
    /// The first line of its Rust source is the marker.
    pub(crate) marked: bool,
    /// It has a frontmatter block: it is a cargo script.
    pub(crate) script: bool,
}

/// How the file at `path` is marked. A file that starts with the marker or
/// with ordinary Rust is decided by its first bytes; one that may start
/// with a shebang or a frontmatter block is read whole.
pub(crate) fn file_marking(path: &Path) -> io::Result<Marking> {
    let mut head = Vec::with_capacity(256);
    fs::File::open(path)?.take(256).read_to_end(&mut head)?;
    let start = if head.starts_with("\u{feff}".as_bytes()) {
        3
    } else {
        0
    };
    let leads = head
        .get(start)
        .is_some_and(|byte| matches!(byte, b'#' | b'-') || byte.is_ascii_whitespace());
    if leads {
        let bytes = fs::read(path)?;
        let text = std::str::from_utf8(&bytes).unwrap_or_default();
        return Ok(Marking {
            marked: is_marked(text),
            script: frontmatter::is_script(text),
        });
    }
    let end = head
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(head.len());
    Ok(Marking {
        marked: std::str::from_utf8(&head[..end]).is_ok_and(is_marked),
        script: false,
    })
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

    #[test]
    fn the_marker_follows_a_shebang_and_frontmatter() {
        for marked in [
            "#!/usr/bin/env rawr\n// rawr\n",
            "---cargo\n[dependencies]\n---\n// rawr\nfn main() {}\n",
            "---\n---\n// rawr\n",
            "#!/usr/bin/env -S cargo +nightly -Zscript\n\n---cargo\n---\n// rawr\r\n",
            "\u{feff}#!/x\n---cargo\n---\n// rawr\n",
        ] {
            assert!(is_marked(marked), "{marked:?}");
        }
        for unmarked in [
            // The marker is the first line of the source, not any line.
            "---cargo\n---\n\n// rawr\n",
            "#!/usr/bin/env rawr\n\n// rawr\n",
            // An inner attribute is Rust, not a shebang.
            "#![allow(dead_code)]\n// rawr\n",
            // Malformed frontmatter gets no search for the marker.
            "---cargo\n[dependencies]\n// rawr\n",
            "---toml\n---\n// rawr\n",
            "---\n---\n---\n---\n// rawr\n",
            "----\n---\n// rawr\n",
        ] {
            assert!(!is_marked(unmarked), "{unmarked:?}");
        }
    }
}
