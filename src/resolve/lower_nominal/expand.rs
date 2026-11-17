//! A `macro_rules!` macro called from a body, the program's own or a
//! dependency's: its rules (`crate::resolve::macro_rules`) expand the call's
//! tokens, which are parsed in its place, as rustc parses an expansion.

use super::*;
use crate::resolve::macro_rules;

/// The input after a crate macro call's `!`, as it is ahead.
#[cfg(feature = "rust-import")]
type CallInput = std::iter::Peekable<std::vec::IntoIter<(Token, Position)>>;

/// The most expansions one function's parse makes, a bound on a macro that
/// keeps expanding to more calls without nesting them.
const EXPANSION_LIMIT: usize = 65_536;

/// Where a macro's call is, which decides what its expansion is parsed as.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// One expression, parenthesised or a block.
    Expression,
    /// The statements it expands to, among the block's.
    Statement,
    /// A type: its tokens as they are.
    Type,
    /// A pattern: its tokens as they are.
    Pattern,
}

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
    /// among them, and the close's position. The calls among them are not
    /// noted for `frame_effects` as they are taken: they run where the
    /// expansion parses them (`stack![page, opaque(child.view(..))]`).
    fn group_rest(
        &mut self,
        open: &Token,
        position: Position,
        name: &str,
    ) -> Result<(Vec<(Token, Position)>, Position), Error> {
        self.unnoted(|parser| parser.take_group(open, position, name))
    }

    /// The tokens of `group_rest`, as they are read.
    fn take_group(
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
        self.splice_expansion(rules, krate, name_span, Place::Expression)?;
        self.parse_primary()
    }

    /// Whether a call of the program's own macro starts the next statement,
    /// or a crate macro's call is the next statement
    /// ([`Self::crate_statement_ahead`]).
    pub(super) fn macro_statement_ahead(&mut self) -> bool {
        #[cfg(feature = "rust-import")]
        if self.crate_statement_ahead().is_some() {
            return true;
        }
        let Token::Identifier(name) = self.peek() else {
            return false;
        };
        matches!(self.peek_second(), Token::Bang) && {
            let at = self.position();
            self.program_macro(&name, at).is_some()
        }
    }

    /// The crate macro ([`Self::crate_macro`]) whose call is next: its path
    /// from its crate's root, how many tokens the path is, and the input
    /// after the call's `!`. The path's first segment reads through the
    /// `use` that binds it, and a single name through the `use` that binds
    /// it as a crate macro (#2149).
    #[cfg(feature = "rust-import")]
    fn crate_call_ahead(&mut self) -> Option<(String, usize, CallInput)> {
        // Only a path that starts at a dependency or a `use` can name one,
        // which is known before the input is copied to look further.
        let Token::Identifier(first) = self.peek() else {
            return None;
        };
        if self.models.import(&first).is_none()
            && !self
                .models
                .crates
                .as_ref()
                .is_some_and(|crates| crates.dependency(&first))
        {
            return None;
        }
        let mut ahead = self.input.clone();
        let mut segments = Vec::new();
        let mut taken = 0;
        loop {
            let Some((Token::Identifier(name), _)) = ahead.next() else {
                return None;
            };
            segments.push(name.to_string());
            taken += 1;
            match ahead.next() {
                Some((Token::DoubleColon, _)) => taken += 1,
                Some((Token::Bang, _)) => break,
                _ => return None,
            }
        }
        let path = match self.models.import(&segments[0]) {
            Some(binding) if segments.len() > 1 => {
                let mut path = binding.qualified.to_string();
                for segment in &segments[1..] {
                    path.push_str("::");
                    path.push_str(segment);
                }
                path
            }
            Some(binding) if binding.target == crate::resolve::item::Import::CrateMacro => {
                binding.qualified.to_string()
            }
            _ => segments.join("::"),
        };
        self.crate_macro(&path).then_some((path, taken, ahead))
    }

    /// The path from its crate's root of the crate macro whose call is the
    /// next statement ([`Self::crate_call_ahead`]), with how many tokens
    /// the path is. A call is a statement as rustc parses one: in braces
    /// not followed by `.` or `?`, or in parentheses or brackets followed
    /// by `;`. Any other is an expression.
    #[cfg(feature = "rust-import")]
    fn crate_statement_ahead(&mut self) -> Option<(String, usize)> {
        let (path, taken, mut ahead) = self.crate_call_ahead()?;
        let (open, _) = ahead.next()?;
        let braced = matches!(open, Token::LeftBrace | Token::MapStart);
        let mut depth = usize::from(open != Token::Unit);
        while depth > 0 {
            match ahead.next()? {
                (Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart, _) => {
                    depth += 1;
                }
                (Token::RightParen | Token::RightBracket | Token::RightBrace, _) => depth -= 1,
                (Token::EOF, _) => return None,
                _ => {}
            }
        }
        let after = ahead.next().map(|(token, _)| token);
        let statement = if braced {
            !matches!(&after, Some(Token::Period))
                && !matches!(&after, Some(Token::Reserved(word)) if word.as_str() == "?")
        } else {
            matches!(open, Token::LeftParen | Token::LeftBracket | Token::Unit)
                && matches!(after, Some(Token::SemiColon))
        };
        statement.then_some((path, taken))
    }

    /// A call of the program's own macro as a statement: its expansion
    /// takes its place among the block's statements, so a `let` of a name
    /// the caller passed in binds it after the call, as in Rust. So does a
    /// crate macro's, as rustc expands it (#1125).
    pub(super) fn parse_macro_statement(&mut self) -> Result<(), Error> {
        #[cfg(feature = "rust-import")]
        if let Some((path, taken)) = self.crate_statement_ahead() {
            let path_span = self.take_path(taken);
            return self.splice_crate_expansion(&path, path_span, Place::Statement);
        }
        let (token, position) = self.next();
        let Token::Identifier(name) = token else {
            unreachable!("`macro_statement_ahead` saw a name");
        };
        let name_span = span(position, text_end(position, &name));
        let rules = self
            .program_macro(&name, position)
            .expect("`macro_statement_ahead` found the macro")?;
        self.splice_expansion(&rules, "crate", name_span, Place::Statement)
    }

    /// The span of the `taken` tokens of a call's path, which are read.
    #[cfg(feature = "rust-import")]
    fn take_path(&mut self, taken: usize) -> Span {
        let start = self.position();
        let mut end = start;
        for _ in 0..taken {
            let (token, position) = self.next();
            end = text_end(position, &token.to_string());
        }
        span(start, end)
    }

    /// When a call of the program's own macro or of a crate macro is next,
    /// where a type or a pattern goes (`place`), put its expansion in its
    /// place, as rustc parses it there, and say so.
    pub(super) fn splice_macro_ahead(&mut self, place: Place) -> Result<bool, Error> {
        #[cfg(feature = "rust-import")]
        if let Some((path, taken, _)) = self.crate_call_ahead() {
            let path_span = self.take_path(taken);
            self.splice_crate_expansion(&path, path_span, place)?;
            return Ok(true);
        }
        let Token::Identifier(name) = self.peek() else {
            return Ok(false);
        };
        if !matches!(self.peek_second(), Token::Bang) {
            return Ok(false);
        }
        let position = self.position();
        let Some(rules) = self.program_macro(&name, position) else {
            return Ok(false);
        };
        let rules = rules?;
        self.next();
        let name_span = span(position, text_end(position, &name));
        self.splice_expansion(&rules, "crate", name_span, place)?;
        Ok(true)
    }

    /// Expand the call of `rules` from its `!` and put its tokens next in
    /// the input, as its `place` parses them.
    fn splice_expansion(
        &mut self,
        rules: &macro_rules::Rules,
        krate: &str,
        name_span: Span,
        place: Place,
    ) -> Result<(), Error> {
        let name: ImmutableString = rules.name().into();
        self.expect(Token::Bang, "'!' after a macro name")?;
        let (open, position) = self.next();
        let (tokens, end) = match open {
            Token::Unit => (Vec::new(), position),
            open => self.group_rest(&open, position, &name)?,
        };
        let call = span(name_span.start(), text_end(end, " "));
        let remaining = self.nest_expansion(&name, call)?;
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
        self.splice_expanded(expansion, name_span.start(), end, place, remaining);
        Ok(())
    }

    /// Count one more expansion of the macro `name` at `call`, inside the
    /// expansions whose tokens are not all read yet: rustc's recursion
    /// limit bounds how deep calls nest. The input's length is where this
    /// expansion's tokens end.
    fn nest_expansion(&mut self, name: &str, call: Span) -> Result<usize, Error> {
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
        Ok(remaining)
    }

    /// Put a call's `expansion` next in the input, as its `place` parses
    /// it: one expression is parenthesised, statements in an expression's
    /// place are a block. `at` is where the call starts, `end` where its
    /// closing delimiter is, and `remaining` where the expansion's tokens
    /// end ([`Self::nest_expansion`]).
    fn splice_expanded(
        &mut self,
        expansion: Vec<(Token, Position)>,
        at: Position,
        end: Position,
        place: Place,
        remaining: usize,
    ) {
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
        let mut spliced = Vec::with_capacity(expansion.len() + 2);
        if matches!(place, Place::Type | Place::Pattern) {
            spliced.extend(expansion);
        } else if place == Place::Statement && statements {
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
    }

    /// Whether `path` names a crate macro Rawr has no rules for
    /// ([`crate::resolve::extern_prelude::Prelude::crate_macro`]).
    #[cfg(feature = "rust-import")]
    pub(super) fn crate_macro(&self, path: &str) -> bool {
        self.models
            .crates
            .as_ref()
            .is_some_and(|crates| crates.crate_macro(&self.models.host, path))
    }

    /// `path!(..)`, `path![..]` or `path! {..}` of a crate's function-like
    /// macro Rawr has no rules for, as a proc macro is, from its `!`:
    /// rustc's expansion of the call, parsed in its place as one
    /// expression (#1125).
    #[cfg(feature = "rust-import")]
    pub(super) fn expand_crate_macro(
        &mut self,
        path: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.splice_crate_expansion(path, name_span, Place::Expression)?;
        self.parse_primary()
    }

    /// Put rustc's expansion of the call of the crate macro at `path`, from
    /// its `!`, next in the input, as its `place` parses it. The project
    /// keeps each call's expansion by its macro, delimiters, input and
    /// place (`rawr_import::expand::call`), so a call whose input an edit
    /// changed has none yet: it is rustc's to expand, and checking again
    /// reads it. The expansion's tokens are reported at the call, as
    /// rustc reports a proc macro's.
    #[cfg(feature = "rust-import")]
    fn splice_crate_expansion(
        &mut self,
        path: &str,
        name_span: Span,
        place: Place,
    ) -> Result<(), Error> {
        use rawr_import::expand::call;
        self.expect(Token::Bang, "'!' after a macro name")?;
        let (open, position) = self.next();
        let delimiter = match open {
            Token::LeftBracket => call::Delimiter::Bracket,
            Token::LeftBrace | Token::MapStart => call::Delimiter::Brace,
            _ => call::Delimiter::Parenthesis,
        };
        let (tokens, end) = match open {
            Token::Unit => (Vec::new(), position),
            open => self.group_rest(&open, position, path)?,
        };
        let call = call::Call {
            path: path.to_owned(),
            delimiter,
            input: token_text(&tokens),
            place: match place {
                Place::Expression => call::Place::Expression,
                Place::Statement => call::Place::Statements,
                Place::Type => call::Place::Type,
                Place::Pattern => call::Place::Pattern,
            },
        };
        let kept =
            self.models.crates.as_ref().and_then(|crates| {
                std::fs::read_to_string(call.source_path(crates.project())).ok()
            });
        let Some(text) = kept.as_deref().and_then(call::kept) else {
            return Err(Error::MacroExpansionRequired {
                path: path.into(),
                call: serde_json::to_vec(&call).unwrap_or_default().into(),
                reference: name_span,
            });
        };
        let remaining = self.nest_expansion(path, span(name_span.start(), text_end(end, " ")))?;
        let at = name_span.start();
        let expansion = lex_rust(&crate::Engine::new_raw(), text)
            .into_iter()
            .filter(|(token, _)| *token != Token::EOF)
            .map(|(token, _)| (token, at))
            .collect();
        self.splice_expanded(expansion, at, end, place, remaining);
        Ok(())
    }
}
