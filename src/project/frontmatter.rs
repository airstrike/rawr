//! What comes before a Rust file's source: an optional UTF-8 byte order
//! mark, an optional shebang line and an optional cargo-script frontmatter
//! block (RFC 3502 and RFC 3503).
//!
//! The grammar is cargo's (`src/cargo/util/frontmatter.rs`), as the
//! importer's `input/membership/manifest/load/embedded.rs` carries it. The
//! importer's copy is tied to its admission session and is optional in this
//! crate, so this one reads the same grammar on its own, and a test holds
//! it to the importer's recorded observations of cargo.

use std::ops::Range;

/// The byte order mark rustc drops before it reads a file.
const BOM: char = '\u{feff}';

/// Where a file's Rust source starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lead {
    /// The byte where the Rust source starts: after the byte order mark,
    /// the shebang line and the frontmatter block, whichever are there.
    pub source: usize,
    /// The frontmatter block, when the file has one.
    pub frontmatter: Option<Frontmatter>,
}

/// A well-formed frontmatter block. Ranges are bytes of the whole text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frontmatter {
    /// The opening fence's dashes.
    pub open: Range<usize>,
    /// The infostring, trimmed. It is empty or `cargo`.
    pub info: Range<usize>,
    /// The manifest: the lines between the two fences.
    pub manifest: Range<usize>,
    /// The closing fence's dashes.
    pub close: Range<usize>,
}

/// A frontmatter block cargo refuses, with cargo's message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Malformed {
    /// What cargo says.
    pub message: String,
    /// The bytes cargo points at.
    pub span: Range<usize>,
    /// How many dashes open the block.
    fence: usize,
    /// Where the Rust source starts when the dashes open no block.
    after_shebang: usize,
}

impl Malformed {
    /// Whether rustc reads the text as opening a frontmatter block, with
    /// three dashes or more. One or two dashes are Rust tokens to rustc,
    /// though cargo refuses them as a script's frontmatter.
    #[must_use]
    pub const fn opens(&self) -> bool {
        self.fence >= 3
    }

    /// Where the Rust source starts when the dashes open no block.
    pub(crate) const fn after_shebang(&self) -> usize {
        self.after_shebang
    }
}

impl std::fmt::Display for Malformed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Read what comes before the Rust source of `text`.
pub fn read(text: &str) -> Result<Lead, Malformed> {
    let bom = if text.starts_with(BOM) {
        BOM.len_utf8()
    } else {
        0
    };
    let shift = |range: Range<usize>| range.start + bom..range.end + bom;
    match select(&text[bom..]) {
        Ok((source, frontmatter)) => Ok(Lead {
            source: source + bom,
            frontmatter: frontmatter.map(|frontmatter| Frontmatter {
                open: shift(frontmatter.open),
                info: shift(frontmatter.info),
                manifest: shift(frontmatter.manifest),
                close: shift(frontmatter.close),
            }),
        }),
        Err(malformed) => Err(Malformed {
            span: shift(malformed.span),
            after_shebang: malformed.after_shebang + bom,
            ..malformed
        }),
    }
}

/// Whether `text` is a cargo script: a file with a frontmatter block. A
/// script is one wherever it sits, as cargo reads it (D608 point 6). A
/// block cargo refuses still makes one, so its error is the block's.
#[must_use]
pub fn is_script(text: &str) -> bool {
    read(text).map_or_else(
        |malformed| malformed.opens(),
        |lead| lead.frontmatter.is_some(),
    )
}

/// Rust's `Pattern_White_Space`, as cargo reads it.
fn whitespace(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{000b}'
            | '\u{000c}'
            | '\r'
            | '\u{0085}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{200e}'
            | '\u{200f}'
            | '\t'
            | ' '
    )
}

fn horizontal(c: char) -> bool {
    matches!(c, ' ' | '\t')
}

/// The bytes of the whitespace-only lines `text` starts with.
fn whitespace_lines(text: &str) -> usize {
    let end = text.find(|c| !whitespace(c)).unwrap_or(text.len());
    text[..end].rfind('\n').map_or(0, |index| index + 1)
}

fn dashes(text: &str) -> usize {
    text.bytes().take_while(|&byte| byte == b'-').count()
}

fn select(raw: &str) -> Result<(usize, Option<Frontmatter>), Malformed> {
    let shebang = if raw
        .strip_prefix("#!")
        .is_some_and(|rest| !rest.trim_start().starts_with('['))
    {
        raw.find('\n').map_or(raw.len(), |index| index + 1)
    } else {
        0
    };
    let open_start = shebang + whitespace_lines(&raw[shebang..]);
    let length = dashes(&raw[open_start..]);
    let open_end = open_start + length;
    let malformed = |message: String, span: Range<usize>| Malformed {
        message,
        span,
        fence: length,
        after_shebang: shebang,
    };
    let unclosed = || {
        malformed(
            format!("unclosed frontmatter; expected `{}`", "-".repeat(length)),
            raw.len()..raw.len(),
        )
    };
    match length {
        0 => return Ok((shebang, None)),
        1 | 2 => {
            return Err(malformed(
                format!("found {length} `-` in rust frontmatter, expected at least 3"),
                raw.len()..raw.len(),
            ));
        }
        256.. => {
            return Err(malformed(
                format!(
                    "too many `-` symbols: frontmatter openings may be delimited by up to 255 \
                     `-` symbols, but found {length}"
                ),
                open_start..open_end,
            ));
        }
        _ => {}
    }
    let Some(info_length) = raw[open_end..].find('\n') else {
        return Err(unclosed());
    };
    let info_end = open_end + info_length;
    let info_raw = &raw[open_end..info_end];
    let info_raw = info_raw.strip_suffix('\r').unwrap_or(info_raw);
    let info_left = info_raw.trim_start_matches(horizontal);
    let info_start = open_end + info_raw.len() - info_left.len();
    let info = info_left.trim_end_matches(horizontal);
    let fence = format!("\n{}", "-".repeat(length));
    let input = &raw[info_end..];
    let Some(found) = input.find(&fence) else {
        // Cargo looks for a shorter closing fence to name the difference.
        for len in (2..length).rev() {
            if let Some(found) = input.find(&fence[..len]) {
                let close_start = info_end + found + 1;
                if raw[close_start..].get(..len).is_none() {
                    // Cargo panics here; it is a block cargo can't read.
                    return Err(unclosed());
                }
                return Err(malformed(
                    format!(
                        "closing code fence has {} less `-` than the opening fence",
                        length - len
                    ),
                    close_start..close_start + len,
                ));
            }
        }
        return Err(unclosed());
    };
    let close_start = info_end + found + 1;
    let close_end = close_start + length;
    let after_length = raw[close_end..]
        .find('\n')
        .map_or(raw.len() - close_end, |index| index + 1);
    let content_start = close_end + after_length;
    let after = &raw[close_end..content_start];
    let extra = dashes(after);
    if extra > 0 {
        return Err(malformed(
            format!("closing code fence has {extra} more `-` than the opening fence"),
            close_end..close_end + extra,
        ));
    }
    let after = after
        .strip_suffix("\r\n")
        .or_else(|| after.strip_suffix('\n'))
        .unwrap_or(after);
    let after_left = after.trim_start_matches(horizontal);
    let after_start = close_end + after.len() - after_left.len();
    let after = after_left.trim_end_matches(horizontal);
    if !after.is_empty() {
        return Err(malformed(
            "unexpected characters after frontmatter close".into(),
            after_start..after_start + after.len(),
        ));
    }
    let next_start = content_start + whitespace_lines(&raw[content_start..]);
    let next_length = dashes(&raw[next_start..]);
    if next_length > 0 {
        return Err(malformed(
            "only one frontmatter is supported".into(),
            next_start..next_start + next_length,
        ));
    }
    if !info.is_empty() && info != "cargo" {
        let span = info_start..info_start + info.len();
        let message = match info.strip_prefix("cargo,") {
            Some(attributes) => {
                format!("unsupported frontmatter infostring attributes: `{attributes}`")
            }
            None => format!(
                "unsupported frontmatter infostring `{info}`; specify `cargo` for embedding a \
                 manifest"
            ),
        };
        return Err(malformed(message, span));
    }
    Ok((
        content_start,
        Some(Frontmatter {
            open: open_start..open_end,
            info: info_start..info_start + info.len(),
            manifest: info_end + 1..close_start,
            close: close_start..close_end,
        }),
    ))
}

#[cfg(test)]
mod tests;
