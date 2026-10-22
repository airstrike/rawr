// Copyright the ripgrep contributors. MIT OR Unlicense.
//! globset 0.4.18 glob.rs Parser, specialized to Cargo's ignore-builder options.
//! Flat branches prevent recursive destruction of nested alternatives.
use crate::input::membership::manifest::{
    load::formatted, session::Session, storage::Buffer, Error, Step, Stop,
};
use rawr_toml::document::Admission;
pub(super) mod aho;
mod collection;
pub(super) mod compiled;
mod literal;
mod matching;
mod regex;
mod strategy;
mod table;

/// Successful pinned GlobBuilder transformation; this is not a compiled matcher.
pub(super) struct Built {
    glob: String,
    regex: regex::Regex,
    parsed: Parsed,
}
impl Built {
    pub(super) fn glob(&self) -> &str {
        &self.glob
    }
    pub(super) fn regex(&self) -> &[u8] {
        self.regex.bytes()
    }
}
pub(super) fn build<A: Admission>(
    glob: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<Built, Failure>> {
    let parsed = match parse(glob, session)? {
        Ok(parsed) => parsed,
        Err(failure) => return Ok(Err(failure)),
    };
    // GlobBuilder owns the source spelling before emitting its regex. Retain
    // that order so a refusal cannot expose later construction work.
    let glob = formatted(format_args!("{glob}"), session)?;
    let regex = parsed.regex(session)?;
    Ok(Ok(Built {
        glob,
        regex,
        parsed,
    }))
}

#[cfg(test)]
mod build_tests;

pub(super) enum Token {
    Literal(char),
    Any,
    ZeroOrMore,
    RecursivePrefix,
    RecursiveSuffix,
    RecursiveZeroOrMore,
    Class {
        negated: bool,
        ranges: Buffer<(char, char)>,
    },
    Alternates(Buffer<usize>),
}
pub(super) struct Parsed {
    pub(super) branches: Buffer<Buffer<Token>>,
    pub(super) root: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Kind {
    UnopenedAlternates,
    UnclosedAlternates,
    DanglingEscape,
    InvalidRange(char, char),
}
impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnopenedAlternates => {
                f.write_str("unopened alternate group; missing '{' (maybe escape '}' with '[}]'?)")
            }
            Self::UnclosedAlternates => {
                f.write_str("unclosed alternate group; missing '}' (maybe escape '{' with '[{]'?)")
            }
            Self::DanglingEscape => f.write_str("dangling '\\'"),
            Self::InvalidRange(start, end) => write!(f, "invalid range; '{start}' > '{end}'"),
        }
    }
}
pub(super) struct Failure {
    pub(super) glob: String,
    pub(super) kind: Kind,
}
enum Abort {
    Stop(Stop),
    Semantic(Kind),
    State,
}
impl From<Stop> for Abort {
    fn from(stop: Stop) -> Self {
        Self::Stop(stop)
    }
}
type Trial<T> = Result<T, Abort>;
struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    prev: Option<char>,
    cur: Option<char>,
    unclosed_class: bool,
    branches: Buffer<Buffer<Token>>,
    active: Buffer<usize>,
    alternates: Buffer<usize>,
}
pub(super) fn parse<A: Admission>(
    glob: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<Parsed, Failure>> {
    session.check()?;
    let mut parser = Parser {
        chars: glob.chars().peekable(),
        prev: None,
        cur: None,
        unclosed_class: false,
        branches: Buffer::new(),
        active: Buffer::new(),
        alternates: Buffer::new(),
    };
    let result = parser.branch(session).and_then(|_| parser.run(session));
    match result {
        Ok(()) => {
            let Some(root) = parser.active.pop() else {
                return session.fail(Error::State);
            };
            Ok(Ok(Parsed {
                branches: parser.branches,
                root,
            }))
        }
        Err(Abort::Semantic(kind)) => {
            let glob = formatted(format_args!("{glob}"), session)?;
            Ok(Err(Failure { glob, kind }))
        }
        Err(Abort::Stop(stop)) => Err(stop),
        Err(Abort::State) => session.fail(Error::State),
    }
}
impl Parser<'_> {
    fn branch<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<()> {
        let index = self.branches.len();
        self.branches.push(Buffer::new(), session)?;
        self.active.push(index, session)?;
        Ok(())
    }
    fn tokens(&mut self) -> Trial<&mut Buffer<Token>> {
        let index = *self
            .active
            .as_slice()
            .last()
            .ok_or(Abort::Semantic(Kind::UnopenedAlternates))?;
        self.branches
            .as_mut_slice()
            .get_mut(index)
            .ok_or(Abort::State)
    }
    fn push<A: Admission>(&mut self, token: Token, session: &mut Session<'_, A>) -> Trial<()> {
        self.tokens()?.push(token, session)?;
        Ok(())
    }
    fn bump<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<Option<char>> {
        session.items(1)?;
        self.prev = self.cur;
        self.cur = self.chars.next();
        Ok(self.cur)
    }
    fn peek<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<Option<char>> {
        session.items(1)?;
        Ok(self.chars.peek().copied())
    }
    fn run<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<()> {
        while let Some(c) = self.bump(session)? {
            match c {
                '?' => self.push(Token::Any, session)?,
                '*' => self.star(session)?,
                '[' if !self.unclosed_class => self.class(session)?,
                '{' => {
                    self.alternates.push(self.active.len(), session)?;
                    self.branch(session)?;
                }
                '}' => {
                    let start = self
                        .alternates
                        .pop()
                        .ok_or(Abort::Semantic(Kind::UnopenedAlternates))?;
                    if start > self.active.len() {
                        return Err(Abort::State);
                    }
                    let mut branches = Buffer::new();
                    for &index in &self.active.as_slice()[start..] {
                        branches.push(index, session)?;
                    }
                    self.active.truncate(start);
                    self.push(Token::Alternates(branches), session)?;
                }
                ',' if !self.alternates.as_slice().is_empty() => self.branch(session)?,
                '\\' => {
                    let c = self
                        .bump(session)?
                        .ok_or(Abort::Semantic(Kind::DanglingEscape))?;
                    self.push(Token::Literal(c), session)?;
                }
                c => self.push(Token::Literal(c), session)?,
            }
        }
        if self.active.len() > 1 {
            return Err(Abort::Semantic(Kind::UnclosedAlternates));
        }
        Ok(())
    }
    fn star<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<()> {
        let prev = self.prev;
        if self.peek(session)? != Some('*') {
            return self.push(Token::ZeroOrMore, session);
        }
        if self.bump(session)? != Some('*') {
            return Err(Abort::State);
        }
        if self.tokens()?.as_slice().is_empty() {
            if !self.peek(session)?.is_none_or(|c| c == '/') {
                self.push(Token::ZeroOrMore, session)?;
                self.push(Token::ZeroOrMore, session)?;
            } else {
                self.push(Token::RecursivePrefix, session)?;
                if !self.bump(session)?.is_none_or(|c| c == '/') {
                    return Err(Abort::State);
                }
            }
            return Ok(());
        }
        if prev != Some('/') && (self.active.len() <= 1 || (prev != Some(',') && prev != Some('{')))
        {
            self.push(Token::ZeroOrMore, session)?;
            return self.push(Token::ZeroOrMore, session);
        }
        let suffix = match self.peek(session)? {
            None => {
                if self.bump(session)?.is_some() {
                    return Err(Abort::State);
                }
                true
            }
            Some(',') | Some('}') if self.active.len() >= 2 => true,
            Some('/') => {
                if self.bump(session)? != Some('/') {
                    return Err(Abort::State);
                }
                false
            }
            _ => {
                self.push(Token::ZeroOrMore, session)?;
                return self.push(Token::ZeroOrMore, session);
            }
        };
        let token = match self.tokens()?.pop().ok_or(Abort::State)? {
            Token::RecursivePrefix => Token::RecursivePrefix,
            Token::RecursiveSuffix => Token::RecursiveSuffix,
            _ if suffix => Token::RecursiveSuffix,
            _ => Token::RecursiveZeroOrMore,
        };
        self.push(token, session)
    }
    fn class<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Trial<()> {
        let saved_chars = self.chars.clone();
        let saved_prev = self.prev;
        let saved_cur = self.cur;
        let mut ranges = Buffer::new();
        let negated = match self.peek(session)? {
            Some('!') | Some('^') => {
                self.bump(session)?;
                true
            }
            _ => false,
        };
        let mut first = true;
        let mut in_range = false;
        loop {
            let Some(c) = self.bump(session)? else {
                self.chars = saved_chars;
                self.prev = saved_prev;
                self.cur = saved_cur;
                self.unclosed_class = true;
                return self.push(Token::Literal('['), session);
            };
            match c {
                ']' if !first => break,
                '-' if !first && !in_range => {
                    if ranges.as_slice().is_empty() {
                        return Err(Abort::State);
                    }
                    in_range = true;
                }
                c if in_range => {
                    let range: &mut (char, char) =
                        ranges.as_mut_slice().last_mut().ok_or(Abort::State)?;
                    range.1 = c;
                    if range.1 < range.0 {
                        return Err(Abort::Semantic(Kind::InvalidRange(range.0, range.1)));
                    }
                    in_range = false;
                }
                c => {
                    ranges.push((c, c), session)?;
                }
            }
            first = false;
        }
        if in_range {
            ranges.push(('-', '-'), session)?;
        }
        self.push(Token::Class { negated, ranges }, session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference::Account;
    fn parsed(input: &str) -> Parsed {
        match parse(input, &mut Session::new(&mut Account)).unwrap() {
            Ok(parsed) => parsed,
            Err(error) => panic!("unexpected error {:?}", error.kind),
        }
    }
    fn codes(parsed: &Parsed, branch: usize) -> Vec<String> {
        parsed.branches.as_slice()[branch]
            .as_slice()
            .iter()
            .map(|token| match token {
                Token::Literal(c) => format!("L{c}"),
                Token::Any => "?".into(),
                Token::ZeroOrMore => "*".into(),
                Token::RecursivePrefix => "prefix".into(),
                Token::RecursiveSuffix => "suffix".into(),
                Token::RecursiveZeroOrMore => "recursive".into(),
                Token::Alternates(branches) => format!("alternates:{}", branches.len()),
                Token::Class { negated, ranges } => {
                    format!("class:{negated}:{:?}", ranges.as_slice())
                }
            })
            .collect()
    }
    #[test]
    fn pinned_wildcards_classes_and_escapes() {
        for (input, expected) in [
            ("**/a", vec!["prefix", "La"]),
            ("a/**", vec!["La", "suffix"]),
            ("a/**/b", vec!["La", "recursive", "Lb"]),
            ("a**b", vec!["La", "*", "*", "Lb"]),
            ("**/**/x", vec!["prefix", "Lx"]),
            ("\\*?", vec!["L*", "?"]),
            ("[[[", vec!["L[", "L[", "L["]),
            ("[!a-c-]", vec!["class:true:[('a', 'c'), ('-', '-')]"]),
            ("[]]", vec!["class:false:[(']', ']')]"]),
        ] {
            let parsed = parsed(input);
            assert_eq!(codes(&parsed, parsed.root), expected, "{input}");
        }
        for (input, kind) in [
            ("}", Kind::UnopenedAlternates),
            ("{", Kind::UnclosedAlternates),
            ("a\\", Kind::DanglingEscape),
            ("[z-a]", Kind::InvalidRange('z', 'a')),
            ("[z-a", Kind::InvalidRange('z', 'a')),
        ] {
            let Err(failure) = parse(input, &mut Session::new(&mut Account)).unwrap() else {
                panic!("expected rejection")
            };
            assert_eq!(failure.kind, kind);
            assert_eq!(failure.glob, input);
        }
    }
    #[test]
    fn nested_alternates_are_flat_and_keep_branch_order() {
        let parsed = parsed("a{b,{c,d},}e");
        assert_eq!(codes(&parsed, parsed.root), ["La", "alternates:3", "Le"]);
        let Token::Alternates(branches) = &parsed.branches.as_slice()[parsed.root].as_slice()[1]
        else {
            panic!("alternatives")
        };
        assert_eq!(codes(&parsed, branches.as_slice()[0]), ["Lb"]);
        assert_eq!(codes(&parsed, branches.as_slice()[1]), ["alternates:2"]);
        assert!(codes(&parsed, branches.as_slice()[2]).is_empty());
        let deep = format!("{}x{}", "{".repeat(1000), "}".repeat(1000));
        let deep = self::parsed(&deep);
        assert_eq!(deep.branches.len(), 1001);
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let call = self.calls;
            self.calls += 1;
            if call == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn parser_refusal_is_terminal_at_every_admission() {
        for input in ["{a/**/[!b-d],x\\?}", "[z-a]"] {
            let mut account = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            let _ = parse(input, &mut Session::new(&mut account)).unwrap();
            for refuse in 0..account.calls {
                let mut account = Count { calls: 0, refuse };
                let mut session = Session::new(&mut account);
                assert!(matches!(parse(input, &mut session), Err(Stop::Refused)));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(account.calls, refuse + 1);
            }
        }
    }
    #[test]
    fn prepared_rule_reaches_glob_parser() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let rule = super::super::rule::prepare("!cache/**", &mut session)
            .unwrap()
            .unwrap();
        let Ok(parsed) = rule.parse(&mut session).unwrap() else {
            panic!("prepared glob")
        };
        assert_eq!(
            codes(&parsed, parsed.root),
            ["Lc", "La", "Lc", "Lh", "Le", "recursive", "*"]
        );
    }
}
