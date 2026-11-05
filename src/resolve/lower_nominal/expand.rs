//! A `macro_rules!` macro called from a body, the program's own or a
//! dependency's: its rules (`crate::resolve::macro_rules`) expand the call's
//! tokens, which are parsed in its place, as rustc parses an expansion.

use super::*;
use crate::resolve::macro_rules;

/// The most expansions one function's parse makes, a bound on a macro that
/// keeps expanding to more calls without nesting them.
const EXPANSION_LIMIT: usize = 65_536;

impl Parser<'_> {
    /// `name!(..)`, `name![..]` or `name! {..}` of a dependency's
    /// `macro_rules!` macro `rules`, from its `!`.
    pub(super) fn expand_macro_rules(
        &mut self,
        rules: &crate::host::item::MacroRules,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let parsed = rules
            .rules()
            .map_err(|reason| Error::InvalidHostOperation {
                item: "macro_rules!".into(),
                reason: format!("the macro's definition cannot be read: {reason}").into(),
                operation: name_span,
            })?;
        self.expand(parsed, rules.krate(), name_span)
    }

    /// A call of the program's own macro `rules`, from its `!`.
    pub(super) fn expand_program_macro(
        &mut self,
        rules: &macro_rules::Rules,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.expand(rules, "crate", name_span)
    }

    /// `macro_rules! name { .. }` in a body: the macro is in scope from
    /// here to the end of the block, as Rust's textual scope has it.
    pub(super) fn parse_local_macro(&mut self) -> Result<(), Error> {
        let mut tokens = vec![self.next()];
        let (bang, position) = self.next();
        if bang != Token::Bang {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "`!` after `macro_rules`".into(),
                source: point_span(position),
            });
        }
        tokens.push((bang, position));
        tokens.push(self.next());
        let (open, position) = self.next();
        let braced = matches!(open, Token::LeftBrace | Token::MapStart);
        tokens.push((open.clone(), position));
        if open != Token::Unit {
            let (inner, close) = self.group_rest(&open, position, "macro_rules!")?;
            tokens.extend(inner);
            let close_token = match open {
                Token::LeftParen => Token::RightParen,
                Token::LeftBracket => Token::RightBracket,
                _ => Token::RightBrace,
            };
            tokens.push((close_token, close));
        }
        // `macro_rules! m (..);` and `macro_rules! m [..];` end with `;`.
        if !braced {
            let end = self.expect(Token::SemiColon, "';' after a macro definition")?;
            tokens.push((Token::SemiColon, end));
        }
        let rules =
            macro_rules::Rules::from_tokens(&tokens).map_err(|malformed| Error::Syntax {
                message: malformed.message().into(),
                label: malformed.label().into(),
                help: malformed.help().map(Into::into),
                at: point_span(malformed.at()),
            })?;
        let name = ImmutableString::from(rules.name());
        self.local_macros.push((name, Shared::new(rules)));
        Ok(())
    }

    /// The tokens after `open` through its matching close, which is not
    /// among them, and the close's position.
    fn group_rest(
        &mut self,
        open: &Token,
        position: Position,
        name: &str,
    ) -> Result<(Vec<(Token, Position)>, Position), Error> {
        let close = match open {
            Token::LeftParen => Token::RightParen,
            Token::LeftBracket => Token::RightBracket,
            Token::LeftBrace | Token::MapStart => Token::RightBrace,
            _ => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!("`(`, `[` or `{{` after `{name}!`").into(),
                    source: point_span(position),
                });
            }
        };
        let mut tokens = Vec::new();
        let mut depth = 0_usize;
        loop {
            let (token, position) = self.next();
            match &token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                token if depth == 0 && *token == close => return Ok((tokens, position)),
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::EOF => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!("`{close}` to close the call of `{name}!`").into(),
                        source: point_span(position),
                    });
                }
                _ => {}
            }
            tokens.push((token, position));
        }
    }

    /// The call of `rules` from its `!`: the expansion, parsed as one
    /// expression, with `$crate` reading as `krate`.
    fn expand(
        &mut self,
        rules: &macro_rules::Rules,
        krate: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.splice_expansion(rules, krate, name_span, false)?;
        self.parse_primary()
    }

    /// Whether a call of the program's own macro starts the next statement.
    pub(super) fn macro_statement_ahead(&mut self) -> bool {
        let Token::Identifier(name) = self.peek() else {
            return false;
        };
        matches!(self.peek_second(), Token::Bang) && {
            let at = self.position();
            self.program_macro(&name, at).is_some()
        }
    }

    /// A call of the program's own macro as a statement: its expansion
    /// takes its place among the block's statements, so a `let` of a name
    /// the caller passed in binds it after the call, as in Rust.
    pub(super) fn parse_macro_statement(&mut self) -> Result<(), Error> {
        let (token, position) = self.next();
        let Token::Identifier(name) = token else {
            unreachable!("`macro_statement_ahead` saw a name");
        };
        let name_span = span(position, text_end(position, &name));
        let rules = self
            .program_macro(&name, position)
            .expect("`macro_statement_ahead` found the macro")?;
        self.splice_expansion(&rules, "crate", name_span, true)
    }

    /// Expand the call of `rules` from its `!` and put its tokens next in
    /// the input: one expression, parenthesised or a block, or, for a call
    /// that is a `statement`, the statements it expands to.
    fn splice_expansion(
        &mut self,
        rules: &macro_rules::Rules,
        krate: &str,
        name_span: Span,
        statement: bool,
    ) -> Result<(), Error> {
        let name: ImmutableString = rules.name().into();
        self.expect(Token::Bang, "'!' after a macro name")?;
        let (open, position) = self.next();
        let (tokens, end) = match open {
            Token::Unit => (Vec::new(), position),
            open => self.group_rest(&open, position, &name)?,
        };
        let call = span(name_span.start(), text_end(end, " "));
        // rustc's recursion limit bounds how deep calls nest: the
        // expansions this call is inside are those whose tokens are not all
        // read yet.
        let remaining = self.input.len();
        while self
            .expansion_ends
            .last()
            .is_some_and(|after| *after >= remaining)
        {
            self.expansion_ends.pop();
        }
        self.expansions += 1;
        if self.expansion_ends.len() >= self.models.macros.limit()
            || self.expansions > EXPANSION_LIMIT
        {
            return Err(Error::Syntax {
                message: format!("recursion limit reached while expanding `{name}!`").into(),
                label: ImmutableString::new(),
                help: Some(
                    format!(
                        "consider increasing the recursion limit by adding a \
                         `#![recursion_limit = \"{}\"]` attribute to your crate",
                        self.models.macros.limit() * 2
                    )
                    .into(),
                ),
                at: call,
            });
        }
        let input = macro_rules::trees(&tokens).map_err(|reason| Error::Syntax {
            message: reason,
            label: ImmutableString::new(),
            help: None,
            at: call,
        })?;
        self.temporaries += 1;
        let expansion = rules
            .expand(&input, krate, name_span.start(), end, self.temporaries)
            .map_err(|failure| {
                let (label, at) = match &failure {
                    macro_rules::Failure::NoRules(Some((token, position))) => (
                        "no rules expected this token in macro call",
                        span(*position, text_end(*position, &token.to_string())),
                    ),
                    macro_rules::Failure::NoRules(None) => {
                        ("missing tokens in macro arguments", call)
                    }
                    macro_rules::Failure::Transcription(_) => ("", call),
                };
                Error::Syntax {
                    message: failure.to_string().into(),
                    label: label.into(),
                    help: None,
                    at,
                }
            })?;
        let statements = {
            let mut depth = 0_usize;
            expansion.iter().any(|(token, _)| {
                match token {
                    Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                    Token::RightParen | Token::RightBracket | Token::RightBrace => {
                        depth = depth.saturating_sub(1);
                    }
                    Token::SemiColon => return depth == 0,
                    _ => {}
                }
                false
            })
        };
        let at = name_span.start();
        let mut spliced = Vec::with_capacity(expansion.len() + 2);
        if statement && statements {
            // The call's own `;` ends the last statement when it has none.
            let ended = matches!(
                expansion.last(),
                Some((Token::SemiColon | Token::RightBrace, _))
            );
            spliced.extend(expansion);
            if matches!(self.peek(), Token::SemiColon) {
                let semicolon = self.next();
                if !ended {
                    spliced.push(semicolon);
                }
            }
        } else if expansion.is_empty() {
            spliced.push((Token::Unit, at));
        } else if statements {
            spliced.push((Token::LeftBrace, at));
            spliced.extend(expansion);
            spliced.push((Token::RightBrace, end));
        } else {
            spliced.push((Token::LeftParen, at));
            spliced.extend(expansion);
            spliced.push((Token::RightParen, end));
        }
        self.expansion_ends.push(remaining);
        self.splice_tokens(spliced);
        Ok(())
    }
}
