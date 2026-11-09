//! Statics declared in a block, written or expanded (D862). Each is named
//! only in its block. One of a native type is a place for the program's
//! life (`place::statics`), keyed by its site and its block's declarations,
//! and initialized the first time its block runs. One of another type keeps
//! its value in the interpreter: each read is its initializer, as a
//! constant's is.
//!
//! A block's statics are all declared before any is initialized, and they
//! are initialized in order. A native static's initializer may name one
//! written before it. One that names a static not written yet, itself or a
//! sibling after it, is refused: statics that refer to each other, as
//! tracing's `META` and `__CALLSITE` do, would need a place referenced
//! before it is written, which Rust's aliasing rules don't allow at run
//! time (D862's Miri check).

use super::*;
use std::hash::{Hash, Hasher};

/// A static the blocks being parsed declare, as its name reads.
pub(super) struct Declared {
    name: ImmutableString,
    ty: Ty,
    kind: Kind,
}

enum Kind {
    /// A native place, by its key, and whether its block wrote it yet.
    Native { key: ImmutableString, written: bool },
    /// A value in the interpreter: its initializer's tokens.
    Interpreted { value: Vec<(Token, Position)> },
}

/// A native static's initializer, which runs once its block's statics are
/// all declared.
struct Initializer {
    key: ImmutableString,
    ty: Ty,
    declaration: Span,
    value: Vec<(Token, Position)>,
}

/// One declaration taken out of a block: its tokens from `static` through
/// the `;`, and its attributes'.
struct Declaration {
    attributes: Vec<(Token, Position)>,
    tokens: Vec<(Token, Position)>,
}

impl Parser<'_> {
    /// Whether a `static` item, perhaps after attributes, starts the next
    /// statement.
    pub(super) fn static_ahead(&mut self) -> bool {
        if !self.attribute_ahead() {
            return is_static(&self.peek());
        }
        let mut ahead = self.input.clone();
        loop {
            match ahead.next() {
                Some((Token::Hash, _)) => {}
                Some((token, _)) => return is_static(&token),
                None => return false,
            }
            let Some((Token::LeftBracket, _)) = ahead.next() else {
                return false;
            };
            let mut depth = 0_usize;
            for (token, _) in ahead.by_ref() {
                match token {
                    Token::LeftBracket | Token::LeftParen | Token::LeftBrace => depth += 1,
                    Token::RightBracket if depth == 0 => break,
                    Token::RightBracket | Token::RightParen | Token::RightBrace => {
                        depth = depth.saturating_sub(1);
                    }
                    Token::EOF => return false,
                    _ => {}
                }
            }
        }
    }

    /// The statics the current block declares from here to its end: each
    /// named from here on, and each initialized here, in order.
    pub(super) fn parse_block_statics(&mut self) -> Result<(), Error> {
        let declarations = self.take_block_statics();
        let group = self.fingerprint(&declarations);
        let mut initializers = Vec::with_capacity(declarations.len());
        for (ordinal, declaration) in declarations.into_iter().enumerate() {
            self.splice_tokens(declaration.attributes);
            if !self.outer_attributes()? {
                continue;
            }
            self.splice_tokens(declaration.tokens);
            let (_, keyword) = self.next();
            if matches!(self.peek(), Token::Mut) {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a `static mut`; keep the value in your state".into(),
                    source: point_span(self.position()),
                });
            }
            let (name, name_span) = self.expect_name("static name")?;
            self.expect(Token::Colon, "':' after a static's name")?;
            let ty = self.parse_local_type()?;
            self.expect(Token::Equals, "'=' after a static's type")?;
            let value = self.static_value()?;
            let kind = if matches!(ty, Ty::Host { .. }) {
                let file = self.source_identity();
                let line = keyword.line().unwrap_or_default();
                let column = keyword.position().unwrap_or_default();
                let key: ImmutableString =
                    format!("{file}:{line}:{column}#{ordinal}@{group:016x}").into();
                initializers.push(Initializer {
                    key: key.clone(),
                    ty: ty.clone(),
                    declaration: span(keyword, name_span.end()),
                    value,
                });
                Kind::Native {
                    key,
                    written: false,
                }
            } else {
                Kind::Interpreted { value }
            };
            self.block_statics.push(Declared { name, ty, kind });
        }
        for initializer in initializers {
            let key = initializer.key.clone();
            self.initialize_static(initializer)?;
            let written =
                self.block_statics
                    .iter_mut()
                    .find_map(|declared| match &mut declared.kind {
                        Kind::Native { key: own, written } if *own == key => Some(written),
                        _ => None,
                    });
            if let Some(written) = written {
                *written = true;
            }
        }
        self.last_type = Ty::Unit;
        self.last_expression = None;
        Ok(())
    }

    /// The tokens of a static's initializer, through its `;`, which is not
    /// among them.
    fn static_value(&mut self) -> Result<Vec<(Token, Position)>, Error> {
        let mut tokens = Vec::new();
        let mut depth = 0_usize;
        loop {
            let (token, position) = self.next();
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::SemiColon if depth == 0 => return Ok(tokens),
                Token::EOF => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a `static` item without its `;`".into(),
                        source: point_span(position),
                    });
                }
                _ => {}
            }
            tokens.push((token, position));
        }
    }

    /// `if reserved { settle(value) }` for the native static `key`: its
    /// value made and moved into its place the first time its block runs.
    fn initialize_static(&mut self, initializer: Initializer) -> Result<(), Error> {
        let Initializer {
            key,
            ty,
            declaration,
            value,
        } = initializer;
        let at = declaration.start();
        let mut tokens = Vec::with_capacity(value.len() + 2);
        tokens.push((Token::LeftBrace, at));
        tokens.extend(value);
        tokens.push((Token::RightBrace, at));
        self.splice_tokens(tokens);
        let mut value = self.parse_expr_expecting(Some(&ty))?;
        self.coerce_expr(&ty, &mut value)?;
        let key = origin::Fragment::plain(format!("{key:?}"));
        let reserved = self.prelude_op(PreludeOp::StaticReserved, [key.clone()]);
        let settle = self.prelude_op(PreludeOp::StaticSettle, [key, value.rendered_source()]);
        let mut statement = origin::Fragment::plain("if ");
        statement.push(reserved);
        statement.push_text(" { ");
        statement.push(settle);
        statement.push_text("; }");
        self.write_output(statement, declaration);
        Ok(())
    }

    /// A read of the static `name` the blocks being parsed declare: its
    /// native place, or its initializer. Either lives as long as the
    /// program, so a borrow of it is `'static`.
    pub(super) fn block_static(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Option<Result<ParsedExpr, Error>> {
        let declared = self
            .block_statics
            .iter()
            .rev()
            .find(|declared| declared.name.as_str() == name)?;
        let ty = declared.ty.clone();
        let read = match &declared.kind {
            Kind::Native { written: false, .. } => {
                return Some(Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "a `static` whose initializer names `{name}` before `{name}` is \
                         written, a cycle of statics such as tracing's `META` and `__CALLSITE`,"
                    )
                    .into(),
                    source: name_span,
                }));
            }
            Kind::Native { key, .. } => {
                let key = origin::Fragment::plain(format!("{key:?}"));
                let source = self.prelude_op(PreludeOp::StaticValue, [key]);
                Ok(self.operator_expr(source, ty.clone(), name_span))
            }
            Kind::Interpreted { value } => {
                let at = name_span.start();
                let mut tokens = Vec::with_capacity(value.len() + 2);
                tokens.push((Token::LeftParen, at));
                tokens.extend(value.iter().cloned());
                tokens.push((Token::RightParen, at));
                self.splice_tokens(tokens);
                self.parse_primary().and_then(|mut value| {
                    self.coerce_expr(&ty, &mut value)?;
                    Ok(value)
                })
            }
        };
        if let Ok(read) = &read {
            self.static_reads.insert(read.span);
            if self.may_hold_borrow(&ty) {
                self.flow().set_origins(read.span, Vec::new());
            }
        }
        Some(read)
    }

    /// The `static` declarations at the top level of the block from here to
    /// its end, taken out of the input with their attributes; the rest of
    /// the input stays as it is.
    fn take_block_statics(&mut self) -> Vec<Declaration> {
        let rest: Vec<(Token, Position)> =
            std::mem::replace(&mut self.input, Vec::new().into_iter().peekable()).collect();
        let mut declarations = Vec::new();
        let mut kept = Vec::with_capacity(rest.len());
        let mut depth = 0_usize;
        let mut boundary = true;
        let mut index = 0;
        while index < rest.len() {
            if depth == 0
                && boundary
                && let Some((attributes, end)) = declaration_at(&rest, index)
            {
                declarations.push(Declaration {
                    attributes: rest[index..attributes].to_vec(),
                    tokens: rest[attributes..=end].to_vec(),
                });
                index = end + 1;
                continue;
            }
            let (token, _) = &rest[index];
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace if depth == 0 => {
                    break;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => depth -= 1,
                Token::EOF => break,
                _ => {}
            }
            boundary = depth == 0 && matches!(token, Token::SemiColon | Token::RightBrace);
            kept.push(rest[index].clone());
            index += 1;
        }
        kept.extend(rest.into_iter().skip(index));
        self.input = kept.into_iter().peekable();
        declarations
    }

    /// What tells one block's statics apart from another's at the same
    /// site, and from an earlier declaration of them: every token of the
    /// block's declarations, with where each `line!` and `column!` is, and
    /// the statics whose initializer the block is in.
    fn fingerprint(&self, declarations: &[Declaration]) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        for declared in &self.block_statics {
            if let Kind::Native { key, .. } = &declared.kind {
                key.as_str().hash(&mut hasher);
            }
        }
        for declaration in declarations {
            let tokens = declaration.attributes.iter().chain(&declaration.tokens);
            for (token, position) in tokens {
                let text = token.to_string();
                text.hash(&mut hasher);
                if matches!(text.as_str(), "line" | "column") {
                    position.line().hash(&mut hasher);
                    position.position().hash(&mut hasher);
                }
            }
        }
        hasher.finish()
    }
}

fn is_static(token: &Token) -> bool {
    matches!(token, Token::Reserved(word) if word.as_str() == "static")
}

/// The declaration starting at `index`, perhaps after attributes: where
/// its `static` is and where its `;` is.
fn declaration_at(tokens: &[(Token, Position)], index: usize) -> Option<(usize, usize)> {
    let mut at = index;
    while matches!(tokens.get(at), Some((Token::Hash, _))) {
        at += 1;
        if !matches!(tokens.get(at), Some((Token::LeftBracket, _))) {
            return None;
        }
        let mut depth = 0_usize;
        loop {
            at += 1;
            match &tokens.get(at)?.0 {
                Token::LeftBracket | Token::LeftParen | Token::LeftBrace => depth += 1,
                Token::RightBracket if depth == 0 => break,
                Token::RightBracket | Token::RightParen | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::EOF => return None,
                _ => {}
            }
        }
        at += 1;
    }
    if !is_static(&tokens.get(at)?.0) {
        return None;
    }
    let mut depth = 0_usize;
    for (offset, (token, _)) in tokens[at..].iter().enumerate() {
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth = depth.checked_sub(1)?;
            }
            Token::SemiColon if depth == 0 => return Some((at, at + offset)),
            Token::EOF => return None,
            _ => {}
        }
    }
    None
}
