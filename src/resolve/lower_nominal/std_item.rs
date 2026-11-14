//! Paths into std written in a body: `std::mem`'s and `std::thread`'s
//! functions, `size_of`, std's aliases through a crate's re-exports, and
//! the errors for std items Rawr doesn't have.

use super::*;

impl<'a> Parser<'a> {
    /// The type Rawr's std holds at `path` under its root, as
    /// `num::IntErrorKind`.
    pub(super) fn std_held(&self, path: &str) -> Option<Ty> {
        let path = format!("{}::{path}", crate::std_facade::ROOT);
        let definition = self.models.host.type_(&path)?.clone();
        Some(Ty::Host {
            proof: HostTyProof::new(
                definition,
                None,
                self.models.host.type_owner(&path).cloned(),
            ),
            arguments: Box::default(),
        })
    }

    /// rustc's error for a std type's bare `name` that nothing in scope
    /// binds: no generic parameter, declaration, alias or `use`, and not
    /// std's prelude (`std_path::unbound_type`). `head` when it heads a
    /// path, as `HashMap::new()`.
    pub(super) fn unbound_std_type(
        &self,
        name: &str,
        head: bool,
        reference: Span,
    ) -> Result<(), Error> {
        if self.generics.contains_key(name)
            || self.models.get(name).is_some()
            || self.models.alias(name).is_some()
            || self.models.import(name).is_some()
        {
            return Ok(());
        }
        match crate::resolve::std_path::unbound_type(name, head, reference) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// A path into std written in an expression, from its first segment
    /// `name`, as rustc resolves it (D711): `core::thread::sleep` is
    /// rustc's E0433, since core has no `thread`, and an unstable item is
    /// E0658. A local or an import of that name is the program's.
    #[inline(never)]
    pub(super) fn check_std_path(&self, name: &str, name_span: Span) -> Result<(), Error> {
        if self.models.get(name).is_some()
            || self.models.import(name).is_some()
            || self.bindings.contains_key(name)
        {
            return Ok(());
        }
        let (path, reference, next) = self.peek_path(name, name_span);
        let kind = match next {
            Some(Token::LeftParen) => "function",
            _ => "value",
        };
        let checked = crate::resolve::std_path::check(
            &path,
            crate::std_table::Namespace::Value,
            kind,
            reference,
        );
        // A std type's method Rawr doesn't run by its path is the method on
        // its first argument, which it may (`std_type_method`).
        if checked.is_err()
            && matches!(next, Some(Token::LeftParen))
            && self.std_type_method(&path, reference)
        {
            return Ok(());
        }
        checked.map(|_| ())
    }

    /// Whether `path` names a method of a std type, as
    /// `std::fmt::Formatter::write_str` does: one that takes `self`, or one
    /// the table doesn't list, as a trait impl's. It is the method on its
    /// first argument. An associated function, as `String::from`, has no
    /// receiver to be called on.
    pub(super) fn std_type_method(&self, path: &str, reference: Span) -> bool {
        let Some((owner, _)) = path.rsplit_once("::") else {
            return false;
        };
        let method = match crate::resolve::std_path::check(
            path,
            crate::std_table::Namespace::Value,
            "function",
            reference,
        ) {
            Err(_) => true,
            Ok(Some(crate::std_table::Resolved::Member {
                member: Some(member),
                ..
            })) => member.receiver().is_some(),
            Ok(_) => false,
        };
        method
            && matches!(
                crate::resolve::std_path::check(owner, crate::std_table::Namespace::Type, "type", reference),
                Ok(Some(crate::std_table::Resolved::Path(resolved))) if resolved.is_type()
            )
    }

    /// The std path `segments` name: as written when no `use` binds the
    /// first, or through the std module a `use` binds it to, as
    /// `fmt::Formatter` after `use std::fmt`. `None` for a path through any
    /// other import.
    pub(super) fn std_module_path(&self, segments: &[(ImmutableString, Span)]) -> Option<String> {
        let written = segments
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("::");
        let first = segments.first()?.0.as_str();
        let Some(binding) = self.models.import(first) else {
            return Some(written);
        };
        let root = binding.qualified.split("::").next().unwrap_or_default();
        (matches!(binding.target, item::Import::Module) && crate::std_table::is_root(root))
            .then(|| format!("{}{}", binding.qualified, &written[first.len()..]))
    }

    /// The path ahead from its first segment `name`, which is read: its
    /// segments up to a turbofish or anything else, their span, and the
    /// token after them.
    pub(super) fn peek_path(&self, name: &str, name_span: Span) -> (String, Span, Option<Token>) {
        let mut ahead = self.input.clone();
        let mut path = name.to_owned();
        let mut end = name_span.end();
        let mut next = ahead.next();
        loop {
            let Some((Token::DoubleColon, _)) = next else {
                break;
            };
            let Some((Token::Identifier(segment), position)) = ahead.next() else {
                next = None;
                break;
            };
            path.push_str("::");
            path.push_str(segment.as_str());
            end = text_end(position, segment.as_str());
            next = ahead.next();
        }
        (
            path,
            span(name_span.start(), end),
            next.map(|(token, _)| token),
        )
    }

    /// rawr's error for a use of `name`, which no local, item or import of
    /// the program's names, when it is a std item Rawr can't run yet: a
    /// path into std, a name a `use` brought in from std, or one of std's
    /// prelude (D712.1).
    pub(super) fn std_not_yet(&self, name: &str, reference: Span) -> Option<Error> {
        let first = name.split("::").next().unwrap_or(name);
        let binding = self.models.import(first);
        let qualified = crate::resolve::std_path::qualified(
            name,
            binding.map(|binding| binding.qualified.as_str()),
        )?;
        // A prelude name reads as written; another as its std path.
        let shown = if binding.is_none() && !crate::std_table::is_root(first) {
            name
        } else {
            qualified.as_str()
        };
        crate::resolve::std_path::not_yet(
            &qualified,
            shown,
            crate::std_table::Namespace::Value,
            reference,
        )
    }

    /// rustc's E0658 when a further type argument follows the `read` ones
    /// of the builtin `name` and fills an unstable parameter, as `Vec<T,
    /// A>`'s `A` does.
    pub(super) fn unstable_argument(
        &mut self,
        name: &str,
        read: usize,
        at: Span,
    ) -> Result<(), Error> {
        if !matches!(self.peek(), Token::Comma) {
            return Ok(());
        }
        match crate::resolve::std_path::unstable_argument(name, read, at) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// rawr's error for a type `name` a `use` brought in from std, which
    /// Rawr has no representation for yet (D712.1).
    pub(super) fn std_type_not_yet(&self, name: &str, reference: Span) -> Option<Error> {
        let binding = self.models.import(name)?;
        crate::resolve::std_path::not_yet(
            &binding.qualified,
            &binding.qualified,
            crate::std_table::Namespace::Type,
            reference,
        )
    }

    /// The std free function the path ahead from its first segment `name`
    /// names, by its path, through a `use` (`identity` after `use
    /// std::convert::identity;`) or from std's prelude, when rawr runs it
    /// here: called, one whose body rawr has (`Body`); named as a
    /// value, any one the facade doesn't bind, which is the closure that
    /// calls it, as a fn item coerces to one. `None` for a name the program
    /// binds, a turbofish, or any other path.
    #[inline(never)]
    pub(super) fn std_function(&self, name: &str, name_span: Span) -> Option<StdFunction> {
        if self.bindings.contains_key(name)
            || self.generics.contains_key(name)
            || self.models.get(name).is_some()
            || self.models.function(name).is_some()
        {
            return None;
        }
        let (written, reference, next) = self.peek_path(name, name_span);
        let called = matches!(next, Some(Token::LeftParen | Token::Unit));
        if matches!(next, Some(Token::DoubleColon)) {
            return None;
        }
        let binding = self.models.import(name);
        if binding.is_some_and(|binding| {
            !crate::std_table::is_root(binding.qualified.split("::").next().unwrap_or_default())
        }) {
            return None;
        }
        let qualified = crate::resolve::std_path::qualified(
            &written,
            binding.map(|binding| binding.qualified.as_str()),
        )?;
        let segments = qualified.split("::").collect::<Vec<_>>();
        let crate::std_table::Resolved::Path(path) =
            crate::std_table::resolve(&segments, crate::std_table::Namespace::Value).ok()?
        else {
            return None;
        };
        if path.kind != "fn" || path.stability != crate::std_table::Stability::Stable {
            return None;
        }
        let use_ = if called {
            Use::Call(Body::of(path.definition)?)
        } else {
            let facade = crate::resolve::std_reexport(&qualified, &self.models.host);
            self.models
                .host
                .entry(&facade)
                .is_none()
                .then_some(Use::Value)?
        };
        let declaration = crate::std_table::item(path.definition)?.declaration();
        let inputs =
            crate::std_table::split_list(crate::std_table::Declared::parse(declaration)?.inputs)
                .into_iter()
                .filter(|input| !input.trim().is_empty())
                .count();
        Some(StdFunction {
            skip: 2 * (written.split("::").count() - 1),
            written,
            reference,
            use_,
            inputs,
        })
    }

    /// The std function `function` names from its first segment, which is
    /// read: its call, or the closure that calls it (`std_function`).
    pub(super) fn parse_std_function(
        &mut self,
        function: StdFunction,
    ) -> Result<ParsedExpr, Error> {
        for _ in 0..function.skip {
            self.next();
        }
        let Use::Call(body) = function.use_ else {
            return self.function_value_closure(
                &function.written,
                function.inputs,
                None,
                function.reference,
            );
        };
        let (arguments, end) =
            self.parse_call_arguments(&format!("`{}` arguments", function.written))?;
        let call = span(function.reference.start(), end);
        match body {
            // `fn identity<T>(x: T) -> T { x }`, as core writes it.
            Body::Identity => {
                let [mut value] = <[ParsedExpr; 1]>::try_from(arguments).map_err(|arguments| {
                    Error::argument_count(
                        crate::resolve::Callee::Function,
                        function.written.clone(),
                        1,
                        arguments.len(),
                        call,
                    )
                })?;
                self.move_local(&mut value)?;
                let ty = self.inference.resolve(&value.ty);
                Ok(self.operator_expr(value.rendered_source(), ty, call))
            }
        }
    }

    /// A call of `std::mem`'s `swap`, `replace` or `take` at `name`, by
    /// its path (`std::mem::swap`, `mem::swap`) or imported (`swap`): the
    /// function and the tokens before its name.
    #[inline(never)]
    pub(super) fn mem_path(&mut self, name: &str) -> Option<(&'static str, usize)> {
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..5)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        let function = |token: &Token| match token {
            Token::Identifier(name) => ["swap", "replace", "take", "size_of", "size_of_val"]
                .into_iter()
                .find(|function| *function == name.as_str()),
            _ => None,
        };
        // `std::mem::drop(x)` and `mem::drop(x)` are the prelude's `drop`.
        let qualified = |token: &Token| match token {
            Token::Identifier(name) if name.as_str() == "drop" => Some("drop"),
            token => function(token),
        };
        // Free, or imported from `std::mem` (`use std::mem::{self, swap}`).
        let unbound = |this: &Self, name: &str| {
            this.models.get(name).is_none()
                && this.models.function(name).is_none()
                && this.models.import(name).is_none_or(|binding| {
                    let path = binding.qualified.as_str();
                    let path = path.strip_suffix(name).unwrap_or(path);
                    matches!(path, "std::" | "core::" | "std::mem::" | "core::mem::")
                })
                && !this.bindings.contains_key(name)
        };
        let mem =
            |token: &Token| matches!(token, Token::Identifier(name) if name.as_str() == "mem");
        let (function, skip) = match name {
            "std" | "core"
                if tokens[0] == Token::DoubleColon
                    && mem(&tokens[1])
                    && tokens[2] == Token::DoubleColon =>
            {
                (qualified(&tokens[3])?, 4)
            }
            "mem" if tokens[0] == Token::DoubleColon && unbound(self, name) => {
                (qualified(&tokens[1])?, 2)
            }
            name if unbound(self, name) => {
                (function(&Token::Identifier(Box::new(name.into())))?, 0)
            }
            _ => return None,
        };
        let next = if function == "size_of" {
            Token::DoubleColon
        } else {
            Token::LeftParen
        };
        (tokens[skip] == next).then_some((function, skip))
    }

    /// A call of `std::thread::sleep` at `name`, by its path or imported
    /// (`use std::thread;`, `use std::thread::sleep;`): the tokens before
    /// its name.
    #[inline(never)]
    pub(super) fn sleep_path(&mut self, name: &str) -> Option<usize> {
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..5)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        let named = |token: &Token, wanted: &str| matches!(token, Token::Identifier(name) if name.as_str() == wanted);
        // Imported from `std::thread`, and not a local item or binding.
        let imported = |this: &Self, name: &str, from: &[&str]| {
            this.models.get(name).is_none()
                && this.models.function(name).is_none()
                && !this.bindings.contains_key(name)
                && this.models.import(name).is_some_and(|binding| {
                    let path = binding.qualified.as_str();
                    from.contains(&path.strip_suffix(name).unwrap_or(path))
                })
        };
        let skip = match name {
            "std" | "core"
                if tokens[0] == Token::DoubleColon
                    && named(&tokens[1], "thread")
                    && tokens[2] == Token::DoubleColon
                    && named(&tokens[3], "sleep") =>
            {
                4
            }
            "thread"
                if tokens[0] == Token::DoubleColon
                    && named(&tokens[1], "sleep")
                    && imported(self, name, &["std::", "core::"]) =>
            {
                2
            }
            "sleep" if imported(self, name, &["std::thread::", "core::thread::"]) => 0,
            _ => return None,
        };
        matches!(tokens[skip], Token::LeftParen).then_some(skip)
    }

    /// `size_of::<T>()` and `size_of_val(&value)`: rustc's size in bytes of
    /// a number, `bool`, `char`, `()` or an array of them. Another type's
    /// layout is rustc's own choice, so it is refused.
    #[inline(never)]
    pub(super) fn parse_size_of(
        &mut self,
        function: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let ty = if function == "size_of" {
            self.expect(Token::DoubleColon, "'::' after `size_of`")?;
            self.expect(Token::LessThan, "'<' after `size_of::`")?;
            let ty = self.parse_local_type()?;
            self.expect_close_angle("'>' after `size_of`'s type")?;
            ty
        } else {
            self.inference.fresh()
        };
        let (mut arguments, end) = self.parse_call_arguments("`std::mem` arguments")?;
        let call = span(name_span.start(), end);
        let count = usize::from(function == "size_of_val");
        if arguments.len() != count {
            return Err(Error::argument_count(
                Callee::Function,
                crate::resolve::Qualified("std::mem", function).to_string(),
                count,
                arguments.len(),
                call,
            ));
        }
        let ty = match arguments.first_mut() {
            Some(argument) => {
                let expected = Ty::Reference {
                    mutable: false,
                    target: Box::new(ty.clone()),
                };
                self.coerce_expr(&expected, argument)?;
                self.inference.resolve(&ty)
            }
            None => self.inference.resolve(&ty),
        };
        let Some(size) = primitive_size(&ty) else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`std::mem::{function}` of `{}`, whose layout is rustc's own; only numbers, \
                     `bool`, `char`, `()` and arrays of them have a size in Rawr",
                    ty.name(&self.models)
                )
                .into(),
                source: call,
            });
        };
        // The argument is still evaluated, for its effects.
        let mut source = origin::Fragment::plain("{ ");
        if let Some(argument) = arguments.first() {
            source.push(argument.rendered_source());
            source.push_text("; ");
        }
        source.push_text(format!("{size} }}"));
        Ok(self.operator_expr(source, Ty::Int(Int::Usize), call))
    }

    /// `swap(&mut a, &mut b)`, `replace(&mut a, value)` (the old value) and
    /// `take(&mut a)` (the old value, `a` left as its `Default`).
    #[inline(never)]
    pub(super) fn parse_mem_call(
        &mut self,
        function: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_borrowing_arguments("`std::mem` arguments")?;
        let call = span(name_span.start(), end);
        let count = if function == "take" { 1 } else { 2 };
        if arguments.len() != count {
            return Err(Error::argument_count(
                Callee::Function,
                crate::resolve::Qualified("std::mem", function).to_string(),
                count,
                arguments.len(),
                call,
            ));
        }
        let borrowed = if function == "swap" { 2 } else { 1 };
        let places = self.mutable_arguments(&arguments, 0..borrowed)?;
        let Ty::Reference { target, .. } = self.inference.resolve(&places[0].ty) else {
            unreachable!("a `&mut` place has a reference type");
        };
        let target = *target;
        if function == "swap" {
            let expected = Ty::Reference {
                mutable: true,
                target: Box::new(target.clone()),
            };
            self.require_type(&expected, &places[1].ty, places[1].span)?;
        } else if function == "replace" {
            self.coerce_expr(&target, &mut arguments[1])?;
        }
        if matches!(target, Ty::Array { .. } | Ty::String) {
            let helper = self.push_op(Op::Prelude(PreludeOp::Memory));
            let operand = if function == "take" {
                if !self.type_provides(&target, capability::Kind::Default) {
                    return Err(Error::MissingCapability {
                        owner: target.name(&self.models),
                        capability: "Default".into(),
                        operation: call,
                        derive: matches!(target, Ty::Script(..)),
                        required_by: None,
                    });
                }
                self.default_source(&target, call)?
            } else {
                arguments[1].rendered_source()
            };
            let code = match function {
                "take" => 0,
                "replace" => 1,
                _ => 2,
            };
            let mut source = self.keyed_place(&places[0])?;
            source.push_text(format!(".{helper}({code}, "));
            source.push(operand);
            source.push_text(")");
            if function == "swap" {
                // Two native places swap where they are (`Swapped`); else
                // the second place takes the first's old value.
                let old = self.temporary("swapped");
                let swapped = self.prelude_op(PreludeOp::Swapped, [old.as_str().into()]);
                source = source.surrounded(&format!("{{ let {old} = "), "; if !");
                source.push(swapped);
                source.push_text(" { ");
                source.push(self.store_into(&places[1], &old)?);
                source.push_text(" } }");
            }
            return Ok(self.operator_expr(
                source,
                if function == "swap" { Ty::Unit } else { target },
                call,
            ));
        }
        let old = self.temporary("old");
        let mut source = origin::Fragment::plain(format!("{{ let {old} = "));
        source.push(places[0].rendered_source());
        source.push_text("; ");
        let ty = match function {
            "swap" => {
                let other = self.temporary("other");
                source.push_text(format!("let {other} = "));
                source.push(places[1].rendered_source());
                source.push_text("; ");
                source.push(self.store_into(&places[0], &other)?);
                source.push_text(" ");
                source.push(self.store_into(&places[1], &old)?);
                source.push_text(" }");
                Ty::Unit
            }
            "replace" => {
                let new = self.temporary("new");
                source.push_text(format!("let {new} = "));
                source.push(arguments[1].rendered_source());
                source.push_text("; ");
                source.push(self.store_into(&places[0], &new)?);
                source.push_text(format!(" {old} }}"));
                target
            }
            _ => {
                if !self.type_provides(&target, capability::Kind::Default) {
                    return Err(Error::MissingCapability {
                        owner: target.name(&self.models),
                        capability: "Default".into(),
                        operation: call,
                        derive: matches!(target, Ty::Script(..)),
                        required_by: None,
                    });
                }
                let new = self.temporary("new");
                let default = self.default_source(&target, call)?;
                source.push_text(format!("let {new} = "));
                source.push(default);
                source.push_text("; ");
                source.push(self.store_into(&places[0], &new)?);
                source.push_text(format!(" {old} }}"));
                target
            }
        };
        Ok(self.operator_expr(source, ty, call))
    }

    /// The path ahead, when it names a std item through a crate's re-export
    /// of it, as `serde::__private::Some` does, read as that item's own path
    /// (`std_alias`), so a pattern or an expression names the item (#593).
    pub(super) fn splice_std_alias(&mut self) {
        let start = self.position();
        let Token::Identifier(first) = self.peek() else {
            return;
        };
        if !matches!(self.peek_second(), Token::DoubleColon) {
            return;
        }
        let first = ImmutableString::from(first.as_str());
        let mut ahead = self.input.clone();
        ahead.next();
        let mut path = first.to_string();
        while let Some((Token::DoubleColon, _)) = ahead.next() {
            let Some((Token::Identifier(segment), _)) = ahead.next() else {
                break;
            };
            path.push_str("::");
            path.push_str(segment.as_str());
        }
        let Some(std) = self.std_alias(&first, &path) else {
            return;
        };
        self.next();
        for _ in 1..path.split("::").count() {
            self.next();
            self.next();
        }
        let mut tokens = Vec::new();
        for segment in std.split("::") {
            if !tokens.is_empty() {
                tokens.push((Token::DoubleColon, start));
            }
            tokens.push((Token::Identifier(Box::new(segment.into())), start));
        }
        self.splice_tokens(tokens);
    }

    /// Whether a path's first segment `root` names nothing in scope: no
    /// crate the program reaches, no `use`, item or local of that name.
    pub(super) fn unresolved_root(&self, root: &str) -> bool {
        !matches!(root, "std" | "core" | "crate" | "self" | "super")
            && self.models.host.entry(root).is_none()
            && self.models.import(root).is_none()
            && self.models.get(root).is_none()
            && self.models.function(root).is_none()
            && !self.bindings.contains_key(root)
            && self
                .models
                .crates
                .as_ref()
                .is_none_or(|crates| crates.reach(root, Span::default()).is_err())
    }

    /// The std macro the path ahead names, from its first segment `first`,
    /// which is read: `concat` of `std::concat!`, or of a crate's re-export
    /// of it, as tracing's `$crate::__macro_support::concat!`. The rest of
    /// the path is taken, so its `!` is next. `None`, and nothing taken, for
    /// a path that names no std macro.
    pub(super) fn std_macro_path(
        &mut self,
        first: &str,
        first_span: Span,
    ) -> Option<ImmutableString> {
        if !matches!(self.peek(), Token::DoubleColon) {
            return None;
        }
        let (path, _, next) = self.peek_path(first, first_span);
        if next != Some(Token::Bang) {
            return None;
        }
        let local = self.bindings.contains_key(first)
            || self.models.get(first).is_some()
            || self.models.import(first).is_some();
        // A macro is std's at its crate's root, wherever it is defined:
        // core's `format_args` is `core::macros::builtin::format_args`.
        let std = if crate::std_table::is_root(first) && !local {
            path.clone().into()
        } else {
            self.std_alias(&first.into(), &path)?
        };
        let name = std.rsplit("::").next()?;
        if !crate::resolve::std_path::is_macro(&format!("std::{name}")) {
            return None;
        }
        let name = ImmutableString::from(name);
        for _ in 0..2 * path.matches("::").count() {
            self.next();
        }
        Some(name)
    }

    /// The miss of the crate's macro path ahead, from its first segment
    /// `first` at `position`, which is read: tracing's
    /// `$crate::__macro_support::file!` inside `concat!` before the
    /// re-export is known, as an expression's path misses it, so it is
    /// asked for. `None` for a path that names something or no crate.
    pub(super) fn macro_path_miss(&mut self, first: &str, position: Position) -> Option<Error> {
        if !matches!(self.peek(), Token::DoubleColon) || self.bindings.contains_key(first) {
            return None;
        }
        let (path, path_span, next) = self.peek_path(first, point_span(position));
        if next != Some(Token::Bang) || crate::std_table::is_root(first) {
            return None;
        }
        let qualified = match self.models.import(first) {
            Some(binding) => format!("{}{}", binding.qualified, &path[first.len()..]),
            None if self.models.host.entry(first).is_some() => path,
            None => return None,
        };
        if self.models.host_items(&qualified).is_some() {
            return None;
        }
        let (module, item) = qualified.rsplit_once("::")?;
        Some(Error::NotInModule {
            module: module.into(),
            item: item.into(),
            reference: path_span,
        })
    }

    /// The std path `name`, whose first segment is `first`, names through a
    /// crate's re-export of it, when it is one.
    pub(super) fn std_alias(&self, first: &ImmutableString, name: &str) -> Option<ImmutableString> {
        if self.models.get(first).is_some() || self.bindings.contains_key(first.as_str()) {
            return None;
        }
        let qualified = match self.models.import(first) {
            Some(binding) => format!("{}{}", binding.qualified, &name[first.len()..]),
            None => name.to_owned(),
        };
        let std = self.models.host.std_alias(&qualified)?;
        if !crate::std_table::is_root(std.split("::").next()?) || *std == *qualified {
            return None;
        }
        // A variant of a std enum Rawr represents, as `std::result::Result::
        // Ok`, goes by the enum's own name: `Result::Ok`.
        let spelled = |path: &str| {
            crate::resolve::std_path::check(
                path,
                crate::std_table::Namespace::Type,
                "type",
                Span::NONE,
            )
            .ok()
            .flatten()
            .and_then(crate::resolve::std_path::spelling)
        };
        if let Some((owner, variant)) = std.rsplit_once("::")
            && let Some(name) = spelled(owner)
        {
            return Some(format!("{name}::{variant}").into());
        }
        Some(std)
    }
}

/// A std free function a path names, which rawr runs there
/// (`Parser::std_function`).
pub(super) struct StdFunction {
    /// The path as the program writes it.
    written: String,
    /// The tokens after its first segment up to its last.
    skip: usize,
    reference: Span,
    use_: Use,
    /// How many inputs std declares it with.
    inputs: usize,
}

/// How a path uses a std function rawr runs.
enum Use {
    /// Called, with the body rawr runs for it.
    Call(Body),
    /// Named as a value: the closure that calls it.
    Value,
}

/// A std function whose call rawr runs from its own body
/// (`Parser::parse_std_function`).
#[derive(Clone, Copy)]
enum Body {
    /// `core::convert::identity`.
    Identity,
}

impl Body {
    /// The body of the std function `definition`, when rawr has it.
    fn of(definition: &str) -> Option<Self> {
        match definition {
            "core::convert::identity" => Some(Self::Identity),
            _ => None,
        }
    }
}
