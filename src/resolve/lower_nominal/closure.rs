//! Closures and async blocks: their parsing, the hints that type their
//! parameters, and the closures lifted into their own functions.

use super::lvalue::Lvalue;
use super::pattern::PatternSyntax;
use super::ty::{Float, HostTyProof, Int, Ty};
use super::{
    assignment_token, contains_inference, drops, origin, places, point_span, span, text_end,
    Binding, BindingKind, ParsedExpr, Parser, ASYNC_RESUME, CORE_ASYNC, SELF_CAPTURE,
};
use crate::resolve::borrowck::record::ScopeEnd;
use crate::resolve::borrowck::{self, record};
use crate::resolve::{BindingSite, Error};
use crate::tokenizer::Token;
use crate::{ImmutableString, Position, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// A closure passed to the host: it becomes its own generated function, an
/// item the host invokes like a named function, after the values it
/// captured.
#[derive(Clone)]
pub(super) struct LiftedClosure {
    pub(super) key: crate::item::Key,
    pub(super) value: Option<crate::item::Value>,
    /// The captures' parameters first, then the closure's own.
    pub(super) parameters: Vec<ImmutableString>,
    /// Each capture's value where the closure is written.
    pub(super) captures: Vec<origin::Fragment>,
    pub(super) body: origin::Fragment,
    pub(super) span: Span,
    /// Its parameters from `|` to `|`.
    pub(super) header: Span,
    /// Whether its body reaches a place through a handle made where it is
    /// written (`places::held`), which its generated function has no way to
    /// be given.
    pub(super) held: bool,
}

/// An executable callable for a lifted closure.
pub(crate) struct ClosureCallable {
    pub(crate) value: crate::item::Value,
    pub(crate) arity: usize,
    pub(crate) dispatch: ImmutableString,
    pub(crate) source: crate::program::source::Id,
}

/// The host type a host callable returns, and how many type arguments it
/// takes.
pub(super) type ClosureOutput = (Rc<HostTyProof>, usize);

/// A host callable parameter's input types, when known, and its output.
pub(super) type ClosureHint = (Option<Box<[Ty]>>, Option<ClosureOutput>);

/// A closure body being parsed: the bindings declared outside it, and the
/// ones it uses (captures).
pub(super) struct ClosureScope {
    pub(super) outer: BTreeMap<ImmutableString, Span>,
    pub(super) captures: BTreeSet<ImmutableString>,
    /// The captures the body moves out, which a closure that is not
    /// `move` takes by value. A closure that has one is `FnOnce`.
    pub(super) consumed: BTreeSet<ImmutableString>,
    /// An async body, where `.await` may appear.
    pub(super) asynchronous: bool,
    /// The `&mut` borrows of places declared outside the closure that its
    /// body uses, each reached through a shared handle made where the
    /// closure is (`places::held`).
    pub(super) held: Vec<places::Held>,
}

impl<'a> Parser<'a> {
    /// Before parsing a single-signature host call's arguments, the input
    /// types of each host callable parameter, so closure parameters have
    /// their types as in Rust.
    /// A closure argument takes its parameter types from the one overload
    /// whose parameter there is a host callable, as iced's `.style(..)`
    /// takes a pinned style or `impl Fn(&Theme, Status) -> Style`.
    pub(super) fn hint_closures(
        &mut self,
        items: &[crate::host::item::Definition],
        receiver: Option<&Ty>,
    ) {
        self.deferred_hints = (!items.is_empty()).then(|| (items.into(), receiver.cloned()));
        let mut overloads = Vec::<Vec<Option<ClosureHint>>>::new();
        for item in items {
            let checkpoint = self.inference.checkpoint();
            let hints: Vec<Option<ClosureHint>> = self
                .instantiate_host_signature(item, receiver, None, Span::NONE)
                .map(|signature| {
                    signature
                        .inputs
                        .iter()
                        .map(|input| match self.inference.resolve(input) {
                            // An input still open gives no parameter types,
                            // but the output still converts, as a table
                            // column's `|row: &Row| text(..)` into `Element`.
                            Ty::Callable { inputs, output, .. } => {
                                let output = match self.inference.resolve(&output) {
                                    Ty::Host { proof, arguments } => Some((proof, arguments.len())),
                                    _ => None,
                                };
                                let inputs = Some(inputs);
                                (inputs.is_some() || output.is_some()).then_some((inputs, output))
                            }
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.inference.rollback(checkpoint);
            // An input still open gives no parameter types, except a host
            // type whose arguments are open: `sipper(|progress| ..)` takes
            // a `Sender<P>` whose `P` the body settles.
            let hints = hints
                .into_iter()
                .map(|hint: Option<ClosureHint>| {
                    hint.and_then(|(inputs, output)| {
                        let mut fresh = BTreeMap::new();
                        let inputs = inputs.and_then(|inputs| {
                            inputs
                                .iter()
                                .map(|input| self.open_host_hint(input, &mut fresh))
                                .collect::<Option<Box<[Ty]>>>()
                        });
                        (inputs.is_some() || output.is_some()).then_some((inputs, output))
                    })
                })
                .collect();
            overloads.push(hints);
        }
        let positions = overloads.iter().map(Vec::len).max().unwrap_or_default();
        let hints = (0..positions)
            .map(|position| {
                let mut callables = overloads
                    .iter()
                    .filter_map(|hints| hints.get(position).cloned().flatten());
                match (callables.next(), callables.next()) {
                    (Some(hint), None) => Some(hint),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        self.closure_outputs = hints
            .iter()
            .map(|hint| hint.as_ref().and_then(|(_, output)| output.clone()))
            .collect();
        self.closure_hints = hints
            .into_iter()
            .map(|hint| hint.and_then(|(inputs, _)| inputs))
            .collect();
    }

    /// A closure parameter hint: `ty` itself when it is settled, or a host
    /// type whose open arguments become fresh inference variables.
    pub(super) fn open_host_hint(
        &mut self,
        ty: &Ty,
        fresh: &mut BTreeMap<usize, Ty>,
    ) -> Option<Ty> {
        if !contains_inference(ty) {
            return Some(ty.clone());
        }
        let Ty::Host { proof, arguments } = ty else {
            return None;
        };
        let arguments = arguments
            .iter()
            .map(|argument| match argument {
                Ty::Infer(id) => Some(
                    fresh
                        .entry(*id)
                        .or_insert_with(|| self.inference.fresh())
                        .clone(),
                ),
                argument => self.open_host_hint(argument, fresh),
            })
            .collect::<Option<Box<[Ty]>>>()?;
        Some(Ty::Host {
            proof: proof.clone(),
            arguments,
        })
    }

    /// The input types of the host callable parameter after `arguments`
    /// in the overloads the arguments so far fit, when they agree:
    /// `slider(0.0..=1.0, value, |c| ..)` takes the `f64` slider's `c`.
    pub(super) fn overload_closure_hint(
        &mut self,
        items: &[crate::host::item::Definition],
        receiver: Option<&Ty>,
        arguments: &[ParsedExpr],
    ) -> Option<Box<[Ty]>> {
        if let [item] = items {
            return self.inferred_closure_hint(item, receiver, arguments, false);
        }
        let mut hints = items
            .iter()
            .filter_map(|item| self.inferred_closure_hint(item, receiver, arguments, true));
        let hint = hints.next()?;
        if !hints.all(|other| other == hint) {
            return None;
        }
        // The facade's `f64` and `i64` stand for any float and integer, as
        // a slider over `f32` is, so the kind stays open.
        Some(
            hint.iter()
                .map(|input| match input {
                    Ty::Float(_) => Ty::Float(Float::Var(self.inference.fresh_id())),
                    Ty::Int(_) => Ty::Int(Int::Var(self.inference.fresh_id())),
                    input => input.clone(),
                })
                .collect(),
        )
    }

    /// The input types of the host callable parameter after `arguments`,
    /// with the generics the arguments before it settle: `None` when it is
    /// no callable or they leave an input open, or, when `exact`, when an
    /// argument does not fit its parameter.
    pub(super) fn inferred_closure_hint(
        &mut self,
        item: &crate::host::item::Definition,
        receiver: Option<&Ty>,
        arguments: &[ParsedExpr],
        exact: bool,
    ) -> Option<Box<[Ty]>> {
        let checkpoint = self.inference.checkpoint();
        let hint = self
            .instantiate_host_signature(item, receiver, None, Span::NONE)
            .ok()
            .and_then(|signature| {
                let Ty::Callable { .. } = self
                    .inference
                    .resolve(signature.inputs.get(arguments.len())?)
                else {
                    return None;
                };
                if exact && signature.inputs.len() <= arguments.len() {
                    return None;
                }
                for (parameter, argument) in signature.inputs.iter().zip(arguments) {
                    let actual = self.inference.resolve(&argument.ty);
                    // `&state` of a value the call borrows, as
                    // `PaneGrid::new(&self.panes, ..)`, settles what the
                    // reference's target does.
                    let unified = self.unify_type(parameter, &actual)
                        || match (self.inference.resolve(parameter), &actual) {
                            (Ty::Reference { target, .. }, Ty::Reference { target: inner, .. }) => {
                                self.unify_type(&target, inner)
                            }
                            (Ty::Reference { target, .. }, actual) => {
                                self.unify_type(&target, actual)
                            }
                            _ => false,
                        };
                    if !unified && exact {
                        return None;
                    }
                }
                let Ty::Callable { inputs, .. } =
                    self.inference.resolve(&signature.inputs[arguments.len()])
                else {
                    return None;
                };
                let inputs = inputs
                    .iter()
                    .map(|input| self.inference.resolve(input))
                    .collect::<Box<[Ty]>>();
                (!inputs.iter().any(contains_inference)).then_some(inputs)
            });
        self.inference.rollback(checkpoint);
        hint
    }

    /// The closures this body passed to the host.
    pub(super) fn take_lifted(&mut self) -> Vec<LiftedClosure> {
        std::mem::take(&mut self.lifted)
            .into_iter()
            .filter(|closure| closure.value.is_some())
            .collect()
    }

    /// A use of a local inside a closure that was declared outside it.
    pub(super) fn record_capture(&mut self, name: &ImmutableString) {
        let Some(declaration) = self
            .bindings
            .get(name.as_str())
            .map(|binding| binding.declaration)
        else {
            return;
        };
        for scope in &mut self.closures {
            if scope.outer.get(name) == Some(&declaration) {
                scope.captures.insert(name.clone());
            }
        }
    }

    /// `|a, b: T| body`, `move |..| ..` or `|| ..`. A closure that captures
    /// nothing is a function item; one that captures is a `fn` value whose
    /// captures are shared with its scope, or copied into it with `move`.
    /// `async { .. }`, `async move { .. }`, `async |x| ..` and
    /// `async move |x| ..`.
    pub(super) fn parse_async(&mut self, start: Span) -> Result<ParsedExpr, Error> {
        let moved = matches!(self.peek(), Token::Identifier(ref name) if name.as_str() == "move")
            && !self.bindings.contains_key("move");
        if moved {
            self.next();
        }
        match self.peek() {
            Token::Pipe | Token::Or => {
                let token = self.next().0;
                self.async_closure = true;
                self.parse_closure(token, start, moved)
            }
            Token::LeftBrace => self.parse_async_block(start, moved, None),
            _ => Err(self.unsupported("`async` before something other than a block or a closure")),
        }
    }

    /// An async block: its body becomes a step function, which the facade
    /// makes a `Task` of its output (`eval::machine`). `output` is the
    /// body's type when the caller knows it, as an `async fn`'s.
    pub(super) fn parse_async_block(
        &mut self,
        start: Span,
        moved: bool,
        output: Option<Ty>,
    ) -> Result<ParsedExpr, Error> {
        // The core's future runs async bodies (D511.3).
        let items = self
            .models
            .host_items(CORE_ASYNC)
            .map(<[_]>::to_vec)
            .ok_or_else(|| self.unsupported("`async`"))?;
        let owner = self.models.item_owner(CORE_ASYNC).cloned();
        self.async_step = Some(output);
        let step = self.parse_closure(Token::Or, start, moved)?;
        let call = span(start.start(), step.span.end());
        self.call_host_function(&items, owner, vec![step], call)
    }

    /// `future.await` in an async body: the machine suspends there
    /// (`eval::machine::AWAIT`), and the future's output is its value.
    pub(super) fn parse_await(
        &mut self,
        future: ParsedExpr,
        at: Position,
    ) -> Result<ParsedExpr, Error> {
        let whole = span(future.span.start(), point_span(at).end());
        if !self.closures.last().is_some_and(|scope| scope.asynchronous) {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "`.await` outside an async block or `async fn`".into(),
                source: whole,
            });
        }
        let Some(output) = self.task_output(&future.ty) else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "`.await` on a value that is not a future".into(),
                source: whole,
            });
        };
        // `.await` takes the future, as Rust's `IntoFuture` does.
        let mut future = future;
        self.move_local(&mut future)?;
        let mut source = origin::Fragment::plain(format!("{}(", crate::eval::machine::AWAIT));
        source.push(future.rendered_source());
        source.push_text(")");
        Ok(self.operator_expr(source, output, whole))
    }

    /// The body of an `async fn`, whose declared output is `Task<T>`: an
    /// `async move` block of its body, as Rust desugars it, and the tail.
    pub(super) fn parse_async_body(&mut self, output: &Ty, body: Span) -> Result<(), Error> {
        let inner = self.task_output(output);
        let block = self.parse_async_block(body, true, inner)?;
        let start = self.write_output(block.rendered_source(), block.span);
        self.last_type = block.ty;
        self.last_expression = Some((start, block.span));
        Ok(())
    }

    /// The output of the core's future, which `.await` gives.
    pub(super) fn task_output(&self, ty: &Ty) -> Option<Ty> {
        match self.inference.resolve(ty) {
            Ty::Host { proof, arguments }
                if proof.definition.path().as_str() == "rawr::future::Future"
                    && arguments.len() == 1 =>
            {
                Some(arguments[0].clone())
            }
            _ => None,
        }
    }

    pub(super) fn parse_closure(
        &mut self,
        token: Token,
        start: Span,
        moved: bool,
    ) -> Result<ParsedExpr, Error> {
        let async_step = self.async_step.take();
        let async_closure = std::mem::take(&mut self.async_closure);
        let mut hint = self.closure_hint.take();
        // A tuple the host hands over spreads over a closure of as many
        // parameters: `event::listen_with(|event, status, window| ..)`.
        if token == Token::Pipe
            && let Some([single]) = hint.as_deref()
            && let Ty::Tuple(members) = self.inference.resolve(single)
            && members.len() > 1
        {
            let mut depth = 0_usize;
            let mut count = 1_usize;
            for (token, _) in self.input.clone() {
                match token {
                    Token::Pipe if depth == 0 => break,
                    Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                    Token::RightParen | Token::RightBracket | Token::RightBrace => {
                        depth = depth.saturating_sub(1);
                    }
                    Token::Comma if depth == 0 => count += 1,
                    Token::EOF => break,
                    _ => {}
                }
            }
            if count == members.len() {
                hint = Some(members);
            }
        }
        let output_hint = self.closure_output.take();
        let outer_changed = self.self_changed.replace(false);
        let mut parameters = Vec::<(ImmutableString, Ty, Span)>::new();
        let mut patterns = Vec::<(usize, PatternSyntax)>::new();
        // `|a, b|` as rustc's note on a call of it shows it.
        let mut header = start;
        if token == Token::Pipe {
            while !matches!(self.peek(), Token::Pipe) {
                let (name, declaration) = if matches!(self.peek(), Token::Underscore) {
                    let position = self.next().1;
                    (self.temporary("discard").into(), point_span(position))
                } else if matches!(
                    self.peek(),
                    Token::LeftParen | Token::Ampersand | Token::And | Token::Mut
                ) || matches!(self.peek_second(), Token::Reserved(ref text) if text.as_str() == "@")
                    || matches!(self.peek(), Token::Identifier(_))
                        && matches!(
                            self.peek_second(),
                            Token::LeftParen | Token::DoubleColon | Token::LeftBrace
                        )
                {
                    // `|(a, b)|`, `|&x|`, `|mut x|`: the parameter is
                    // destructured at the top of the body.
                    let pattern = self.parse_pattern_no_alt()?;
                    let declaration = pattern.span();
                    patterns.push((parameters.len(), pattern));
                    (self.temporary("parameter").into(), declaration)
                } else {
                    self.expect_name("closure parameter")?
                };
                let ty = if matches!(self.peek(), Token::Colon) {
                    self.next();
                    self.parse_local_type()?
                } else if let Some(ty) = hint.as_ref().and_then(|hint| hint.get(parameters.len())) {
                    // The host callable's parameter type, as Rust infers it.
                    ty.clone()
                } else {
                    self.inference.fresh()
                };
                parameters.push((name, ty, declaration));
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else if !matches!(self.peek(), Token::Pipe) {
                    return Err(self.unsupported("closure parameters without ','"));
                }
            }
            header = span(start.start(), text_end(self.next().1, "|"));
        }
        let annotated = if matches!(self.peek(), Token::ThinArrow) {
            self.next();
            let ty = self.parse_local_output_type()?;
            if !matches!(self.peek(), Token::LeftBrace) {
                return Err(self.unsupported("a closure return type without a block body"));
            }
            Some(ty)
        } else {
            async_step.clone().flatten()
        };
        // An async body's step function takes the value it resumes with
        // last (`eval::machine`).
        if async_step.is_some() {
            parameters.push((ASYNC_RESUME.into(), Ty::Unit, start));
        }

        // `?` in the body returns from the closure, as in Rust.
        let closure_output = annotated.clone().unwrap_or_else(|| self.inference.fresh());
        let outer_return = self.return_ty.replace(closure_output.clone());
        let outer_returns = std::mem::replace(&mut self.returns, 0);
        let outer_blocked = self.return_blocked.take();
        let outer_bindings = self.bindings.clone();
        self.closures.push(ClosureScope {
            outer: outer_bindings
                .iter()
                .map(|(name, binding)| (name.clone(), binding.declaration))
                .collect(),
            captures: BTreeSet::new(),
            consumed: BTreeSet::new(),
            asynchronous: async_step.is_some(),
            held: Vec::new(),
        });
        let outer_loops = std::mem::take(&mut self.loops);
        let closure_split = self.flow().closure_enter();
        // A `&mut T` parameter is copied in and back out, as a function's
        // (`copied_parameters`): the closure returns `[value, ..parameters]`.
        let copied = parameters
            .iter()
            .filter(|(_, ty, _)| {
                matches!(
                    self.inference.resolve(ty),
                    Ty::Reference { mutable: true, .. }
                )
            })
            .map(|(name, _, _)| name.clone())
            .collect::<Vec<_>>();
        let outer_copy_out = std::mem::replace(&mut self.copy_out, copied.clone());
        let outer_copy_depth = std::mem::replace(&mut self.copy_out_depth, self.closures.len());
        // The closure's body is a body of its own: its parameters drop at
        // its end, and a `return` leaves only its scopes.
        let outer_drops = std::mem::take(&mut self.drop_scopes);
        let outer_drop_base = std::mem::replace(&mut self.drop_base, 0);
        self.open_drops();
        for (name, ty, declaration) in &parameters {
            self.parameter_declarations.insert(*declaration);
            self.bindings.insert(
                name.clone(),
                Binding {
                    ty: ty.clone(),
                    mutable: copied.contains(name),
                    available: true,
                    declaration: *declaration,
                    kind: BindingKind::Local,
                },
            );
            if self.drop_scoped() && name.as_str() != ASYNC_RESUME {
                self.note_drop_here(name, *declaration, ty)?;
            }
        }
        let mut destructured = String::new();
        for (index, pattern) in &patterns {
            let (name, ty, _) = &parameters[*index];
            let (name, ty) = (name.clone(), ty.clone());
            let destructure = self
                .refuse_refutable(pattern, &ty, BindingSite::Argument)
                .and_then(|()| self.destructure(pattern, &name, &ty))
                .and_then(|statements| {
                    if !self.drops() {
                        return Ok(statements);
                    }
                    let bound = self.pattern_bindings(pattern);
                    let holder = drops::Holder::Local(name.to_string(), Vec::new());
                    let clears = self.pattern_drops(
                        Some(pattern),
                        &ty,
                        &holder,
                        &bound,
                        drops::Clears::InArm,
                    )?;
                    Ok(statements + clears.as_str())
                });
            match destructure {
                Ok(statements) => destructured.push_str(&statements),
                Err(error) => {
                    self.copy_out = outer_copy_out;
                    self.copy_out_depth = outer_copy_depth;
                    self.loops = outer_loops;
                    let _ = self.flow().closure_exit(closure_split);
                    self.bindings = outer_bindings;
                    self.return_ty = outer_return;
                    self.returns = outer_returns;
                    self.return_blocked = outer_blocked;
                    self.drop_scopes = outer_drops;
                    self.drop_base = outer_drop_base;
                    self.closures.pop();
                    return Err(error);
                }
            }
        }
        let body = if async_closure {
            // `async |x| body` is `|x| async move { body }`.
            self.parse_async_block(start, true, None)
        } else if matches!(self.peek(), Token::LeftBrace) {
            self.parse_match_action().map(|branch| branch.value)
        } else {
            let pending = self.field_entries.len();
            self.open_temporaries();
            self.flow().open_temporaries();
            // A closure's body opens its own scope frames (D558 tier 1).
            self.open_frames();
            let body = self.parse_expr();
            let frames = self.close_frames();
            self.flow().close_temporaries(ScopeEnd::Statement);
            let temporaries = self.close_temporaries();
            body.and_then(|mut value| {
                if !temporaries.is_empty() {
                    value.source = self.with_temporaries(&temporaries, value.rendered_source());
                    value.prelude = origin::Fragment::default();
                }
                if !frames.is_empty() {
                    let rendered = value.rendered_source();
                    value.source = self.framed(frames, rendered);
                    value.prelude = origin::Fragment::default();
                    value.lvalue = None;
                }
                if !assignment_token(&self.peek()) {
                    // `|| x` returns its capture `x` by value, and `|| s.a`
                    // its field: the closure takes it and is called once,
                    // as in Rust.
                    if matches!(value.lvalue, Some(Lvalue::Binding { .. })) {
                        self.move_local(&mut value)?;
                    } else {
                        self.move_out_value(&mut value)?;
                    }
                    return Ok(value);
                }
                self.parse_assignment_expr(value)
            })
            // A place the body changes through a field (`|x| p.b.push(x)`)
            // is reached in the body, at each call, not where the closure
            // is made.
            .map(|mut value| {
                if self.field_entries.len() > pending {
                    let entries = self.field_entries.split_off(pending);
                    let result = self.temporary("body");
                    let mut block = origin::Fragment::plain("{ ");
                    for (read, _) in &entries {
                        block.push(read.clone());
                    }
                    block.push_text(format!(" let {result} = "));
                    block.push(value.rendered_source());
                    block.push_text("; ");
                    for (_, store) in entries {
                        block.push(store);
                    }
                    block.push_text(format!(" {result} }}"));
                    value.source = block;
                    value.prelude = origin::Fragment::default();
                    value.lvalue = None;
                }
                value
            })
        };
        let parameter_drops = self.close_drops();
        self.drop_scopes = outer_drops;
        self.drop_base = outer_drop_base;
        self.copy_out = outer_copy_out;
        self.copy_out_depth = outer_copy_depth;
        self.loops = outer_loops;
        let closure_blocks = self.flow().closure_exit(closure_split);
        self.bindings = outer_bindings;
        self.return_ty = outer_return;
        let returned = std::mem::replace(&mut self.returns, outer_returns) > 0;
        self.return_blocked = outer_blocked;
        let scope = self.closures.pop().expect("the closure pushed its scope");
        // What the closure captures, made where it is, and what a call of
        // it keeps (D572, T105).
        let captured: Vec<record::Captured> = scope
            .captures
            .iter()
            .filter_map(|name| {
                let declaration = *scope.outer.get(name)?;
                let binding = self.bindings.get(name.as_str())?;
                Some(record::Captured {
                    declaration,
                    holder: self.may_hold_borrow(&binding.ty),
                    copy: !self.moves_by_value(&binding.ty),
                    // A function's `&mut` parameter, or a `&mut` pattern
                    // binding, can be changed through but was not declared
                    // `mut`. A closure inside one that captures the same
                    // local reaches it through the enclosing closure's
                    // capture, and borrows it as rustc does a `mut` one.
                    declared_mut: binding.mutable
                        && !self.parameter_declarations.contains(&declaration)
                        && !self.undeclared_mut.contains(&declaration)
                        || self
                            .closures
                            .last()
                            .is_some_and(|outer| outer.outer.get(name) == Some(&declaration)),
                })
            })
            .collect();
        let tail = match &body {
            Ok(value) if self.may_hold_borrow(&value.ty) => {
                self.flow().origins(value.span).map(<[_]>::to_vec)
            }
            _ => Some(Vec::new()),
        };
        let declared: Vec<Span> = parameters.iter().map(|(_, _, at)| *at).collect();
        // Each captured local's type, by its declaration, for whether a part
        // captured by value is `Copy`.
        let capture_types: BTreeMap<Span, Ty> = scope
            .captures
            .iter()
            .filter_map(|name| {
                let declaration = *scope.outer.get(name)?;
                Some((declaration, self.bindings.get(name.as_str())?.ty.clone()))
            })
            .collect();
        let closure = {
            let this = &*self;
            let copy_of = |declaration: Span, path: &[borrowck::Projection]| {
                this.part_copy(capture_types.get(&declaration)?, path)
            };
            let mut flow = this.flow();
            if !scope.captures.is_empty() {
                flow.captured();
            }
            flow.captures(
                closure_blocks,
                &captured,
                &declared,
                tail,
                moved,
                point_span(start.start()),
                &copy_of,
            )
        };
        let mut body = body?;
        if let Some(expected) = &annotated {
            self.coerce_expr(expected, &mut body)?;
        } else if returned {
            // A body that returns early has the type of its `return`s.
            self.coerce_expr(&closure_output, &mut body)?;
            body.ty = closure_output.clone();
        } else if let Ty::Option(_) | Ty::Result(..) = self.inference.resolve(&closure_output) {
            // A body that used `?` returns its `Option` or `Result`.
            self.coerce_expr(&closure_output, &mut body)?;
        } else if let Some((proof, arity)) = output_hint {
            // A host callable's output, as iced's `impl Into<Element>`.
            let arguments = (0..arity).map(|_| self.inference.fresh()).collect();
            let expected = Ty::Host { proof, arguments };
            self.coerce_expr(&expected, &mut body)?;
            body.ty = expected;
        }
        // An enclosing closure captures what this one captures from outside it.
        for name in &scope.captures {
            self.record_capture(name);
        }

        let names_list = parameters
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect::<Vec<_>>();
        let names = names_list
            .iter()
            .map(ImmutableString::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        // A parameter inferred as `&mut T` only after the body was read
        // would not be copied back out.
        if let Some((name, _, declaration)) = parameters.iter().find(|(name, ty, _)| {
            matches!(
                self.inference.resolve(ty),
                Ty::Reference { mutable: true, .. }
            ) && !copied.contains(name)
        }) {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "a closure parameter `{name}` used as `&mut` without its `&mut T` type written"
                )
                .into(),
                source: *declaration,
            });
        }
        let mut body_block = if destructured.is_empty() {
            body.rendered_block()
        } else {
            let mut block = origin::Fragment::plain(format!("{{{destructured} "));
            block.push(body.rendered_block());
            block.push_text(" }");
            block
        };
        let parameter_drops = parameter_drops?;
        if !parameter_drops.is_empty() {
            body_block = self.then_drops(body_block, &parameter_drops);
        }
        if !copied.is_empty() {
            let mut block = origin::Fragment::plain("{ let __rawr_returned = ");
            block.push(body_block);
            block.push_text(format!("; [__rawr_returned, {}] }}", copied.join(", ")));
            body_block = block;
        }
        let mut source = origin::Fragment::plain(format!("|{names}| "));
        source.push(body_block.clone());
        let inputs: Box<[Ty]> = parameters.into_iter().map(|(_, ty, _)| ty).collect();
        let output = Box::new(annotated.unwrap_or_else(|| body.ty.clone()));
        let expression = span(start.start(), body.span.end());
        // How the borrows of a closure's output relate to its inputs' is
        // rustc's closure signature inference, which may make them one
        // region for any borrow (`impl Fn(&u32) -> &u32`).
        if self.may_hold_borrow(&output) && inputs.iter().any(|input| self.may_hold_borrow(input)) {
            self.flow().unmodeled(
                "a closure that takes and returns borrows, whose signature relates them",
                expression,
            );
        }
        // A closure that captures by `&mut`, or by value a part that is not
        // `Copy`, is not `Copy` (T146).
        let copy = closure.copy || scope.asynchronous;
        // A closure holds its captures' borrows. An async block's are left
        // to `cargo check`: its value is a future the facts don't follow.
        if !scope.asynchronous {
            let mut flow = self.flow();
            flow.set_origins(expression, closure.origins.clone());
            flow.set_closure_keeps(expression, closure.keeps.clone());
        }
        if let Some(ty) = scope
            .captures
            .iter()
            .filter_map(|name| self.bindings.get(name.as_str()))
            .map(|binding| self.inference.resolve(&binding.ty))
            .find(|ty| self.not_send(ty))
        {
            self.unsent_captures.insert(expression, ty);
        }
        if !moved
            && let Some(name) = scope
                .captures
                .iter()
                .find(|name| !scope.consumed.contains(*name))
        {
            let name = if name == "self" {
                "self".into()
            } else {
                name.clone()
            };
            self.borrowed_captures.insert(expression, name);
        }
        // `move` moves each captured place that is not `Copy` into the
        // closure as it is created, and copies the others: a field leaves
        // the local partly moved (T145). A closure that is not `move` moves
        // in only the captures its body moves out.
        let mut moving = BTreeSet::new();
        for name in &scope.captures {
            if name == "self" || !(moved || scope.consumed.contains(name)) {
                continue;
            }
            // A `&mut` borrow of a place moves into a `move` closure: the
            // closure holds the place's handle (`held`), and a later use of
            // the borrow is a use after move.
            if moved
                && let Some(declaration) = scope.outer.get(name)
                && self
                    .aliases
                    .get(name)
                    .is_some_and(|(at, _)| at == declaration)
            {
                if let Some(binding) = self.bindings.get_mut(name.as_str())
                    && binding.declaration == *declaration
                {
                    binding.available = false;
                    self.moved_at.insert(*declaration, expression);
                }
                continue;
            }
            let parts = match scope.outer.get(name) {
                Some(declaration) if moved && !scope.asynchronous => closure
                    .by_value
                    .iter()
                    .filter(|(captured, ..)| captured == declaration)
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            if parts.is_empty() || parts.iter().any(|(_, path, _)| path.is_empty()) {
                if self.move_capture(name, expression) {
                    moving.insert(name.clone());
                }
                continue;
            }
            if self.move_capture_parts(name, &parts, expression)? {
                moving.insert(name.clone());
            }
        }
        // A closure that owns a value with drop glue drops it with itself,
        // which Rawr's closures, values copied freely, cannot do.
        for name in &moving {
            let ty = self
                .bindings
                .get(name.as_str())
                .map(|binding| binding.ty.clone());
            if let Some(ty) = ty
                && self.needs_drop(&ty, expression)?
            {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "a closure or async block that takes `{name}`, whose type implements \
                         `Drop`, by move: dropping it with the closure is not supported in Rawr yet"
                    )
                    .into(),
                    source: expression,
                });
            }
        }
        let ty = match &self.callable {
            Some(callable) => {
                self.closure_count += 1;
                let key = crate::item::Key::closure(callable, self.closure_count);
                // Passed to the host, the closure is its generated function
                // with each capture's value bound where it is written.
                let mut lifted_parameters = Vec::new();
                let mut captures = Vec::new();
                for name in &scope.captures {
                    if name == "self" {
                        lifted_parameters.push(SELF_CAPTURE.into());
                        captures.push(origin::Fragment::plain(if self.closures.is_empty() {
                            "self"
                        } else {
                            SELF_CAPTURE
                        }));
                    } else if moving.contains(name) {
                        lifted_parameters.push(name.clone());
                        captures.push(origin::Fragment::call(
                            crate::engine::KEYWORD_MOVE,
                            [origin::Fragment::plain(name.as_str())],
                        ));
                    } else if !moved
                        && !scope.asynchronous
                        && !self.aliases.contains_key(name.as_str())
                    {
                        // A closure that isn't `move` borrows what it
                        // captures, as one native code calls only during
                        // its call does, as image's `from_fn(w, h, |x, y|
                        // { calls += 1; .. })`: the local is lent, its
                        // shared handle and not a copy, so a change the
                        // closure makes is the local's own.
                        lifted_parameters.push(name.clone());
                        captures.push(origin::Fragment::call(
                            crate::engine::KEYWORD_LEND,
                            [origin::Fragment::plain(name.as_str())],
                        ));
                    } else {
                        lifted_parameters.push(name.clone());
                        captures.push(origin::Fragment::plain(name.as_str()));
                    }
                }
                lifted_parameters.extend(names_list);
                self.lifted.push(LiftedClosure {
                    key: key.clone(),
                    value: None,
                    parameters: lifted_parameters,
                    captures,
                    body: body_block,
                    span: expression,
                    header,
                    held: !scope.held.is_empty(),
                });
                Ty::FnItem {
                    key: Box::new(key),
                    inputs,
                    output,
                }
            }
            None => Ty::Fn { inputs, output },
        };
        // A closure that owns a capture which is not `Copy` is not `Copy`,
        // and one that moves out a capture is called once.
        if let Ty::FnItem { key, .. } = &ty {
            // A closure that isn't `move` takes by value only what its body
            // moves out, such as a field (T152).
            let moves_out = !moved && closure.by_value.iter().any(|(.., copy)| !copy);
            // A `Copy` closure changes only what it captured by value: a
            // copy of it copies those, and changes its own (T150).
            if moved && copy && closure.mutable && !scope.asynchronous {
                let state: Vec<ImmutableString> = scope
                    .captures
                    .iter()
                    .filter(|name| {
                        name.as_str() != "self"
                            && !self.aliases.contains_key(name.as_str())
                            && self.bindings.get(name.as_str()).is_some_and(|binding| {
                                !matches!(self.inference.resolve(&binding.ty), Ty::Reference { .. })
                            })
                    })
                    .cloned()
                    .collect();
                if !state.is_empty() {
                    self.copied_state.insert((**key).clone(), state);
                }
            }
            if !scope.asynchronous {
                self.flow().set_closure((**key).clone(), closure);
            }
            if !moving.is_empty() || !copy {
                self.owning_closures.insert((**key).clone());
            }
            if !scope.consumed.is_empty() || moves_out {
                // A use that needs `Fn` or `FnMut` refuses it (E0507), which
                // is left to rustc: the lowering does not read those bounds.
                self.flow().unmodeled(
                    "a closure that moves out a capture, where `Fn` or `FnMut` is needed",
                    expression,
                );
                self.once_closures.insert((**key).clone());
            }
        }
        if moved && !scope.captures.is_empty() || !moved && !moving.is_empty() {
            let mut copied = origin::Fragment::plain("{ ");
            for name in &scope.captures {
                // A `&mut` borrow of a place is no local of its own at run
                // time: the closure holds the place's handle (`held`).
                if self
                    .aliases
                    .get(name)
                    .is_some_and(|(at, _)| scope.outer.get(name) == Some(at))
                {
                    continue;
                }
                if name == "self" {
                    if moved {
                        copied.push_text(format!("let {SELF_CAPTURE} = {SELF_CAPTURE}; "));
                    }
                } else if moving.contains(name) {
                    let take = crate::engine::KEYWORD_MOVE;
                    copied.push_text(format!("let {name} = {take}({name}); "));
                } else if moved {
                    copied.push_text(format!("let {name} = {name}; "));
                }
            }
            copied.push(source);
            copied.push_text(" }");
            source = copied;
        }
        let changed = self.self_changed.replace(outer_changed);
        if changed && self.closures.is_empty() {
            // The closure changes `self`: the statement binds its `self`
            // and writes it back after it.
            self.lend_self = true;
        } else if changed {
            self.self_changed.set(true);
        }
        if scope.captures.contains("self") && self.closures.is_empty() && !changed {
            let mut bound = origin::Fragment::plain(format!("{{ let {SELF_CAPTURE} = self; "));
            bound.push(source);
            bound.push_text(" }");
            source = bound;
        }
        let source = Self::with_held(&scope.held, source);
        Ok(self.operator_expr(source.surrounded("(", ")"), ty, expression))
    }
}
