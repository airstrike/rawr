//! A dependency's `macro_rules!` macro called from a script: its rules
//! (`crate::resolve::macro_rules`) expand the call's tokens, which are
//! parsed in its place, as rustc parses an expansion.

use super::*;
use crate::resolve::macro_rules;

/// The most expansions one function's parse makes, as rustc's recursion
/// limit stops a macro that never stops calling itself.
const EXPANSION_LIMIT: usize = 4096;

impl Parser<'_> {
    /// `name!(..)`, `name![..]` or `name! {..}` of the `macro_rules!` macro
    /// `rules`, from its `!`: the expansion, parsed as one expression.
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
        let name: ImmutableString = parsed.name().into();
        self.expect(Token::Bang, "'!' after a macro name")?;
        let (open, position) = self.next();
        let close = match open {
            Token::Unit => None,
            Token::LeftParen => Some(Token::RightParen),
            Token::LeftBracket => Some(Token::RightBracket),
            Token::LeftBrace => Some(Token::RightBrace),
            _ => {
                return Err(Error::InvalidHostOperation {
                    item: name,
                    reason: "write the macro's arguments in `(..)`, `[..]` or `{..}`".into(),
                    operation: span(name_span.start(), position),
                });
            }
        };
        let mut tokens = Vec::new();
        let mut end = position;
        if let Some(close) = close {
            let mut depth = 0_usize;
            loop {
                let (token, position) = self.next();
                end = position;
                match &token {
                    Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                        depth += 1;
                    }
                    token if depth == 0 && *token == close => break,
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
        let call = span(name_span.start(), end);
        self.expansions += 1;
        if self.expansions > EXPANSION_LIMIT {
            return Err(Error::InvalidHostOperation {
                item: name,
                reason: format!(
                    "expanding it reached the limit of {EXPANSION_LIMIT} macro expansions"
                )
                .into(),
                operation: call,
            });
        }
        let input = macro_rules::trees(&tokens).map_err(|reason| Error::InvalidHostOperation {
            item: name.clone(),
            reason,
            operation: call,
        })?;
        self.temporaries += 1;
        let expansion = parsed
            .expand(&input, rules.krate(), name_span.start(), self.temporaries)
            .map_err(|reason| Error::InvalidHostOperation {
                item: name.clone(),
                reason,
                operation: call,
            })?;
        // One expression: parenthesised, or a block when the expansion is
        // statements, as a macro called as a statement may expand to.
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
        if expansion.is_empty() {
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
        self.splice_tokens(spliced);
        self.parse_primary()
    }
}
