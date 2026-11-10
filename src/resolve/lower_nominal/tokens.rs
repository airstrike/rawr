//! Reading the token stream: the next token, lookahead, the expected
//! tokens of a construct, and where a name is in its source.

use super::*;

impl<'a> Parser<'a> {
    /// `at`, a point where `name` is written, over the name, as rustc
    /// marks a local where it is declared, moved and used.
    pub(super) fn name_span(&self, at: Span, name: &str) -> Span {
        let named = span(at.start(), text_end(at.start(), name));
        if at.start() == at.end() && named.text(self.text) == name {
            named
        } else {
            at
        }
    }

    /// Where `name` is in `written`, the inside of a `{...}` that starts
    /// at char `at` of a `format!` string's value, in the literal at
    /// `literal`: rustc points at it there. None for a literal the source
    /// doesn't show as written.
    pub(super) fn template_name_span(
        &self,
        literal: Span,
        template: &str,
        written: &str,
        at: usize,
        name: &str,
    ) -> Option<Span> {
        let identifier = |character: char| character == '_' || character.is_alphanumeric();
        let offset = written.match_indices(name).find_map(|(offset, _)| {
            let before = written[..offset].chars().next_back();
            let after = written[offset + name.len()..].chars().next();
            (!before.is_some_and(identifier) && !after.is_some_and(identifier))
                .then(|| written[..offset].chars().count())
        })?;
        let start = at + offset;
        literal::value_span(
            literal.text(self.text),
            literal.start(),
            template,
            start..start + name.chars().count(),
        )
    }

    pub(super) fn expect_name(
        &mut self,
        construct: &str,
    ) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name)
                if name.starts_with(SENTINEL)
                    || name.starts_with(METHOD_SENTINEL)
                    || name.starts_with(NOMINAL_MATCH_SENTINEL) =>
            {
                Err(Error::UnsupportedNominalSyntax {
                    construct: "compiler-private nominal intrinsic".into(),
                    source: point_span(position),
                })
            }
            Token::Identifier(name) => Ok(((*name).into(), point_span(position))),
            Token::Reserved(name) if is_valid_identifier(&name) => {
                Ok(((*name).into(), point_span(position)))
            }
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("{construct}; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    pub(super) fn expect_qualified_name(&mut self) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name) | Token::Reserved(name) => {
                Ok(((*name).into(), point_span(position)))
            }
            Token::SelfValue => Err(Error::StdPath {
                code: "E0433",
                message: "`self` in paths can only be used in start position or last position"
                    .into(),
                label: Some("can only be used in path start position or last position".into()),
                note: None,
                reference: Span::new(position, text_end(position, "self")),
            }),
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("qualified nominal member; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    pub(super) fn expect_member_name(&mut self) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name) | Token::Reserved(name) => {
                Ok(((*name).into(), point_span(position)))
            }
            Token::IntegerConstant(index) if index >= 0 => {
                Ok((index.to_string().into(), point_span(position)))
            }
            // `t.0.1`: the number `0.1` is two tuple fields.
            Token::FloatConstant(ref number)
                if let Some((first, second)) = number.1.split_once('.')
                    && !first.is_empty()
                    && first.bytes().all(|byte| byte.is_ascii_digit())
                    && !second.is_empty()
                    && second.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                let second_position = Position::new(
                    position.line().unwrap_or(0) as u16,
                    (position.position().unwrap_or(0) + first.len() + 1) as u16,
                );
                self.pending_member = Some((second.into(), point_span(second_position)));
                Ok((first.into(), point_span(position)))
            }
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("field or capability method; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    pub(super) fn expect(&mut self, expected: Token, construct: &str) -> Result<Position, Error> {
        let (token, position) = self.next();
        if token == expected {
            Ok(position)
        } else {
            Err(Error::UnsupportedNominalSyntax {
                construct: format!("{construct}; found `{token}`").into(),
                source: point_span(position),
            })
        }
    }

    pub(super) fn unsupported(&mut self, construct: &str) -> Error {
        Error::UnsupportedNominalSyntax {
            construct: construct.into(),
            source: point_span(self.position()),
        }
    }

    /// The `>` closing generic arguments, splitting a `>>`.
    pub(super) fn expect_close_angle(&mut self, construct: &str) -> Result<Position, Error> {
        if matches!(self.peek(), Token::RightShift) {
            let (_, position) = self.next();
            let mut second = position;
            second.advance();
            self.split_angle = Some(second);
            return Ok(position);
        }
        self.expect(Token::GreaterThan, construct)
    }

    /// The token after the next one.
    pub(super) fn peek_second(&mut self) -> Token {
        let mut ahead = self.input.clone();
        ahead.next();
        ahead.next().map_or(Token::EOF, |(token, _)| token)
    }

    pub(super) fn peek_identifier(&mut self) -> Option<&str> {
        if self.split_angle.is_some() {
            return None;
        }
        match self.input.peek() {
            Some((Token::Identifier(name), _)) => Some(name),
            _ => None,
        }
    }

    pub(super) fn peek(&mut self) -> Token {
        if self.split_angle.is_some() {
            return Token::GreaterThan;
        }
        self.input
            .peek()
            .expect("token streams always end with EOF")
            .0
            .clone()
    }

    pub(super) fn position(&mut self) -> Position {
        if let Some(position) = self.split_angle {
            return position;
        }
        self.input
            .peek()
            .expect("token streams always end with EOF")
            .1
    }

    pub(super) fn next(&mut self) -> (Token, Position) {
        let (token, position) = match self.split_angle.take() {
            Some(position) => (Token::GreaterThan, position),
            None => self
                .input
                .next()
                .expect("token streams always end with EOF"),
        };
        self.track_calls(&token, position);
        self.models.at = position;
        (token, position)
    }
}

pub(super) fn source_span(source: &str, selected: Span) -> String {
    let mut output = String::with_capacity(source.len());
    let mut line = 1_u16;
    let mut column = 0_u16;

    for character in source.chars() {
        if character == '\n' {
            output.push(character);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        column = column.saturating_add(1);
        let position = Position::new(line, column);
        output.push(
            if position >= selected.start() && position < selected.end() {
                character
            } else {
                ' '
            },
        );
    }

    output
}

pub(super) fn source_full_span(source: &str) -> Span {
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in source.chars() {
        if character == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    Span::new(Position::START, Position::new(line, column))
}

/// Tokens, each with its position.
pub(super) type Tokens = Vec<(Token, Position)>;

/// A call's argument tokens split after the first argument: the first,
/// and the rest after its comma. Commas inside brackets, braces,
/// parentheses, a turbofish or a closure's parameters do not split.
/// How many arguments `tokens` gives, a trailing comma aside.
pub(super) fn argument_count(tokens: &Tokens) -> usize {
    let mut rest = tokens.clone();
    let mut count = 0;
    while !rest.is_empty() {
        rest = split_first_argument(rest).1;
        count += 1;
    }
    count
}

pub(super) fn split_first_argument(tokens: Tokens) -> (Tokens, Tokens) {
    let mut depth = 0usize;
    let mut angle = 0usize;
    let mut closure = false;
    let mut previous_colons = false;
    for (index, (token, _)) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth = depth.saturating_sub(1);
            }
            Token::LessThan if previous_colons || angle > 0 => angle += 1,
            Token::GreaterThan if angle > 0 => angle -= 1,
            Token::RightShift if angle > 0 => angle = angle.saturating_sub(2),
            Token::Pipe if depth == 0 && angle == 0 => closure = !closure,
            Token::Comma if depth == 0 && angle == 0 && !closure => {
                let mut first = tokens;
                let rest = first.split_off(index + 1);
                first.pop();
                return (first, rest);
            }
            _ => {}
        }
        previous_colons = matches!(token, Token::DoubleColon);
    }
    (tokens, Vec::new())
}

/// An argument's tokens without a leading `&`: `Clone::clone(&x)` is
/// `x.clone()`, as Rust's auto-reference makes it.
/// The tokens of the Rust `text`, each at `at`, with each named hole the
/// parenthesised tokens it stands for: a call rewritten as source.
pub(super) fn template_tokens(
    text: &str,
    holes: &[(&str, Vec<(Token, Position)>)],
    at: Position,
) -> Vec<(Token, Position)> {
    let engine = Engine::new_raw();
    let mut tokens = Vec::new();
    for (token, _) in lex_rust(&engine, text) {
        if token == Token::EOF {
            break;
        }
        if let Token::Identifier(name) = &token
            && let Some((_, hole)) = holes.iter().find(|(hole, _)| name.as_str() == *hole)
        {
            tokens.push((Token::LeftParen, at));
            tokens.extend(hole.iter().cloned());
            tokens.push((Token::RightParen, at));
            continue;
        }
        tokens.push((token, at));
    }
    tokens
}

/// The tokens of a receiver taken by `&self` or `&mut self` as a method
/// call's receiver: its `&` or `&mut` dropped (`Iterator::next(&mut it)`
/// is `it.next()`).
pub(super) fn without_borrow(mut tokens: Vec<(Token, Position)>) -> Vec<(Token, Position)> {
    if matches!(tokens.first(), Some((Token::Ampersand, _))) && tokens.len() > 1 {
        tokens.remove(0);
        if matches!(tokens.first(), Some((Token::Mut, _))) && tokens.len() > 1 {
            tokens.remove(0);
        }
    }
    tokens
}

pub(super) fn without_reference(mut tokens: Vec<(Token, Position)>) -> Vec<(Token, Position)> {
    if matches!(tokens.first(), Some((Token::Ampersand, _))) && tokens.len() > 1 {
        tokens.remove(0);
    }
    tokens
}

/// `stringify!`'s text of `tokens` (each with where it starts in `text`),
/// which end where `end` closes the macro, as rustc's `print_tts` prints
/// them: each token as written, and a space after one that the source
/// spaces from the next unless `space_between` says no. `None` when a
/// position is not in `text`.
pub(super) fn stringify_tokens(
    text: &str,
    tokens: &[(Token, Position)],
    end: Position,
) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = vec![0usize];
    for (index, c) in chars.iter().enumerate() {
        if *c == '\n' {
            lines.push(index + 1);
        }
    }
    let offset = |at: Position| -> Option<usize> {
        let line = *lines.get(at.line()?.checked_sub(1)?)?;
        let index = line + at.position()?.checked_sub(1)?;
        (index <= chars.len()).then_some(index)
    };
    let starts = tokens
        .iter()
        .map(|(_, at)| offset(*at))
        .chain(std::iter::once(offset(end)))
        .collect::<Option<Vec<usize>>>()?;
    // A token runs to the next one's start. One a macro's expansion put
    // before tokens from elsewhere (`stringify!($hex)`) runs as far as it
    // is written: a word, a number or a quoted literal.
    let written = |start: usize| -> String {
        let rest = &chars[start..];
        match rest.first() {
            Some(quote @ ('"' | '\'')) => {
                let mut end = 1;
                while end < rest.len() && rest[end] != *quote {
                    end += if rest[end] == '\\' { 2 } else { 1 };
                }
                rest[..(end + 1).min(rest.len())].iter().collect()
            }
            _ => rest
                .iter()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
                .collect(),
        }
    };
    let pieces: Vec<String> = starts
        .windows(2)
        .zip(tokens)
        .map(|(pair, (token, _))| {
            // Tokens a macro's own body wrote share its call's position.
            if pair[1] == pair[0] {
                return token.to_string();
            }
            if pair[1] < pair[0] {
                return written(pair[0]);
            }
            chars[pair[0]..pair[1]]
                .iter()
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    // Rust's keywords, which an `(` after them is spaced from.
    const KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "for",
        "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "ref", "return", "self",
        "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
        "async", "await", "dyn",
    ];
    let word = |piece: &str| {
        piece
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '"' | '\''))
    };
    let opens = |token: &Token| {
        matches!(
            token,
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace
        )
    };
    let closes = |token: &Token| {
        matches!(
            token,
            Token::RightParen | Token::RightBracket | Token::RightBrace
        )
    };
    let mut out = String::new();
    for (index, (token, _)) in tokens.iter().enumerate() {
        let piece = if matches!(token, Token::Unit) {
            "()"
        } else {
            pieces[index].as_str()
        };
        out.push_str(piece);
        let Some((next, _)) = tokens.get(index + 1) else {
            break;
        };
        let next_piece = pieces[index + 1].as_str();
        // Joint in the source: no space.
        let spaced = chars
            .get(starts[index + 1].wrapping_sub(1))
            .is_some_and(|c| c.is_whitespace());
        if !spaced {
            continue;
        }
        // `{ b }` keeps its spaces inside; `(x)` and `[0]` have none.
        if matches!(token, Token::LeftBrace) || matches!(next, Token::RightBrace) {
            out.push(' ');
            continue;
        }
        if opens(token) || closes(next) {
            continue;
        }
        // A group, a word or a literal is not punctuation.
        let punct = |token: &Token, piece: &str| {
            !word(piece) && !opens(token) && !closes(token) && !matches!(token, Token::Unit)
        };
        let (left, right) = (punct(token, piece), punct(next, next_piece));
        let space = match () {
            // `x.y`, `tup.0`
            () if piece == "." && !right => false,
            // `foo,`, `x = 3;`, `x.y`
            () if !left && matches!(next_piece, "," | ";" | ".") => false,
            // `f(3)`, `fn(x: u8)`, `Self()`, `pub(crate)`
            () if word(piece)
                && !piece.starts_with(['"', '\''])
                && (!KEYWORDS.contains(&piece) || matches!(piece, "fn" | "Self" | "pub"))
                && matches!(next, Token::LeftParen | Token::Unit) =>
            {
                false
            }
            // `#[attr]`
            () if piece == "#" && matches!(next, Token::LeftBracket) => false,
            () => true,
        };
        if space {
            out.push(' ');
        }
    }
    Some(out)
}

/// Macro arguments split at their top-level commas.
pub(super) fn split_arguments(tokens: Vec<(Token, Position)>) -> Vec<Vec<(Token, Position)>> {
    let mut parts = vec![Vec::new()];
    let mut depth = 0usize;
    for (token, position) in tokens {
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth = depth.saturating_sub(1);
            }
            Token::Comma if depth == 0 => {
                parts.push(Vec::new());
                continue;
            }
            _ => {}
        }
        parts
            .last_mut()
            .expect("there is always a part")
            .push((token, position));
    }
    if parts.len() > 1 && parts.last().is_some_and(Vec::is_empty) {
        parts.pop();
    }
    parts
}

/// An expression's tokens as Rust's `stringify!` prints them: binary
/// operators spaced, nothing around `.`, `::` and brackets.
pub(super) fn token_text(tokens: &[(Token, Position)]) -> String {
    let mut text = String::new();
    let mut previous: Option<&Token> = None;
    let mut unary = false;
    for (token, _) in tokens {
        let word = match token {
            Token::StringConstant(value) => format!("{:?}", value.as_str()),
            Token::CharConstant(value) => format!("{value:?}"),
            token => token.to_string(),
        };
        let operand_before = previous.is_some_and(ends_operand);
        let tight_before = matches!(
            token,
            Token::Period
                | Token::Comma
                | Token::RightParen
                | Token::RightBracket
                | Token::SemiColon
                | Token::DoubleColon
                | Token::Unit
        ) || matches!(token, Token::Reserved(word) if word.as_str() == "?")
            || matches!(token, Token::LeftParen | Token::LeftBracket | Token::Bang)
                && operand_before
            || matches!(token, Token::LeftParen | Token::LeftBracket)
                && matches!(previous, Some(Token::Bang));
        let tight_after_previous = unary
            || matches!(
                previous,
                Some(Token::Period | Token::LeftParen | Token::LeftBracket | Token::DoubleColon)
            );
        if previous.is_some() && !tight_before && !tight_after_previous {
            text.push(' ');
        }
        // `-x`, `!x`, `&x`, `*x`: a prefix operator where no operand ends.
        unary = matches!(
            token,
            Token::UnaryMinus | Token::Minus | Token::Bang | Token::Ampersand | Token::Multiply
        ) && !operand_before;
        text.push_str(&word);
        previous = Some(token);
    }
    text
}

/// `ty` under all its references.
pub(super) fn without_references(mut ty: &Ty) -> &Ty {
    while let Ty::Reference { target, .. } = ty {
        ty = target;
    }
    ty
}

/// Whether an operand can end at `token`, so an operator after it is binary.
pub(super) fn ends_operand(token: &Token) -> bool {
    match token {
        Token::Identifier(_)
        | Token::IntegerConstant(_)
        | Token::StringConstant(_)
        | Token::ByteStringConstant(_)
        | Token::ByteConstant(_)
        | Token::CharConstant(_)
        | Token::True
        | Token::False
        | Token::Unit
        | Token::RightParen
        | Token::RightBracket
        | Token::RightBrace
        | Token::SelfValue => true,
        Token::FloatConstant(_) => true,
        Token::Reserved(word) => word.as_str() == "?",
        _ => false,
    }
}

pub(super) fn assignment_token(token: &Token) -> bool {
    *token == Token::Equals || compound_operator(token).is_some()
}

pub(super) fn point_span(position: Position) -> Span {
    span(position, position)
}

/// Whether the evaluator's standard library has a function `name`.
pub(super) fn standard_function(name: &str) -> bool {
    use crate::packages::Package as _;
    static NAMES: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            crate::packages::StandardPackage::new()
                .as_shared_module()
                .iter_fn()
                .map(|(_, metadata)| metadata.name.to_string())
                .collect()
        })
        .contains(name)
}

/// What an expression ends, which decides the tokens rustc lists as
/// expected after it.
#[derive(Clone, Copy)]
pub(super) enum Continuation {
    /// A path, which `!`, `::` or `{` may continue.
    Path,
    /// A statement, which the block's `}` may end.
    Statement,
    /// A `let`'s value, which `else` may continue.
    Let,
}

pub(super) fn text_end(mut position: Position, text: &str) -> Position {
    for _ in text.chars() {
        position.advance();
    }
    position
}

/// The source from `start` to `end`. An expression a macro's expansion
/// makes of an argument and the macro's own tokens, which sit where the
/// macro is called, may end before it starts, as `$lvl <=
/// $crate::level_filters::LevelFilter::current()` of `level_enabled!`
/// does: it covers the call from the one to the other, where rustc reports
/// an error in it (#1603).
pub(super) fn span(start: Position, end: Position) -> Span {
    if end < start {
        return Span::new(end, start);
    }
    Span::new(start, end)
}

/// The index of a free function's `&mut` parameter, which is its receiver
/// `this` at runtime: the function changes the caller's place in place,
/// and a call `f(&mut v, x)` is the method call `v.f(x)`.
/// `source` lexed as checked Rawr reads Rust, through its EOF marker.
pub(crate) fn lex_rust(engine: &Engine, source: &str) -> Vec<(Token, Position)> {
    let inputs = [source];
    let (mut stream, control) = engine.lex_raw(&inputs, None);
    control.borrow_mut().rust = true;
    let mut tokens = Vec::new();
    loop {
        let token = stream
            .next()
            .expect("token streams always produce their EOF marker");
        let is_eof = token.0 == Token::EOF;
        let token = match token {
            // Rust has no power operator: binary `**` is `*` then a
            // deref (`a * *b`), and `**=` is `*` then `*=`. The untyped
            // script lexer keeps `**`.
            (Token::PowerOf | Token::PowerOfAssign, position) => {
                let second = if token.0 == Token::PowerOf {
                    Token::Multiply
                } else {
                    Token::MultiplyAssign
                };
                let mut next = position;
                next.advance();
                tokens.push((Token::Multiply, position));
                (second, next)
            }
            token => token,
        };
        tokens.push(token);
        if is_eof {
            break;
        }
    }
    tokens
}
