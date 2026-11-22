//! `macro_rules!` macros, the program's own and its dependencies' (D594),
//! expanded as rustc expands them. The rules are tried in order against the
//! invocation's token trees, with Rust's fragment specifiers and
//! repetitions, and the first that matches is transcribed. `$crate` reads
//! as the path to the defining crate's root: `crate` in the program, and
//! the path through the dependency a crate's macro came from. The local
//! names the macro's own tokens bind are its own (hygiene): a caller's name
//! passed in never meets one of them.

use std::collections::BTreeSet;
use std::fmt;

use crate::tokenizer::Token;
use crate::{ImmutableString, Position};

/// A definition rustc refuses, with its message and label at a token.
#[derive(Clone, Debug)]
pub(crate) struct Malformed {
    message: ImmutableString,
    label: &'static str,
    help: Option<&'static str>,
    at: Position,
}

/// Why a call does not expand, as rustc reports it.
#[derive(Clone, Debug)]
pub(crate) enum Failure {
    /// No rule matches: the token the rule that got furthest stopped at, or
    /// `None` when it ran out of the call's tokens.
    NoRules(Option<(Token, Position)>),
    /// A transcription rustc refuses, in its words.
    Transcription(ImmutableString),
}

/// One token tree: a token, or a delimited group of trees.
#[derive(Clone, Debug)]
pub(crate) enum Tree {
    Leaf(Token, Position),
    Group(Delimiter, Position, Vec<Tree>, Position),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Delimiter {
    Paren,
    Bracket,
    Brace,
}

/// A macro's rules, parsed once from its source.
#[derive(Debug)]
pub(crate) struct Rules {
    name: ImmutableString,
    rules: Vec<Rule>,
    /// Whether a transcriber holds a token that starts an item, or a path
    /// through a module or a type.
    declares: bool,
}

#[derive(Debug)]
struct Rule {
    matcher: Vec<Pattern>,
    transcriber: Vec<Tree>,
    /// Each metavariable's repetition depth in the matcher.
    depths: Vec<(ImmutableString, usize)>,
}

/// One part of a rule's matcher, at the position the definition writes it.
#[derive(Debug)]
enum Pattern {
    Token(Token, Position),
    Group(Delimiter, Vec<Pattern>, Position),
    Fragment(ImmutableString, Fragment, Position),
    /// A repetition, with its separator's position when it has one.
    Repeat(Vec<Pattern>, Option<Token>, Op, Position),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Op {
    /// `*`: any number.
    Star,
    /// `+`: one or more.
    Plus,
    /// `?`: at most one.
    Question,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Fragment {
    Expr,
    Ty,
    Path,
    Ident,
    Pat,
    PatParam,
    Stmt,
    Block,
    Item,
    Literal,
    Lifetime,
    Meta,
    Tt,
    Vis,
}

/// One metavariable's match: its trees at its repetition indices.
#[derive(Clone, Debug)]
struct Bound {
    name: ImmutableString,
    indices: Vec<usize>,
    fragment: Fragment,
    trees: Vec<Tree>,
}

/// One repetition of a rule's matcher as a call matched it: the
/// metavariables inside it, the indices of the repetitions around it and
/// how many times it matched there.
struct Repeated {
    names: Box<[ImmutableString]>,
    indices: Vec<usize>,
    count: usize,
}

/// A token of an expansion, and whether the macro's own body wrote it
/// (rather than the caller, through a metavariable).
type Output = Vec<(Token, Position, bool)>;

/// A match in progress: what it has bound, and where the attempt that got
/// furthest stopped, which rustc names when no rule matches.
struct State {
    bound: Vec<Bound>,
    /// How many times each repetition matched, at its indices: an
    /// iteration whose own repetitions matched nothing binds nothing, so
    /// the bindings alone can't count it.
    repeated: Vec<Repeated>,
    /// The furthest stop: a token, or `None` for the call's end.
    stop: Option<(Position, Option<Token>)>,
    /// What closes the trees being matched: the call (`None`), or the
    /// group whose contents they are, at its closing delimiter.
    end: (Position, Option<Token>),
}

impl State {
    /// Note that a rule stops at the start of `input`.
    fn stop_at(&mut self, input: &[Tree]) {
        let at = match input.first() {
            // A lifetime is one token to rustc.
            Some(Tree::Leaf(Token::Apostrophe, position))
                if let Some(Tree::Leaf(name, _)) = input.get(1) =>
            {
                let lifetime = format!("'{}", text(name));
                (
                    *position,
                    Some(Token::Identifier(Box::new(lifetime.into()))),
                )
            }
            Some(Tree::Leaf(token, position)) => (*position, Some(token.clone())),
            Some(tree @ Tree::Group(_, position, ..)) => {
                (*position, Some(first_token(tree).clone()))
            }
            None => self.end.clone(),
        };
        if self
            .stop
            .as_ref()
            .is_none_or(|(furthest, _)| at.0 > *furthest)
        {
            self.stop = Some(at);
        }
    }

    /// Whether the trees after a match are none, as a rule needs at the
    /// end of its matcher or of a group.
    fn exhausted(&mut self, rest: &[Tree]) -> bool {
        if rest.is_empty() {
            return true;
        }
        self.stop_at(rest);
        false
    }
}

impl Malformed {
    fn new(message: impl Into<ImmutableString>, label: &'static str, at: Position) -> Self {
        Self {
            message: message.into(),
            label,
            help: None,
            at,
        }
    }

    /// A fragment specifier rustc doesn't know, or one left out.
    fn fragment(message: impl Into<ImmutableString>, at: Position) -> Self {
        Self {
            help: Some(SPECIFIERS),
            ..Self::new(message, "", at)
        }
    }

    /// rustc's help, when it gives one.
    pub(crate) const fn help(&self) -> Option<&'static str> {
        self.help
    }

    /// rustc's message.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    /// rustc's label under the token, empty when it gives none.
    pub(crate) const fn label(&self) -> &'static str {
        self.label
    }

    /// Where rustc reports it.
    pub(crate) const fn at(&self) -> Position {
        self.at
    }
}

impl fmt::Display for Malformed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRules(Some((token, _))) => {
                write!(formatter, "no rules expected {}", described(token))
            }
            Self::NoRules(None) => formatter.write_str("unexpected end of macro invocation"),
            Self::Transcription(message) => formatter.write_str(message),
        }
    }
}

/// The prefix hygiene renames a macro's local with, before its number.
const HYGIENE_PREFIX: &str = "__rawr_macro";

/// `text` with each macro's local named as the macro writes it: hygiene's
/// `__rawr_macro3_x` is `x`.
pub(crate) fn written(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(HYGIENE_PREFIX) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(HYGIENE_PREFIX) {
        output.push_str(&rest[..at]);
        let after = &rest[at + HYGIENE_PREFIX.len()..];
        let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        match after[digits..].strip_prefix('_') {
            Some(name) if digits > 0 => rest = name,
            _ => {
                output.push_str(HYGIENE_PREFIX);
                rest = after;
            }
        }
    }
    output.push_str(rest);
    std::borrow::Cow::Owned(output)
}

/// `token` as rustc names it in an error: a keyword says it is one.
fn described(token: &Token) -> String {
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where", "while", "async", "await", "dyn",
    ];
    const RESERVED: &[&str] = &[
        "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof",
        "unsized", "virtual", "yield", "try",
    ];
    let written = text(token);
    let raw = written.starts_with("r#");
    let kind = match written.as_str() {
        "_" => "reserved identifier ",
        word if !raw && KEYWORDS.contains(&word) => "keyword ",
        word if !raw && RESERVED.contains(&word) => "reserved keyword ",
        _ => "",
    };
    format!("{kind}`{written}`")
}

/// rustc's list of fragment specifiers, in its help.
const SPECIFIERS: &str = "valid fragment specifiers are `ident`, `block`, `stmt`, `expr`, \
                          `pat`, `ty`, `lifetime`, `literal`, `path`, `meta`, `tt`, `item` and \
                          `vis`, along with `expr_2021` and `pat_param` for edition compatibility";

/// Group `tokens` into token trees. `()` is one token to the lexer, so it
/// is an empty parenthesised group.
pub(crate) fn trees(tokens: &[(Token, Position)]) -> Result<Vec<Tree>, ImmutableString> {
    let mut stack: Vec<(Delimiter, Position, Vec<Tree>)> = Vec::new();
    let mut current = Vec::new();
    for (token, position) in tokens {
        let position = *position;
        let open = match token {
            Token::LeftParen => Some(Delimiter::Paren),
            Token::LeftBracket => Some(Delimiter::Bracket),
            Token::LeftBrace => Some(Delimiter::Brace),
            Token::MapStart => {
                current.push(Tree::Leaf(Token::Hash, position));
                Some(Delimiter::Brace)
            }
            _ => None,
        };
        if let Some(delimiter) = open {
            stack.push((delimiter, position, std::mem::take(&mut current)));
            continue;
        }
        let close = match token {
            Token::RightParen => Some(Delimiter::Paren),
            Token::RightBracket => Some(Delimiter::Bracket),
            Token::RightBrace => Some(Delimiter::Brace),
            _ => None,
        };
        if let Some(delimiter) = close {
            let Some((open, start, outer)) = stack.pop() else {
                return Err(format!("unexpected closing delimiter `{token}`").into());
            };
            if open != delimiter {
                return Err(format!("mismatched closing delimiter `{token}`").into());
            }
            let inner = std::mem::replace(&mut current, outer);
            current.push(Tree::Group(open, start, inner, position));
            continue;
        }
        match token {
            Token::EOF => break,
            Token::Unit => current.push(Tree::Group(
                Delimiter::Paren,
                position,
                Vec::new(),
                position,
            )),
            token => current.push(Tree::Leaf(token.clone(), position)),
        }
    }
    if !stack.is_empty() {
        return Err("unclosed delimiter".into());
    }
    Ok(current)
}

/// The text a token is written as.
/// Whether `trees` hold a token that starts an item, at any depth.
fn holds_item(trees: &[Tree]) -> bool {
    trees.iter().any(|tree| match tree {
        Tree::Leaf(token, _) => starts_item(token),
        Tree::Group(_, _, inner, _) => holds_item(inner),
    })
}

/// Whether `trees` hold a token that starts an item, or a path's `::`,
/// at any depth.
fn holds_item_or_path(trees: &[Tree]) -> bool {
    trees.iter().any(|tree| match tree {
        Tree::Leaf(token, _) => starts_item(token) || *token == Token::DoubleColon,
        Tree::Group(_, _, inner, _) => holds_item_or_path(inner),
    })
}

/// Whether `token` is a keyword only an item starts with.
fn starts_item(token: &Token) -> bool {
    match token {
        Token::Mod | Token::Struct | Token::Enum | Token::Fn | Token::Impl | Token::Use => true,
        Token::Identifier(word) => matches!(word.as_str(), "trait" | "type" | "union"),
        Token::Reserved(word) => word.as_str() == "static",
        _ => false,
    }
}

fn text(token: &Token) -> String {
    match token {
        Token::Identifier(name) => crate::tokenizer::raw_names(name).into_owned(),
        token => token.to_string(),
    }
}

fn is_dollar(token: &Token) -> bool {
    matches!(token, Token::Reserved(text) if text.as_str() == "$")
}

/// Two tokens are the same token as Rust compares them: the lexer's unary
/// `+` and `-` are the binary ones.
fn same(left: &Token, right: &Token) -> bool {
    let plain = |token: &Token| match token {
        Token::UnaryPlus => Token::Plus,
        Token::UnaryMinus => Token::Minus,
        token => token.clone(),
    };
    plain(left) == plain(right)
}

fn op(token: &Token) -> Option<Op> {
    match token {
        Token::Multiply => Some(Op::Star),
        Token::Plus | Token::UnaryPlus => Some(Op::Plus),
        Token::Reserved(text) if text.as_str() == "?" => Some(Op::Question),
        _ => None,
    }
}

impl Rules {
    /// Parse `macro_rules! name { (matcher) => { transcriber }; .. }` from
    /// its source: a dependency's, which rustc has checked under its own
    /// crate's edition, or a definition the program's parse checked.
    pub(crate) fn parse(source: &str) -> Result<Self, ImmutableString> {
        let engine = crate::Engine::new_raw();
        let tokens = super::lower_nominal::lex_rust(&engine, source);
        if let Some((Token::LexError(error), _)) = tokens
            .iter()
            .find(|(token, _)| matches!(token, Token::LexError(_)))
        {
            return Err(format!("its source does not lex: {error}").into());
        }
        Self::read(&tokens).map_err(|malformed| malformed.message)
    }

    /// Parse the program's definition from its tokens, from `macro_rules`
    /// to its end, with rustc's error for one it refuses: one it cannot
    /// read, or a fragment followed by what Rust's follow set does not
    /// allow after it.
    pub(crate) fn from_tokens(tokens: &[(Token, Position)]) -> Result<Self, Malformed> {
        let rules = Self::read(tokens)?;
        for rule in &rules.rules {
            check_follow(&rule.matcher, &[Follower::End])?;
        }
        Ok(rules)
    }

    /// A definition read from its tokens, from `macro_rules` to its end,
    /// with rustc's error for one it cannot read.
    fn read(tokens: &[(Token, Position)]) -> Result<Self, Malformed> {
        let start = tokens
            .first()
            .map_or(Position::NONE, |(_, position)| *position);
        let trees = trees(tokens).map_err(|reason| Malformed::new(reason, "", start))?;
        let (name, body, close) = match trees.as_slice() {
            [Tree::Leaf(keyword, _), Tree::Leaf(Token::Bang, _), Tree::Leaf(name, _), Tree::Group(_, _, body, close), ..]
                if text(keyword) == "macro_rules" =>
            {
                (text(name), body, *close)
            }
            _ => {
                return Err(Malformed::new(
                    "expected a `macro_rules!` definition",
                    "",
                    start,
                ));
            }
        };
        if body.is_empty() {
            return Err(Malformed::new(
                "macros must contain at least one rule",
                "",
                start,
            ));
        }
        let ended = |label| Malformed::new("macro definition ended unexpectedly", label, close);
        let mut rules = Vec::new();
        let mut index = 0;
        while index < body.len() {
            let matcher = match &body[index] {
                Tree::Group(_, _, matcher, _) => matcher,
                Tree::Leaf(_, position) => {
                    return Err(Malformed::new(
                        "invalid macro matcher; matchers must be contained in balanced delimiters",
                        "",
                        *position,
                    ));
                }
            };
            match body.get(index + 1) {
                Some(Tree::Leaf(Token::DoubleArrow, _)) => {}
                Some(tree) => {
                    return Err(Malformed::new(
                        format!("expected `=>`, found `{}`", text(first_token(tree))),
                        "expected `=>`",
                        tree_position(tree),
                    ));
                }
                None => return Err(ended("expected `=>`")),
            }
            let transcriber = match body.get(index + 2) {
                Some(Tree::Group(_, _, transcriber, _)) => transcriber,
                Some(Tree::Leaf(_, position)) => {
                    return Err(Malformed::new("macro rhs must be delimited", "", *position));
                }
                None => return Err(ended("expected right-hand side of macro rule")),
            };
            let matcher = patterns(matcher)?;
            let mut depths = Vec::new();
            collect_depths(&matcher, 0, &mut depths);
            rules.push(Rule {
                matcher,
                transcriber: transcriber.clone(),
                depths,
            });
            index += 3;
            match body.get(index) {
                Some(Tree::Leaf(Token::SemiColon, _)) => index += 1,
                Some(tree) => {
                    return Err(Malformed::new(
                        format!("expected `;`, found `{}`", text(first_token(tree))),
                        "unexpected token",
                        tree_position(tree),
                    ));
                }
                None => {}
            }
        }
        let declares = rules
            .iter()
            .any(|rule| holds_item_or_path(&rule.transcriber));
        Ok(Self {
            name: name.into(),
            rules,
            declares,
        })
    }

    /// Whether a call with `input` may expand to an item, or to a path the
    /// resolver resolves before the lowering reads it: its tokens come from
    /// the rules' transcribers and the input, so one of them holds a token
    /// that starts an item, or a transcriber a path's `::`. A path in the
    /// input is resolved where it is written.
    pub(crate) fn may_declare(&self, input: &[Tree]) -> bool {
        self.declares || holds_item(input)
    }

    /// The macro's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The tokens `input` (the trees inside the invocation's delimiters)
    /// expands to under the first rule that matches it, `$crate` reading as
    /// `krate`. Tokens of the macro's own body take `at`, the invocation's
    /// position, as rustc reports them; the local names they bind are
    /// renamed apart with `hygiene`. `end` is where the call's closing
    /// delimiter is.
    pub(crate) fn expand(
        &self,
        input: &[Tree],
        krate: &str,
        at: Position,
        end: Position,
        hygiene: usize,
    ) -> Result<Vec<(Token, Position)>, Failure> {
        let mut state = State {
            bound: Vec::new(),
            repeated: Vec::new(),
            stop: None,
            end: (end, None),
        };
        for rule in &self.rules {
            state.bound.clear();
            state.repeated.clear();
            if !matches(
                &rule.matcher,
                input,
                &mut Vec::new(),
                &mut state,
                &mut |rest, _, state| state.exhausted(rest),
            ) {
                continue;
            }
            let transcription = Transcription {
                bound: &state.bound,
                repeated: &state.repeated,
                depths: &rule.depths,
                krate,
                at,
            };
            let mut output = Output::new();
            transcription
                .transcribe(&rule.transcriber, &mut Vec::new(), &mut output)
                .map_err(Failure::Transcription)?;
            rename_locals(&mut output, hygiene);
            resign(&mut output);
            return Ok(output
                .into_iter()
                .map(|(token, position, _)| (token, position))
                .collect());
        }
        Err(Failure::NoRules(state.stop.and_then(
            |(position, token)| token.map(|token| (token, position)),
        )))
    }
}

fn first_token(tree: &Tree) -> &Token {
    match tree {
        Tree::Leaf(token, _) => token,
        Tree::Group(Delimiter::Paren, ..) => &Token::LeftParen,
        Tree::Group(Delimiter::Bracket, ..) => &Token::LeftBracket,
        Tree::Group(Delimiter::Brace, ..) => &Token::LeftBrace,
    }
}

const fn tree_position(tree: &Tree) -> Position {
    match tree {
        Tree::Leaf(_, position) | Tree::Group(_, position, ..) => *position,
    }
}

fn patterns(trees: &[Tree]) -> Result<Vec<Pattern>, Malformed> {
    let mut patterns = Vec::new();
    let mut index = 0;
    while index < trees.len() {
        match &trees[index] {
            Tree::Leaf(dollar, at) if is_dollar(dollar) => match trees.get(index + 1) {
                Some(Tree::Leaf(name, _)) => {
                    let (Some(Tree::Leaf(Token::Colon, _)), Some(Tree::Leaf(kind, _))) =
                        (trees.get(index + 2), trees.get(index + 3))
                    else {
                        return Err(Malformed::fragment("missing fragment specifier", *at));
                    };
                    let fragment = match text(kind).as_str() {
                        "expr" | "expr_2021" => Fragment::Expr,
                        "ty" => Fragment::Ty,
                        "path" => Fragment::Path,
                        "ident" => Fragment::Ident,
                        "pat" => Fragment::Pat,
                        "pat_param" => Fragment::PatParam,
                        "stmt" => Fragment::Stmt,
                        "block" => Fragment::Block,
                        "item" => Fragment::Item,
                        "literal" => Fragment::Literal,
                        "lifetime" => Fragment::Lifetime,
                        "meta" => Fragment::Meta,
                        "tt" => Fragment::Tt,
                        "vis" => Fragment::Vis,
                        other => {
                            return Err(Malformed::fragment(
                                format!("invalid fragment specifier `{other}`"),
                                *at,
                            ));
                        }
                    };
                    patterns.push(Pattern::Fragment(text(name).into(), fragment, *at));
                    index += 4;
                }
                Some(Tree::Group(Delimiter::Paren, _, inner, _)) => {
                    let inner = self::patterns(inner)?;
                    let (separator, op, next) = repetition(trees, index + 2)?;
                    let separator_at = trees.get(index + 2).map_or(Position::NONE, tree_position);
                    patterns.push(Pattern::Repeat(inner, separator, op, separator_at));
                    index = next;
                }
                _ => {
                    return Err(Malformed::new(
                        "expected identifier or `(` after `$`",
                        "",
                        *at,
                    ));
                }
            },
            Tree::Leaf(token, at) => {
                patterns.push(Pattern::Token(token.clone(), *at));
                index += 1;
            }
            Tree::Group(delimiter, open, inner, _) => {
                patterns.push(Pattern::Group(*delimiter, self::patterns(inner)?, *open));
                index += 1;
            }
        }
    }
    Ok(patterns)
}

/// The separator and operator after a repetition's `$( .. )`, and the
/// index after them.
fn repetition(trees: &[Tree], index: usize) -> Result<(Option<Token>, Op, usize), Malformed> {
    match (trees.get(index), trees.get(index + 1)) {
        (Some(Tree::Leaf(token, _)), _) if let Some(op) = op(token) => Ok((None, op, index + 1)),
        (Some(Tree::Leaf(separator, _)), Some(Tree::Leaf(token, _)))
            if let Some(op) = op(token) =>
        {
            Ok((Some(separator.clone()), op, index + 2))
        }
        (first, second) => Err(Malformed::new(
            "expected one of: `*`, `+`, or `?`",
            "",
            second
                .or(first)
                .or(index.checked_sub(1).and_then(|last| trees.get(last)))
                .map_or(Position::NONE, tree_position),
        )),
    }
}

/// What may come after a fragment in a matcher, for rustc's follow-set
/// check ([`check_follow`]).
#[derive(Clone, Copy)]
enum Follower<'a> {
    /// The matcher's end, or a group's, which any fragment may meet.
    End,
    Token(&'a Token, Position),
    Fragment(&'a str, Fragment, Position),
    Open(Delimiter, Position),
}

/// Check that each fragment among `patterns` is followed only by what Rust
/// allows after it, as rustc checks a definition, `after` being what may
/// follow the sequence. A repetition's last fragment meets its separator
/// and what follows the repetition, not its own start again, as in rustc.
fn check_follow<'a>(patterns: &'a [Pattern], after: &[Follower<'a>]) -> Result<(), Malformed> {
    for (index, pattern) in patterns.iter().enumerate() {
        match pattern {
            Pattern::Token(..) => {}
            Pattern::Group(_, inner, _) => check_follow(inner, &[Follower::End])?,
            Pattern::Repeat(inner, separator, _, at) => {
                let mut within: Vec<Follower<'a>> = separator
                    .iter()
                    .map(|separator| Follower::Token(separator, *at))
                    .collect();
                within.extend(firsts(&patterns[index + 1..], after));
                check_follow(inner, &within)?;
            }
            Pattern::Fragment(name, fragment, _) => {
                let followers = firsts(&patterns[index + 1..], after);
                let Some(follower) = followers
                    .iter()
                    .find(|follower| !follows(*fragment, follower))
                else {
                    continue;
                };
                let (shown, at) = match follower {
                    Follower::End => unreachable!("any fragment may end a matcher"),
                    Follower::Token(token, at) => (text(token), *at),
                    Follower::Fragment(name, fragment, at) => {
                        (format!("${name}:{}", fragment.specifier()), *at)
                    }
                    Follower::Open(delimiter, at) => (delimiters(*delimiter).0.to_string(), *at),
                };
                let counted = followers
                    .iter()
                    .filter(|follower| !matches!(follower, Follower::End))
                    .count();
                let verb = if counted == 1 { "is" } else { "may be" };
                let specifier = fragment.specifier();
                return Err(Malformed::new(
                    format!(
                        "`${name}:{specifier}` {verb} followed by `{shown}`, which is not \
                         allowed for `{specifier}` fragments"
                    ),
                    fragment.not_followed(),
                    at,
                ));
            }
        }
    }
    Ok(())
}

/// What may start `patterns`, `after` when they may match nothing.
fn firsts<'a>(patterns: &'a [Pattern], after: &[Follower<'a>]) -> Vec<Follower<'a>> {
    let mut found = Vec::new();
    for pattern in patterns {
        match pattern {
            Pattern::Token(token, at) => {
                found.push(Follower::Token(token, *at));
                return found;
            }
            Pattern::Fragment(name, fragment, at) => {
                found.push(Follower::Fragment(name, *fragment, *at));
                return found;
            }
            Pattern::Group(delimiter, _, at) => {
                found.push(Follower::Open(*delimiter, *at));
                return found;
            }
            Pattern::Repeat(inner, _, op, _) => {
                found.extend(firsts(inner, &[]));
                if *op == Op::Plus {
                    return found;
                }
            }
        }
    }
    found.extend_from_slice(after);
    found
}

/// Whether Rust lets `follower` come after a `fragment`: its follow set.
fn follows(fragment: Fragment, follower: &Follower<'_>) -> bool {
    let token = match follower {
        Follower::End => return true,
        Follower::Token(token, _) => Some(*token),
        Follower::Fragment(..) | Follower::Open(..) => None,
    };
    let word = token.map(text);
    let word = word.as_deref();
    match fragment {
        Fragment::Expr | Fragment::Stmt => {
            matches!(
                token,
                Some(Token::DoubleArrow | Token::Comma | Token::SemiColon)
            )
        }
        Fragment::Pat | Fragment::PatParam => {
            matches!(
                token,
                Some(Token::DoubleArrow | Token::Comma | Token::Equals | Token::If | Token::In)
            ) || fragment == Fragment::PatParam && matches!(token, Some(Token::Pipe))
        }
        Fragment::Path | Fragment::Ty => {
            matches!(
                follower,
                Follower::Open(Delimiter::Brace | Delimiter::Bracket, _)
                    | Follower::Fragment(_, Fragment::Block, _)
            ) || matches!(
                token,
                Some(
                    Token::DoubleArrow
                        | Token::Comma
                        | Token::GreaterThan
                        | Token::RightShift
                        | Token::Equals
                        | Token::Colon
                        | Token::SemiColon
                        | Token::Pipe
                        | Token::As
                )
            ) || word == Some("where")
        }
        Fragment::Vis => match follower {
            Follower::Fragment(_, fragment, _) => {
                matches!(fragment, Fragment::Ident | Fragment::Ty | Fragment::Path)
            }
            Follower::Open(delimiter, _) => *delimiter != Delimiter::Brace,
            _ => {
                let named = word.is_some_and(|word| {
                    word.starts_with(|c: char| c.is_alphabetic() || c == '_')
                        && word.chars().all(|c| c.is_alphanumeric() || c == '_')
                });
                named && word != Some("priv")
                    || matches!(
                        token,
                        Some(
                            Token::Comma
                                | Token::Bang
                                | Token::Multiply
                                | Token::Ampersand
                                | Token::And
                                | Token::Apostrophe
                                | Token::LessThan
                                | Token::LeftShift
                                | Token::DoubleColon
                                | Token::Underscore
                        )
                    )
                    || word == Some("?")
            }
        },
        _ => true,
    }
}

impl Fragment {
    /// The specifier a matcher writes it with.
    const fn specifier(self) -> &'static str {
        match self {
            Self::Expr => "expr",
            Self::Ty => "ty",
            Self::Path => "path",
            Self::Ident => "ident",
            Self::Pat => "pat",
            Self::PatParam => "pat_param",
            Self::Stmt => "stmt",
            Self::Block => "block",
            Self::Item => "item",
            Self::Literal => "literal",
            Self::Lifetime => "lifetime",
            Self::Meta => "meta",
            Self::Tt => "tt",
            Self::Vis => "vis",
        }
    }

    /// rustc's label under a token its follow set does not allow.
    const fn not_followed(self) -> &'static str {
        match self {
            Self::Expr => "not allowed after `expr` fragments",
            Self::Ty => "not allowed after `ty` fragments",
            Self::Path => "not allowed after `path` fragments",
            Self::Pat => "not allowed after `pat` fragments",
            Self::PatParam => "not allowed after `pat_param` fragments",
            Self::Stmt => "not allowed after `stmt` fragments",
            Self::Vis => "not allowed after `vis` fragments",
            _ => "",
        }
    }
}

fn collect_depths(patterns: &[Pattern], depth: usize, depths: &mut Vec<(ImmutableString, usize)>) {
    for pattern in patterns {
        match pattern {
            Pattern::Token(..) => {}
            Pattern::Group(_, inner, _) => collect_depths(inner, depth, depths),
            Pattern::Fragment(name, ..) => depths.push((name.clone(), depth)),
            Pattern::Repeat(inner, ..) => collect_depths(inner, depth + 1, depths),
        }
    }
}

type Continue<'k> = dyn FnMut(&[Tree], &mut Vec<usize>, &mut State) -> bool + 'k;

/// Whether `patterns` match a prefix of `input` that `then` accepts the
/// rest after, binding metavariables in `state` at the repetition
/// `indices`. Rules backtrack over repetitions as rustc's do over rules.
fn matches(
    patterns: &[Pattern],
    input: &[Tree],
    indices: &mut Vec<usize>,
    state: &mut State,
    then: &mut Continue<'_>,
) -> bool {
    let Some((first, rest)) = patterns.split_first() else {
        return then(input, indices, state);
    };
    match first {
        Pattern::Token(token, _) => match input.first() {
            Some(Tree::Leaf(found, _)) if same(found, token) => {
                matches(rest, &input[1..], indices, state, then)
            }
            _ => {
                state.stop_at(input);
                false
            }
        },
        Pattern::Group(delimiter, inner, _) => match input.first() {
            Some(Tree::Group(found, _, contents, close)) if found == delimiter => {
                let mark = (state.bound.len(), state.repeated.len());
                let outer =
                    std::mem::replace(&mut state.end, (*close, Some(delimiters(*delimiter).1)));
                let within = matches(inner, contents, indices, state, &mut |rest, _, state| {
                    state.exhausted(rest)
                });
                state.end = outer;
                if within && matches(rest, &input[1..], indices, state, then) {
                    return true;
                }
                state.bound.truncate(mark.0);
                state.repeated.truncate(mark.1);
                false
            }
            _ => {
                state.stop_at(input);
                false
            }
        },
        Pattern::Fragment(name, fragment, _) => {
            let Some(length) = super::fragment::length(*fragment, input) else {
                state.stop_at(input);
                return false;
            };
            state.bound.push(Bound {
                name: name.clone(),
                indices: indices.clone(),
                fragment: *fragment,
                trees: input[..length].to_vec(),
            });
            if matches(rest, &input[length..], indices, state, then) {
                return true;
            }
            state.bound.pop();
            false
        }
        Pattern::Repeat(inner, separator, op, _) => repeat(
            inner,
            separator.as_ref(),
            *op,
            rest,
            input,
            0,
            indices,
            state,
            then,
        ),
    }
}

/// A repetition after `count` iterations: one more, greedily, else the
/// rest of the rule.
#[allow(clippy::too_many_arguments)]
fn repeat(
    inner: &[Pattern],
    separator: Option<&Token>,
    op: Op,
    rest: &[Pattern],
    input: &[Tree],
    count: usize,
    indices: &mut Vec<usize>,
    state: &mut State,
    then: &mut Continue<'_>,
) -> bool {
    if !(op == Op::Question && count == 1) {
        let after = match (count, separator) {
            (0, _) | (_, None) => Some(input),
            (_, Some(separator)) => match input.first() {
                Some(Tree::Leaf(found, _)) if same(found, separator) => Some(&input[1..]),
                _ => None,
            },
        };
        if let Some(after) = after {
            let mark = (state.bound.len(), state.repeated.len());
            indices.push(count);
            let found = matches(
                inner,
                after,
                indices,
                state,
                &mut |remaining, indices, state| {
                    // Each iteration takes at least one token, as in rustc.
                    if remaining.len() == after.len() {
                        return false;
                    }
                    let index = indices.pop();
                    let found = repeat(
                        inner,
                        separator,
                        op,
                        rest,
                        remaining,
                        count + 1,
                        indices,
                        state,
                        then,
                    );
                    indices.extend(index);
                    found
                },
            );
            indices.pop();
            if found {
                return true;
            }
            state.bound.truncate(mark.0);
            state.repeated.truncate(mark.1);
        }
    }
    let least = usize::from(op == Op::Plus);
    if count < least {
        return false;
    }
    let mut names = Vec::new();
    collect_depths(inner, 0, &mut names);
    state.repeated.push(Repeated {
        names: names.into_iter().map(|(name, _)| name).collect(),
        indices: indices.clone(),
        count,
    });
    if matches(rest, input, indices, state, then) {
        return true;
    }
    state.repeated.pop();
    false
}

struct Transcription<'a> {
    bound: &'a [Bound],
    repeated: &'a [Repeated],
    depths: &'a [(ImmutableString, usize)],
    krate: &'a str,
    at: Position,
}

impl Transcription<'_> {
    fn depth(&self, name: &str) -> Option<usize> {
        self.depths
            .iter()
            .find(|(bound, _)| bound.as_str() == name)
            .map(|(_, depth)| *depth)
    }

    fn transcribe(
        &self,
        trees: &[Tree],
        indices: &mut Vec<usize>,
        output: &mut Output,
    ) -> Result<(), ImmutableString> {
        let mut index = 0;
        while index < trees.len() {
            match &trees[index] {
                Tree::Leaf(dollar, _) if is_dollar(dollar) => match trees.get(index + 1) {
                    Some(Tree::Leaf(name, _)) if text(name) == "crate" => {
                        for (position, segment) in self.krate.split("::").enumerate() {
                            if position > 0 {
                                output.push((Token::DoubleColon, self.at, true));
                            }
                            output.push((
                                Token::Identifier(Box::new(segment.into())),
                                self.at,
                                true,
                            ));
                        }
                        index += 2;
                    }
                    Some(Tree::Leaf(name, _)) if self.depth(&text(name)).is_some() => {
                        self.substitute(&text(name), indices, output)?;
                        index += 2;
                    }
                    Some(Tree::Group(Delimiter::Paren, _, inner, _)) => {
                        let (separator, _, next) =
                            repetition(trees, index + 2).map_err(|malformed| malformed.message)?;
                        let count = self.count(inner, indices)?;
                        for iteration in 0..count {
                            if iteration > 0
                                && let Some(separator) = &separator
                            {
                                output.push((separator.clone(), self.at, true));
                            }
                            indices.push(iteration);
                            self.transcribe(inner, indices, output)?;
                            indices.pop();
                        }
                        index = next;
                    }
                    _ => {
                        output.push((dollar.clone(), self.at, true));
                        index += 1;
                    }
                },
                Tree::Leaf(token, _) => {
                    output.push((token.clone(), self.at, true));
                    index += 1;
                }
                Tree::Group(delimiter, ..) => {
                    let Tree::Group(_, _, inner, _) = &trees[index] else {
                        unreachable!()
                    };
                    let (open, close) = delimiters(*delimiter);
                    let start = output.len();
                    output.push((open, self.at, true));
                    self.transcribe(inner, indices, output)?;
                    if output.len() == start + 1 && *delimiter == Delimiter::Paren {
                        output[start].0 = Token::Unit;
                    } else {
                        output.push((close, self.at, true));
                    }
                    index += 1;
                }
            }
        }
        Ok(())
    }

    /// The number of iterations of a repetition over `inner` at `indices`:
    /// how many times the matcher's repetition of each metavariable inside
    /// it that repeats deeper matched there.
    fn count(&self, inner: &[Tree], indices: &[usize]) -> Result<usize, ImmutableString> {
        let mut names = BTreeSet::new();
        variables(inner, &mut names);
        let mut count = None;
        for name in names {
            let Some(depth) = self.depth(&name) else {
                continue;
            };
            if depth <= indices.len() {
                continue;
            }
            let found = self
                .repeated
                .iter()
                .find(|repeated| {
                    repeated.indices == indices
                        && repeated.names.iter().any(|named| named.as_str() == name)
                })
                .map_or(0, |repeated| repeated.count);
            match count {
                Some(count) if count != found => {
                    return Err(format!(
                        "meta-variable `{name}` repeats {found} times, but another repeats {count} times"
                    )
                    .into());
                }
                _ => count = Some(found),
            }
        }
        count.ok_or_else(|| {
            "attempted to repeat an expression containing no syntax variables matched as repeating at this depth".into()
        })
    }

    fn substitute(
        &self,
        name: &str,
        indices: &[usize],
        output: &mut Output,
    ) -> Result<(), ImmutableString> {
        let depth = self.depth(name).unwrap_or(0);
        if depth > indices.len() {
            return Err(format!("variable `{name}` is still repeating at this depth").into());
        }
        let Some(bound) = self
            .bound
            .iter()
            .find(|bound| bound.name.as_str() == name && bound.indices == indices[..depth])
        else {
            return Ok(());
        };
        // An `expr` fragment is one expression where it lands, as rustc's
        // parsed fragment is: parenthesised unless it is one operand.
        let wrap = bound.fragment == Fragment::Expr && !operand(&bound.trees);
        if wrap {
            output.push((Token::LeftParen, self.at, false));
        }
        flatten(&bound.trees, output);
        if wrap {
            output.push((Token::RightParen, self.at, false));
        }
        Ok(())
    }
}

fn delimiters(delimiter: Delimiter) -> (Token, Token) {
    match delimiter {
        Delimiter::Paren => (Token::LeftParen, Token::RightParen),
        Delimiter::Bracket => (Token::LeftBracket, Token::RightBracket),
        Delimiter::Brace => (Token::LeftBrace, Token::RightBrace),
    }
}

/// The caller's trees as tokens, at their own positions.
fn flatten(trees: &[Tree], output: &mut Output) {
    for tree in trees {
        match tree {
            Tree::Leaf(token, position) => output.push((token.clone(), *position, false)),
            Tree::Group(Delimiter::Paren, open, inner, _) if inner.is_empty() => {
                output.push((Token::Unit, *open, false));
            }
            Tree::Group(delimiter, open, inner, close) => {
                let (left, right) = delimiters(*delimiter);
                output.push((left, *open, false));
                flatten(inner, output);
                output.push((right, *close, false));
            }
        }
    }
}

/// Whether `trees` are one operand that needs no parentheses: a path, a
/// literal, a call, a field, an index, a method call or a macro call.
fn operand(trees: &[Tree]) -> bool {
    trees.iter().all(|tree| match tree {
        Tree::Group(..) => true,
        Tree::Leaf(token, _) => {
            super::fragment::is_literal(token)
                || matches!(
                    token,
                    Token::Identifier(_)
                        | Token::SelfValue
                        | Token::Period
                        | Token::DoubleColon
                        | Token::Bang
                        | Token::Underscore
                )
                || matches!(token, Token::Reserved(text) if text.as_str() == "?")
        }
    })
}

fn variables(trees: &[Tree], names: &mut BTreeSet<String>) {
    for (index, tree) in trees.iter().enumerate() {
        match tree {
            Tree::Leaf(dollar, _) if is_dollar(dollar) => {
                if let Some(Tree::Leaf(name, _)) = trees.get(index + 1) {
                    names.insert(text(name));
                }
            }
            Tree::Leaf(..) => {}
            Tree::Group(_, _, inner, _) => variables(inner, names),
        }
    }
}

/// Rename the local names the macro's own tokens bind (`let x`, `for x`,
/// `|x|`, a `match` arm's pattern) and every use of them the macro wrote,
/// so the caller's tokens never meet them, as Rust's hygiene keeps them
/// apart. A `{x}` a format string of the macro's captures is a use.
///
/// A field's name and a path's segment are no local's use, as Rust's
/// hygiene leaves a field and a path alone: they are kept, and a field
/// written by its local's name alone is written `field: local`.
fn rename_locals(output: &mut Output, hygiene: usize) {
    let roles = roles(output);
    let mut locals = BTreeSet::new();
    for index in 0..output.len() {
        let (token, _, own) = &output[index];
        if !own || roles[index] == Role::Field {
            continue;
        }
        if matches!(token, Token::Reserved(word) if word.as_str() == "match") {
            arm_bindings(output, &roles, index, &mut locals);
            continue;
        }
        let end: fn(&Token) -> bool = match token {
            Token::Let => {
                |token: &Token| matches!(token, Token::Equals | Token::Colon | Token::SemiColon)
            }
            Token::For => |token: &Token| matches!(token, Token::In),
            Token::Pipe
                if index == 0
                    || matches!(
                        output[index - 1].0,
                        Token::LeftParen
                            | Token::Comma
                            | Token::Equals
                            | Token::LeftBrace
                            | Token::SemiColon
                    )
                    || text(&output[index - 1].0) == "move" =>
            {
                |token: &Token| matches!(token, Token::Pipe)
            }
            _ => continue,
        };
        let closure = matches!(token, Token::Pipe);
        let mut depth = 0_usize;
        // A closure parameter's `: Type`, to its `,` outside the type's
        // `<..>`, names a type and binds nothing.
        let mut typed = false;
        let mut angles = 0_usize;
        for position in index + 1..output.len() {
            let (token, _, own) = &output[position];
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                token if depth == 0 && end(token) => break,
                Token::Colon if closure && depth == 0 => typed = true,
                Token::LessThan if typed => angles += 1,
                Token::GreaterThan if typed => angles = angles.saturating_sub(1),
                Token::RightShift if typed => angles = angles.saturating_sub(2),
                Token::Comma if depth == 0 && angles == 0 => typed = false,
                _ => {}
            }
            if !typed
                && *own
                && roles[position] != Role::Field
                && let Token::Identifier(name) = token
                && binding_name(name)
                && !matches!(
                    output.get(position + 1).map(|(token, ..)| token),
                    Some(
                        Token::LeftParen
                            | Token::LeftBrace
                            | Token::DoubleColon
                            | Token::Bang
                            | Token::Unit
                    )
                )
            {
                locals.insert(name.to_string());
            }
        }
    }
    if locals.is_empty() {
        return;
    }
    let renamed = |name: &str| format!("{HYGIENE_PREFIX}{hygiene}_{name}");
    let mut written = Output::with_capacity(output.len());
    for (index, (token, position, own)) in std::mem::take(output).into_iter().enumerate() {
        let token = match token {
            Token::Identifier(name)
                if own
                    && matches!(roles[index], Role::Plain | Role::Shorthand(_))
                    && locals.contains(name.as_str()) =>
            {
                if let Role::Shorthand(start) = roles[index] {
                    // `Point { x }` is `Point { x: x }`, its local renamed.
                    let at = written.len() - (index - start);
                    written.insert(at, (Token::Colon, position, true));
                    written.insert(at, (Token::Identifier(name.clone()), position, true));
                }
                Token::Identifier(Box::new(renamed(&name).into()))
            }
            Token::StringConstant(text) if own && roles[index] == Role::Format => {
                Token::StringConstant(Box::new(captures(&text, &locals, &renamed).into()))
            }
            token => token,
        };
        written.push((token, position, own));
    }
    *output = written;
}

/// Note the names the arm patterns of the `match` at `index` bind among
/// the macro's own tokens: from the body's `{` or an arm's end to its
/// `=>` or its guard's `if`.
fn arm_bindings(output: &Output, roles: &[Role], index: usize, locals: &mut BTreeSet<String>) {
    let Some(open) = (index + 1..output.len()).find(|at| output[*at].0 == Token::LeftBrace) else {
        return;
    };
    let mut depth = 0_usize;
    let mut pattern = true;
    // Whether the group open at the arms' depth is an arm's block body.
    let mut block = false;
    for position in open + 1..output.len() {
        let (token, _, own) = &output[position];
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => {
                if depth == 0 {
                    block = output[position - 1].0 == Token::DoubleArrow;
                }
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace if depth == 0 => return,
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth -= 1;
                // A block arm's `}` ends it without a `,`.
                if depth == 0
                    && std::mem::take(&mut block)
                    && output
                        .get(position + 1)
                        .is_some_and(|(next, ..)| *next != Token::Comma)
                {
                    pattern = true;
                }
            }
            Token::DoubleArrow | Token::If if depth == 0 => pattern = false,
            Token::Comma if depth == 0 && !pattern => pattern = true,
            Token::Identifier(name)
                if pattern
                    && *own
                    && roles[position] == Role::Plain
                    && binding_name(name)
                    && !matches!(
                        output.get(position + 1).map(|(token, ..)| token),
                        Some(Token::LeftParen | Token::LeftBrace | Token::Unit)
                    ) =>
            {
                locals.insert(name.to_string());
            }
            _ => {}
        }
    }
}

/// What a token of an expansion is to hygiene (`rename_locals`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// A name that may use a local.
    Plain,
    /// A field's name before its `:`, in a struct's definition, expression
    /// or pattern.
    Field,
    /// A field written by its binding's name alone, `Point { x }`, with its
    /// `ref` or `mut` from the index given.
    Shorthand(usize),
    /// A path's segment: before `::` or `!`, or after `::` or `.`.
    Segment,
    /// A string a macro call reads, whose `{name}` may use a local.
    Format,
}

/// Each token's [`Role`] in `output`.
fn roles(output: &Output) -> Vec<Role> {
    let mut roles = vec![Role::Plain; output.len()];
    // For each group open around the token, whether it is a struct's
    // braces: `Name {` or `Self {`, the name capitalised as a type's is.
    let mut groups: Vec<bool> = Vec::new();
    for (index, (token, ..)) in output.iter().enumerate() {
        let previous = index.checked_sub(1).map(|before| &output[before].0);
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => {
                let named = matches!(previous, Some(Token::Identifier(name))
                    if name.chars().next().is_some_and(char::is_uppercase));
                groups.push(*token == Token::LeftBrace && named);
                continue;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                groups.pop();
                continue;
            }
            Token::StringConstant(_)
                if matches!(
                    previous,
                    Some(Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::Comma)
                ) && called_group(output, index) =>
            {
                roles[index] = Role::Format;
                continue;
            }
            Token::Identifier(_) => {}
            _ => continue,
        }
        let next = output.get(index + 1).map(|(token, ..)| token);
        if matches!(previous, Some(Token::Period | Token::DoubleColon))
            || matches!(next, Some(Token::DoubleColon | Token::Bang))
        {
            roles[index] = Role::Segment;
            continue;
        }
        if groups.last() != Some(&true) {
            continue;
        }
        // The field starts at the group's `{` or after a `,`, with `ref`
        // and `mut` before its name in a pattern.
        let mut start = index;
        while start > 0
            && (matches!(output[start - 1].0, Token::Mut)
                || matches!(&output[start - 1].0, Token::Identifier(word) if word.as_str() == "ref"))
        {
            start -= 1;
        }
        if start == 0 || !matches!(output[start - 1].0, Token::LeftBrace | Token::Comma) {
            continue;
        }
        roles[index] = match next {
            Some(Token::Colon) if start == index => Role::Field,
            Some(Token::Comma | Token::RightBrace) => Role::Shorthand(start),
            _ => Role::Plain,
        };
    }
    roles
}

/// Whether the token at `index` is directly inside the delimited group of
/// a macro call, `name!(..)`.
fn called_group(output: &Output, index: usize) -> bool {
    let mut depth = 0_usize;
    for before in (0..index).rev() {
        match output[before].0 {
            Token::RightParen | Token::RightBracket | Token::RightBrace => depth += 1,
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace if depth == 0 => {
                return before > 0 && output[before - 1].0 == Token::Bang;
            }
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth -= 1,
            _ => {}
        }
    }
    false
}

/// `text`, a format string, with each `{name}` that names one of `locals`
/// naming it `renamed`, as the implicit argument it captures.
fn captures(text: &str, locals: &BTreeSet<String>, renamed: &dyn Fn(&str) -> String) -> String {
    let mut written = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        written.push_str(&rest[..=open]);
        rest = &rest[open + 1..];
        if let Some(escaped) = rest.strip_prefix('{') {
            written.push('{');
            rest = escaped;
            continue;
        }
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let name = &rest[..end];
        if locals.contains(name) && rest[end..].starts_with(['}', ':']) {
            written.push_str(&renamed(name));
        } else {
            written.push_str(name);
        }
        rest = &rest[end..];
    }
    written.push_str(rest);
    written
}

/// Read each `+` and `-` of an expansion as unary or binary by the token
/// before it in the expansion, as the lexer reads a written one: a
/// transcriber's `$(+ $x)*` follows its `(` in the definition, and a
/// binary operator where it lands after `0`.
fn resign(output: &mut Output) {
    for index in 0..output.len() {
        let unary = index == 0 || output[index - 1].0.is_next_unary();
        let signed = match output[index].0 {
            Token::Plus | Token::UnaryPlus if unary => Token::UnaryPlus,
            Token::Plus | Token::UnaryPlus => Token::Plus,
            Token::Minus | Token::UnaryMinus if unary => Token::UnaryMinus,
            Token::Minus | Token::UnaryMinus => Token::Minus,
            _ => continue,
        };
        output[index].0 = signed;
    }
}

/// A name a pattern binds: lowercase, as Rust's patterns bind (an
/// uppercase name is a constant, a unit struct or a variant), and not the
/// `ref` before one.
fn binding_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_lowercase() || c == '_')
        && !matches!(name, "_" | "ref")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(definition: &str, call: &str) -> String {
        let rules = Rules::parse(definition).unwrap_or_else(|error| panic!("{error}"));
        let engine = crate::Engine::new_raw();
        let tokens = super::super::lower_nominal::lex_rust(&engine, call);
        let input = trees(&tokens).unwrap();
        let output = rules
            .expand(
                &input,
                "dep::__rawr_crate_dep_0",
                Position::NONE,
                Position::NONE,
                1,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        output
            .iter()
            .map(|(token, _)| text(token))
            .collect::<Vec<_>>()
            .join(" ")
    }

    const COLUMN: &str = "macro_rules! column {
        () => (
            $crate::Column::new()
        );
        ($($x:expr),+ $(,)?) => (
            $crate::Column::with_children([$($crate::core::Widget::_boxed($x)),+])
        );
    }";

    #[test]
    fn repetitions_and_crate_paths_expand() {
        assert_eq!(
            expand(COLUMN, ""),
            "dep :: __rawr_crate_dep_0 :: Column :: new ()"
        );
        // An `expr` fragment of more than one operand is parenthesised.
        assert_eq!(
            expand(COLUMN, "a, b + 1,"),
            "dep :: __rawr_crate_dep_0 :: Column :: with_children ( [ \
             dep :: __rawr_crate_dep_0 :: core :: Widget :: _boxed ( a ) , \
             dep :: __rawr_crate_dep_0 :: core :: Widget :: _boxed ( ( b + 1 ) ) ] )"
        );
    }

    #[test]
    fn rules_are_tried_in_order_by_fragment() {
        let color = "macro_rules! color {
            ($r:expr, $g:expr, $b:expr) => { rgb($r, $g, $b) };
            ($hex:literal) => {{ $crate::color!($hex, 1.0) }};
            ($hex:literal, $a:expr) => {{ let mut hex = $hex as u32; packed(hex, $a) }};
        }";
        assert_eq!(expand(color, "1, 2, 3"), "rgb ( 1 , 2 , 3 )");
        assert_eq!(
            expand(color, "0xff"),
            "{ dep :: __rawr_crate_dep_0 :: color ! ( 255 , 1.0 ) }"
        );
        // The macro's `hex` is its own; the caller's `hex` stays.
        assert_eq!(
            expand(color, "0xff, hex"),
            "{ let mut __rawr_macro1_hex = 255 as u32 ; packed ( __rawr_macro1_hex , hex ) }"
        );
    }

    #[test]
    fn a_closure_parameter_type_is_not_renamed() {
        let add = "macro_rules! add { ($v:expr) => {{ let f = |c: i32, m: Vec<u8>, d| c + d; f($v, vec![], 1) }}; }";
        assert_eq!(
            expand(add, "2"),
            "{ let __rawr_macro1_f = | __rawr_macro1_c : i32 , __rawr_macro1_m : Vec < u8 > , \
             __rawr_macro1_d | __rawr_macro1_c + __rawr_macro1_d ; __rawr_macro1_f ( 2 , vec ! [ ] , 1 ) }"
        );
    }

    #[test]
    fn a_fragment_never_starts_at_a_token_that_ends_it() {
        // tracing's `fieldset!` reaches `@ { "message", , }`: one
        // expression, then the trailing commas.
        let list = "macro_rules! list { ($($out:expr),* $(,)*) => { [$($out),*] }; }";
        assert_eq!(expand(list, "\"message\", ,"), "[ \"message\" ]");
        let ty = "macro_rules! ty { ($($t:ty),* $(,)*) => { ($($t,)*) }; }";
        assert_eq!(expand(ty, "u8, , "), "( u8 , )");
    }

    #[test]
    fn token_trees_pass_through() {
        let text = "macro_rules! text { ($($arg:tt)*) => { Text::new(format!($($arg)*)) }; }";
        assert_eq!(
            expand(text, "\"{} {n}\", 1"),
            "Text :: new ( format ! ( \"{} {n}\" , 1 ) )"
        );
    }

    #[test]
    fn a_call_no_rule_matches_names_the_token_the_furthest_rule_stopped_at() {
        let one = "macro_rules! one { ($x:ident) => { $x }; }";
        assert_eq!(failure(one, "1"), "no rules expected `1`");
        assert_eq!(failure(one, "a b"), "no rules expected `b`");
        assert_eq!(failure(one, ""), "unexpected end of macro invocation");
        let two = "macro_rules! two { (a b c) => { 1 }; (a) => { 2 }; }";
        assert_eq!(failure(two, "a b d"), "no rules expected `d`");
        // A group's contents end at its closing delimiter.
        let group = "macro_rules! group { ((a b)) => { 1 }; }";
        assert_eq!(failure(group, "(a)"), "no rules expected `)`");
    }

    fn failure(definition: &str, call: &str) -> String {
        let rules = Rules::parse(definition).unwrap();
        let engine = crate::Engine::new_raw();
        let tokens = super::super::lower_nominal::lex_rust(&engine, call);
        let end = tokens
            .last()
            .map_or(Position::NONE, |(_, position)| *position);
        rules
            .expand(&trees(&tokens).unwrap(), "dep", Position::NONE, end, 0)
            .unwrap_err()
            .to_string()
    }

    fn malformed(definition: &str) -> String {
        let engine = crate::Engine::new_raw();
        let tokens = super::super::lower_nominal::lex_rust(&engine, definition);
        Rules::from_tokens(&tokens).unwrap_err().to_string()
    }

    #[test]
    fn a_definition_rustc_refuses_is_its_error() {
        assert_eq!(
            malformed("macro_rules! m { () => {} () => {} }"),
            "expected `;`, found `(`"
        );
        assert_eq!(
            malformed("macro_rules! m { () {} }"),
            "expected `=>`, found `{`"
        );
        assert_eq!(
            malformed("macro_rules! m { ($x) => {} }"),
            "missing fragment specifier"
        );
        assert_eq!(
            malformed("macro_rules! m { ($x:foo) => {} }"),
            "invalid fragment specifier `foo`"
        );
        assert_eq!(
            malformed("macro_rules! m { ($($x:expr),) => {} }"),
            "expected one of: `*`, `+`, or `?`"
        );
        assert_eq!(
            malformed("macro_rules! m { }"),
            "macros must contain at least one rule"
        );
        assert_eq!(
            malformed("macro_rules! m { () => }"),
            "macro definition ended unexpectedly"
        );
    }
}
