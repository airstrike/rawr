//! Methods of rawr's builtin values as Rust names them: numbers, `char`,
//! `String` and `&str`, `Option`, `Result` and `bool`.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_number_method(
        &mut self,
        value: ParsedExpr,
        number: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // Rust's sign and rounding methods, through the runtime's math.
        let unary = match (&number, member.as_str()) {
            (Ty::Int(kind), "abs") if kind.signed() && !kind.wide() => {
                Some(("abs", number.clone()))
            }
            (Ty::Float(_), member)
                if let Some((_, function)) =
                    FLOAT_UNARY_METHODS.iter().find(|(name, _)| *name == member) =>
            {
                let ty = if member.starts_with("is_") {
                    Ty::Bool
                } else {
                    number.clone()
                };
                Some((*function, ty))
            }
            (Ty::Int(kind), "signum") if kind.signed() && !kind.wide() => {
                Some(("sign", number.clone()))
            }
            _ => None,
        };
        if let Some((function, ty)) = unary {
            let (arguments, end) = self.parse_call_arguments("number method arguments")?;
            if !arguments.is_empty() {
                return Err(self.unsupported("arguments to a number method that takes none"));
            }
            let source = value
                .rendered_source()
                .surrounded(&format!("{function}("), ")");
            let value = self.operator_expr(source, ty, span(value.span.start(), end));
            return Ok(self.checked_arithmetic(value, "neg"));
        }
        if let Ty::Int(kind) = number
            && let Some(code) = int_method_code(&member)
        {
            return self.parse_int_method(value, kind, code, member_span);
        }
        // `x.try_into()`: a `Result` of the integer kind inference picks.
        if let (Ty::Int(_), "try_into") = (&number, member.as_str()) {
            let (arguments, end) = self.parse_call_arguments("number method arguments")?;
            if !arguments.is_empty() {
                return Err(self.unsupported("arguments to `try_into`"));
            }
            let target = Ty::Int(Int::Var(self.inference.fresh_id()));
            let source = self.int_op(
                "try_from",
                value.rendered_source(),
                "0".into(),
                &target,
                &number,
            );
            let ty = Ty::Result(
                Box::new(target),
                Box::new(Ty::ParseError(ParseError::TryFromInt)),
            );
            return Ok(self.operator_expr(source, ty, span(value.span.start(), end)));
        }
        if let Ty::Float(_) = number
            && let Some((_, function)) = FLOAT_BINARY_METHODS
                .iter()
                .find(|(name, _)| *name == member)
        {
            let (mut arguments, end) = self.parse_call_arguments("number method arguments")?;
            let [argument] = arguments.as_mut_slice() else {
                return Err(self.unsupported("a float method without exactly one argument"));
            };
            self.coerce_expr(&number, argument)?;
            let mut source = origin::Fragment::plain(format!("{function}("));
            source.push(value.rendered_source());
            source.push_text(", ");
            source.push(argument.rendered_source());
            source.push_text(")");
            let value = self.operator_expr(source, number, span(value.span.start(), end));
            return Ok(self.checked_arithmetic(value, "neg"));
        }
        let exponent = match (&number, member.as_str()) {
            (Ty::Int(_), "pow") => Ty::Int(Int::U32),
            (Ty::Float(_), "powi") => Ty::Int(Int::I32),
            (Ty::Float(_), "powf") => number.clone(),
            _ => {
                return Err(Error::UnknownMember {
                    owner: number.name(&self.models),
                    member,
                    reference: member_span,
                })
            }
        };
        let (arguments, end) = self.parse_call_arguments("number method arguments")?;
        let [argument] = arguments.as_slice() else {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified(&number.name(&self.models), &member).to_string(),
                1,
                arguments.len(),
                span(member_span.start(), end),
            ));
        };
        self.require_type(&exponent, &argument.ty, argument.span)?;
        // A `u64`'s bits and an `i128` or a `u128` past `i64`: `INT_METHODS`.
        if let Ty::Int(Int::U64 | Int::Usize | Int::I128 | Int::U128) = number {
            let expression = span(value.span.start(), end);
            let source = self.int_op(
                "**",
                value.rendered_source(),
                argument.rendered_source(),
                &number,
                &exponent,
            );
            return Ok(self.operator_expr(source, number, expression));
        }
        let mut source = value.rendered_source().surrounded("((", ") ** (");
        source.push(argument.rendered_source());
        source.push_text("))");
        let value = self.operator_expr(source, number, span(value.span.start(), end));
        Ok(self.checked_arithmetic(value, "pow"))
    }

    /// An integer method of `INT_METHODS` on `value` of `kind`.
    pub(super) fn parse_int_method(
        &mut self,
        value: ParsedExpr,
        kind: Int,
        code: usize,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (name, operand, output) = INT_METHODS[code];
        let (mut arguments, end) = self.parse_call_arguments("integer method arguments")?;
        let ty = Ty::Int(kind);
        let operand_ty = if operand == 'u' {
            Ty::Int(Int::U32)
        } else {
            ty.clone()
        };
        let argument = match (operand, arguments.as_mut_slice()) {
            ('n', []) => origin::Fragment::plain("0"),
            ('s' | 'u', [argument]) => {
                self.coerce_expr(&operand_ty, argument)?;
                argument.rendered_source()
            }
            _ => {
                return Err(Error::argument_count(
                    Callee::Method,
                    crate::resolve::Qualified(kind.name(), name).to_string(),
                    usize::from(operand != 'n'),
                    arguments.len(),
                    span(member_span.start(), end),
                ))
            }
        };
        if name == "is_power_of_two" && kind.signed()
            || SIGNED_INT_METHODS.contains(&name) && !kind.signed()
        {
            return Err(Error::UnknownMember {
                owner: kind.name().into(),
                member: name.into(),
                reference: member_span,
            });
        }
        let output = match output {
            'o' => Ty::Option(Box::new(ty.clone())),
            'p' => Ty::Tuple(Box::new([ty.clone(), Ty::Bool])),
            'u' => Ty::Int(Int::U32),
            'b' => Ty::Bool,
            'a' => Ty::Int(kind.unsigned()),
            _ => ty.clone(),
        };
        let source = self.int_op(name, value.rendered_source(), argument, &ty, &operand_ty);
        Ok(self.operator_expr(source, output, span(value.span.start(), end)))
    }

    /// `char`'s classification and case methods (`CHAR_METHODS`), through
    /// one native op; `to_digit` and `is_digit` take a radix.
    /// `RangeInclusive`'s `start()`, `end()`, `contains(&x)`, `is_empty()`
    /// and `clone()`; any other method is an iterator's, on the range's items.
    pub(super) fn parse_range_method(
        &mut self,
        value: ParsedExpr,
        range: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // An integer range keeps its kind. A float range's kind is not kept:
        // `start()` of a `RangeInclusive<f32>` is an `f32` where one is
        // expected.
        let element = match range {
            Ty::InclusiveRange(kind) => Ty::Int(kind),
            _ => Ty::Float(Float::Var(self.inference.fresh_id())),
        };
        let code = match member.as_str() {
            "start" => 0,
            "end" => 1,
            "contains" => 2,
            "is_empty" => 3,
            "clone" => 4,
            _ => {
                if range == Ty::FloatRange {
                    return Err(Error::UnknownMember {
                        owner: "RangeInclusive<f64>".into(),
                        member,
                        reference: member_span,
                    });
                }
                let Some(items) = self.iterator_of(&value) else {
                    return Err(self.unsupported("this method of a range"));
                };
                return self.parse_iterator_method(items, member, member_span);
            }
        };
        let (mut arguments, end) = self.parse_call_arguments("range method arguments")?;
        let expected = if code == 2 { 1 } else { 0 };
        if arguments.len() != expected {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified(&range.name(&self.models), &member).to_string(),
                expected,
                arguments.len(),
                span(member_span.start(), end),
            ));
        }
        let call = span(value.span.start(), end);
        if code == 4 {
            return Ok(self.operator_expr(value.rendered_source(), range, call));
        }
        let argument = match arguments.first_mut() {
            Some(argument) => {
                let reference = Ty::Reference {
                    mutable: false,
                    target: Box::new(element.clone()),
                };
                self.coerce_expr(&reference, argument)?;
                argument.rendered_source()
            }
            None => "()".into(),
        };
        let source = self.prelude_op(
            PreludeOp::RangeMethod,
            [value.rendered_source(), code.to_string().into(), argument],
        );
        let ty = match code {
            0 | 1 => Ty::Reference {
                mutable: false,
                target: Box::new(element),
            },
            _ => Ty::Bool,
        };
        Ok(self.operator_expr(source, ty, call))
    }

    pub(super) fn parse_char_method(
        &mut self,
        value: ParsedExpr,
        code: usize,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("char method arguments")?;
        let (name, output) = CHAR_METHODS[code];
        let argument = match (name, arguments.as_mut_slice()) {
            ("to_digit" | "is_digit", [radix]) => {
                self.coerce_expr(&Ty::Int(Int::U32), radix)?;
                radix.rendered_source()
            }
            ("eq_ignore_ascii_case", [other]) => {
                let expected = Ty::Reference {
                    mutable: false,
                    target: Box::new(Ty::Char),
                };
                self.coerce_expr(&expected, other)?;
                other.rendered_source()
            }
            (_, []) if !matches!(name, "to_digit" | "is_digit" | "eq_ignore_ascii_case") => {
                "()".into()
            }
            _ => {
                return Err(Error::argument_count(
                    Callee::Method,
                    crate::resolve::Qualified("char", name).to_string(),
                    usize::from(matches!(
                        name,
                        "to_digit" | "is_digit" | "eq_ignore_ascii_case"
                    )),
                    arguments.len(),
                    span(member_span.start(), end),
                ))
            }
        };
        let ty = match output {
            'c' => Ty::Char,
            'b' => Ty::Bool,
            'u' => Ty::Int(Int::Usize),
            _ => Ty::Option(Box::new(Ty::Int(Int::U32))),
        };
        let source = self.prelude_op(
            PreludeOp::CharMethod,
            [value.rendered_source(), code.to_string().into(), argument],
        );
        Ok(self.operator_expr(source, ty, span(value.span.start(), end)))
    }

    /// The `String` methods Rust code reaches for, with Rust results.
    /// `Trait::method(text, ..)` of the crate trait at `trait_path`: the
    /// function the facade declares for the method of the trait's impl for
    /// `&str`, with the text as its first input (`surface::text`). A trait
    /// whose impls for `&str` weren't extracted yet misses `trait::member`,
    /// which the importer demands.
    #[cfg(feature = "rust-import")]
    pub(super) fn parse_text_member(
        &mut self,
        receiver: ParsedExpr,
        trait_path: &ImmutableString,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let path = rawr_import::surface::text::path(trait_path, &member);
        let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec) else {
            let error = Error::UnknownMember {
                owner: trait_path.clone(),
                member,
                reference: member_span,
            };
            return Err(self.native_miss(true, error));
        };
        let owner = self.models.item_owner(&path).cloned();
        let start = receiver.span.start();
        let (arguments, end) = self.parse_call_arguments("trait method arguments")?;
        let mut inputs = Vec::with_capacity(arguments.len() + 1);
        inputs.push(receiver);
        for mut argument in arguments {
            self.move_local(&mut argument)?;
            inputs.push(argument);
        }
        self.call_host_function(&items, owner, inputs, span(start, end))
    }

    pub(super) fn parse_string_method(
        &mut self,
        value: ParsedExpr,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if member.as_str() == "parse" {
            return self.parse_str_parse(value);
        }
        let (mut arguments, end) =
            self.parse_std_arguments(&value.ty, &member, "string method arguments")?;
        for argument in &mut arguments {
            self.strip_string_reference(argument);
        }
        let receiver = value.rendered_source().surrounded("(", ")");
        let expression = span(value.span.start(), end);
        // `s.retain(f)`: the chars `f` keeps, in order, stored back as `s`.
        if let ("retain", [filter]) = (member.as_str(), arguments.as_mut_slice()) {
            let expected = Ty::Fn {
                inputs: Box::new([Ty::Char]),
                output: Box::new(Ty::Bool),
            };
            self.coerce_expr(&expected, filter)?;
            let helper = self.push_op(Op::Prelude(PreludeOp::StrChange));
            let code = STR_CHANGES
                .iter()
                .position(|name| *name == "retain")
                .expect("listed String operation");
            let native = self.mutation_call(
                &value,
                &helper,
                vec![
                    code.to_string().into(),
                    "()".into(),
                    filter.rendered_source(),
                ],
            )?;
            return Ok(self.operator_expr(native.surrounded("{ ", "; }"), Ty::Unit, expression));
        }
        if let ("split_at", [mid]) = (member.as_str(), arguments.as_mut_slice()) {
            self.coerce_expr(&Ty::Int(Int::Usize), mid)?;
            let source = self.prelude_op(PreludeOp::SplitAt, [receiver, mid.rendered_source()]);
            let ty = Ty::Tuple(Box::new([Ty::Str, Ty::Str]));
            return Ok(self.operator_expr(source, ty, expression));
        }
        let argument_type = arguments
            .first()
            .map(|argument| self.inference.resolve(&argument.ty));
        let (rendered, ty) = match (member.as_str(), arguments.as_slice()) {
            ("len", []) => (receiver.surrounded("", ".bytes"), Ty::Int(Int::Usize)),

            ("is_empty", []) => (receiver.surrounded("(", ".len() == 0)"), Ty::Bool),
            ("to_string" | "clone" | "as_str", [])
                if without_references(&self.inference.resolve(&value.ty)).is_text()
                    || member.as_str() != "to_string" =>
            {
                // `as_str` borrows, and a `&str`'s clone is a `&str`.
                let borrowed = member.as_str() == "as_str"
                    || member.as_str() == "clone"
                        && *without_references(&self.inference.resolve(&value.ty)) != Ty::String;
                let source = if borrowed {
                    receiver
                } else {
                    self.prelude_op(PreludeOp::ValueClone, [receiver])
                };
                (source, if borrowed { Ty::Str } else { Ty::String })
            }
            ("to_string", [])
                if matches!(
                    without_references(&self.inference.resolve(&value.ty)),
                    Ty::Int(Int::U64 | Int::Usize)
                ) =>
            {
                let digits = self.prelude_op(PreludeOp::FormatRadix, [receiver, "10".into()]);
                (digits, Ty::String)
            }
            // `Display` of a float: Rust's text (`1` for `1.0`).
            ("to_string", [])
                if let Ty::Float(kind) = without_references(&self.inference.resolve(&value.ty)) =>
            {
                let mode = if *kind == Float::F32 { "2" } else { "0" };
                let text =
                    self.prelude_op(PreludeOp::FormatFloat, [receiver, "-1".into(), mode.into()]);
                (text, Ty::String)
            }
            ("to_string", []) => (receiver.surrounded("", ".to_string()"), Ty::String),
            ("to_uppercase", []) => (receiver.surrounded("", ".to_upper()"), Ty::String),
            ("chars", []) => return Ok(self.chars_iterator(&value, end)),
            // Iterators over the string's pieces, as `&str`s.
            ("lines" | "split_whitespace", []) | ("split", [_])
                if member.as_str() != "split"
                    || matches!(argument_type, Some(Ty::String | Ty::Str | Ty::Char)) =>
            {
                let op = match member.as_str() {
                    "lines" => PreludeOp::StrLines,
                    "split_whitespace" => PreludeOp::StrSplitWhitespace,
                    _ => PreludeOp::StrSplit,
                };
                let operands = std::iter::once(value.rendered_source())
                    .chain(arguments.iter().map(ParsedExpr::rendered_source))
                    .collect::<Vec<_>>();
                let pieces = self.prelude_op(op, operands);
                let pieces = self.operator_expr(
                    pieces,
                    Ty::Array {
                        element: Box::new(Ty::Str),
                        kind: sequence::Kind::Vec,
                    },
                    expression,
                );
                return Ok(self.array_iterator(&pieces, Ty::Str, end));
            }
            ("to_lowercase", []) => (receiver.surrounded("", ".to_lower()"), Ty::String),
            ("to_owned", []) => (
                self.prelude_op(PreludeOp::ValueClone, [receiver]),
                Ty::String,
            ),
            // `Cow<str>::into_owned`: an imported `Cow<'_, str>` is Rawr's
            // string (D558 slice 2).
            ("into_owned", []) => (receiver, Ty::String),
            (name, _)
                if let Some(code) = STR_METHODS.iter().position(|(method, ..)| *method == name) =>
            {
                let (_, count, output) = STR_METHODS[code];
                if arguments.len() != count {
                    return Err(Error::argument_count(
                        Callee::Method,
                        crate::resolve::Qualified("str", &member).to_string(),
                        count,
                        arguments.len(),
                        span(member_span.start(), end),
                    ));
                }
                let mut operands = vec![value.rendered_source(), code.to_string().into()];
                for argument in &mut arguments {
                    let expected = match (name, self.inference.resolve(&argument.ty)) {
                        ("repeat" | "is_char_boundary", _) => Ty::Int(Int::Usize),
                        (_, Ty::Char) => Ty::Char,
                        _ => Ty::Str,
                    };
                    self.coerce_expr(&expected, argument)?;
                    operands.push(argument.rendered_source());
                }
                operands.resize_with(4, || "()".into());
                // The trims borrow from the receiver; the others make a
                // new `String`.
                let ty = match output {
                    's' if name.starts_with("trim") => Ty::Str,
                    's' => Ty::String,
                    'b' => Ty::Bool,
                    'f' => Ty::Option(Box::new(Ty::Int(Int::Usize))),
                    'o' => Ty::Option(Box::new(Ty::Str)),
                    _ => Ty::Option(Box::new(Ty::Tuple(Box::new([Ty::Str, Ty::Str])))),
                };
                (self.prelude_op(PreludeOp::StrMethod, operands), ty)
            }
            ("contains" | "starts_with" | "ends_with", [argument])
                if matches!(argument_type, Some(Ty::String | Ty::Str | Ty::Char)) =>
            {
                let mut call = receiver;
                call.push_text(format!(".{member}("));
                call.push(argument.rendered_source());
                call.push_text(")");
                (call, Ty::Bool)
            }
            // A `String` changed in place, by bytes, with Rust's panics.
            ("pop" | "clear", [])
            | ("insert" | "insert_str", [_, _])
            | ("remove" | "truncate" | "split_off", [_]) => {
                let code = STR_CHANGES
                    .iter()
                    .position(|name| *name == member.as_str())
                    .expect("a listed `String` method");
                let mut operands = vec![origin::Fragment::from(code.to_string())];
                for (index, argument) in arguments.iter_mut().enumerate() {
                    let expected = match (member.as_str(), index) {
                        ("insert", 1) => Ty::Char,
                        ("insert_str", 1) => Ty::Str,
                        _ => Ty::Int(Int::Usize),
                    };
                    self.coerce_expr(&expected, argument)?;
                    operands.push(argument.rendered_source());
                }
                operands.resize(3, "()".into());
                let helper = self.push_op(Op::Prelude(PreludeOp::StrChange));
                let source = self.mutation_call(&value, &helper, operands)?;
                let ty = match member.as_str() {
                    "pop" => Ty::Option(Box::new(Ty::Char)),
                    "remove" => Ty::Char,
                    "split_off" => Ty::String,
                    _ => Ty::Unit,
                };
                (source, ty)
            }
            // `bytes()`, `char_indices()`, `splitn(n, pattern)` and `rsplitn(n, pattern)`: iterators
            // over the string's pieces; `as_bytes()` a `&[u8]` and
            // `into_bytes()` a `Vec<u8>`.
            ("bytes" | "char_indices" | "as_bytes" | "into_bytes", [])
            | ("splitn" | "rsplitn", [_, _]) => {
                let code = STR_PIECES
                    .iter()
                    .position(|name| *name == member.as_str())
                    .unwrap_or(0);
                let mut operands = vec![value.rendered_source(), code.to_string().into()];
                if let [count, pattern] = arguments.as_mut_slice() {
                    self.coerce_expr(&Ty::Int(Int::Usize), count)?;
                    let expected = match self.inference.resolve(&pattern.ty) {
                        Ty::Char => Ty::Char,
                        _ => Ty::Str,
                    };
                    self.coerce_expr(&expected, pattern)?;
                    operands.push(count.rendered_source());
                    operands.push(pattern.rendered_source());
                }
                operands.resize(4, "()".into());
                let pieces = self.prelude_op(PreludeOp::StrPieces, operands);
                let item = match member.as_str() {
                    "bytes" | "as_bytes" | "into_bytes" => Ty::Int(Int::U8),
                    "char_indices" => Ty::Tuple(Box::new([Ty::Int(Int::Usize), Ty::Char])),
                    _ => Ty::Str,
                };
                let array = Ty::Array {
                    element: Box::new(item.clone()),
                    kind: sequence::Kind::Vec,
                };
                if member.as_str() == "into_bytes" {
                    return Ok(self.operator_expr(pieces, array, expression));
                }
                if member.as_str() == "as_bytes" {
                    let slice = Ty::Reference {
                        mutable: false,
                        target: Box::new(Ty::sequence(sequence::Kind::Slice, item)),
                    };
                    return Ok(self.operator_expr(pieces, slice, expression));
                }
                let pieces = self.operator_expr(pieces, array, expression);
                return Ok(self.array_iterator(&pieces, item, end));
            }
            ("push_str" | "push", [_]) => {
                let expected = if member.as_str() == "push" {
                    Ty::Char
                } else {
                    Ty::Str
                };
                self.coerce_expr(&expected, &mut arguments[0])?;
                let argument = &arguments[0];
                let Some(Lvalue::Binding { binding, .. }) = &value.lvalue else {
                    // A field, an element or a nested place: changed in
                    // place and stored back like the other `String` changes.
                    let code = STR_CHANGES
                        .iter()
                        .position(|name| *name == member.as_str())
                        .expect("a listed `String` method");
                    let operands = vec![
                        origin::Fragment::from(code.to_string()),
                        "()".into(),
                        argument.rendered_source(),
                    ];
                    let helper = self.push_op(Op::Prelude(PreludeOp::StrChange));
                    let source = self.mutation_call(&value, &helper, operands)?;
                    return Ok(self.operator_expr(source, Ty::Unit, expression));
                };
                let Some(local) = self.bindings.get(binding) else {
                    return Err(self.unsupported("assignment through an unknown binding"));
                };
                if !local.mutable {
                    return Err(Error::borrow_immutable(binding, value.span));
                }
                self.mutable_use(&value, expression);
                let mut call = origin::Fragment::plain(format!("{{ {binding} += "));
                call.push(argument.rendered_source());
                call.push_text("; }");
                (call, Ty::Unit)
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: "String".into(),
                    member,
                    reference: member_span,
                })
            }
        };
        Ok(self.operator_expr(rendered, ty, expression))
    }

    /// The `Option` methods Rust code reaches for, as Rust names them. Each
    /// composes the runtime primitives; arguments are evaluated once, after
    /// the receiver.
    pub(super) fn parse_option_method(
        &mut self,
        mut value: ParsedExpr,
        element: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let owner = Ty::Option(Box::new(element.clone()));
        let keeps_place = matches!(member.as_str(), "unwrap" | "expect")
            && matches!(
                self.inference.resolve(&element),
                Ty::Reference { mutable: true, .. }
            );
        let reference = Ty::Reference {
            mutable: false,
            target: Box::new(element.clone()),
        };
        self.script_callables = true;
        let (mut arguments, end) =
            self.parse_std_arguments(&owner, &member, "`Option` method arguments")?;
        self.builtin_enum_receiver(&mut value, None, &member, &arguments, member_span)?;
        let expression = span(value.span.start(), end);
        let arity = |expected: usize, arguments: &[ParsedExpr]| {
            if arguments.len() == expected {
                Ok(())
            } else {
                Err(Error::argument_count(
                    Callee::Method,
                    crate::resolve::Qualified("Option", &member).to_string(),
                    expected,
                    arguments.len(),
                    span(member_span.start(), end),
                ))
            }
        };
        let o = self.temporary("option");
        let is = |this: &mut Self| this.prelude_op(PreludeOp::IsSome, [o.as_str().into()]);
        let unwrap = |this: &mut Self| this.prelude_op(PreludeOp::Unwrap, [o.as_str().into()]);
        let mut source = origin::Fragment::plain(format!("{{ let {o} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        let ty = match member.as_str() {
            "is_some" | "is_none" => {
                arity(0, &arguments)?;
                if member.as_str() == "is_none" {
                    source.push_text("!");
                }
                let test = is(self);
                source.push(test);
                Ty::Bool
            }
            "unwrap" => {
                arity(0, &arguments)?;
                let unwrapped = unwrap(self);
                source.push(unwrapped);
                element
            }
            "expect" => {
                arity(1, &arguments)?;
                self.coerce_expr(&Ty::Str, &mut arguments[0])?;
                let expected = self.prelude_op(
                    PreludeOp::Expect,
                    [o.as_str().into(), arguments[0].rendered_source()],
                );
                source.push(expected);
                element
            }
            "unwrap_or" => {
                arity(1, &arguments)?;
                self.coerce_expr(&element, &mut arguments[0])?;
                // The default, unused, drops.
                if self.needs_drop(&element, member_span)? {
                    self.move_local(&mut arguments[0])?;
                }
                source.push_text("let __rawr_or = ");
                source.push(arguments[0].rendered_source());
                source.push_text("; if ");
                let test = is(self);
                source.push(test);
                source.push_text(" { ");
                if self.needs_drop(&element, member_span)? {
                    source.push_text(drops::glue_call("__rawr_or", &self.drop_shape(&element)));
                    source.push_text(" ");
                }
                let unwrapped = unwrap(self);
                source.push(unwrapped);
                source.push_text(" } else { __rawr_or }");
                element
            }
            "unwrap_or_default" => {
                arity(0, &arguments)?;
                let default = self.default_source(&element, member_span)?;
                source.push_text("if ");
                let test = is(self);
                source.push(test);
                source.push_text(" { ");
                let unwrapped = unwrap(self);
                source.push(unwrapped);
                source.push_text(" } else { ");
                source.push(default);
                source.push_text(" }");
                element
            }
            "unwrap_or_else" => {
                arity(1, &arguments)?;
                let call = self.callable_call(&mut arguments[0], &[], &element, Vec::new())?;
                source.push_text("if ");
                let test = is(self);
                source.push(test);
                source.push_text(" { ");
                let unwrapped = unwrap(self);
                source.push(unwrapped);
                source.push_text(" } else { ");
                source.push(call);
                source.push_text(" }");
                element
            }
            "map" | "and_then" => {
                arity(1, &arguments)?;
                let output = if member.as_str() == "map" {
                    self.inference.fresh()
                } else {
                    Ty::Option(Box::new(self.inference.fresh()))
                };
                let unwrapped = unwrap(self);
                let call = self.callable_call(
                    &mut arguments[0],
                    std::slice::from_ref(&element),
                    &output,
                    vec![unwrapped],
                )?;
                let output = self.inference.resolve(&output);
                source.push_text("if ");
                let test = is(self);
                source.push(test);
                source.push_text(" { ");
                let result = if member.as_str() == "map" {
                    self.prelude_op(PreludeOp::Some, [call])
                } else {
                    call
                };
                source.push(result);
                source.push_text(" } else { ");
                let none = self.prelude_op(PreludeOp::None, []);
                source.push(none);
                source.push_text(" }");
                if member.as_str() == "map" {
                    Ty::Option(Box::new(output))
                } else {
                    output
                }
            }
            "map_or" => {
                arity(2, &arguments)?;
                let output = self.inference.resolve(&arguments[0].ty);
                let unwrapped = unwrap(self);
                let call = self.callable_call(
                    &mut arguments[1],
                    std::slice::from_ref(&element),
                    &output,
                    vec![unwrapped],
                )?;
                source.push_text("let __rawr_or = ");
                source.push(arguments[0].rendered_source());
                source.push_text("; if ");
                let test = is(self);
                source.push(test);
                source.push_text(" { ");
                source.push(call);
                source.push_text(" } else { __rawr_or }");
                self.inference.resolve(&output)
            }
            "is_some_and" | "is_none_or" => {
                arity(1, &arguments)?;
                let unwrapped = unwrap(self);
                let call = self.callable_call(
                    &mut arguments[0],
                    std::slice::from_ref(&element),
                    &Ty::Bool,
                    vec![unwrapped],
                )?;
                let test = is(self);
                if member.as_str() == "is_none_or" {
                    source.push_text("!");
                    source.push(test);
                    source.push_text(" || ");
                } else {
                    source.push(test);
                    source.push_text(" && ");
                }
                source.push(call);
                Ty::Bool
            }
            "filter" => {
                arity(1, &arguments)?;
                let unwrapped = unwrap(self);
                let call = self.callable_call(
                    &mut arguments[0],
                    std::slice::from_ref(&reference),
                    &Ty::Bool,
                    vec![unwrapped],
                )?;
                source.push_text("if ");
                let test = is(self);
                source.push(test);
                source.push_text(" && ");
                source.push(call);
                source.push_text(format!(" {{ {o} }} else {{ "));
                let none = self.prelude_op(PreludeOp::None, []);
                source.push(none);
                source.push_text(" }");
                owner
            }
            "or" | "xor" => {
                arity(1, &arguments)?;
                self.coerce_expr(&owner, &mut arguments[0])?;
                source.push_text("let __rawr_or = ");
                source.push(arguments[0].rendered_source());
                source.push_text("; ");
                let test = is(self);
                if member.as_str() == "or" {
                    source.push_text("if ");
                    source.push(test);
                    source.push_text(format!(" {{ {o} }} else {{ __rawr_or }}"));
                } else {
                    let other = self.prelude_op(PreludeOp::IsSome, ["__rawr_or".into()]);
                    let none = self.prelude_op(PreludeOp::None, []);
                    source.push_text("if ");
                    source.push(test.clone());
                    source.push_text(" && !");
                    source.push(other.clone());
                    source.push_text(format!(" {{ {o} }} else if !"));
                    source.push(test);
                    source.push_text(" && ");
                    source.push(other);
                    source.push_text(" { __rawr_or } else { ");
                    source.push(none);
                    source.push_text(" }");
                }
                owner
            }
            "or_else" => {
                arity(1, &arguments)?;
                let call = self.callable_call(&mut arguments[0], &[], &owner, Vec::new())?;
                source.push_text("if ");
                let test = is(self);
                source.push(test);
                source.push_text(format!(" {{ {o} }} else {{ "));
                source.push(call);
                source.push_text(" }");
                owner
            }
            "and" => {
                arity(1, &arguments)?;
                let other = Ty::Option(Box::new(self.inference.fresh()));
                self.coerce_expr(&other, &mut arguments[0])?;
                source.push_text("let __rawr_and = ");
                source.push(arguments[0].rendered_source());
                source.push_text("; if ");
                let test = is(self);
                source.push(test);
                source.push_text(" { __rawr_and } else { ");
                let none = self.prelude_op(PreludeOp::None, []);
                source.push(none);
                source.push_text(" }");
                self.inference.resolve(&other)
            }
            "zip" => {
                arity(1, &arguments)?;
                let other_element = self.inference.fresh();
                let other = Ty::Option(Box::new(other_element.clone()));
                self.coerce_expr(&other, &mut arguments[0])?;
                source.push_text("let __rawr_zip = ");
                source.push(arguments[0].rendered_source());
                source.push_text("; if ");
                let test = is(self);
                source.push(test);
                source.push_text(" && ");
                let other_test = self.prelude_op(PreludeOp::IsSome, ["__rawr_zip".into()]);
                source.push(other_test);
                source.push_text(" { ");
                let left = unwrap(self);
                let right = self.prelude_op(PreludeOp::Unwrap, ["__rawr_zip".into()]);
                let mut pair = origin::Fragment::plain("[");
                pair.push(left);
                pair.push_text(", ");
                pair.push(right);
                pair.push_text("]");
                let some = self.prelude_op(PreludeOp::Some, [pair]);
                source.push(some);
                source.push_text(" } else { ");
                let none = self.prelude_op(PreludeOp::None, []);
                source.push(none);
                source.push_text(" }");
                Ty::Option(Box::new(Ty::Tuple(Box::new([
                    element,
                    self.inference.resolve(&other_element),
                ]))))
            }
            // `Some(v)` is `Ok(v)`; `None` is `Err` of the argument.
            "ok_or" | "ok_or_else" => {
                arity(1, &arguments)?;
                let error = self.inference.fresh();
                let produced = if member.as_str() == "ok_or" {
                    self.coerce_expr(&error, &mut arguments[0])?;
                    arguments[0].rendered_source()
                } else {
                    self.callable_call(&mut arguments[0], &[], &error, Vec::new())?
                };
                // `ok_or` evaluates its argument eagerly, as Rust does.
                let eager = member.as_str() == "ok_or";
                if eager {
                    source.push_text("let __rawr_or = ");
                    source.push(produced.clone());
                    source.push_text("; ");
                }
                source.push_text("if ");
                let test = is(self);
                source.push(test);
                source.push_text(" { ");
                let unwrapped = unwrap(self);
                let ok = self.prelude_op(PreludeOp::Ok, [unwrapped]);
                source.push(ok);
                source.push_text(" } else { ");
                let err = self.prelude_op(
                    PreludeOp::Err,
                    [if eager { "__rawr_or".into() } else { produced }],
                );
                source.push(err);
                source.push_text(" }");
                Ty::Result(Box::new(element), Box::new(self.inference.resolve(&error)))
            }
            "iter" | "into_iter" => {
                arity(0, &arguments)?;
                let item = self.held_item(&value.ty, &member, element);
                let parts = [is(self), unwrap(self)];
                return Ok(self.held_iterator(source, parts, item, expression));
            }
            "as_ref" | "as_mut" => {
                arity(0, &arguments)?;
                source.push_text(o.as_str());
                Ty::Option(Box::new(Ty::Reference {
                    mutable: member.as_str() == "as_mut",
                    target: Box::new(element),
                }))
            }
            // `as_deref` borrows through `Deref` (D711 step 3):
            // `Option<String>` gives `Option<&str>`, `Option<Vec<u8>>`
            // `Option<&[u8]>`, `Option<Box<T>>` `Option<&T>`.
            "as_deref" | "as_deref_mut" => {
                arity(0, &arguments)?;
                let mutable = member.as_str() == "as_deref_mut";
                let reference = Ty::Reference {
                    mutable,
                    target: Box::new(self.inference.resolve(&element)),
                };
                match self.deref_reference(&reference, false) {
                    Some((target, std_impl::DerefCall::Value)) => {
                        source.push_text(o.as_str());
                        Ty::Option(Box::new(target))
                    }
                    // A program's `deref` runs on the value inside.
                    Some((target, std_impl::DerefCall::Program(call))) if !mutable => {
                        source.push_text("if ");
                        let test = is(self);
                        source.push(test);
                        source.push_text(" { ");
                        let unwrapped = unwrap(self);
                        let dereferenced = Self::std_call(&call, unwrapped, &[]);
                        let some = self.prelude_op(PreludeOp::Some, [dereferenced]);
                        source.push(some);
                        source.push_text(" } else { ");
                        let none = self.prelude_op(PreludeOp::None, []);
                        source.push(none);
                        source.push_text(" }");
                        Ty::Option(Box::new(target))
                    }
                    Some(_) => {
                        return Err(Error::NotYet {
                            path: format!("Option::{member}").into(),
                            reference: member_span,
                        });
                    }
                    None => {
                        return Err(Error::UnknownMember {
                            owner: owner.name(&self.models),
                            member,
                            reference: member_span,
                        });
                    }
                }
            }
            "copied" | "cloned" => {
                arity(0, &arguments)?;
                let Ty::Reference { target, .. } = self.inference.resolve(&element) else {
                    return Err(Error::UnknownMember {
                        owner: owner.name(&self.models),
                        member,
                        reference: member_span,
                    });
                };
                if member.as_str() == "copied"
                    && !self.type_provides(&target, capability::Kind::Copy)
                {
                    return Err(Error::MissingCapability {
                        owner: target.name(&self.models),
                        capability: "Copy".into(),
                        operation: member_span,
                        derive: matches!(target.as_ref(), Ty::Script(..)),
                        required_by: None,
                    });
                }
                source.push_text(o.as_str());
                Ty::Option(target)
            }
            "clone" => {
                arity(0, &arguments)?;
                source.push_text(o.as_str());
                owner
            }
            "take" | "replace" => {
                arity(usize::from(member.as_str() == "replace"), &arguments)?;
                let Some(lvalue) = value.lvalue.clone() else {
                    return Err(
                        self.unsupported("`take` or `replace` on a value that is not a place")
                    );
                };
                // `Option::take(&mut self)` borrows its receiver for the call.
                self.mutable_use(&value, member_span);
                let replacement = match arguments.first_mut() {
                    Some(argument) => {
                        self.coerce_expr(&element, argument)?;
                        let argument = argument.rendered_source();
                        self.prelude_op(PreludeOp::Some, [argument])
                    }
                    None => self.prelude_op(PreludeOp::None, []),
                };
                let assignment = self.assign_place(&lvalue, replacement, value.span)?;
                source.push(assignment);
                source.push_text(format!(" {o}"));
                owner
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: owner.name(&self.models),
                    member,
                    reference: member_span,
                })
            }
        };
        source.push_text(" }");
        // A block is not an `if` condition in the legacy parser.
        let source = source.surrounded("(", ")");
        // `v.get_mut(i).unwrap()` is the element's place, which the
        // `Option` locates: it is evaluated, panicking on `None`, each time
        // the place is reached.
        let place = match &value.lvalue {
            Some(lvalue) if keeps_place && !matches!(lvalue, Lvalue::Slice(_)) => {
                let mut nested = places::as_nested(lvalue);
                let mut setup = source.clone();
                setup.push_text(";");
                nested.steps.insert(0, PlaceStep::Setup(setup));
                Some(Lvalue::Nested(Box::new(nested)))
            }
            _ => None,
        };
        if matches!(member.as_str(), "as_mut" | "as_deref_mut") {
            self.lend_place(&value, expression);
        }
        let mut parsed = self.operator_expr(source, ty, expression);
        if place.is_some() {
            parsed.lvalue = place;
        }
        Ok(parsed)
    }

    /// `b.then_some(v)` (`v` evaluated eagerly, as Rust does) and
    /// `b.then(|| v)`: `Some(v)` when `b`, else `None`.
    pub(super) fn parse_bool_method(
        &mut self,
        value: ParsedExpr,
        member: ImmutableString,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) =
            self.parse_std_arguments(&Ty::Bool, &member, "`bool` method arguments")?;
        let expression = span(value.span.start(), end);
        let [argument] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified("bool", &member).to_string(),
                1,
                arguments.len(),
                expression,
            ));
        };
        let element = self.inference.fresh();
        let produced = if member.as_str() == "then_some" {
            self.coerce_expr(&element, argument)?;
            argument.rendered_source()
        } else {
            self.callable_call(argument, &[], &element, Vec::new())?
        };
        let b = self.temporary("bool");
        let mut source = origin::Fragment::plain(format!("{{ let {b} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        let eager = member.as_str() == "then_some";
        if eager {
            source.push_text("let __rawr_some = ");
            source.push(produced.clone());
            source.push_text("; ");
        }
        source.push_text(format!("if {b} {{ "));
        let some = self.prelude_op(
            PreludeOp::Some,
            [if eager {
                "__rawr_some".into()
            } else {
                produced
            }],
        );
        source.push(some);
        source.push_text(" } else { ");
        let none = self.prelude_op(PreludeOp::None, []);
        source.push(none);
        source.push_text(" } }");
        let ty = Ty::Option(Box::new(self.inference.resolve(&element)));
        Ok(self.operator_expr(source.surrounded("(", ")"), ty, expression))
    }

    /// The `Result` methods Rust code reaches for, as Rust names them.
    /// Each composes the runtime primitives; arguments are evaluated once,
    /// after the receiver.
    pub(super) fn parse_result_method(
        &mut self,
        mut value: ParsedExpr,
        ok: Ty,
        err: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let owner = Ty::Result(Box::new(ok.clone()), Box::new(err.clone()));
        self.script_callables = true;
        // The error type of a `try_into()` whose target is known, which
        // `unwrap` and `expect` show.
        if matches!(self.inference.resolve(&err), Ty::Infer(_)) {
            self.settle_try_from_errors();
        }
        let (mut arguments, end) =
            self.parse_std_arguments(&owner, &member, "`Result` method arguments")?;
        self.builtin_enum_receiver(
            &mut value,
            Some((&ok, &err)),
            &member,
            &arguments,
            member_span,
        )?;
        let expression = span(value.span.start(), end);
        let arity = |expected: usize, arguments: &[ParsedExpr]| {
            if arguments.len() == expected {
                Ok(())
            } else {
                Err(Error::argument_count(
                    Callee::Method,
                    crate::resolve::Qualified("Result", &member).to_string(),
                    expected,
                    arguments.len(),
                    span(member_span.start(), end),
                ))
            }
        };
        // `unwrap` and `expect` show the other variant's value, so it must be
        // `Debug`, as in Rust.
        let debug = |this: &Self, ty: &Ty| {
            // A `parse` whose target is still open: each error it can have
            // is `Debug`.
            let parse_error = matches!(this.inference.resolve(ty), Ty::Infer(_))
                && this.pending_parses.iter().any(|(_, error, ..)| {
                    this.inference.resolve(error) == this.inference.resolve(ty)
                });
            if parse_error || this.type_provides(ty, capability::Kind::Debug) {
                Ok(())
            } else {
                Err(Error::MissingCapability {
                    owner: this.inference.resolve(ty).name(&this.models),
                    capability: "Debug".into(),
                    operation: member_span,
                    derive: matches!(this.inference.resolve(ty), Ty::Script(..)),
                    required_by: None,
                })
            }
        };
        let r = self.temporary("result");
        let is = |this: &mut Self| this.prelude_op(PreludeOp::IsOk, [r.as_str().into()]);
        let value_of = |this: &mut Self| this.prelude_op(PreludeOp::UnwrapOk, [r.as_str().into()]);
        let error_of = |this: &mut Self| this.prelude_op(PreludeOp::UnwrapErr, [r.as_str().into()]);
        let mut source = origin::Fragment::plain(format!("{{ let {r} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        // `if is_ok { then } else { otherwise }`.
        let branch = |source: &mut origin::Fragment,
                      test: origin::Fragment,
                      then: origin::Fragment,
                      otherwise: origin::Fragment| {
            source.push_text("if ");
            source.push(test);
            source.push_text(" { ");
            source.push(then);
            source.push_text(" } else { ");
            source.push(otherwise);
            source.push_text(" }");
        };
        let ty = match member.as_str() {
            "is_ok" | "is_err" => {
                arity(0, &arguments)?;
                if member.as_str() == "is_err" {
                    source.push_text("!");
                }
                let test = is(self);
                source.push(test);
                Ty::Bool
            }
            "is_ok_and" | "is_err_and" => {
                arity(1, &arguments)?;
                let (input, operand, negate) = if member.as_str() == "is_ok_and" {
                    (ok.clone(), value_of(self), "")
                } else {
                    (err.clone(), error_of(self), "!")
                };
                let call = self.callable_call(
                    &mut arguments[0],
                    std::slice::from_ref(&input),
                    &Ty::Bool,
                    vec![operand],
                )?;
                let test = is(self);
                source.push_text(negate);
                source.push(test);
                source.push_text(" && ");
                source.push(call);
                Ty::Bool
            }
            "ok" | "err" => {
                arity(0, &arguments)?;
                let test = is(self);
                let none = self.prelude_op(PreludeOp::None, []);
                if member.as_str() == "ok" {
                    let inner = value_of(self);
                    let some = self.prelude_op(PreludeOp::Some, [inner]);
                    branch(&mut source, test, some, none);
                    Ty::Option(Box::new(ok))
                } else {
                    let inner = error_of(self);
                    let some = self.prelude_op(PreludeOp::Some, [inner]);
                    branch(&mut source, test, none, some);
                    Ty::Option(Box::new(err))
                }
            }
            "unwrap" | "unwrap_err" => {
                arity(0, &arguments)?;
                if member.as_str() == "unwrap" {
                    debug(self, &err)?;
                    let unwrapped = value_of(self);
                    source.push(unwrapped);
                    ok
                } else {
                    debug(self, &ok)?;
                    let unwrapped = error_of(self);
                    source.push(unwrapped);
                    err
                }
            }
            "expect" | "expect_err" => {
                arity(1, &arguments)?;
                self.coerce_expr(&Ty::Str, &mut arguments[0])?;
                let (op, other, ty) = if member.as_str() == "expect" {
                    (PreludeOp::ExpectOk, &err, ok.clone())
                } else {
                    (PreludeOp::ExpectErr, &ok, err.clone())
                };
                debug(self, other)?;
                let expected =
                    self.prelude_op(op, [r.as_str().into(), arguments[0].rendered_source()]);
                source.push(expected);
                ty
            }
            "unwrap_or" => {
                arity(1, &arguments)?;
                self.coerce_expr(&ok, &mut arguments[0])?;
                source.push_text("let __rawr_or = ");
                source.push(arguments[0].rendered_source());
                source.push_text("; ");
                let test = is(self);
                let unwrapped = value_of(self);
                branch(&mut source, test, unwrapped, "__rawr_or".into());
                ok
            }
            "unwrap_or_default" => {
                arity(0, &arguments)?;
                let default = self.default_source(&ok, member_span)?;
                let test = is(self);
                let unwrapped = value_of(self);
                branch(&mut source, test, unwrapped, default);
                ok
            }
            "unwrap_or_else" => {
                arity(1, &arguments)?;
                let error = error_of(self);
                let call = self.callable_call(
                    &mut arguments[0],
                    std::slice::from_ref(&err),
                    &ok,
                    vec![error],
                )?;
                let test = is(self);
                let unwrapped = value_of(self);
                branch(&mut source, test, unwrapped, call);
                ok
            }
            // `Err` passes through `map` and `and_then` unchanged, and `Ok`
            // through `map_err` and `or_else`: at runtime it is the receiver.
            "map" | "and_then" => {
                arity(1, &arguments)?;
                let output = self.inference.fresh();
                let expected = if member.as_str() == "map" {
                    output.clone()
                } else {
                    Ty::Result(Box::new(output.clone()), Box::new(err.clone()))
                };
                let operand = value_of(self);
                let call = self.callable_call(
                    &mut arguments[0],
                    std::slice::from_ref(&ok),
                    &expected,
                    vec![operand],
                )?;
                let produced = if member.as_str() == "map" {
                    self.prelude_op(PreludeOp::Ok, [call])
                } else {
                    call
                };
                let test = is(self);
                branch(&mut source, test, produced, r.as_str().into());
                Ty::Result(Box::new(self.inference.resolve(&output)), Box::new(err))
            }
            "map_err" | "or_else" => {
                arity(1, &arguments)?;
                let output = self.inference.fresh();
                let expected = if member.as_str() == "map_err" {
                    output.clone()
                } else {
                    Ty::Result(Box::new(ok.clone()), Box::new(output.clone()))
                };
                let operand = error_of(self);
                let call = self.callable_call(
                    &mut arguments[0],
                    std::slice::from_ref(&err),
                    &expected,
                    vec![operand],
                )?;
                let produced = if member.as_str() == "map_err" {
                    self.prelude_op(PreludeOp::Err, [call])
                } else {
                    call
                };
                let test = is(self);
                branch(&mut source, test, r.as_str().into(), produced);
                Ty::Result(Box::new(ok), Box::new(self.inference.resolve(&output)))
            }
            "iter" | "into_iter" => {
                arity(0, &arguments)?;
                let item = self.held_item(&value.ty, &member, ok);
                let parts = [is(self), value_of(self)];
                return Ok(self.held_iterator(source, parts, item, expression));
            }
            "as_ref" | "as_mut" => {
                arity(0, &arguments)?;
                source.push_text(r.as_str());
                let mutable = member.as_str() == "as_mut";
                Ty::Result(
                    Box::new(Ty::Reference {
                        mutable,
                        target: Box::new(ok),
                    }),
                    Box::new(Ty::Reference {
                        mutable,
                        target: Box::new(err),
                    }),
                )
            }
            // `as_deref` borrows the `Ok` value through `Deref`, as
            // `Option`'s does: `Result<String, E>` gives `Result<&str, &E>`.
            "as_deref" | "as_deref_mut" => {
                arity(0, &arguments)?;
                let mutable = member.as_str() == "as_deref_mut";
                let reference = Ty::Reference {
                    mutable,
                    target: Box::new(self.inference.resolve(&ok)),
                };
                let err = Box::new(Ty::Reference {
                    mutable,
                    target: Box::new(err),
                });
                match self.deref_reference(&reference, false) {
                    Some((target, std_impl::DerefCall::Value)) => {
                        source.push_text(r.as_str());
                        Ty::Result(Box::new(target), err)
                    }
                    // A program's `deref` runs on the `Ok` value.
                    Some((target, std_impl::DerefCall::Program(call))) if !mutable => {
                        let test = is(self);
                        let unwrapped = value_of(self);
                        let dereferenced = Self::std_call(&call, unwrapped, &[]);
                        let produced = self.prelude_op(PreludeOp::Ok, [dereferenced]);
                        branch(&mut source, test, produced, r.as_str().into());
                        Ty::Result(Box::new(target), err)
                    }
                    Some(_) => {
                        return Err(Error::NotYet {
                            path: format!("Result::{member}").into(),
                            reference: member_span,
                        });
                    }
                    None => {
                        return Err(Error::UnknownMember {
                            owner: owner.name(&self.models),
                            member,
                            reference: member_span,
                        });
                    }
                }
            }
            "clone" => {
                arity(0, &arguments)?;
                source.push_text(r.as_str());
                owner
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: owner.name(&self.models),
                    member,
                    reference: member_span,
                })
            }
        };
        source.push_text(" }");
        if matches!(member.as_str(), "as_mut" | "as_deref_mut") {
            self.lend_place(&value, expression);
        }
        // A block is not an `if` condition in the legacy parser.
        Ok(self.operator_expr(source.surrounded("(", ")"), ty, expression))
    }

    /// `o.as_mut()` of a place `o`: the value at `expression` lends `&mut`
    /// to the place's parts, which a pattern binds as `Some(x)` of
    /// `&mut o` does (`pattern_place`).
    pub(super) fn lend_place(&mut self, value: &ParsedExpr, expression: Span) {
        if let Some(lvalue) = &value.lvalue
            && !matches!(lvalue, Lvalue::Slice(_))
        {
            self.lent_places.insert(expression, lvalue.clone());
        }
    }

    /// A call of the closure or `fn` argument `callable` with `operands`,
    /// checked as `fn(inputs) -> output`.
    pub(super) fn callable_call(
        &mut self,
        callable: &mut ParsedExpr,
        inputs: &[Ty],
        output: &Ty,
        operands: Vec<origin::Fragment>,
    ) -> Result<origin::Fragment, Error> {
        let expected = Ty::Fn {
            inputs: inputs.into(),
            output: Box::new(output.clone()),
        };
        self.coerce_expr(&expected, callable)?;
        let mut call = callable.rendered_source().surrounded("(", ")");
        call.push_text(".__rawr_call(");
        call.push(origin::Fragment::join(operands, ", "));
        call.push_text(")");
        Ok(call)
    }

    /// The value of `T::default()`, for `unwrap_or_default`.
    pub(super) fn default_source(
        &mut self,
        ty: &Ty,
        span: Span,
    ) -> Result<origin::Fragment, Error> {
        let ty = self.inference.resolve(ty);
        Ok(match &ty {
            Ty::Unit => "()".into(),
            Ty::Bool => "false".into(),
            Ty::Char => "'\\0'".into(),
            Ty::Int(_) => "0".into(),
            Ty::Float(_) => "0.0".into(),
            Ty::String | Ty::Str => "\"\"".into(),
            Ty::Array {
                kind: sequence::Kind::Vec | sequence::Kind::Deque,
                ..
            }
            | Ty::Keyed(..) => "[]".into(),
            Ty::Array {
                element,
                kind: sequence::Kind::Array(length),
            } => {
                let element = self.default_source(element, span)?;
                origin::Fragment::join(vec![element; *length], ", ").surrounded("[", "]")
            }
            Ty::Option(_) => self.prelude_op(PreludeOp::None, []),
            Ty::Time(Time::Duration) => self.prelude_op(
                PreludeOp::Time,
                [
                    "()".into(),
                    time_code("ZERO").to_string().into(),
                    "()".into(),
                ],
            ),
            Ty::Boxed(target) => self.default_source(target, span)?,
            // `Rc::default()`: a new allocation of the value's default, and
            // `Weak::new()`.
            Ty::Counted(_, target) => {
                let value = self.default_source(target, span)?;
                self.prelude_op(PreludeOp::CountedNew, [value])
            }
            Ty::Weak(..) => self.prelude_op(PreludeOp::WeakNew, []),
            Ty::Reverse(inner) => {
                let value = self.default_source(inner, span)?;
                self.prelude_op(PreludeOp::ReverseWrap, [value])
            }
            Ty::Cell(kind, inner) => {
                let value = self.default_source(inner, span)?;
                self.prelude_op(kind.new_op(), [value])
            }
            Ty::Tuple(members) => {
                let members = members
                    .iter()
                    .map(|member| self.default_source(member, span))
                    .collect::<Result<Vec<_>, _>>()?;
                origin::Fragment::join(members, ", ").surrounded("[", "]")
            }
            Ty::Script(id, ..) => {
                let model = self.models.definition(*id).clone();
                self.require_capability(&model, capability::Kind::Default, span)?;
                // A hand-written `impl Default` runs.
                if let Some(definition) = self.models.std_impl(*id, "Default", "default") {
                    return Ok(origin::Fragment::call(method_name(definition.id()), []));
                }
                // A derived `Default` with a field whose `Default` runs code
                // is the struct of each field's `Default`, built each time.
                if let Nominal::Struct { definition, item } = &model
                    && item.generics.is_empty()
                    && !self.models.default_folds(&ty, &mut Vec::new())
                {
                    let fields = item
                        .fields
                        .iter()
                        .map(|field| self.default_source(&Ty::from_resolved(&field.ty), span))
                        .collect::<Result<Vec<_>, _>>()?;
                    let helper = self.push_op(Op::Struct {
                        definition: definition.clone(),
                        item: item.clone(),
                    });
                    return Ok(origin::Fragment::call(&helper, fields));
                }
                let value =
                    default_value(&model, &self.models, &mut Vec::new()).ok_or_else(|| {
                        Error::MissingCapability {
                            owner: model.name().into(),
                            capability: "Default".into(),
                            operation: span,
                            derive: true,
                            required_by: None,
                        }
                    })?;
                let helper = self.push_op(Op::Default {
                    value,
                    definition: model.definition().clone(),
                });
                origin::Fragment::call(&helper, [])
            }
            // A library type's own `Default`: `T::default()`, which for an
            // imported Rust type calls `<T as Default>::default()`.
            // A generic one's is the impl's at the written arguments.
            Ty::Host { proof, arguments }
                if proof
                    .definition
                    .capabilities()
                    .contains(capability::Kind::Default) =>
            {
                let owner = proof.definition.path().to_string();
                // A type only a crate's alias names, as iced's `canvas::Cache`
                // names `geometry::Cache`, has its `default` under the alias.
                #[cfg(feature = "rust-import")]
                let owner = match proof
                    .definition
                    .identity()
                    .imported()
                    .and_then(|identity| self.models.host.passing_alias(identity))
                {
                    Some(alias)
                        if self
                            .models
                            .host_items(&format!("{owner}::default"))
                            .is_none() =>
                    {
                        alias.path().to_string()
                    }
                    _ => owner,
                };
                let path = format!("{owner}::default");
                let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec) else {
                    if proof.definition.native_identity().is_some() {
                        return Err(Error::MissingCapability {
                            owner: ty.name(&self.models),
                            capability: "Default".into(),
                            operation: span,
                            derive: matches!(ty, Ty::Script(..)),
                            required_by: None,
                        });
                    }
                    // An imported type's `default` comes from its crate's
                    // `impl Default`, demanded as `Type::default`.
                    return Err(Error::UnknownMember {
                        owner: owner.into(),
                        member: "default".into(),
                        reference: span,
                    });
                };
                let item_owner = self.models.item_owner(&path).cloned();
                let value = self.call_host_function(&items, item_owner, Vec::new(), span)?;
                if !arguments.is_empty() {
                    self.require_type(&ty, &value.ty, span)?;
                }
                value.rendered_source()
            }
            _ => {
                return Err(Error::MissingCapability {
                    owner: ty.name(&self.models),
                    capability: "Default".into(),
                    operation: span,
                    derive: matches!(ty, Ty::Script(..)),
                    required_by: None,
                })
            }
        })
    }
}
