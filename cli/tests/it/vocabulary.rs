//! No message a user can reach says "facade" (D662.4). The test reads the
//! source of every crate rawr ships and fails on a string literal that
//! names it outside test code: a `Display` string, a diagnostic code, a
//! panic or `expect` message, or anything else written as text.
//!
//! Byte strings are left out. They are hash domains and the generated
//! runner's templates, whose remaining module paths go with D662.5's rename.

use std::fs;
use std::path::{Path, PathBuf};

/// The source directories of the crates that make up rawr's binaries.
const SHIPPED: [&str; 10] = [
    "src",
    "importer/src",
    "import-cli/src",
    "cli/src",
    "watch/src",
    "macros/src",
    "codegen/src",
    "play/src",
    "play/snippet/src",
    "web/src",
];

/// Literals that are not text anyone reads, each with its reason.
const EXEMPT: [(&str, &str); 1] = [
    // The on-disk directory of the store's binding plans, renamed with
    // the rest of the internal vocabulary (D662.5).
    ("importer/src/node.rs", "facade"),
];

#[test]
fn no_user_reachable_text_says_facade() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for directory in SHIPPED {
        collect(&root.join(directory), &mut files);
    }
    let excluded = files
        .iter()
        .flat_map(|path| test_modules(path, &fs::read_to_string(path).unwrap()))
        .collect::<Vec<_>>();
    let mut found = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy();
        if name.ends_with("tests.rs")
            || name == "test_fixture.rs"
            || excluded
                .iter()
                .any(|excluded| path == excluded || path.starts_with(excluded))
        {
            continue;
        }
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let text = fs::read_to_string(path).unwrap();
        for (line, literal) in literals(&text) {
            let lower = literal.to_lowercase();
            if (lower.contains("facade") || lower.contains("façade"))
                && !EXEMPT.contains(&(relative.as_str(), literal.as_str()))
            {
                found.push(format!("{relative}:{line}: {literal:?}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "user-reachable text says \"facade\":\n{}",
        found.join("\n")
    );
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// The files a `#[cfg(test)] mod name;` in `path` makes test-only.
fn test_modules(path: &Path, text: &str) -> Vec<PathBuf> {
    let stem = path.file_stem().unwrap().to_string_lossy();
    let parent = path.parent().unwrap();
    let folder = if matches!(stem.as_ref(), "lib" | "main" | "mod") {
        parent.to_path_buf()
    } else {
        parent.join(stem.as_ref())
    };
    let mut modules = Vec::new();
    let mut test = false;
    for line in text.lines().map(str::trim) {
        if is_test_attribute(line) {
            test = true;
            continue;
        }
        if test && line.starts_with("#[") {
            continue;
        }
        if test
            && let Some(name) = line
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ")
                .strip_prefix("mod ")
                .and_then(|rest| rest.strip_suffix(';'))
        {
            modules.push(folder.join(format!("{name}.rs")));
            modules.push(folder.join(name));
        }
        test = false;
    }
    modules
}

fn is_test_attribute(text: &str) -> bool {
    text.starts_with("#[test]")
        || text.starts_with("#[cfg(test)]")
        || text.starts_with("#[cfg(all(test")
        || text.starts_with("#[cfg(any(test")
        || text.starts_with("#[cfg(feature = \"test-fixtures\")]")
}

/// Each text string literal outside comments and test-only items, with the
/// line it starts on.
fn literals(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let (mut index, mut line, mut depth) = (0, 1, 0_usize);
    let mut skip_next = false;
    let mut skipping: Option<usize> = None;
    let quiet = |skip_next: bool, skipping: Option<usize>| skip_next || skipping.is_some();
    while index < bytes.len() {
        let rest = &text[index..];
        let previous = index.checked_sub(1).map(|at| bytes[at]);
        let word_before = previous.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        match bytes[index] {
            b'\n' => {
                line += 1;
                index += 1;
            }
            b'/' if rest.starts_with("//") => {
                index += rest.find('\n').unwrap_or(rest.len());
            }
            b'/' if rest.starts_with("/*") => {
                let (mut level, mut at) = (1, 2);
                while level > 0 && at < rest.len() {
                    if rest[at..].starts_with("/*") {
                        level += 1;
                        at += 2;
                    } else if rest[at..].starts_with("*/") {
                        level -= 1;
                        at += 2;
                    } else {
                        line += usize::from(rest.as_bytes()[at] == b'\n');
                        at += 1;
                    }
                }
                index += at;
            }
            b'#' if rest.starts_with("#[") => {
                if skipping.is_none() && is_test_attribute(rest) {
                    skip_next = true;
                }
                index += 2;
            }
            b'b' | b'r' if !word_before && raw_or_byte(rest).is_some() => {
                let (open, hashes, byte) = raw_or_byte(rest).unwrap();
                let close = format!("\"{}", "#".repeat(hashes));
                let body_start = index + open;
                let end = if hashes == 0 && !rest.starts_with("br") && !rest.starts_with('r') {
                    string_end(text, body_start)
                } else {
                    body_start + text[body_start..].find(&close).unwrap()
                };
                let literal = &text[body_start..end];
                if !byte && !quiet(skip_next, skipping) {
                    found.push((line, literal.to_owned()));
                }
                line += literal.matches('\n').count();
                index = end + close.len();
            }
            b'"' => {
                let end = string_end(text, index + 1);
                let literal = &text[index + 1..end];
                if !quiet(skip_next, skipping) {
                    found.push((line, literal.to_owned()));
                }
                line += literal.matches('\n').count();
                index = end + 1;
            }
            b'\'' => {
                // A char literal, or a lifetime.
                if rest[1..].starts_with('\\') {
                    index += 3 + rest[3..].find('\'').unwrap() + 1;
                } else {
                    let character = rest[1..].chars().next().map_or(0, char::len_utf8);
                    index += if rest[1 + character..].starts_with('\'') {
                        character + 2
                    } else {
                        1
                    };
                }
            }
            b'{' => {
                depth += 1;
                if skip_next && skipping.is_none() {
                    skipping = Some(depth);
                    skip_next = false;
                }
                index += 1;
            }
            b'}' => {
                if skipping == Some(depth) {
                    skipping = None;
                }
                depth = depth.saturating_sub(1);
                index += 1;
            }
            b';' if skip_next && skipping.is_none() => {
                skip_next = false;
                index += 1;
            }
            _ => index += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    found
}

/// A raw or byte string's opening: its length, its `#`s, and whether it is
/// a byte string.
fn raw_or_byte(rest: &str) -> Option<(usize, usize, bool)> {
    let (byte, after) = match rest.strip_prefix('b') {
        Some(after) => (true, after),
        None => (false, rest),
    };
    match after.strip_prefix('r') {
        Some(raw) => {
            let hashes = raw.len() - raw.trim_start_matches('#').len();
            raw[hashes..].starts_with('"').then_some((
                usize::from(byte) + 1 + hashes + 1,
                hashes,
                byte,
            ))
        }
        None if byte && after.starts_with('"') => Some((2, 0, true)),
        None => None,
    }
}

/// Where a string literal whose body starts at `start` ends: its closing
/// quote, past escapes.
fn string_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut at = start;
    while bytes[at] != b'"' {
        at += if bytes[at] == b'\\' { 2 } else { 1 };
    }
    at
}
