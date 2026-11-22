//! A `&mut` chosen by a branch (T122): `let r = if c { &mut a } else {
//! &mut b };`, or `r = match k { .. }` of a `&mut` local given new borrows.
//! Only the run decides which place `r` refers to, so `r` is a `&mut`
//! local given new borrows (`reborrow.rs`): each tail of the value gives
//! where its place is, and each use of `r` reaches that place itself.
//!
//! Which expressions are the value's tails is syntax, as in Rust: the
//! value itself, each branch's block's last expression, each arm's body,
//! and the tails of those. A token scan finds where each starts before
//! the value is read, so a `&mut` written anywhere else in it (an
//! argument's block, a discarded statement) stays what it was.

use super::*;

/// The tails of a block-like value that may choose a `&mut`, while the
/// value is read.
pub(super) struct Chosen {
    /// Where each tail starts.
    tails: BTreeSet<Position>,
    /// How many closures were open where the value starts: a tail inside
    /// a closure of the value is the closure's.
    closures: usize,
    /// How many tails gave where their place is.
    held: usize,
    /// The tails that are `&mut place` and have not given where their
    /// place is: one whose place no runtime local can hold stays here.
    borrowed: BTreeMap<Position, Span>,
}

impl Chosen {
    /// What the value chose, once it is read: `Ok(true)` when its tails
    /// gave where their places are, so the value is where its place is.
    /// A `&mut place` tail that gave no place is an error at its span.
    pub(super) fn chose(&self) -> Result<bool, Span> {
        match self.borrowed.values().next() {
            Some(refused) => Err(*refused),
            None => Ok(self.held > 0),
        }
    }
}

impl Parser<'_> {
    /// The tails of the block-like value ahead, for a `&mut` it may
    /// choose. `None` where the value can't be one: in an async body,
    /// whose locals move between scopes, or in a function whose `&mut`
    /// output gives paths of its own (`returned_place`).
    pub(super) fn choose_ahead(&self) -> Option<Chosen> {
        if self.in_async_body() || self.place_output {
            return None;
        }
        let tokens: Vec<(Token, Position)> = self.input.clone().collect();
        let mut tails = BTreeSet::new();
        // The value is the whole expression: `if c { &mut a } else { &mut
        // b }.len()` is a call on the chosen place, not one.
        let end = expression_tails(&tokens, 0, &mut tails)?;
        if !matches!(tokens.get(end), Some((Token::SemiColon, _))) {
            return None;
        }
        Some(Chosen {
            tails,
            closures: self.closures.len(),
            held: 0,
            borrowed: BTreeMap::new(),
        })
    }

    /// Whether `&mut` written at `at` is a tail of the value being read
    /// for a `&mut` it may choose.
    fn chosen_tail(&self, at: Position) -> bool {
        self.chosen.as_ref().is_some_and(|chosen| {
            chosen.closures == self.closures.len() && chosen.tails.contains(&at)
        })
    }

    /// Whether `&mut` written at `at` is a tail of the value being read
    /// for a `&mut` it may choose: it must then give where its place is.
    pub(super) fn borrow_chosen(&mut self, at: Position) -> bool {
        if !self.chosen_tail(at) {
            return false;
        }
        if let Some(chosen) = self.chosen.as_mut() {
            chosen.borrowed.insert(at, span(at, at));
        }
        true
    }

    /// `value`, a tail of a block-like value: where its place is, when it
    /// is one of the value's tails that may choose a `&mut`, of a `&mut`
    /// type. A block-like tail gave that at its own tails.
    pub(super) fn chosen_place(&mut self, mut value: ParsedExpr, block_like: bool) -> ParsedExpr {
        if !self.chosen_tail(value.span.start())
            || !matches!(
                self.inference.resolve(&value.ty),
                Ty::Reference { mutable: true, .. }
            )
            || block_like && value.lvalue.is_none()
        {
            return value;
        }
        let source = value
            .prelude
            .is_empty()
            .then(|| self.reborrow_source(&value))
            .flatten();
        let Some(chosen) = self.chosen.as_mut() else {
            return value;
        };
        match source {
            Some((source, _)) => {
                chosen.held += 1;
                chosen.borrowed.remove(&value.span.start());
                value.source = origin::Fragment::plain(source);
                value.lvalue = None;
            }
            None => {
                if let Some(refused) = chosen.borrowed.get_mut(&value.span.start()) {
                    *refused = value.span;
                }
            }
        }
        value
    }
}

/// Whether `token` opens a group that a scan steps over whole.
fn opens(token: &Token) -> bool {
    matches!(
        token,
        Token::LeftBrace | Token::MapStart | Token::LeftParen | Token::LeftBracket
    )
}

/// Whether `token` closes a group.
fn closes(token: &Token) -> bool {
    matches!(
        token,
        Token::RightBrace | Token::RightParen | Token::RightBracket
    )
}

/// Whether `token` is the keyword `match`.
fn is_match(token: &Token) -> bool {
    matches!(token, Token::Reserved(keyword) if keyword.as_str() == "match")
}

/// The index of the first `{` at the top level from `at`: the block of an
/// `if` or the arms of a `match`, after its condition or scrutinee.
fn block_start(tokens: &[(Token, Position)], mut at: usize) -> Option<usize> {
    let mut depth = 0usize;
    while let Some((token, _)) = tokens.get(at) {
        match token {
            Token::LeftBrace if depth == 0 => return Some(at),
            Token::SemiColon | Token::EOF if depth == 0 => return None,
            token if opens(token) => depth += 1,
            token if closes(token) => depth = depth.checked_sub(1)?,
            _ => {}
        }
        at += 1;
    }
    None
}

/// The index just past the group opened at `at`.
fn group_end(tokens: &[(Token, Position)], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, (token, _)) in tokens.iter().enumerate().skip(at) {
        if opens(token) {
            depth += 1;
        } else if closes(token) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index + 1);
            }
        } else if matches!(token, Token::EOF) {
            return None;
        }
    }
    None
}

/// The tails of the expression starting at `at`, a block-like one (`if`,
/// `match` or a block), into `tails`: where each starts. The index just
/// past the expression, or `None` for one the scan doesn't read.
fn expression_tails(
    tokens: &[(Token, Position)],
    at: usize,
    tails: &mut BTreeSet<Position>,
) -> Option<usize> {
    let (token, _) = tokens.get(at)?;
    match token {
        Token::LeftBrace => block_tails(tokens, at, tails),
        Token::If => {
            let block = block_start(tokens, at + 1)?;
            let end = block_tails(tokens, block, tails)?;
            match tokens.get(end) {
                Some((Token::Else, _)) => match tokens.get(end + 1) {
                    Some((Token::If, _)) => expression_tails(tokens, end + 1, tails),
                    Some((Token::LeftBrace, _)) => block_tails(tokens, end + 1, tails),
                    _ => None,
                },
                _ => Some(end),
            }
        }
        token if is_match(token) => {
            let arms = block_start(tokens, at + 1)?;
            arm_tails(tokens, arms, tails)
        }
        _ => None,
    }
}

/// The tail of the block opened at `at` and that tail's own tails. The
/// index just past the block.
fn block_tails(
    tokens: &[(Token, Position)],
    at: usize,
    tails: &mut BTreeSet<Position>,
) -> Option<usize> {
    let mut statement = at + 1;
    let mut index = at + 1;
    loop {
        let (token, _) = tokens.get(index)?;
        match token {
            Token::RightBrace => {
                if statement < index {
                    statement_tail(tokens, statement, index, tails);
                }
                return Some(index + 1);
            }
            Token::SemiColon => {
                index += 1;
                statement = index;
            }
            Token::EOF => return None,
            token if opens(token) => {
                index = group_end(tokens, index)?;
                // A statement that is a block-like expression ends with
                // its block, unless an `else` or a method chain goes on.
                let block_like = matches!(
                    tokens.get(statement).map(|(token, _)| token),
                    Some(Token::If | Token::While | Token::Loop | Token::For | Token::LeftBrace)
                ) || tokens
                    .get(statement)
                    .is_some_and(|(token, _)| is_match(token));
                let goes_on = matches!(
                    tokens.get(index).map(|(token, _)| token),
                    Some(Token::Else | Token::Period | Token::RightBrace)
                ) || tokens
                    .get(index)
                    .is_some_and(|(token, _)| matches!(token, Token::Reserved(keyword) if keyword.as_str() == "?"));
                if block_like && matches!(token, Token::LeftBrace) && !goes_on {
                    statement = index;
                }
            }
            _ => index += 1,
        }
    }
}

/// The expression from `at` to `end` is a tail: where it starts, and its
/// own tails when it is a block-like expression and nothing more.
fn statement_tail(
    tokens: &[(Token, Position)],
    at: usize,
    end: usize,
    tails: &mut BTreeSet<Position>,
) {
    let Some((token, position)) = tokens.get(at) else {
        return;
    };
    tails.insert(*position);
    if matches!(token, Token::If | Token::LeftBrace) || is_match(token) {
        let mut inner = BTreeSet::new();
        if expression_tails(tokens, at, &mut inner) == Some(end) {
            tails.extend(inner);
        }
    }
}

/// Each arm body of the `match` whose arms open at `at`, and its tails.
/// The index just past the arms.
fn arm_tails(
    tokens: &[(Token, Position)],
    at: usize,
    tails: &mut BTreeSet<Position>,
) -> Option<usize> {
    let mut index = at + 1;
    loop {
        // The pattern and its guard, to the `=>`.
        loop {
            let (token, _) = tokens.get(index)?;
            match token {
                Token::RightBrace => return Some(index + 1),
                Token::DoubleArrow => break,
                Token::EOF => return None,
                token if opens(token) => index = group_end(tokens, index)?,
                _ => index += 1,
            }
        }
        index += 1;
        let body = index;
        // The body: a block, or an expression to the `,` or the arms' end.
        let mut depth = 0usize;
        loop {
            let (token, _) = tokens.get(index)?;
            match token {
                Token::Comma if depth == 0 => break,
                Token::RightBrace if depth == 0 => break,
                Token::EOF => return None,
                Token::LeftBrace if depth == 0 && index == body => {
                    index = group_end(tokens, index)?;
                    if !matches!(
                        tokens.get(index).map(|(token, _)| token),
                        Some(Token::Period | Token::Comma | Token::RightBrace)
                    ) {
                        break;
                    }
                    continue;
                }
                token if opens(token) => depth += 1,
                token if closes(token) => depth = depth.checked_sub(1)?,
                _ => {}
            }
            index += 1;
        }
        statement_tail(tokens, body, index, tails);
        if matches!(tokens.get(index), Some((Token::Comma, _))) {
            index += 1;
        }
    }
}
