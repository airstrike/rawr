//! Expressions: an expression with its expected type, operands, primaries,
//! blocks and parenthesised values, and names read as values.

use super::*;

impl<'a> Parser<'a> {
    /// rustc's syntax error at the next token when no expression continues
    /// with it: a name, a literal, a keyword or a `!` after an expression
    /// (`2 in y`, `1 !in v`). What rustc lists as expected depends on what
    /// the expression ends ([`Continuation`]).
    pub(super) fn expected_operator(&mut self, continuation: Continuation) -> Option<Error> {
        let (token, position) = self.input.peek().map(|(token, at)| (token.clone(), *at))?;
        let stops = matches!(
            token,
            Token::Identifier(_)
                | Token::IntegerConstant(_)
                | Token::StringConstant(_)
                | Token::CharConstant(_)
                | Token::ByteConstant(_)
                | Token::Bang
                | Token::True
                | Token::False
                | Token::Fn
                | Token::Let
                | Token::Const
                | Token::In
                | Token::If
                | Token::While
                | Token::Loop
                | Token::For
                | Token::Return
        ) || {
            {
                matches!(token, Token::FloatConstant(_))
            }
        };
        if !stops || self.split_angle.is_some() {
            return None;
        }
        let (expected, label) = match continuation {
            Continuation::Path => (
                "expected one of `!`, `.`, `::`, `;`, `?`, `{`, `}`, or an operator",
                "expected one of 8 possible tokens",
            ),
            Continuation::Statement => (
                "expected one of `.`, `;`, `?`, `}`, or an operator",
                "expected one of `.`, `;`, `?`, `}`, or an operator",
            ),
            Continuation::Let => (
                "expected one of `.`, `;`, `?`, `else`, or an operator",
                "expected one of `.`, `;`, `?`, `else`, or an operator",
            ),
        };
        let found = crate::parser::item::found(&token);
        let written = found
            .trim_start_matches("reserved ")
            .trim_start_matches("keyword ")
            .trim_matches('`');
        Some(Error::Syntax {
            message: format!("{expected}, found {found}").into(),
            label: label.into(),
            help: None,
            at: Span::new(position, text_end(position, written)),
        })
    }

    /// An expression expected to have type `expected`: an array literal's
    /// elements coerce to its element type.
    pub(super) fn parse_expr_expecting(
        &mut self,
        expected: Option<&Ty>,
    ) -> Result<ParsedExpr, Error> {
        if let Some(expected) = expected {
            self.array_hint = self.literal_element(expected);
        }
        let hint = expected.map(|expected| (expected.clone(), self.expression_stack.len() + 1));
        let outer = std::mem::replace(&mut self.collect_hint, hint);
        let value = self.parse_expr();
        self.collect_hint = outer;
        self.array_hint = None;
        value
    }

    /// The element type the array or `vec!` literal ahead is expected to
    /// have when the expression is expected to be `expected`, as rustc's
    /// expectation reaches it: `[..]` and `vec![..]` of a sequence, and
    /// `&[..]` of a reference to one (`&[&A, &B]` as a `&[&dyn Named]`).
    /// A `&vec![..]` reaches only a `&Vec<T>`: a `&[T]` is its deref, which
    /// carries no expectation.
    pub(super) fn literal_element(&mut self, expected: &Ty) -> Option<Ty> {
        if self.split_angle.is_some() {
            return None;
        }
        let mut ahead = self.input.clone().map(|(token, _)| token);
        let mut expected = self.inference.resolve(expected);
        let mut token = ahead.next()?;
        let referenced = matches!(token, Token::Ampersand);
        if referenced {
            let Ty::Reference { target, .. } = expected else {
                return None;
            };
            expected = self.inference.resolve(&target);
            token = ahead.next()?;
            if matches!(token, Token::Mut) {
                token = ahead.next()?;
            }
        }
        let vec = match token {
            Token::LeftBracket => false,
            Token::Identifier(name) if name.as_str() == "vec" => {
                matches!(ahead.next(), Some(Token::Bang)).then_some(true)?
            }
            _ => return None,
        };
        let Ty::Array { element, kind } = expected else {
            return None;
        };
        (!(referenced && vec) || kind == sequence::Kind::Vec).then_some(*element)
    }

    /// The statement ahead, a block-like tail of its block expected to be
    /// `expected` as rustc expects it: its arms coerce to that type.
    pub(super) fn parse_tail_expecting(&mut self, expected: Option<&Ty>) -> Result<(), Error> {
        let expected = expected.filter(|_| self.peek_block_like() && self.block_like_ends_block());
        let Some(expected) = expected else {
            return self.parse_statement();
        };
        let hint = Some((expected.clone(), self.expression_stack.len() + 1));
        let outer = std::mem::replace(&mut self.collect_hint, hint);
        let statement = self.parse_statement();
        self.collect_hint = outer;
        statement
    }

    /// Whether the block-like expression ahead (`if .. {} else {}`, `match
    /// .. {}`, `loop {}`, `{}`) is its block's tail: the enclosing `}`
    /// follows its last block.
    pub(super) fn block_like_ends_block(&mut self) -> bool {
        let mut ahead = self.input.clone().map(|(token, _)| token);
        let mut depth = 0_usize;
        while let Some(token) = ahead.next() {
            match token {
                Token::LeftBracket | Token::LeftParen | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                // A block at the expression's own level closes: an `else`
                // goes on to the next one.
                Token::RightBrace if depth == 1 => {
                    depth = 0;
                    match ahead.next() {
                        Some(Token::Else) => {}
                        next => return matches!(next, Some(Token::RightBrace)),
                    }
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace if depth > 0 => {
                    depth -= 1;
                }
                Token::RightBracket
                | Token::RightParen
                | Token::RightBrace
                | Token::SemiColon
                | Token::EOF
                    if depth == 0 =>
                {
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    /// Whether the expression ahead ends at the enclosing block's `}`: its
    /// first bracket's match is followed by it, as a body's tail literal
    /// `vec![..]` is.
    pub(super) fn literal_ends_block(&mut self) -> bool {
        let mut ahead = self.input.clone().map(|(token, _)| token);
        let mut depth = 0_usize;
        while let Some(token) = ahead.next() {
            match token {
                Token::LeftBracket | Token::LeftParen | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace if depth > 1 => {
                    depth -= 1;
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace if depth == 1 => {
                    return matches!(ahead.next(), Some(Token::RightBrace));
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace | Token::EOF => {
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    /// Parse an expression, on a stack grown when it runs low: Rust limits
    /// no expression's nesting, so the native stack mustn't (#962).
    pub(super) fn parse_expr(&mut self) -> Result<ParsedExpr, Error> {
        crate::eval::stack::with_room(|| self.parse_expr_in(false))
    }

    pub(super) fn peek_block_like(&mut self) -> bool {
        match self.peek() {
            Token::If
            | Token::While
            | Token::Loop
            | Token::For
            | Token::LeftBrace
            | Token::Apostrophe => true,
            Token::Reserved(keyword) => keyword.as_str() == "match",
            _ => false,
        }
    }

    /// A statement that starts with a block-like expression ends with it
    /// (`block`), as in Rust; otherwise this parses a full expression.
    pub(super) fn parse_expr_in(&mut self, block: bool) -> Result<ParsedExpr, Error> {
        let path = match self.expression_stack.last() {
            Some(parent) => {
                let child = self
                    .expression_child_indexes
                    .last_mut()
                    .expect("every active expression path retains its next child index");
                let index = *child;
                *child = child
                    .checked_add(1)
                    .expect("one expression cannot contain more than u32::MAX children");
                let mut path = parent.to_vec();
                path.push(index);
                path.into_boxed_slice()
            }
            None => {
                let index = self.root_expression_index;
                self.root_expression_index = self
                    .root_expression_index
                    .checked_add(1)
                    .expect("one callable cannot contain more than u32::MAX root expressions");
                vec![index].into_boxed_slice()
            }
        };
        self.expression_stack.push(path.clone());
        self.expression_child_indexes.push(0);
        let expression = if block {
            // As in Rust, `match self { .. }.fmt(f)` and `{ .. }?` go on
            // after the block. A `(` or `[` there starts a new statement.
            self.parse_primary().and_then(|value| {
                let continues = match self.peek() {
                    Token::Period => true,
                    Token::Reserved(keyword) => keyword.as_str() == "?",
                    _ => false,
                };
                if continues {
                    self.parse_postfix_of(value)
                } else {
                    Ok(value)
                }
            })
        } else {
            self.parse_range()
        };
        self.expression_stack.pop();
        self.expression_child_indexes.pop();
        let expression = expression?;
        self.expression_paths.push((expression.span, path));
        Ok(expression)
    }

    /// `&mut place` of a `Box<dyn Iterator>` place given whole as an
    /// argument, as `column(&mut self.content)` or `.chain(&mut
    /// self.actions)`: the iterator by `&mut`, which the callee runs to
    /// its end, so its items move out and the place is left empty. Any
    /// other `&mut` outside a function's `&mut` argument is refused.
    fn drained_iterator(&mut self, start: Position) -> Result<ParsedExpr, Error> {
        self.next();
        let operand = self.parse_operand()?;
        let whole_argument = matches!(self.peek(), Token::Comma | Token::RightParen);
        let place = operand.lvalue.is_some();
        let iterator = match self.inference.resolve(&operand.ty) {
            Ty::Boxed(boxed) => self.inference.resolve(&boxed),
            ty => ty,
        };
        let (Ty::Iter(item, kind), true, true) = (iterator, whole_argument, place) else {
            return Err(self
                .unsupported("`&mut` borrow expressions other than a function's `&mut` argument"));
        };
        self.mutable_use(&operand, operand.span);
        let drained = self.mutation_call(
            &operand,
            "drain",
            vec!["0".into(), crate::INT::MAX.to_string().into()],
        )?;
        Ok(self.operator_expr(
            drained,
            Ty::Iter(item, kind),
            span(start, operand.span.end()),
        ))
    }

    pub(super) fn parse_operand(&mut self) -> Result<ParsedExpr, Error> {
        // `&&x`, which the lexer reads as one `&&`: `& &x`.
        if matches!(self.peek(), Token::And) {
            let at = self.next().1;
            let mut second = at;
            second.advance();
            self.splice_tokens(vec![(Token::Ampersand, at), (Token::Ampersand, second)]);
        }
        if matches!(self.peek(), Token::Ampersand) {
            let start = self.next().1;
            if matches!(self.peek(), Token::Mut) {
                // `&mut place` as a function's `&mut` argument, which is
                // the call's receiver; a by-value runtime cannot keep one
                // in a local.
                // `index_mut`'s `&mut place` is its value; writes reach the
                // place where it is inlined (`indexed_place`).
                let index_mut = self.place_output
                    || matches!(&self.return_ty, Some(Ty::Reference { mutable: true, .. }));
                if !index_mut && self.mutable_borrow != Some(self.expression_stack.len()) {
                    return self.drained_iterator(start);
                }
                self.next();
                let operand_start = self.position();
                let lent = self.lent_mut_field.replace(operand_start);
                let lone = self.lone_name_ahead();
                let operand = self.parse_operand();
                self.lent_mut_field = lent;
                let mut operand =
                    operand.map_err(|error| self.borrowed_operand(error, lone, start))?;
                let whole = span(start, operand.span.end());
                if operand.lvalue.is_none() {
                    // `twice(&mut |x| v.push(x))`: the closure is the
                    // borrowed temporary, and the callee calls it where it
                    // is, as its captures are where the closure reaches them.
                    // So is a crate's value, as `&mut shell::Bus::new()`
                    // fills iced's `update(.., shell: &mut Bus<_>)`: native
                    // code changes it where it is, and it drops at the
                    // statement's end, as Rust drops the temporary.
                    let host = matches!(self.inference.resolve(&operand.ty), Ty::Host { .. });
                    if !host && !matches!(self.inference.resolve(&operand.ty), Ty::FnItem { .. }) {
                        return Err(self.unsupported("`&mut` of a value that is not a place"));
                    }
                    self.borrow_temporary_mut(&operand, whole);
                    if host {
                        self.hold_temporary(&mut operand)?;
                    }
                    operand.source = operand.rendered_source().surrounded("(", ")");
                    operand.prelude = origin::Fragment::default();
                    operand.ty = Ty::Reference {
                        mutable: true,
                        target: Box::new(self.inference.resolve(&operand.ty)),
                    };
                    operand.span = whole;
                    return Ok(operand.retain_origin());
                }
                let inner = self.may_hold_borrow(&operand.ty);
                let reference = borrows::is_reference(&self.inference.resolve(&operand.ty));
                self.flow().borrow(
                    operand.span.start(),
                    operand.span,
                    whole,
                    true,
                    inner,
                    reference,
                );
                self.flow().alias(operand.span.start(), start);
                operand.ty = Ty::Reference {
                    mutable: true,
                    target: Box::new(self.inference.resolve(&operand.ty)),
                };
                operand.span = whole;
                return Ok(operand.retain_origin());
            }
            let operand_start = self.position();
            let lent = self.lent_field.replace(operand_start);
            let lone = self.lone_name_ahead();
            let operand = self.parse_operand();
            self.lent_field = lent;
            let mut operand = operand.map_err(|error| self.borrowed_operand(error, lone, start))?;
            let whole = span(start, operand.span.end());
            if operand.lvalue.is_some() || self.flow().is_place(operand.span) {
                let inner = self.may_hold_borrow(&operand.ty);
                let reference = borrows::is_reference(&self.inference.resolve(&operand.ty));
                self.flow().borrow(
                    operand.span.start(),
                    operand.span,
                    whole,
                    false,
                    inner,
                    reference,
                );
            } else {
                // `&String::from("a")`, `&(n + 1)`: a borrow of a temporary.
                self.borrow_temporary(&operand, whole);
            }
            self.hold_temporary(&mut operand)?;
            // `&*s` of a `String`: a `&str`.
            operand.ty = match self.inference.resolve(&operand.ty) {
                Ty::UnsizedStr => Ty::Str,
                target => Ty::Reference {
                    mutable: false,
                    target: Box::new(target),
                },
            };
            operand.span = span(start, operand.span.end());
            // `&s.a.b`: the field chain a call may lend (`lend_argument`).
            if let Some(field) = fields_below_local(&operand) {
                self.borrowed_fields.insert(operand.span, field);
            }
            operand.lvalue = None;
            return Ok(operand.retain_origin());
        }
        if matches!(self.peek(), Token::Multiply) {
            let start = self.next().1;
            let operand = self.parse_operand()?;
            // `*r` reads what the holder `r` refers to: its use's place.
            self.flow().deref(operand.span.start(), start);
            let had_place = operand.lvalue.is_some() || self.flow().is_place(operand.span);
            // `*w` of a program type is `*Deref::deref(&w)`, and
            // `*DerefMut::deref_mut(&mut w)` where it is assigned, as
            // rustc's place deref is.
            let (mut operand, dereferenced) = self.program_deref(operand)?;
            let resolved = self.inference.resolve(&operand.ty);
            let target = match resolved.clone() {
                Ty::Reference { target, .. } | Ty::Boxed(target) | Ty::Counted(_, target) => target,
                // `*s` of a `&str` is the `str` it refers to.
                Ty::Str => Box::new(Ty::UnsizedStr),
                // `*s` of a `String` or a `Cow`: its `Deref` target, from
                // std's impls.
                ty => match self.deref_target(&ty, false) {
                    Some((
                        target,
                        call @ (std_impl::DerefCall::Value | std_impl::DerefCall::Cow),
                    )) => {
                        self.apply_derefs(&mut operand, vec![call], ty)?;
                        Box::new(target)
                    }
                    _ => {
                        return Err(
                            self.unsupported("`*` on a value that is not a reference or box")
                        )
                    }
                },
            };
            // `*v = x` through a `&mut` local or place (a map's entry, an
            // `and_modify` value) writes it.
            // `*self = x` in a `&mut self` method replaces the receiver.
            let receiver = operand.lvalue.is_none()
                && self.closures.is_empty()
                && operand.source.as_str() == "self"
                && self.bindings.get("self").is_some_and(|binding| {
                    binding.mutable && matches!(binding.kind, BindingKind::Receiver { .. })
                });
            if receiver {
                operand.lvalue = Some(Lvalue::Binding {
                    binding: "self".into(),
                    ty: Ty::Unknown,
                });
            }
            let through_reference = matches!(resolved, Ty::Reference { .. });
            operand.lvalue = match (resolved, operand.lvalue.take()) {
                (Ty::Reference { mutable: true, .. }, Some(Lvalue::Binding { binding, .. })) => {
                    Some(Lvalue::Binding {
                        binding,
                        ty: (*target).clone(),
                    })
                }
                (
                    Ty::Reference { mutable: true, .. },
                    Some(Lvalue::Place { binding, place, .. }),
                ) => Some(Lvalue::Place {
                    binding,
                    ty: (*target).clone(),
                    place,
                }),
                (Ty::Reference { mutable: true, .. }, Some(Lvalue::Nested(mut nested))) => {
                    nested.ty = (*target).clone();
                    Some(Lvalue::Nested(nested))
                }
                // `*b` of a local `Box`: the box's own place, which a use
                // by value moves, with drop glue in the crate.
                (Ty::Boxed(_), Some(Lvalue::Binding { binding, .. })) if self.drops() => {
                    Some(Lvalue::Binding {
                        binding,
                        ty: (*target).clone(),
                    })
                }
                _ => None,
            };
            // `*rc` reads the value through std's `Deref`.
            if matches!(self.inference.resolve(&operand.ty), Ty::Counted(..)) {
                self.counted_value(&mut operand);
            }
            let base = (operand.span, operand.ty.clone());
            operand.ty = *target;
            operand.span = span(start, operand.span.end());
            // A move out of `*r` or `*rc` is rustc's E0507; a `Box` gives
            // its value up.
            let behind = match self.inference.resolve(&base.1) {
                _ if let Some(owner) = &dereferenced => Some(format!("dereference of `{owner}`")),
                Ty::Reference { mutable: true, .. } => {
                    Some("a value behind a mutable reference".into())
                }
                Ty::Reference { .. } => Some("a value behind a shared reference".into()),
                Ty::Counted(Counter::Rc, _) => Some("a value of an `Rc`".into()),
                Ty::Counted(Counter::Arc, _) => Some("a value of an `Arc`".into()),
                _ => None,
            };
            if let Some(behind) = behind {
                let assigned = match self.inference.resolve(&base.1) {
                    Ty::Counted(counter, _) => Some(counter.unassignable()),
                    _ => None,
                };
                self.unmovable.insert(
                    operand.span,
                    Unplaced {
                        moved: ("E0507", format!("cannot move out of {behind}")),
                        assigned,
                    },
                );
            }
            // `*x` of a `&&T` is the `&T` it refers to.
            self.part_of(
                (base.0, &base.1),
                operand.span,
                &operand.ty,
                borrows::PartOf::Deref,
            );
            if had_place {
                self.flow().mark_place(operand.span);
            }
            if through_reference && operand.lvalue.is_none() && self.drops() {
                self.borrowed.insert(operand.span);
            }
            return Ok(operand.retain_origin());
        }
        if matches!(self.peek(), Token::Minus | Token::UnaryMinus | Token::Bang) {
            self.parse_unary()
        } else {
            self.parse_postfix()
        }
    }

    pub(super) fn parse_unary(&mut self) -> Result<ParsedExpr, Error> {
        let (operator, allowed) = match self.peek() {
            Token::Bang => ("!", [Ty::Bool, Ty::Int(Int::I64)]),
            _ => ("-", [Ty::Int(Int::I64), Ty::Float(Float::F64)]),
        };
        let start = self.next().1;
        let operand = self.parse_operand()?;
        let symbol = if operator == "-" { "neg" } else { "!" };
        if let Some(mut value) = self.operator_impl(
            symbol,
            Some(span(start, operand.span.end())),
            &operand,
            None,
        )? {
            value.span = span(start, operand.span.end());
            return Ok(value);
        }
        let mut ty = self.inference.resolve(&operand.ty);
        // `-x` and `!x` of a `&i64` or `&bool`: std's `impl Neg for &i64`
        // and kin give the value.
        if let Ty::Reference { target, .. } = &ty
            && let target @ (Ty::Int(_) | Ty::Float(_) | Ty::Bool) = self.inference.resolve(target)
        {
            ty = target;
        }
        // `!x` of an integer flips its bits.
        if operator == "!"
            && let Ty::Int(_) = ty
        {
            let expression = span(start, operand.span.end());
            let source = self.int_op("!", operand.rendered_source(), "0".into(), &ty, &ty);
            return Ok(self.operator_expr(source, ty, expression));
        }
        // `-x` on an unsigned integer is E0600.
        if operator == "-"
            && let Ty::Int(kind) = ty
            && !kind.signed()
        {
            let operand_span = match &operand.lvalue {
                Some(Lvalue::Binding { binding, .. }) => self.name_span(operand.span, binding),
                _ => operand.span,
            };
            return Err(unsigned_negation(kind, span(start, operand_span.end())));
        }
        if !allowed_type(&allowed, &ty) {
            return Err(Error::TypeMismatch {
                expected: allowed[0].name(&self.models),
                actual: ty.name(&self.models),
                expression: operand.span,
            });
        }
        if let Ty::Int(kind) = ty
            && kind.wide()
        {
            let expression = span(start, operand.span.end());
            let source = self.int_op("neg", operand.rendered_source(), "0".into(), &ty, &ty);
            return Ok(self.operator_expr(source, ty, expression));
        }
        let source = operand
            .rendered_source()
            .surrounded(&format!("{operator}("), ")");
        let value = self.operator_expr(source, ty, span(start, operand.span.end()));
        Ok(if operator == "-" {
            self.note_known_negation(operand.span, &value);
            self.checked_arithmetic(value, "neg")
        } else {
            value
        })
    }

    pub(super) fn parse_primary(&mut self) -> Result<ParsedExpr, Error> {
        if matches!(self.peek(), Token::LeftBrace) {
            return self.parse_block_expr();
        }
        // `unsafe { .. }` is its block: Rawr has none of the operations
        // that need it, so what it holds is checked as anywhere else.
        if matches!(self.peek(), Token::Identifier(ref name) if name.as_str() == "unsafe")
            && matches!(self.peek_second(), Token::LeftBrace)
        {
            self.next();
            return self.parse_block_expr();
        }
        let (token, position) = self.next();
        let token_span = point_span(position);

        match token {
            Token::Unit => Ok(self.literal("()", Ty::Unit, token_span)),
            Token::True => Ok(self.literal("true", Ty::Bool, token_span)),
            Token::False => Ok(self.literal("false", Ty::Bool, token_span)),
            Token::IntegerConstant(value) => {
                let (ty, float) = self.literal_kind(value, true, token_span)?;
                let text = if float {
                    format!("{value}.0")
                } else {
                    value.to_string()
                };
                let literal = self.literal(text, ty, token_span);
                if !float {
                    self.note_known_literal(&literal, value);
                }
                Ok(literal)
            }
            Token::CharConstant(value) => {
                Ok(self.literal(format!("{value:?}"), Ty::Char, token_span))
            }
            Token::StringConstant(value) => {
                // A literal is `&'static str`: it borrows nothing of the body.
                self.flow().set_origins(token_span, Vec::new());
                Ok(self.literal(format!("{value:?}"), Ty::Str, token_span))
            }
            // `b"…"`: a `&[u8; N]`, an array of its bytes at runtime.
            Token::ByteStringConstant(bytes) => Ok(self.byte_literal(&bytes, token_span)),
            Token::ByteConstant(byte) => {
                Ok(self.literal(byte.to_string(), Ty::Int(Int::U8), token_span))
            }
            Token::FloatConstant(value)
                if let Some(literal) = self.wide_literal(&value.1, token_span)? =>
            {
                Ok(literal)
            }
            Token::FloatConstant(value) => {
                let ty = self.literal_kind(0, false, token_span)?.0;
                let number = *value.0;
                let text = match &ty {
                    // An `f32` holds the nearest `f32`, as Rust's literal.
                    Ty::Float(Float::F32) => format!("{:?}", number as f32 as f64),
                    // Its kind is known once inference settles.
                    Ty::Float(Float::Var(_)) => {
                        let placeholder =
                            format!("__rawr_float_literal_{:08}", self.pending_float_kinds.len());
                        self.pending_float_kinds.push(PendingFloat {
                            placeholder: placeholder.clone(),
                            ty: ty.clone(),
                            kind: PendingFloatKind::Literal(number),
                        });
                        placeholder
                    }
                    _ => value.1.to_string(),
                };
                Ok(self.literal(text, ty, token_span))
            }
            Token::LeftParen => {
                let outer = std::mem::replace(&mut self.no_struct, false);
                let value = self.parse_parenthesized(position);
                self.no_struct = outer;
                value
            }
            Token::If => self.parse_if(token_span),
            Token::While | Token::Loop | Token::For => self.parse_loop(token, token_span, None),
            Token::Apostrophe => {
                let (label, _) = self.expect_name("loop label")?;
                self.expect(Token::Colon, "':' after a loop label")?;
                let (token, position) = self.next();
                if !matches!(token, Token::While | Token::Loop | Token::For) {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "label on something other than a loop".into(),
                        source: point_span(position),
                    });
                }
                self.parse_loop(token, token_span, Some(label))
            }
            Token::Break | Token::Continue => self.parse_jump(token, token_span),
            Token::Return => self.parse_return(token_span),
            Token::Pipe | Token::Or => self.parse_closure(token, token_span, false),
            Token::Reserved(keyword) if keyword.as_str() == "async" => self.parse_async(token_span),
            Token::LeftBracket => self.parse_array(position, Token::RightBracket, Some(0)),
            Token::Reserved(keyword) if keyword.as_str() == "match" => self.parse_match(token_span),
            Token::Identifier(name) => self.parse_name_primary((*name).into(), token_span),
            // `::image::ImageReader::new(..)`: a leading `::` names an
            // outside crate, which no `use`, local or function shadows.
            Token::DoubleColon if matches!(self.peek(), Token::Identifier(_)) => {
                let (Token::Identifier(name), position) = self.next() else {
                    unreachable!("an identifier after `::`");
                };
                let name: ImmutableString = (*name).into();
                let name_span = point_span(position);
                if matches!(name.as_str(), "crate" | "Self")
                    || self.bindings.contains_key(&name)
                    || self.models.function(&name).is_some()
                    || !matches!(self.peek(), Token::DoubleColon)
                {
                    let reference = span(token_span.start(), name_span.end());
                    return Err(crate::resolve::not_a_crate(&name, reference));
                }
                let outer = self.models.global_head.replace(name.clone());
                let value = self.parse_name_primary(name, name_span);
                self.models.global_head = outer;
                value
            }
            // `<T as Trait>::f(..)`: a trait's function for the type `T`.
            Token::LessThan => {
                let self_ty = self.parse_local_type()?;
                self.expect(Token::As, "'as' in a qualified path")?;
                let (mut trait_name, mut trait_span) =
                    self.expect_name("trait in a qualified path")?;
                let root = trait_name.clone();
                let root_span = trait_span;
                let mut segments = 1_usize;
                let mut written = vec![trait_name.clone()];
                // `<T as std::default::Default>`: the path's last segment.
                while matches!(self.peek(), Token::DoubleColon)
                    && matches!(self.peek_second(), Token::Identifier(_))
                {
                    self.next();
                    (trait_name, trait_span) = self.expect_name("trait in a qualified path")?;
                    written.push(trait_name.clone());
                    segments += 1;
                }
                // `<C as Shape>` of the module's `Shape`, when another
                // module's trait has that name too.
                if segments == 1
                    && let Some(known) = self.models.trait_name(&trait_name)
                {
                    trait_name = known.clone();
                }
                // `<C as home::Shape>`: a crate trait through its module.
                let path = Span::new(root_span.start(), text_end(trait_span.start(), &trait_name));
                if segments > 1
                    && let Some(known) = self.models.qualified_trait(path).cloned()
                {
                    trait_name = known;
                    segments = 1;
                }
                // A path through a crate, as `serde::de::Error`, names the
                // crate's trait, never std's of the same name.
                // `fmt::Display` after `use std::fmt;` is std's.
                let root = self.models.import(&root).map_or_else(
                    || root.to_string(),
                    |binding| {
                        binding
                            .qualified
                            .split("::")
                            .next()
                            .unwrap_or_default()
                            .to_owned()
                    },
                );
                let through_crate = segments > 1 && !crate::std_table::is_root(&root);
                let explicit = if matches!(self.peek(), Token::LessThan) {
                    Some(self.parse_trait_path_arguments()?)
                } else {
                    None
                };
                self.expect_close_angle("'>' after a qualified path's trait")?;
                self.expect(Token::DoubleColon, "'::' after a qualified path")?;
                // `<u64 as common::cast::NumCast>::from(n)`: the member the
                // crate trait's impl for the scalar gives it
                // (`surface::scalar`).
                #[cfg(feature = "rust-import")]
                if explicit.is_none()
                    && let Some(path) = self.qualified_scalar_member(&self_ty, &written)
                {
                    return self.qualified_scalar_call(&path, position);
                }
                if !through_crate
                    && !self.ufcs_trait(&trait_name)
                    && let Some(std_name) = crate::resolve::std_trait::anywhere(&trait_name)
                        .and_then(|definition| definition.rsplit("::").next())
                {
                    return self.parse_typed_std_trait_call(
                        std_name,
                        Some(self_ty),
                        explicit,
                        position,
                    );
                }
                self.parse_trait_path_call(
                    Some(self_ty),
                    trait_name,
                    trait_span,
                    explicit,
                    position,
                )
            }
            Token::Reserved(keyword)
                if keyword.as_str() == "super" && matches!(self.peek(), Token::DoubleColon) =>
            {
                self.parse_qualified("super".into(), token_span)
            }
            // `self::m::f()`: a path from this module, never the receiver.
            Token::SelfValue if matches!(self.peek(), Token::DoubleColon) => {
                self.parse_qualified("self".into(), token_span)
            }
            Token::SelfValue => {
                // A free function's `&mut` parameter is `this` too.
                let Some(binding) = self
                    .bindings
                    .get("self")
                    .filter(|binding| matches!(binding.kind, BindingKind::Receiver { .. }))
                else {
                    return Err(Error::SelfOutsideImpl {
                        reference: token_span,
                        parameter: false,
                    });
                };
                let (ty, declaration) = (binding.ty.clone(), binding.declaration);
                // An owned `self` is a local of the method's body, as rustc
                // holds it: a move out of it or of a part of it is followed.
                if self.closures.is_empty() && self.owned_receiver() {
                    self.mention("self", declaration, &ty, token_span);
                }
                // A closure reads `self` through the local its outermost
                // closure binds (`parse_closure`): the runtime binds `this`
                // only in the method's own body.
                self.record_capture(&"self".into());
                let source = if self.closures.is_empty() {
                    "self"
                } else {
                    SELF_CAPTURE
                };
                Ok(ParsedExpr {
                    prelude: origin::Fragment::default(),
                    source: origin::Fragment::plain(source),
                    ty,
                    span: token_span,
                    lvalue: None,
                    outcome: crate::host::usage::Outcome::Conservative,
                    evaluation: crate::host::usage::Evaluation::Required,
                }
                .retain_origin())
            }
            // `try { .. } catch ..`: rustc reads a `try` block, which no
            // `catch` may follow.
            Token::Try if let Some(at) = self.catch_after_block() => Err(Error::Syntax {
                message: "keyword `catch` cannot follow a `try` block".into(),
                label: "".into(),
                help: Some("try using `match` on the result of the `try` block instead".into()),
                at: Span::new(at, text_end(at, "catch")),
            }),
            // `throw`, `do`, `#{ .. }` and the like: rustc's syntax error.
            token => {
                let found = crate::parser::item::found(&token);
                let written = found
                    .trim_start_matches("reserved ")
                    .trim_start_matches("keyword ")
                    .trim_matches('`');
                Err(Error::Syntax {
                    message: format!("expected expression, found {found}").into(),
                    label: "expected expression".into(),
                    help: None,
                    at: Span::new(token_span.start(), text_end(token_span.start(), written)),
                })
            }
        }
    }

    /// `{ statements; tail }`: a block whose bindings end with it and whose
    /// value is its tail, as in Rust.
    #[inline(never)]
    pub(super) fn parse_block_expr(&mut self) -> Result<ParsedExpr, Error> {
        let original = self.bindings.clone();
        let outer = std::mem::replace(&mut self.no_struct, false);
        let block = self.parse_match_action();
        self.no_struct = outer;
        let inner = std::mem::replace(&mut self.bindings, original);
        for (name, binding) in &mut self.bindings {
            // A binding the block shadowed keeps its own state.
            if let Some(after) = inner.get(name)
                && after.declaration == binding.declaration
            {
                binding.available = after.available;
            }
        }
        let block = block?;
        if matches!(self.inference.resolve(&block.value.ty), Ty::FnItem { .. }) {
            self.closure_blocks
                .insert(block.syntax, block.value.prelude.clone());
        }
        let source = block.value.rendered_block();
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: block.value.ty,
            span: block.syntax,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// `(value)`, or a tuple `(a, b, ..)`, which is an array at runtime.
    pub(super) fn parse_parenthesized(&mut self, open: Position) -> Result<ParsedExpr, Error> {
        // A tuple keeps its form when a `#[cfg(..)]` leaves elements out.
        let mut left_out = false;
        while self.element_left_out()? {
            left_out = true;
        }
        let mut value = self.parse_expr()?;
        if left_out && !matches!(self.peek(), Token::Comma) {
            let end = self.expect(Token::RightParen, "')' after a tuple")?;
            self.move_local(&mut value)?;
            return Ok(self.tuple_of(open, end, vec![value]));
        }
        if !matches!(self.peek(), Token::Comma) {
            let end = self.expect(Token::RightParen, "')' after expression")?;
            let before = value.source.as_str().to_string();
            value.source = value.source.surrounded("(", ")");
            self.flow().same_origins(span(open, end), value.span);
            if let Some(unmovable) = self.unmovable.get(&value.span).cloned() {
                self.unmovable.insert(span(open, end), unmovable);
            }
            self.note_known_parenthesized(value.span, span(open, end));
            value.span = span(open, end);
            value.lvalue = None;
            value.source.cover(value.span);
            self.keep_pipeline(&before, &value);
            return Ok(value);
        }
        let mut members = vec![value];
        while matches!(self.peek(), Token::Comma) {
            self.next();
            while self.element_left_out()? {}
            if matches!(self.peek(), Token::RightParen) {
                break;
            }
            members.push(self.parse_expr()?);
        }
        let end = self.expect(Token::RightParen, "')' after a tuple")?;
        for member in &mut members {
            self.move_local(member)?;
        }
        Ok(self.tuple_of(open, end, members))
    }

    /// The tuple of `members`, which have moved into it, written from
    /// `open` to `end`.
    pub(super) fn tuple_of(
        &mut self,
        open: Position,
        end: Position,
        members: Vec<ParsedExpr>,
    ) -> ParsedExpr {
        let ty = Ty::Tuple(
            members
                .iter()
                .map(|member| self.inference.resolve(&member.ty))
                .collect(),
        );
        let parts: Vec<_> = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                (
                    member,
                    borrows::Slot::At(borrowck::Path::of(&[borrowck::Step::Arg(index as u8)])),
                )
            })
            .collect();
        self.compose(span(open, end), &ty, &parts);
        self.tuple_fields.insert(
            span(open, end),
            members.iter().map(|member| member.span).collect(),
        );
        let source = origin::Fragment::join(members.iter().map(ParsedExpr::rendered_source), ", ")
            .surrounded("[", "]");
        self.operator_expr(source, ty, span(open, end))
    }

    pub(super) fn parse_name_primary(
        &mut self,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if name.starts_with(SENTINEL)
            || name.starts_with(METHOD_SENTINEL)
            || name.starts_with(NOMINAL_MATCH_SENTINEL)
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "compiler-private nominal intrinsic".into(),
                source: name_span,
            });
        }
        // `std::concat!(..)`, or a crate's re-export of it, as tracing's
        // `$crate::__macro_support::concat!(..)`: std's macro by its name.
        let std_macro = std::mem::take(&mut self.std_macro_ahead);
        if !std_macro && let Some(name) = self.std_macro_path(&name, name_span) {
            self.std_macro_ahead = true;
            self.splice_tokens(vec![(
                Token::Identifier(Box::new(name.as_str().into())),
                name_span.start(),
            )]);
            return self.parse_primary();
        }
        // `Self::with_text_size(..)` in an impl for a crate's type is the
        // type's own function, by the path the impl writes (#1466).
        if name.as_str() == "Self"
            && matches!(self.peek(), Token::DoubleColon)
            && let Some(path) = self.self_path.clone()
        {
            let at = name_span.start();
            let mut tokens = Vec::new();
            for segment in path.split("::") {
                if !tokens.is_empty() {
                    tokens.push((Token::DoubleColon, at));
                }
                tokens.push((Token::Identifier(Box::new(segment.into())), at));
            }
            self.splice_tokens(tokens);
            return self.parse_primary();
        }
        if crate::std_table::is_root(&name) && matches!(self.peek(), Token::DoubleColon) {
            self.check_std_path(&name, name_span)?;
        }
        // `S::Error::custom(..)` or `S::f(..)` of a type parameter native
        // code fills: the function of its opaque type (#593).
        #[cfg(feature = "rust-import")]
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some(path) = self.opaque.get(&name).cloned()
        {
            self.next();
            let (first, first_span) = self.expect_qualified_name()?;
            let projected = rawr_import::surface::opaque::parameter_projection(&path, &first)
                .filter(|projected| self.models.host.source_type(projected).is_some());
            let (owner, member, member_span) = match projected {
                Some(projected) if matches!(self.peek(), Token::DoubleColon) => {
                    self.next();
                    let (member, member_span) = self.expect_qualified_name()?;
                    (projected, member, member_span)
                }
                _ => (path.to_string(), first, first_span),
            };
            return self.opaque_function_call(&owner, &member, member_span);
        }
        if matches!(self.peek(), Token::DoubleColon) && !self.bindings.contains_key(name.as_str()) {
            self.unbound_std_type(&name, true, name_span)?;
        }

        // `Type::<A>::member`: the path without its arguments, its value
        // of the type `Type<A>` when it is one (a constructor).
        if matches!(self.peek(), Token::DoubleColon)
            && let Some(self_ty) = self.take_path_arguments(&name, name_span)?
        {
            let value = self.parse_name_primary(name, name_span)?;
            let same_head = match (&self.inference.resolve(&value.ty), &self_ty) {
                (Ty::Script(value, _), Ty::Script(written, _)) => value == written,
                (Ty::Keyed(value, _), Ty::Keyed(written, _)) => value == written,
                // `Task::<Message>::none()`: a facade type's arguments.
                (Ty::Host { proof: value, .. }, Ty::Host { proof: written, .. }) => {
                    value.definition.path() == written.definition.path()
                }
                (Ty::Array { .. }, Ty::Array { .. })
                | (Ty::Option(_), Ty::Option(_))
                | (Ty::Result(..), Ty::Result(..))
                | (Ty::Boxed(..), Ty::Boxed(..))
                | (Ty::Counted(..), Ty::Counted(..))
                | (Ty::Weak(..), Ty::Weak(..)) => true,
                _ => false,
            };
            if same_head {
                self.require_type(&self_ty, &value.ty, value.span)?;
            }
            return Ok(value);
        }

        // The program's own macro: textual scope shadows the prelude's.
        if !std_macro
            && matches!(self.peek(), Token::Bang)
            && let Some(rules) = self.program_macro(&name, name_span.start())
        {
            let rules = rules?;
            return self.expand_program_macro(&rules, name_span);
        }
        // std's macro, by its name or its path.
        let unbound = std_macro || self.models.import(&name).is_none();
        // `format_args!(..)` is the text it formats, as rawr holds std's
        // other `Display` values (`Path::display`), of core's
        // `fmt::Arguments` type, apart from a `String` (#1561).
        if matches!(self.peek(), Token::Bang)
            && matches!(name.as_str(), "format" | "format_args")
            && unbound
        {
            self.next();
            let mut formatted = self.parse_format(name_span, false)?;
            if name.as_str() == "format_args" {
                formatted.ty = Ty::Arguments;
            }
            return Ok(formatted);
        }
        // `print!`, `println!`, `eprint!`, `eprintln!`.
        if matches!(self.peek(), Token::Bang)
            && let Some(stream) = match name.as_str() {
                "print" => Some((false, false)),
                "println" => Some((false, true)),
                "eprint" => Some((true, false)),
                "eprintln" => Some((true, true)),
                _ => None,
            }
            && unbound
        {
            return self.parse_print(stream, name_span);
        }
        if matches!(self.peek(), Token::Bang) && name.as_str() == "dbg" && unbound {
            return self.parse_dbg(name_span);
        }
        // `write!(s, ..)` and `writeln!(&mut s, ..)` into a `String`, through
        // `std::fmt::Write`: the text pushed onto the place, and `Ok(())`.
        if matches!(self.peek(), Token::Bang)
            && matches!(name.as_str(), "write" | "writeln")
            && unbound
            && !self.writes_into_formatter()
        {
            return self.parse_write_string(&name, name_span);
        }
        // `write!(f, ..)` and `writeln!(f, ..)` into a `fmt` body's formatter.
        if matches!(self.peek(), Token::Bang)
            && matches!(name.as_str(), "write" | "writeln")
            && let Some(formatter) = self.formatter.clone()
        {
            self.next();
            self.expect(Token::LeftParen, "'(' after `write!`")?;
            let (target, target_span) = self.expect_name("the formatter of `write!`")?;
            if target != formatter {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "`write!` into anything but the `fmt` formatter".into(),
                    source: target_span,
                });
            }
            let newline = if name.as_str() == "writeln" {
                " + \"\\n\""
            } else {
                ""
            };
            let (text, end) = if matches!(self.peek(), Token::Comma) {
                self.next();
                let text = self.parse_format(name_span, true)?;
                let end = text.span.end();
                (text.rendered_source(), end)
            } else {
                let end = self.expect(Token::RightParen, "')' after `writeln!`")?;
                (origin::Fragment::plain("\"\""), end)
            };
            let expression = span(name_span.start(), end);
            let mut source = origin::Fragment::plain(format!("{{ {formatter} += ("));
            source.push(text);
            source.push_text(format!("){newline}; "));
            let ok = self.prelude_op(PreludeOp::Ok, ["()".into()]);
            source.push(ok);
            source.push_text(" }");
            return Ok(self.operator_expr(
                source,
                Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Unit)),
                expression,
            ));
        }
        if matches!(self.peek(), Token::Bang) && PANIC_MACROS.contains(&name.as_str()) && unbound {
            self.next();
            return self.parse_panic_macro(&name, name_span);
        }
        if matches!(self.peek(), Token::Bang) && name.as_str() == "vec" && unbound {
            self.next();
            let (open, close) = match self.next() {
                (Token::LeftBracket, position) => (position, Token::RightBracket),
                (Token::LeftParen, position) => (position, Token::RightParen),
                (_, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`vec!` without `[` or `(`".into(),
                        source: point_span(position),
                    })
                }
            };
            return self.parse_array(open, close, None);
        }
        if matches!(self.peek(), Token::Bang)
            && matches!(
                name.as_str(),
                "concat"
                    | "env"
                    | "option_env"
                    | "include_str"
                    | "include_bytes"
                    | "stringify"
                    | "line"
                    | "column"
                    | "file"
                    | "module_path"
            )
            && unbound
        {
            self.next();
            return self.parse_compile_time_macro(&name, name_span);
        }
        if matches!(self.peek(), Token::Bang) && name.as_str() == "cfg" && unbound {
            self.next();
            let open = self.expect(Token::LeftParen, "`(` after `cfg!`")?;
            let value = self.cfg_predicate()?;
            let close = self.expect(Token::RightParen, "`)` to close `cfg!`")?;
            let _ = open;
            return Ok(self.literal(
                if value { "true" } else { "false" },
                Ty::Bool,
                span(name_span.start(), close),
            ));
        }
        // `join!`, `try_join!` and `select!` after `use tokio::join;`.
        if !std_macro
            && matches!(self.peek(), Token::Bang)
            && let Some(binding) = self.models.import(&name)
            && let Some(name) = crate::resolve::tokio_macro(&binding.qualified, &self.models.host)
        {
            return self.parse_tokio_macro(name, name_span);
        }
        if matches!(self.peek(), Token::Bang) && name.as_str() == "matches" && unbound {
            self.next();
            return self.parse_matches(name_span);
        }
        // `RefCell::new(value)` and `RefCell::default()`, and `Cell`'s.
        if let Some((skip, kind)) = self.cell_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            return self.parse_cell_constructor(name_span, kind);
        }
        // `PhantomData`, also `PhantomData::<T>` or in full: a value that is
        // nothing at run time.
        if let Some(skip) = self.phantom_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            let mut end = name_span.end();
            if matches!(self.peek(), Token::DoubleColon)
                && matches!(self.peek_second(), Token::LessThan)
            {
                self.next();
                self.parse_phantom_type()?;
                end = self.position();
            }
            return Ok(self.literal("()", Ty::Unit, span(name_span.start(), end)));
        }
        // `spawn(f)` and `channel()` imported from `std::thread` and
        // `std::sync::mpsc`, unless a local or the program names them.
        if !self.bindings.contains_key(name.as_str())
            && self.models.function(&name).is_none()
            && matches!(
                self.peek(),
                Token::LeftParen | Token::Unit | Token::DoubleColon
            )
            && let Some(binding) = self.models.import(&name)
        {
            match binding.qualified.as_str() {
                "std::thread::spawn" => return self.parse_thread_spawn(name_span.start()),
                "std::thread::scope" => return self.parse_thread_scope(name_span.start()),
                "std::sync::mpsc::channel" => {
                    return self.parse_channel(name_span.start(), thread::Bound::Unbounded);
                }
                "std::sync::mpsc::sync_channel" => {
                    return self.parse_channel(name_span.start(), thread::Bound::Bounded);
                }
                _ => {}
            }
        }
        // `std::thread::sleep(duration)`, `thread::sleep`, `sleep`.
        if let Some(skip) = self.sleep_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            let (mut arguments, end) = self.parse_call_arguments("`sleep` arguments")?;
            let call = span(name_span.start(), end);
            let [duration] = arguments.as_mut_slice() else {
                return Err(Error::argument_count(
                    Callee::Function,
                    crate::resolve::Qualified("std::thread", "sleep").to_string(),
                    1,
                    arguments.len(),
                    call,
                ));
            };
            self.coerce_expr(&Ty::Time(Time::Duration), duration)?;
            let code = time_code("sleep").to_string();
            let source = self.prelude_op(
                PreludeOp::Time,
                ["()".into(), code.into(), duration.rendered_source()],
            );
            return Ok(self.operator_expr(source, Ty::Unit, call));
        }
        // `std::mem::swap(&mut a, &mut b)`, `mem::replace`, `take`.
        if let Some((function, skip)) = self.mem_path(&name) {
            for _ in 0..skip {
                self.next();
            }
            if function.starts_with("size_of") {
                return self.parse_size_of(function, name_span);
            }
            if function == "drop" {
                return self.parse_script_call("drop".into(), None, name_span);
            }
            return self.parse_mem_call(function, name_span);
        }
        // `std::convert::identity(x)`, and a std function named as a value,
        // as `.on_link_click(std::convert::identity)`.
        if let Some(function) = self.std_function(&name, name_span) {
            return self.parse_std_function(function);
        }
        // `Reverse(x)`, `std::cmp::Reverse(x)`.
        if self.reverse_path(&name) {
            return self.parse_reverse(name_span);
        }
        // `HashMap::new()`, `std::collections::BTreeSet::from(..)`.
        if let Some(kind) = self.keyed_path(&name) {
            return self.parse_keyed_constructor(kind, name_span);
        }
        if let Some(kind) = self.growable_path(&name) {
            self.next();
            let (member, member_span) = self.expect_member_name()?;
            // `Vec::len`, `Vec::is_empty(&v)`.
            let owner = Ty::sequence(kind, self.inference.fresh());
            let written = match kind {
                sequence::Kind::Deque => "VecDeque",
                _ => "Vec",
            };
            if let Some(value) =
                self.std_method_path(owner, written, &member, name_span, member_span)
            {
                return value;
            }
            let (arguments, end) = self.parse_call_arguments(&format!("`{name}` arguments"))?;
            // `Vec<T>` from `[T; N]`, or `VecDeque<T>` from a `Vec<T>` or
            // `[T; N]`, as std's `From` impls say: the same array at
            // runtime.
            if member.as_str() == "from"
                && let [value] = &arguments[..]
            {
                let call = span(name_span.start(), end);
                let ty = Ty::sequence(kind, self.inference.fresh());
                if self.std_conversion(&ty, &value.ty).is_none() {
                    let source = self.inference.resolve(&value.ty);
                    if let Some(error) = self.conversion_not_yet(&ty, &source, call) {
                        return Err(error);
                    }
                    return Err(Error::MissingCapability {
                        owner: format!("{name}<T>").into(),
                        capability: format!("From<{}>", source.name(&self.models)).into(),
                        operation: call,
                        derive: false,
                        required_by: None,
                    });
                }
                let ty = self.inference.resolve(&ty);
                return Ok(self.operator_expr(value.rendered_source(), ty, call));
            }
            let mut arguments = arguments;
            // `Vec::with_capacity(n)`: an empty `Vec` with room for `n`.
            if member.as_str() == "with_capacity"
                && let [capacity] = arguments.as_mut_slice()
            {
                self.coerce_expr(&Ty::Int(Int::Usize), capacity)?;
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let code = vec_method_code("with_capacity");
                let mut source = origin::Fragment::plain(format!("{helper}([], {code}, "));
                source.push(capacity.rendered_source());
                source.push_text(", (), false)");
                let ty = Ty::sequence(kind, self.inference.fresh());
                let at = span(name_span.start(), end);
                #[cfg(feature = "rust-import")]
                let source =
                    self.sequence_source(&ty, at, capacity.rendered_source(), "()".into(), 0);
                return Ok(self.operator_expr(source, ty, at));
            } else if member.as_str() != "new" || !arguments.is_empty() {
                return Err(Error::UnknownMember {
                    owner: name,
                    member,
                    reference: member_span,
                });
            }
            let ty = Ty::sequence(kind, self.inference.fresh());
            let at = span(name_span.start(), end);
            #[cfg(feature = "rust-import")]
            let source = self.sequence_source(&ty, at, "0".into(), "()".into(), 0);
            #[cfg(not(feature = "rust-import"))]
            let source = origin::Fragment::plain("[]");
            return Ok(self.operator_expr(source, ty, at));
        }
        // `Rc::strong_count(&rc)`, `Arc::new(value)`, `Weak::new()`: an
        // `Rc`'s, `Arc`'s or `Weak`'s associated function (`counted.rs`).
        if matches!(self.peek(), Token::DoubleColon)
            && let Some((counter, weak)) = self
                .pointer_named(&name)
                .and_then(Pointer::counter)
                .map(|counter| (counter, false))
                .or_else(|| self.weak_named(&name).map(|counter| (counter, true)))
        {
            self.next();
            let (member, member_span) = self.expect_member_name()?;
            return self.parse_counted_path(counter, weak, member, name_span, member_span);
        }
        // `Box::new(value)`: the value itself at runtime.
        if matches!(self.peek(), Token::DoubleColon)
            && let Some(Pointer::Box) = self.pointer_named(&name)
        {
            self.next();
            let (member, member_span) = self.expect_member_name()?;
            let unknown = || Error::UnknownMember {
                owner: "Box".into(),
                member: member.clone(),
                reference: member_span,
            };
            // `.map(Box::new)`: the function as a value.
            if member.as_str() == "new" && !matches!(self.peek(), Token::LeftParen | Token::Unit) {
                let input = self.inference.fresh();
                let output = Ty::Boxed(Box::new(input.clone()));
                let parameter = self.temporary("value");
                let source = origin::Fragment::plain(format!("|{parameter}| {parameter}"));
                let ty = Ty::Fn {
                    inputs: Box::new([input]),
                    output: Box::new(output),
                };
                return Ok(self.operator_expr(
                    source,
                    ty,
                    span(name_span.start(), member_span.end()),
                ));
            }
            let (arguments, end) = self.parse_call_arguments("`Box` arguments")?;
            let [argument] = <[ParsedExpr; 1]>::try_from(arguments).map_err(|_| unknown())?;
            let call = span(name_span.start(), end);
            if member.as_str() != "new" {
                return Err(unknown());
            }
            let ty = Ty::Boxed(Box::new(self.inference.resolve(&argument.ty)));
            // A value with drop glue moves into its `Box`.
            let mut argument = argument;
            if self.needs_drop(&ty, call)? {
                self.move_local(&mut argument)?;
            }
            let source = argument.rendered_source();
            return Ok(self.operator_expr(source, ty, call));
        }

        if matches!(self.peek(), Token::Bang) {
            let Some(binding) = self.models.import(&name).cloned() else {
                let scope =
                    match self
                        .models
                        .macros
                        .lookup(&name, self.models.context, name_span.start())
                    {
                        crate::resolve::macros::Lookup::Later(definition) => {
                            crate::resolve::MacroScope::Later(definition)
                        }
                        crate::resolve::macros::Lookup::Elsewhere => {
                            crate::resolve::MacroScope::Elsewhere
                        }
                        crate::resolve::macros::Lookup::Found(_)
                        | crate::resolve::macros::Lookup::Missing => {
                            crate::resolve::MacroScope::Undefined
                        }
                    };
                return Err(Error::UnknownMacro {
                    reference: Span::new(name_span.start(), text_end(name_span.start(), &name)),
                    name,
                    scope,
                });
            };
            let (item::Import::Item {
                definitions: items,
                owner,
            }
            | item::Import::ModuleItem {
                definitions: items,
                owner,
            }) = binding.target
            else {
                return Err(Error::InvalidHostOperation {
                    item: binding.qualified,
                    reason: "it is not a macro".into(),
                    operation: name_span,
                });
            };
            let items = macro_or_value(items.into_vec(), true).into_boxed_slice();
            return self.parse_host_macro(items, owner, name_span);
        }
        self.parse_name_value(name, name_span)
    }

    /// The rest of [`Self::parse_name_primary`], kept out of its frame: a
    /// nested `column![..]` recurses through that frame once per level.
    #[inline(never)]
    pub(super) fn parse_name_value(
        &mut self,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if name.as_str() == "move"
            && matches!(self.peek(), Token::Pipe | Token::Or)
            && !self.bindings.contains_key("move")
        {
            let token = self.next().0;
            return self.parse_closure(token, name_span, true);
        }

        // A local binding shadows an item of the same name, as in Rust.
        let local = self.bindings.contains_key(name.as_str())
            && !matches!(
                self.peek(),
                Token::DoubleColon | Token::LeftParen | Token::Unit
            );
        // A static its block declares shadows an item of the same name.
        if !local
            && !matches!(self.peek(), Token::DoubleColon)
            && let Some(read) = self.block_static(&name, name_span)
        {
            return read;
        }

        #[cfg(feature = "rust-import")]
        if let Some(binding) = self.models.import(&name).cloned().filter(|_| !local)
            && let item::Import::Type { definition, owner } = binding.target
            && let crate::host::type_def::Shape::Struct { form, .. } = definition.shape()
        {
            if matches!(form, crate::host::type_def::Form::Record)
                && matches!(self.peek(), Token::LeftBrace)
            {
                return self.parse_imported_struct(
                    binding.qualified,
                    definition,
                    owner,
                    &[],
                    name_span,
                );
            }
            if let Some(value) = self.imported_struct_value(
                binding.qualified,
                definition,
                owner,
                &[(&name, name_span)],
                name_span,
            )? {
                return Ok(value);
            }
        }
        // A crate's struct through the crate's type alias a `use` binds,
        // as `Owned { .. }` after `use natives::Owned;` (#1961).
        #[cfg(feature = "rust-import")]
        if matches!(self.peek(), Token::LeftBrace)
            && !self.no_struct
            && let Some(binding) = self.models.import(&name).cloned().filter(|_| !local)
            && matches!(binding.target, item::Import::Alias)
            && let Some(value) = self.parse_aliased_struct(&binding.qualified, name_span)?
        {
            return Ok(value);
        }

        if matches!(self.peek(), Token::LeftBrace)
            && !self.no_struct
            && !self.bindings.contains_key(name.as_str())
        {
            return self.parse_struct(name, name_span);
        }
        // `S` of `struct S;`, `W(x)` of `struct W(T);`, `W` uncalled.
        if !self.bindings.contains_key(name.as_str())
            && !matches!(self.peek(), Token::DoubleColon)
            && let Some(model) = self.named_model(&name)
            && let Some(value) = self.parse_struct_value(model, name_span)?
        {
            return Ok(value);
        }

        // `String::new()`, `String::from(x)`, `i64::from(true)`, and a crate
        // trait's functions for a builtin type (`i64::make()`).
        if matches!(self.peek(), Token::DoubleColon)
            && let Some(builtin) = builtin_path_type(&name)
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
        {
            return self.parse_builtin_path(builtin, name, name_span);
        }

        // The prelude's `Some(value)`, `None`, `Ok(value)` and `Err(error)`,
        // also as `Option::Some(value)` or `Result::Ok(value)`.
        let shadowed = |this: &Self, name: &str| {
            this.bindings.contains_key(name)
                || this.models.function(name).is_some()
                || this.models.get(name).is_some()
                || this.models.import(name).is_some()
        };
        // The prelude's `impl<T> From<T> for Option<T>`: `Option::from(x)`
        // is `Some(x)`.
        if name.as_str() == "Option"
            && matches!(self.peek(), Token::DoubleColon)
            && !shadowed(self, &name)
        {
            let mut ahead = self.input.clone();
            ahead.next();
            if let Some((Token::Identifier(member), _)) = ahead.next()
                && member.as_str() == "from"
            {
                self.next();
                let (_, member_span) = self.expect_member_name()?;
                let start = Span::new(name_span.start(), member_span.end());
                return self.parse_prelude_constructor(Prelude::Some, start);
            }
        }
        // `Cow::Borrowed(..)`, `std::borrow::Cow::Owned(..)`.
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some((member, member_span)) = self.cow_member_ahead(&name)
        {
            let start = Span::new(name_span.start(), member_span.end());
            return self.parse_cow_associated(&member, start);
        }
        if matches!(name.as_str(), "Option" | "Result")
            && matches!(self.peek(), Token::DoubleColon)
            && !shadowed(self, &name)
        {
            let mut ahead = self.input.clone();
            ahead.next();
            if let Some((Token::Identifier(variant), _)) = ahead.next()
                && let Some(variant) = Prelude::from_name(&variant)
                && variant.owner() == name.as_str()
            {
                self.next();
                let (_, variant_span) = self.expect_member_name()?;
                let start = Span::new(name_span.start(), variant_span.end());
                return self.parse_prelude_constructor(variant, start);
            }
        }
        if let Some(variant) = Prelude::from_name(&name)
            && !shadowed(self, name.as_str())
        {
            return self.parse_prelude_constructor(variant, name_span);
        }

        // `first::<i64>(..)`: a generic function's type arguments.
        if matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::LessThan)
            && !self.bindings.contains_key(name.as_str())
            && self
                .models
                .function(&name)
                .is_some_and(|function| !function.definition.generics.is_empty())
        {
            self.next();
            self.next();
            let mut types = vec![self.parse_local_type()?];
            while matches!(self.peek(), Token::Comma) {
                self.next();
                if matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                    break;
                }
                types.push(self.parse_local_type()?);
            }
            self.expect_close_angle("'>' after turbofish type arguments")?;
            self.pending_turbofish = Some(types);
        }
        // `T::default()` for a generic `T: Default`: the default of the type
        // its witness names.
        if matches!(self.peek(), Token::DoubleColon)
            && self.generics.contains_key(name.as_str())
            && !self.bindings.contains_key(name.as_str())
        {
            self.next();
            let param = Ty::Param(name.clone());
            // `T::name(&t)`: a method of a trait bounding `T`, called as
            // `<T as Trait>::name(&t)` is (T376).
            if let Some((Token::Identifier(member), position)) = self.input.clone().next()
                && let Some((trait_name, function)) =
                    self.generic_trait_function(&name, &member, point_span(position))?
                && function.receiver.is_some()
            {
                return self.parse_trait_path_call(
                    Some(param),
                    trait_name,
                    name_span,
                    None,
                    name_span.start(),
                );
            }
            let (member, member_span) = self.expect_member_name()?;
            // `T::new(..)`: a function without `self` of a trait bounding
            // `T`, run for the type its witness names.
            let function = self
                .generic_trait_function(&name, &member, member_span)?
                .filter(|(_, function)| function.receiver.is_none());
            // `T::SIDES`: an associated const is its value for `T`.
            let (arguments, end) = if function
                .as_ref()
                .is_some_and(|(_, function)| function.body.constant)
            {
                (Vec::new(), text_end(member_span.start(), &member))
            } else {
                self.parse_call_arguments("generic type function arguments")?
            };
            let call = span(name_span.start(), end);
            if let Some((trait_name, function)) = function {
                let witness = origin::Fragment::plain(witness_variable(&name));
                self.capture_witnesses();
                return self.call_static_trait_function(
                    param,
                    witness,
                    &trait_name,
                    &function,
                    arguments,
                    call,
                );
            }
            if member.as_str() != "default" || !arguments.is_empty() {
                return Err(Error::UnknownMember {
                    owner: name,
                    member,
                    reference: member_span,
                });
            }
            if !self.type_provides(&param, capability::Kind::Default) {
                return Err(Error::MissingCapability {
                    owner: name,
                    capability: "Default".into(),
                    operation: member_span,
                    derive: false,
                    required_by: None,
                });
            }
            self.capture_witnesses();
            let source = origin::Fragment::plain(format!(
                "{DEFAULT_DISPATCHER}({})",
                witness_variable(&name)
            ));
            return Ok(self.operator_expr(source, param, call));
        }
        // `Into::<String>::into(x)`, `From::<N>::from(x)`, `TryFrom::<N>`:
        // a prelude trait's argument written.
        if matches!(self.peek(), Token::DoubleColon)
            && self.peek_second() == Token::LessThan
            && let Some(std_name) = ["From", "Into", "TryFrom", "TryInto"]
                .into_iter()
                .find(|std_name| *std_name == name.as_str())
            && !self.ufcs_trait(&name)
            && self.models.get(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
        {
            self.next();
            let explicit = self.parse_trait_path_arguments()?;
            self.expect(Token::DoubleColon, "'::' after a trait's arguments")?;
            return self.parse_typed_std_trait_call(
                std_name,
                None,
                Some(explicit),
                name_span.start(),
            );
        }
        // `std::convert::Into::<String>::into(x)`: the same in full.
        if matches!(name.as_str(), "std" | "core")
            && matches!(self.peek(), Token::DoubleColon)
            && self.models.import(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
        {
            let mut ahead = self.input.clone();
            let mut segments = Vec::new();
            while matches!(ahead.peek(), Some((Token::DoubleColon, _))) {
                ahead.next();
                let Some((Token::Identifier(segment), _)) = ahead.peek() else {
                    break;
                };
                segments.push(segment.to_string());
                ahead.next();
            }
            if let [module, std_name] = &segments[..]
                && module == "convert"
                && let Some(std_name) = ["From", "Into", "TryFrom", "TryInto"]
                    .into_iter()
                    .find(|name| name == std_name)
                && matches!(ahead.peek(), Some((Token::LessThan, _)))
                && !self.ufcs_trait(std_name)
            {
                for _ in 0..5 {
                    self.next();
                }
                let explicit = self.parse_trait_path_arguments()?;
                self.expect(Token::DoubleColon, "'::' after a trait's arguments")?;
                return self.parse_typed_std_trait_call(
                    std_name,
                    None,
                    Some(explicit),
                    name_span.start(),
                );
            }
        }
        // `Duration::from_millis(5)`, `std::time::Instant::now()`.
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some((time, member, skip, end)) = self.std_time_member(&name)
        {
            for _ in 0..skip {
                self.next();
            }
            return self.parse_time_function(time, member, Span::new(name_span.start(), end));
        }
        // `Ordering::Less`, `std::cmp::Ordering::Greater`: its discriminant.
        if matches!(self.peek(), Token::DoubleColon)
            && !self.bindings.contains_key(name.as_str())
            && let Some((value, skip, end)) = self.ordering_variant(&name)
        {
            for _ in 0..skip {
                self.next();
            }
            return Ok(self.literal(
                format!("({value})"),
                Ty::Ordering,
                Span::new(name_span.start(), end),
            ));
        }
        // `.map(Result::ok)`, `.and_then(Option::as_ref)`: a method of
        // `Option` or `Result` named as a value is the closure that calls it.
        // `str::trim`, `Option::is_some(&x)`.
        if matches!(name.as_str(), "Option" | "Result" | "str")
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none()
            && !self.bindings.contains_key(name.as_str())
            && let Some(member) = self.std_method_member()
        {
            let owner = match name.as_str() {
                "Option" => Ty::Option(Box::new(self.inference.fresh())),
                "Result" => Ty::Result(
                    Box::new(self.inference.fresh()),
                    Box::new(self.inference.fresh()),
                ),
                _ => Ty::Str,
            };
            if std_method::path_function(&owner, &member).is_some() {
                self.next();
                let (member, member_span) = self.expect_member_name()?;
                if let Some(value) =
                    self.std_method_path(owner, &name, &member, name_span, member_span)
                {
                    return value;
                }
            }
        }
        // `Default::default()`, `From::from(x)`, `std::convert::Into::into(x)`:
        // a std or prelude trait's function called by path.
        if matches!(self.peek(), Token::DoubleColon)
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none_or(|binding| {
                binding.qualified.starts_with("std::") || binding.qualified.starts_with("core::")
            })
            && !self.bindings.contains_key(name.as_str())
            && !self.generics.contains_key(name.as_str())
            && let Some((trait_name, member, skip)) = self.std_trait_path(&name)
        {
            for _ in 0..skip {
                self.next();
            }
            return self.parse_std_trait_call(trait_name, member, name_span.start());
        }
        // `Shape::area(&c)`, `Convert::<i64>::convert(&c)`: a crate trait's
        // function called by path. A name another module's trait has too
        // is the trait this module declares or imports by it.
        let known = self.models.trait_name(&name).cloned();
        if matches!(self.peek(), Token::DoubleColon)
            && (known.is_some()
                || self.ufcs_trait(&name)
                    && self.models.get(&name).is_none()
                    && self.models.import(&name).is_none())
            && !self.bindings.contains_key(name.as_str())
        {
            let name = known.unwrap_or(name);
            self.next();
            let explicit = if matches!(self.peek(), Token::LessThan) {
                let arguments = self.parse_trait_path_arguments()?;
                self.expect(Token::DoubleColon, "'::' after a trait's arguments")?;
                Some(arguments)
            } else {
                None
            };
            return self.parse_trait_path_call(None, name, name_span, explicit, name_span.start());
        }
        if matches!(self.peek(), Token::DoubleColon) {
            return self.parse_qualified(name, name_span);
        }

        if matches!(self.peek(), Token::LeftParen | Token::Unit) {
            if self.bindings.contains_key(name.as_str()) {
                return self.parse_local_call(name, name_span);
            }
            // `min(a, b)` and `max(a, b)` imported from `std::cmp`.
            if let Some(binding) = self.models.import(&name)
                && let Some(member) = ["min", "max"].into_iter().find(|member| {
                    [
                        format!("std::cmp::{member}"),
                        format!("core::cmp::{member}"),
                    ]
                    .contains(&binding.qualified.to_string())
                })
            {
                return self.parse_std_trait_call("cmp", member, name_span.start());
            }
            if let Some(binding) = self.models.import(&name).cloned() {
                if let item::Import::Item {
                    definitions: items,
                    owner,
                }
                | item::Import::ModuleItem {
                    definitions: items,
                    owner,
                } = binding.target
                {
                    let items = value_or_call(macro_or_value(items.into_vec(), false), true)
                        .into_boxed_slice();
                    return self.parse_host_function(items, owner, name_span);
                }
                // `Degrees(90.0)`: a facade tuple struct's constructor, which
                // the facade declares as the type's `from`.
                if let item::Import::Type { definition, .. } = &binding.target
                    && definition.identity().imported().is_none()
                    && self
                        .models
                        .host_items(&format!("{}::from", definition.path()))
                        .is_some()
                {
                    let at = name_span.start();
                    self.splice_tokens(vec![
                        (Token::DoubleColon, at),
                        (Token::Identifier(Box::new("from".into())), at),
                    ]);
                    return self.parse_qualified(name, name_span);
                }
            }
            let function = self.models.function(&name).cloned();
            return self.parse_script_call(name, function, name_span);
        }

        if let Some(binding) = self.models.import(&name).cloned().filter(|_| !local)
            && let item::Import::Item {
                definitions: items,
                owner,
            }
            | item::Import::ModuleItem {
                definitions: items,
                owner,
            } = binding.target
        {
            // Not called, the name is in the value namespace, as rustc's:
            // `.map(text)` names the function a macro shares its name with.
            let items = value_or_call(macro_or_value(items.into_vec(), false), false);
            #[cfg(feature = "rust-import")]
            if items.len() == 1
                && items[0].imported_identity().is_some()
                && matches!(
                    items[0].kind(),
                    crate::host::item::Kind::Function | crate::host::item::Kind::AssociatedFunction
                )
            {
                // The name's own text, which the item's proof points at. A
                // std method Rawr runs takes it as the closure that calls it.
                let named = span(name_span.start(), text_end(name_span.start(), &name));
                return self.parse_host_function_or_item(items.into(), owner, named);
            }
            let [item] = items.as_slice() else {
                // `.map(text)`: an overloaded function as a value is the
                // closure `|a| text(a)`, whose call picks the overload.
                let arities = items
                    .iter()
                    .filter(|item| {
                        matches!(
                            item.kind(),
                            crate::host::item::Kind::Function
                                | crate::host::item::Kind::AssociatedFunction
                        )
                    })
                    .map(|item| item.signature().parameters().len())
                    .collect::<BTreeSet<_>>();
                if !matches!(self.peek(), Token::LeftParen | Token::Unit)
                    && arities.len() == 1
                    && let Some(&arity) = arities.first()
                    && arity > 0
                {
                    let at = name_span.start();
                    let parameter =
                        |index: usize| Token::Identifier(Box::new(format!("__a{index}").into()));
                    let mut tokens = Vec::new();
                    for index in 0..arity {
                        if index > 0 {
                            tokens.push((Token::Comma, at));
                        }
                        tokens.push((parameter(index), at));
                    }
                    tokens.push((Token::Pipe, at));
                    tokens.push((Token::Identifier(Box::new(name.as_str().into())), at));
                    tokens.push((Token::LeftParen, at));
                    for index in 0..arity {
                        if index > 0 {
                            tokens.push((Token::Comma, at));
                        }
                        tokens.push((parameter(index), at));
                    }
                    tokens.push((Token::RightParen, at));
                    self.splice_tokens(tokens);
                    return self.parse_closure(Token::Pipe, name_span, false);
                }
                return Err(Error::InvalidHostOperation {
                    reason: format!(
                        "`{}` has several forms here; call it with `(..)` to pick one",
                        binding.qualified
                    )
                    .into(),
                    item: binding.qualified,
                    operation: name_span,
                });
            };
            return self.parse_host_value(item.clone(), owner, name_span);
        }

        if let Some(function) = self.models.function(&name).cloned().filter(|_| !local) {
            if function.definition.body.constant {
                return Ok(self.constant_use(&function, name_span));
            }
            if this_parameter(&function.definition).is_some()
                || !copied_parameters(
                    &function.definition.parameters,
                    None,
                    function.definition.formatter.as_ref(),
                )
                .is_empty()
            {
                // A `&mut` parameter is copied in and back out of a call, so
                // the value is the closure `|a, ..| f(a, ..)` over the
                // function's parameter types, whose call does that.
                return self.mutable_function_value(&function, name, name_span);
            }
            return Ok(self.function_value(&function, name_span));
        }

        // `PI` after `use std::f64::consts::PI;`.
        if !self.bindings.contains_key(name.as_str())
            && let Some(binding) = self.models.import(&name)
            && let Some((value, single)) = crate::resolve::float_constant(&binding.qualified)
        {
            return Ok(self.float_constant(value, single, name_span));
        }
        self.record_capture(&name);
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Err(self.unknown_variable(&name, name_span));
        };
        let (declaration, ty) = (binding.declaration, binding.ty.clone());
        self.reject_shadowed_root(&name, declaration, name_span)?;
        self.mention(&name, declaration, &ty, name_span);
        if let Some(element) = self.aliased_element(&name, declaration, &ty, name_span)? {
            return Ok(element);
        }
        if let Some(held) = self
            .held_iter_mut
            .get(&name)
            .filter(|held| held.declaration == declaration)
            .cloned()
        {
            return self.step_iter_mut(&name, held, name_span);
        }
        // A local bound to a `RefCell` borrow: the cell's value. A closure
        // or async body reads its own borrows, not one it captures.
        if self
            .closures
            .last()
            .is_none_or(|scope| !scope.captures.contains(name.as_str()))
            && let Some(value) = self.cell_guard_use(&name, name_span)
        {
            return Ok(value);
        }
        let assigned = self.peek() == Token::Equals;
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Err(self.unknown_variable(&name, name_span));
        };
        // A local holding a map entry's `&mut`: the entry, read again.
        if let Some((_, tokens)) = self
            .entry_aliases
            .get(&name)
            .filter(|(declaration, _)| *declaration == binding.declaration)
            .cloned()
            && self.closures.is_empty()
        {
            let at = name_span.start();
            self.splice_tokens(tokens.into_iter().map(|(token, _)| (token, at)).collect());
            let mut value = self.parse_operand()?;
            value.span = span(name_span.start(), name_span.end());
            return Ok(value);
        }
        // A local iterator's chain, run where it is used; a closure using
        // it captures what its binding evaluated.
        if let Some(bound) = self
            .bound_iterators
            .get(&name)
            .filter(|bound| bound.declaration == binding.declaration)
            .cloned()
        {
            for scope in &mut self.closures {
                if scope.outer.get(&name) == Some(&bound.declaration) {
                    scope
                        .captures
                        .extend(bound.names.iter().map(|name| name.as_str().into()));
                }
            }
            let ty = Ty::iter(bound.item.clone());
            let mut value = self.iterator_expr(bound.pipeline, bound.item, name_span);
            value.lvalue = Some(Lvalue::Binding { binding: name, ty });
            return Ok(value);
        }
        // A moved local may be assigned again, as in Rust. A `let x;` used
        // before it has a value is rustc's E0381, which its borrow check
        // reports after the body's types.
        if !binding.available
            && !assigned
            && let Some(first) = self.deferred.get(&binding.declaration).copied()
            && !self.moved_at.contains_key(&binding.declaration)
        {
            let (declaration, ty) = (binding.declaration, binding.ty.clone());
            let unknown = matches!(self.inference.resolve(&ty), Ty::Infer(_));
            let operator = match self.peek() {
                Token::EqualsTo
                | Token::NotEqualsTo
                | Token::LessThan
                | Token::LessThanEqualsTo
                | Token::GreaterThan
                | Token::GreaterThanEqualsTo => Some("E0283"),
                Token::Plus | Token::Minus | Token::Multiply | Token::Divide | Token::Modulo => {
                    Some("E0284")
                }
                _ => None,
            }
            .filter(|_| unknown)
            .map(|code| (code, point_span(self.position())));
            self.uninitialized.push(UninitializedUse {
                name: name.clone(),
                declaration,
                reference: name_span,
                ty,
                possibly: first.is_some(),
                operator,
            });
        } else if !binding.available && !assigned {
            return Err(self.moved_value(&name, binding, name_span, false));
        }
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Err(self.unknown_variable(&name, name_span));
        };
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::plain(name.to_string()),
            ty: binding.ty.clone(),
            span: name_span,
            lvalue: Some(Lvalue::Binding {
                binding: name,
                ty: binding.ty.clone(),
            }),
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// Where a `catch` follows the block that starts at the next token.
    pub(super) fn catch_after_block(&self) -> Option<Position> {
        let mut ahead = self.input.clone();
        if !matches!(ahead.next(), Some((Token::LeftBrace, _))) {
            return None;
        }
        let mut depth = 1_usize;
        while depth > 0 {
            match ahead.next()? {
                (Token::LeftBrace | Token::MapStart, _) => depth += 1,
                (Token::RightBrace, _) => depth -= 1,
                (Token::EOF, _) => return None,
                _ => {}
            }
        }
        match ahead.next()? {
            (Token::Identifier(word), at) if word.as_str() == "catch" => Some(at),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(super) struct ParsedExpr {
    pub(super) prelude: origin::Fragment,
    pub(super) source: origin::Fragment,
    pub(super) ty: Ty,
    pub(super) span: Span,
    pub(super) lvalue: Option<Lvalue>,
    pub(super) outcome: crate::host::usage::Outcome,
    pub(super) evaluation: crate::host::usage::Evaluation,
}

impl ParsedExpr {
    pub(super) fn rendered_source(&self) -> origin::Fragment {
        if self.prelude.is_empty() {
            self.source.clone()
        } else {
            self.rendered_block()
        }
    }

    pub(super) fn rendered_block(&self) -> origin::Fragment {
        let mut rendered = origin::Fragment::plain("{\n");
        rendered.push(self.prelude.clone());
        rendered.push(self.source.clone());
        rendered.push_text("\n}");
        rendered
    }

    pub(super) fn retain_origin(mut self) -> Self {
        self.source.cover(self.span);
        self
    }

    /// Whether this value is a local read by its name, which a call or a
    /// `format!` can borrow where it is instead of a copy (D568).
    pub(super) fn is_local(&self) -> bool {
        self.prelude.is_empty() && is_valid_identifier(self.source.as_str())
    }
}
