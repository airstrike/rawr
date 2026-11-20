//! Destructuring assignment: `(a, b) = (b, a)`, `[x, .., y] = array`,
//! `S { f, .. } = s`, `T(x, _) = t` and `_ = value`, nested in any way.
//!
//! As rustc lowers it, the assignee becomes a pattern in which a fresh
//! binding stands for each place it names. A `let` binds that pattern to
//! the value, and each place is then assigned its binding in written
//! order: `(a, b) = (b, a)` reads as `{ let (t0, t1) = (b, a); a = t0; b =
//! t1; }`. The value is evaluated first and the places after it, left to
//! right, and each of those statements is checked as any `let` and any
//! assignment is. The tokens are rewritten before the expression is
//! parsed, so the lowering reads only ordinary statements.

use super::*;

/// One element of an assignee, as its tokens' range.
type Range = std::ops::Range<usize>;

/// A destructuring assignment's pattern and the places it assigns.
#[derive(Default)]
struct Assignee {
    /// The pattern's tokens, a fresh binding where each place is.
    pattern: Vec<(Token, Position)>,
    /// Each place's tokens with the binding standing for it, in written
    /// order.
    places: Vec<(Range, Token)>,
}

impl Parser<'_> {
    /// Rewrite a destructuring assignment ahead into its block. Any other
    /// expression is left as it is, an assignment to a single place among
    /// them.
    pub(super) fn splice_destructuring(&mut self) -> Result<(), Error> {
        if self.split_angle.is_some() || !assignee_start(&self.peek()) {
            return Ok(());
        }
        let Some(tokens) = self.assignment_ahead() else {
            return Ok(());
        };
        let Some(equals) = tokens.iter().position(|(token, _)| *token == Token::Equals) else {
            return Ok(());
        };
        let mut assignee = Assignee::default();
        if !self.destructures(&tokens, 0..equals, &mut assignee) {
            return Ok(());
        }
        let (start, at) = (tokens[0].1, tokens[equals].1);
        let end = tokens.last().map_or(at, |(_, position)| *position);
        self.destructuring.insert(start);
        let mut block = vec![(Token::LeftBrace, start), (Token::Let, start)];
        block.append(&mut assignee.pattern);
        block.push((Token::Equals, at));
        block.extend_from_slice(&tokens[equals + 1..]);
        block.push((Token::SemiColon, end));
        for (place, binding) in assignee.places {
            // The binding is written where the token after its place is,
            // which no expression is parsed at: the borrow check knows
            // each use by where it starts.
            let written = tokens[place.end].1;
            block.extend_from_slice(&tokens[place]);
            block.push((Token::Equals, at));
            block.push((binding, written));
            block.push((Token::SemiColon, written));
        }
        block.push((Token::RightBrace, end));
        // Taken unread: the calls among them are noted once, as the block
        // is parsed.
        for _ in 0..tokens.len() {
            self.input.next();
        }
        self.splice_tokens(block);
        Ok(())
    }

    /// The tokens of an assignment ahead whose assignee is one operand
    /// (a group, a path with or without a group after it, `_`), from the
    /// assignee to the value's end; `None` when no `=` follows the operand.
    fn assignment_ahead(&self) -> Option<Vec<(Token, Position)>> {
        let mut ahead = self.input.clone();
        let mut tokens = Vec::new();
        let (first, position) = ahead.next()?;
        let grouped = matches!(first, Token::LeftParen | Token::LeftBracket);
        let pathed = path_token(&first);
        tokens.push((first, position));
        if grouped {
            take_group(&mut ahead, &mut tokens)?;
        } else if pathed {
            while let (Token::DoubleColon, _) = ahead.peek()? {
                tokens.push(ahead.next()?);
                match ahead.peek()? {
                    (Token::LessThan, _) => take_angles(&mut ahead, &mut tokens)?,
                    (token, _) if path_token(token) => tokens.push(ahead.next()?),
                    _ => return None,
                }
            }
            match ahead.peek()? {
                (Token::LeftParen, _) => {
                    tokens.push(ahead.next()?);
                    take_group(&mut ahead, &mut tokens)?;
                }
                (Token::LeftBrace, _) if !self.no_struct => {
                    tokens.push(ahead.next()?);
                    take_group(&mut ahead, &mut tokens)?;
                }
                (Token::Unit, _) => tokens.push(ahead.next()?),
                _ => return None,
            }
        }
        if !matches!(ahead.peek()?, (Token::Equals, _)) {
            return None;
        }
        tokens.push(ahead.next()?);
        // The value goes on to the end of the expression: a `;`, a `,` or
        // the group around it closing.
        let mut depth = 0usize;
        loop {
            match ahead.peek()? {
                (Token::SemiColon | Token::Comma | Token::EOF, _) if depth == 0 => break,
                (Token::RightParen | Token::RightBracket | Token::RightBrace, _) if depth == 0 => {
                    break
                }
                (Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart, _) => {
                    depth += 1;
                }
                (Token::RightParen | Token::RightBracket | Token::RightBrace, _) => depth -= 1,
                (Token::DoubleColon, _) => {
                    tokens.push(ahead.next()?);
                    if matches!(ahead.peek()?, (Token::LessThan, _)) {
                        take_angles(&mut ahead, &mut tokens)?;
                    }
                    continue;
                }
                _ => {}
            }
            tokens.push(ahead.next()?);
        }
        (tokens.len() > 1 && !matches!(tokens.last(), Some((Token::Equals, _)))).then_some(tokens)
    }

    /// Whether the assignee `tokens[range]` destructures, as rustc tells
    /// it apart from a place: a tuple, an array, a struct, a tuple struct
    /// or variant's constructor, `_`, or one of them in parentheses. Its
    /// pattern and places go into `assignee`.
    fn destructures(
        &mut self,
        tokens: &[(Token, Position)],
        range: Range,
        assignee: &mut Assignee,
    ) -> bool {
        let element = &tokens[range.clone()];
        match element {
            [(Token::Underscore, position)] => {
                assignee.pattern.push((Token::Underscore, *position));
                true
            }
            [(Token::Unit, position)] => {
                assignee.pattern.push((Token::Unit, *position));
                true
            }
            [(Token::LeftParen, open), .., (Token::RightParen, close)] => {
                let inner = range.start + 1..range.end - 1;
                let Some((members, comma)) = members(tokens, inner.clone()) else {
                    return false;
                };
                // `(a,)` and `(..)` are tuples, as in a pattern; `(x)` is `x`.
                let tuple = comma || matches!(&tokens[inner.clone()], [(Token::ExclusiveRange, _)]);
                if !tuple {
                    return self.destructures(tokens, inner, assignee);
                }
                assignee.pattern.push((Token::LeftParen, *open));
                self.elements(tokens, members, assignee);
                assignee.pattern.push((Token::RightParen, *close));
                true
            }
            [(Token::LeftBracket, open), .., (Token::RightBracket, close)] => {
                let Some((members, _)) = members(tokens, range.start + 1..range.end - 1) else {
                    return false;
                };
                assignee.pattern.push((Token::LeftBracket, *open));
                self.elements(tokens, members, assignee);
                assignee.pattern.push((Token::RightBracket, *close));
                true
            }
            [.., (Token::RightBrace, close)] if path_token(&element[0].0) => {
                let open = path_length(element);
                if element[open].0 != Token::LeftBrace {
                    return false;
                }
                let Some((fields, _)) = members(tokens, range.start + open + 1..range.end - 1)
                else {
                    return false;
                };
                assignee.pattern.extend_from_slice(&element[..=open]);
                for field in fields {
                    let comma = tokens[field.end - 1].1;
                    let rest = matches!(&tokens[field.clone()], [(Token::ExclusiveRange, _)]);
                    self.field(tokens, field, assignee);
                    if !rest {
                        assignee.pattern.push((Token::Comma, comma));
                    }
                }
                assignee.pattern.push((Token::RightBrace, *close));
                true
            }
            [.., (Token::RightParen | Token::Unit, _)] if path_token(&element[0].0) => {
                let open = path_length(element);
                if !matches!(element[open].0, Token::LeftParen | Token::Unit)
                    || !self.constructs(&element[..open])
                {
                    return false;
                }
                assignee.pattern.extend_from_slice(&element[..open]);
                if element[open].0 == Token::Unit {
                    assignee.pattern.push(element[open].clone());
                    return true;
                }
                assignee.pattern.push(element[open].clone());
                let Some((members, _)) = members(tokens, range.start + open + 1..range.end - 1)
                else {
                    return false;
                };
                self.elements(tokens, members, assignee);
                assignee.pattern.push(element[element.len() - 1].clone());
                true
            }
            _ => false,
        }
    }

    /// The members of a tuple, an array or a tuple struct in an assignee,
    /// each followed by a `,`: `..` as itself, a unit struct or unit
    /// variant as its path, and any other place as a fresh binding.
    fn elements(
        &mut self,
        tokens: &[(Token, Position)],
        members: Vec<Range>,
        assignee: &mut Assignee,
    ) {
        for member in members {
            let comma = tokens[member.end - 1].1;
            self.element(tokens, member, assignee);
            assignee.pattern.push((Token::Comma, comma));
        }
    }

    /// One member of an assignee: a nested assignee that destructures, or
    /// else a place.
    fn element(&mut self, tokens: &[(Token, Position)], range: Range, assignee: &mut Assignee) {
        let element = &tokens[range.clone()];
        if let [(Token::ExclusiveRange, _)] = element {
            assignee.pattern.extend_from_slice(element);
            return;
        }
        if self.unit_constructor(element) {
            assignee.pattern.extend_from_slice(element);
            return;
        }
        let pattern = assignee.pattern.len();
        let places = assignee.places.len();
        if self.destructures(tokens, range.clone(), assignee) {
            return;
        }
        assignee.pattern.truncate(pattern);
        assignee.places.truncate(places);
        self.place(tokens, range, assignee);
    }

    /// One field of a struct in an assignee: `name` assigns the local of
    /// that name, `name: assignee` its own assignee, and `..` stays.
    fn field(&mut self, tokens: &[(Token, Position)], range: Range, assignee: &mut Assignee) {
        match &tokens[range.clone()] {
            [(Token::ExclusiveRange, _)] => assignee.pattern.extend_from_slice(&tokens[range]),
            [name @ (_, position)] => {
                assignee.pattern.push(name.clone());
                assignee.pattern.push((Token::Colon, *position));
                self.place(tokens, range, assignee);
            }
            [name, colon @ (Token::Colon, _), ..] => {
                assignee.pattern.push(name.clone());
                assignee.pattern.push(colon.clone());
                self.element(tokens, range.start + 2..range.end, assignee);
            }
            // Not a field, which the pattern refuses as rustc does.
            written => assignee.pattern.extend_from_slice(written),
        }
    }

    /// The place `tokens[range]`: a fresh binding in the pattern, assigned
    /// to it after the `let`. The binding is written where the token after
    /// the place is.
    fn place(&mut self, tokens: &[(Token, Position)], range: Range, assignee: &mut Assignee) {
        let binding = Token::Identifier(Box::new(self.temporary(ROLE).as_str().into()));
        assignee
            .pattern
            .push((binding.clone(), tokens[range.end].1));
        assignee.places.push((range, binding));
    }

    /// Whether `path` names a tuple struct or a tuple variant, whose
    /// constructor an assignee destructures; a call of anything else is a
    /// value, which rustc refuses to assign to (E0070).
    fn constructs(&self, path: &[(Token, Position)]) -> bool {
        let names = path_names(path);
        match names.as_slice() {
            [name] if self.bindings.contains_key(*name) => false,
            [name] => {
                pattern::Prelude::from_name(name).is_some()
                    || matches!(
                        self.named_model(name),
                        Some(Nominal::Struct { item, .. })
                            if item.form == crate::item::StructForm::Tuple
                    )
            }
            [owner, variant] => self.variant_of(owner, variant).is_some_and(|unit| !unit),
            _ => false,
        }
    }

    /// Whether `element` is a unit struct or a unit variant's path, which
    /// an assignee matches rather than assigns.
    fn unit_constructor(&self, element: &[(Token, Position)]) -> bool {
        if !element
            .iter()
            .all(|(token, _)| path_token(token) || *token == Token::DoubleColon)
        {
            return false;
        }
        match path_names(element).as_slice() {
            [name] if self.bindings.contains_key(*name) => false,
            [name] => *name == "None" || self.unit_struct(name).is_some(),
            [owner, variant] => self.variant_of(owner, variant).is_some_and(|unit| unit),
            _ => false,
        }
    }

    /// Whether `variant` of the enum `owner` (or std's `Option` and
    /// `Result`) carries nothing; `None` when `owner` has no such variant.
    fn variant_of(&self, owner: &str, variant: &str) -> Option<bool> {
        if let Some(Nominal::Enum { item, .. }) = self.named_model(owner) {
            return item
                .variants
                .iter()
                .find(|declared| declared.name.text.as_str() == variant)
                .map(|declared| declared.payload.is_empty() && declared.fields.is_none());
        }
        pattern::Prelude::from_name(variant)
            .filter(|prelude| prelude.owner() == owner && self.models.get(owner).is_none())
            .map(|prelude| prelude == pattern::Prelude::None)
    }
}

/// The role a destructuring assignment's bindings are named for.
const ROLE: &str = "assignee";

/// The span of the assignment of `replacement` to the place at `place`:
/// the place alone when `replacement` is a destructuring assignment's
/// binding, as rustc's desugaring spans that assignment, and both
/// otherwise.
pub(super) fn assignment_span(place: Span, replacement: &ParsedExpr) -> Span {
    let binding = replacement
        .source
        .as_str()
        .strip_prefix("__rawr_")
        .and_then(|name| name.strip_prefix(ROLE))
        .is_some_and(|number| number.starts_with('_'));
    if binding {
        place
    } else {
        span(place.start(), replacement.span.end())
    }
}

/// Whether `token` may start an assignee that destructures.
fn assignee_start(token: &Token) -> bool {
    matches!(
        token,
        Token::LeftParen | Token::LeftBracket | Token::Underscore | Token::Unit
    ) || path_token(token)
}

/// Whether `token` is a segment of a path.
fn path_token(token: &Token) -> bool {
    match token {
        Token::Identifier(_) => true,
        Token::Reserved(name) => matches!(name.as_str(), "Self" | "crate" | "super"),
        _ => false,
    }
}

/// The names of the path `tokens`, its generic arguments left out.
fn path_names(tokens: &[(Token, Position)]) -> Vec<&str> {
    tokens
        .iter()
        .filter_map(|(token, _)| match token {
            Token::Identifier(name) | Token::Reserved(name) => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

/// The rest of a group whose opening token was taken, up to its closing
/// token, into `tokens`.
fn take_group(
    ahead: &mut Peekable<impl Iterator<Item = (Token, Position)>>,
    tokens: &mut Vec<(Token, Position)>,
) -> Option<()> {
    let mut depth = 1usize;
    while depth > 0 {
        let (token, position) = ahead.next()?;
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => depth -= 1,
            Token::EOF => return None,
            _ => {}
        }
        tokens.push((token, position));
    }
    Some(())
}

/// Generic arguments `<..>` after a `::`, into `tokens`.
fn take_angles(
    ahead: &mut Peekable<impl Iterator<Item = (Token, Position)>>,
    tokens: &mut Vec<(Token, Position)>,
) -> Option<()> {
    let mut depth = 0usize;
    loop {
        let (token, position) = ahead.next()?;
        match token {
            Token::LessThan => depth += 1,
            Token::GreaterThan => depth = depth.checked_sub(1)?,
            Token::RightShift => depth = depth.checked_sub(2)?,
            Token::SemiColon | Token::EOF => return None,
            _ => {}
        }
        tokens.push((token, position));
        if depth == 0 {
            return Some(());
        }
    }
}

/// The members of the group inside `range`, split at its own commas, and
/// whether it has one: `(a,)` is a tuple and `(a)` is not. A trailing
/// comma ends the last member. `None` for an empty member before a comma,
/// which the ordinary parse refuses.
fn members(tokens: &[(Token, Position)], range: Range) -> Option<(Vec<Range>, bool)> {
    let mut members = Vec::new();
    let mut depth = 0usize;
    let mut start = range.start;
    let mut comma = false;
    for index in range.clone() {
        match tokens[index].0 {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => depth -= 1,
            Token::Comma if depth == 0 => {
                if start == index {
                    return None;
                }
                members.push(start..index);
                start = index + 1;
                comma = true;
            }
            _ => {}
        }
    }
    if start < range.end {
        members.push(start..range.end);
    }
    Some((members, comma))
}

/// How many of `element`'s tokens are its leading path: names, `::` and
/// generic arguments.
fn path_length(element: &[(Token, Position)]) -> usize {
    let mut depth = 0usize;
    element
        .iter()
        .take_while(|(token, _)| {
            match token {
                Token::LessThan => depth += 1,
                Token::GreaterThan => depth = depth.saturating_sub(1),
                Token::RightShift => depth = depth.saturating_sub(2),
                _ if depth > 0 => {}
                token => return path_token(token) || *token == Token::DoubleColon,
            }
            true
        })
        .count()
}
