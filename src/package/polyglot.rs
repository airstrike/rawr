//! Native modules of a marked crate: unmarked `.rs` modules beside the
//! marked ones in one module tree.
//!
//! Rawr walks the tree from the crate root under Rust's `mod` rules. A
//! native module is recorded, never compiled by Rawr; its own `mod`
//! declarations and paths are scanned syntactically so a native module that
//! depends on a marked one is refused at load (native code never depends on
//! marked code).

use std::ops::Range;

use crate::project::cargo::features::{Enabled, Operator};

/// One file-module declaration (`mod name;`) found in Rust source.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Declaration {
    /// Inline module names enclosing the declaration, then its own name.
    pub(crate) path: Vec<String>,
    /// Byte range of the declaration's name.
    pub(crate) range: Range<usize>,
    /// The declaration carries `#[path = ...]`, written alone or applied by
    /// a `#[cfg_attr(..)]` that holds, so Rust, not the module tree,
    /// chooses its file.
    pub(crate) explicit_path: bool,
    /// The declaration, or an inline module enclosing it, carries a `#[cfg]`
    /// (alone or applied by a `#[cfg_attr(..)]` that holds) that does not
    /// hold under the package's features and its target's options, as the
    /// item parser reads it ([`Enabled::cfg`]). The app crate never builds
    /// as a test. Rust never loads it, so neither does Rawr.
    pub(crate) disabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Token<'a> {
    Ident(&'a str, usize),
    Punct(u8),
    /// A string literal's contents, as written: what a `cfg` value or a
    /// `#[path]` reads.
    Str(&'a str),
}

/// What one outer or inner attribute says of the item it is on.
#[derive(Clone, Copy, Debug, Default)]
struct Attribute {
    /// It is `#[path = ..]`.
    path: bool,
    /// It is a `#[cfg(..)]` that does not hold.
    disabled: bool,
}

/// Every `mod name;` in Rust source, with enclosing inline modules
/// (`mod a { mod b; }` gives `a::b`), its `cfg`s read under `features`.
/// Comments, strings, chars and raw strings are skipped; no parse is
/// needed.
pub(crate) fn declarations(text: &str, features: &Enabled) -> Vec<Declaration> {
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
                let (attribute, end) = attribute(&tokens, index, features);
                explicit_path |= attribute.path;
                disabled |= attribute.disabled;
                index = end;
                continue;
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
    /// The path sits in an item or inline module whose `#[cfg]` does not
    /// hold (see [`Declaration::disabled`]).
    pub(crate) disabled: bool,
}

/// Every `crate::…` and `super::…` path in Rust source, its `cfg`s read
/// under `features`. A `use` group (`use crate::{a, b::c}`) gives one path
/// per item. Visibility paths (`pub(in crate::a)`) are not references and
/// are left out.
pub(crate) fn references(text: &str, features: &Enabled) -> Vec<Reference> {
    let tokens = tokens(text);
    let colons = |at: usize| {
        tokens.get(at) == Some(&Token::Punct(b':'))
            && tokens.get(at + 1) == Some(&Token::Punct(b':'))
    };
    let end_of = |at: usize| match tokens[at] {
        Token::Ident(name, start) => start + name.len(),
        Token::Punct(_) | Token::Str(_) => 0,
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
                // Read on past the `#`: an attribute's own paths
                // (`#[derive(crate::Make)]`) are references too.
                disabled |= attribute(&tokens, index, features).0.disabled;
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
/// literals become absolute against the original `file`, and
/// `env!("CARGO_MANIFEST_DIR")` names the package's own directory,
/// `package`, so a path through it reaches what Cargo's build reaches, even
/// outside the package (#1136).
#[cfg_attr(
    not(all(feature = "rust-import", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(crate) fn widened(
    text: &str,
    declared_by_rawr: bool,
    file: &std::path::Path,
    package: &std::path::Path,
) -> String {
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
            Token::Ident(name @ ("env" | "option_env"), start) => {
                let Some(from) = std_path_start(text, start) else {
                    continue;
                };
                let bang = skip(start + name.len());
                if bytes.get(bang) != Some(&b'!') || bytes.get(skip(bang + 1)) != Some(&b'(') {
                    continue;
                }
                let Some(range) = literal(skip(skip(bang + 1) + 1)) else {
                    continue;
                };
                let close = skip(range.end + 1);
                if &text[range] != "CARGO_MANIFEST_DIR" || bytes.get(close) != Some(&b')') {
                    continue;
                }
                let directory = format!("{:?}", package.display().to_string());
                edits.push((
                    from..close + 1,
                    if name == "env" {
                        directory
                    } else {
                        format!("::core::option::Option::Some({directory})")
                    },
                ));
            }
            Token::Ident("path", start)
                if index >= 2
                    && (tokens[index - 1] == Token::Punct(b'[')
                        && tokens[index - 2] == Token::Punct(b'#')
                        || tokens[index - 1] == Token::Punct(b',')
                            && in_cfg_attr(&tokens, index)) =>
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

/// `text`, a crate root, with `pub ` before each of its own items that has
/// no visibility: its modules, functions, types, constants, statics, traits
/// and `use` declarations. The marked modules are the root's descendants, so
/// rustc lets them name every one of these; the runner reaches them from
/// outside the crate, so they must be public there (#1185). An `impl`, an
/// `extern` block or crate, a macro and an item written `pub(self)` are
/// left as written.
#[cfg_attr(
    not(all(feature = "rust-import", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(crate) fn root_items_public(text: &str) -> String {
    let inserts = private_items(&tokens(text))
        .into_iter()
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    inserted(text, &inserts, "pub ")
}

/// The top-level `use` declarations of `text`, a native crate root, alone,
/// each with its visibility: what a marked module's path through `crate`
/// may stand for, as the root's `pub use theme::Theme;` makes
/// `crate::Theme` the marked `theme::Theme`.
pub(crate) fn uses_text(text: &str) -> String {
    let tokens = tokens(text);
    let mut uses = String::new();
    let mut depth = 0_usize;
    let mut start = true;
    let mut index = 0;
    while index < tokens.len() {
        if depth == 0 && start {
            if tokens[index] == Token::Punct(b'#') {
                index = attribute_end(&tokens, index);
                continue;
            }
            start = false;
            if let Some(end) = use_end(&tokens, index) {
                for token in &tokens[index..=end] {
                    match *token {
                        Token::Ident(word, _) => {
                            uses.push(' ');
                            uses.push_str(word);
                        }
                        Token::Punct(byte) => uses.push(char::from(byte)),
                        Token::Str(_) => {}
                    }
                }
                uses.push('\n');
                index = end + 1;
                start = true;
                continue;
            }
        }
        match tokens[index] {
            Token::Punct(b'{' | b'(' | b'[') => depth += 1,
            Token::Punct(close @ (b'}' | b')' | b']')) => {
                depth = depth.saturating_sub(1);
                start = depth == 0 && close == b'}';
            }
            Token::Punct(b';') if depth == 0 => start = true,
            _ => {}
        }
        index += 1;
    }
    uses
}

/// One name a module's top-level `use` declaration binds: `use
/// iced::wgpu;` binds `wgpu` to `iced::wgpu`. A glob and a `_` bind no
/// name and are left out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    /// The declaration has a visibility (`pub`, `pub(crate)`, ...).
    pub(crate) visible: bool,
    /// The path starts at `::`: an extern crate, by its name.
    pub(crate) global: bool,
    /// The path's segments, as written.
    pub(crate) path: Vec<String>,
    /// The name it binds: the last segment, or its `as` rename.
    pub(crate) name: String,
}

impl Binding {
    /// The binding as its own declaration: `use iced::wgpu as wgpu;`,
    /// visible to the crate when it was written with a visibility.
    pub(crate) fn declaration(&self) -> String {
        format!(
            "{}use {}{} as {};\n",
            if self.visible { "pub(crate) " } else { "" },
            if self.global { "::" } else { "" },
            self.path.join("::"),
            self.name,
        )
    }
}

/// The names the top-level `use` declarations of `text`, a module, bind,
/// under `features`: a declaration whose `#[cfg]` does not hold binds
/// none.
pub(crate) fn bindings(text: &str, features: &Enabled) -> Vec<Binding> {
    let tokens = tokens(text);
    let mut bindings = Vec::new();
    let mut depth = 0_usize;
    let mut start = true;
    let mut disabled = false;
    let mut index = 0;
    while index < tokens.len() {
        if depth == 0 && start {
            if tokens[index] == Token::Punct(b'#') {
                let (attribute, end) = attribute(&tokens, index, features);
                disabled |= attribute.disabled;
                index = end;
                continue;
            }
            start = false;
            if let Some(end) = use_end(&tokens, index) {
                if !disabled {
                    let visible = matches!(tokens[index], Token::Ident("pub", _));
                    let at = (index..end)
                        .find(|at| matches!(tokens[*at], Token::Ident("use", _)))
                        .map_or(end, |at| at + 1);
                    let tree = UseTree {
                        tokens: &tokens[..end],
                        visible,
                    };
                    tree.read(at, &mut Vec::new(), false, &mut bindings);
                }
                disabled = false;
                index = end + 1;
                start = true;
                continue;
            }
            disabled = false;
        }
        match tokens[index] {
            Token::Punct(b'{' | b'(' | b'[') => depth += 1,
            Token::Punct(close @ (b'}' | b')' | b']')) => {
                depth = depth.saturating_sub(1);
                start = depth == 0 && close == b'}';
            }
            Token::Punct(b';') if depth == 0 => start = true,
            _ => {}
        }
        index += 1;
    }
    bindings
}

/// The tokens of one `use` declaration's tree, up to its `;`.
struct UseTree<'t, 'a> {
    tokens: &'t [Token<'a>],
    visible: bool,
}

impl UseTree<'_, '_> {
    fn colons(&self, at: usize) -> bool {
        self.tokens.get(at) == Some(&Token::Punct(b':'))
            && self.tokens.get(at + 1) == Some(&Token::Punct(b':'))
    }

    /// The bindings of the tree at `at` below `prefix`, and the index past
    /// it.
    fn read(
        &self,
        mut at: usize,
        prefix: &mut Vec<String>,
        mut global: bool,
        bindings: &mut Vec<Binding>,
    ) -> usize {
        let outer = prefix.len();
        if prefix.is_empty() && self.colons(at) {
            global = true;
            at += 2;
        }
        loop {
            match self.tokens.get(at) {
                Some(Token::Ident(segment, _)) => {
                    prefix.push((*segment).to_owned());
                    at += 1;
                    if self.colons(at) {
                        at += 2;
                        continue;
                    }
                    break;
                }
                Some(Token::Punct(b'{')) => {
                    at += 1;
                    while at < self.tokens.len() && self.tokens[at] != Token::Punct(b'}') {
                        at = self.read(at, prefix, global, bindings);
                        if self.tokens.get(at) == Some(&Token::Punct(b',')) {
                            at += 1;
                        }
                    }
                    prefix.truncate(outer);
                    return at + 1;
                }
                // A glob binds no name of its own.
                _ => {
                    prefix.truncate(outer);
                    return at + 1;
                }
            }
        }
        if prefix.last().map(String::as_str) == Some("self") {
            prefix.pop();
        }
        let mut name = prefix.last().cloned().unwrap_or_default();
        if let (Some(Token::Ident("as", _)), Some(Token::Ident(rename, _))) =
            (self.tokens.get(at), self.tokens.get(at + 1))
        {
            name = (*rename).to_owned();
            at += 2;
        }
        if !name.is_empty() && name != "_" {
            bindings.push(Binding {
                visible: self.visible,
                global,
                path: prefix.clone(),
                name,
            });
        }
        prefix.truncate(outer);
        at
    }
}

/// The index of the `;` that ends the `use` declaration whose first token,
/// past its attributes, is at `index`: `pub use ..;`, `pub(crate) use ..;`
/// or `use ..;`. `None` for another item.
fn use_end(tokens: &[Token<'_>], index: usize) -> Option<usize> {
    let mut at = index;
    if matches!(tokens.get(at), Some(Token::Ident("pub", _))) {
        at += 1;
        if tokens.get(at) == Some(&Token::Punct(b'(')) {
            at += tokens[at..]
                .iter()
                .position(|token| *token == Token::Punct(b')'))?
                + 1;
        }
    }
    if !matches!(tokens.get(at), Some(Token::Ident("use", _))) {
        return None;
    }
    let mut depth = 0_usize;
    tokens[at..]
        .iter()
        .position(|token| match token {
            Token::Punct(b'{') => {
                depth += 1;
                false
            }
            Token::Punct(b'}') => {
                depth = depth.saturating_sub(1);
                false
            }
            Token::Punct(b';') => depth == 0,
            _ => false,
        })
        .map(|offset| at + offset)
}

/// `text`, a module of the crate, with `pub ` before each of its own items
/// named in `names` that has no visibility: a module on the way to a type
/// native code calls a marked generic at, or the type itself, which the
/// support table at the crate's root and the runner name by its path
/// (D869), as [`root_items_public`]'s are. rustc lets the native caller
/// name the type where it calls, though no path from the root may reach
/// it, as inboard's `workspace::spreadsheet` declares `mod sidebar;` and
/// uses its `Message`.
#[cfg_attr(
    not(all(feature = "rust-import", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(crate) fn named_items_public(text: &str, names: &std::collections::BTreeSet<String>) -> String {
    let tokens = tokens(text);
    let inserts = private_items(&tokens)
        .into_iter()
        .filter(|(_, index)| item_name(&tokens, *index).is_some_and(|name| names.contains(name)))
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    inserted(text, &inserts, "pub ")
}

/// `text`, a native module of a crate whose root is marked, with `pub `
/// before each of its own `mod` items that has no visibility. A marked
/// module reads a native value's public fields and methods whatever
/// module its type is declared in, as `custom_shader`'s `Scene` holds a
/// `Camera` of the private `scene::camera` (#1932); the runner names
/// the type by its path, so each module on the way is public in rawr's
/// copy, as D876 widens one for an instance's type. The user's source and
/// cargo's own build are untouched, so cargo check still reports privacy
/// errors.
#[cfg_attr(
    not(all(feature = "rust-import", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(crate) fn modules_public(text: &str) -> String {
    let tokens = tokens(text);
    let inserts = private_items(&tokens)
        .into_iter()
        .filter(|(_, index)| matches!(tokens[*index], Token::Ident("mod", _)))
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    inserted(text, &inserts, "pub ")
}

/// Each of the module's own items that has no visibility and may take one:
/// where a visibility goes, and the index of its first token past its
/// attributes.
fn private_items(tokens: &[Token<'_>]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut depth = 0_usize;
    let mut start = true;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if depth == 0 && start {
            if token == Token::Punct(b'#') {
                index = attribute_end(tokens, index);
                continue;
            }
            start = false;
            if let Some(at) = private_item(tokens, index) {
                found.push((at, index));
            }
        }
        match token {
            Token::Punct(b'{' | b'(' | b'[') => depth += 1,
            Token::Punct(close @ (b'}' | b')' | b']')) => {
                depth = depth.saturating_sub(1);
                start = depth == 0 && close == b'}';
            }
            Token::Punct(b';') if depth == 0 => start = true,
            _ => {}
        }
        index += 1;
    }
    found
}

/// The name the item whose first token is at `index` declares: the first
/// word past its keywords, and none for a `use`.
fn item_name<'t>(tokens: &[Token<'t>], index: usize) -> Option<&'t str> {
    tokens[index..].iter().find_map(|token| match *token {
        Token::Ident(
            "fn" | "const" | "static" | "mut" | "struct" | "enum" | "type" | "trait" | "union"
            | "mod" | "async" | "unsafe" | "extern",
            _,
        )
        | Token::Str(_) => None,
        Token::Ident("use", _) | Token::Punct(_) => Some(None),
        Token::Ident(name, _) => Some(Some(name)),
    })?
}

/// `text` with `visibility` at each of `inserts`, in order.
fn inserted(text: &str, inserts: &[usize], visibility: &str) -> String {
    let mut written = String::with_capacity(text.len() + visibility.len() * inserts.len());
    let mut cursor = 0;
    for &at in inserts {
        written.push_str(&text[cursor..at]);
        written.push_str(visibility);
        cursor = at;
    }
    written.push_str(&text[cursor..]);
    written
}

/// Where `pub ` goes for the item whose first token (past its attributes)
/// is at `index`, when it has no visibility and may take one.
fn private_item(tokens: &[Token<'_>], index: usize) -> Option<usize> {
    let Token::Ident(word, at) = tokens[index] else {
        return None;
    };
    // An ABI string (`extern "C" fn`) is not a word.
    let word_at = |offset: usize| match tokens[index..]
        .iter()
        .filter(|token| !matches!(token, Token::Str(_)))
        .nth(offset)
    {
        Some(Token::Ident(word, _)) => Some(*word),
        _ => None,
    };
    match word {
        "fn" | "const" | "static" | "struct" | "enum" | "type" | "trait" | "union" | "use"
        | "mod" | "async" => Some(at),
        // `unsafe fn`, `unsafe trait` and `unsafe extern "C" fn`; never
        // `unsafe impl` or an `unsafe extern` block.
        "unsafe" => match word_at(1) {
            Some("impl") => None,
            Some("extern") => (word_at(2) == Some("fn")).then_some(at),
            _ => Some(at),
        },
        "extern" => (word_at(1) == Some("fn")).then_some(at),
        _ => None,
    }
}

/// The index just past the attribute (`#[…]` or `#![…]`) at `index`.
fn attribute_end(tokens: &[Token<'_>], index: usize) -> usize {
    let mut cursor = index + 1;
    if tokens.get(cursor) == Some(&Token::Punct(b'!')) {
        cursor += 1;
    }
    if tokens.get(cursor) != Some(&Token::Punct(b'[')) {
        return cursor;
    }
    let mut depth = 0_usize;
    while let Some(token) = tokens.get(cursor) {
        cursor += 1;
        match token {
            Token::Punct(b'[') => depth += 1,
            Token::Punct(b']') => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    cursor
}

/// `text`, a crate root, with its own `fn main` renamed `name`: `None` when
/// it declares none.
#[cfg_attr(
    not(all(feature = "rust-import", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(crate) fn main_renamed(text: &str, name: &str) -> Option<String> {
    let tokens = tokens(text);
    let mut depth = 0_usize;
    let mut previous = None;
    for token in tokens {
        match token {
            Token::Punct(b'{') => depth += 1,
            Token::Punct(b'}') => depth = depth.saturating_sub(1),
            Token::Ident("main", start) if depth == 0 && previous == Some("fn") => {
                return Some(format!(
                    "{}{name}{}",
                    &text[..start],
                    &text[start + "main".len()..]
                ));
            }
            _ => {}
        }
        previous = match token {
            Token::Ident(word, _) => Some(word),
            Token::Punct(_) | Token::Str(_) => None,
        };
    }
    None
}

/// Where the path of a standard macro named at `start` begins: `start`
/// itself, or the start of a `std::`, `core::`, `::std::` or `::core::`
/// before it. `None` when another path names it.
fn std_path_start(text: &str, start: usize) -> Option<usize> {
    let is_ident = |c: char| c == '_' || c.is_ascii_alphanumeric();
    let Some(before) = text[..start].trim_end().strip_suffix("::") else {
        return Some(start);
    };
    let before = before.trim_end();
    let before = before
        .strip_suffix("std")
        .or_else(|| before.strip_suffix("core"))
        .filter(|before| !before.ends_with(is_ident))?;
    Some(match before.trim_end().strip_suffix("::") {
        Some(root) if !root.trim_end().ends_with(is_ident) => root.len(),
        _ => before.len(),
    })
}

/// Whether the token at `index` sits in a `#[cfg_attr(..)]`'s list of
/// attributes, where `path = ".."` is a `#[path]` as written alone.
fn in_cfg_attr(tokens: &[Token<'_>], index: usize) -> bool {
    let Some(hash) = tokens[..index]
        .iter()
        .rposition(|token| *token == Token::Punct(b'#'))
    else {
        return false;
    };
    let mut cursor = hash + 1;
    if tokens.get(cursor) == Some(&Token::Punct(b'!')) {
        cursor += 1;
    }
    tokens.get(cursor) == Some(&Token::Punct(b'['))
        && matches!(tokens.get(cursor + 1), Some(Token::Ident("cfg_attr", _)))
        && attribute_end(tokens, hash) > index
}

/// What the attribute whose `#` is at `index` (`#[..]` or `#![..]`) says
/// under `features`, and the index just past it.
fn attribute(tokens: &[Token<'_>], index: usize, features: &Enabled) -> (Attribute, usize) {
    let end = attribute_end(tokens, index);
    let mut cursor = index + 1;
    if tokens.get(cursor) == Some(&Token::Punct(b'!')) {
        cursor += 1;
    }
    if tokens.get(cursor) != Some(&Token::Punct(b'[')) {
        return (Attribute::default(), end);
    }
    cursor += 1;
    (meta(tokens, &mut cursor, features), end)
}

/// What the attribute's contents at `cursor` say under `features`, left
/// past them at the `,`, `)` or `]` after: `path = ..`, `cfg(..)`, and
/// the attributes of a `cfg_attr(..)` whose predicate holds.
fn meta(tokens: &[Token<'_>], cursor: &mut usize, features: &Enabled) -> Attribute {
    let mut said = Attribute::default();
    if let Some(Token::Ident(name, _)) = tokens.get(*cursor).copied() {
        *cursor += 1;
        let next = tokens.get(*cursor).copied();
        match (name, next) {
            ("path", Some(Token::Punct(b'='))) => said.path = true,
            ("cfg", Some(Token::Punct(b'('))) => {
                *cursor += 1;
                said.disabled = !cfg(tokens, cursor, features);
                close(tokens, cursor);
            }
            ("cfg_attr", Some(Token::Punct(b'('))) => {
                *cursor += 1;
                if cfg(tokens, cursor, features) {
                    while tokens.get(*cursor) == Some(&Token::Punct(b',')) {
                        *cursor += 1;
                        let applied = meta(tokens, cursor, features);
                        said.path |= applied.path;
                        said.disabled |= applied.disabled;
                    }
                }
                close(tokens, cursor);
            }
            _ => {}
        }
    }
    // The rest of this attribute: up to the `,`, `)` or `]` that ends it.
    let mut depth = 0_usize;
    while let Some(token) = tokens.get(*cursor) {
        match token {
            Token::Punct(b'(' | b'[' | b'{') => depth += 1,
            Token::Punct(b',' | b')' | b']') if depth == 0 => break,
            Token::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        *cursor += 1;
    }
    said
}

/// Past the rest of a parenthesized group whose `(` is behind `cursor`,
/// through its `)`.
fn close(tokens: &[Token<'_>], cursor: &mut usize) {
    let mut depth = 0_usize;
    while let Some(token) = tokens.get(*cursor) {
        *cursor += 1;
        match token {
            Token::Punct(b'(' | b'[' | b'{') => depth += 1,
            Token::Punct(b')') if depth == 0 => return,
            Token::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}

/// Whether the `cfg` predicate starting at `cursor` holds under
/// `features`, as the item parser reads one ([`Enabled::cfg`],
/// [`Operator`]); the app crate never builds as a test. Leaves `cursor`
/// just past it.
fn cfg(tokens: &[Token<'_>], cursor: &mut usize, features: &Enabled) -> bool {
    let Some(Token::Ident(name, _)) = tokens.get(*cursor).copied() else {
        return false;
    };
    *cursor += 1;
    match tokens.get(*cursor) {
        Some(Token::Punct(b'=')) => {
            *cursor += 1;
            let value = match tokens.get(*cursor) {
                Some(Token::Str(value)) => Some(*value),
                _ => None,
            };
            *cursor += 1;
            value.is_some_and(|value| features.cfg(name, Some(value), false))
        }
        Some(Token::Punct(b'(')) => {
            *cursor += 1;
            let mut values = Vec::new();
            while *cursor < tokens.len() && tokens[*cursor] != Token::Punct(b')') {
                let before = *cursor;
                values.push(cfg(tokens, cursor, features));
                if tokens.get(*cursor) == Some(&Token::Punct(b',')) || *cursor == before {
                    *cursor += 1;
                }
            }
            *cursor += 1;
            Operator::named(name).is_some_and(|operator| operator.holds(&values))
        }
        _ => features.cfg(name, None, false),
    }
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    // Rust starts after a shebang line and a frontmatter block.
    let mut index = crate::project::frontmatter::read(text).map_or(0, |lead| lead.source);
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
            b'"' => {
                let end = skip_string(bytes, index + 1).min(bytes.len());
                // Unterminated, it runs to the end of the text.
                let closed = end > index + 1 && bytes[end - 1] == b'"';
                let contents = index + 1..if closed { end - 1 } else { end };
                tokens.push(Token::Str(&text[contents]));
                index = end;
            }
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
    use crate::project::cargo::features::Enabled;

    /// A Linux target with the feature `x`, as rustc and Cargo give them.
    fn features() -> Enabled {
        Enabled::of_lines(["feature=\"x\"", "target_os=\"linux\"", "unix"])
    }

    fn paths(text: &str) -> Vec<(String, bool)> {
        declarations(text, &features())
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
const U: &[u8] = include_bytes!(concat!(env!( "CARGO_MANIFEST_DIR" ), "/../x"));
const V: Option<&str> = option_env!("CARGO_MANIFEST_DIR");
const W: &str = std::env!("CARGO_MANIFEST_DIR");
#[path = "other.rs"]
mod other;
#[cfg_attr(unix, path = "unix.rs")]
mod configured;
mod inline { pub(super) fn kept() {} #[path = "deep.rs"] mod deep; }
"#;
        let file = Path::new("/app/src/model.rs");
        let top = widened(text, true, file, Path::new("/app"));
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
        assert!(
            top.contains(r#"include_bytes!(concat!("/app", "/../x"))"#),
            "{top}"
        );
        assert!(
            top.contains(r#"= ::core::option::Option::Some("/app");"#),
            "{top}"
        );
        assert!(top.contains(r#"const W: &str = "/app";"#), "{top}");
        assert!(top.contains(r#"#[path = "/app/src/other.rs"]"#), "{top}");
        assert!(
            top.contains(r#"#[cfg_attr(unix, path = "/app/src/unix.rs")]"#),
            "{top}"
        );
        assert!(top.contains("pub(super) fn kept()"), "{top}");
        assert!(
            top.contains(r#"#[path = "/app/src/model/inline/deep.rs"]"#),
            "{top}"
        );
        // A module a `.rs` module declares: its `super` is Rust, not Rawr.
        assert!(widened(text, false, file, Path::new("/app")).contains("pub(super) fn up()"));
    }

    /// The modules and types an instance names become public, and nothing
    /// else does (D869).
    #[test]
    fn a_modules_named_private_items_become_public() {
        let text =
            "mod tiles;\nmod other;\npub(super) mod kept;\n#[derive(Debug)]\nenum Message { A }\n\
                    struct Hidden;\nuse tiles::strip;\nconst fn tiles() -> u8 { 1 }\n";
        let names = ["tiles", "Message", "kept", "strip"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let visible = super::named_items_public(text, &names);
        assert_eq!(
            visible,
            "pub mod tiles;\nmod other;\npub(super) mod kept;\n#[derive(Debug)]\n\
             pub enum Message { A }\nstruct Hidden;\nuse tiles::strip;\n\
             pub const fn tiles() -> u8 { 1 }\n"
        );
    }

    #[test]
    fn a_crate_roots_own_items_become_public() {
        let text = "#![allow(dead_code)]\n//! fn hidden\nmod page;\npub mod open;\npub(self) mod own;\n\
                    #[derive(Debug)]\nenum Message { Page(usize, page::Message), Noop }\n\
                    type Element<'a> = &'a [u8; 3];\nconst SCALE: [u8; 2] = [1, 2];\nstatic mut N: u8 = 0;\n\
                    use std::fmt;\nstruct Unit;\nunsafe trait Marker {}\nunsafe impl Marker for Unit {}\n\
                    impl Unit { fn inner() {} }\nunsafe extern \"C\" { fn c(); }\nextern crate core;\n\
                    extern \"C\" fn exported() {}\nmacro_rules! m { () => {}; }\nm!();\n\
                    async fn wait() {}\nconst fn konst() -> u8 { 1 }\nfn main() { let x = 1; }\n";
        let public = super::root_items_public(text);
        for line in [
            "pub mod page;",
            "pub mod open;",
            "pub(self) mod own;",
            "pub enum Message { Page(usize, page::Message), Noop }",
            "pub type Element<'a> = &'a [u8; 3];",
            "pub const SCALE: [u8; 2] = [1, 2];",
            "pub static mut N: u8 = 0;",
            "pub use std::fmt;",
            "pub struct Unit;",
            "pub unsafe trait Marker {}",
            "unsafe impl Marker for Unit {}",
            "impl Unit { fn inner() {} }",
            "unsafe extern \"C\" { fn c(); }",
            "extern crate core;",
            "pub extern \"C\" fn exported() {}",
            "macro_rules! m { () => {}; }",
            "m!();",
            "pub async fn wait() {}",
            "pub const fn konst() -> u8 { 1 }",
            "pub fn main() { let x = 1; }",
            "//! fn hidden",
            "#![allow(dead_code)]",
            "#[derive(Debug)]",
        ] {
            assert!(
                public.lines().any(|kept| kept == line),
                "{line} in\n{public}"
            );
        }
    }

    #[test]
    fn a_crate_roots_uses_are_kept_alone() {
        let text = "//! use hidden;\nmod theme;\n#[cfg(test)]\npub use theme::Theme;\n\
                    pub(crate) use theme::{palette::{self, Colors}, Kind as K};\n\
                    fn main() { use std::fmt; let s = \"use x;\"; }\nuse ui::Widget;\n";
        assert_eq!(
            super::uses_text(text),
            " pub use theme:: Theme;\n pub( crate) use theme::{ palette::{ self, Colors}, \
             Kind as K};\n use ui:: Widget;\n"
        );
    }

    #[test]
    fn a_modules_uses_bind_their_names() {
        let text = "use iced::wgpu;\n#[cfg(feature = \"off\")]\nuse gone::Gone;\n\
                    pub(crate) use ::serde::{self, de::{Visitor as V, *}, _x as _};\n\
                    fn f() { use inner::Hidden; }\nuse self::scene::Scene;\n";
        let bound = super::bindings(text, &features())
            .into_iter()
            .map(|binding| binding.declaration())
            .collect::<Vec<_>>();
        assert_eq!(
            bound,
            [
                "use iced::wgpu as wgpu;\n",
                "pub(crate) use ::serde as serde;\n",
                "pub(crate) use ::serde::de::Visitor as V;\n",
                "use self::scene::Scene as Scene;\n",
            ]
        );
    }

    #[test]
    fn a_native_modules_own_modules_are_public() {
        let text = "mod camera;\npub mod cube;\n#[cfg(test)]\nmod tests { mod inner {} }\n\
                    pub(super) mod near;\nfn f() { mod local {} }\n";
        assert_eq!(
            super::modules_public(text),
            "pub mod camera;\npub mod cube;\n#[cfg(test)]\npub mod tests { mod inner {} }\n\
             pub(super) mod near;\nfn f() { mod local {} }\n"
        );
    }

    #[test]
    fn a_crate_roots_own_main_is_renamed() {
        let text =
            "//! fn main\nmod inner { fn main() {} }\nfn main() -> Result<(), ()> { Ok(()) }\n";
        assert_eq!(
            super::main_renamed(text, "run").as_deref(),
            Some(
                "//! fn main\nmod inner { fn main() {} }\nfn run() -> Result<(), ()> { Ok(()) }\n"
            )
        );
        assert_eq!(super::main_renamed("fn other() {}", "run"), None);
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
#[cfg(feature = "y")]
mod unfeatured;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(not(target_os = "macos"))]
mod not_mac;
#[cfg(all(unix, target_os = "linux"))]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg_attr(windows, cfg(any()))]
mod unless_windows;
#[cfg_attr(unix, allow(dead_code), cfg(not(unix)))]
mod unless_unix;
#[cfg_attr(unix, path = "unix.rs")]
mod moved;
#[cfg_attr(windows, path = "windows.rs")]
mod unmoved;
mod plain;
"#;
        let disabled = declarations(text, &features())
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
                ("unfeatured".to_owned(), true),
                ("mac".to_owned(), true),
                ("not_mac".to_owned(), false),
                ("linux".to_owned(), false),
                ("windows".to_owned(), true),
                ("unless_windows".to_owned(), false),
                ("unless_unix".to_owned(), true),
                ("moved".to_owned(), false),
                ("unmoved".to_owned(), false),
                ("plain".to_owned(), false),
            ]
        );
        let moved = r#"#[cfg_attr(unix, path = "unix.rs")] mod moved;
#[cfg_attr(windows, path = "windows.rs")] mod unmoved;"#;
        assert_eq!(
            paths(moved),
            [("moved".to_owned(), true), ("unmoved".to_owned(), false)]
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
        let found = super::references(text, &features())
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
