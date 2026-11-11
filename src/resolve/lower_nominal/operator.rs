//! Operators: binary and unary operators by precedence, casts, ranges,
//! integer arithmetic, and crate types' `std::ops` and `Index` impls.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn operator_expr(&self, source: origin::Fragment, ty: Ty, span: Span) -> ParsedExpr {
        ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty,
            span,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin()
    }

    /// Rust binary operators take two operands of one type from `allowed`.
    pub(super) fn operand_type(
        &mut self,
        lhs: &ParsedExpr,
        rhs: &mut ParsedExpr,
        allowed: &[Ty],
    ) -> Result<Ty, Error> {
        let mut ty = self.inference.resolve(&lhs.ty);
        // An operand not yet inferred (a closure parameter) takes the other's type.
        if let Ty::Infer(id) = ty {
            let other = self.inference.resolve(&rhs.ty);
            if allowed_type(allowed, &other) {
                self.inference.bind(id, other.clone());
                ty = other;
            }
        }
        if !allowed_type(allowed, &ty) {
            return Err(Error::TypeMismatch {
                expected: allowed[0].name(&self.models),
                actual: ty.name(&self.models),
                expression: lhs.span,
            });
        }
        self.coerce_expr(&ty, rhs)?;
        Ok(self.inference.resolve(&ty))
    }

    pub(super) fn binary(
        &self,
        lhs: ParsedExpr,
        operator: &str,
        rhs: ParsedExpr,
        ty: Ty,
    ) -> ParsedExpr {
        let mut source = origin::Fragment::plain("(");
        source.push(lhs.rendered_source());
        source.push_text(format!(" {operator} "));
        source.push(rhs.rendered_source());
        source.push_text(")");
        self.operator_expr(source, ty, span(lhs.span.start(), rhs.span.end()))
    }

    /// `a..=b`, Rawr's range value; `a..b` and `a..`, iterators.
    pub(super) fn parse_range(&mut self) -> Result<ParsedExpr, Error> {
        let lhs = self.parse_binary(0)?;
        let inclusive = match self.peek() {
            Token::InclusiveRange => true,
            Token::ExclusiveRange => false,
            _ => return Ok(lhs),
        };

        self.next();
        let rhs = if !inclusive
            && matches!(
                self.peek(),
                Token::RightParen
                    | Token::Comma
                    | Token::SemiColon
                    | Token::RightBrace
                    | Token::RightBracket
            ) {
            None
        } else {
            Some(self.parse_binary(0)?)
        };
        // `0.0..=1.0`: Rust's `RangeInclusive<f64>`, a value of two bounds.
        if inclusive
            && let Some(rhs) = &rhs
            && matches!(self.inference.resolve(&lhs.ty), Ty::Float(_))
        {
            self.require_type(&lhs.ty, &rhs.ty, rhs.span)?;
            return Ok(self.binary(lhs, "..=", rhs.clone(), Ty::FloatRange));
        }
        // `0.5..1.0`: Rust's `Range<f32>` or `Range<f64>`, a value of two
        // bounds that an imported generic item such as `random_range` takes.
        if !inclusive
            && let Some(rhs) = &rhs
            && matches!(self.inference.resolve(&lhs.ty), Ty::Float(_))
        {
            self.require_type(&lhs.ty, &rhs.ty, rhs.span)?;
            let element = Box::new(lhs.ty.clone());
            return Ok(self.binary(lhs, "..", rhs.clone(), Ty::Range(element)));
        }
        // `'a'..='e'`: an iterator over the chars between.
        if matches!(self.inference.resolve(&lhs.ty), Ty::Char) {
            if let Some(rhs) = &rhs {
                self.require_type(&lhs.ty, &rhs.ty, rhs.span)?;
            }
            let range = self.range_iterator(&lhs, rhs.as_ref(), Ty::Char, inclusive);
            self.record_range_value(&lhs, rhs.as_ref(), Ty::Char, inclusive, &range);
            return Ok(range);
        }
        let item = self.integer_bounds(&lhs, rhs.as_ref())?;
        let range = self.range_iterator(&lhs, rhs.as_ref(), item.clone(), inclusive);
        self.record_range_value(&lhs, rhs.as_ref(), item, inclusive, &range);
        self.last_range = Some((lhs, rhs, inclusive, range.span));
        Ok(range)
    }

    /// Keep `low..high` of integers or chars as the value Rust's
    /// `Range<T>` is, a Rawr `Range<i64>` of the bounds (a char's code
    /// point), for an imported generic item that takes the range itself.
    pub(super) fn record_range_value(
        &mut self,
        low: &ParsedExpr,
        high: Option<&ParsedExpr>,
        item: Ty,
        inclusive: bool,
        range: &ParsedExpr,
    ) {
        let Some(high) = high.filter(|_| !inclusive) else {
            return;
        };
        let bound = |this: &mut Self, value: &ParsedExpr| {
            let value = value.rendered_source().surrounded("(", ")");
            if item == Ty::Char {
                this.prelude_op(PreludeOp::Cast, [value, "7".into()])
            } else {
                value
            }
        };
        let mut value = bound(self, low);
        value.push_text(" .. ");
        value.push(bound(self, high));
        self.range_values.insert(
            range.source.as_str().to_string(),
            (value.surrounded("(", ")"), Ty::Range(Box::new(item))),
        );
    }

    /// A range's bounds: integers of one kind, which its items have.
    pub(super) fn integer_bounds(
        &mut self,
        lhs: &ParsedExpr,
        rhs: Option<&ParsedExpr>,
    ) -> Result<Ty, Error> {
        if let Some(rhs) = rhs {
            self.require_type(&lhs.ty, &rhs.ty, rhs.span)?;
        }
        if let Ty::Infer(id) = self.inference.resolve(&lhs.ty) {
            let kind = Ty::Int(Int::Var(self.inference.fresh_id()));
            self.inference.bind(id, kind);
        }
        match self.inference.resolve(&lhs.ty) {
            ty @ Ty::Int(_) => Ok(ty),
            ty => Err(Error::TypeMismatch {
                expected: "an integer".into(),
                actual: ty.name(&self.models),
                expression: lhs.span,
            }),
        }
    }

    /// Rust binary operators by precedence climbing, so plain operands add
    /// no parser frames: `||` < `&&` < comparisons < `+ -` < `* / %`.
    pub(super) fn parse_binary(&mut self, minimum: u8) -> Result<ParsedExpr, Error> {
        let mut lhs = self.parse_cast()?;
        while let Some(precedence) = binary_precedence(&self.peek()) {
            if precedence < minimum {
                break;
            }
            // `&&` ends a let chain's link; `||` cannot join one, as in Rust.
            if self.chain_link == Some(self.expression_stack.len()) {
                match self.peek() {
                    Token::And => break,
                    Token::Or => {
                        return Err(self.unsupported("`||` in a `let` chain"));
                    }
                    _ => {}
                }
            }
            let (operator, operator_at) = self.next();
            let operator_at = span(
                operator_at,
                text_end(operator_at, operator.literal_syntax()),
            );
            // `===` and `!==` lex as `==` or `!=` and then `=`, as rustc lexes
            // them, and rustc names the three characters together.
            if matches!(operator, Token::EqualsTo | Token::NotEqualsTo)
                && matches!(self.peek(), Token::Equals)
                && self.position() == operator_at.end()
            {
                let (written, meant) = match operator {
                    Token::EqualsTo => ("===", "=="),
                    _ => ("!==", "!="),
                };
                return Err(Error::Syntax {
                    message: format!("invalid comparison operator `{written}`").into(),
                    label: "".into(),
                    help: Some(
                        format!("`{written}` is not a valid comparison operator, use `{meant}`")
                            .into(),
                    ),
                    at: span(operator_at.start(), text_end(operator_at.start(), written)),
                });
            }
            // `String + &str` takes the `String` by value.
            if matches!(operator, Token::Plus)
                && matches!(self.inference.resolve(&lhs.ty), Ty::String)
            {
                self.move_local(&mut lhs)?;
            }
            // The second operand of `&&` and `||` is a temporary scope.
            let lazy = matches!(operator, Token::And | Token::Or);
            if lazy {
                self.open_temporaries();
                self.flow().open_temporaries();
                self.open_frames();
            }
            let rhs = self.parse_binary(precedence + 1);
            let (temporaries, frames) = if lazy {
                let frames = self.close_frames();
                self.flow().close_temporaries(ScopeEnd::Statement);
                (self.close_temporaries(), frames)
            } else {
                (Vec::new(), Vec::new())
            };
            let mut rhs = rhs?;
            if !temporaries.is_empty() {
                rhs.source = self.with_temporaries(&temporaries, rhs.rendered_source());
                rhs.prelude = origin::Fragment::default();
            }
            // It opens its frames only when it is evaluated (D558 tier 1).
            if !frames.is_empty() {
                let rendered = rhs.rendered_source();
                rhs.source = self.framed(frames, rendered);
                rhs.prelude = origin::Fragment::default();
                rhs.lvalue = None;
            }
            lhs = self.apply_binary(operator, Some(operator_at), lhs, rhs)?;
            if precedence == COMPARISON && binary_precedence(&self.peek()) == Some(COMPARISON) {
                return Err(self.unsupported("chained comparison operators"));
            }
        }
        Ok(lhs)
    }

    /// An operand and its `as` casts, which bind tighter than the binary
    /// operators and looser than unary ones and postfix: `-x as u8` is
    /// `(-x) as u8`, `v.len() as i64` casts the call.
    pub(super) fn parse_cast(&mut self) -> Result<ParsedExpr, Error> {
        let mut value = self.parse_operand()?;
        while matches!(self.peek(), Token::As) {
            self.next();
            let start = self.position();
            let target = self.parse_local_type()?;
            let end = self.position();
            value = self.cast(value, target, span(start, end))?;
        }
        Ok(value)
    }

    /// `value as target` between numbers, `char`, `bool` and `Ordering`
    /// (E0605 otherwise). A widening or same-kind cast costs nothing; a
    /// literal takes the target's kind, as Rust infers it.
    pub(super) fn cast(
        &mut self,
        mut value: ParsedExpr,
        target: Ty,
        written: Span,
    ) -> Result<ParsedExpr, Error> {
        let mut source = self.inference.resolve(&value.ty);
        let target = self.inference.resolve(&target);
        // `&mut c as &mut dyn Bump`, `Box::new(c) as Box<dyn Bump>`: an
        // unsized coercion, the value (and its place) unchanged.
        // `double as fn(i64) -> i64`: a function item or closure coerced
        // to its pointer type. A host style function (`button::primary as
        // fn(&Theme, _) -> _`) stays the host value, which Rawr passes on
        // as it is.
        if let Ty::Fn { .. } = &target {
            if matches!(source, Ty::Host { .. } | Ty::Callable { .. }) {
                value.span = span(value.span.start(), written.end());
                value.lvalue = None;
                return Ok(value);
            }
            self.coerce_expr(&target, &mut value)?;
            value.ty = target;
            value.span = span(value.span.start(), written.end());
            value.lvalue = None;
            return Ok(value);
        }
        if matches!(
            (&source, &target),
            (Ty::Reference { .. }, Ty::Reference { .. })
                | (Ty::Boxed(..), Ty::Boxed(..))
                | (Ty::Counted(..), Ty::Counted(..))
        ) {
            let lvalue = value.lvalue.clone();
            self.coerce_expr(&target, &mut value)?;
            value.ty = target;
            value.span = span(value.span.start(), written.end());
            value.lvalue = lvalue;
            return Ok(value);
        }
        // `Color::Red as i32`: a fieldless enum's discriminant, cast on as
        // an `isize` is.
        if let (Ty::Script(id, _), Ty::Int(_)) = (&source, &target)
            && let Nominal::Enum { item, .. } = self.models.definition(*id)
            && item
                .variants
                .iter()
                .all(|variant| variant.payload.is_empty())
        {
            let operand = value.rendered_source();
            value.prelude = origin::Fragment::default();
            value.source = self.prelude_op(PreludeOp::Discriminant, [operand]);
            value.lvalue = None;
            source = Ty::Int(Int::I64);
            value.ty = source.clone();
        }
        // A literal past `i64` (`wide_literal`) takes the cast's kind too.
        let literal = value.source.as_str().parse::<crate::FLOAT>().is_ok()
            || self
                .pending_wide_literals
                .iter()
                .any(|pending| pending.ty == source);
        match (&source, &target) {
            (Ty::Int(Int::Var(_)), Ty::Int(_)) | (Ty::Float(Float::Var(_)), Ty::Float(_))
                if literal => {}
            (Ty::Int(Int::Var(id)), _) => {
                self.inference.bind(*id, Ty::Int(Int::I64));
                source = Ty::Int(Int::I64);
            }
            (Ty::Float(Float::Var(id)), _) => {
                self.inference.bind(*id, Ty::Float(Float::F64));
                source = Ty::Float(Float::F64);
            }
            _ => {}
        }
        let invalid = || Error::TypeMismatch {
            expected: "a primitive type that `as` converts to".into(),
            actual: format!(
                "{} as {}",
                source.name(&self.models),
                target.name(&self.models)
            )
            .into(),
            expression: written,
        };
        let free = match (&source, &target) {
            (Ty::Int(Int::Var(id)), Ty::Int(_)) | (Ty::Float(Float::Var(id)), Ty::Float(_))
                if literal =>
            {
                if let (Ty::Int(to), Ok(literal)) = (&target, value.source.as_str().parse::<INT>())
                    && !(to.range().0..=to.range().1).contains(&literal)
                {
                    return Err(invalid());
                }
                self.inference.bind(*id, target.clone());
                true
            }
            // An `i128` or a `u128` keeps a narrower kind's value as it is
            // (`Int`), but for a `u64`'s or `usize`'s bits and a negative
            // number into a `u128`.
            (Ty::Int(from), Ty::Int(to)) if from.wide() || to.wide() => {
                from == to
                    || !from.wide()
                        && !matches!(from, Int::U64 | Int::Usize)
                        && (*to == Int::I128 || !from.signed())
            }
            // Every kind keeps a value as Rust's 64-bit `as` leaves it.
            (Ty::Int(from), Ty::Int(to)) => {
                let ((low, high), (to_low, to_high)) = (from.full_range(), to.full_range());
                to.bits() == 64 || to_low <= low && high <= to_high
            }
            (Ty::Ordering, Ty::Int(to)) => to.signed(),
            (Ty::Float(from), Ty::Float(to)) => from == to || *to == Float::F64,
            (Ty::Int(_) | Ty::Float(_), Ty::Int(_) | Ty::Float(_))
            | (Ty::Char | Ty::Bool, Ty::Int(_))
            | (Ty::Int(Int::U8), Ty::Char) => false,
            (Ty::Char, Ty::Char) => true,
            _ => return Err(invalid()),
        };
        let end = written.end();
        value.span = span(value.span.start(), end);
        if !free {
            let operand = value.rendered_source();
            value.prelude = origin::Fragment::default();
            let code = match (&source, &target) {
                (Ty::Int(Int::U64 | Int::Usize), Ty::Float(Float::F32)) => 13,
                (Ty::Int(Int::U64 | Int::Usize), Ty::Float(_)) => 14,
                (Ty::Int(Int::U64 | Int::Usize), Ty::Int(to)) if to.wide() => {
                    kind_code(target.clone()) + UNSIGNED_SOURCE
                }
                _ => kind_code(target.clone()),
            };
            value.source = self.prelude_op(PreludeOp::Cast, [operand, code.to_string().into()]);
        }
        value.ty = target;
        value.lvalue = None;
        value.source.cover(value.span);
        Ok(value.retain_origin())
    }

    /// `lhs operator rhs`; `operator_at` is the operator's span where the
    /// program wrote one.
    pub(super) fn apply_binary(
        &mut self,
        operator: Token,
        operator_at: Option<Span>,
        lhs: ParsedExpr,
        mut rhs: ParsedExpr,
    ) -> Result<ParsedExpr, Error> {
        const NUMBERS: &[Ty] = &[Ty::Int(Int::I64), Ty::Float(Float::F64)];
        // With drop glue in the crate: a comparison borrows its operands,
        // so a value made for it is a temporary; another operator's trait
        // takes them by value.
        let mut lhs = lhs;
        if self.drops() && !matches!(operator, Token::And | Token::Or) {
            let compares = matches!(
                operator,
                Token::EqualsTo
                    | Token::NotEqualsTo
                    | Token::LessThan
                    | Token::LessThanEqualsTo
                    | Token::GreaterThan
                    | Token::GreaterThanEqualsTo
            );
            for operand in [&mut lhs, &mut rhs] {
                if compares {
                    self.hold_temporary(operand)?;
                } else if self.needs_drop(&operand.ty, operand.span)? {
                    self.move_local(operand)?;
                }
            }
        }
        let lhs = lhs;
        // Bit operators, also through `&i64` and `&bool`: `& | ^` on two
        // integers of one kind or two `bool`s, the runtime's own; a shift
        // of any integer by any integer, with Rust's overflow panic.
        if let Token::Ampersand | Token::Pipe | Token::XOr | Token::LeftShift | Token::RightShift =
            operator
        {
            let symbol = match operator {
                Token::Ampersand => "&",
                Token::Pipe => "|",
                Token::XOr => "^",
                Token::LeftShift => "<<",
                _ => ">>",
            };
            if let Some(value) = self.operator_impl(symbol, operator_at, &lhs, Some(&mut rhs))? {
                return Ok(value);
            }
            let mut lhs = lhs;
            for operand in [&mut lhs, &mut rhs] {
                if let Ty::Reference {
                    mutable: false,
                    target,
                } = self.inference.resolve(&operand.ty)
                    && matches!(*target, Ty::Int(_) | Ty::Bool)
                {
                    operand.ty = *target;
                }
            }
            let symbol = match operator {
                Token::Ampersand => "&",
                Token::Pipe => "|",
                Token::XOr => "^",
                Token::LeftShift => "<<",
                _ => ">>",
            };
            if matches!(symbol, "&" | "|" | "^") {
                let ty = self.operand_type(&lhs, &mut rhs, &[Ty::Int(Int::I64), Ty::Bool])?;
                if let Ty::Int(kind) = ty
                    && kind.wide()
                {
                    return Ok(self.wide_binary(symbol, lhs, rhs, ty.clone(), ty));
                }
                return Ok(self.binary(lhs, symbol, rhs, ty));
            }
            let ty = self.integer_operand(&lhs)?;
            let amount = self.integer_operand(&rhs)?;
            let expression = span(lhs.span.start(), rhs.span.end());
            let source = self.int_op(
                symbol,
                lhs.rendered_source(),
                rhs.rendered_source(),
                &ty,
                &amount,
            );
            return Ok(self.operator_expr(source, ty, expression));
        }
        let (symbol, allowed, output) = match operator {
            Token::Or => ("||", &[Ty::Bool][..], Some(Ty::Bool)),
            Token::And => ("&&", &[Ty::Bool][..], Some(Ty::Bool)),
            Token::EqualsTo | Token::NotEqualsTo => {
                return self.equality(operator, operator_at, lhs, rhs)
            }
            Token::LessThan => (
                "<",
                &[
                    Ty::Int(Int::I64),
                    Ty::Float(Float::F64),
                    Ty::String,
                    Ty::Str,
                    Ty::Char,
                    Ty::Ordering,
                ][..],
                Some(Ty::Bool),
            ),
            Token::GreaterThan => (
                ">",
                &[
                    Ty::Int(Int::I64),
                    Ty::Float(Float::F64),
                    Ty::String,
                    Ty::Str,
                    Ty::Char,
                    Ty::Ordering,
                ][..],
                Some(Ty::Bool),
            ),
            Token::LessThanEqualsTo => (
                "<=",
                &[
                    Ty::Int(Int::I64),
                    Ty::Float(Float::F64),
                    Ty::String,
                    Ty::Str,
                    Ty::Char,
                    Ty::Ordering,
                ][..],
                Some(Ty::Bool),
            ),
            Token::GreaterThanEqualsTo => (
                ">=",
                &[
                    Ty::Int(Int::I64),
                    Ty::Float(Float::F64),
                    Ty::String,
                    Ty::Str,
                    Ty::Char,
                    Ty::Ordering,
                ][..],
                Some(Ty::Bool),
            ),
            Token::Plus => ("+", NUMBERS, None),
            Token::Minus => ("-", NUMBERS, None),
            Token::Multiply => ("*", NUMBERS, None),
            Token::Divide => ("/", NUMBERS, None),
            Token::Modulo => ("%", NUMBERS, None),
            _ => unreachable!("binary_precedence admits only binary operators"),
        };
        // `&a < &b` through core's `impl PartialOrd<&B> for &A where A:
        // PartialOrd<B>` (and its `&mut` form): the referents compare, and
        // a reference is its referent's value at runtime. Every other
        // `PartialOrd` impl of Rawr's types is the type's own, so the right
        // operand takes the left's type, as rustc infers it.
        let mut lhs = lhs;
        if matches!(symbol, "<" | ">" | "<=" | ">=") {
            while let (
                Ty::Reference {
                    mutable,
                    target: left,
                },
                Ty::Reference {
                    mutable: other,
                    target: right,
                },
            ) = (
                self.inference.resolve(&lhs.ty),
                self.inference.resolve(&rhs.ty),
            ) && mutable == other
            {
                lhs.ty = *left;
                rhs.ty = *right;
            }
        }
        let lhs = lhs;
        if let Some(value) = self.time_binary(symbol, lhs.clone(), rhs.clone())? {
            return Ok(value);
        }
        // `impl Add<&str> for String`: a `&String` derefs to the `&str`.
        if symbol == "+" && self.inference.resolve(&lhs.ty) == Ty::String {
            self.coerce_expr(&Ty::Str, &mut rhs)?;
            return Ok(self.binary(lhs, symbol, rhs, Ty::String));
        }
        if matches!(symbol, "<" | ">" | "<=" | ">=")
            && let Some(value) = self.partial_ord_operator(symbol, operator_at, &lhs, &mut rhs)?
        {
            return Ok(value);
        }
        if output.is_none()
            && let Some(value) = self.operator_impl(symbol, operator_at, &lhs, Some(&mut rhs))?
        {
            return Ok(value);
        }
        let mut lhs = lhs;
        if output.is_none() {
            // Rust's arithmetic takes `&i64` and `&f64` operands too.
            for operand in [&mut lhs, &mut rhs] {
                if let Ty::Reference {
                    mutable: false,
                    target,
                } = self.inference.resolve(&operand.ty)
                    && matches!(*target, Ty::Int(_) | Ty::Float(_))
                {
                    operand.ty = *target;
                }
            }
            // A left operand with no impl of the operator's trait, as
            // `"a" + "b"`: rustc's E0369.
            let left = self.inference.resolve(&lhs.ty);
            if !matches!(left, Ty::Infer(_)) && !allowed_type(allowed, &left) {
                let right = self.inference.resolve(&rhs.ty);
                let (left, right) = (left.name(&self.models), right.name(&self.models));
                let message = match symbol {
                    "+" => format!("cannot add `{right}` to `{left}`"),
                    "-" => format!("cannot subtract `{right}` from `{left}`"),
                    "*" => format!("cannot multiply `{left}` by `{right}`"),
                    "/" => format!("cannot divide `{left}` by `{right}`"),
                    _ => format!("cannot calculate the remainder of `{left}` divided by `{right}`"),
                };
                return Err(Error::StdPath {
                    code: "E0369",
                    message: message.into(),
                    label: None,
                    note: None,
                    reference: operator_at.unwrap_or(span(lhs.span.start(), rhs.span.end())),
                });
            }
            // A number with another type, as `f64 / usize` or `2.0 *
            // angle` with no crate's `impl Mul<Angle> for f64`: std's impls
            // of the operator's trait for the number take only its own
            // type, rustc's E0277.
            let right = self.inference.resolve(&rhs.ty);
            if matches!(left, Ty::Int(_) | Ty::Float(_))
                && !matches!(
                    right,
                    Ty::Infer(_) | Ty::Param(_) | Ty::Projection(..) | Ty::Unknown
                )
                && !self.unifies(&left, &right)
            {
                let (left, right) = (left.name(&self.models), right.name(&self.models));
                if let Some((message, trait_name)) = arithmetic_trait(symbol, &left, &right) {
                    return Err(Error::Unimplemented {
                        message: message.into(),
                        label: format!("no implementation for `{left} {symbol} {right}`").into(),
                        help: Some(
                            format!(
                                "the trait `{trait_name}<{right}>` is not implemented for `{left}`"
                            )
                            .into(),
                        ),
                        at: operator_at.unwrap_or(span(lhs.span.start(), rhs.span.end())),
                    });
                }
            }
        }
        let ty = self.operand_type(&lhs, &mut rhs, allowed)?;
        // An `i128` or a `u128` past `i64` is boxed: its operators are
        // `INT_METHODS`'.
        if let Ty::Int(kind) = ty
            && kind.wide()
        {
            let output = output.unwrap_or_else(|| ty.clone());
            return Ok(self.wide_binary(symbol, lhs, rhs, ty, output));
        }
        // A `u64` or a `usize` is its bits in the runtime's `i64`: its
        // arithmetic is unsigned, and it orders with the sign bit flipped.
        if let Ty::Int(Int::U64 | Int::Usize) = ty {
            if output.is_none() {
                let expression = span(lhs.span.start(), rhs.span.end());
                let source = self.int_op(
                    symbol,
                    lhs.rendered_source(),
                    rhs.rendered_source(),
                    &ty,
                    &ty,
                );
                return Ok(self.operator_expr(source, ty, expression));
            }
            if symbol != "==" && symbol != "!=" {
                let mut lhs = lhs;
                for operand in [&mut lhs, &mut rhs] {
                    operand.source = unsigned_order(operand.rendered_source());
                    operand.prelude = origin::Fragment::default();
                }
                return Ok(self.binary(lhs, symbol, rhs, Ty::Bool));
            }
        }
        // `/` and `%` of a kind narrower than the runtime's `i64` panic as
        // debug Rust's do, by zero and at `MIN / -1` and `MIN % -1`, which
        // the runtime's `i64` computes.
        if output.is_none()
            && matches!(symbol, "/" | "%")
            && let Ty::Int(kind) = ty
            && kind.checked()
        {
            let expression = span(lhs.span.start(), rhs.span.end());
            let source = self.int_op(
                symbol,
                lhs.rendered_source(),
                rhs.rendered_source(),
                &ty,
                &ty,
            );
            return Ok(self.operator_expr(source, ty, expression));
        }
        let Some(output) = output else {
            let value = self.binary(lhs, symbol, rhs, ty.clone());
            return Ok(self.checked_arithmetic(value, symbol));
        };
        Ok(self.binary(lhs, symbol, rhs, output))
    }

    /// The integer type of an operand, an unknown one taking `{integer}`.
    pub(super) fn integer_operand(&mut self, operand: &ParsedExpr) -> Result<Ty, Error> {
        if let Ty::Infer(id) = self.inference.resolve(&operand.ty) {
            let kind = Ty::Int(Int::Var(self.inference.fresh_id()));
            self.inference.bind(id, kind);
        }
        match self.inference.resolve(&operand.ty) {
            ty @ Ty::Int(_) => Ok(ty),
            ty => Err(Error::TypeMismatch {
                expected: "an integer".into(),
                actual: ty.name(&self.models),
                expression: operand.span,
            }),
        }
    }

    /// `INT_METHODS`' `name` on `value` of the integer type `ty`, with
    /// `operand` of `operand_ty`, for the kinds inference settles on.
    pub(super) fn int_op(
        &mut self,
        name: &str,
        value: origin::Fragment,
        operand: origin::Fragment,
        ty: &Ty,
        operand_ty: &Ty,
    ) -> origin::Fragment {
        let code = INT_METHODS
            .iter()
            .position(|(method, ..)| *method == name)
            .expect("a known integer method");
        let helper = self.push_op(Op::Kinded {
            op: PreludeOp::IntMethod,
            kinds: [ty.clone(), operand_ty.clone()],
        });
        origin::Fragment::call(&helper, [value, code.to_string().into(), operand])
    }

    /// An arithmetic result of a kind narrower than the runtime's `i64`
    /// checked as debug Rust does (`attempt to add with overflow`), and an
    /// `f32` result rounded to `f32`.
    pub(super) fn checked_arithmetic(&mut self, mut value: ParsedExpr, symbol: &str) -> ParsedExpr {
        let code = match self.inference.resolve(&value.ty) {
            Ty::Int(kind) if kind.checked() => kind_code(Ty::Int(kind)).to_string(),
            Ty::Float(Float::F32) => kind_code(Ty::Float(Float::F32)).to_string(),
            // An `f32` once inference settles is rounded; an `f64` is not.
            Ty::Float(Float::Var(_)) => {
                let helper = self.push_op(Op::Prelude(PreludeOp::RoundF32));
                let placeholder = format!(
                    "__rawr_float_rounding_{:016}",
                    self.pending_float_kinds.len()
                );
                debug_assert!(helper.len() <= placeholder.len());
                self.pending_float_kinds.push(PendingFloat {
                    placeholder: placeholder.clone(),
                    ty: value.ty.clone(),
                    kind: PendingFloatKind::Round(helper),
                });
                let source = std::mem::take(&mut value.source);
                value.source = source.surrounded(&format!("{placeholder}("), ")");
                value.source.cover(value.span);
                return value;
            }
            // Its kind is known once inference settles (`finish_witnesses`).
            Ty::Int(Int::Var(_)) => {
                let placeholder = format!("__rawr_int_kind_{:08}", self.pending_int_kinds.len());
                self.pending_int_kinds
                    .push((placeholder.clone(), value.ty.clone()));
                placeholder
            }
            _ => return value,
        };
        let verb = match symbol {
            "+" => "add",
            "-" => "subtract",
            "*" => "multiply",
            "/" => "divide",
            "%" => "calculate the remainder",
            "pow" => "exponentiate",
            _ => "negate",
        };
        let source = std::mem::take(&mut value.source);
        value.source = self.prelude_op(
            PreludeOp::Narrow,
            [
                source,
                code.into(),
                format!("{:?}", format!("attempt to {verb} with overflow")).into(),
            ],
        );
        value.source.cover(value.span);
        value
    }

    /// `a + b`, `-a` or `a += b` on a crate type through its `std::ops`
    /// impl (`OPERATOR_TRAITS`), as Rust's operators are: the impl whose
    /// `Rhs` the right operand fits, its `Output` the value (for `+=`, the
    /// changed left operand). `None` for an operand of another type; a
    /// crate type without the impl is Rust's E0369 (E0600 unary, E0368
    /// compound), at `operator_at`: a binary operator where the program
    /// wrote one, the whole expression of a unary one.
    pub(super) fn operator_impl(
        &mut self,
        symbol: &str,
        operator_at: Option<Span>,
        lhs: &ParsedExpr,
        rhs: Option<&mut ParsedExpr>,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some((_, trait_name, member)) = crate::resolve::OPERATOR_TRAITS
            .iter()
            .find(|(operator, ..)| *operator == symbol)
        else {
            return Ok(None);
        };
        let lhs_ty = self.inference.resolve(&lhs.ty);
        let expression = span(
            lhs.span.start(),
            rhs.as_ref().map_or(lhs.span.end(), |rhs| rhs.span.end()),
        );
        // `&a + &b` through `impl Add<&V> for &V`, which is `V`'s (the parser
        // reads it so), and `2.0 * v` through `impl Mul<V> for f64`.
        let lhs_ty = match lhs_ty {
            Ty::Reference { target, .. }
                if matches!(self.inference.resolve(&target), Ty::Script(..)) =>
            {
                self.inference.resolve(&target)
            }
            ty => ty,
        };
        // A library type's operator: the facade method named for the
        // trait's (`Duration::add`); `a += b` is `a = a.add(b)`.
        let host = match &lhs_ty {
            Ty::Reference { target, .. } => self.inference.resolve(target),
            ty => ty.clone(),
        };
        // `2.0 * angle` through a crate's `impl Mul<Radians> for f32`: the
        // operand type's function of the number (`Radians::mul(f32,
        // Radians)`), among which the number's kind selects one as rustc
        // selects the impl. `y += angle` through its `impl
        // AddAssign<Radians> for f32` is the function that gives the
        // number back changed (`Radians::add_assign(f32, Radians) -> f32`).
        if matches!(host, Ty::Float(_) | Ty::Int(_) | Ty::Bool | Ty::Char)
            && let Some(rhs) = rhs.as_deref()
            && let Ty::Host { proof, .. } = self.inference.resolve(&rhs.ty)
        {
            let path = format!("{}::{member}", proof.definition.path());
            let operands = self
                .models
                .host_items(&path)
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| {
                            item.kind() == crate::host::item::Kind::AssociatedFunction
                                && item.signature().parameters().len() == 2
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !operands.is_empty() {
                let owner = self.models.item_owner(&path).cloned();
                let arguments = vec![lhs.clone(), rhs.clone()];
                return self
                    .call_host_function(&operands, owner, arguments, expression)
                    .map(Some);
            }
            // The importer answers a miss with each impl of the operator's
            // trait that takes the type, its own and a number's.
            #[cfg(feature = "rust-import")]
            if proof.definition.identity().imported().is_some()
                && self.models.host_items(&path).is_none()
                && self.models.host.entry(&path).is_none()
            {
                let error = Error::UnknownMember {
                    owner: host_type_name(&proof, &self.models),
                    member: (*member).into(),
                    reference: expression,
                };
                return Err(self.native_miss(true, error));
            }
        }
        if let Ty::Host { proof, .. } = &host
            && let Some(rhs) = rhs.as_deref()
        {
            let method = member.trim_end_matches("_assign");
            let path = format!("{}::{method}", proof.definition.path());
            if self.models.host_items(&path).is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.kind() == crate::host::item::Kind::Method)
            }) {
                let receiver = self.operator_expr(lhs.rendered_source(), host.clone(), lhs.span);
                self.pending_arguments = Some((vec![rhs.clone()], expression.end()));
                let mut value = self.parse_host_method(receiver, method.into(), expression)?;
                value.span = expression;
                return Ok(Some(value));
            }
            // An imported type's operator no binding has yet, as clock's
            // `Radians / 12.0`: a miss the importer answers with the
            // type's impl of the operator's trait.
            #[cfg(feature = "rust-import")]
            if proof.definition.identity().imported().is_some()
                && self.models.host_items(&path).is_none()
                && self.models.host.entry(&path).is_none()
            {
                let error = Error::UnknownMember {
                    owner: host_type_name(proof, &self.models),
                    member: method.into(),
                    reference: expression,
                };
                return Err(self.native_miss(true, error));
            }
        }
        // A generic bounded by the trait (`T: Add<Output = T>`): through
        // its dispatcher, whose fallback is the builtin operator.
        if matches!(lhs_ty, Ty::Param(_) | Ty::Projection(..))
            && self.type_implements(&lhs_ty, trait_name)
            && let Some((_, declared)) = self.models.traits.get(*trait_name).cloned()
            && let Some(function) = declared
                .functions
                .iter()
                .find(|function| function.name.text == *member)
        {
            let arguments = rhs.map(|rhs| rhs.clone()).into_iter().collect::<Vec<_>>();
            let mut value = self.call_trait_method(
                lhs.clone(),
                lhs_ty.clone(),
                &(*trait_name).into(),
                function,
                arguments,
                lhs.span,
                expression.end(),
            )?;
            if symbol.ends_with('=') {
                value.ty = lhs_ty;
            }
            return Ok(Some(value));
        }
        let implemented = |owner: type_def::Id| {
            self.models
                .trait_methods
                .get(&(owner, (*member).into()))
                .is_some_and(|definitions| {
                    definitions.iter().any(|definition| {
                        definition
                            .trait_name()
                            .is_some_and(|implemented| implemented == trait_name)
                    })
                })
        };
        let owner = match &lhs_ty {
            Ty::Script(owner, _) => *owner,
            // A crate's impl for a number (`impl Mul<V> for f64`).
            // An unsuffixed literal (`2.0 * v`) takes the kind an impl is for;
            // a number with a number is the builtin operator.
            ty @ (Ty::Int(_) | Ty::Float(_))
                if rhs.as_ref().is_some_and(|rhs| {
                    !matches!(
                        self.inference.resolve(&rhs.ty),
                        Ty::Int(_) | Ty::Float(_) | Ty::Infer(_)
                    )
                }) =>
            {
                let kinds = match ty {
                    Ty::Int(Int::Var(_)) => {
                        Int::KINDS.iter().map(|(_, kind)| Ty::Int(*kind)).collect()
                    }
                    Ty::Float(Float::Var(_)) => vec![Ty::Float(Float::F64), Ty::Float(Float::F32)],
                    ty => vec![ty.clone()],
                };
                match kinds
                    .iter()
                    .filter_map(owner_id)
                    .find(|owner| implemented(*owner))
                {
                    Some(owner) => owner,
                    None => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
        let candidates = self
            .models
            .trait_methods
            .get(&(owner, (*member).into()))
            .map(|definitions| {
                definitions
                    .iter()
                    .filter(|definition| {
                        definition
                            .trait_name()
                            .is_some_and(|implemented| implemented == trait_name)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let operand_ty = rhs.as_ref().map(|rhs| self.inference.resolve(&rhs.ty));
        // An unsuffixed literal that impls for two kinds fit (`2.0 * v` of
        // `impl Mul<V>` for both `f32` and `f64`) is the literal's
        // fallback kind, as rustc settles it.
        let candidates = match literal_fallback(&lhs_ty) {
            Some(fallback) => {
                let (preferred, others): (Vec<_>, Vec<_>) =
                    candidates.into_iter().partition(|candidate| {
                        candidate
                            .builtin_owner()
                            .is_some_and(|owner| Ty::from_resolved(owner) == fallback)
                    });
                preferred.into_iter().chain(others).collect()
            }
            None => candidates,
        };
        // The one impl there is takes the right operand as its `Rhs`, which
        // a right operand of another type mismatches (rustc's E0308).
        let impls = candidates.len();
        let only = (impls == 1).then(|| candidates[0].clone());
        let mut chosen = None;
        for candidate in candidates {
            let probe = candidate
                .generics()
                .iter()
                .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
                .collect();
            let owner_ty = impl_self_ty(&candidate, &probe);
            let checkpoint = self.inference.checkpoint();
            let mut fits = self.unify_type(&owner_ty, &lhs_ty);
            if let (Some(parameter), Some(operand)) = (candidate.parameters().first(), &operand_ty)
            {
                let parameter = instantiate(&Ty::from_resolved(&parameter.ty), &probe);
                fits &= self.unify_type(&parameter, operand);
            }
            self.inference.rollback(checkpoint);
            if fits {
                chosen = Some(candidate);
                break;
            }
        }
        // A number whose kind no impl is for, as `x * angle` with `x: f64`
        // and only `impl Mul<Angle> for f32`: std's impls of the trait for
        // the number, which take only its own type (rustc's E0277).
        if chosen.is_none() && matches!(lhs_ty, Ty::Int(_) | Ty::Float(_)) && !symbol.ends_with('=')
        {
            return Ok(None);
        }
        let Some(definition) = chosen.or(only) else {
            return Err(self.missing_operator_impl(
                Operation {
                    symbol,
                    trait_name,
                    impls,
                },
                &lhs_ty,
                operand_ty.as_ref(),
                operator_at.unwrap_or(expression),
                expression,
            ));
        };
        let substitution = self.method_substitution(&definition, Some(&lhs.ty), expression)?;
        let mut arguments = Vec::new();
        if let (Some(rhs), Some(parameter)) = (rhs, definition.parameters().first()) {
            let expected = instantiate(&Ty::from_resolved(&parameter.ty), &substitution);
            self.coerce_expr(&expected, rhs)?;
            arguments.push(rhs.rendered_source());
        }
        // The operator takes its own copy of the left operand, as a move or
        // copy does; `+=` gives back the changed copy.
        let receiver = self.temporary("operand");
        let mut source = origin::Fragment::plain(format!("({{\nlet {receiver} = "));
        source.push(lhs.rendered_source());
        source.push_text(format!(";\n{receiver}.{}(", method_name(definition.id())));
        source.push(origin::Fragment::join(arguments, ", "));
        let ty = if symbol.ends_with('=') {
            // `a += b` through a crate's `AddAssign` borrows `a` before `b`
            // is made, which these facts do not record yet.
            self.flow().unmodeled(
                "a compound assignment through a crate's operator impl",
                expression,
            );
            source.push_text(format!(");\n{receiver}\n}})"));
            lhs_ty
        } else {
            source.push_text(")\n})");
            instantiate(&Ty::from_resolved(definition.output()), &substitution)
        };
        Ok(Some(self.operator_expr(source, ty, expression)))
    }

    /// rustc's error for `operation` on `lhs`, a crate type with no impl of
    /// its trait for `operand`, at `operator`: E0600 for a unary operator,
    /// E0369 for a binary operator whose trait the type has no impl of, and
    /// E0277 for one whose impls take other types. E0368 for a compound
    /// assignment, at `expression`.
    fn missing_operator_impl(
        &self,
        operation: Operation<'_>,
        lhs: &Ty,
        operand: Option<&Ty>,
        operator: Span,
        expression: Span,
    ) -> Error {
        let Operation {
            symbol,
            trait_name,
            impls,
        } = operation;
        let left = lhs.name(&self.models);
        let Some(operand) = operand else {
            let symbol = if symbol == "neg" { "-" } else { symbol };
            return Error::StdPath {
                code: "E0600",
                message: format!("cannot apply unary operator `{symbol}` to type `{left}`").into(),
                label: Some(format!("cannot apply unary operator `{symbol}`").into()),
                note: None,
                reference: operator,
            };
        };
        if symbol.ends_with('=') {
            return Error::StdPath {
                code: "E0368",
                message: format!(
                    "binary assignment operation `{symbol}` cannot be applied to type `{left}`"
                )
                .into(),
                label: None,
                note: None,
                reference: expression,
            };
        }
        let right = operand.name(&self.models);
        let message = arithmetic_trait(symbol, &left, &right).map_or_else(
            || format!("no implementation for `{left} {symbol} {right}`"),
            |(message, _)| message,
        );
        if impls == 0 {
            return Error::StdPath {
                code: "E0369",
                message: message.into(),
                label: None,
                note: None,
                reference: operator,
            };
        }
        Error::Unimplemented {
            message: message.into(),
            label: format!("no implementation for `{left} {symbol} {right}`").into(),
            help: Some(
                format!("the trait `{trait_name}<{right}>` is not implemented for `{left}`").into(),
            ),
            at: operator,
        }
    }

    /// `value[index]` on a crate type whose `index_mut` is `&mut <place>`
    /// (`fn index_mut(&mut self, i: usize) -> &mut T { &mut self.cells[i] }`):
    /// that place, `self` the indexed local and the parameter the index, so
    /// it is read, assigned and changed in place as the place itself is,
    /// with the rest of the postfix chain. `None` for any other value.
    pub(super) fn indexed_place(
        &mut self,
        value: &ParsedExpr,
    ) -> Result<Option<ParsedExpr>, Error> {
        let mut owner_ty = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } | Ty::Boxed(target) = owner_ty {
            owner_ty = self.inference.resolve(&target);
        }
        let Ty::Script(owner, _) = owner_ty else {
            return Ok(None);
        };
        let Some((parameter, body)) = self.models.index_mut.get(&owner).cloned() else {
            return Ok(None);
        };
        let receiver = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) if binding.as_str() == "self" => Token::SelfValue,
            None if value.source.as_str() == "self" => Token::SelfValue,
            Some(Lvalue::Binding { binding, .. }) => {
                Token::Identifier(Box::new(binding.as_str().into()))
            }
            _ => return Ok(None),
        };
        let engine = crate::Engine::new_raw();
        let inputs = [body.as_str()];
        let (mut stream, _) = engine.lex_raw(&inputs, None);
        let mut body_tokens = Vec::new();
        loop {
            let (token, position) = stream.next().unwrap_or((Token::EOF, Position::NONE));
            if token == Token::EOF {
                break;
            }
            body_tokens.push((token, position));
        }
        // `{ &mut place }`.
        let place = match &body_tokens[..] {
            [(Token::LeftBrace, _), (Token::Ampersand, _), (Token::Mut, _), place @ .., (Token::RightBrace, _)]
                if !place.is_empty() =>
            {
                place.to_vec()
            }
            _ => return Ok(None),
        };
        // The index's own tokens, up to its `]`.
        let (_, open) = self.next();
        let mut index = Vec::new();
        let mut depth = 0usize;
        loop {
            let (token, position) = self.next();
            match token {
                Token::EOF => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "an index without its ']'".into(),
                        source: point_span(open),
                    })
                }
                Token::RightBracket if depth == 0 => break,
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
            index.push((token, position));
        }
        let at = value.span.start();
        let mut spliced = Vec::new();
        for (token, _) in place {
            match token {
                Token::SelfValue => spliced.push((receiver.clone(), at)),
                Token::Identifier(name) if name.as_str() == parameter.as_str() => {
                    spliced.push((Token::LeftParen, open));
                    spliced.extend(index.iter().cloned());
                    spliced.push((Token::RightParen, open));
                }
                token => spliced.push((token, at)),
            }
        }
        self.splice_tokens(spliced);
        self.parse_postfix().map(Some)
    }

    /// `value[index]` on a crate type through its `impl Index<Idx>`: the
    /// impl whose `Idx` the index fits, the value its `Output`. Changing
    /// the element through `IndexMut` is not provided (it is not a place).
    pub(super) fn index_impl(
        &mut self,
        value: &ParsedExpr,
        index: &mut ParsedExpr,
        end: Position,
    ) -> Result<Option<ParsedExpr>, Error> {
        let mut owner_ty = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } | Ty::Boxed(target) = owner_ty {
            owner_ty = self.inference.resolve(&target);
        }
        let Ty::Script(owner, _) = owner_ty else {
            return Ok(None);
        };
        let Some(definitions) = self.models.trait_methods.get(&(owner, "index".into())) else {
            return Ok(None);
        };
        let candidates = definitions
            .iter()
            .filter(|definition| {
                definition
                    .trait_name()
                    .is_some_and(|implemented| implemented == "Index")
            })
            .cloned()
            .collect::<Vec<_>>();
        let expression = span(value.span.start(), end);
        let index_ty = self.inference.resolve(&index.ty);
        let mut chosen = None;
        for candidate in candidates {
            let probe = candidate
                .generics()
                .iter()
                .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
                .collect();
            let Some(parameter) = candidate.parameters().first() else {
                continue;
            };
            let parameter = instantiate(&Ty::from_resolved(&parameter.ty), &probe);
            if self.unifies(&parameter, &index_ty) {
                chosen = Some(candidate);
                break;
            }
        }
        let Some(definition) = chosen else {
            return Err(Error::MissingCapability {
                owner: owner_ty.name(&self.models),
                capability: format!("Index<{}>", index_ty.name(&self.models)).into(),
                operation: expression,
                derive: matches!(owner_ty, Ty::Script(..)),
                required_by: None,
            });
        };
        let substitution = self.method_substitution(&definition, Some(&owner_ty), expression)?;
        if let Some(parameter) = definition.parameters().first() {
            let expected = instantiate(&Ty::from_resolved(&parameter.ty), &substitution);
            self.coerce_expr(&expected, index)?;
        }
        let source = Self::std_call(
            &definition,
            value.rendered_source(),
            &[index.rendered_source()],
        );
        let ty = match instantiate(&Ty::from_resolved(definition.output()), &substitution) {
            Ty::Reference { target, .. } => *target,
            ty => ty,
        };
        Ok(Some(self.operator_expr(source, ty, expression)))
    }
}

pub(super) fn kind_code(ty: Ty) -> INT {
    match ty {
        Ty::Int(Int::I128) => WIDE_I128,
        Ty::Int(Int::U128) => WIDE_U128,
        Ty::Int(kind) => Int::KINDS
            .iter()
            .position(|(_, candidate)| *candidate == kind)
            .map_or(3, |index| index as INT),
        Ty::Float(Float::F32) => 10,
        Ty::Char => 12,
        Ty::Bool => 15,
        Ty::String | Ty::Str => 16,
        _ => 11,
    }
}

/// rustc's E0600 for `-` of the unsigned integer type `ty` at
/// `expression`, the minus and its operand: unsigned integers have no
/// `Neg` impl.
pub(super) fn unsigned_negation(ty: Int, expression: Span) -> Error {
    Error::StdPath {
        code: "E0600",
        message: format!("cannot apply unary operator `-` to type `{}`", ty.name()).into(),
        label: Some("cannot apply unary operator `-`".into()),
        note: Some("unsigned values cannot be negated".into()),
        reference: expression,
    }
}

/// An operator a crate type's impls of its trait don't cover.
struct Operation<'a> {
    /// The operator, `neg` for unary minus.
    symbol: &'a str,
    /// Its `std::ops` trait.
    trait_name: &'a str,
    /// How many impls of the trait the type has.
    impls: usize,
}

/// rustc's message for arithmetic `symbol` of `left` and `right` that no
/// impl covers, and the operator's trait.
pub(super) fn arithmetic_trait(
    symbol: &str,
    left: &str,
    right: &str,
) -> Option<(String, &'static str)> {
    Some(match symbol {
        "+" => (format!("cannot add `{right}` to `{left}`"), "Add"),
        "-" => (format!("cannot subtract `{right}` from `{left}`"), "Sub"),
        "*" => (format!("cannot multiply `{left}` by `{right}`"), "Mul"),
        "/" => (format!("cannot divide `{left}` by `{right}`"), "Div"),
        "%" => (
            format!("cannot calculate the remainder of `{left}` divided by `{right}`"),
            "Rem",
        ),
        _ => return None,
    })
}

/// `value as` the kind of `code` (`kind_code`), Rust's `as`: integers wrap
/// to the width, floats saturate (`NaN` is `0`), `char` and `bool` are their
/// numbers, a `u8` is a `char`, an `f32` is rounded.
pub(super) fn cast_value(value: &Dynamic, code: INT) -> RawrResult {
    let fail = |message: &str| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    // Into an `i128` or a `u128` (from a `u64`'s or a `usize`'s bits when
    // `UNSIGNED_SOURCE` is added), or out of a boxed one.
    let (code, source) = match code {
        code if code >= UNSIGNED_SOURCE => (code - UNSIGNED_SOURCE, Int::U64),
        code => (code, Int::I64),
    };
    let target = Int::from_code(code).filter(|kind| kind.wide());
    if let (Some(target), Ok(float)) = (target, value.as_float()) {
        // Rust's `as` saturates, and `NaN` is `0`.
        return Ok(if target == Int::I128 {
            wide_i128(float as i128)
        } else {
            wide_u128(float as u128)
        });
    }
    if (target.is_some() || matches!(value.0, Union::Variant(..)))
        && let Some(number) = wide_value(value).map(|_| kind_value(value, source))
    {
        return Ok(match (code, Int::from_code(code)) {
            (_, Some(kind)) => number.store(kind),
            (10, _) => Dynamic::from_float(match number {
                Wide::Signed(value) => value as f32 as crate::FLOAT,
                Wide::Unsigned(value) => value as f32 as crate::FLOAT,
            }),
            (12, _) => Dynamic::from(char::from(number.unsigned() as u8)),
            _ => Dynamic::from_float(match number {
                Wide::Signed(value) => value as crate::FLOAT,
                Wide::Unsigned(value) => value as crate::FLOAT,
            }),
        });
    }
    if let Ok(float) = value.as_float() {
        return Ok(match code {
            10 => Dynamic::from_float(float as f32 as crate::FLOAT),
            11 => Dynamic::from_float(float),
            _ if matches!(Int::KINDS[code as usize].1, Int::U64 | Int::Usize) => {
                (float as u64 as INT).into()
            }
            _ => {
                let (low, high) = Int::KINDS[code as usize].1.range();
                let saturated = if float.is_nan() {
                    0
                } else if float <= low as crate::FLOAT {
                    low
                } else if float >= high as crate::FLOAT {
                    high
                } else {
                    float as INT
                };
                saturated.into()
            }
        });
    }
    let integer = match (value.as_int(), value.as_char(), value.as_bool()) {
        (Ok(integer), ..) => integer,
        (_, Ok(c), _) => INT::from(u32::from(c)),
        (.., Ok(b)) => INT::from(b),
        _ => return fail("an `as` operand is not a number at runtime"),
    };
    Ok(match code {
        10 => Dynamic::from_float(integer as f32 as crate::FLOAT),
        11 => Dynamic::from_float(integer as crate::FLOAT),
        12 => Dynamic::from(char::from(integer as u8)),
        // A char range's item by its code point (`iterator::CHAR_OF_CODE`).
        15 => {
            match u32::try_from(integer).ok().and_then(char::from_u32) {
                Some(c) => Dynamic::from(c),
                None => return fail(
                    "a `char` range across the surrogates U+D800 to U+DFFF is not supported in \
                     Rawr yet",
                ),
            }
        }
        // A `u64`'s or a `usize`'s bits as an `f32` or an `f64`.
        13 => Dynamic::from_float(integer as u64 as f32 as crate::FLOAT),
        14 => Dynamic::from_float(integer as u64 as crate::FLOAT),
        // A `char` or a `bool` into an `i128` or a `u128`.
        _ if target.is_some() => integer.into(),
        _ => {
            let kind = Int::KINDS[code as usize].1;
            let wrapped = match kind {
                Int::I8 => integer as i8 as INT,
                Int::I16 => integer as i16 as INT,
                Int::I32 => integer as i32 as INT,
                Int::U8 => integer as u8 as INT,
                Int::U16 => integer as u16 as INT,
                Int::U32 => integer as u32 as INT,
                _ => integer,
            };
            wrapped.into()
        }
    })
}

pub(super) fn compound_operator(assignment: &Token) -> Option<Token> {
    Some(match assignment {
        Token::PlusAssign => Token::Plus,
        Token::MinusAssign => Token::Minus,
        Token::MultiplyAssign => Token::Multiply,
        Token::DivideAssign => Token::Divide,
        Token::ModuloAssign => Token::Modulo,
        Token::AndAssign => Token::Ampersand,
        Token::OrAssign => Token::Pipe,
        Token::XOrAssign => Token::XOr,
        Token::LeftShiftAssign => Token::LeftShift,
        Token::RightShiftAssign => Token::RightShift,
        _ => return None,
    })
}

pub(super) fn binary_precedence(token: &Token) -> Option<u8> {
    Some(match token {
        Token::Or => 1,
        Token::And => 2,
        Token::EqualsTo
        | Token::NotEqualsTo
        | Token::LessThan
        | Token::GreaterThan
        | Token::LessThanEqualsTo
        | Token::GreaterThanEqualsTo => COMPARISON,
        Token::Pipe => 4,
        Token::XOr => 5,
        Token::Ampersand => 6,
        Token::LeftShift | Token::RightShift => 7,
        Token::Plus | Token::Minus => 8,
        Token::Multiply | Token::Divide | Token::Modulo => 9,
        _ => return None,
    })
}

/// A `u64`'s or a `usize`'s bits with the sign bit flipped, which orders
/// them as `i64`s the way they order as unsigned numbers.
pub(super) fn unsigned_order(value: origin::Fragment) -> origin::Fragment {
    value.surrounded("((", ") ^ (-9223372036854775807 - 1))")
}
