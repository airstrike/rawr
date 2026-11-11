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
        // `o.as_mut().unwrap()`'s place: the payload of `o`'s.
        let mut payload_place = None;
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
                if member.as_str() == "as_mut" && self.unwrap_ahead() {
                    payload_place = self.option_payload_place(&value, &element);
                }
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
            _ => payload_place,
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

    /// Whether `.unwrap()` or `.expect(..)` comes next.
    fn unwrap_ahead(&self) -> bool {
        let mut ahead = self.input.clone();
        let mut next = || ahead.next().map_or(Token::EOF, |(token, _)| token);
        next() == Token::Period
            && matches!(next(), Token::Identifier(name) if matches!(name.as_str(), "unwrap" | "expect"))
            && matches!(next(), Token::Unit | Token::LeftParen)
    }

    /// The place of the payload of `value`, an `Option` of `element` that
    /// is a place: read as its `Some`'s value, stored as `Some` of the
    /// value stored, as `*o.as_mut().unwrap() += 1` changes it (#1001).
    fn option_payload_place(&mut self, value: &ParsedExpr, element: &Ty) -> Option<Lvalue> {
        let lvalue = value.lvalue.as_ref()?;
        let place = self.temporary("place");
        let stored = self.temporary("stored");
        let read = self.prelude_op(PreludeOp::Unwrap, [origin::Fragment::plain(place.clone())]);
        let store = self.prelude_op(PreludeOp::Some, [origin::Fragment::plain(stored.clone())]);
        let step = PlaceStep::Accessor {
            place,
            stored,
            read,
            store,
        };
        Some(match lvalue {
            Lvalue::Binding { binding, .. } => Lvalue::Nested(Box::new(lvalue::Nested {
                binding: binding.clone(),
                base: origin::Fragment::plain(binding.to_string()),
                steps: vec![step],
                ty: element.clone(),
            })),
            Lvalue::Slice(_) => return None,
            lvalue => lvalue::nested_step(lvalue, step, element.clone()),
        })
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
                if let Some(definition) = self.models.std_impl(*id, "Default", "default").cloned() {
                    // An impl compiled per instantiation, as native code's
                    // mention of `Content<grid::Message>` makes `impl<'a,
                    // Message> Default for Content<'a, Message>` (D869), runs
                    // its instance at the type's arguments (D629).
                    #[cfg(feature = "rust-import")]
                    let definition = {
                        let substitution =
                            self.method_substitution(&definition, Some(&ty), span)?;
                        self.bind_method_instance(definition, &substitution, span)?
                    };
                    return Ok(origin::Fragment::call(method_name(&definition), []));
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
                        item: Box::new(item.clone()),
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

/// rustc's size in bytes of a value of `ty`, for a type whose layout Rust
/// fixes: a number, `bool`, `char`, `()` or an array of them.
pub(super) fn primitive_size(ty: &Ty) -> Option<u64> {
    Some(match ty {
        Ty::Unit => 0,
        Ty::Bool => 1,
        Ty::Char => 4,
        // An integer literal's type defaults to `i32`.
        Ty::Int(Int::Var(_)) => 4,
        Ty::Int(kind) => u64::from(kind.bits() / 8),
        Ty::Float(Float::F32) => 4,
        Ty::Float(_) => 8,
        Ty::Array {
            element,
            kind: sequence::Kind::Array(length),
        } => primitive_size(element)?.checked_mul(u64::try_from(*length).ok()?)?,
        _ => return None,
    })
}

/// The slice methods `SliceChunks` runs, by code: each gives the slices.
pub(super) const SLICE_CHUNKS: &[&str] = &[
    "chunks",
    "windows",
    "chunks_exact",
    "rchunks",
    "rchunks_exact",
];

/// The `Vec` methods `VecMethod` runs in place, by code.
pub(super) const VEC_METHODS: &[&str] = &[
    "insert",
    "remove",
    "swap",
    "dedup",
    "drain",
    "pop",
    "store",
    "fill",
    "copy_from_slice",
    "split_off",
    "swap_remove",
    "pop_front",
    "with_capacity",
    "push",
    "clear",
    "truncate",
    "reverse",
    "append",
    "resize",
    "resize_with",
    "retain",
    "sort_by",
    "sort_unstable_by",
    "sort",
    "sort_unstable",
    "extend_move",
    "extend_copy",
    "extend_clone",
    "deque_remove",
    "swap_remove_back",
    "swap_remove_front",
    "rotate_left",
    "rotate_right",
];

/// The `VecDeque` methods rawr runs, each as the `Vec` method of an array
/// whose back is its end, or as an operation of the deque's own (`remove`'s
/// `Option`, `range`, `as_slices`). std's others are not run yet (D712.1).
pub(super) const DEQUE_METHODS: &[(&str, &str)] = &[
    ("push_back", "push"),
    ("pop_back", "pop"),
    ("push_front", "push_front"),
    ("pop_front", "pop_front"),
    ("front", "first"),
    ("back", "last"),
    ("front_mut", "first_mut"),
    ("back_mut", "last_mut"),
    ("len", "len"),
    ("is_empty", "is_empty"),
    ("iter", "iter"),
    ("iter_mut", "iter_mut"),
    ("into_iter", "into_iter"),
    ("get", "get"),
    ("get_mut", "get_mut"),
    ("contains", "contains"),
    ("insert", "insert"),
    ("swap", "swap"),
    ("clear", "clear"),
    ("truncate", "truncate"),
    ("retain", "retain"),
    ("extend", "extend"),
    ("append", "append"),
    ("split_off", "split_off"),
    ("drain", "drain"),
    ("resize", "resize"),
    ("resize_with", "resize_with"),
    ("clone", "clone"),
    ("remove", "deque_remove"),
    ("swap_remove_back", "swap_remove_back"),
    ("swap_remove_front", "swap_remove_front"),
    ("rotate_left", "rotate_left"),
    ("rotate_right", "rotate_right"),
    ("range", "range"),
    ("as_slices", "as_slices"),
    ("make_contiguous", "as_mut_slice"),
];

/// A `VEC_METHODS` code by name.
pub(super) fn vec_method_code(name: &str) -> usize {
    VEC_METHODS
        .iter()
        .position(|method| *method == name)
        .expect("a listed `Vec` method")
}

/// `insert(i, x)`, `remove(i)`, `swap_remove(i)`, `swap(a, b)`, `dedup()` and
/// `drain(low, high, inclusive)` (`high` `-1` for an open end) on the array
/// taken by reference, with Rust's panics.
pub(super) fn vec_method(
    context: Option<&NativeCallContext<'_>>,
    args: &mut FnCallArgs,
) -> RawrResult {
    let fail = |message: String| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let code = args[1].as_int().unwrap_or(-1);
    let at = |value: &Dynamic| usize::try_from(value.as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    if let Some(place) = crate::place::get(args[0]) {
        use crate::place::Operation;
        let name = VEC_METHODS
            .get(usize::try_from(code).unwrap_or(usize::MAX))
            .copied();
        let (operation, indexes): (Operation, &[usize]) = match name {
            Some("insert") => (Operation::Insert, &[2, 3]),
            Some("remove") => (Operation::Remove, &[2]),
            Some("swap_remove") => (Operation::SwapRemove, &[2]),
            Some("swap") => (Operation::Swap, &[2, 3]),
            Some("pop") => (Operation::Pop, &[]),
            Some("fill") => (Operation::Fill, &[3]),
            Some("dedup") => (Operation::Dedup, &[]),
            Some("drain") => (Operation::Drain, &[2, 3, 4]),
            Some("copy_from_slice") => (Operation::CopyFromSlice, &[3]),
            Some("split_off") => (Operation::SplitOff, &[2]),
            Some("push") => (Operation::Push, &[2]),
            Some("clear") => (Operation::Clear, &[]),
            Some("truncate") => (Operation::Truncate, &[2]),
            Some("reverse") => (Operation::Reverse, &[]),
            Some("append") => (Operation::Append, &[2]),
            Some("extend_move") => (Operation::Extend(crate::place::ExtendMode::Move), &[2]),
            Some("extend_copy") => (Operation::Extend(crate::place::ExtendMode::Copy), &[2]),
            Some("extend_clone") => (Operation::Extend(crate::place::ExtendMode::Clone), &[2]),
            Some("resize") => (Operation::Resize, &[2, 3]),
            Some("resize_with") => (Operation::ResizeWith, &[2, 3]),
            Some("retain") => (Operation::Retain, &[2]),
            Some("sort") => (Operation::Sort { stable: true }, &[]),
            Some("sort_unstable") => (Operation::Sort { stable: false }, &[]),
            Some("sort_by") => (Operation::SortBy { stable: true }, &[2]),
            Some("sort_unstable_by") => (Operation::SortBy { stable: false }, &[2]),
            _ => return Err(invariant("native sequence operation has no typed dispatch")),
        };
        let mut operands = indexes
            .iter()
            .map(|index| args[*index].take())
            .collect::<Vec<_>>();
        return match context {
            Some(context) => place.invoke_in(context, operation, &mut operands),
            None => place.invoke(operation, &mut operands),
        };
    }
    let (a, b) = (at(args[2]), at(args[3]));
    // The operands move in, as Rust's `push(x)` and `insert(i, x)` do.
    let first = args[2].take();
    let value = args[3].take();
    let inclusive = args[4].as_bool().unwrap_or(false);
    let mut array = args[0]
        .as_array_mut()
        .map_err(|_| invariant("a `Vec` is not an array at runtime"))?;
    let len = array.len();
    match VEC_METHODS
        .get(usize::try_from(code).unwrap_or(usize::MAX))
        .copied()
    {
        Some("extend_move" | "extend_copy" | "extend_clone") => {
            array.extend(
                first
                    .into_array()
                    .map_err(|_| invariant("an extend source is not an array"))?,
            );
            Ok(Dynamic::UNIT)
        }
        Some("push") => {
            array.push(first);
            Ok(Dynamic::UNIT)
        }
        Some("clear") => {
            array.clear();
            Ok(Dynamic::UNIT)
        }
        Some("truncate") => {
            array.truncate(a);
            Ok(Dynamic::UNIT)
        }
        Some("reverse") => {
            array.reverse();
            Ok(Dynamic::UNIT)
        }
        Some("append") => {
            let mut incoming = first
                .into_array()
                .map_err(|_| invariant("an append operand is not an array"))?;
            array.append(&mut incoming);
            Ok(Dynamic::UNIT)
        }
        Some("insert") if a > len => fail(format!(
            "insertion index (is {a}) should be <= len (is {len})"
        )),
        Some("insert") => {
            array.insert(a, value);
            Ok(Dynamic::UNIT)
        }
        Some("remove") if a >= len => {
            fail(format!("removal index (is {a}) should be < len (is {len})"))
        }
        Some("remove") => Ok(array.remove(a)),
        Some("swap_remove") if a >= len => fail(format!(
            "swap_remove index (is {a}) should be < len (is {len})"
        )),
        Some("swap_remove") => Ok(array.swap_remove(a)),
        // A new `Vec` with room for `a` items, as `VecCapacity` reads it
        // back; it grows by doubling from 4, as Rust's does.
        Some("with_capacity") => Ok(Dynamic::from_array(crate::Array::with_capacity(a))),
        Some("swap") => match [a, b].into_iter().find(|index| *index >= len) {
            Some(index) => fail(format!(
                "index out of bounds: the len is {len} but the index is {index}"
            )),
            None => {
                array.swap(a, b);
                Ok(Dynamic::UNIT)
            }
        },
        Some("dedup") => {
            array.dedup_by(|later, earlier| equal_value(earlier, later));
            Ok(Dynamic::UNIT)
        }
        Some("pop") => Ok(match array.pop() {
            Some(last) => crate::option::some(last),
            None => crate::option::none(),
        }),
        // A deque's removals, `None` past its end.
        Some("deque_remove" | "swap_remove_back" | "swap_remove_front") if a >= len => {
            Ok(crate::option::none())
        }
        Some("deque_remove") => Ok(crate::option::some(array.remove(a))),
        Some("swap_remove_back") => Ok(crate::option::some(array.swap_remove(a))),
        Some("swap_remove_front") => {
            array.swap(0, a);
            Ok(crate::option::some(array.remove(0)))
        }
        Some("rotate_left" | "rotate_right") if a > len => {
            fail("assertion failed: n <= self.len()".into())
        }
        Some("rotate_left") => {
            array.rotate_left(a);
            Ok(Dynamic::UNIT)
        }
        Some("rotate_right") => {
            array.rotate_right(a);
            Ok(Dynamic::UNIT)
        }
        Some("pop_front") if array.is_empty() => Ok(crate::option::none()),
        Some("pop_front") => Ok(crate::option::some(array.remove(0))),
        // A changed copy of `array[a..]` stored back over it.
        Some("store") => {
            let copy = value.into_array().unwrap_or_default();
            for (slot, element) in array.iter_mut().skip(a).zip(copy) {
                *slot = element;
            }
            Ok(Dynamic::UNIT)
        }
        Some("fill") => {
            array.fill(value);
            Ok(Dynamic::UNIT)
        }
        Some("split_off") if a > len => fail(format!(
            "`at` split index (is {a}) should be <= len (is {len})"
        )),
        Some("split_off") => Ok(Dynamic::from_array(array.split_off(a))),
        Some("copy_from_slice") => {
            let source = value.into_array().unwrap_or_default();
            if source.len() != len {
                return fail(format!(
                    "source slice length ({}) does not match destination slice length ({len})",
                    source.len()
                ));
            }
            *array = source;
            Ok(Dynamic::UNIT)
        }
        Some("drain") => {
            let high = (b != usize::MAX).then_some(b);
            match drain_window(a, high, inclusive, len) {
                Ok((low, high)) => Ok(Dynamic::from_array(array.drain(low..high).collect())),
                Err(message) => fail(message),
            }
        }
        _ => Err(invariant("an unknown `Vec` method")),
    }
}

/// The `String` methods `StrChange` runs in place, by code.
pub(super) const STR_CHANGES: &[&str] = &[
    "pop",
    "clear",
    "insert",
    "insert_str",
    "remove",
    "truncate",
    "split_off",
    "push",
    "push_str",
    "retain",
];

/// `STR_CHANGES`' `code` on the string taken by reference, by bytes and
/// with Rust's panics.
pub(super) fn str_change(
    context: Option<&NativeCallContext<'_>>,
    args: &mut FnCallArgs,
) -> RawrResult {
    let fail = |message: String| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let code = usize::try_from(args[1].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    let at = usize::try_from(args[2].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    let operand = args[3].clone();
    if let Some(place) = crate::place::get(args[0]) {
        use crate::place::Operation;
        let (operation, indexes): (Operation, &[usize]) = match STR_CHANGES.get(code).copied() {
            Some("pop") => (Operation::Pop, &[]),
            Some("clear") => (Operation::Clear, &[]),
            Some("truncate") => (Operation::Truncate, &[2]),
            Some("push") => (Operation::Push, &[3]),
            Some("push_str") => (Operation::PushStr, &[3]),
            Some("insert") => (Operation::Insert, &[2, 3]),
            Some("insert_str") => (Operation::InsertStr, &[2, 3]),
            Some("remove") => (Operation::Remove, &[2]),
            Some("split_off") => (Operation::SplitOff, &[2]),
            Some("retain") => (Operation::Retain, &[3]),
            _ => return Err(invariant("native String operation has no typed dispatch")),
        };
        let mut operands = indexes
            .iter()
            .map(|index| args[*index].take())
            .collect::<Vec<_>>();
        return match context {
            Some(context) => place.invoke_in(context, operation, &mut operands),
            None => place.invoke(operation, &mut operands),
        };
    }
    let mut text = args[0]
        .write_lock::<ImmutableString>()
        .ok_or_else(|| invariant("a `String` is not a string at runtime"))?;
    let name = STR_CHANGES.get(code).copied().unwrap_or_default();
    // Rust asserts a char boundary before changing the string there.
    let boundary = |text: &str, at: usize, what: &str| -> Result<(), Box<EvalAltResult>> {
        if text.is_char_boundary(at) {
            Ok(())
        } else {
            Err(EvalAltResult::ErrorRuntime(
                format!("assertion failed: self.is_char_boundary({what})").into(),
                Position::NONE,
            )
            .into())
        }
    };
    match name {
        "retain" => crate::place::text::retain_with(
            context.ok_or_else(|| invariant("String::retain requires its script call context"))?,
            operand,
            |predicate| text.make_mut().retain(predicate),
        ),
        "pop" => Ok(match text.make_mut().pop() {
            Some(c) => crate::option::some(c.into()),
            None => crate::option::none(),
        }),
        "clear" => {
            *text = ImmutableString::new();
            Ok(Dynamic::UNIT)
        }
        "insert" | "insert_str" => {
            boundary(&text, at, "idx")?;
            match operand.as_char() {
                Ok(c) => text.make_mut().insert(at, c),
                Err(_) => {
                    let piece = operand.clone_cast::<ImmutableString>();
                    text.make_mut().insert_str(at, &piece);
                }
            }
            Ok(Dynamic::UNIT)
        }
        "remove" => {
            let len = text.len();
            if at > len {
                return fail(format!(
                    "start byte index {at} is out of bounds for string of length {len}"
                ));
            }
            if !text.is_char_boundary(at) {
                let start = (0..at)
                    .rev()
                    .find(|index| text.is_char_boundary(*index))
                    .unwrap_or(0);
                let inside = text[start..].chars().next().unwrap_or_default();
                let end = start + inside.len_utf8();
                return fail(format!(
                    "start byte index {at} is not a char boundary; it is inside {inside:?} \
                     (bytes {start}..{end} of string)"
                ));
            }
            match text[at..].chars().next() {
                Some(c) => {
                    text.make_mut().remove(at);
                    Ok(c.into())
                }
                None => fail("cannot remove a char from the end of a string".into()),
            }
        }
        "truncate" => {
            if at <= text.len() {
                boundary(&text, at, "new_len")?;
                text.make_mut().truncate(at);
            }
            Ok(Dynamic::UNIT)
        }
        "split_off" => {
            boundary(&text, at, "at")?;
            let rest = text.make_mut().split_off(at);
            Ok(Dynamic::from(ImmutableString::from(rest.as_str())))
        }
        "push" | "push_str" => {
            match operand.as_char() {
                Ok(c) => text.make_mut().push(c),
                Err(_) => {
                    let piece = operand.clone_cast::<ImmutableString>();
                    text.make_mut().push_str(&piece);
                }
            }
            Ok(Dynamic::UNIT)
        }
        _ => Err(invariant("an unknown `String` method")),
    }
}

/// The string methods `StrPieces` runs, by code: each gives an array.
/// `from_utf8` gives a `Result` of the text or its error's `Debug` text,
/// `utf8_display` that error's message from its `Debug` text, and `debug`
/// a string's or a char's `Debug` text.
pub(super) const STR_PIECES: &[&str] = &[
    "bytes",
    "char_indices",
    "splitn",
    "from_utf8",
    "debug",
    "utf8_display",
    "rsplitn",
];

pub(super) fn str_pieces(args: &mut FnCallArgs) -> RawrResult {
    let code = usize::try_from(args[1].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    let name = STR_PIECES.get(code).copied().unwrap_or_default();
    if name == "from_utf8" {
        let bytes: Vec<u8> = args[0]
            .as_array_ref()
            .map_err(|_| invariant("a `Vec<u8>` is not an array at runtime"))?
            .iter()
            .map(|byte| u8::try_from(byte.as_int().unwrap_or_default()).unwrap_or_default())
            .collect();
        return Ok(match String::from_utf8(bytes) {
            Ok(text) => crate::result::ok(Dynamic::from(ImmutableString::from(text))),
            Err(error) => {
                crate::result::err(Dynamic::from(ImmutableString::from(format!("{error:?}"))))
            }
        });
    }
    // `FromUtf8Error { bytes: [..], error: Utf8Error { valid_up_to: 0,
    // error_len: Some(1) } }`: std's `Display` of that `Utf8Error`.
    if name == "utf8_display" {
        let debug = args[0].clone_cast::<ImmutableString>();
        let number = |key: &str| {
            let rest = debug.rsplit_once(key)?.1;
            let digits = rest.split(|c: char| !c.is_ascii_digit()).next()?;
            digits.parse::<usize>().ok()
        };
        let valid_up_to = number("valid_up_to: ").unwrap_or_default();
        let text = match number("error_len: Some(") {
            Some(length) => {
                format!("invalid utf-8 sequence of {length} bytes from index {valid_up_to}")
            }
            None => format!("incomplete utf-8 byte sequence from index {valid_up_to}"),
        };
        return Ok(Dynamic::from(ImmutableString::from(text)));
    }
    if name == "debug" {
        if let Some(place) = crate::place::get(args[0]) {
            return place.invoke(crate::place::Operation::Debug, &mut []);
        }
        let text = match args[0].as_char() {
            Ok(c) => format!("{c:?}"),
            Err(_) => format!("{:?}", args[0].clone_cast::<ImmutableString>().as_str()),
        };
        return Ok(Dynamic::from(ImmutableString::from(text)));
    }
    let text = args[0].clone_cast::<ImmutableString>();
    let string = |piece: &str| Dynamic::from(ImmutableString::from(piece));
    let pieces: crate::Array = match name {
        "bytes" => text
            .bytes()
            .map(|byte| Dynamic::from(crate::INT::from(byte)))
            .collect(),
        "char_indices" => text
            .char_indices()
            .map(|(index, c)| Dynamic::from_array(vec![(index as crate::INT).into(), c.into()]))
            .collect(),
        "splitn" => {
            let count = usize::try_from(args[2].as_int().unwrap_or_default()).unwrap_or(0);
            match args[3].as_char() {
                Ok(c) => text.splitn(count, c).map(string).collect(),
                Err(_) => {
                    let pattern = args[3].clone_cast::<ImmutableString>();
                    text.splitn(count, pattern.as_str()).map(string).collect()
                }
            }
        }
        "rsplitn" => {
            let count = usize::try_from(args[2].as_int().unwrap_or_default()).unwrap_or(0);
            match args[3].as_char() {
                Ok(c) => text.rsplitn(count, c).map(string).collect(),
                Err(_) => {
                    let pattern = args[3].clone_cast::<ImmutableString>();
                    text.rsplitn(count, pattern.as_str()).map(string).collect()
                }
            }
        }
        _ => return Err(invariant("an unknown string method")),
    };
    Ok(Dynamic::from_array(pieces))
}

/// `char`'s methods by name and result (`c` a `char`, `b` a `bool`, `u` a
/// `usize`, `o` an `Option<u32>`): the code `CharMethod` runs.
/// The `str` methods `StrMethod` runs, by code, each with its arguments'
/// count and its output: `s` a `String`, `b` a `bool`, `f` an
/// `Option<usize>`, `o` an `Option<&str>`, `p` an `Option<(&str, &str)>`.
pub(super) const STR_METHODS: &[(&str, usize, char)] = &[
    ("trim", 0, 's'),
    ("trim_start", 0, 's'),
    ("trim_end", 0, 's'),
    ("to_ascii_uppercase", 0, 's'),
    ("to_ascii_lowercase", 0, 's'),
    ("replace", 2, 's'),
    ("repeat", 1, 's'),
    ("find", 1, 'f'),
    ("rfind", 1, 'f'),
    ("strip_prefix", 1, 'o'),
    ("strip_suffix", 1, 'o'),
    ("split_once", 1, 'p'),
    ("rsplit_once", 1, 'p'),
    ("eq_ignore_ascii_case", 1, 'b'),
    ("is_char_boundary", 1, 'b'),
    ("trim_matches", 1, 's'),
    ("trim_start_matches", 1, 's'),
    ("trim_end_matches", 1, 's'),
];

/// `STR_METHODS`' `code` on `text`, with Rust's results; a pattern is a
/// `char` or a string.
pub(super) fn str_method(
    text: &str,
    code: usize,
    a: &Dynamic,
    b: &Dynamic,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let string = |value: &str| Dynamic::from(ImmutableString::from(value));
    let pattern = |value: &Dynamic| match value.as_char() {
        Ok(c) => c.to_string(),
        Err(_) => value.clone_cast::<ImmutableString>().to_string(),
    };
    let option = |value: Option<Dynamic>| match value {
        Some(value) => crate::option::some(value),
        None => crate::option::none(),
    };
    let name = STR_METHODS.get(code).map_or("", |(name, ..)| *name);
    Ok(match name {
        "trim" => string(text.trim()),
        "trim_start" => string(text.trim_start()),
        "trim_end" => string(text.trim_end()),
        "to_ascii_uppercase" => string(&text.to_ascii_uppercase()),
        "to_ascii_lowercase" => string(&text.to_ascii_lowercase()),
        "replace" => string(&text.replace(&pattern(a), &pattern(b))),
        "repeat" => string(&text.repeat(usize::try_from(a.as_int().unwrap_or(0)).unwrap_or(0))),
        "find" => option(text.find(&pattern(a)).map(|at| Dynamic::from(at as INT))),
        "rfind" => option(text.rfind(&pattern(a)).map(|at| Dynamic::from(at as INT))),
        "strip_prefix" => option(text.strip_prefix(&pattern(a)).map(string)),
        "strip_suffix" => option(text.strip_suffix(&pattern(a)).map(string)),
        // rustc takes only a `char` here; a string trims as both ends do.
        "trim_matches" => match a.as_char() {
            Ok(c) => string(text.trim_matches(c)),
            Err(_) => {
                let pattern = pattern(a);
                string(
                    text.trim_start_matches(pattern.as_str())
                        .trim_end_matches(pattern.as_str()),
                )
            }
        },
        "trim_start_matches" => string(text.trim_start_matches(pattern(a).as_str())),
        "trim_end_matches" => string(text.trim_end_matches(pattern(a).as_str())),
        "split_once" | "rsplit_once" => {
            let separator = pattern(a);
            let split = if name == "split_once" {
                text.split_once(separator.as_str())
            } else {
                text.rsplit_once(separator.as_str())
            };
            option(
                split.map(|(left, right)| Dynamic::from_array(vec![string(left), string(right)])),
            )
        }
        "eq_ignore_ascii_case" => text.eq_ignore_ascii_case(&pattern(a)).into(),
        "is_char_boundary" => text
            .is_char_boundary(usize::try_from(a.as_int().unwrap_or(0)).unwrap_or(0))
            .into(),
        _ => Dynamic::UNIT,
    })
}

pub(super) const CHAR_METHODS: &[(&str, char)] = &[
    ("to_ascii_uppercase", 'c'),
    ("to_ascii_lowercase", 'c'),
    ("is_alphabetic", 'b'),
    ("is_numeric", 'b'),
    ("is_alphanumeric", 'b'),
    ("is_whitespace", 'b'),
    ("is_uppercase", 'b'),
    ("is_lowercase", 'b'),
    ("is_control", 'b'),
    ("is_ascii", 'b'),
    ("is_ascii_digit", 'b'),
    ("is_ascii_alphabetic", 'b'),
    ("is_ascii_alphanumeric", 'b'),
    ("is_ascii_uppercase", 'b'),
    ("is_ascii_lowercase", 'b'),
    ("is_ascii_whitespace", 'b'),
    ("is_ascii_punctuation", 'b'),
    ("is_ascii_hexdigit", 'b'),
    ("is_digit", 'b'),
    ("eq_ignore_ascii_case", 'b'),
    ("to_digit", 'o'),
    ("len_utf8", 'u'),
];

pub(super) fn char_method(c: char, name: &str, argument: &Dynamic) -> Dynamic {
    let radix = || u32::try_from(argument.as_int().unwrap_or(10)).unwrap_or(10);
    match name {
        "to_ascii_uppercase" => c.to_ascii_uppercase().into(),
        "to_ascii_lowercase" => c.to_ascii_lowercase().into(),
        "to_digit" => match c.to_digit(radix()) {
            Some(digit) => crate::option::some(INT::from(digit).into()),
            None => crate::option::none(),
        },
        "len_utf8" => (c.len_utf8() as INT).into(),
        name => match name {
            "is_alphabetic" => c.is_alphabetic(),
            "is_numeric" => c.is_numeric(),
            "is_alphanumeric" => c.is_alphanumeric(),
            "is_whitespace" => c.is_whitespace(),
            "is_uppercase" => c.is_uppercase(),
            "is_lowercase" => c.is_lowercase(),
            "is_control" => c.is_control(),
            "is_ascii" => c.is_ascii(),
            "is_ascii_digit" => c.is_ascii_digit(),
            "is_ascii_alphabetic" => c.is_ascii_alphabetic(),
            "is_ascii_alphanumeric" => c.is_ascii_alphanumeric(),
            "is_ascii_uppercase" => c.is_ascii_uppercase(),
            "is_ascii_lowercase" => c.is_ascii_lowercase(),
            "is_ascii_whitespace" => c.is_ascii_whitespace(),
            "is_ascii_punctuation" => c.is_ascii_punctuation(),
            "is_ascii_hexdigit" => c.is_ascii_hexdigit(),
            "is_digit" => c.is_digit(radix()),
            _ => argument
                .as_char()
                .is_ok_and(|other| c.eq_ignore_ascii_case(&other)),
        }
        .into(),
    }
}

/// `Slice` (`v[low..high]`) and `SplitAt` (`v.split_at(mid)`) of an array
/// or a string, a copy of the range, with Rust's panics for a range out of
/// bounds or a string index off a `char` boundary. `SliceGetRange`
/// (`v.get(low..high)`) is `None` where `Slice` panics, and `Some` of the
/// copy.
pub(super) fn slice_value(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    let checked = op == PreludeOp::SliceGetRange;
    let fail = |message: String| -> RawrResult {
        if checked {
            return Ok(crate::option::none());
        }
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let found = |value: Dynamic| -> RawrResult {
        Ok(if checked {
            crate::option::some(value)
        } else {
            value
        })
    };
    // A native slice, as a marked page's `&BLOCKS[1..2]` (#1291), is cut
    // where it is: the range borrows its elements, as Rust's does.
    let place = crate::place::get(args[0]);
    let text = args[0].clone().into_immutable_string().ok();
    let len = match (&place, &text, args[0].as_array_ref()) {
        (Some(place), ..) => place
            .invoke(crate::place::Operation::Len, &mut [])?
            .as_int()
            .ok()
            .and_then(|len| usize::try_from(len).ok())
            .ok_or_else(|| invariant("a native slice's length is not a usize"))?,
        (None, Some(text), _) => text.len(),
        (None, None, Ok(array)) => array.len(),
        (None, None, Err(_)) => {
            return Err(invariant("a slice is not an array or a string at runtime"))
        }
    };
    let at = |value: &Dynamic| usize::try_from(value.as_int().unwrap_or_default()).ok();
    let (low, high) = if op == PreludeOp::SplitAt {
        let mid = at(args[1]).filter(|mid| *mid <= len);
        let Some(mid) = mid else {
            return fail("mid > len".into());
        };
        (mid, mid)
    } else {
        let low = at(args[1]).unwrap_or(usize::MAX);
        let high = match args[2].as_int().unwrap_or(-1) {
            -1 => None,
            high => Some(usize::try_from(high).unwrap_or(usize::MAX)),
        };
        match slice_window(low, high, args[3].as_bool().unwrap_or(false), len) {
            Ok(window) => window,
            Err(message) => return fail(message),
        }
    };
    if let Some(place) = place {
        let window = |range: std::ops::Range<usize>| {
            place
                .window(range)
                .unwrap_or_else(|| Err(invariant("a native slice that can't be cut")))
        };
        if op == PreludeOp::SplitAt {
            return Ok(Dynamic::from_array(vec![
                window(0..low)?,
                window(low..len)?,
            ]));
        }
        return window(low..high).and_then(found);
    }
    let pieces: Vec<Dynamic> = match (&text, args[0].as_array_ref()) {
        (Some(text), _) => {
            for index in [low, high] {
                if !text.is_char_boundary(index) {
                    return fail(format!("byte index {index} is not a char boundary"));
                }
            }
            if op == PreludeOp::SplitAt {
                let (left, right) = text.split_at(low);
                vec![left.into(), right.into()]
            } else {
                return found(Dynamic::from(ImmutableString::from(&text[low..high])));
            }
        }
        (None, Ok(array)) => {
            if op == PreludeOp::SplitAt {
                vec![
                    Dynamic::from_array(array[..low].to_vec()),
                    Dynamic::from_array(array[low..].to_vec()),
                ]
            } else {
                return found(Dynamic::from_array(array[low..high].to_vec()));
            }
        }
        (None, Err(_)) => return Err(invariant("a slice is not an array at runtime")),
    };
    Ok(Dynamic::from_array(pieces))
}

/// Rust's integer methods (`checked_*`, `wrapping_*`, `saturating_*`,
/// `overflowing_*`, bit counts, rotations, Euclidean division) and the bit
/// operators `<<`, `>>` and `!`, by code: `(name, operand, output)`, the
/// operand `n` none, `s` the same kind, `u` a `u32`; the output `o` an
/// `Option`, `t` the kind, `p` a `(value, overflowed)` pair, `u` a `u32`,
/// `b` a `bool`, `a` the unsigned kind, `r` a `Result`.
pub(super) const INT_METHODS: &[(&str, char, char)] = &[
    ("checked_add", 's', 'o'),
    ("checked_sub", 's', 'o'),
    ("checked_mul", 's', 'o'),
    ("checked_div", 's', 'o'),
    ("checked_rem", 's', 'o'),
    ("checked_neg", 'n', 'o'),
    ("checked_pow", 'u', 'o'),
    ("wrapping_add", 's', 't'),
    ("wrapping_sub", 's', 't'),
    ("wrapping_mul", 's', 't'),
    ("wrapping_div", 's', 't'),
    ("wrapping_rem", 's', 't'),
    ("wrapping_neg", 'n', 't'),
    ("wrapping_pow", 'u', 't'),
    ("saturating_add", 's', 't'),
    ("saturating_sub", 's', 't'),
    ("saturating_mul", 's', 't'),
    ("saturating_div", 's', 't'),
    ("saturating_pow", 'u', 't'),
    ("overflowing_add", 's', 'p'),
    ("overflowing_sub", 's', 'p'),
    ("overflowing_mul", 's', 'p'),
    ("overflowing_div", 's', 'p'),
    ("overflowing_rem", 's', 'p'),
    ("overflowing_neg", 'n', 'p'),
    ("overflowing_pow", 'u', 'p'),
    ("count_ones", 'n', 'u'),
    ("count_zeros", 'n', 'u'),
    ("leading_zeros", 'n', 'u'),
    ("trailing_zeros", 'n', 'u'),
    ("rotate_left", 'u', 't'),
    ("rotate_right", 'u', 't'),
    ("rem_euclid", 's', 't'),
    ("div_euclid", 's', 't'),
    ("abs_diff", 's', 'a'),
    ("is_power_of_two", 'n', 'b'),
    ("ilog2", 'n', 'u'),
    ("ilog10", 'n', 'u'),
    // The operators, not spellable as methods.
    ("<<", 'x', 't'),
    (">>", 'x', 't'),
    ("!", 'n', 't'),
    ("try_from", 'x', 'r'),
    // A `u64`'s and a `usize`'s arithmetic, with Rust's overflow panics.
    ("+", 'x', 't'),
    ("-", 'x', 't'),
    ("*", 'x', 't'),
    ("/", 'x', 't'),
    ("%", 'x', 't'),
    ("**", 'x', 't'),
    // An `i128`'s and a `u128`'s operators, which the runtime's `i64` ones
    // do not compute past `i64`.
    ("neg", 'x', 't'),
    ("&", 'x', 't'),
    ("|", 'x', 't'),
    ("^", 'x', 't'),
    ("<", 'x', 'b'),
    (">", 'x', 'b'),
    ("<=", 'x', 'b'),
    (">=", 'x', 'b'),
    ("==", 'x', 'b'),
    ("!=", 'x', 'b'),
    // Methods of the signed kinds; `abs` and `signum` of the narrower ones
    // are the runtime's.
    ("abs", 'n', 't'),
    ("signum", 'n', 't'),
    ("unsigned_abs", 'n', 'a'),
    ("is_negative", 'n', 'b'),
    ("is_positive", 'n', 'b'),
];

/// The `INT_METHODS` only a signed kind has.
pub(super) const SIGNED_INT_METHODS: &[&str] = &[
    "abs",
    "signum",
    "unsigned_abs",
    "is_negative",
    "is_positive",
];

/// An `INT_METHODS` entry by name, for a method call.
pub(super) fn int_method_code(name: &str) -> Option<usize> {
    INT_METHODS
        .iter()
        .position(|(method, operand, _)| *method == name && *operand != 'x' && *method != "!")
}

/// std's panic for a range of a slice of `len` from `start` to `end`, in
/// the order `SliceIndex`'s checks find it.
pub(super) fn slice_index_fail(start: usize, end: usize, len: usize) -> String {
    if start > len {
        format!("range start index {start} out of range for slice of length {len}")
    } else if start > end && end <= len {
        format!("slice index starts at {start} but ends at {end}")
    } else {
        format!("range end index {end} out of range for slice of length {len}")
    }
}

/// The elements `low..high` of a slice of `len` that `s[low..high]`,
/// `s[low..=high]` (`inclusive`) or `s[low..]` (`high` `None`) takes, or
/// std's panic for the range, as `Index` on a slice checks it.
pub(crate) fn slice_window(
    low: usize,
    high: Option<usize>,
    inclusive: bool,
    len: usize,
) -> Result<(usize, usize), String> {
    let (end, fits) = match high {
        None => (len, low <= len),
        Some(high) if inclusive && high < len => (high + 1, low <= high + 1),
        Some(high) if inclusive => (high, false),
        Some(high) => (high, low <= high && high <= len),
    };
    if fits {
        Ok((low, end))
    } else {
        Err(slice_index_fail(low, end, len))
    }
}

/// The elements `Vec::drain` takes of a `Vec` of `len`, or std's panic,
/// which checks the range's end before its start.
pub(super) fn drain_window(
    low: usize,
    high: Option<usize>,
    inclusive: bool,
    len: usize,
) -> Result<(usize, usize), String> {
    let end = match high {
        None => len,
        Some(high) if inclusive && high >= len || !inclusive && high > len => {
            return Err(slice_index_fail(0, high, len));
        }
        Some(high) if inclusive => high + 1,
        Some(high) => high,
    };
    if low > end {
        Err(slice_index_fail(low, end, len))
    } else {
        Ok((low, end))
    }
}

/// `INT_METHODS[code]` on `value` of the kind `kinds[0]` (`Int::KINDS`
/// index), its operand of the kind `kinds[1]`, as debug Rust computes it:
/// in `i128`, a `u64`/`usize` read from its bits, the result truncated to
/// the kind; `try_from` converts from `kinds[1]` to `kinds[0]`.
pub(super) fn int_method(kinds: [INT; 2], args: &mut FnCallArgs) -> RawrResult {
    let panic = |message: &str| -> RawrResult {
        Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
    };
    let kind = |code: INT| Int::from_code(code).unwrap_or(Int::I64);
    let (kind, mut operand_kind) = (kind(kinds[0]), kind(kinds[1]));
    let code = usize::try_from(args[1].as_int().unwrap_or_default()).unwrap_or(0);
    let (name, operand_type, output) = INT_METHODS.get(code).copied().unwrap_or(("", 'n', 't'));
    if kind.wide() || operand_kind.wide() && name == "try_from" {
        return wide_method(kind, operand_kind, args);
    }
    // A shift by an `i128` or a `u128`: an amount past `i64` overflows.
    let operand = if operand_kind.wide() {
        operand_kind = Int::I64;
        let amount = wide_value(args[2]).map_or(0, Wide::signed);
        INT::try_from(amount.max(-1)).unwrap_or(INT::MAX)
    } else {
        args[2].as_int().unwrap_or_default()
    };
    let bits = kind.bits();
    let mask = u128::MAX >> (128 - bits);
    let (low, high) = kind.full_range();
    let value = args[0].as_int().unwrap_or_default();
    if name == "try_from" {
        let source = operand_kind.widen(value);
        // `TryFromIntError`'s `Debug`, its runtime value.
        return Ok(if (low..=high).contains(&source) {
            crate::result::ok(kind.store(source).into())
        } else if source > high {
            crate::result::err(ImmutableString::from("TryFromIntError(PosOverflow)").into())
        } else {
            crate::result::err(ImmutableString::from("TryFromIntError(NegOverflow)").into())
        });
    }
    let a = kind.widen(value);
    let b = if operand_type == 's' {
        kind.widen(operand)
    } else {
        operand_kind.widen(operand)
    };
    let wrap = |x: u128| -> INT { kind.store(kind.sign((x & mask) as i128)) };
    let overflow_division = kind.signed() && a == low && b == -1;
    let operation = match name {
        "+" => "add",
        "-" => "sub",
        "*" => "mul",
        "/" => "div",
        "%" => "rem",
        "**" => "pow",
        _ => name.rsplit('_').next().unwrap_or(name),
    };
    // The exact result, `None` past `i128`, and the wrapped one.
    let (exact, wrapped) = match operation {
        "add" => (a.checked_add(b), wrap((a as u128).wrapping_add(b as u128))),
        "sub" => (a.checked_sub(b), wrap((a as u128).wrapping_sub(b as u128))),
        "mul" => (a.checked_mul(b), wrap((a as u128).wrapping_mul(b as u128))),
        "neg" => (Some(-a), wrap(0u128.wrapping_sub(a as u128))),
        "pow" => {
            let exponent = u32::try_from(b).unwrap_or(u32::MAX);
            (
                a.checked_pow(exponent),
                wrap((a as u128).wrapping_pow(exponent)),
            )
        }
        "div" | "rem" | "euclid" if b == 0 => {
            if name.starts_with("checked") {
                return Ok(crate::option::none());
            }
            return panic(if operation == "rem" || name == "rem_euclid" {
                "attempt to calculate the remainder with a divisor of zero"
            } else {
                "attempt to divide by zero"
            });
        }
        "div" => (Some(a / b), wrap((a / b) as u128)),
        "rem" if overflow_division => (None, 0),
        "rem" => (Some(a % b), wrap((a % b) as u128)),
        _ => (None, 0),
    };
    let in_range = exact.filter(|exact| (low..=high).contains(exact));
    let result: Dynamic = match output {
        'o' => match in_range {
            Some(exact) => crate::option::some(kind.store(exact).into()),
            None => crate::option::none(),
        },
        'p' => Dynamic::from_array(vec![wrapped.into(), in_range.is_none().into()]),
        't' if name.len() <= 2 && name != "<<" && name != ">>" && name != "!" => match in_range {
            Some(exact) => kind.store(exact).into(),
            None => {
                let verb = match operation {
                    "add" => "add",
                    "sub" => "subtract",
                    "div" => "divide",
                    "rem" => "calculate the remainder",
                    "pow" => "exponentiate",
                    _ => "multiply",
                };
                return panic(&format!("attempt to {verb} with overflow"));
            }
        },
        't' if name.starts_with("wrapping") => wrapped.into(),
        't' if name.starts_with("saturating") => {
            let negative = match operation {
                "add" => b < 0,
                "sub" => b > 0,
                "mul" | "div" => (a < 0) != (b < 0),
                _ => a < 0 && b % 2 == 1,
            };
            kind.store(in_range.unwrap_or(if negative { low } else { high }))
                .into()
        }
        _ => {
            let pattern = (a as u128) & mask;
            let ones = pattern.count_ones() as INT;
            match name {
                "count_ones" => ones,
                "count_zeros" => INT::from(bits) - ones,
                "leading_zeros" => INT::from(pattern.leading_zeros()) - (128 - INT::from(bits)),
                "trailing_zeros" => INT::from(pattern.trailing_zeros().min(bits)),
                "rotate_left" | "rotate_right" => {
                    let turn = b.rem_euclid(i128::from(bits)) as u32;
                    let turn = if name == "rotate_left" {
                        turn
                    } else {
                        (bits - turn) % bits
                    };
                    match turn {
                        0 => wrap(pattern),
                        _ => wrap((pattern << turn) | (pattern >> (bits - turn))),
                    }
                }
                "rem_euclid" | "div_euclid" if overflow_division => {
                    return panic(if name == "rem_euclid" {
                        "attempt to calculate the remainder with overflow"
                    } else {
                        "attempt to divide with overflow"
                    })
                }
                "rem_euclid" => kind.store(a.rem_euclid(b)),
                "div_euclid" => kind.store(a.div_euclid(b)),
                "abs_diff" => kind.unsigned().store((a - b).abs()),
                "unsigned_abs" => kind.unsigned().store(a.abs()),
                "is_negative" => return Ok((a < 0).into()),
                "is_positive" => return Ok((a > 0).into()),
                "is_power_of_two" => return Ok((a > 0 && a & (a - 1) == 0).into()),
                "ilog2" | "ilog10" if a <= 0 => {
                    return panic("argument of integer logarithm must be positive")
                }
                "ilog2" => INT::from(a.ilog2()),
                "ilog10" => INT::from(a.ilog10()),
                "<<" | ">>" if !(0..i128::from(bits)).contains(&b) => {
                    return panic(if name == "<<" {
                        "attempt to shift left with overflow"
                    } else {
                        "attempt to shift right with overflow"
                    })
                }
                "<<" => wrap(pattern << b as u32),
                ">>" => wrap((a >> b) as u128),
                "!" => wrap(!(a as u128)),
                _ => return Err(invariant("an unknown integer method at runtime")),
            }
            .into()
        }
    };
    Ok(result)
}

pub(super) fn default_value(
    model: &Nominal,
    models: &Models,
    _stack: &mut Vec<type_def::Id>,
) -> Option<Dynamic> {
    let definition = model.definition();
    let identity = type_def::Identity::Script(definition.key().clone());
    models
        .runtime
        .materialize_default(&identity, definition.schema())
        .ok()
}
