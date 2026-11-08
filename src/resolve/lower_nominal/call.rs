//! Calls and their arguments: a script function's call, a call through a
//! local or a function value, function items as values, and `&mut`
//! arguments copied in and back out.

use super::*;

impl<'a> Parser<'a> {
    /// The tokens of a call's parenthesised arguments, from its `(` to the
    /// matching `)` (consumed), without a trailing comma; with the
    /// position of the `)`. The calls among them, and the call they close,
    /// are noted for `frame_effects` as the tokens are parsed, not as they
    /// are taken: the call runs after its arguments, so a scope frame one
    /// of them opens is not preceded by it (`Into::<T>::into(label("x"))`).
    pub(super) fn take_argument_tokens(
        &mut self,
    ) -> Result<(Vec<(Token, Position)>, Position), Error> {
        let (open, position) = self.next();
        if open == Token::Unit {
            return Ok((Vec::new(), position));
        }
        if open != Token::LeftParen {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!("'(' to start call arguments; found `{open}`").into(),
                source: point_span(position),
            });
        }
        let noted = self
            .frame_effects
            .iter()
            .map(|(_, effects)| effects.len())
            .collect::<Vec<_>>();
        let taken = self.take_until_close();
        for ((_, effects), noted) in self.frame_effects.iter_mut().zip(noted) {
            effects.truncate(noted);
        }
        taken
    }

    /// The tokens up to the `)` that closes an open `(` (consumed), for
    /// `take_argument_tokens`.
    pub(super) fn take_until_close(&mut self) -> Result<(Vec<(Token, Position)>, Position), Error> {
        let mut depth = 0usize;
        let mut tokens = Vec::new();
        loop {
            let (token, position) = self.next();
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen if depth == 0 => {
                    if matches!(tokens.last(), Some((Token::Comma, _))) {
                        tokens.pop();
                    }
                    return Ok((tokens, position));
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::EOF => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "')' to close call arguments".into(),
                        source: point_span(position),
                    });
                }
                _ => {}
            }
            tokens.push((token, position));
        }
    }

    /// The `fn` pointer `function` as the closure `|a, ..| f(a, ..)` that
    /// holds it, with `f` evaluated once, first: what native code calls
    /// where it takes a callable, as Rust's `fn` pointers implement `Fn`.
    #[cfg(feature = "rust-import")]
    pub(super) fn fn_pointer_closure(
        &mut self,
        function: ParsedExpr,
        inputs: &[Ty],
    ) -> Result<ParsedExpr, Error> {
        let at = function.span.start();
        let callee = self.temporary("function");
        let mut prelude = origin::Fragment::plain(format!("let {callee} = "));
        prelude.push(function.rendered_source());
        prelude.push_text(";\n");
        let outer = self.bindings.clone();
        self.bindings.insert(
            callee.as_str().into(),
            Binding {
                ty: function.ty.clone(),
                mutable: false,
                available: true,
                declaration: function.span,
                kind: BindingKind::Local,
            },
        );
        let identifier = |name: &str| (Token::Identifier(Box::new(name.into())), at);
        let names = (0..inputs.len())
            .map(|index| format!("__rawr_input{index}"))
            .collect::<Vec<_>>();
        let mut tokens = Vec::new();
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                tokens.push((Token::Comma, at));
            }
            tokens.push(identifier(name));
        }
        tokens.extend([
            (Token::Pipe, at),
            identifier(&callee),
            (Token::LeftParen, at),
        ]);
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                tokens.push((Token::Comma, at));
            }
            tokens.push(identifier(name));
        }
        tokens.push((Token::RightParen, at));
        self.splice_tokens(tokens);
        self.closure_hint = Some(inputs.to_vec().into_boxed_slice());
        let closure = self.parse_closure(Token::Pipe, function.span, true);
        self.bindings = outer;
        let closure = closure?;
        prelude.push(closure.prelude.clone());
        Ok(ParsedExpr {
            prelude,
            source: closure.source,
            ty: closure.ty,
            span: function.span,
            lvalue: None,
            outcome: closure.outcome,
            evaluation: closure.evaluation,
        }
        .retain_origin())
    }

    /// A method named as a value (`Type::method`) is the closure
    /// `|receiver, ..| receiver.method(..)` over `inputs`, the receiver's
    /// type first.
    pub(super) fn method_value(
        &mut self,
        member: ImmutableString,
        inputs: Box<[Ty]>,
        owner_span: Span,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let at = member_span.start();
        let name = |index: usize| {
            (
                Token::Identifier(Box::new(format!("__rawr_method{index}").into())),
                at,
            )
        };
        let mut tokens = Vec::new();
        for index in 0..inputs.len() {
            if index > 0 {
                tokens.push((Token::Comma, at));
            }
            tokens.push(name(index));
        }
        tokens.extend([
            (Token::Pipe, at),
            name(0),
            (Token::Period, at),
            (Token::Identifier(Box::new(member.as_str().into())), at),
        ]);
        if inputs.len() == 1 {
            tokens.push((Token::Unit, at));
        } else {
            tokens.push((Token::LeftParen, at));
            for index in 1..inputs.len() {
                if index > 1 {
                    tokens.push((Token::Comma, at));
                }
                tokens.push(name(index));
            }
            tokens.push((Token::RightParen, at));
        }
        // What the value's use gives it (`map`'s items) fixes the inputs the
        // path leaves open (`Iterator::count`'s `Self`); a mismatch is the
        // closure's, reported where its use checks it.
        if let Some(given) = self.closure_hint.take()
            && given.len() == inputs.len()
        {
            let checkpoint = self.inference.checkpoint();
            if inputs
                .iter()
                .zip(&given)
                .all(|(own, given)| self.unify_type(own, given))
            {
                self.inference.commit(checkpoint);
            } else {
                self.inference.rollback(checkpoint);
            }
        }
        #[cfg(feature = "rust-import")]
        let written = match inputs.first().map(|input| self.inference.resolve(input)) {
            Some(Ty::Script(owner, _)) => Some(owner),
            Some(Ty::Reference { target, .. }) => match *target {
                Ty::Script(owner, _) => Some(owner),
                _ => None,
            },
            _ => None,
        }
        .and_then(|owner| self.models.method(owner, &member))
        .and_then(|method| method.lifetimes().written_output.as_deref().cloned());
        self.splice_tokens(tokens);
        self.closure_hint = Some(inputs);
        let value = self.parse_closure(Token::Pipe, owner_span, false)?;
        #[cfg(feature = "rust-import")]
        if let (Some(written), Ty::FnItem { key, .. }) = (written, &value.ty) {
            self.path_outputs.insert((**key).clone(), written);
        }
        Ok(self.path_value(value))
    }

    /// `Type::method(receiver, a, ..)` read from its `(`: the tokens of
    /// `(receiver.method(a, ..))`, the receiver without its `&` or `&mut`,
    /// which the method call takes again. `None`, consuming the arguments,
    /// when there is no receiver argument.
    pub(super) fn path_method_call(
        &mut self,
        member: &str,
        member_span: Span,
        trait_path: Option<ImmutableString>,
    ) -> Option<Vec<(Token, Position)>> {
        let (arguments, end) = self.take_argument_tokens().ok()?;
        let mut depth = 0_usize;
        let comma = arguments.iter().position(|(token, _)| {
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::Comma => return depth == 0,
                _ => {}
            }
            false
        });
        let (receiver, rest) = match comma {
            Some(comma) => (&arguments[..comma], &arguments[comma + 1..]),
            None => (&arguments[..], &[][..]),
        };
        let receiver = match receiver {
            [(Token::Ampersand, _), (Token::Mut, _), rest @ ..]
            | [(Token::Ampersand, _), rest @ ..] => rest,
            receiver => receiver,
        };
        if receiver.is_empty() {
            return None;
        }
        let at = member_span.start();
        // A place (`a`, `a.b`, `v[i].c`) stays one, so a `&mut self`
        // method changes it; any other receiver is parenthesised.
        let mut depth = 0_usize;
        let plain = receiver.iter().all(|(token, _)| match token {
            Token::LeftBracket => {
                depth += 1;
                true
            }
            Token::RightBracket => {
                depth = depth.saturating_sub(1);
                true
            }
            _ if depth > 0 => true,
            token => matches!(
                token,
                Token::Identifier(_) | Token::Period | Token::IntegerConstant(_)
            ),
        });
        // What follows the receiver takes its last position, so each span
        // the call makes runs forward.
        let after = receiver.last().map_or(end, |(_, position)| *position);
        if let Some(trait_path) = trait_path {
            self.spliced_traits.insert(after, trait_path);
        }
        if let Some(turbofish) = self.pending_turbofish.take() {
            self.spliced_turbofish.insert(after, turbofish);
        }
        let mut tokens = vec![(Token::LeftParen, at)];
        if !plain {
            tokens.push((Token::LeftParen, at));
        }
        tokens.extend(receiver.iter().cloned());
        if !plain {
            tokens.push((Token::RightParen, after));
        }
        tokens.push((Token::Period, after));
        tokens.push((Token::Identifier(Box::new(member.into())), after));
        if rest.is_empty() {
            tokens.push((Token::Unit, end));
        } else {
            tokens.push((Token::LeftParen, after));
            tokens.extend(rest.iter().cloned());
            tokens.push((Token::RightParen, end));
        }
        tokens.push((Token::RightParen, end));
        Some(tokens)
    }

    /// Put `tokens` before the rest of the input, to be parsed next.
    /// After `name`, a path `::seg::<A, B>::member`: the type `name::seg<A,
    /// B>` it writes, its arguments taken from the input so the path reads
    /// `name::seg::member`. `None`, consuming nothing, for any other shape
    /// (a function's turbofish `f::<T>(..)` among them).
    pub(super) fn take_path_arguments(
        &mut self,
        name: &ImmutableString,
        name_span: Span,
    ) -> Result<Option<Ty>, Error> {
        let mut ahead = self.input.clone();
        let mut segments = 0;
        let mut last = name.to_string();
        loop {
            match (ahead.next(), ahead.next()) {
                (Some((Token::DoubleColon, _)), Some((Token::Identifier(segment), _))) => {
                    segments += 1;
                    last = segment.to_string();
                }
                (Some((Token::DoubleColon, _)), Some((Token::LessThan, _))) => break,
                _ => return Ok(None),
            }
        }
        // `Into::<T>::into(x)` and kin are a trait's turbofish.
        if self.models.traits.contains_key(last.as_str())
            || self.models.get(&last).is_none()
                && crate::resolve::std_trait::anywhere(&last).is_some()
        {
            return Ok(None);
        }
        let mut depth = 1usize;
        let mut arguments = 0;
        while depth > 0 {
            let closing = match ahead.next() {
                Some((Token::LessThan, _)) => {
                    depth += 1;
                    0
                }
                Some((Token::GreaterThan, _)) => 1,
                Some((Token::RightShift, _)) => 2,
                Some((Token::EOF | Token::SemiColon | Token::LeftBrace, _)) | None => {
                    return Ok(None)
                }
                Some(_) => 0,
            };
            let Some(rest) = depth.checked_sub(closing) else {
                return Ok(None);
            };
            depth = rest;
            arguments += 1;
        }
        if !matches!(
            (ahead.next(), ahead.next()),
            (
                Some((Token::DoubleColon, _)),
                Some((Token::Identifier(_), _))
            )
        ) {
            return Ok(None);
        }
        // The path's segments, then `::<`, the arguments and `>`.
        let path = (0..segments * 2).map(|_| self.next()).collect::<Vec<_>>();
        let (_, open) = self.next();
        self.next();
        let mut written = vec![(
            Token::Identifier(Box::new(name.as_str().into())),
            name_span.start(),
        )];
        written.extend(path.iter().cloned());
        written.push((Token::LessThan, open));
        written.extend((0..arguments).map(|_| self.next()));
        written.extend(path);
        self.splice_tokens(written);
        self.parse_local_type().map(Some)
    }

    pub(super) fn splice_tokens(&mut self, tokens: Vec<(Token, Position)>) {
        let rest = std::mem::replace(&mut self.input, Vec::new().into_iter().peekable());
        self.input = tokens
            .into_iter()
            .chain(rest)
            .collect::<Vec<_>>()
            .into_iter()
            .peekable();
    }

    /// What a function copying `&mut` parameters back out returns for
    /// `value`: `[value, ..parameters]`; `None` in other functions and in
    /// closures.
    pub(super) fn copied_return(&self, value: origin::Fragment) -> Option<origin::Fragment> {
        if self.copy_out.is_empty() || self.closures.len() != self.copy_out_depth {
            return None;
        }
        let mut returned = value.surrounded("[", "");
        for name in &self.copy_out {
            returned.push_text(format!(", {}({name})", crate::engine::KEYWORD_MOVE));
        }
        returned.push_text("]");
        Some(returned)
    }

    /// A call's arguments, each of which may be `&mut place` for a `&mut`
    /// parameter.
    pub(super) fn parse_borrowing_arguments(
        &mut self,
        context: &str,
    ) -> Result<(Vec<ParsedExpr>, Position), Error> {
        let borrow = self.mutable_borrow.replace(self.expression_stack.len() + 1);
        let arguments = self.parse_call_arguments(context);
        self.mutable_borrow = borrow;
        arguments
    }

    /// A call `call` of a function copying the `&mut` arguments `places`
    /// in and back out: each place is stored from the returned array,
    /// then the call's value is the first element.
    pub(super) fn copied_call(
        &mut self,
        call: origin::Fragment,
        places: &[ParsedExpr],
    ) -> Result<origin::Fragment, Error> {
        if places.is_empty() {
            return Ok(call);
        }
        // Each value moves out of the returned array, which ends here.
        let moved = crate::engine::KEYWORD_MOVE;
        let returned = self.temporary("returned");
        let mut source = origin::Fragment::plain(format!("{{ let {returned} = "));
        source.push(call);
        source.push_text("; ");
        for (index, place) in places.iter().enumerate() {
            let value = format!("{moved}({returned}[{}])", index + 1);
            source.push(self.store_into(place, &value)?);
            source.push_text(" ");
        }
        source.push_text(format!("{moved}({returned}[0]) }})"));
        Ok(source.surrounded("(", ""))
    }

    /// `value` stored into the `&mut` place `place`: a local, an element or
    /// a field.
    pub(super) fn store_into(
        &mut self,
        place: &ParsedExpr,
        value: &str,
    ) -> Result<origin::Fragment, Error> {
        // A closure written in place (`&mut |x| ..`) is a temporary: its
        // captures are where it reaches them, and nothing is stored back.
        if place.lvalue.is_none() {
            return Ok(origin::Fragment::default());
        }
        if let Some(Lvalue::Slice(slice)) = &place.lvalue {
            self.require_mutable_binding(&slice.binding, place.span)?;
            let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
            let code = vec_method_code("store");
            let mut source = slice.base.clone();
            source.push_text(format!(".{helper}({code}, "));
            source.push(slice.low.clone());
            source.push_text(format!(", {value}, ());"));
            return Ok(source);
        }
        // A nested place is read and stored back where it is, so two
        // stores below one field (`swap(&mut s.pair.0, &mut s.pair.1)`)
        // each keep the other's.
        if let Some(Lvalue::Nested(nested)) = &place.lvalue {
            self.require_mutable_binding(&nested.binding, place.span)?;
            let nested = (**nested).clone();
            let mut store = self.store_nested(&nested, value.into());
            store.push_text(";");
            return Ok(store);
        }
        let helper = self.push_op(Op::Prelude(PreludeOp::ReferentStore));
        let mut target = self.keyed_place(place)?;
        target.push_text(format!(".{helper}({value});"));
        Ok(target)
    }

    /// The `&mut` arguments at `indexes` of a call, each a `&mut` place
    /// borrowed once, as Rust's borrow rules have them (E0499): two
    /// borrows of one local are apart only as two of its fields.
    pub(super) fn mutable_arguments(
        &mut self,
        arguments: &[ParsedExpr],
        indexes: impl IntoIterator<Item = usize>,
    ) -> Result<Vec<ParsedExpr>, Error> {
        let mut places: Vec<ParsedExpr> = Vec::new();
        for index in indexes {
            let Some(place) = arguments.get(index) else {
                continue;
            };
            // `self` in a `&mut self` method, passed on to a `&mut`
            // parameter, is the receiver's place.
            let receiver = place.lvalue.is_none()
                && self.closures.is_empty()
                && place.source.as_str() == "self"
                && self.bindings.get("self").is_some_and(|binding| {
                    binding.mutable && matches!(binding.kind, BindingKind::Receiver { .. })
                });
            let mut place = place.clone();
            if receiver {
                place.lvalue = Some(Lvalue::Binding {
                    binding: "self".into(),
                    ty: place.ty.clone(),
                });
            }
            let place = &place;
            // `twice(&mut |x| v.push(x))`: the closure is a temporary the
            // callee calls where it is; nothing is stored back.
            if place.lvalue.is_none()
                && let Ty::Reference {
                    mutable: true,
                    target,
                } = self.inference.resolve(&place.ty)
                && matches!(self.inference.resolve(&target), Ty::FnItem { .. })
            {
                places.push(place.clone());
                continue;
            }
            if !matches!(
                self.inference.resolve(&place.ty),
                Ty::Reference { mutable: true, .. }
            ) || place.lvalue.is_none()
            {
                return Err(Error::TypeMismatch {
                    expected: "a `&mut` place".into(),
                    actual: place.ty.name(&self.models),
                    expression: place.span,
                });
            }
            let (name, parts) = self.borrowed_parts(place);
            if let Some(other) = places.iter().find(|other| {
                let (other_name, other_parts) = self.borrowed_parts(other);
                let shared = parts.len().min(other_parts.len());
                other_name == name && parts[..shared] == other_parts[..shared]
            }) {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "cannot borrow `{name}` as mutable more than once at a time (E0499)"
                    )
                    .into(),
                    source: Span::new(other.span.start(), place.span.end()),
                });
            }
            // `bump(c.slot(0))`: the call is made once, before the
            // statement, and its place is read and stored through its path.
            let mut place = place.clone();
            if let Some(Lvalue::Nested(nested)) = &place.lvalue
                && Self::reached_once(nested)
            {
                let (statement, held) = self.hold_place((**nested).clone());
                self.field_entries
                    .push((statement, origin::Fragment::default()));
                let (read, at, store) = self.nested_parts(&held);
                let value = self.temporary("value");
                let mut source = origin::Fragment::plain("({ ");
                source.push(read);
                source.push_text(format!("let {value} = "));
                source.push(at);
                source.push_text("; ");
                source.push(store);
                source.push_text(format!("{value} }})"));
                place.source = source;
                place.prelude = origin::Fragment::default();
                place.lvalue = Some(Lvalue::Nested(Box::new(held)));
            }
            places.push(place);
        }
        Ok(places)
    }

    /// The local a `&mut` place borrows, and the fields (`Ok(slot)`) and
    /// tuple members (`Err(index)`) below it that it borrows: two borrows
    /// of one local are apart only when these differ, as Rust's borrow
    /// rules have them (E0499). An element `v[i]` borrows all of `v`.
    pub(super) fn borrowed_parts(
        &self,
        place: &ParsedExpr,
    ) -> (ImmutableString, Vec<Result<usize, usize>>) {
        let mut parts = Vec::new();
        let (binding, base, steps) = match &place.lvalue {
            Some(Lvalue::Binding { binding, .. }) => return (binding.clone(), parts),
            Some(Lvalue::Place { binding, place, .. }) => (binding, place.as_str(), &[][..]),
            Some(Lvalue::Field(field)) => {
                return (field.binding.clone(), vec![Ok(field.slot)]);
            }
            Some(Lvalue::Nested(nested)) => {
                (&nested.binding, nested.base.as_str(), &nested.steps[..])
            }
            Some(Lvalue::Slice(slice)) => return (slice.binding.clone(), parts),
            None => return (ImmutableString::new(), parts),
        };
        // `t.0.1` of a tuple local is written `t[0][1]`.
        let mut rest = base.strip_prefix(binding.as_str()).unwrap_or("?");
        let mut ty = self
            .bindings
            .get(binding)
            .map_or(Ty::Unknown, |binding| self.inference.resolve(&binding.ty));
        while let Some(after) = rest.strip_prefix('[') {
            let Ty::Tuple(members) = ty else {
                return (binding.clone(), parts);
            };
            let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let (Ok(index), Some(after)) = (
                after[..digits].parse::<usize>(),
                after[digits..].strip_prefix(']'),
            ) else {
                return (binding.clone(), parts);
            };
            let Some(member) = members.get(index) else {
                return (binding.clone(), parts);
            };
            parts.push(Err(index));
            ty = self.inference.resolve(member);
            rest = after;
        }
        if !rest.is_empty() {
            return (binding.clone(), parts);
        }
        for step in steps {
            match step {
                PlaceStep::Field { slot, .. } => parts.push(Ok(*slot)),
                PlaceStep::Member(index) => parts.push(Err(*index)),
                _ => break,
            }
        }
        (binding.clone(), parts)
    }

    /// A call through a local binding, which shadows any item of its name.
    pub(super) fn parse_local_call(
        &mut self,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.record_capture(&name);
        let binding = &self.bindings[name.as_str()];
        // Calling a local closure borrows it: `&mut` when it changes what
        // it captures (`FnMut`).
        self.mention(&name, binding.declaration, &binding.ty, name_span);
        let mutable = match self.inference.resolve(&binding.ty) {
            Ty::FnItem { key, .. } => self
                .flow()
                .closure(&key)
                .is_some_and(|closure| closure.mutable),
            // A closure a function returned, as `impl FnMut(i32)`: a call of
            // one declared `mut` is an `FnMut` call, which borrows it `&mut`,
            // unless its type is `impl Fn` or `fn` (T179).
            Ty::Fn { .. } => {
                binding.mutable
                    && matches!(binding.kind, BindingKind::Local)
                    && !self.parameter_declarations.contains(&binding.declaration)
                    && !self.shared_call_fns.contains(&binding.declaration)
            }
            _ => false,
        };
        if mutable {
            self.flow().mutable_call(name_span.start(), name_span);
        } else {
            self.flow().shared(name_span.start());
        }
        let binding = &self.bindings[name.as_str()];
        let mut ty = self.inference.resolve(&binding.ty);
        if !binding.available {
            return Err(self.moved_value(&name, binding, name_span, false));
        }
        // A closure that moves out a capture is `FnOnce`: its call moves it.
        if let Ty::FnItem { key, .. } = &ty
            && self.once_closures.contains(&**key)
            && matches!(binding.kind, BindingKind::Local)
        {
            let declaration = binding.declaration;
            let outer = self
                .closures
                .last()
                .is_some_and(|scope| scope.outer.get(name.as_str()) == Some(&declaration));
            if !outer {
                if let Some(binding) = self.bindings.get_mut(name.as_str()) {
                    binding.available = false;
                }
                self.moved_at.insert(declaration, name_span);
            }
        }
        // `f: &dyn Fn(A) -> B` (or `Box<dyn Fn>`) is called through the
        // reference, as in Rust; references and boxes are values at runtime.
        // An `Rc<dyn Fn>` is called through its value.
        let mut through = 0;
        loop {
            ty = match ty {
                Ty::Reference { target, .. } | Ty::Boxed(target) => self.inference.resolve(&target),
                Ty::Counted(_, target) => {
                    through += 1;
                    self.inference.resolve(&target)
                }
                other => {
                    ty = other;
                    break;
                }
            };
        }
        self.reject_invocation(&ty, name_span, "called as a function")?;
        // `f: &mut impl FnMut(i32)` is the closure where the caller has it
        // (`this`, or a place a closure reaches): the call goes through it.
        let declaration = self.bindings[name.as_str()].declaration;
        let reference = self.bindings[name.as_str()].ty.clone();
        self.reject_shadowed_root(&name, declaration, name_span)?;
        let callee = match self.aliased_element(&name, declaration, &reference, name_span)? {
            Some(place) => place.rendered_source().surrounded("(", ")"),
            None => origin::Fragment::plain(name.to_string()),
        };
        let mut callee = callee;
        for _ in 0..through {
            callee = self.counted_call(PreludeOp::CountedGet, callee, []);
        }
        self.call_function_value(callee, ty, name, name_span)
    }

    /// A call of a function value of type `ty` (a closure, `fn` pointer or
    /// function item) rendered by `callee`.
    pub(super) fn call_function_value(
        &mut self,
        callee: origin::Fragment,
        ty: Ty,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // `let f = Shape::Circle; f(1.0)`.
        let (callee, ty) = match self.constructor_function(&ty, name_span) {
            Some(function) => (function.source.surrounded("(", ")"), function.ty),
            None => (callee, ty),
        };
        let choice = matches!(ty, Ty::FnChoice { .. });
        // A crate function's signature says what its value keeps; a
        // closure's body does.
        let closure = match &ty {
            Ty::FnItem { key, .. } => self
                .flow()
                .closure(key)
                .map(|closure| closure.keeps.clone()),
            _ => None,
        };
        let function = match &ty {
            Ty::FnItem { key, .. } => self
                .models
                .functions_by_key
                .get(&**key)
                .map(|function| function.definition.clone()),
            _ => None,
        };
        // rustc's E0061 for a function's value or a `fn` pointer, and E0057
        // for a closure, at its definition where this file has it.
        let (callee_kind, definition) = match &ty {
            Ty::FnItem { key, .. } => match self.models.functions_by_key.get(&**key) {
                Some(function) => (
                    Callee::Function,
                    (function.source == self.models.context)
                        .then_some(function.definition.name.span),
                ),
                None => (
                    Callee::Closure,
                    self.lifted
                        .iter()
                        .find(|lifted| lifted.key == **key)
                        .map(|lifted| lifted.header),
                ),
            },
            Ty::FnChoice { .. } => (Callee::Function, None),
            _ => (Callee::Closure, None),
        };
        let (Ty::Fn { inputs, output }
        | Ty::FnItem { inputs, output, .. }
        | Ty::FnChoice { inputs, output, .. }) = ty
        else {
            return Err(Error::TypeMismatch {
                expected: "a function".into(),
                actual: ty.name(&self.models),
                expression: name_span,
            });
        };
        let copied = inputs
            .iter()
            .enumerate()
            .filter(|(_, input)| {
                matches!(
                    self.inference.resolve(input),
                    Ty::Reference { mutable: true, .. }
                )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let (mut arguments, end) = if copied.is_empty() {
            self.parse_call_arguments("function arguments")?
        } else {
            self.parse_borrowing_arguments("function arguments")?
        };
        let call = span(name_span.start(), end);
        if arguments.len() != inputs.len() {
            return Err(Error::ArgumentCount {
                callee: callee_kind,
                name,
                expected: inputs.len(),
                found: arguments.len(),
                call,
                definition,
            });
        }
        for (expected, argument) in inputs.iter().zip(arguments.iter_mut()) {
            self.coerce_expr(expected, argument)?;
            self.move_local(argument)?;
        }
        let places = self.mutable_arguments(&arguments, copied)?;
        let pointer = if choice { "[1]" } else { "" };
        let mut source = callee;
        source.push_text(format!("{pointer}.__rawr_call("));
        source.push(origin::Fragment::join(
            arguments.iter().map(ParsedExpr::rendered_source),
            ", ",
        ));
        source.push_text(")");
        let source = self.copied_call(source, &places)?;
        let kept = self.inference.resolve(&output);
        match (&function, closure) {
            (Some(function), _) => {
                let summary = self.function_summary(function);
                self.kept_by_call(call, &kept, None, &summary);
            }
            // A closure's value keeps what its body returns of what it
            // captured.
            (None, Some(Some(keeps))) => {
                if self.may_hold_borrow(&kept) {
                    self.flow().set_origins(call, keeps);
                }
            }
            (None, _) if self.may_hold_borrow(&kept) => self
                .flow()
                .unmodeled("a borrow a closure's value keeps", call),
            (None, _) => {
                let summary = crate::resolve::borrowck::signature::Summary::unknown();
                self.kept_by_call(call, &kept, None, &summary);
            }
        }
        Ok(self.operator_expr(source, *output, call))
    }

    /// A function item passed to the host becomes its symbolic callable: a
    /// named function's item, or a lifted closure's generated item.
    /// `Some(f)` where the host takes an `Option` of a callable, as iced's
    /// `on_input_maybe` does: a named function or closure has its own type,
    /// so the `Some` holds that one item, which becomes the host callable.
    pub(super) fn option_host_callable(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        let Ty::Option(element) = self.inference.resolve(&value.ty) else {
            return Ok(());
        };
        if !matches!(*element, Ty::FnItem { .. }) {
            return Ok(());
        }
        let mut item = self.operator_expr("()".into(), *element, value.span);
        self.host_callable(&mut item)?;
        if !matches!(item.ty, Ty::Callable { .. }) {
            return Ok(());
        }
        let option = self.temporary("option");
        let mut rendered = origin::Fragment::plain(format!("{{ let {option} = "));
        rendered.push(value.rendered_source());
        rendered.push_text("; if ");
        let test = self.prelude_op(PreludeOp::IsSome, [option.as_str().into()]);
        rendered.push(test);
        rendered.push_text(" { ");
        let some = self.prelude_op(PreludeOp::Some, [item.rendered_source()]);
        rendered.push(some);
        rendered.push_text(" } else { ");
        let none = self.prelude_op(PreludeOp::None, []);
        rendered.push(none);
        rendered.push_text(" } }");
        value.prelude = origin::Fragment::default();
        value.source = rendered.surrounded("(", ")");
        value.ty = Ty::Option(Box::new(item.ty));
        value.lvalue = None;
        value.source.cover(value.span);
        Ok(())
    }

    pub(super) fn host_callable(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        self.host_callable_of(value, false)
    }

    /// [`host_callable`](Self::host_callable), where a function of no
    /// inputs is a callable too when `nullary`: an imported item takes
    /// Freya's `fn app()` as the closure that calls it (D551.6).
    pub(super) fn host_callable_of(
        &mut self,
        value: &mut ParsedExpr,
        nullary: bool,
    ) -> Result<(), Error> {
        self.lift_fn_pointer(value);
        let Ty::FnItem {
            key,
            inputs,
            output,
        } = self.inference.resolve(&value.ty)
        else {
            return Ok(());
        };
        if let Some(function) = self.models.functions_by_key.get(&*key).cloned() {
            // A function of no inputs stays itself, as `State::default`
            // passed to `iced::application`.
            if inputs.is_empty() && !nullary {
                return Ok(());
            }
            let symbolic = self.symbolic_function(&function, nullary, value.span)?;
            value.source = symbolic.source;
            value.prelude = symbolic.prelude;
            value.ty = symbolic.ty;
            return Ok(());
        }
        // An associated function named as a value (`associated_value`).
        if let Some(definition) = self
            .models
            .methods
            .values()
            .chain(self.models.trait_methods.values().flatten())
            .find(|definition| definition.receiver().is_none() && definition.key() == &*key)
            .cloned()
        {
            // So does an associated one, as a hand-written `Default`'s
            // `State::default`.
            if inputs.is_empty() && !nullary {
                return Ok(());
            }
            let symbolic = self.symbolic_associated(&definition, nullary, value.span)?;
            value.source = symbolic.source;
            value.prelude = symbolic.prelude;
            value.ty = symbolic.ty;
            return Ok(());
        }
        let Some(index) = self.lifted.iter().position(|closure| closure.key == *key) else {
            return Ok(());
        };
        if self.lifted[index].held {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a closure using a `&mut` borrow of a place declared outside it, \
                            passed to a native function: not supported in Rawr yet"
                    .into(),
                source: self.lifted[index].span,
            });
        }
        // The closure's type keeps its parameters' borrows, which the
        // runtime passes by value. Its signature names a shared borrow of a
        // script value only, as `|event: &Event|` makes a crate's `impl
        // Fn(T)` take `T = &Event`, which is `Clone` where `Event` isn't;
        // its proxy shares the value either way.
        let signature = Ty::Fn {
            inputs: inputs
                .iter()
                .map(|input| match input {
                    Ty::Reference {
                        mutable: false,
                        target,
                    } if !matches!(self.inference.resolve(target), Ty::Script(..)) => {
                        (**target).clone()
                    }
                    input => input.clone(),
                })
                .collect(),
            output: output.clone(),
        }
        .name(&self.models);
        // Each capture's name and type, read where the closure is written,
        // as its captures are.
        let closure = &self.lifted[index];
        let captured = closure.parameters[..closure.captures.len()]
            .iter()
            .map(|name| {
                let local = if name.as_str() == SELF_CAPTURE {
                    "self"
                } else {
                    name.as_str()
                };
                let ty = self
                    .bindings
                    .get(local)
                    .map_or_else(String::new, |binding| {
                        self.inference
                            .resolve(&binding.ty)
                            .name(&self.models)
                            .to_string()
                    });
                (name.as_str(), ty)
            })
            .collect::<Vec<_>>();
        let item = crate::item::Value::new(
            (*key).clone(),
            crate::item::Kind::Function,
            None,
            None,
            crate::signature::Fingerprint::closure(inputs.len(), &signature, &captured),
            capability::Fingerprint::default(),
        );
        // Its captures are read where the closure is written.
        if !closure.captures.is_empty()
            && !(value.span.start() <= closure.span.start()
                && closure.span.end() <= value.span.end())
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a capturing closure passed to native code from a variable is not supported in Rawr yet; write it where the call takes it".into(),
                source: value.span,
            });
        }
        let captures = closure.captures.clone();
        self.lifted[index].value = Some(item.clone());
        let symbolic = self.symbolic_expr(item, inputs, *output, value.span, captures);
        value.source = symbolic.source;
        // The closure's own prelude binds what it captures, as `f.with(x)`
        // binds `f` and `x`.
        let mut prelude = std::mem::take(&mut value.prelude);
        prelude.push(symbolic.prelude);
        value.prelude = prelude;
        value.ty = symbolic.ty;
        Ok(())
    }

    /// A `fn` pointer value passed to the host, as a style read from an
    /// array of them or a closure's `message: fn(f32) -> _`: the closure
    /// `move |a, ..| pointer(a, ..)`, lifted as a written closure is, which
    /// captures the pointer where it is passed. The host takes its
    /// generated function as it takes any closure's.
    pub(super) fn lift_fn_pointer(&mut self, value: &mut ParsedExpr) {
        let Ty::Fn { inputs, output } = self.inference.resolve(&value.ty) else {
            return;
        };
        // A `&mut` input a closure copies back after its call (`copied`)
        // has no place to go through a pointer's call.
        if inputs.iter().any(|input| {
            matches!(
                self.inference.resolve(input),
                Ty::Reference { mutable: true, .. }
            )
        }) {
            return;
        }
        let Some(callable) = &self.callable else {
            return;
        };
        self.closure_count += 1;
        let key = crate::item::Key::closure(callable, self.closure_count);
        let pointer = ImmutableString::from(self.temporary("pointer"));
        let arguments = (0..inputs.len())
            .map(|_| ImmutableString::from(self.temporary("eta")))
            .collect::<Vec<_>>();
        let mut body = origin::Fragment::plain(format!(
            "{{ {pointer}.__rawr_call({}) }}",
            arguments.join(", ")
        ));
        body.cover(value.span);
        let mut parameters = vec![pointer];
        parameters.extend(arguments);
        self.lifted.push(closure::Lifted {
            key: key.clone(),
            value: None,
            parameters,
            captures: vec![value.rendered_source()],
            body,
            span: value.span,
            header: value.span,
            held: false,
        });
        value.prelude = origin::Fragment::default();
        value.source = origin::Fragment::plain("()");
        value.source.cover(value.span);
        value.ty = Ty::FnItem {
            key: Box::new(key),
            inputs,
            output,
        };
        value.lvalue = None;
    }

    /// A named function used as a value: a function item that coerces to
    /// a `fn` pointer or a host callable.
    pub(super) fn function_value(&mut self, function: &ScriptFunction, span: Span) -> ParsedExpr {
        let substitution = self.fresh_generics(&function.definition.generics, span);
        let inputs: Box<[Ty]> = function
            .definition
            .parameters
            .iter()
            .map(|parameter| instantiate(&Ty::from_resolved(&parameter.ty), &substitution))
            .collect();
        let output = instantiate(
            &function
                .definition
                .output
                .as_ref()
                .map_or(Ty::Unit, Ty::from_resolved),
            &substitution,
        );
        let parameters = (0..inputs.len())
            .map(|index| format!("__rawr_argument_{index}"))
            .collect::<Vec<_>>();
        let witnesses = self
            .witness_arguments(&function.definition.generics, &substitution, span)
            .into_iter()
            .map(|witness| witness.as_str().to_string());
        let arguments = parameters
            .iter()
            .cloned()
            .chain(witnesses)
            .collect::<Vec<_>>()
            .join(", ");
        let parameters = parameters.join(", ");
        let source = origin::Fragment::plain(format!(
            "(|{parameters}| {}({arguments}))",
            function.generated
        ));
        self.operator_expr(
            source,
            Ty::FnItem {
                key: Box::new(function.key.clone()),
                inputs,
                output: Box::new(output),
            },
            span,
        )
    }

    /// A named function with a `&mut` parameter used as a value: the
    /// closure `|a, ..| name(a, ..)` over its parameter types, as a call of
    /// it copies the `&mut` arguments in and back out.
    pub(super) fn mutable_function_value(
        &mut self,
        function: &ScriptFunction,
        name: ImmutableString,
        span: Span,
    ) -> Result<ParsedExpr, Error> {
        let substitution = self.fresh_generics(&function.definition.generics, span);
        let inputs: Box<[Ty]> = function
            .definition
            .parameters
            .iter()
            .map(|parameter| instantiate(&Ty::from_resolved(&parameter.ty), &substitution))
            .collect();
        let at = span.start();
        let parameter =
            |index: usize| Token::Identifier(Box::new(format!("__rawr_argument_{index}").into()));
        let mut tokens = Vec::new();
        for index in 0..inputs.len() {
            if index > 0 {
                tokens.push((Token::Comma, at));
            }
            tokens.push((parameter(index), at));
        }
        tokens.push((Token::Pipe, at));
        tokens.push((Token::Identifier(Box::new(name.as_str().into())), at));
        tokens.push((Token::LeftParen, at));
        for index in 0..inputs.len() {
            if index > 0 {
                tokens.push((Token::Comma, at));
            }
            tokens.push((parameter(index), at));
        }
        tokens.push((Token::RightParen, at));
        self.splice_tokens(tokens);
        self.closure_hint = Some(inputs);
        self.parse_closure(Token::Pipe, span, false)
    }

    /// An enum variant with a payload used as a value: the function from
    /// its payload to the enum, as Rust's tuple-variant constructors are.
    pub(super) fn variant_function(
        &mut self,
        definition: &type_def::Definition,
        item: &item::Enum,
        variant: usize,
        expression: Span,
    ) -> ParsedExpr {
        let (enum_ty, substitution) = self.fresh_nominal(
            &Nominal::Enum {
                definition: definition.clone(),
                item: item.clone(),
            },
            expression,
        );
        let inputs: Box<[Ty]> = item.variants[variant]
            .payload
            .iter()
            .map(|payload| instantiate(&Ty::from_resolved(payload), &substitution))
            .collect();
        let helper = self.push_op(Op::Enum {
            definition: definition.clone(),
            item: item.clone(),
            variant,
        });
        let parameters = (0..inputs.len())
            .map(|index| format!("__rawr_argument_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = origin::Fragment::plain(format!("(|{parameters}| {helper}({parameters}))"));
        self.operator_expr(
            source,
            Ty::Fn {
                inputs,
                output: Box::new(enum_ty),
            },
            expression,
        )
    }

    /// The type a call of the engine's native function `name` returns: the
    /// Rust return type of each function of that name and arity registered
    /// with its signature (`Engine::register_fn`), when they agree on one
    /// that Rawr names.
    pub(super) fn native_output(&self, name: &str, arity: usize) -> Option<Ty> {
        use std::any::TypeId;
        let ty = |id: TypeId| -> Option<Ty> {
            Some(if id == TypeId::of::<crate::INT>() {
                Ty::Int(Int::I64)
            } else if id == TypeId::of::<crate::FLOAT>() {
                Ty::Float(Float::F64)
            } else if id == TypeId::of::<bool>() {
                Ty::Bool
            } else if id == TypeId::of::<char>() {
                Ty::Char
            } else if id == TypeId::of::<String>() || id == TypeId::of::<ImmutableString>() {
                Ty::String
            } else if id == TypeId::of::<()>() {
                Ty::Unit
            } else {
                return None;
            })
        };
        let mut found = None;
        for module in &self.native_modules {
            for (_, metadata) in module.iter_fn() {
                if metadata.name.as_str() != name || metadata.num_params != arity {
                    continue;
                }
                let output = ty(metadata.output_type?)?;
                if found.as_ref().is_some_and(|found| *found != output) {
                    return None;
                }
                found = Some(output);
            }
        }
        found
    }

    pub(super) fn parse_script_call(
        &mut self,
        mut name: ImmutableString,
        function: Option<ScriptFunction>,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // `drop(value)` moves the value out and drops it, as Rust's
        // `std::mem::drop` does. A tokio channel's sender without tokio is
        // also ended for each copy of it.
        if function.is_none()
            && name.as_str() == "drop"
            && matches!(self.peek(), Token::LeftParen)
            && self.unknown_function(&name, name_span).is_some()
        {
            let (mut arguments, end) = self.parse_call_arguments("call arguments")?;
            let call = span(name_span.start(), end);
            let [argument] = arguments.as_mut_slice() else {
                return Err(Error::argument_count(
                    Callee::Function,
                    name,
                    1,
                    arguments.len(),
                    call,
                ));
            };
            self.move_local(argument)?;
            // A value with drop glue runs it here.
            let source = if self.needs_drop(&argument.ty, argument.span)? {
                let shape = origin::Fragment::plain(self.drop_shape(&argument.ty));
                origin::Fragment::call(drops::GLUE, [argument.rendered_source(), shape])
            } else {
                origin::Fragment::call(crate::engine::KEYWORD_DROP, [argument.rendered_source()])
            };
            return Ok(self.operator_expr(source, Ty::Unit, call));
        }
        // A name that no function of the crate's has is an error at the
        // name, unless a function the engine registered serves it.
        if function.is_none()
            && let Some(error) = self.unknown_function(&name, name_span)
        {
            // A `use` of a crate module with a function of its name beside
            // it, as iced's `combo_box`, names the function rawr can't
            // call, and the import says why.
            let written = crate::tokenizer::raw_names(&name);
            let reference = Span::new(name_span.start(), text_end(name_span.start(), &written));
            return Err(self.module_value(&name, reference).unwrap_or(error));
        }
        // A non-generic function's parameter types are each argument's
        // expected type.
        if let Some(function) = &function
            && function.definition.generics.is_empty()
        {
            self.argument_hints = function
                .definition
                .parameters
                .iter()
                .map(|parameter| Some(Ty::from_resolved(&parameter.ty)))
                .collect();
        }
        // What the call's value keeps of its arguments (D572.4).
        let summary = function
            .as_ref()
            .map(|function| self.function_summary(&function.definition));
        let receiver = function
            .as_ref()
            .and_then(|function| this_parameter(&function.definition));
        let copied = function.as_ref().map_or_else(Vec::new, |function| {
            copied_parameters(
                &function.definition.parameters,
                receiver,
                function.definition.formatter.as_ref(),
            )
        });
        let borrow = std::mem::replace(
            &mut self.mutable_borrow,
            (receiver.is_some() || !copied.is_empty()).then(|| self.expression_stack.len() + 1),
        );
        // A generic function's parameter types, its parameters fresh, for
        // what each argument read tells the ones after it.
        if let Some(function) = &function
            && !function.definition.generics.is_empty()
        {
            self.generic_argument_hints = self.generic_parameter_hints(&function.definition);
        }
        let arguments = self.parse_call_arguments("function arguments");
        self.mutable_borrow = borrow;
        let (mut arguments, end) = arguments?;
        for argument in &arguments {
            self.reject_invocation(&argument.ty, argument.span, "passed to a function")?;
        }
        let mut witnesses = Vec::new();
        let output = if let Some(function) = function {
            if arguments.len() != function.definition.parameters.len() {
                return Err(Error::ArgumentCount {
                    callee: Callee::Function,
                    expected: function.definition.parameters.len(),
                    found: arguments.len(),
                    call: span(name_span.start(), end),
                    definition: (function.source == self.models.context)
                        .then_some(function.definition.name.span),
                    name: function.definition.name.text,
                });
            }
            // A generic function's parameters are fresh at each call, as
            // Rust infers them, and its bounds are checked once they are known.
            let call = span(name_span.start(), end);
            let substitution = self.instantiate_generics(&function.definition.generics, call)?;
            // A function's `&mut fmt::Formatter` takes the formatter of the
            // `fmt` body that calls it, whose text the call gives back.
            let formatter = function
                .definition
                .formatter
                .as_ref()
                .and_then(|formatter| {
                    function
                        .definition
                        .parameters
                        .iter()
                        .position(|parameter| parameter.name.text == *formatter)
                });
            for (index, (argument, parameter)) in arguments
                .iter_mut()
                .zip(function.definition.parameters.iter())
                .enumerate()
            {
                if Some(index) == formatter {
                    if self.formatter.as_ref().map(ImmutableString::as_str)
                        != Some(argument.source.as_str())
                    {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "a `fmt::Formatter` other than a `fmt` body's own".into(),
                            source: argument.span,
                        });
                    }
                    continue;
                }
                let expected = instantiate(&Ty::from_resolved(&parameter.ty), &substitution);
                self.coerce_argument(&parameter.ty, &expected, argument)?;
                self.convert_into_host(&function.definition.generics, &parameter.ty, argument)?;
                self.move_local(argument)?;
                self.lend_argument(&parameter.ty, &expected, argument);
            }
            // A generic function compiled per instantiation is its
            // instance for the types this call gives it (D629).
            #[cfg(feature = "rust-import")]
            let function = self.bind_instance(function, &substitution, call)?;
            let output = instantiate(
                &function
                    .definition
                    .output
                    .as_ref()
                    .map_or(Ty::Unit, Ty::from_resolved),
                &substitution,
            );
            witnesses = self.witness_arguments(&function.definition.generics, &substitution, call);
            let kept = self.inference.resolve(&output);
            if let Some(summary) = &summary {
                self.kept_by_call(call, &kept, None, summary);
            }
            name = function.generated;
            if function
                .definition
                .output
                .as_ref()
                .is_some_and(type_ref::Type::is_shared_call_fn)
            {
                self.shared_call_fns.insert(call);
            }
            let places =
                self.mutable_arguments(&arguments, receiver.into_iter().chain(copied.clone()))?;
            self.move_copied_in(&mut arguments, &copied);
            // `f(a, &mut v)` is `v.f(a)`: the place is the receiver.
            if let Some(index) = receiver
                && index < arguments.len()
            {
                // The place as `mutable_arguments` found it: `self` in a
                // `&mut self` method is the receiver's.
                arguments.remove(index);
                let place = places[0].clone();
                let operands: Vec<_> = arguments
                    .iter()
                    .map(ParsedExpr::rendered_source)
                    .chain(witnesses)
                    .collect();
                // `biggest(&mut v)` of `fn biggest(v: &mut Vec<i32>) ->
                // &mut i32`: the place below `v` the output borrows.
                if let Some(borrowed) = self.place_output(&output) {
                    let receiver = self.borrowed_receiver(&place, call)?;
                    let mut source =
                        origin::Fragment::plain(format!("{}.{name}(", places::RECEIVER_PLACE));
                    source.push(origin::Fragment::join(operands, ", "));
                    source.push_text(")");
                    let source = self.copied_call(source, &places[1..])?;
                    return Ok(self.borrowed_call(&receiver, source, borrowed, call));
                }
                // A nested place copied in and back beside it is stored
                // after it, so the receiver is read and stored around the
                // call (`two(&mut s.p.x, &mut s.p.y)`).
                let source = match &place.lvalue {
                    Some(Lvalue::Nested(nested)) if places.len() > 1 => {
                        let nested = (**nested).clone();
                        self.require_mutable_binding(&nested.binding, place.span)?;
                        let (read, at, store) = self.nested_parts(&nested);
                        let result = self.temporary("result");
                        let mut source = origin::Fragment::plain("({ ");
                        source.push(read);
                        source.push_text(format!("let {result} = "));
                        source.push(at);
                        source.push_text(format!(".{name}("));
                        source.push(origin::Fragment::join(operands, ", "));
                        source.push_text("); ");
                        source.push(store);
                        source.push_text(format!("{result} }})"));
                        source
                    }
                    // A closure written in place (`&mut |x| ..`) is called
                    // where it is.
                    None => {
                        let mut source = place.rendered_source();
                        source.push_text(format!(".{name}("));
                        source.push(origin::Fragment::join(operands, ", "));
                        source.push_text(")");
                        source
                    }
                    _ => self.mutation_call(&place, &name, operands)?,
                };
                let source = self.copied_call(source, &places[1..])?;
                return Ok(self.operator_expr(source, output, call));
            }
            if !places.is_empty() {
                let source = origin::Fragment::call(
                    name.as_str(),
                    arguments
                        .iter()
                        .map(ParsedExpr::rendered_source)
                        .chain(witnesses),
                );
                let source = self.copied_call(source, &places)?;
                return Ok(self.operator_expr(source, output, call));
            }
            if let Some(formatter) = formatter.and_then(|index| arguments.get(index)) {
                let mut source = origin::Fragment::plain("{ ");
                source.push(formatter.rendered_source());
                source.push_text(" = ");
                source.push(origin::Fragment::call(
                    name.as_str(),
                    arguments
                        .iter()
                        .map(ParsedExpr::rendered_source)
                        .chain(witnesses),
                ));
                source.push_text("; ");
                source.push(self.prelude_op(PreludeOp::Ok, ["()".into()]));
                source.push_text(" }");
                return Ok(self.operator_expr(source, output, call));
            }
            output
        } else {
            let output = self
                .native_output(&name, arguments.len())
                .unwrap_or(Ty::Unknown);
            let kept = self.inference.resolve(&output);
            let summary = crate::resolve::borrowck::signature::Summary::unknown();
            self.kept_by_call(span(name_span.start(), end), &kept, None, &summary);
            output
        };
        let source = origin::Fragment::call(
            name.as_str(),
            arguments
                .iter()
                .map(ParsedExpr::rendered_source)
                .chain(witnesses),
        );
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: output,
            span: span(name_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// The arguments of a call of std's method `member` on the native
    /// `receiver`, each closure among them with the parameter types std's
    /// declaration gives it (#795), whether or not rawr runs the method:
    /// `v.dedup_by_key(|x| *x / 2)` reads `x` as `&mut T`, and fails with the
    /// method's not-yet error, not inside the closure.
    pub(super) fn parse_std_arguments(
        &mut self,
        receiver: &Ty,
        member: &str,
        context: &str,
    ) -> Result<(Vec<ParsedExpr>, Position), Error> {
        let receiver = self.inference.resolve(receiver);
        let declared = std_method::arguments(&receiver, member, &mut || self.inference.fresh());
        self.closure_hints = declared.closures;
        self.generic_argument_hints = declared.generics;
        let arguments = self.parse_call_arguments(context);
        self.closure_hints = Vec::new();
        self.generic_argument_hints = Vec::new();
        arguments
    }

    pub(super) fn parse_call_arguments(
        &mut self,
        context: &str,
    ) -> Result<(Vec<ParsedExpr>, Position), Error> {
        let mut expected = std::mem::take(&mut self.argument_hints).into_iter();
        let mut generic = std::mem::take(&mut self.generic_argument_hints).into_iter();
        let deferred = self.deferred_hints.take();
        let script_callables = std::mem::take(&mut self.script_callables);
        if let Some(pending) = self.pending_arguments.take() {
            return Ok(pending);
        }
        if matches!(self.peek(), Token::Unit) {
            let end = self.next().1;
            self.flow().called(end);
            return Ok((Vec::new(), end));
        }

        self.expect(Token::LeftParen, "'(' to start call arguments")?;
        let mut arguments = Vec::new();
        let mut hints = std::mem::take(&mut self.closure_hints).into_iter();
        let mut outputs = std::mem::take(&mut self.closure_outputs).into_iter();
        while !matches!(self.peek(), Token::RightParen) {
            if self.element_left_out()? {
                continue;
            }
            let outer = std::mem::replace(&mut self.no_struct, false);
            self.closure_hint = hints.next().flatten();
            if self.closure_hint.is_none()
                && let Some((items, receiver)) = &deferred
            {
                self.closure_hint =
                    self.overload_closure_hint(items, receiver.as_ref(), &arguments);
            }
            self.closure_output = outputs.next().flatten();
            let expected = expected.next().flatten();
            let generic = generic.next();
            // A closure where a crate function takes `impl Fn(f32) -> C`
            // has those parameter types, as in Rust.
            if self.closure_hint.is_none()
                && let Some(Ty::Fn { inputs, .. }) = expected
                    .as_ref()
                    .or(generic.as_ref())
                    .map(|expected| self.inference.resolve(expected))
                && !inputs.iter().any(contains_inference)
            {
                self.closure_hint = Some(inputs);
            }
            self.script_callable = script_callables;
            let argument = self.parse_expr_expecting(expected.as_ref());
            // An argument of a generic function fixes the parameters its
            // type gives, unless it is one a coercion takes, which the call
            // checks once all are read.
            if let (Ok(argument), Some(generic)) = (&argument, &generic) {
                let checkpoint = self.inference.checkpoint();
                if self.unify_type(generic, &argument.ty) {
                    self.inference.commit(checkpoint);
                } else {
                    self.inference.rollback(checkpoint);
                }
            }
            self.script_callable = false;
            self.closure_hint = None;
            self.closure_output = None;
            self.no_struct = outer;
            arguments.push(argument?);
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightParen) {
                return Err(self.unsupported(context));
            }
        }
        let end = self.expect(Token::RightParen, "')' after call arguments")?;
        let operands: Vec<_> = arguments
            .iter()
            .map(|argument| self.argument_fact(argument))
            .collect();
        self.flow().call_operands(&operands, end);
        Ok((arguments, end))
    }

    pub(super) fn require_arguments(
        &mut self,
        definition: &method::Definition,
        arguments: &mut [ParsedExpr],
        call: Span,
        substitution: &BTreeMap<ImmutableString, Ty>,
    ) -> Result<(), Error> {
        if arguments.len() != definition.parameters().len() {
            return Err(Error::ArgumentCount {
                callee: if definition.receiver().is_some() {
                    Callee::Method
                } else {
                    Callee::AssociatedFunction
                },
                name: crate::resolve::Qualified(definition.owner_name(), definition.name())
                    .to_string()
                    .into(),
                expected: definition.parameters().len(),
                found: arguments.len(),
                call,
                definition: (definition.source() == self.models.context)
                    .then_some(definition.name_span()),
            });
        }
        for (argument, parameter) in arguments.iter_mut().zip(definition.parameters()) {
            let expected = instantiate(&Ty::from_resolved(&parameter.ty), substitution);
            self.coerce_argument(&parameter.ty, &expected, argument)?;
            self.move_local(argument)?;
        }
        Ok(())
    }

    /// Each argument of a script method's `&` parameter lent where it is
    /// (`lend_argument`), unless it is the local `receiver_root` the call's
    /// receiver is in, which the call holds while it runs.
    pub(super) fn lend_arguments(
        &self,
        definition: &method::Definition,
        arguments: &mut [ParsedExpr],
        substitution: &BTreeMap<ImmutableString, Ty>,
        receiver_root: Option<&str>,
    ) {
        for (argument, parameter) in arguments.iter_mut().zip(definition.parameters()) {
            if receiver_root != Some(argument.source.as_str()) {
                let expected = instantiate(&Ty::from_resolved(&parameter.ty), substitution);
                self.lend_argument(&parameter.ty, &expected, argument);
            }
        }
    }
}
