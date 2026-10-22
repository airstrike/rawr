// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: path/LICENSE-MIT and path/LICENSE-APACHE beside this module.

//! Complete pinned frontmatter expansion, before TOML interpretation.

use super::super::{session::Session, storage, Diagnostic, Error, Span, Step, Tag};
use rawr_toml::document;
use std::alloc::Layout;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    ShortOpening(usize),
    LongOpening(usize),
    Unclosed,
    Fewer(usize),
    More(usize),
    Trailing,
    Multiple,
    Attributes,
    Info,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership::manifest) struct Invalid {
    kind: Kind,
    visible: [Option<Span>; 2],
}

fn reject<A: document::Admission, T>(
    kind: Kind,
    primary: Span,
    visible: [Option<Span>; 2],
    session: &mut Session<'_, A>,
) -> Step<T> {
    session.reject(Diagnostic::at(
        Tag::Frontmatter(Invalid { kind, visible }),
        Some(primary),
    ))
}

fn span(start: usize, end: usize) -> Span {
    Span { start, end }
}
fn horizontal(c: char) -> bool {
    matches!(c, ' ' | '\t')
}
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
fn whitespace_lines(text: &str) -> usize {
    let end = text.find(|c| !whitespace(c)).unwrap_or(text.len());
    text[..end].rfind('\n').map_or(0, |index| index + 1)
}
fn dashes(text: &str) -> usize {
    text.bytes().take_while(|&byte| byte == b'-').count()
}

struct Selected {
    end: usize,
    comment: Option<usize>,
}

fn select<A: document::Admission>(raw: &str, session: &mut Session<'_, A>) -> Step<Selected> {
    session.items(1)?;
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
    let open = span(open_start, open_end);
    let visible = [Some(open), None];
    match length {
        0 => {
            return Ok(Selected {
                end: shebang,
                comment: None,
            })
        }
        1 | 2 => {
            return reject(
                Kind::ShortOpening(length),
                span(raw.len(), raw.len()),
                visible,
                session,
            )
        }
        256.. => return reject(Kind::LongOpening(length), open, [None; 2], session),
        _ => {}
    }
    let Some(info_length) = raw[open_end..].find('\n') else {
        return reject(Kind::Unclosed, span(raw.len(), raw.len()), visible, session);
    };
    let info_end = open_end + info_length;
    let info_raw = &raw[open_end..info_end];
    let info_raw = info_raw.strip_suffix('\r').unwrap_or(info_raw);
    let info_left = info_raw.trim_start_matches(horizontal);
    let info_start = open_end + info_raw.len() - info_left.len();
    let info = info_left.trim_end_matches(horizontal);
    // Source creates a newline-prefixed fence string. A fixed stack buffer
    // avoids that temporary allocation, while preserving every search order.
    let mut pattern = [b'-'; 256];
    pattern[0] = b'\n';
    let pattern = match std::str::from_utf8(&pattern[..length + 1]) {
        Ok(pattern) => pattern,
        Err(_) => return session.fail(Error::State),
    };
    let input = &raw[info_end..];
    let Some(found) = input.find(pattern) else {
        for len in (2..length).rev() {
            if let Some(found) = input.find(&pattern[..len]) {
                let close_start = info_end + found + 1;
                if raw[close_start..].get(..len).is_none() {
                    let Ok(bytes) = u8::try_from(len) else {
                        return session.fail(Error::State);
                    };
                    return session.source_frontmatter(close_start, bytes);
                }
                return reject(
                    Kind::Fewer(length - len),
                    span(close_start, close_start + len),
                    visible,
                    session,
                );
            }
        }
        return reject(Kind::Unclosed, span(raw.len(), raw.len()), visible, session);
    };
    let close_start = info_end + found + 1;
    let close_end = close_start + length;
    let close = span(close_start, close_end);
    let after_length = raw[close_end..]
        .find('\n')
        .map_or(raw.len() - close_end, |index| index + 1);
    let content_start = close_end + after_length;
    let after = &raw[close_end..content_start];
    let extra = dashes(after);
    if extra > 0 {
        return reject(
            Kind::More(extra),
            span(close_end, close_end + extra),
            visible,
            session,
        );
    }
    let after = after
        .strip_suffix("\r\n")
        .or_else(|| after.strip_suffix('\n'))
        .unwrap_or(after);
    let after_left = after.trim_start_matches(horizontal);
    let after_start = close_end + after.len() - after_left.len();
    let after = after_left.trim_end_matches(horizontal);
    if !after.is_empty() {
        return reject(
            Kind::Trailing,
            span(after_start, after_start + after.len()),
            visible,
            session,
        );
    }
    let next_start = content_start + whitespace_lines(&raw[content_start..]);
    let next_length = dashes(&raw[next_start..]);
    if next_length > 0 {
        return reject(
            Kind::Multiple,
            span(next_start, next_start + next_length),
            [Some(open), Some(close)],
            session,
        );
    }
    if !info.is_empty() && info != "cargo" {
        let kind = if info.starts_with("cargo,") {
            Kind::Attributes
        } else {
            Kind::Info
        };
        return reject(
            kind,
            span(info_start, info_start + info.len()),
            [Some(close), None],
            session,
        );
    }
    Ok(Selected {
        end: close_start,
        comment: Some(open_start),
    })
}

pub(super) fn expand<A: document::Admission>(
    raw: &str,
    session: &mut Session<'_, A>,
) -> Step<String> {
    let selected = select(raw, session)?;
    let Some(length) = selected
        .end
        .checked_add(usize::from(selected.comment.is_some()))
    else {
        return session.fail(Error::Overflow);
    };
    // This is whole-document storage, not a decoded TOML scalar. The loader
    // separately admits manifest extent; retain only actual allocation here.
    if length == 0 {
        return Ok(String::new());
    }
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut output = String::new();
    if let Err(error) = output.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    #[cfg(test)]
    storage::ALLOCATIONS.with(|count| count.set(count.get() + 1));
    if let Some(comment) = selected.comment {
        output.push_str(&raw[..comment]);
        output.push('#');
        output.push_str(&raw[comment..selected.end]);
    } else {
        output.push_str(&raw[..selected.end]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
