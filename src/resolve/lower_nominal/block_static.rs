//! Statics declared in a block, written or expanded (D862), and the keys a
//! `thread_local!` call in a block declares (`thread_local`). Each is named
//! only in its block. One of a native type is a place for the program's
//! life (`place::statics`), keyed by its site and its block's declarations,
//! and initialized the first time its block runs. One of another type keeps
//! its value in the interpreter, by the same key: its initializer runs at
//! its first read, and every read shares the value it made (#2519). One of
//! literals alone is its initializer at each read, as a constant is.
//!
//! A block's statics are all declared before any is initialized, and they
//! are initialized in order. An initializer may name a static not written
//! yet, itself or a sibling after it, as tracing's `META` and `__CALLSITE`
//! refer to each other. The statics written from then on, through the
//! write of the one named, are a cycle, which the runner builds as
//! `place::cycle` does (D865). Each needs its type's layout from the
//! import driver, with no pointer to a static inside an enum variant or a
//! union, or the cycle is refused.

use super::*;
use std::hash::{Hash, Hasher};

/// A static the blocks being parsed declare, as its name reads.
pub(super) struct Declared {
    name: ImmutableString,
    ty: Ty,
    kind: Kind,
}

enum Kind {
    /// A native place, by its key, and how far its block has come with it.
    Native {
        key: ImmutableString,
        progress: Progress,
    },
    /// A value in the interpreter: its initializer's tokens, and the key it
    /// is kept by, `None` for literals alone.
    Interpreted {
        value: Vec<(Token, Position)>,
        key: Option<ImmutableString>,
    },
    /// A `thread_local!` key of a `LocalKey<T>`, `T` the declared type: its
    /// initializer's tokens and the key each thread's value is found by.
    ThreadLocal {
        value: Vec<(Token, Position)>,
        key: ImmutableString,
    },
}

/// How far a block has come with one of its native statics.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Progress {
    /// Not written yet, and named by no initializer.
    Declared,
    /// Named by an initializer before it is written: the statics written
    /// from then on, through its own write, are a cycle (D865).
    Named,
    Written,
}

/// A native static's initializer, which runs once its block's statics are
/// all declared.
struct Initializer {
    name: ImmutableString,
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
        let mut ahead = self.input.clone().map(|(token, _)| token);
        if !self.attribute_ahead() {
            return declares(&mut ahead);
        }
        loop {
            match ahead.next() {
                Some(Token::Hash) => {}
                Some(token) => return declares(&mut std::iter::once(token).chain(ahead)),
                None => return false,
            }
            let Some(Token::LeftBracket) = ahead.next() else {
                return false;
            };
            let mut depth = 0_usize;
            for token in ahead.by_ref() {
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
            if !is_static(&self.peek()) {
                self.parse_block_thread_locals(ordinal, group)?;
                continue;
            }
            let (_, keyword) = self.next();
            if matches!(self.peek(), Token::Mut) {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: crate::resolve::Unsupported::construct("a `static mut`")
                        .with_help("keep the value in your state"),
                    source: point_span(self.position()),
                });
            }
            let (name, name_span) = self.expect_name("static name")?;
            self.expect(Token::Colon, "':' after a static's name")?;
            let start = self.position();
            let ahead = self.input.clone();
            let ty = self.parse_local_type()?;
            let equals = self.expect(Token::Equals, "'=' after a static's type")?;
            // Every thread shares a static, so its type is `Sync`.
            if let Some((last, at)) = ahead.take_while(|(_, at)| *at < equals).last()
                && let Some(error) =
                    self.unshared_static(&ty, span(start, text_end(at, &last.to_string())))
            {
                return Err(error);
            }
            let value = self.static_value()?;
            let file = self.source_identity();
            let line = keyword.line().unwrap_or_default();
            let column = keyword.position().unwrap_or_default();
            let key: ImmutableString =
                format!("{file}:{line}:{column}#{ordinal}@{group:016x}").into();
            let kind = if matches!(ty, Ty::Host { .. }) {
                initializers.push(Initializer {
                    name: name.clone(),
                    key: key.clone(),
                    ty: ty.clone(),
                    declaration: span(keyword, name_span.end()),
                    value,
                });
                Kind::Native {
                    key,
                    progress: Progress::Declared,
                }
            } else {
                let plain = constant::plain_initializer(value.iter().map(|(token, _)| token));
                let kept = crate::place::statics::KEPT;
                let key = (!plain).then(|| format!("{kept}{key}").into());
                Kind::Interpreted { value, key }
            };
            self.block_statics.push(Declared { name, ty, kind });
        }
        for initializer in initializers {
            let key = initializer.key.clone();
            self.initialize_static(initializer)?;
            let progress =
                self.block_statics
                    .iter_mut()
                    .find_map(|declared| match &mut declared.kind {
                        Kind::Native { key: own, progress } if *own == key => Some(progress),
                        _ => None,
                    });
            if let Some(progress) = progress {
                *progress = Progress::Written;
            }
        }
        self.last_type = Ty::Unit;
        self.last_expression = None;
        Ok(())
    }

    /// The keys of the `thread_local!` call next, at `ordinal` among the
    /// block's declarations, `group` their fingerprint: each a `LocalKey<T>`
    /// named from here on, whose initializer runs at a first access on a
    /// thread. A key needn't be `Sync`, as each thread has its own value.
    fn parse_block_thread_locals(&mut self, ordinal: usize, group: u64) -> Result<(), Error> {
        while !matches!(self.peek(), Token::Bang) {
            self.next();
        }
        self.next();
        let (open, _) = self.next();
        let close = match open {
            Token::LeftParen => Token::RightParen,
            Token::LeftBracket => Token::RightBracket,
            _ => Token::RightBrace,
        };
        while self.peek() != close {
            let enabled = self.outer_attributes()?;
            if matches!(self.peek(), Token::Pub) {
                self.next();
                if matches!(self.peek(), Token::LeftParen) {
                    while !matches!(self.next().0, Token::RightParen | Token::EOF) {}
                }
            }
            let (word, keyword) = self.next();
            if !is_static(&word) {
                return Err(no_thread_local_rule(&word, keyword));
            }
            let (name, _) = self.expect_name("thread-local key name")?;
            self.expect(Token::Colon, "':' after a thread-local key's name")?;
            let ty = self.parse_local_type()?;
            self.expect(Token::Equals, "'=' after a thread-local key's type")?;
            // `const { init }`: the block is the initializer.
            if matches!(self.peek(), Token::Const) {
                self.next();
            }
            let value = self.thread_local_value(&close)?;
            if !enabled {
                continue;
            }
            let file = self.source_identity();
            let line = keyword.line().unwrap_or_default();
            let column = keyword.position().unwrap_or_default();
            let key = format!("{file}:{line}:{column}#{ordinal}@{group:016x}").into();
            self.block_statics.push(Declared {
                name,
                ty,
                kind: Kind::ThreadLocal { value, key },
            });
        }
        self.next();
        if close != Token::RightBrace && matches!(self.peek(), Token::SemiColon) {
            self.next();
        }
        Ok(())
    }

    /// The tokens of a thread-local key's initializer, through its `;` or
    /// up to the call's `close`, neither among them. The next key's
    /// `static` where the `;` goes is rustc's error.
    fn thread_local_value(&mut self, close: &Token) -> Result<Vec<(Token, Position)>, Error> {
        let mut tokens = Vec::new();
        let mut depth = 0_usize;
        loop {
            if depth == 0 && self.peek() == *close {
                return Ok(tokens);
            }
            let (token, position) = self.next();
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::SemiColon if depth == 0 => return Ok(tokens),
                Token::EOF => return Ok(tokens),
                ref word if depth == 0 && is_static(word) => {
                    return Err(no_thread_local_rule(word, position));
                }
                _ => {}
            }
            tokens.push((token, position));
        }
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

    /// `if reserved { settle(value, layout) }` for the native static `key`:
    /// its value made and moved into its place the first time its block
    /// runs.
    fn initialize_static(&mut self, initializer: Initializer) -> Result<(), Error> {
        let Initializer {
            name,
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
        let layout = self.static_layout(&name, &ty, declaration)?;
        let key = origin::Fragment::plain(format!("{key:?}"));
        let reserved = self.prelude_op(PreludeOp::StaticReserved, [key.clone()]);
        let settle = self.prelude_op(
            PreludeOp::StaticSettle,
            [key, value.rendered_source(), layout],
        );
        let mut statement = origin::Fragment::plain("if ");
        statement.push(reserved);
        statement.push_text(" { ");
        statement.push(settle);
        statement.push_text("; }");
        self.write_output(statement, declaration);
        Ok(())
    }

    /// The layout of the static `name`'s type `ty`, as the runner takes it:
    /// `[size, pointer offsets..]`, or `()` when the import driver gave
    /// none. A static written while one that was named before its write
    /// isn't written yet is in a cycle, which needs the layout, with no
    /// pointer to a static inside an enum variant or a union.
    fn static_layout(
        &self,
        name: &str,
        ty: &Ty,
        declaration: Span,
    ) -> Result<origin::Fragment, Error> {
        let layout = match ty {
            Ty::Host { proof, .. } => proof.definition.layout(),
            _ => None,
        };
        let cycle = self.block_statics.iter().any(|declared| {
            matches!(
                declared.kind,
                Kind::Native {
                    progress: Progress::Named,
                    ..
                }
            )
        });
        let refused = match layout {
            _ if !cycle => None,
            None => Some("whose type's layout rawr doesn't know,"),
            Some(layout) if layout.varied() => Some(
                "whose type keeps a pointer inside an enum variant or a union, as \
                 `Option<&'static T>` does,",
            ),
            Some(_) => None,
        };
        if let Some(reason) = refused {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "a `static` `{name}` in a cycle of statics, which name each other before \
                     they are written, {reason}"
                )
                .into(),
                source: declaration,
            });
        }
        let Some(layout) = layout else {
            return Ok(origin::Fragment::plain("()"));
        };
        let words: Vec<String> = std::iter::once(layout.size())
            .chain(layout.pointers().iter().copied())
            .map(|word| word.to_string())
            .collect();
        Ok(origin::Fragment::plain(format!("[{}]", words.join(", "))))
    }

    /// A read of the static `name` the blocks being parsed declare: its
    /// native place, or its initializer. Either lives as long as the
    /// program, so a borrow of it is `'static`. A native static named
    /// before its block writes it starts a cycle.
    pub(super) fn block_static(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Option<Result<ParsedExpr, Error>> {
        let declared = self
            .block_statics
            .iter_mut()
            .rev()
            .find(|declared| declared.name.as_str() == name)?;
        if let Kind::Native { progress, .. } = &mut declared.kind
            && *progress == Progress::Declared
        {
            *progress = Progress::Named;
        }
        let ty = declared.ty.clone();
        let read = match &declared.kind {
            Kind::ThreadLocal { value, key } => {
                let (value, key) = (value.clone(), key.clone());
                return Some(self.block_thread_local(&key, value, ty, name_span));
            }
            Kind::Native { key, .. } => {
                let key = origin::Fragment::plain(format!("{key:?}"));
                let source = self.prelude_op(PreludeOp::StaticValue, [key]);
                Ok(self.operator_expr(source, ty.clone(), name_span))
            }
            Kind::Interpreted { value, key } => {
                let key = key.clone();
                let at = name_span.start();
                let mut tokens = Vec::with_capacity(value.len() + 2);
                tokens.push((Token::LeftParen, at));
                tokens.extend(value.iter().cloned());
                tokens.push((Token::RightParen, at));
                self.splice_tokens(tokens);
                self.parse_primary().and_then(|mut value| {
                    self.coerce_expr(&ty, &mut value)?;
                    let Some(key) = key else {
                        return Ok(value);
                    };
                    let read = self.kept_static(&key, value.rendered_source());
                    Ok(self.operator_expr(read, ty.clone(), value.span))
                })
            }
        };
        if let Ok(read) = &read {
            self.static_reads.insert(read.span);
            self.unmovable
                .insert(read.span, Unplaced::static_item(name));
            if self.may_hold_borrow(&ty) {
                self.flow().set_origins(read.span, Vec::new());
            }
        }
        Some(read)
    }

    /// A use of the block's thread-local key `key`, a `LocalKey<ty>` whose
    /// initializer is `value`'s tokens: its runtime value, which lives as
    /// long as the program, so a borrow of it is `'static`.
    fn block_thread_local(
        &mut self,
        key: &str,
        value: Vec<(Token, Position)>,
        ty: Ty,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let at = name_span.start();
        let mut tokens = Vec::with_capacity(value.len() + 2);
        tokens.push((Token::LeftParen, at));
        tokens.extend(value);
        tokens.push((Token::RightParen, at));
        self.splice_tokens(tokens);
        let mut initializer = self.parse_primary()?;
        self.coerce_expr(&ty, &mut initializer)?;
        let source = self.local_key(key, initializer.rendered_source(), &ty, name_span)?;
        let read = self.operator_expr(
            source,
            Ty::Thread(thread::Kind::LocalKey, Box::new(ty)),
            name_span,
        );
        self.static_reads.insert(read.span);
        Ok(read)
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

/// rustc's error for `token` at `position` in a `thread_local!` call, where
/// std's `macro_rules!` matcher has no rule for it.
fn no_thread_local_rule(token: &Token, position: Position) -> Error {
    Error::Syntax {
        message: format!("no rules expected {}", crate::parser::item::found(token)).into(),
        label: "no rules expected this token in macro call".into(),
        help: None,
        at: span(position, text_end(position, &token.to_string())),
    }
}

/// Whether `tokens` start a declaration a block's statics take: a `static`,
/// or a call of std's `thread_local!`, as the prelude or `std::` names it.
fn declares(tokens: &mut impl Iterator<Item = Token>) -> bool {
    let name = |token: Option<Token>, text: &str| matches!(token, Some(Token::Identifier(name)) if name.as_str() == text);
    match tokens.next() {
        Some(token) if is_static(&token) => true,
        Some(Token::Identifier(first)) if first.as_str() == "thread_local" => {
            matches!(tokens.next(), Some(Token::Bang))
        }
        Some(Token::Identifier(first)) if first.as_str() == "std" => {
            matches!(tokens.next(), Some(Token::DoubleColon))
                && name(tokens.next(), "thread_local")
                && matches!(tokens.next(), Some(Token::Bang))
        }
        _ => false,
    }
}

/// Where the `thread_local!` call starting at `at` ends: its closing
/// delimiter, or the `;` after one in parentheses or brackets.
fn thread_local_end(tokens: &[(Token, Position)], at: usize) -> Option<usize> {
    let bang = at
        + tokens[at..]
            .iter()
            .position(|(token, _)| matches!(token, Token::Bang))?;
    let braced = matches!(tokens.get(bang + 1)?.0, Token::LeftBrace);
    let mut depth = 0_usize;
    for (offset, (token, _)) in tokens[bang + 1..].iter().enumerate() {
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let end = bang + 1 + offset;
                    let semicolon = matches!(tokens.get(end + 1), Some((Token::SemiColon, _)));
                    return Some(if !braced && semicolon { end + 1 } else { end });
                }
            }
            Token::EOF => return None,
            _ => {}
        }
    }
    None
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
    let mut rest = tokens[at..].iter().map(|(token, _)| token.clone());
    if !declares(&mut rest) {
        return None;
    }
    if !is_static(&tokens.get(at)?.0) {
        return Some((at, thread_local_end(tokens, at)?));
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
