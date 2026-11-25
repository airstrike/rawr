//! Calls and their arguments: a script function's call, a call through a
//! local or a function value, function items as values, and `&mut`
//! arguments copied in and back out.

use super::*;

/// How a call of a function value borrows it: shared, as `Fn::call` and a
/// `fn` pointer's call do, or `&mut`, as `FnMut::call_mut` does (T179,
/// T188).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Borrow {
    Shared,
    Mutable,
}

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
                construct: crate::resolve::Unsupported::token("'(' to start call arguments", open),
                source: point_span(position),
            });
        }
        self.unnoted(Self::take_until_close)
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

    /// Whether `name` is a crate's trait a `use` names, which Rawr names as
    /// a module of its functions, as `use iced::widget::Widget;` does.
    pub(super) fn imported_trait(&self, name: &str) -> bool {
        self.models.import(name).is_some_and(|binding| {
            matches!(binding.target, item::Import::Module)
                && self.models.host_items(&binding.qualified).is_none()
                && binding
                    .qualified
                    .rsplit("::")
                    .next()
                    .and_then(|last| last.chars().next())
                    .is_some_and(char::is_uppercase)
        })
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
        // `Into::<T>::into(x)` and kin are a trait's turbofish, as is
        // `Counted::<u16>::doubled(x)` of a crate's trait (#1282).
        if self.models.traits.contains_key(last.as_str())
            || segments == 0 && self.imported_trait(name)
            || self.models.get(&last).is_none()
                && crate::resolve::std_trait::anywhere(&last).is_some()
        {
            return Ok(None);
        }
        let mut depth = 1usize;
        let mut arguments = 0;
        // Inside a const argument's braces (`Buf::<{ 0 + 1 }>`), `<`
        // and `>` compare and shift.
        let mut braces = 0usize;
        while depth > 0 {
            let closing = match ahead.next() {
                Some((Token::LeftBrace, _)) => {
                    braces += 1;
                    0
                }
                Some((Token::RightBrace, _)) if braces > 0 => {
                    braces -= 1;
                    0
                }
                Some((Token::EOF, _)) | None => return Ok(None),
                Some(_) if braces > 0 => 0,
                Some((Token::SemiColon, _)) => return Ok(None),
                Some((Token::LessThan, _)) => {
                    depth += 1;
                    0
                }
                Some((Token::GreaterThan, _)) => 1,
                Some((Token::RightShift, _)) => 2,
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
        // A head nothing in scope names heads the path: rustc's E0433 over
        // the name, as without the turbofish.
        self.parse_local_type()
            .map(Some)
            .map_err(|error| match error {
                Error::UnknownType {
                    name: missing,
                    imports,
                    ..
                } if segments == 0 && missing == *name => Error::UnknownType {
                    reference: Span::new(name_span.start(), text_end(name_span.start(), name)),
                    name: missing,
                    path: true,
                    imports,
                },
                error => error,
            })
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
        // The stores and the value after the call are the compiler's, at
        // the arguments: no debugger stops there again once the call ran.
        let mut stores = origin::Fragment::default();
        for (index, place) in places.iter().enumerate() {
            let value = format!("{moved}({returned}[{}])", index + 1);
            stores.push(self.store_into(place, &value)?);
            stores.push_text(" ");
        }
        stores.push_text(format!("{moved}({returned}[0])"));
        stores.mark_written();
        if let (Some(first), Some(last)) = (places.first(), places.last()) {
            stores.cover_written(span(first.span.start(), last.span.end()));
        }
        source.push(stores);
        source.push_text(" })");
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
        if let Some(Lvalue::Slice(slice)) = &place.lvalue
            && let Some(single) = &slice.single
        {
            let mut element = place.clone();
            element.lvalue = Some((**single).clone());
            return self.store_into(&element, &format!("{value}[0]"));
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

    /// The place `value` is when it is `self` in a `&mut self` method:
    /// the receiver's binding, whose fields are places too.
    pub(super) fn mutable_receiver_place(&self, value: &ParsedExpr) -> Option<Lvalue> {
        let receiver = value.lvalue.is_none()
            && self.closures.is_empty()
            && value.source.as_str() == "self"
            && self.bindings.get("self").is_some_and(|binding| {
                binding.mutable && matches!(binding.kind, BindingKind::Receiver { .. })
            });
        receiver.then(|| Lvalue::Binding {
            binding: "self".into(),
            ty: value.ty.clone(),
        })
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
            let mut place = place.clone();
            if place.lvalue.is_none() {
                place.lvalue = self.mutable_receiver_place(&place);
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

    /// The value spanning `value` is of the declared type `ty`: a function
    /// whose call borrows it shared (`impl Fn`, `fn`), or a boxed one whose
    /// call borrows it `&mut` (`Box<dyn FnMut>`), as a local given it is
    /// called (T179, T188).
    pub(super) fn note_fn_value(&mut self, ty: &type_ref::Type, value: Span) {
        if ty.is_shared_call_fn() {
            self.shared_call_fns.insert(value);
        } else if ty.is_boxed_mutable_call_fn() {
            self.mutable_call_fns.insert(value);
        }
    }

    /// The value spanning `whole` is one of `branches`, by their spans and
    /// types (an `if`'s or a `match`'s): a function a call borrows shared
    /// when each branch that gives a value is one (T188).
    pub(super) fn note_fn_branches(&mut self, whole: Span, branches: &[(Span, &Ty)]) {
        let mut valued = branches
            .iter()
            .filter(|(_, ty)| self.inference.resolve(ty) != Ty::Never)
            .peekable();
        if valued.peek().is_some()
            && valued.all(|(branch, _)| self.shared_call_fns.contains(branch))
        {
            self.shared_call_fns.insert(whole);
        }
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
            // A `Box<dyn FnMut(i32)>` is called through the box, which a
            // call borrows `&mut` (T188).
            Ty::Boxed(target) if matches!(self.inference.resolve(&target), Ty::Fn { .. }) => {
                binding.mutable
                    && matches!(binding.kind, BindingKind::Local)
                    && self.mutable_call_fns.contains(&binding.declaration)
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
            && self.once_closures.contains_key(&**key)
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
        // `let f = styles::warning; f(&theme)`: a crate function's item
        // names the one function its call reaches, as rustc's type of it
        // does (#874).
        #[cfg(feature = "rust-import")]
        if let Ty::ImportedFunctionItem {
            item: Some(proof), ..
        } = self.inference.resolve(&ty)
        {
            let ImportedFunctionProof { item, owner, .. } = *proof;
            let (arguments, end) = self.parse_call_arguments("function arguments")?;
            let mut inputs = Vec::with_capacity(arguments.len());
            for mut argument in arguments {
                self.move_local(&mut argument)?;
                inputs.push(argument);
            }
            let call = span(name_span.start(), end);
            return self.call_host_function(&[item], Some(owner), inputs, call);
        }
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
        let closure_key = match &ty {
            Ty::FnItem { key, .. } => Some((**key).clone()),
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
                    self.models
                        .in_this_file(function.source)
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
            let parameters = function
                .as_ref()
                .map_or(&[][..], |function| &function.parameters[..]);
            let places = self.argument_places(
                name_span,
                &inputs,
                parameters,
                definition.is_some(),
                &arguments,
            );
            return Err(Error::ArgumentCount {
                callee: callee_kind,
                name,
                expected: inputs.len(),
                found: arguments.len(),
                call,
                definition,
                arguments: Some(places),
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
        if let Some(key) = &closure_key {
            let spans: Vec<Span> = arguments.iter().map(|argument| argument.span).collect();
            self.flow().give(key, &spans);
        }
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

    /// `&mut f` or `&f` of a named function or closure where the host takes
    /// a borrow of a callable, as `operation.traverse(&mut |operation| ..)`
    /// gives iced's `&mut dyn FnMut(&mut dyn Operation)`: the host callable
    /// it is, borrowed by the binding for the call alone.
    pub(super) fn borrowed_host_callable(
        &mut self,
        value: &mut ParsedExpr,
        nullary: bool,
    ) -> Result<(), Error> {
        let Ty::Reference { mutable, target } = self.inference.resolve(&value.ty) else {
            return Ok(());
        };
        if !matches!(self.inference.resolve(&target), Ty::FnItem { .. }) {
            return Ok(());
        }
        let mut item = value.clone();
        item.ty = *target;
        self.host_callable_of(&mut item, nullary)?;
        if !matches!(item.ty, Ty::Callable { .. }) {
            return Ok(());
        }
        value.source = item.source;
        value.prelude = item.prelude;
        value.ty = Ty::Reference {
            mutable,
            target: Box::new(item.ty),
        };
        value.lvalue = None;
        Ok(())
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
                construct: crate::resolve::Unsupported::said(
                    "a capturing closure passed to native code from a variable is not supported \
                     in Rawr yet",
                )
                .with_help("write it where the call takes it"),
                source: value.span,
            });
        }
        let captures = closure.captures.clone();
        let held = closure.held.clone();
        self.lifted[index].value = Some(item.clone());
        let mut symbolic = self.symbolic_expr(item, inputs, *output, value.span, captures);
        // The places its body reaches are taken first, and their handles
        // are among its captures (`places::held`).
        if !held.as_str().is_empty() {
            let mut taken = origin::Fragment::plain("{ ");
            taken.push(held);
            taken.push(symbolic.source);
            taken.push_text(" }");
            symbolic.source = taken;
        }
        // A block's tail closure captures what the block's statements bind,
        // as `{ let todo = Rc::clone(&todo); move |key| .. }`: they run
        // first, in the block (#1450).
        if let Some(statements) = self.closure_blocks.get(&value.span) {
            let mut block = origin::Fragment::plain("{\n");
            block.push(statements.clone());
            block.push(std::mem::take(&mut symbolic.prelude));
            block.push(symbolic.source);
            block.push_text("\n}");
            symbolic.source = block;
        }
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
            held: origin::Fragment::default(),
            frames: crate::eval::machine::Frames::Resumed,
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
            item: Box::new(item.clone()),
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
            let guard = self.dropped_cell_guard();
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
            // A borrow's guard drops here, and its borrow ends with it.
            if let Some(source) =
                guard.and_then(|guard| self.drop_cell_guard(&guard, argument.span))
            {
                return Ok(self.operator_expr(source, Ty::Unit, call));
            }
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
        // The type the call is expected to have, which a generic function's
        // output gives its arguments.
        let expectation = self.coercion_target();
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
        let copied = function
            .as_ref()
            .map_or_else(Vec::new, |function| function_copied(&function.definition));
        let keeps = function
            .as_ref()
            .map_or_else(Vec::new, |function| function_kept(&function.definition));
        let lent = function.as_ref().map_or_else(Vec::new, |function| {
            lent_parameters(
                &function.definition.parameters,
                receiver,
                function.definition.formatter.as_ref(),
                function.definition.body.asynchronous,
            )
        });
        let borrow = std::mem::replace(
            &mut self.mutable_borrow,
            (receiver.is_some() || !copied.is_empty() || !lent.is_empty() || !keeps.is_empty())
                .then(|| self.expression_stack.len() + 1),
        );
        // A generic function's parameter types, its parameters fresh, for
        // what each argument read tells the ones after it, and the types the
        // call's expectation gives them.
        let mut expected_inputs = Vec::new();
        if let Some(function) = &function
            && !function.definition.generics.is_empty()
        {
            if let Some(expected) = expectation {
                expected_inputs = self.expected_inputs(&function.definition, &expected);
                self.argument_hints.clone_from(&expected_inputs);
            }
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
                let declared = self.models.in_this_file(function.source);
                let inputs: Vec<Ty> = function
                    .definition
                    .parameters
                    .iter()
                    .map(|parameter| Ty::from_resolved(&parameter.ty))
                    .collect();
                let places = self.argument_places(
                    name_span,
                    &inputs,
                    &function.definition.parameters,
                    declared,
                    &arguments,
                );
                return Err(Error::ArgumentCount {
                    callee: Callee::Function,
                    expected: function.definition.parameters.len(),
                    found: arguments.len(),
                    call: span(name_span.start(), end),
                    definition: declared.then_some(function.definition.name.span),
                    name: function.definition.name.text,
                    arguments: Some(places),
                });
            }
            // A generic function's parameters are fresh at each call, as
            // Rust infers them, and its bounds are checked once they are known.
            let call = span(name_span.start(), end);
            let pending = (self.pending_bounds.len(), self.pending_traits.len());
            let substitution = self.instantiate_generics(&function.definition.generics, call)?;
            let callee = self.callee_path(call, &function.definition.name.text);
            self.place_bounds(
                pending,
                &function.definition,
                callee,
                &arguments,
                &substitution,
            );
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
                let called = Called {
                    function: &function.definition,
                    parameter,
                    callee,
                    call: {
                        let mut after = end;
                        after.advance();
                        Span::new(call.start(), after)
                    },
                    declared: self.models.in_this_file(function.source),
                };
                // rustc coerces the argument to the type the expectation
                // gives it, a sized one (`rvalue_hint`), and reports it there.
                // An `impl IntoIterator` parameter takes an iterable that
                // isn't an iterator yet as the iterator over it
                // (`coerce_argument`), as `[a, b]` is.
                let iterable = parameter.ty.iterator_item().is_some()
                    && !matches!(self.inference.resolve(&argument.ty), Ty::Iter(..));
                if !iterable
                    && let Some(Some(input)) = expected_inputs.get(index)
                    && let Some(mismatch) = self.coerce_expected_input(input, &expected, argument)
                    && !unsized_hint(&self.inference.resolve(input))
                    && !shared_generic(&function.definition.parameters, index)
                    && let Some(error) = called.mismatch(&mismatch, argument.span)
                {
                    return Err(error);
                }
                // A parameter of a type the function names no generic in.
                let fixed = !function
                    .definition
                    .generics
                    .iter()
                    .any(|generic| mentions(&parameter.ty, &generic.name.text));
                if let Err(error) = self.coerce_argument(&parameter.ty, &expected, argument) {
                    return Err(fixed
                        .then(|| called.mismatch(&error, argument.span))
                        .flatten()
                        .unwrap_or(error));
                }
                self.closure_kind_needed(&parameter.ty, argument)?;
                self.convert_into_host(&function.definition.generics, &parameter.ty, argument)?;
                self.store_argument(&parameter.ty, argument)?;
                self.move_local(argument)?;
                self.move_generic_reference(argument, &parameter.ty);
                if function.source == self.models.context {
                    let owning = OwningParameter {
                        function: function.definition.name.text.clone(),
                        kind: "function",
                        name: function.definition.name.span,
                        ty: parameter.ty.span(),
                    };
                    self.note_owning_parameter(argument, &parameter.ty, owning);
                }
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
            #[cfg(feature = "rust-import")]
            let output = self.note_opaque_value(
                output,
                function
                    .definition
                    .lifetimes
                    .written_output
                    .as_deref()
                    .or(function.definition.output.as_ref()),
                &substitution,
                &function.key,
            );
            #[cfg(feature = "rust-import")]
            self.note_opaque_bound(
                function.definition.lifetimes.written_output.as_deref(),
                call,
                &output,
            );
            witnesses = self.witness_arguments(&function.definition.generics, &substitution, call);
            let kept = self.inference.resolve(&output);
            if let Some(summary) = &summary {
                self.kept_by_call(call, &kept, None, summary);
            }
            name = self
                .template_call(&function.key, &substitution, call, false)
                .unwrap_or(function.generated);
            if let Some(output) = &function.definition.output {
                self.note_fn_value(output, call);
            }
            let places =
                self.mutable_arguments(&arguments, receiver.into_iter().chain(copied.clone()))?;
            // The lent places are the caller's own: nothing is copied back.
            self.mutable_arguments(&arguments, lent.iter().chain(&keeps).copied())?;
            self.move_copied_in(&mut arguments, &copied);
            self.lend_mutable(&mut arguments, &lent)?;
            self.lend_kept(&mut arguments, &keeps)?;
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
            // A function's formatter takes the body's spec, as std passes
            // its `Formatter` on, and what it writes is the body's.
            if let Some(index) = formatter.filter(|index| *index < arguments.len()) {
                let mut source = origin::Fragment::plain("{ ");
                source.push(arguments[index].rendered_source());
                source.push_text(" += ");
                source.push(origin::Fragment::call(
                    name.as_str(),
                    arguments
                        .iter()
                        .enumerate()
                        .map(|(at, argument)| {
                            if at == index {
                                format::FORMAT_SPEC.into()
                            } else {
                                argument.rendered_source()
                            }
                        })
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
        self.closure_returns = declared.returns;
        self.generic_argument_hints = declared.generics;
        let arguments = self.parse_call_arguments(context);
        self.closure_hints = Vec::new();
        self.closure_returns = Vec::new();
        self.generic_argument_hints = Vec::new();
        arguments
    }

    /// The `)` that closes a call's arguments after the last is read, past
    /// the trailing comma Rust allows there (`v.get(\n index + 1,\n)`).
    pub(super) fn close_arguments(&mut self, expected: &str) -> Result<Position, Error> {
        if matches!(self.peek(), Token::Comma) && matches!(self.peek_second(), Token::RightParen) {
            self.next();
        }
        self.expect(Token::RightParen, expected)
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
        let mut returns = std::mem::take(&mut self.closure_returns).into_iter();
        // Closures whose parameter types a later argument fixes, read after
        // the others, as rustc checks closure arguments last.
        let mut postponed = Vec::new();
        while !matches!(self.peek(), Token::RightParen) {
            if self.element_left_out()? {
                continue;
            }
            let mut argument = Argument {
                index: arguments.len() + postponed.len(),
                tokens: Vec::new(),
                hint: hints.next().flatten(),
                output: outputs.next().flatten(),
                returns: returns.next().flatten(),
                expected: expected.next().flatten(),
                generic: generic.next(),
            };
            if argument.hint.is_none()
                && let Some((items, receiver)) = &deferred
            {
                argument.hint = self.overload_closure_hint(items, receiver.as_ref(), &arguments);
            }
            if argument.hint.is_none()
                && argument.expected.is_none()
                && let Some(Ty::Fn { inputs, .. }) = argument
                    .generic
                    .as_ref()
                    .map(|generic| self.inference.resolve(generic))
                && inputs.iter().any(contains_inference)
                && let Some(tokens) = self.take_closure_argument()
            {
                argument.tokens = tokens;
                postponed.push(argument);
                continue;
            }
            let parsed = self.parse_argument(argument, script_callables)?;
            arguments.push(parsed);
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightParen) {
                return Err(self.unsupported(context));
            }
        }
        for mut argument in postponed {
            self.splice_tokens(std::mem::take(&mut argument.tokens));
            let index = argument.index;
            let parsed = self.parse_argument(argument, script_callables)?;
            arguments.insert(index, parsed);
        }
        let end = self.expect(Token::RightParen, "')' after call arguments")?;
        let operands: Vec<_> = arguments
            .iter()
            .map(|argument| self.argument_fact(argument))
            .collect();
        self.flow().call_operands(&operands, end);
        Ok((arguments, end))
    }

    /// One call argument, read with what the call expects of it.
    fn parse_argument(
        &mut self,
        argument: Argument,
        script_callables: bool,
    ) -> Result<ParsedExpr, Error> {
        let outer = std::mem::replace(&mut self.no_struct, false);
        self.closure_hint = argument.hint;
        self.closure_output = argument.output;
        self.closure_return = argument.returns;
        // A closure where a crate function takes `impl Fn(f32) -> C`
        // has those parameter types, as in Rust.
        if self.closure_hint.is_none()
            && let Some(Ty::Fn { inputs, .. }) = argument
                .expected
                .as_ref()
                .or(argument.generic.as_ref())
                .map(|expected| self.inference.resolve(expected))
            && !inputs.iter().any(contains_inference)
        {
            self.closure_hint = Some(inputs);
        }
        self.script_callable = script_callables;
        let parsed = self.parse_expr_expecting(argument.expected.as_ref());
        // An argument of a generic function fixes the parameters its
        // type gives, unless it is one a coercion takes, which the call
        // checks once all are read.
        if let (Ok(parsed), Some(generic)) = (&parsed, &argument.generic) {
            let checkpoint = self.inference.checkpoint();
            if self.unify_type(generic, &parsed.ty) {
                self.inference.commit(checkpoint);
            } else {
                self.inference.rollback(checkpoint);
            }
        }
        self.script_callable = false;
        self.closure_hint = None;
        self.closure_output = None;
        self.closure_return = None;
        self.no_struct = outer;
        parsed
    }

    /// The tokens of the closure the next argument is, with the `,` after
    /// it consumed, when another argument follows it; `None`, consuming
    /// nothing, for any other argument.
    fn take_closure_argument(&mut self) -> Option<Vec<(Token, Position)>> {
        let mut ahead = self.input.clone().map(|(token, _)| token).peekable();
        let mut length = 1_usize;
        let mut head = ahead.next()?;
        if matches!(&head, Token::Identifier(name) if name.as_str() == "move") {
            head = ahead.next()?;
            length += 1;
        }
        // The parameters: up to the `|` that closes them.
        match head {
            Token::Or => {}
            Token::Pipe => {
                let mut depth = 0_usize;
                loop {
                    length += 1;
                    match ahead.next()? {
                        Token::Pipe if depth == 0 => break,
                        Token::LeftParen | Token::LeftBracket | Token::LessThan => depth += 1,
                        Token::RightParen | Token::RightBracket | Token::GreaterThan => {
                            depth = depth.checked_sub(1)?;
                        }
                        Token::RightShift => depth = depth.checked_sub(2)?,
                        Token::EOF => return None,
                        _ => {}
                    }
                }
            }
            _ => return None,
        }
        // The body: up to the `,` after it, past any a bracket or a
        // turbofish holds.
        let mut depth = 0_usize;
        let mut angles = 0_usize;
        let mut previous = Token::EOF;
        loop {
            let token = ahead.next()?;
            match &token {
                Token::Comma if depth == 0 && angles == 0 => break,
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen if depth == 0 => return None,
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.checked_sub(1)?;
                }
                Token::LessThan if angles > 0 || previous == Token::DoubleColon => angles += 1,
                Token::GreaterThan if angles > 0 => angles -= 1,
                Token::RightShift if angles > 0 => angles = angles.saturating_sub(2),
                Token::EOF => return None,
                _ => {}
            }
            length += 1;
            previous = token;
        }
        if matches!(ahead.peek(), Some(Token::RightParen | Token::EOF) | None) {
            return None;
        }
        let tokens = (0..length).map(|_| self.next()).collect();
        // The `,` after it.
        self.next();
        Some(tokens)
    }

    pub(super) fn require_arguments(
        &mut self,
        definition: &method::Definition,
        arguments: &mut [ParsedExpr],
        call: Span,
        substitution: &BTreeMap<ImmutableString, Ty>,
    ) -> Result<(), Error> {
        if arguments.len() != definition.parameters().len() {
            let declared = self.models.in_this_file(definition.source());
            let inputs: Vec<Ty> = definition
                .parameters()
                .iter()
                .map(|parameter| instantiate(&Ty::from_resolved(&parameter.ty), substitution))
                .collect();
            let callee = self.callee_path(call, definition.name());
            let places = self.argument_places(
                callee,
                &inputs,
                definition.parameters(),
                declared,
                arguments,
            );
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
                definition: declared.then_some(definition.name_span()),
                arguments: Some(places),
            });
        }
        for (argument, parameter) in arguments.iter_mut().zip(definition.parameters()) {
            let expected = instantiate(&Ty::from_resolved(&parameter.ty), substitution);
            self.coerce_argument(&parameter.ty, &expected, argument)?;
            self.closure_kind_needed(&parameter.ty, argument)?;
            self.store_argument(&parameter.ty, argument)?;
            self.move_local(argument)?;
            self.move_generic_reference(argument, &parameter.ty);
            if definition.source() == self.models.context {
                let owning = OwningParameter {
                    function: definition.name().into(),
                    kind: if definition.receiver().is_some() {
                        "method"
                    } else {
                        "function"
                    },
                    name: definition.name_span(),
                    ty: parameter.ty.span(),
                };
                self.note_owning_parameter(argument, &parameter.ty, owning);
            }
        }
        Ok(())
    }

    /// The places rustc's E0061 labels at a call, written `callee`, of a
    /// callee taking `inputs` with `arguments`: the arguments past the
    /// inputs, or the inputs the call gives nothing. Each argument fills
    /// the next input whose type it fits, as rustc matches them; where
    /// that leaves another count, the arguments past the inputs are the
    /// extra ones, or the inputs past the arguments the missing ones.
    /// `parameters` names the inputs, and gives each one's place where
    /// the program declares it in this file (`declared`).
    pub(super) fn argument_places(
        &mut self,
        callee: Span,
        inputs: &[Ty],
        parameters: &[item::Parameter],
        declared: bool,
        arguments: &[ParsedExpr],
    ) -> Box<crate::resolve::Arguments> {
        let (expected, found) = (inputs.len(), arguments.len());
        let mut unexpected = Vec::new();
        let mut missing = Vec::new();
        if found > expected {
            let mut next = 0;
            for (index, argument) in arguments.iter().enumerate() {
                if next < expected && self.unifies(&inputs[next], &argument.ty) {
                    next += 1;
                } else {
                    unexpected.push(index);
                }
            }
            if unexpected.len() != found - expected {
                unexpected = (expected..found).collect();
            }
        } else {
            let mut next = 0;
            for (index, input) in inputs.iter().enumerate() {
                if next < found && self.unifies(input, &arguments[next].ty) {
                    next += 1;
                } else {
                    missing.push(index);
                }
            }
            if missing.len() != expected - found {
                missing = (found..expected).collect();
            }
        }
        let unexpected = unexpected
            .into_iter()
            .map(|index| crate::resolve::Unexpected {
                index,
                ty: self.argument_type(&arguments[index]),
            })
            .collect();
        let missing = missing
            .into_iter()
            .map(|index| {
                let parameter = parameters.get(index);
                let ty = self.inference.resolve(&inputs[index]);
                let open = matches!(ty, Ty::Infer(_) | Ty::Param(..) | Ty::Unknown);
                // rustc writes a type it knows whole, and the parameter's
                // name for one it doesn't.
                let placeholder = if ty == Ty::Unit {
                    "()".into()
                } else if open || contains_inference(&ty) || contains_open_literal(&ty) {
                    let name = parameter
                        .filter(|parameter| parameter.pattern.is_none())
                        .map_or("value", |parameter| parameter.name.text.as_str());
                    format!("/* {name} */")
                } else {
                    format!("/* {} */", ty.name(&self.models))
                };
                crate::resolve::Missing {
                    index,
                    ty: (!open).then(|| ty.name(&self.models)),
                    placeholder: placeholder.into(),
                    declared: parameter
                        .filter(|_| declared)
                        .map(|parameter| parameter.span),
                }
            })
            .collect();
        Box::new(crate::resolve::Arguments {
            callee,
            given: arguments
                .iter()
                .map(|argument| self.whole_expression(argument.span))
                .collect(),
            unexpected,
            missing,
        })
    }

    /// The callee of `call`, a call from the callee's path to its `)`, as
    /// rustc's E0061 marks it: the path up to its `(` or turbofish, or
    /// the name `name` from the call's start where the call spans lines.
    pub(super) fn callee_path(&self, call: Span, name: &str) -> Span {
        let path = self.written(call).map(|text| {
            let end = text.find("::<").into_iter().chain(text.find('(')).min();
            text[..end.unwrap_or(text.len())].trim_end().to_owned()
        });
        let path = path.as_deref().unwrap_or(name);
        Span::new(call.start(), text_end(call.start(), path))
    }

    /// The whole of the expression `at`, which ends where its last token
    /// starts or already past it: through that token, short of any `,` or
    /// closing bracket it doesn't open.
    pub(super) fn whole_expression(&self, at: Span) -> Span {
        let whole = self.expression_span(at);
        let Some(text) = self.written(whole) else {
            return whole;
        };
        let mut depth = 0_i32;
        let mut kept = 0;
        for (count, character) in text.chars().enumerate() {
            match character {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
            if depth < 0 || (depth == 0 && character == ',') {
                break;
            }
            if !character.is_whitespace() {
                kept = count + 1;
            }
        }
        Span::new(
            whole.start(),
            text_end(whole.start(), &text.chars().take(kept).collect::<String>()),
        )
    }

    /// The type of `argument` as rustc's E0061 names it: a string
    /// literal's is `&'static str`, as rustc gives a literal before it
    /// erases its lifetime.
    fn argument_type(&self, argument: &ParsedExpr) -> ImmutableString {
        let ty = self.inference.resolve(&argument.ty);
        let literal = self
            .written(self.whole_expression(argument.span))
            .is_some_and(|text| {
                text.starts_with('"') || text.starts_with("r\"") || text.starts_with("r#")
            });
        if ty == Ty::Str && literal {
            return "&'static str".into();
        }
        ty.name(&self.models)
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

/// Whether `ty` is unsized, which rustc's expectation of an argument only
/// hints at (`ExpectRvalueLikeUnsized`): `str`, a slice, a trait object.
fn unsized_hint(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::UnsizedStr
            | Ty::Dyn(..)
            | Ty::Array {
                kind: sequence::Kind::Slice,
                ..
            }
    )
}

/// Whether the type of the parameter at `index` of `parameters` is a type
/// another parameter's names too, whose mismatch rustc labels with what
/// each must match.
fn shared_generic(parameters: &[item::Parameter], index: usize) -> bool {
    let named = |ty: &type_ref::Type| match ty {
        type_ref::Type::Path(path) => Some(path.name.clone()),
        _ => None,
    };
    let Some(name) = parameters
        .get(index)
        .and_then(|parameter| named(&parameter.ty))
    else {
        return false;
    };
    parameters
        .iter()
        .enumerate()
        .any(|(other, parameter)| other != index && mentions(&parameter.ty, &name))
}

/// Whether `ty` names the type `name`.
pub(super) fn mentions(ty: &type_ref::Type, name: &str) -> bool {
    match ty {
        type_ref::Type::Path(path) => {
            path.name == name
                || path.arguments.iter().any(|argument| match argument {
                    type_ref::Argument::Type(ty) => mentions(ty, name),
                    type_ref::Argument::Lifetime(_) => false,
                })
        }
        type_ref::Type::Reference { target, .. } => mentions(target, name),
        type_ref::Type::Tuple { members, .. } => {
            members.iter().any(|member| mentions(member, name))
        }
        type_ref::Type::Unit(_) => false,
    }
}

/// A call of a program function, at one of its parameters, as rustc's
/// E0308 at an argument marks it.
struct Called<'a> {
    function: &'a item::Function,
    parameter: &'a item::Parameter,
    /// The callee as the call writes it.
    callee: Span,
    /// The call, through its `)`.
    call: Span,
    /// The function is defined in the call's file.
    declared: bool,
}

impl Called<'_> {
    /// rustc's E0308 at the argument starting where `argument` does, of
    /// the mismatch `error` reports there, or `None` for another error.
    fn mismatch(&self, error: &Error, argument: Span) -> Option<Error> {
        let Error::TypeMismatch {
            expected,
            actual,
            expression,
        } = error
        else {
            return None;
        };
        if expression.start() != argument.start() {
            return None;
        }
        let function = self.function;
        let output = function.output.as_ref().map_or(Ty::Unit, Ty::from_resolved);
        let generic = Ty::from_resolved(&self.parameter.ty);
        let generic = matches!(&self.parameter.ty, type_ref::Type::Path(path)
            if function.generics.iter().any(|declared| declared.name.text == path.name))
            && contains(&output, &|ty| *ty == generic);
        Some(Error::ArgumentMismatch(Box::new(
            crate::resolve::ArgumentMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
                argument: *expression,
                callee: self.callee,
                call: self.call,
                function: function.name.text.clone(),
                generic,
                definition: self
                    .declared
                    .then_some((function.name.span, self.parameter.span)),
            },
        )))
    }
}

/// A call argument and what the call expects of it: its place among the
/// arguments, the closure parameter types and output it is given, its
/// expected type, and its generic parameter's type. A closure read after
/// the arguments that follow it keeps its tokens.
struct Argument {
    index: usize,
    tokens: Vec<(Token, Position)>,
    hint: Option<Box<[Ty]>>,
    output: Option<closure::Output>,
    /// The return type std's `Fn` bound gives a closure argument (#804).
    returns: Option<Ty>,
    expected: Option<Ty>,
    generic: Option<Ty>,
}
