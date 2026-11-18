//! A list of gitignore rules, read as Cargo reads a package's `include` and
//! `exclude`: each rule is a glob added as a gitignore line, the last rule
//! that matches a path decides, and a path no rule matches is decided by
//! its nearest matching parent directory.
//!
//! A rule without a slash matches a name at any depth; a leading slash
//! anchors it to the package root; a trailing slash matches directories
//! only; `!` makes a match keep the path. In the glob, `*`, `?` and a class
//! never cross a `/` unless the class names it, `**/`, `/**/` and `/**`
//! cross any number of directories, `{a,b}` is a choice and `\` escapes.

use std::path::Path;

/// The rules of one `include` or `exclude` list.
pub(super) struct Set {
    rules: Vec<Rule>,
}

/// A rule that is not a glob, as the manifest spells it.
#[derive(Debug, thiserror::Error)]
#[error("invalid glob `{rule}`: {reason}")]
pub(super) struct Invalid {
    rule: Box<str>,
    reason: &'static str,
}

struct Rule {
    tokens: Vec<Token>,
    keeps: bool,
    directory_only: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Literal(char),
    /// `?`: one character other than `/`.
    Any,
    /// `*`: any characters other than `/`.
    Star,
    /// A leading `**/`: nothing, or any path ending in `/`.
    LeadingDirectories,
    /// A trailing `/**`: `/` and anything after it.
    TrailingDirectories,
    /// An inner `/**/`: `/`, or `/` and any path ending in `/`.
    InnerDirectories,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
    Choice(Vec<Vec<Token>>),
}

impl Set {
    /// The rules of `lines`, each prepared as a gitignore line.
    pub(super) fn new<'a>(lines: impl IntoIterator<Item = &'a str>) -> Result<Self, Invalid> {
        let mut rules = Vec::new();
        for line in lines {
            if let Some(rule) = Rule::new(line)? {
                rules.push(rule);
            }
        }
        Ok(Self { rules })
    }

    /// Whether the rules leave out `relative`: the last rule matching it
    /// decides, else the one matching its nearest parent directory.
    pub(super) fn ignores(&self, relative: &Path, directory: bool) -> bool {
        let mut path = relative;
        let mut directory = directory;
        loop {
            if let Some(rule) = self.matched(path, directory) {
                return !rule.keeps;
            }
            match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => {
                    path = parent;
                    directory = true;
                }
                _ => return false,
            }
        }
    }

    fn matched(&self, path: &Path, directory: bool) -> Option<&Rule> {
        let text = path.to_string_lossy();
        let text: Vec<char> = text.chars().collect();
        self.rules
            .iter()
            .rev()
            .find(|rule| (directory || !rule.directory_only) && rule.matches(&text))
    }
}

impl Rule {
    /// A gitignore line as a rule; a blank line or a comment is none.
    fn new(line: &str) -> Result<Option<Self>, Invalid> {
        if line.starts_with('#') {
            return Ok(None);
        }
        let mut line = line;
        if !line.ends_with("\\ ") {
            line = line.trim_end();
        }
        if line.is_empty() {
            return Ok(None);
        }
        let original = line;
        let mut keeps = false;
        let mut anchored = false;
        if line.starts_with("\\!") || line.starts_with("\\#") {
            line = &line[1..];
            anchored = line.starts_with('/');
        } else {
            if let Some(rest) = line.strip_prefix('!') {
                keeps = true;
                line = rest;
            }
            if let Some(rest) = line.strip_prefix('/') {
                line = rest;
                anchored = true;
            }
        }
        let mut directory_only = false;
        if let Some(rest) = line.strip_suffix('/') {
            directory_only = true;
            line = rest.strip_suffix('\\').unwrap_or(rest);
        }
        let mut glob = line.to_owned();
        if !anchored && !line.contains('/') && !(glob.starts_with("**/") || glob == "**") {
            glob.insert_str(0, "**/");
        }
        if glob.ends_with("/**") {
            glob.push_str("/*");
        }
        let tokens = parse(&glob).map_err(|reason| Invalid {
            rule: original.into(),
            reason,
        })?;
        Ok(Some(Self {
            tokens,
            keeps,
            directory_only,
        }))
    }

    fn matches(&self, text: &[char]) -> bool {
        if self.tokens == [Token::LeadingDirectories] {
            return true;
        }
        matches(&self.tokens, text, &[])
    }
}

/// The tokens of `glob`, parsed as a glob whose `*` stops at `/`.
fn parse(glob: &str) -> Result<Vec<Token>, &'static str> {
    // The innermost choice's alternatives, the current one last.
    let mut stack: Vec<Vec<Vec<Token>>> = vec![vec![Vec::new()]];
    let mut chars = glob.chars().peekable();
    let mut previous: Option<char> = None;
    while let Some(c) = chars.next() {
        let inside = stack.len() > 1;
        let tokens = current(&mut stack);
        match c {
            '?' => tokens.push(Token::Any),
            '*' => star(tokens, &mut chars, previous, inside),
            '[' => tokens.push(class(&mut chars)?),
            '{' => stack.push(vec![Vec::new()]),
            '}' if inside => {
                let alternatives = stack.pop().ok_or("unopened alternates")?;
                current(&mut stack).push(Token::Choice(alternatives));
            }
            '}' => return Err("unopened alternates"),
            ',' if inside => {
                if let Some(choice) = stack.last_mut() {
                    choice.push(Vec::new());
                }
            }
            '\\' => match chars.next() {
                Some(escaped) => tokens.push(Token::Literal(escaped)),
                None => return Err("dangling escape"),
            },
            c => tokens.push(Token::Literal(c)),
        }
        previous = Some(c);
    }
    if stack.len() > 1 {
        return Err("unclosed alternates");
    }
    Ok(stack
        .pop()
        .and_then(|mut alternatives| alternatives.pop())
        .unwrap_or_default())
}

fn current(stack: &mut [Vec<Vec<Token>>]) -> &mut Vec<Token> {
    let alternatives = stack
        .last_mut()
        .expect("the outermost glob stays on the stack");
    alternatives
        .last_mut()
        .expect("each choice starts with one alternative")
}

/// A `*` and, when another follows it, the `**` it starts: one that
/// stands between slashes or at an end crosses directories, any other is
/// two stars.
fn star(
    tokens: &mut Vec<Token>,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    previous: Option<char>,
    inside: bool,
) {
    if chars.peek() != Some(&'*') {
        tokens.push(Token::Star);
        return;
    }
    chars.next();
    if tokens.is_empty() {
        if chars.peek().is_none_or(|&next| next == '/') {
            chars.next();
            tokens.push(Token::LeadingDirectories);
        } else {
            tokens.extend([Token::Star, Token::Star]);
        }
        return;
    }
    if previous != Some('/') && (!inside || (previous != Some(',') && previous != Some('{'))) {
        tokens.extend([Token::Star, Token::Star]);
        return;
    }
    let trailing = match chars.peek() {
        None => true,
        Some(',' | '}') if inside => true,
        Some('/') => {
            chars.next();
            false
        }
        Some(_) => {
            tokens.extend([Token::Star, Token::Star]);
            return;
        }
    };
    match tokens.pop() {
        Some(Token::LeadingDirectories) => tokens.push(Token::LeadingDirectories),
        Some(Token::TrailingDirectories) => tokens.push(Token::TrailingDirectories),
        _ if trailing => tokens.push(Token::TrailingDirectories),
        _ => tokens.push(Token::InnerDirectories),
    }
}

/// A class after its `[`: `!` or `^` first negates it, a `]` first or a
/// `-` at either end is literal, and `a-z` is a range.
fn class(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<Token, &'static str> {
    let negated = matches!(chars.peek(), Some('!' | '^'));
    if negated {
        chars.next();
    }
    let mut ranges: Vec<(char, char)> = Vec::new();
    let mut first = true;
    let mut in_range = false;
    loop {
        let c = chars.next().ok_or("unclosed character class")?;
        match c {
            ']' if !first => break,
            '-' if !first && !in_range => in_range = true,
            c => {
                if in_range {
                    let last = ranges.last_mut().ok_or("invalid range")?;
                    if last.0 > c {
                        return Err("invalid range");
                    }
                    last.1 = c;
                    in_range = false;
                } else {
                    ranges.push((c, c));
                }
            }
        }
        first = false;
    }
    if in_range {
        ranges.push(('-', '-'));
    }
    Ok(Token::Class { negated, ranges })
}

/// Whether `tokens` and then `rest` match all of `text`.
fn matches(tokens: &[Token], text: &[char], rest: &[&[Token]]) -> bool {
    let Some((token, tokens)) = tokens.split_first() else {
        return match rest.split_first() {
            Some((next, rest)) => matches(next, text, rest),
            None => text.is_empty(),
        };
    };
    let then = |at: usize| matches(tokens, &text[at..], rest);
    match token {
        Token::Literal(c) => text.first() == Some(c) && then(1),
        Token::Any => text.first().is_some_and(|&c| c != '/') && then(1),
        Token::Star => {
            let run = text.iter().take_while(|&&c| c != '/').count();
            (0..=run).any(then)
        }
        Token::LeadingDirectories => {
            then(0)
                || (text.first() == Some(&'/') && then(1))
                || (1..=text.len()).any(|at| text[at - 1] == '/' && then(at))
        }
        Token::TrailingDirectories => text.first() == Some(&'/') && (1..=text.len()).any(then),
        Token::InnerDirectories => {
            text.first() == Some(&'/')
                && (then(1) || (2..=text.len()).any(|at| text[at - 1] == '/' && then(at)))
        }
        Token::Class { negated, ranges } => text.first().is_some_and(|&c| {
            ranges.iter().any(|&(low, high)| low <= c && c <= high) != *negated && then(1)
        }),
        Token::Choice(alternatives) => alternatives.iter().any(|alternative| {
            let mut after = Vec::with_capacity(rest.len() + 1);
            after.push(tokens);
            after.extend_from_slice(rest);
            matches(alternative, text, &after)
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Set;

    fn ignores(rules: &[&str], path: &str, directory: bool) -> bool {
        Set::new(rules.iter().copied())
            .expect("the rules are globs")
            .ignores(Path::new(path), directory)
    }

    #[test]
    fn a_rule_without_a_slash_matches_a_name_at_any_depth() {
        assert!(ignores(&["*.log"], "build.log", false));
        assert!(ignores(&["*.log"], "a/b/build.log", false));
        assert!(!ignores(&["*.log"], "build.log.txt", false));
        assert!(ignores(&[".*"], ".git", true));
        assert!(ignores(&[".*"], "src/.hidden", false));
        assert!(!ignores(&[".*"], "src/lib.rs", false));
    }

    #[test]
    fn a_rule_with_a_slash_is_anchored_and_a_star_stays_in_one_directory() {
        assert!(ignores(&["/src/*.rs"], "src/lib.rs", false));
        assert!(!ignores(&["/src/*.rs"], "src/a/lib.rs", false));
        assert!(!ignores(&["src/*.rs"], "a/src/lib.rs", false));
        assert!(ignores(&["src/**/*.rs"], "src/a/b/lib.rs", false));
        assert!(ignores(&["src/**/*.rs"], "src/lib.rs", false));
        assert!(ignores(&["src/**"], "src/a/b", false));
        assert!(ignores(&["**/tests"], "a/tests", true));
        assert!(ignores(&["**"], "anything/at/all", false));
    }

    #[test]
    fn a_directory_rule_leaves_out_what_is_inside_it_and_the_last_rule_decides() {
        assert!(ignores(&["logs/"], "logs/today.txt", false));
        assert!(!ignores(&["logs/"], "logs", false));
        assert!(ignores(&["/benches"], "benches/a/b.rs", false));
        assert!(!ignores(&["*.txt", "!keep.txt"], "keep.txt", false));
        assert!(ignores(&["!keep.txt", "*.txt"], "keep.txt", false));
        assert!(ignores(&["src/**", "!src/lib.rs"], "src/main.rs", false));
        assert!(!ignores(&["src/**", "!src/lib.rs"], "src/lib.rs", false));
    }

    #[test]
    fn classes_choices_and_escapes_read_as_globs() {
        assert!(ignores(&["data[0-9].bin"], "data7.bin", false));
        assert!(!ignores(&["data[!0-9].bin"], "data7.bin", false));
        assert!(ignores(&["*.{png,jpg}"], "img/a.jpg", false));
        assert!(!ignores(&["*.{png,jpg}"], "img/a.gif", false));
        assert!(ignores(&["\\#name"], "#name", false));
        assert!(ignores(&["\\!name"], "!name", false));
        assert!(Set::new(["[a-"]).is_err());
        assert!(Set::new(["{a,b"]).is_err());
    }
}
