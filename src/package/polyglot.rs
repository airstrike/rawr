//! Native modules of a marked crate: unmarked `.rs` modules beside the
//! marked ones in one module tree.
//!
//! Rawr walks the tree from the crate root under Rust's `mod` rules. A
//! native module is recorded, never compiled by Rawr; its own `mod`
//! declarations and paths are scanned syntactically so a native module that
//! depends on a marked one is refused at load (native code never depends on
//! marked code).

use std::ops::Range;

/// One file-module declaration (`mod name;`) found in Rust source.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Declaration {
    /// Inline module names enclosing the declaration, then its own name.
    pub(crate) path: Vec<String>,
    /// Byte range of the declaration's name.
    pub(crate) range: Range<usize>,
    /// The declaration carries `#[path = ...]`, so Rust, not the module
    /// tree, chooses its file.
    pub(crate) explicit_path: bool,
    /// The declaration, or an inline module enclosing it, carries a `#[cfg]`
    /// that is false whatever the target: `test` (the app crate never builds
    /// as a test), `any()`, and `not`/`all`/`any` of those. Rust never loads
    /// it, so neither does Rawr. Other predicates count as enabled.
    pub(crate) disabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Token<'a> {
    Ident(&'a str, usize),
    Punct(u8),
}

/// Every `mod name;` in Rust source, with enclosing inline modules
/// (`mod a { mod b; }` gives `a::b`). Comments, strings, chars and raw
/// strings are skipped; no parse is needed.
pub(crate) fn declarations(text: &str) -> Vec<Declaration> {
    let tokens = tokens(text);
    let mut declarations = Vec::new();
    // Inline modules: (name, brace depth inside it, disabled).
    let mut inline: Vec<(String, usize, bool)> = Vec::new();
    let mut depth = 0_usize;
    let mut explicit_path = false;
    let mut disabled = false;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            Token::Punct(b'#') => {
                // `#[path = "..."]`, `#[cfg(...)]` or `#![...]`
                let mut next = index + 1;
                if tokens.get(next) == Some(&Token::Punct(b'!')) {
                    next += 1;
                }
                if tokens.get(next) == Some(&Token::Punct(b'[')) {
                    match tokens.get(next + 1) {
                        Some(Token::Ident("path", _)) => explicit_path = true,
                        Some(Token::Ident("cfg", _))
                            if tokens.get(next + 2) == Some(&Token::Punct(b'(')) =>
                        {
                            let mut cursor = next + 3;
                            if cfg(&tokens, &mut cursor) == Some(false) {
                                disabled = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Token::Punct(b'{') => {
                depth += 1;
                explicit_path = false;
                disabled = false;
            }
            Token::Punct(b'}') => {
                if inline.last().is_some_and(|(_, inner, _)| *inner == depth) {
                    inline.pop();
                }
                depth = depth.saturating_sub(1);
                explicit_path = false;
                disabled = false;
            }
            Token::Punct(b';') => {
                explicit_path = false;
                disabled = false;
            }
            Token::Ident("mod", _) => {
                if let Some(Token::Ident(name, start)) = tokens.get(index + 1).copied() {
                    let enclosed = inline.iter().any(|(_, _, disabled)| *disabled);
                    match tokens.get(index + 2) {
                        Some(Token::Punct(b';')) => {
                            let mut path = inline
                                .iter()
                                .map(|(name, _, _)| name.clone())
                                .collect::<Vec<_>>();
                            path.push(name.to_owned());
                            declarations.push(Declaration {
                                path,
                                range: start..start + name.len(),
                                explicit_path,
                                disabled: disabled || enclosed,
                            });
                            explicit_path = false;
                            disabled = false;
                            index += 3;
                            continue;
                        }
                        Some(Token::Punct(b'{')) => {
                            depth += 1;
                            inline.push((name.to_owned(), depth, disabled || enclosed));
                            explicit_path = false;
                            disabled = false;
                            index += 3;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    declarations
}

/// One path in Rust source that starts at `crate` or `super`: a place a
/// native module may reach outside itself.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Reference {
    /// Inline modules enclosing the path.
    pub(crate) inline: Vec<String>,
    /// The path's segments, `crate` or the leading `super`s included.
    pub(crate) segments: Vec<String>,
    /// Byte range of the whole path.
    pub(crate) range: Range<usize>,
    /// The path sits in an item or inline module whose `#[cfg]` is false
    /// on every target (see [`Declaration::disabled`]).
    pub(crate) disabled: bool,
}

/// Every `crate::…` and `super::…` path in Rust source. A `use` group
/// (`use crate::{a, b::c}`) gives one path per item. Visibility paths
/// (`pub(in crate::a)`) are not references and are left out.
pub(crate) fn references(text: &str) -> Vec<Reference> {
    let tokens = tokens(text);
    let colons = |at: usize| {
        tokens.get(at) == Some(&Token::Punct(b':'))
            && tokens.get(at + 1) == Some(&Token::Punct(b':'))
    };
    let end_of = |at: usize| match tokens[at] {
        Token::Ident(name, start) => start + name.len(),
        Token::Punct(_) => 0,
    };
    // One `a::b::c` run of identifiers from `at`: the segments and the
    // index of the last identifier.
    let run = |mut at: usize| {
        let mut segments = Vec::new();
        let mut last = at;
        while let Some(Token::Ident(name, _)) = tokens.get(at) {
            segments.push((*name).to_owned());
            last = at;
            if colons(at + 1) && matches!(tokens.get(at + 3), Some(Token::Ident(..))) {
                at += 3;
            } else {
                break;
            }
        }
        (segments, last)
    };
    let mut references = Vec::new();
    let mut inline: Vec<(String, usize, bool)> = Vec::new();
    let mut depth = 0_usize;
    let mut disabled = false;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            Token::Punct(b'#') => {
                let mut next = index + 1;
                if tokens.get(next) == Some(&Token::Punct(b'!')) {
                    next += 1;
                }
                if tokens.get(next) == Some(&Token::Punct(b'['))
                    && tokens
                        .get(next + 1)
                        .is_some_and(|token| matches!(token, Token::Ident("cfg", _)))
                    && tokens.get(next + 2) == Some(&Token::Punct(b'('))
                {
                    let mut cursor = next + 3;
                    if cfg(&tokens, &mut cursor) == Some(false) {
                        disabled = true;
                    }
                }
            }
            Token::Punct(b'{') => {
                depth += 1;
                if let (Some(Token::Ident(name, _)), Some(Token::Ident("mod", _))) = (
                    index.checked_sub(1).map(|at| tokens[at]),
                    index.checked_sub(2).map(|at| tokens[at]),
                ) {
                    let enclosed = inline.iter().any(|(_, _, disabled)| *disabled);
                    inline.push((name.to_owned(), depth, disabled || enclosed));
                }
                disabled = false;
            }
            Token::Punct(b'}') => {
                if inline.last().is_some_and(|(_, inner, _)| *inner == depth) {
                    inline.pop();
                }
                depth = depth.saturating_sub(1);
                disabled = false;
            }
            Token::Punct(b';') => disabled = false,
            Token::Ident(first @ ("crate" | "super"), start)
                if colons(index + 1)
                    && !(index >= 2 && colons(index - 2))
                    && !(index >= 1 && matches!(tokens[index - 1], Token::Ident("in", _))) =>
            {
                let inside = inline
                    .iter()
                    .map(|(name, _, _)| name.clone())
                    .collect::<Vec<_>>();
                let off = disabled || inline.iter().any(|(_, _, disabled)| *disabled);
                let (prefix, last) = run(index);
                debug_assert_eq!(prefix.first().map(String::as_str), Some(first));
                let group = colons(last + 1) && tokens.get(last + 3) == Some(&Token::Punct(b'{'));
                if !group {
                    references.push(Reference {
                        inline: inside,
                        segments: prefix,
                        range: start..end_of(last),
                        disabled: off,
                    });
                    index = last + 1;
                    continue;
                }
                // `prefix::{item, item::path, self, nested::{…}}`: each
                // item's leading path.
                let mut cursor = last + 4;
                let mut nesting = 1_usize;
                let mut item_start = true;
                while cursor < tokens.len() && nesting > 0 {
                    match tokens[cursor] {
                        Token::Punct(b'{') => nesting += 1,
                        Token::Punct(b'}') => nesting -= 1,
                        Token::Punct(b',') if nesting == 1 => item_start = true,
                        Token::Ident(..) if nesting == 1 && item_start => {
                            item_start = false;
                            let (rest, item_last) = run(cursor);
                            let mut segments = prefix.clone();
                            segments.extend(rest.into_iter().filter(|segment| segment != "self"));
                            references.push(Reference {
                                inline: inside.clone(),
                                segments,
                                range: start..end_of(item_last),
                                disabled: off,
                            });
                            cursor = item_last;
                        }
                        _ => {}
                    }
                    cursor += 1;
                }
                index = cursor;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    references
}

/// A native module as the generated app crate compiles it. The marked
/// modules are part of the same crate, so what Rust lets them see Rawr must
/// see through the generated crate's public API: `pub(crate)`/`pub(in crate)`
/// become `pub`, and so do `pub(super)`/`pub(in super)` at the top level of a
/// module a marked module (or the crate root) declares. Private items and
/// other restrictions stay as written. The keyword is padded to its old width
/// so lines and columns still match the original. The copy lives elsewhere,
/// so relative `include!`/`include_str!`/`include_bytes!` and `#[path]`
/// literals become absolute against the original `file`.
#[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
pub(crate) fn widened(text: &str, declared_by_rawr: bool, file: &std::path::Path) -> String {
    let bytes = text.as_bytes();
    let skip = |mut at: usize| {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        at
    };
    let ident = |at: usize| {
        let end = at
            + bytes[at.min(bytes.len())..]
                .iter()
                .take_while(|byte| **byte == b'_' || byte.is_ascii_alphanumeric())
                .count();
        (&text[at.min(end)..end], end)
    };
    // A plain string literal at `at` (no escapes): its contents' range.
    let literal = |at: usize| {
        if bytes.get(at) != Some(&b'"') {
            return None;
        }
        let end = at + 1 + bytes[at + 1..].iter().position(|byte| *byte == b'"')?;
        (!text[at + 1..end].contains('\\')).then_some(at + 1..end)
    };
    let directory = file.parent().unwrap_or(std::path::Path::new(""));
    let mod_rs = file.file_name().is_some_and(|name| name == "mod.rs");
    let absolute = |base: &std::path::Path, range: Range<usize>| {
        let path = std::path::Path::new(&text[range.clone()]);
        (!path.is_absolute()).then(|| (range, format!("{}", base.join(path).display())))
    };

    let tokens = tokens(text);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut inline: Vec<(String, usize)> = Vec::new();
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match *token {
            Token::Punct(b'{') => {
                depth += 1;
                if let (Some(Token::Ident(name, _)), Some(Token::Ident("mod", _))) = (
                    index.checked_sub(1).map(|at| tokens[at]),
                    index.checked_sub(2).map(|at| tokens[at]),
                ) {
                    inline.push((name.to_owned(), depth));
                }
            }
            Token::Punct(b'}') => {
                if inline.last().is_some_and(|(_, inner)| *inner == depth) {
                    inline.pop();
                }
                depth = depth.saturating_sub(1);
            }
            Token::Ident("pub", start) => {
                let open = skip(start + 3);
                if bytes.get(open) != Some(&b'(') {
                    continue;
                }
                let (mut scope, mut end) = ident(skip(open + 1));
                if scope == "in" {
                    (scope, end) = ident(skip(end));
                }
                let close = skip(end);
                let wider = match scope {
                    "crate" => true,
                    "super" => declared_by_rawr && inline.is_empty(),
                    _ => false,
                };
                if wider && bytes.get(close) == Some(&b')') {
                    edits.push((
                        start..close + 1,
                        format!("{:width$}", "pub", width = close + 1 - start),
                    ));
                }
            }
            Token::Ident(name @ ("include" | "include_str" | "include_bytes"), start) => {
                let bang = skip(start + name.len());
                if bytes.get(bang) != Some(&b'!') {
                    continue;
                }
                let open = skip(bang + 1);
                if bytes.get(open) == Some(&b'(')
                    && let Some(edit) =
                        literal(skip(open + 1)).and_then(|range| absolute(directory, range))
                {
                    edits.push(edit);
                }
            }
            Token::Ident("path", start)
                if index >= 2
                    && tokens[index - 1] == Token::Punct(b'[')
                    && tokens[index - 2] == Token::Punct(b'#') =>
            {
                let equals = skip(start + 4);
                if bytes.get(equals) != Some(&b'=') {
                    continue;
                }
                // Rust: outside inline modules, relative to the file's
                // directory; inside them, to the directory the inline
                // modules name (under the file stem for a non-`mod.rs` file).
                let mut base = directory.to_path_buf();
                if !inline.is_empty() {
                    if !mod_rs && let Some(stem) = file.file_stem() {
                        base.push(stem);
                    }
                    base.extend(inline.iter().map(|(name, _)| name));
                }
                if let Some(edit) =
                    literal(skip(equals + 1)).and_then(|range| absolute(&base, range))
                {
                    edits.push(edit);
                }
            }
            _ => {}
        }
    }
    let mut widened = String::with_capacity(text.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        widened.push_str(&text[cursor..range.start]);
        widened.push_str(&replacement);
        cursor = range.end;
    }
    widened.push_str(&text[cursor..]);
    widened
}

/// One `cfg` predicate starting at `cursor`, left just past it: `Some(false)`
/// when false on every target, `Some(true)` when true on every target, `None`
/// when it depends on the build (string values are not tokens, so
/// `feature = "x"` is `feature =`).
fn cfg(tokens: &[Token<'_>], cursor: &mut usize) -> Option<bool> {
    let Some(Token::Ident(name, _)) = tokens.get(*cursor).copied() else {
        return None;
    };
    *cursor += 1;
    if tokens.get(*cursor) == Some(&Token::Punct(b'=')) {
        *cursor += 1;
        return None;
    }
    if tokens.get(*cursor) != Some(&Token::Punct(b'(')) {
        return (name == "test").then_some(false);
    }
    *cursor += 1;
    let mut values = Vec::new();
    while *cursor < tokens.len() && tokens[*cursor] != Token::Punct(b')') {
        let before = *cursor;
        values.push(cfg(tokens, cursor));
        if tokens.get(*cursor) == Some(&Token::Punct(b',')) || *cursor == before {
            *cursor += 1;
        }
    }
    *cursor += 1;
    match name {
        "not" => match values.as_slice() {
            [value] => value.map(|value| !value),
            _ => None,
        },
        "all" if values.contains(&Some(false)) => Some(false),
        "all" if values.iter().all(|value| *value == Some(true)) => Some(true),
        "any" if values.contains(&Some(true)) => Some(true),
        "any" if values.iter().all(|value| *value == Some(false)) => Some(false),
        _ => None,
    }
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let mut nesting = 0_usize;
                while index < bytes.len() {
                    if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                        nesting += 1;
                        index += 2;
                    } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                        nesting -= 1;
                        index += 2;
                        if nesting == 0 {
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
            }
            b'"' => index = skip_string(bytes, index + 1),
            b'\'' => {
                // A char literal (`'a'`, `'\n'`) or a lifetime (`'a`).
                if bytes.get(index + 1) == Some(&b'\\') {
                    index += 2;
                    while index < bytes.len() && bytes[index] != b'\'' {
                        index += 1;
                    }
                    index += 1;
                } else if bytes.get(index + 2) == Some(&b'\'') {
                    index += 3;
                } else {
                    let width = text[index + 1..].chars().next().map_or(1, char::len_utf8);
                    if bytes.get(index + 1 + width) == Some(&b'\'') {
                        index += 2 + width;
                    } else {
                        index += 1;
                    }
                }
            }
            b'r' | b'b' if raw_string(bytes, index).is_some() => {
                index = raw_string(bytes, index).unwrap_or(bytes.len());
            }
            b'b' if bytes.get(index + 1) == Some(&b'"') => index = skip_string(bytes, index + 2),
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                let mut start = index;
                if byte == b'r' && bytes.get(index + 1) == Some(&b'#') {
                    start = index + 2;
                }
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end] == b'_' || bytes[end].is_ascii_alphanumeric())
                {
                    end += 1;
                }
                if end == start {
                    end = index + 1;
                    start = index;
                }
                tokens.push(Token::Ident(&text[start..end], start));
                index = end;
            }
            byte if byte.is_ascii_whitespace() || !byte.is_ascii() => index += 1,
            byte => {
                tokens.push(Token::Punct(byte));
                index += 1;
            }
        }
    }
    tokens
}

fn skip_string(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    index
}

/// The end of a raw string (`r"…"`, `r#"…"#`, `br#"…"#`) starting at `index`.
fn raw_string(bytes: &[u8], index: usize) -> Option<usize> {
    let mut cursor = index;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let mut hashes = 0;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes[cursor + 1..]
                .iter()
                .take(hashes)
                .filter(|byte| **byte == b'#')
                .count()
                == hashes
        {
            return Some(cursor + 1 + hashes);
        }
        cursor += 1;
    }
    Some(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::declarations;

    fn paths(text: &str) -> Vec<(String, bool)> {
        declarations(text)
            .into_iter()
            .map(|declaration| (declaration.path.join("::"), declaration.explicit_path))
            .collect()
    }

    #[test]
    fn rust_mod_declarations_skip_comments_strings_and_nest_inline_modules() {
        let text = r##"
//! Docs: mod fake;
/* mod hidden; /* nested */ mod hidden2; */
pub mod model;
mod a { pub(crate) mod b; fn f() -> &'static str { "mod nope;" } mod c { mod d; } }
#[path = "elsewhere.rs"]
mod moved;
const RAW: &str = r#"mod raw;"#;
const C: char = '"';
mod last;
"##;
        assert_eq!(
            paths(text),
            [
                ("model".to_owned(), false),
                ("a::b".to_owned(), false),
                ("a::c::d".to_owned(), false),
                ("moved".to_owned(), true),
                ("last".to_owned(), false),
            ]
        );
    }

    #[test]
    fn crate_visibility_widens_and_relative_paths_become_absolute() {
        use super::widened;
        use std::path::Path;
        let text = r#"pub(crate) struct Thing { pub(crate) count: i64, secret: i64 }
pub(super) fn up() {}
pub( in crate ) fn spaced() {}
pub(in crate::model) fn scoped() {}
pub(self) fn mine() {}
const S: &str = "pub(crate) fn no()";
const T: &str = include_str!("data.txt");
#[path = "other.rs"]
mod other;
mod inline { pub(super) fn kept() {} #[path = "deep.rs"] mod deep; }
"#;
        let file = Path::new("/app/src/model.rs");
        let top = widened(text, true, file);
        let widths = |text: &str| text.lines().take(5).map(str::len).collect::<Vec<_>>();
        assert_eq!(widths(&top), widths(text), "{top}");
        assert!(top.starts_with("pub        struct Thing { pub        count: i64, secret: i64 }\npub        fn up()"), "{top}");
        assert!(top.contains("pub             fn spaced()"), "{top}");
        assert!(
            top.contains("pub(in crate::model) fn scoped()") && top.contains("pub(self) fn mine()"),
            "{top}"
        );
        assert!(top.contains(r#""pub(crate) fn no()""#), "{top}");
        assert!(
            top.contains(r#"include_str!("/app/src/data.txt")"#),
            "{top}"
        );
        assert!(top.contains(r#"#[path = "/app/src/other.rs"]"#), "{top}");
        assert!(top.contains("pub(super) fn kept()"), "{top}");
        assert!(
            top.contains(r#"#[path = "/app/src/model/inline/deep.rs"]"#),
            "{top}"
        );
        // A module a `.rs` module declares: its `super` is Rust, not Rawr.
        assert!(widened(text, false, file).contains("pub(super) fn up()"));
    }

    #[test]
    fn cfg_disabled_mod_declarations_are_marked() {
        let text = r#"
#[cfg(test)]
mod tests;
#[cfg(not(test))]
mod live;
#[cfg(all(test, feature = "x"))]
mod both;
#[cfg(feature = "x")]
mod featured;
#[cfg(any())]
mod never;
#[cfg(test)]
mod inline { mod inner; }
#[cfg(target_os = "macos")]
mod mac;
mod plain;
"#;
        let disabled = declarations(text)
            .into_iter()
            .map(|declaration| (declaration.path.join("::"), declaration.disabled))
            .collect::<Vec<_>>();
        assert_eq!(
            disabled,
            [
                ("tests".to_owned(), true),
                ("live".to_owned(), false),
                ("both".to_owned(), true),
                ("featured".to_owned(), false),
                ("never".to_owned(), true),
                ("inline::inner".to_owned(), true),
                ("mac".to_owned(), false),
                ("plain".to_owned(), false),
            ]
        );
    }
    #[test]
    fn references_are_crate_and_super_paths_with_their_inline_modules() {
        let text = r#"
use crate::{Item, view::{self, Panel}, money::twice as double};
pub(in crate::model) fn scoped<'a>(x: &'a str) -> super::Up { crate::deep::f(); x.len() }
// crate::commented::Out
const S: &str = "crate::quoted::Out";
fn g() { other::crate_like(); a::super_b(); }
mod inner { fn h() { super::super::far(); } }
#[cfg(test)]
mod tests { use super::*; }
#[cfg(test)]
use crate::only_tests;
"#;
        let found = super::references(text)
            .into_iter()
            .map(|reference| {
                (
                    reference.inline.join("::"),
                    reference.segments.join("::"),
                    text[reference.range].to_owned(),
                    reference.disabled,
                )
            })
            .collect::<Vec<_>>();
        let expected = [
            ("", "crate::Item", "crate::{Item", false),
            ("", "crate::view", "crate::{Item, view", false),
            (
                "",
                "crate::money::twice",
                "crate::{Item, view::{self, Panel}, money::twice",
                false,
            ),
            ("", "super::Up", "super::Up", false),
            ("", "crate::deep::f", "crate::deep::f", false),
            ("inner", "super::super::far", "super::super::far", false),
            ("tests", "super", "super", true),
            ("", "crate::only_tests", "crate::only_tests", true),
        ];
        assert_eq!(
            found,
            expected.map(|(inline, segments, text, disabled)| (
                inline.to_owned(),
                segments.to_owned(),
                text.to_owned(),
                disabled
            ))
        );
    }
}
