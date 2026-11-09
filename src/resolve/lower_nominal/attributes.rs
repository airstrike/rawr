//! Attributes and items inside a body: `#[cfg(..)]` on statements, list
//! elements and arms, and nested `fn`, `struct` and `macro_rules!` items.

use super::*;

impl<'a> Parser<'a> {
    /// Whether a `struct`, `enum`, `impl`, `trait` or `type` item, or an
    /// attribute, starts here in a body.
    /// The program's own macro `name` names at `at`: the innermost block's
    /// that defines one so far, else the one textual scope makes visible.
    pub(super) fn program_macro(
        &self,
        name: &str,
        at: Position,
    ) -> Option<Result<Shared<crate::resolve::macro_rules::Rules>, Error>> {
        if let Some((_, rules)) = self
            .local_macros
            .iter()
            .rev()
            .find(|(defined, _)| defined.as_str() == name)
        {
            return Some(Ok(rules.clone()));
        }
        let crate::resolve::macros::Lookup::Found(definition) =
            self.models.macros.lookup(name, self.models.context, at)
        else {
            return None;
        };
        let rules = self
            .models
            .macro_rules
            .get(&(definition.source(), definition.span()))
            .expect("every definition's rules are read with the tables");
        Some(rules.clone().map_err(|malformed| Error::Syntax {
            message: malformed.message().into(),
            label: malformed.label().into(),
            help: malformed.help().map(Into::into),
            at: point_span(malformed.at()),
        }))
    }

    /// Whether a `macro_rules!` definition is next.
    pub(super) fn macro_definition_ahead(&mut self) -> bool {
        matches!(self.peek(), Token::Identifier(ref name) if name.as_str() == "macro_rules")
            && matches!(self.peek_second(), Token::Bang)
    }

    pub(super) fn nested_item_ahead(&mut self) -> bool {
        if self.attribute_ahead() {
            return true;
        }
        match self.peek() {
            Token::Struct | Token::Enum | Token::Impl => true,
            Token::Identifier(name) if name.as_str() == "unsafe" => {
                matches!(self.peek_second(), Token::Impl)
            }
            Token::Identifier(name) => {
                matches!(name.as_str(), "trait" | "type")
                    && matches!(self.peek_second(), Token::Identifier(_))
            }
            _ => false,
        }
    }

    pub(super) fn attribute_ahead(&mut self) -> bool {
        matches!(self.peek(), Token::Hash)
    }

    /// The outer attributes ahead, read as rustc reads them: whether what
    /// they are on is configured in. A `#[cfg(..)]` that does not hold
    /// leaves it out, a `#[cfg_attr(..)]` whose predicate holds applies the
    /// attributes it lists, and every other attribute has no effect here.
    pub(super) fn outer_attributes(&mut self) -> Result<bool, Error> {
        let mut configured = true;
        while self.attribute_ahead() {
            self.next();
            self.expect(Token::LeftBracket, "`[` to open an attribute")?;
            configured &= self.attribute()?;
            self.expect(Token::RightBracket, "`]` to close an attribute")?;
        }
        Ok(configured)
    }

    /// One attribute inside `#[..]` or a `cfg_attr(..)`, up to the `,`,
    /// `)` or `]` after it: whether it keeps what it is on.
    pub(super) fn attribute(&mut self) -> Result<bool, Error> {
        let name = match self.peek() {
            Token::Identifier(name) if matches!(self.peek_second(), Token::LeftParen) => Some(name),
            _ => None,
        };
        match name.as_ref().map(|name| name.as_str()) {
            Some("cfg") => {
                self.next();
                self.next();
                let holds = self.cfg_predicate()?;
                self.expect(Token::RightParen, "`)` to close `cfg`")?;
                Ok(holds)
            }
            Some("cfg_attr") => {
                self.next();
                self.next();
                let holds = self.cfg_predicate()?;
                let mut configured = true;
                while matches!(self.peek(), Token::Comma) {
                    self.next();
                    if matches!(self.peek(), Token::RightParen) {
                        break;
                    }
                    if holds {
                        configured &= self.attribute()?;
                    } else {
                        self.skip_listed()?;
                    }
                }
                self.expect(Token::RightParen, "`)` to close `cfg_attr`")?;
                Ok(configured)
            }
            _ => {
                self.skip_listed()?;
                Ok(true)
            }
        }
    }

    /// Whether the element ahead in a list (a struct expression's field,
    /// an array's or a tuple's element, a call's argument) is left out by
    /// its attributes, as rustc leaves it out where a `#[cfg(..)]` does not
    /// hold. Its attributes are read; a left-out element is passed over
    /// with the `,` after it.
    pub(super) fn element_left_out(&mut self) -> Result<bool, Error> {
        if !self.attribute_ahead() || self.outer_attributes()? {
            return Ok(false);
        }
        self.skip_listed()?;
        if matches!(self.peek(), Token::Comma) {
            self.next();
        }
        Ok(true)
    }

    /// Whether the match arm ahead is left out by its attributes, as
    /// [`Self::element_left_out`] reads them. A left-out arm is passed over
    /// with its `,`.
    pub(super) fn arm_left_out(&mut self) -> Result<bool, Error> {
        if !self.attribute_ahead() || self.outer_attributes()? {
            return Ok(false);
        }
        let start = self.position();
        let mut depth = 0usize;
        loop {
            match self.next() {
                (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                    depth += 1;
                }
                (Token::RightBrace | Token::RightParen | Token::RightBracket, _) => {
                    depth = depth.saturating_sub(1);
                }
                (Token::DoubleArrow, _) if depth == 0 => break,
                (Token::EOF, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a match arm under `#[cfg(..)]` without its end".into(),
                        source: span(start, position),
                    });
                }
                _ => {}
            }
        }
        // A block body ends the arm; any other runs to its `,`.
        let block = matches!(self.peek(), Token::LeftBrace);
        if block {
            self.next();
            self.skip_listed()?;
            self.expect(Token::RightBrace, "`}` to close a match arm's block")?;
        } else {
            self.skip_listed()?;
        }
        if matches!(self.peek(), Token::Comma) {
            self.next();
        }
        Ok(true)
    }

    /// Pass over the tokens of one element of a list, up to the `,` or the
    /// closing delimiter at its depth, which it leaves. A turbofish's `<..>`
    /// is a depth, as its `,`s separate its own arguments.
    pub(super) fn skip_listed(&mut self) -> Result<(), Error> {
        let start = self.position();
        let mut depth = 0usize;
        let mut angles = 0usize;
        loop {
            match self.peek() {
                Token::Comma | Token::RightParen | Token::RightBracket | Token::RightBrace
                    if depth == 0 && angles == 0 =>
                {
                    return Ok(());
                }
                Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart => {
                    depth += 1;
                }
                Token::RightBrace | Token::RightParen | Token::RightBracket => {
                    depth = depth.saturating_sub(1);
                }
                Token::DoubleColon if matches!(self.peek_second(), Token::LessThan) => {
                    self.next();
                    angles += 1;
                }
                Token::GreaterThan if angles > 0 => angles -= 1,
                Token::RightShift if angles > 0 => angles = angles.saturating_sub(2),
                Token::EOF => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "an element under `#[cfg(..)]` without its end".into(),
                        source: span(start, self.position()),
                    });
                }
                _ => {}
            }
            self.next();
        }
    }

    /// A `struct`, `enum`, `impl`, `trait` or `type` item inside a body,
    /// with its attributes: the item parser hoisted it to the file's items,
    /// so its tokens are passed over here. A statement's attributes have no
    /// effect and are passed over too, except a `#[cfg(..)]` that is false,
    /// which leaves the statement out (`outer_attributes`).
    pub(super) fn skip_nested_item(&mut self) -> Result<(), Error> {
        let start = self.position();
        let unclosed = |position| Error::UnsupportedNominalSyntax {
            construct: "an item inside a body without its end".into(),
            source: span(start, position),
        };
        let configured = self.outer_attributes()?;
        if !configured && !self.nested_item_ahead() {
            return self.skip_statement();
        }
        if !self.nested_item_ahead() {
            return Ok(());
        }
        let mut depth = 0usize;
        loop {
            match self.next() {
                (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                    depth += 1;
                }
                (Token::RightParen | Token::RightBracket, _) => depth = depth.saturating_sub(1),
                (Token::RightBrace, _) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Ok(());
                    }
                }
                (Token::SemiColon, _) if depth == 0 => return Ok(()),
                (Token::EOF, position) => return Err(unclosed(position)),
                _ => {}
            }
        }
    }

    /// One statement a false `#[cfg(..)]` leaves out: through its `;`, or
    /// through the `}` that ends a block, `if`, `match` or loop.
    pub(super) fn skip_statement(&mut self) -> Result<(), Error> {
        let start = self.position();
        let braced = match self.peek() {
            Token::LeftBrace | Token::If | Token::For | Token::While | Token::Loop => true,
            Token::Reserved(keyword) => matches!(keyword.as_str(), "match" | "unsafe"),
            _ => false,
        };
        let mut depth = 0usize;
        loop {
            match self.next() {
                (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                    depth += 1;
                }
                (Token::RightParen | Token::RightBracket, _) => depth = depth.saturating_sub(1),
                (Token::RightBrace, _) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && braced && !matches!(self.peek(), Token::Else) {
                        return Ok(());
                    }
                }
                (Token::SemiColon, _) if depth == 0 => return Ok(()),
                (Token::EOF, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a statement under `#[cfg(..)]` without its end".into(),
                        source: span(start, position),
                    });
                }
                _ => {}
            }
        }
    }

    /// The predicate of `cfg(..)` or `cfg!(..)`: `test` holding as the
    /// engine builds, `feature = ".."` for each feature Cargo enables for
    /// the package, and every other option as rustc sets it for the target
    /// Cargo builds the package for.
    pub(super) fn cfg_predicate(&mut self) -> Result<bool, Error> {
        let (name, at) = match self.next() {
            (Token::Identifier(name), position) => (name.to_string(), position),
            (_, position) => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a `cfg` predicate that is not a name".into(),
                    source: point_span(position),
                })
            }
        };
        match name.as_str() {
            "not" | "all" | "any" => {
                self.expect(Token::LeftParen, "`(` after a `cfg` operator")?;
                let mut values = Vec::new();
                while !matches!(self.peek(), Token::RightParen) {
                    values.push(self.cfg_predicate()?);
                    if matches!(self.peek(), Token::Comma) {
                        self.next();
                    }
                }
                self.next();
                match name.as_str() {
                    "not" if values.len() == 1 => Ok(!values[0]),
                    "not" => Err(Error::UnsupportedNominalSyntax {
                        construct: "`not` in `cfg` takes one predicate".into(),
                        source: point_span(at),
                    }),
                    "all" => Ok(values.iter().all(|value| *value)),
                    _ => Ok(values.iter().any(|value| *value)),
                }
            }
            _ if matches!(self.peek(), Token::Equals) => {
                self.next();
                let value = match self.next() {
                    (Token::StringConstant(value), _) => value.to_string(),
                    (_, position) => {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "a `cfg` value that is not a string".into(),
                            source: point_span(position),
                        })
                    }
                };
                Ok(self
                    .models
                    .features
                    .cfg(&name, Some(&value), self.models.test))
            }
            _ => Ok(self.models.features.cfg(&name, None, self.models.test)),
        }
    }

    /// A `fn` item inside a body: the item parser hoisted it to the file's
    /// items, so its tokens are passed over here.
    pub(super) fn skip_nested_fn(&mut self) -> Result<(), Error> {
        let start = self.expect(Token::Fn, "`fn` to start a nested function")?;
        let mut depth = 0usize;
        loop {
            match self.next() {
                (Token::LeftBrace, _) => depth += 1,
                (Token::RightBrace, _) if depth == 1 => return Ok(()),
                (Token::RightBrace, _) => depth = depth.saturating_sub(1),
                (Token::EOF, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "nested `fn` without a body".into(),
                        source: span(start, position),
                    });
                }
                _ => {}
            }
        }
    }
}
