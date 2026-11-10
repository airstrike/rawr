//! Qualified paths: `Type::item`, `<T as Trait>::item`, an owner's
//! associated functions and an imported enum's variants.

use super::*;

impl<'a> Parser<'a> {
    /// A path expression from its first segment. One that names a std
    /// item Rawr can't run yet says so (D712.1). An unknown name in the
    /// path's arguments, as one in `thread::spawn`'s closure, is that name's
    /// error.
    pub(super) fn parse_qualified(
        &mut self,
        first: ImmutableString,
        first_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (path, reference, next) = self.peek_path(&first, first_span);
        match self.parse_qualified_path(first, first_span) {
            Err(
                error @ (Error::UnknownType { .. }
                | Error::UnknownVariable { .. }
                | Error::UnknownFunction { .. }
                | Error::UnknownMember { .. }
                | Error::NotInModule { .. }),
            ) if error.primary_span().start() <= reference.end() => {
                let expected = match next {
                    Some(Token::LeftParen) => "function",
                    _ => "value",
                };
                Err(self
                    .std_not_yet(&path, reference)
                    .or_else(|| self.macro_without_bang(&path, expected, reference))
                    .unwrap_or(error))
            }
            parsed => parsed,
        }
    }

    pub(super) fn parse_qualified_path(
        &mut self,
        first: ImmutableString,
        first_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let mut segments = vec![(first, first_span)];
        while matches!(self.peek(), Token::DoubleColon) {
            // `home::Shape::size(&x)`, `home::Shape::<i64>::convert(&x)`: a
            // crate trait's function through the trait's module path.
            if let [_, .., (last, last_span)] = &segments[..]
                && let Some(known) = self
                    .models
                    .qualified_trait(Span::new(
                        first_span.start(),
                        text_end(last_span.start(), last),
                    ))
                    .cloned()
            {
                let trait_span = *last_span;
                self.next();
                let explicit = if matches!(self.peek(), Token::LessThan) {
                    let arguments = self.parse_trait_path_arguments()?;
                    self.expect(Token::DoubleColon, "'::' after a trait's arguments")?;
                    Some(arguments)
                } else {
                    None
                };
                return self.parse_trait_path_call(
                    None,
                    known,
                    trait_span,
                    explicit,
                    first_span.start(),
                );
            }
            // `path::f::<T>(..)`: the called function's type arguments.
            if matches!(self.peek_second(), Token::LessThan) {
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
                break;
            }
            self.next();
            segments.push(self.expect_qualified_name()?);
        }
        let path_end = segments.last().map_or(first_span.end(), |(name, span)| {
            text_end(span.start(), name)
        });
        // `serde::__private::Ok(..)`: a crate's re-export of a std item is
        // that item (#593).
        if segments.len() > 1
            && let Some(std) = self.std_alias(
                &segments[0].0,
                &segments
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::"),
            )
        {
            let at = segments[0].1.start();
            let mut tokens = Vec::new();
            for segment in std.split("::") {
                if !tokens.is_empty() {
                    tokens.push((Token::DoubleColon, at));
                }
                tokens.push((Token::Identifier(Box::new(segment.into())), at));
            }
            self.splice_tokens(tokens);
            return self.parse_primary();
        }
        // `serde::__private::Formatter::write_str(..)`: a crate's re-export
        // of a std type is that type, and so is its function (#593).
        if let [owner @ .., (member, member_span)] = &segments[..]
            && owner.len() > 1
            && self.pending_turbofish.is_none()
            && let Some(std) = self.std_alias(
                &owner[0].0,
                &owner
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::"),
            )
        {
            let at = member_span.start();
            let mut tokens = Vec::new();
            for segment in std.split("::").chain([member.as_str()]) {
                if !tokens.is_empty() {
                    tokens.push((Token::DoubleColon, at));
                }
                tokens.push((Token::Identifier(Box::new(segment.into())), at));
            }
            self.splice_tokens(tokens);
            return self.parse_primary();
        }
        // `std::fmt::Formatter::write_str(f, ..)`: a std type's method by its
        // path is the method on its first argument, as rustc calls it.
        if let [owner @ .., (member, member_span)] = &segments[..]
            && owner.len() > 1
            && self.pending_turbofish.is_none()
            && matches!(self.peek(), Token::LeftParen)
            && !self.bindings.contains_key(owner[0].0.as_str())
            && self.models.get(&owner[0].0).is_none()
            && self.models.import(&owner[0].0).is_none()
            && self.std_type_method(
                &segments
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::"),
                *member_span,
            )
            && let Some(tokens) = self.path_method_call(member, *member_span, None)
        {
            self.splice_tokens(tokens);
            return self.parse_primary();
        }
        // `std::f64::consts::PI`, `f32::consts::TAU`, `consts::E`.
        {
            let written = segments
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join("::");
            let full = match self.models.import(&segments[0].0) {
                Some(binding) if segments.len() > 1 => {
                    format!("{}{}", binding.qualified, &written[segments[0].0.len()..])
                }
                _ => written,
            };
            // `tokio::join!(..)`, `try_join!` and `select!` without tokio.
            if matches!(self.peek(), Token::Bang)
                && let Some(name) = crate::resolve::tokio_macro(&full, &self.models.host)
            {
                if let Some(crates) = &self.models.crates {
                    let (written, _) = &segments[0];
                    let at = span(first_span.start(), text_end(first_span.start(), written));
                    crates.reach(full.split("::").next().unwrap_or_default(), at)?;
                }
                return self.parse_tokio_macro(name, first_span);
            }
            // `std::thread::spawn(f)` and `std::sync::mpsc::channel()`.
            let turbofish = matches!(self.peek(), Token::DoubleColon)
                && matches!(self.peek_second(), Token::LessThan);
            if matches!(self.peek(), Token::LeftParen | Token::Unit) || turbofish {
                match full.as_str() {
                    "std::thread::spawn" => return self.parse_thread_spawn(first_span.start()),
                    "std::sync::mpsc::channel" => return self.parse_channel(first_span.start()),
                    _ => {}
                }
            }
            if let Some((value, single)) = crate::resolve::float_constant(&full) {
                return Ok(self.float_constant(
                    value,
                    single,
                    Span::new(first_span.start(), path_end),
                ));
            }
            // `std::iter::once(x)` and `std::iter::empty()`: an iterator of
            // one item, or of none.
            let iterator = full
                .strip_prefix("std::iter::")
                .or_else(|| full.strip_prefix("core::iter::"));
            if let Some(member @ ("once" | "empty")) = iterator
                && matches!(self.peek(), Token::LeftParen | Token::Unit)
            {
                let (arguments, end) = self.parse_call_arguments("iterator arguments")?;
                let call = span(first_span.start(), end);
                let item = match (member, arguments.as_slice()) {
                    ("once", [value]) => value.ty.clone(),
                    ("empty", []) => self.inference.fresh(),
                    _ => {
                        return Err(Error::argument_count(
                            Callee::Function,
                            crate::resolve::Qualified("std::iter", member).to_string(),
                            usize::from(member == "once"),
                            arguments.len(),
                            call,
                        ))
                    }
                };
                let values =
                    origin::Fragment::join(arguments.iter().map(ParsedExpr::rendered_source), ", ")
                        .surrounded("[", "]");
                let values = self.operator_expr(
                    values,
                    Ty::Array {
                        element: Box::new(item.clone()),
                        kind: sequence::Kind::Array(arguments.len()),
                    },
                    call,
                );
                return Ok(self.array_iterator(&values, item, end));
            }
        }
        if let Some(spelled) = self
            .models
            .qualified_rust(Span::new(first_span.start(), path_end))
        {
            // Spelled through the app crate; every segment keeps the
            // written path's first span, the member keeps its own.
            let member = segments.last().cloned().expect("a qualified member");
            let mut respelled = spelled[..spelled.len() - 1]
                .iter()
                .map(|name| (name.clone(), first_span))
                .collect::<Vec<_>>();
            respelled.push(member);
            segments = respelled;
        }
        let (member, member_span) = segments
            .pop()
            .expect("qualified paths retain a member after their owner");
        // `Nope::<i32>`: a turbofish on a name that is no value.
        if segments.is_empty() {
            if let Some(error) = self.module_value(&member, member_span) {
                return Err(error);
            }
            if let Some(error) = self.std_not_yet(&member, member_span) {
                return Err(error);
            }
            return Err(Error::UnknownVariable {
                name: member,
                reference: member_span,
            });
        }
        let owner: ImmutableString = segments
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("::")
            .into();
        let owner_span = Span::new(
            first_span.start(),
            segments.last().map_or(first_span.end(), |(name, span)| {
                text_end(span.start(), name)
            }),
        );
        let path_span = Span::new(first_span.start(), text_end(member_span.start(), &member));
        if matches!(self.peek(), Token::LeftBrace) && !self.no_struct {
            if let Some(model) = self.models.qualified_owner(path_span).cloned() {
                return self.parse_struct_fields(member, Some(model), path_span);
            }
            // A facade struct named through an imported module or in full,
            // or an enum's struct variant through its imported type.
            let full = match self.models.import(&segments[0].0) {
                Some(binding)
                    if matches!(
                        binding.target,
                        item::Import::Module
                            | item::Import::ModuleItem { .. }
                            | item::Import::Type { .. }
                    ) =>
                {
                    format!(
                        "{}{}::{member}",
                        binding.qualified,
                        owner
                            .strip_prefix(segments[0].0.as_str())
                            .unwrap_or_default()
                    )
                }
                _ => format!("{owner}::{member}"),
            };
            if let Some(definition) = self.models.host.source_type(&full).cloned()
                && definition.identity().imported().is_none()
            {
                return self.parse_host_struct(definition, path_span);
            }
            // An imported struct named through an imported module, as
            // `pane_grid::ResizeEvent { split, ratio }`.
            #[cfg(feature = "rust-import")]
            if let Some(definition) = self.models.host.source_type(&full).cloned()
                && definition.identity().imported().is_some()
                && matches!(
                    definition.shape(),
                    crate::host::type_def::Shape::Struct {
                        form: crate::host::type_def::Form::Record,
                        ..
                    }
                )
            {
                let owner = self.models.host.type_owner(&full).cloned();
                return self.parse_imported_struct(full.into(), definition, owner, path_span);
            }
        }
        // `iced::Degrees(90.0)`: a facade tuple struct's constructor named
        // in full, which the facade declares as the type's `from`, as an
        // imported `Degrees(90.0)` reads it.
        if matches!(self.peek(), Token::LeftParen)
            && self.models.qualified_owner(path_span).is_none()
        {
            let full = match self.models.import(&segments[0].0) {
                Some(binding)
                    if matches!(
                        binding.target,
                        item::Import::Module | item::Import::ModuleItem { .. }
                    ) =>
                {
                    format!(
                        "{}{}::{member}",
                        binding.qualified,
                        &owner[segments[0].0.len()..]
                    )
                }
                _ => format!("{owner}::{member}"),
            };
            if let Some(definition) = self.models.host.source_type(&full)
                && definition.identity().imported().is_none()
                && self
                    .models
                    .host_items(&format!("{}::from", definition.path()))
                    .is_some()
            {
                let at = member_span.start();
                let mut tokens = Vec::new();
                for (name, name_span) in segments.iter().skip(1).chain([&(member, member_span)]) {
                    tokens.push((Token::DoubleColon, name_span.start()));
                    tokens.push((
                        Token::Identifier(Box::new(name.as_str().into())),
                        name_span.start(),
                    ));
                }
                tokens.push((Token::DoubleColon, at));
                tokens.push((Token::Identifier(Box::new("from".into())), at));
                self.splice_tokens(tokens);
                let (first, first_span) = segments.swap_remove(0);
                return self.parse_qualified(first, first_span);
            }
        }
        // `module::S`, `module::W(x)`, `module::W`, and `module::S` before
        // the block of a `match` or an `if let`, where no struct literal
        // stands.
        if !matches!(self.peek(), Token::DoubleColon)
            && (self.no_struct || !matches!(self.peek(), Token::LeftBrace))
            && let Some(model) = self.models.qualified_owner(path_span).cloned()
            && let Some(value) = self.parse_struct_value(model, path_span)?
        {
            return Ok(value);
        }
        if let Some(function) = self.models.qualified_function(path_span).cloned() {
            if function.definition.body.constant {
                return Ok(self.constant_use(&function, path_span));
            }
            if !matches!(self.peek(), Token::LeftParen | Token::Unit) {
                return Ok(self.function_value(&function, path_span));
            }
            return self.parse_script_call(member, Some(function), path_span);
        }
        // `Self::NAME` in a trait's default body names the impl's owner.
        let qualified_model = self
            .models
            .qualified_owner(owner_span)
            .cloned()
            .or_else(|| {
                let (owner_id, _) = self.default_trait.as_ref()?;
                (owner.as_str() == "Self").then(|| self.models.definition(*owner_id).clone())
            })
            .or_else(|| {
                self.named_model(&owner)
                    .filter(|_| owner.as_str() == "Self")
            });
        // `C::NAME` and `C::new(..)` where the generic parameter `C` is the
        // type this body takes it at: an instance compiled for one
        // instantiation (D629), or a trait's default body in an impl. The
        // item is that type's, as rustc resolves it through the bound.
        let qualified_model = match self.parameter_type(&segments, &owner) {
            Some(Ty::Script(id, _)) if qualified_model.is_none() => {
                Some(self.models.definition(id).clone())
            }
            Some(ty) if qualified_model.is_none() => {
                let Some(definition) = owner_id(&ty)
                    .and_then(|id| self.models.method(id, &member))
                    .cloned()
                else {
                    return Err(Error::UnknownMember {
                        owner: ty.name(&self.models).to_string().into(),
                        member,
                        reference: member_span,
                    });
                };
                return self.parse_owner_function(ty, definition, owner_span, member, member_span);
            }
            _ => qualified_model,
        };

        // `iced::Color::from(value)` through the crate's `impl From<T> for
        // iced::Color`, when the facade has no `from` of its own.
        if qualified_model.is_none()
            && member.as_str() == "from"
            && matches!(self.peek(), Token::LeftParen)
        {
            let full = match self.models.import(&segments[0].0) {
                Some(binding) => format!("{}{}", binding.qualified, &owner[segments[0].0.len()..]),
                None => owner.to_string(),
            };
            if let Some(definition) = self.models.host.type_(&full).cloned()
                && self.models.host_items(&format!("{full}::from")).is_none()
            {
                let target = type_def::Id::foreign(&definition.path());
                // `Element::from(widget)`: a widget has no `From` conversion
                // into the type its facade trait erases to, as in rustc.
                if !self.models.conversions.contains_key(&target)
                    && self.erasure_types().iter().any(|erased| {
                        matches!(erased, Ty::Host { proof, .. } if proof.definition.path() == definition.path())
                    })
                {
                    let (arguments, end) = self.parse_call_arguments("`from` arguments")?;
                    let call = span(owner_span.start(), end);
                    if let [argument] = &arguments[..] {
                        let source = self.inference.resolve(&argument.ty);
                        if let Some(erased) = self.widget_erasure(&source) {
                            return Err(self.no_widget_conversion(&source, &erased, "from", call));
                        }
                    }
                    return Err(Error::UnknownMember {
                        owner: full.into(),
                        member,
                        reference: member_span,
                    });
                }
                if self.models.conversions.contains_key(&target) {
                    let (mut arguments, end) = self.parse_call_arguments("`from` arguments")?;
                    let call = span(owner_span.start(), end);
                    let [argument] = arguments.as_mut_slice() else {
                        return Err(Error::argument_count(
                            Callee::AssociatedFunction,
                            crate::resolve::Qualified(&full, &member).to_string(),
                            1,
                            arguments.len(),
                            call,
                        ));
                    };
                    let source = self.inference.resolve(&argument.ty);
                    let Some((conversion, from, owner)) =
                        self.select_conversion(target, None, &source, call)?
                    else {
                        return Err(Error::MissingCapability {
                            owner: full.into(),
                            capability: format!("From<{}>", self.fallback_name(&source)).into(),
                            operation: call,
                            derive: false,
                            required_by: None,
                        });
                    };
                    self.coerce_expr(&from, argument)?;
                    return Ok(self.operator_expr(
                        origin::Fragment::call(
                            method_name(conversion.id()),
                            [argument.rendered_source()],
                        ),
                        owner,
                        call,
                    ));
                }
            }
        }

        let imported_owner = (qualified_model.is_none() && segments.len() == 1)
            .then(|| self.models.import(&owner).cloned())
            .flatten()
            // A crate's type alias of a crate type, as `Element::new`
            // through `use iced::Element;`, is that type's.
            .map(|binding| match &binding.target {
                item::Import::Alias => match self
                    .models
                    .alias(&owner)
                    .and_then(|alias| alias.target.as_deref().cloned())
                {
                    Some(type_ref::Type::Path(type_ref::Path {
                        target: type_ref::Target::Host { definition, owner },
                        ..
                    })) => item::Binding {
                        name: binding.name,
                        qualified: definition.path().as_str().into(),
                        target: item::Import::Type { definition, owner },
                    },
                    _ => binding,
                },
                _ => binding,
            })
            .filter(|binding| {
                matches!(
                    &binding.target,
                    item::Import::Type { .. }
                        | item::Import::Module
                        | item::Import::ModuleItem { .. }
                )
            });
        if let Some(binding) = imported_owner {
            #[cfg(feature = "rust-import")]
            if let item::Import::Type { definition, owner } = &binding.target
                && let Some(value) = self.imported_to_string_value(
                    definition,
                    owner.clone(),
                    &member,
                    owner_span,
                    member_span,
                )?
            {
                return Ok(value);
            }
            #[cfg(feature = "rust-import")]
            if let item::Import::Type { definition, owner } = &binding.target
                && names_variant(definition, &member)
            {
                return self.parse_imported_variant(
                    binding.qualified,
                    definition.clone(),
                    owner.clone(),
                    member,
                    owner_span,
                    member_span,
                );
            }
            let path = format!("{}::{member}", binding.qualified);
            // `Trait::method(receiver, ..)` through a crate's trait a `use`
            // names (`use iced::widget::Widget;` then `Widget::_boxed(w)`),
            // which Rawr names as a module: the method called on its
            // receiver, as through the trait's whole path.
            let crate_trait = self.models.host_items(&path).is_none()
                && matches!(binding.target, item::Import::Module)
                && binding
                    .qualified
                    .rsplit("::")
                    .next()
                    .and_then(|name| name.chars().next())
                    .is_some_and(char::is_uppercase);
            // A trait's function with no receiver, of the one type parameter
            // native code fills that has it (`opaque_function_owner`).
            #[cfg(feature = "rust-import")]
            if crate_trait
                && matches!(self.peek(), Token::LeftParen)
                && let Some(opaque) = self.opaque_function_owner(&binding.qualified, &member)
            {
                self.pending_turbofish = None;
                return self.opaque_function_call(&opaque, &member, member_span);
            }
            if crate_trait
                && matches!(self.peek(), Token::LeftParen)
                && let Some(tokens) =
                    self.path_method_call(&member, member_span, Some(binding.qualified.clone()))
            {
                self.pending_turbofish = None;
                self.splice_tokens(tokens);
                return self.parse_primary();
            }
            // `.map(Tone::tone)`: the closure that calls the method on its
            // first input, whose type's impl of the trait gives it. Its
            // inputs are the ones its use gives it, the receiver first.
            if crate_trait
                && !matches!(self.peek(), Token::LeftParen | Token::Unit)
                && member.starts_with(|c: char| c.is_ascii_lowercase())
            {
                let arity = self
                    .closure_hint
                    .as_ref()
                    .map_or(1, |hint| hint.len().max(1));
                let inputs = (0..arity).map(|_| self.inference.fresh()).collect();
                return self.method_value(member, inputs, owner_span, member_span);
            }
            // `badge::Secondary` after `use components::badge;`: the
            // crate's struct at the module's path, as a value.
            #[cfg(feature = "rust-import")]
            if self.models.host_items(&path).is_none()
                && matches!(
                    binding.target,
                    item::Import::Module | item::Import::ModuleItem { .. }
                )
                && let Some(value) = self.imported_struct_path(
                    &path,
                    &[(&segments[0].0, segments[0].1), (&member, member_span)],
                    path_span,
                )?
            {
                return Ok(value);
            }
            // `layout::Center` after `use iced::layout;`, of the module's
            // `pub use Alignment::Center;`.
            #[cfg(feature = "rust-import")]
            if self.models.host_items(&path).is_none()
                && matches!(
                    binding.target,
                    item::Import::Module | item::Import::ModuleItem { .. }
                )
                && let Some(value) = self.reexported_variant(&path, owner_span, member_span)?
            {
                return Ok(value);
            }
            let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec) else {
                #[cfg(feature = "rust-import")]
                if let Some(error) = self.pending_family_call(&path, &member, member_span) {
                    return Err(error);
                }
                #[cfg(feature = "rust-import")]
                let native = self.models.host.entry(&path).is_none()
                    && (matches!(&binding.target, item::Import::Type { definition, .. } if definition.identity().imported().is_some())
                        || self
                            .native_site()
                            .is_some_and(|site| site.missing_binding(&self.models.host, &path)));
                // A module a `use` names lacks only the item, as `fonts::
                // UI_FAMILY` after `use common::fonts;`: rustc's E0425 for
                // the module, not a type's E0599 (#1146). A crate's trait,
                // which Rawr names as a module, is a type.
                let error = if matches!(binding.target, item::Import::Module) && !crate_trait {
                    Error::NotInModule {
                        module: binding.qualified,
                        item: member,
                        reference: member_span,
                    }
                } else {
                    Error::UnknownMember {
                        owner: binding.qualified,
                        member,
                        reference: member_span,
                    }
                };
                #[cfg(feature = "rust-import")]
                let error = self.native_miss(native, error);
                // A crate's path call, as a macro's `$crate::core::Widget::
                // _boxed(x)`: the pass goes on to the body's next miss.
                #[cfg(feature = "rust-import")]
                return self.recover_miss(member_span.start(), |_| Err(error));
                #[cfg(not(feature = "rust-import"))]
                return Err(error);
            };
            let item_owner = self.models.item_owner(&path).cloned();
            let items = macro_or_value(items, matches!(self.peek(), Token::Bang));
            let items = value_or_call(items, matches!(self.peek(), Token::LeftParen | Token::Unit));
            return if items.first().is_some_and(|item| item.kind().is_macro())
                && matches!(self.peek(), Token::Bang)
            {
                self.parse_host_macro(
                    items.into_boxed_slice(),
                    item_owner,
                    span(owner_span.start(), member_span.end()),
                )
            } else if items.len() == 1
                && matches!(
                    items[0].kind(),
                    crate::host::item::Kind::AssociatedValue | crate::host::item::Kind::Value
                )
                && !matches!(self.peek(), Token::LeftParen | Token::Unit)
            {
                self.parse_host_value(
                    items[0].clone(),
                    item_owner,
                    span(owner_span.start(), member_span.end()),
                )
            } else {
                self.parse_host_function_or_item(items.into_boxed_slice(), item_owner, path_span)
            };
        }

        // A facade function named in full or through an imported module:
        // `iced::widget::slider::default(..)`, `widget::slider::default(..)`,
        // and a facade root's own item, as `iced::Center`.
        if qualified_model.is_none()
            && (segments.len() > 1 || self.models.import(&segments[0].0).is_none())
        {
            let path = match self.models.import(&segments[0].0) {
                Some(binding)
                    if matches!(
                        binding.target,
                        item::Import::Module | item::Import::ModuleItem { .. }
                    ) =>
                {
                    format!(
                        "{}{}::{member}",
                        binding.qualified,
                        &owner[segments[0].0.len()..]
                    )
                }
                // `std::time::Duration::from_millis` is the facade's where
                // its `time` re-exports std's, as `use std::time::Duration`
                // is (`resolve::std_reexport`).
                _ => crate::resolve::std_reexport(&format!("{owner}::{member}"), &self.models.host)
                    .to_string(),
            };
            if let Some(items) = self
                .models
                .host_items(&path)
                .filter(|items| items.iter().all(|item| item.imported_identity().is_none()))
                .map(<[_]>::to_vec)
            {
                let item_owner = self.models.item_owner(&path).cloned();
                let items = macro_or_value(items, matches!(self.peek(), Token::Bang));
                let items =
                    value_or_call(items, matches!(self.peek(), Token::LeftParen | Token::Unit));
                return if items.first().is_some_and(|item| item.kind().is_macro())
                    && matches!(self.peek(), Token::Bang)
                {
                    self.parse_host_macro(items.into_boxed_slice(), item_owner, path_span)
                } else if items.len() == 1
                    && matches!(
                        items[0].kind(),
                        crate::host::item::Kind::AssociatedValue | crate::host::item::Kind::Value
                    )
                    && !matches!(self.peek(), Token::LeftParen | Token::Unit)
                {
                    self.parse_host_value(items[0].clone(), item_owner, path_span)
                } else {
                    self.parse_host_function_or_item(
                        items.into_boxed_slice(),
                        item_owner,
                        path_span,
                    )
                };
            }
            // A crate's struct named in full or through a nested module, as
            // `natives::badge::Secondary`.
            #[cfg(feature = "rust-import")]
            if self.models.host_items(&path).is_none() {
                let written = segments
                    .iter()
                    .map(|(name, name_span)| (name, *name_span))
                    .chain([(&member, member_span)])
                    .collect::<Vec<_>>();
                if let Some(value) = self.imported_struct_path(&path, &written, path_span)? {
                    return Ok(value);
                }
                if let Some(value) = self.reexported_variant(&path, owner_span, member_span)? {
                    return Ok(value);
                }
            }
        }

        // Whether `owner` is a path through a crate's module a `use`
        // binds: what it names, or misses, is the crate's by that path.
        #[cfg(feature = "rust-import")]
        let through_module = qualified_model.is_none()
            && segments.len() > 1
            && self.models.import(&segments[0].0).is_some_and(|binding| {
                matches!(
                    binding.target,
                    item::Import::Module | item::Import::ModuleItem { .. }
                ) && matches!(
                    self.models.host.entry(&binding.qualified),
                    Some(crate::host::Entry::Module | crate::host::Entry::ModuleItem(_))
                )
            });
        // An imported type named through an imported module, as
        // `pane_grid::Axis` after `use iced::widget::pane_grid;`, is the
        // type at the module's path.
        #[cfg(feature = "rust-import")]
        let owner: ImmutableString = match self.models.import(&segments[0].0) {
            Some(binding)
                if qualified_model.is_none()
                    && segments.len() > 1
                    && matches!(
                        binding.target,
                        item::Import::Module | item::Import::ModuleItem { .. }
                    ) =>
            {
                let through = format!("{}{}", binding.qualified, &owner[segments[0].0.len()..]);
                if self.models.host.type_(&through).is_some() {
                    through.into()
                } else if let Some(crate::host::Entry::TypeAlias(alias)) =
                    self.models.host.entry(&through)
                    && let Some(type_ref::Alias {
                        target: Some(target),
                        ..
                    }) = type_ref::Alias::host(alias, &self.models.host, owner_span)
                    && let type_ref::Type::Path(type_ref::Path {
                        target: type_ref::Target::Host { definition, .. },
                        ..
                    }) = *target
                {
                    // A crate's alias of a crate type through the module,
                    // as the_matrix's `canvas::Cache::with_group`: that
                    // type's, as `Cache::with_group` is after `use` (T327).
                    definition.path().as_str().into()
                } else if through_module {
                    // A module nested under the imported one, at any depth,
                    // as `operation::scrollable::snap_to` after `use
                    // iced::widget::operation;` (#851): the path through
                    // the module, as rustc resolves it.
                    through.into()
                } else {
                    owner
                }
            }
            _ => owner,
        };
        #[cfg(feature = "rust-import")]
        let imported_nominal_owner = if qualified_model.is_none() && segments.len() > 1 {
            self.models
                .host
                .type_(&owner)
                .filter(|definition| definition.identity().imported().is_some())
                .cloned()
                .zip(self.models.host.type_owner(&owner).cloned())
        } else {
            None
        };
        // A generic owner (`Rgb::new`) takes its arguments from the call,
        // which carries them; it needs no declaration of its own.
        #[cfg(feature = "rust-import")]
        if let Some((definition, _)) = &imported_nominal_owner
            && definition.parameters().is_empty()
        {
            let selector = definition
                .imported_declaration()
                .expect("the owner was kept only as a crate's type");
            self.push_imported_op(&owner, &selector, None, owner_span)?;
        }

        #[cfg(feature = "rust-import")]
        if qualified_model.is_none() {
            let path = format!("{owner}::{member}");
            // Without `!`, a path names the value namespace's items, as
            // rustc's: the function a macro shares its name with.
            let bang = matches!(self.peek(), Token::Bang);
            let called = matches!(self.peek(), Token::LeftParen | Token::Unit);
            if let Some(items) = self.models.host_items(&path).map(|items| {
                let items = items.to_vec();
                if bang {
                    items
                } else {
                    value_or_call(macro_or_value(items, false), called)
                }
            }) && !items.is_empty()
                && items.iter().all(|item| item.imported_identity().is_some())
            {
                let item_owner = self.models.item_owner(&path).cloned();
                if items.len() == 1
                    && matches!(
                        items[0].kind(),
                        crate::host::item::Kind::AssociatedValue | crate::host::item::Kind::Value
                    )
                    && !matches!(self.peek(), Token::LeftParen | Token::Unit)
                {
                    return self.parse_host_value(items[0].clone(), item_owner, path_span);
                }
                return self.parse_host_function_or_item(
                    items.into_boxed_slice(),
                    item_owner,
                    path_span,
                );
            }
        }

        #[cfg(feature = "rust-import")]
        if let Some((definition, facade)) = imported_nominal_owner {
            if let Some(value) = self.imported_to_string_value(
                &definition,
                Some(facade.clone()),
                &member,
                owner_span,
                member_span,
            )? {
                return Ok(value);
            }
            if names_variant(&definition, &member) {
                return self.parse_imported_variant(
                    owner,
                    definition,
                    Some(facade),
                    member,
                    owner_span,
                    member_span,
                );
            }
            let path = format!("{owner}::{member}");
            let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec) else {
                if let Some(error) = self.pending_family_call(&path, &member, member_span) {
                    return Err(error);
                }
                let error = Error::UnknownMember {
                    owner,
                    member,
                    reference: member_span,
                };
                return Err(self.native_miss(self.models.host.entry(&path).is_none(), error));
            };
            let item_owner = self.models.item_owner(&path).cloned();
            let items = macro_or_value(items, matches!(self.peek(), Token::Bang));
            let items = value_or_call(items, matches!(self.peek(), Token::LeftParen | Token::Unit));
            return if items.first().is_some_and(|item| item.kind().is_macro())
                && matches!(self.peek(), Token::Bang)
            {
                self.parse_host_macro(
                    items.into_boxed_slice(),
                    item_owner,
                    span(owner_span.start(), member_span.end()),
                )
            } else if items.len() == 1
                && matches!(
                    items[0].kind(),
                    crate::host::item::Kind::AssociatedValue | crate::host::item::Kind::Value
                )
                && !matches!(self.peek(), Token::LeftParen | Token::Unit)
            {
                self.parse_host_value(
                    items[0].clone(),
                    item_owner,
                    span(owner_span.start(), member_span.end()),
                )
            } else {
                self.parse_host_function_or_item(items.into_boxed_slice(), item_owner, path_span)
            };
        }

        let Some(model) = qualified_model.or_else(|| {
            (segments.len() == 1)
                .then(|| self.models.get(&owner).cloned())
                .flatten()
        }) else {
            #[cfg(feature = "rust-import")]
            let native = segments.first().is_some_and(|(root, _)| {
                self.native_site().is_some_and(|site| {
                    site.missing_binding(&self.models.host, &format!("{owner}::{member}"))
                }) && (through_module || self.models.import(root).is_none())
                    && self.models.get(root).is_none()
                    && self.models.function(root).is_none()
                    && !self.bindings.contains_key(root)
            });
            // `de::Error::invalid_length(..)`, a trait's function that takes
            // no receiver, where one type parameter native code fills, or
            // one of its associated types, has it: that type's, as rustc
            // infers `Self` from the one type in scope it can be (#593).
            #[cfg(feature = "rust-import")]
            if matches!(self.peek(), Token::LeftParen)
                && let Some(opaque) = self.opaque_function_owner(&owner, &member)
            {
                self.pending_turbofish = None;
                return self.opaque_function_call(&opaque, &member, member_span);
            }
            // `Trait::method(receiver, ..)` through a crate's trait, which
            // Rawr names as a module (`iced::advanced::Widget::_boxed(w)`,
            // as a macro writes it): the method called on its receiver.
            if matches!(
                self.models.host.entry(&owner),
                Some(crate::host::Entry::Module | crate::host::Entry::ModuleItem(_))
            ) && owner
                .rsplit("::")
                .next()
                .and_then(|name| name.chars().next())
                .is_some_and(char::is_uppercase)
                && matches!(self.peek(), Token::LeftParen)
                && let Some(tokens) =
                    self.path_method_call(&member, member_span, Some(owner.clone()))
            {
                self.pending_turbofish = None;
                self.splice_tokens(tokens);
                return self.parse_primary();
            }
            #[cfg(feature = "rust-import")]
            if let Some(error) =
                self.pending_family_call(&format!("{owner}::{member}"), &member, member_span)
            {
                return Err(error);
            }
            // A library module or type that exists lacks only the member:
            // `iced::widget::Column::missing`. A module is rustc's E0425,
            // `fount::google::load` naming no item of `fount::google`; a
            // crate's trait, which Rawr also names as a module, is a type.
            let error = if matches!(
                self.models.host.entry(&owner),
                Some(crate::host::Entry::Module | crate::host::Entry::ModuleItem(_))
            ) && owner
                .rsplit("::")
                .next()
                .and_then(|name| name.chars().next())
                .is_some_and(|first| first.is_lowercase() || first == '_')
            {
                Error::NotInModule {
                    module: owner,
                    item: member.clone(),
                    reference: member_span,
                }
            } else if self.models.host.entry(&owner).is_some() {
                Error::UnknownMember {
                    owner,
                    member: member.clone(),
                    reference: member_span,
                }
            } else if let [(root, root_span), _, ..] = &segments[..]
                && self.unresolved_root(root)
            {
                // rustc stops at the first segment it can't resolve: a
                // crate the program doesn't depend on, as `tokio` in
                // `tokio::task::spawn_local(..)` without tokio (#1149).
                Error::UnknownType {
                    name: root.clone(),
                    reference: span(root_span.start(), text_end(root_span.start(), root)),
                    path: true,
                }
            } else {
                Error::UnknownType {
                    name: owner,
                    reference: owner_span,
                    path: true,
                }
            };
            // A crate's module path no binding names, as a macro's
            // `$crate::module::function` is: the whole path is unresolved,
            // so the importer is asked for all of it.
            #[cfg(feature = "rust-import")]
            let crate_path = through_module
                || segments.first().is_some_and(|(root, _)| {
                    matches!(
                        self.models.host.entry(root),
                        Some(crate::host::Entry::Module | crate::host::Entry::ModuleItem(_))
                    ) && self.models.import(root).is_none()
                        && self.models.get(root).is_none()
                        && self.models.function(root).is_none()
                        && !self.bindings.contains_key(root)
                });
            #[cfg(feature = "rust-import")]
            let error = match error {
                Error::UnknownType { name, .. }
                    if crate_path
                        && name
                            .rsplit("::")
                            .next()
                            .and_then(|segment| segment.chars().next())
                            .is_some_and(|first| first.is_lowercase() || first == '_') =>
                {
                    Error::UnknownImport {
                        path: format!("{name}::{member}").into(),
                        reference: span(owner_span.start(), member_span.end()),
                        missing: Unresolved::Path,
                    }
                }
                error => error,
            };
            #[cfg(feature = "rust-import")]
            let error = self.native_miss(native, error);
            // A crate's path call, as a macro's `$crate::core::Widget::
            // _boxed(x)`: the pass goes on to the body's next miss.
            #[cfg(feature = "rust-import")]
            return self.recover_miss(owner_span.start(), |_| Err(error));
            #[cfg(not(feature = "rust-import"))]
            return Err(error);
        };

        // `Owner::from(value)` through the `impl From<T> for Owner` that
        // takes the value; an inherent `from` comes first, as in Rust.
        if member == "from"
            && matches!(self.peek(), Token::LeftParen)
            && self
                .models
                .method(model.definition().id(), &member)
                .is_none()
            && self
                .models
                .conversions
                .contains_key(&model.definition().id())
        {
            let target = model.definition().id();
            let (mut arguments, end) = self.parse_call_arguments("`from` arguments")?;
            let call = span(owner_span.start(), end);
            let [argument] = arguments.as_mut_slice() else {
                return Err(Error::argument_count(
                    Callee::AssociatedFunction,
                    crate::resolve::Qualified(model.name(), &member).to_string(),
                    1,
                    arguments.len(),
                    call,
                ));
            };
            let source = self.inference.resolve(&argument.ty);
            let Some((definition, from, owner)) =
                self.select_conversion(target, None, &source, call)?
            else {
                return Err(Error::MissingCapability {
                    owner: model.name().into(),
                    capability: format!("From<{}>", self.fallback_name(&source)).into(),
                    operation: call,
                    derive: true,
                    required_by: None,
                });
            };
            self.coerce_expr(&from, argument)?;
            return Ok(self.operator_expr(
                origin::Fragment::call(method_name(definition.id()), [argument.rendered_source()]),
                owner,
                call,
            ));
        }
        // `Owner::from` as a value where several `impl From` give it: the
        // closure `|value| value.into()` into `Owner`, whose use selects
        // the impl by the value's type, as rustc infers `From`'s argument.
        if member == "from"
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && self
                .models
                .method(model.definition().id(), &member)
                .is_none()
            && self
                .models
                .conversions
                .get(&model.definition().id())
                .is_some_and(|conversions| conversions.len() > 1)
        {
            let at = member_span.start();
            let argument = || (Token::Identifier(Box::new("__rawr_from_value".into())), at);
            self.splice_tokens(vec![
                argument(),
                (Token::Pipe, at),
                argument(),
                (Token::Period, at),
                (Token::Identifier(Box::new("into".into())), at),
                (Token::Unit, at),
            ]);
            let closure = self.parse_closure(
                Token::Pipe,
                span(owner_span.start(), member_span.end()),
                false,
            )?;
            if let Ty::Fn { output, .. } = self.inference.resolve(&closure.ty) {
                self.unify_type(&output, &model.ty());
            }
            return Ok(closure);
        }
        // `C::make(x)` where impls of one generic trait for several of its
        // instances give `make`: the arguments pick one, by inference.
        if matches!(self.peek(), Token::LeftParen | Token::Unit)
            && self
                .models
                .method(model.definition().id(), &member)
                .is_none()
            && let Some((trait_name, function)) =
                self.generic_trait_method(model.definition().id(), &member)
            && function.receiver.is_none()
        {
            let owner = model.definition().id();
            let (arguments, end) = self.parse_call_arguments("associated function arguments")?;
            return self.call_static_trait_function(
                model.ty(),
                origin::Fragment::plain(owner_key(owner)),
                &trait_name,
                &function,
                arguments,
                span(owner_span.start(), end),
            );
        }
        if let Some(definition) = self
            .models
            .method(model.definition().id(), &member)
            .cloned()
        {
            return self.parse_owner_function(
                model.ty(),
                definition,
                owner_span,
                member,
                member_span,
            );
        }

        // `Type::to_string` through a hand-written `Display`.
        if member == "to_string"
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && self
                .display_text(&model.ty(), origin::Fragment::default())
                .is_some()
        {
            let receiver = Ty::Reference {
                mutable: false,
                target: Box::new(model.ty()),
            };
            return self.method_value(member, Box::new([receiver]), owner_span, member_span);
        }

        // `Type::default` as a value, as `iced::application(State::default,
        // ..)` takes it: the closure `|| Type::default()`, which a derived
        // `Default` has no function of its own for.
        if member == "default" && !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            let mut path = segments
                .iter()
                .map(|(segment, _)| segment.as_str())
                .collect::<Vec<_>>()
                .join("::");
            if !path.ends_with("::default") {
                path.push_str("::default");
            }
            return self.function_value_closure(
                &path,
                0,
                None,
                span(owner_span.start(), member_span.end()),
            );
        }
        if member == "default" {
            let end = if matches!(self.peek(), Token::Unit) {
                self.next().1
            } else {
                self.expect(Token::LeftParen, "'(' after default")?;
                self.expect(Token::RightParen, "')' after default")?
            };
            // A generic type's `default()` has fresh arguments, which its
            // use settles, as `let w: W<i64> = W::default()` does.
            let ty = if model.generics().is_empty() {
                model.ty()
            } else {
                self.fresh_nominal(&model, member_span).0
            };
            let source = self.default_source(&ty, member_span)?;
            return Ok(ParsedExpr {
                prelude: origin::Fragment::default(),
                source,
                ty,
                span: span(owner_span.start(), end),
                lvalue: None,
                outcome: crate::host::usage::Outcome::Conservative,
                evaluation: crate::host::usage::Evaluation::Required,
            }
            .retain_origin());
        }

        let owner_id = model.definition().id();
        let Nominal::Enum { definition, item } = model else {
            return Err(self
                .private_member(owner_id, &member, member_span)
                .unwrap_or(Error::UnknownMember {
                    owner,
                    member,
                    reference: member_span,
                }));
        };
        let Some((variant, declaration)) = item
            .variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name.text == member)
        else {
            return Err(self
                .private_member(owner_id, &member, member_span)
                .unwrap_or_else(|| Error::UnknownMember {
                    owner: definition.name().into(),
                    member,
                    reference: member_span,
                }));
        };
        // `E::V { x, y: 1 }` of a struct variant.
        if let Some(fields) = &declaration.fields {
            if !matches!(self.peek(), Token::LeftBrace) || self.no_struct {
                return Err(Error::InvalidConstruction {
                    owner: definition.name().into(),
                    reason: format!(
                        "`{}` is a struct variant, built with `{{ .. }}`",
                        declaration.name.text
                    )
                    .into(),
                    construction: span(owner_span.start(), member_span.end()),
                });
            }
            let (enum_ty, substitution) = self.fresh_nominal(
                &Nominal::Enum {
                    definition: definition.clone(),
                    item: item.clone(),
                },
                span(owner_span.start(), member_span.end()),
            );
            let fields = fields
                .iter()
                .zip(declaration.payload.iter())
                .map(|(field, ty)| {
                    (
                        field.text.clone(),
                        instantiate(&Ty::from_resolved(ty), &substitution),
                    )
                })
                .collect::<Vec<_>>();
            let helper = self.push_op(Op::Enum {
                definition: definition.clone(),
                item,
                variant,
            });
            let (source, end) = self.parse_named_values(&definition, &fields, &helper)?;
            return Ok(self.operator_expr(source, enum_ty, span(owner_span.start(), end)));
        }
        if !matches!(self.peek(), Token::LeftParen | Token::Unit)
            && !declaration.payload.is_empty()
            && (declaration.payload.len() > 1
                || !item.generics.is_empty()
                || exact_symbolic_type(&declaration.payload[0]).is_none())
        {
            // `Shape::Rect` as a value: a function from its payload, as in Rust.
            return Ok(self.variant_function(
                &definition,
                &item,
                variant,
                span(owner_span.start(), member_span.end()),
            ));
        }
        if !matches!(self.peek(), Token::LeftParen | Token::Unit) && declaration.payload.len() == 1
        {
            let owner_identity = type_def::Identity::Script(definition.key().clone());
            let input = exact_symbolic_type(&declaration.payload[0])
                .expect("variants without a symbolic payload are functions above");
            let value = crate::item::Value::new(
                crate::item::Key::associated(
                    definition.key(),
                    declaration.name.text.clone(),
                    crate::item::Kind::VariantConstructor,
                ),
                crate::item::Kind::VariantConstructor,
                Some(owner_identity.clone()),
                None,
                crate::signature::Fingerprint::variant(&declaration.payload, owner_identity),
                definition.capabilities().fingerprint(),
            );
            return Ok(self.symbolic_expr(
                value,
                Box::new([input]),
                Ty::Script(definition.id(), Box::default()),
                span(owner_span.start(), member_span.end()),
                Vec::new(),
            ));
        }
        let mut args = Vec::new();
        let end = if matches!(self.peek(), Token::Unit) {
            self.next().1
        } else if matches!(self.peek(), Token::LeftParen) {
            self.next();
            while !matches!(self.peek(), Token::RightParen) {
                args.push(self.parse_expr()?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
            self.expect(Token::RightParen, "')' after variant payload")?
        } else {
            member_span.end()
        };
        if args.len() != declaration.payload.len() {
            return Err(Error::argument_count(
                Callee::Variant,
                crate::resolve::Qualified(definition.name(), &declaration.name.text).to_string(),
                declaration.payload.len(),
                args.len(),
                span(owner_span.start(), end),
            ));
        }
        let (enum_ty, substitution) = self.fresh_nominal(
            &Nominal::Enum {
                definition: definition.clone(),
                item: item.clone(),
            },
            span(owner_span.start(), end),
        );
        for (value, expected) in args.iter_mut().zip(declaration.payload.iter()) {
            self.coerce_expr(
                &instantiate(&Ty::from_resolved(expected), &substitution),
                value,
            )?;
            self.move_local(value)?;
        }
        let parts: Vec<_> = args
            .iter()
            .enumerate()
            .map(|(slot, value)| {
                (
                    value,
                    borrows::Slot::Field(
                        definition.id(),
                        Some(declaration.name.text.as_str()),
                        slot,
                    ),
                )
            })
            .collect();
        self.compose(span(owner_span.start(), end), &enum_ty, &parts);
        let helper = self.push_op(Op::Enum {
            definition: definition.clone(),
            item,
            variant,
        });
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::call(&helper, args.iter().map(ParsedExpr::rendered_source)),
            ty: enum_ty,
            span: span(owner_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// `Type::function(..)`, `Type::method` as a value or `Type::NAME`, of
    /// the function `definition` of an impl for `owner_ty`: a script type's,
    /// or a crate type's that a script impl gives a function.
    pub(super) fn parse_owner_function(
        &mut self,
        owner_ty: Ty,
        definition: method::Definition,
        owner_span: Span,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // `Type::method` as a value: `fn(&Type, ..) -> Output`, or
        // `fn(&mut Type, ..)` for a `&mut self` method.
        if let Some(mode) = definition.receiver()
            && !definition.constant()
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
        {
            let receiver = match mode {
                receiver::Mode::Owned => owner_ty.clone(),
                mode => Ty::Reference {
                    mutable: mode == receiver::Mode::Mutable,
                    target: Box::new(owner_ty.clone()),
                },
            };
            // A generic method's parameters take its turbofish, or are
            // inferred where the value is used, as in Rust.
            let substitution = self.instantiate_generics(
                definition.generics(),
                span(owner_span.start(), member_span.end()),
            )?;
            let inputs =
                std::iter::once(receiver)
                    .chain(definition.parameters().iter().map(|parameter| {
                        instantiate(&Ty::from_resolved(&parameter.ty), &substitution)
                    }))
                    .collect();
            return self.method_value(member, inputs, owner_span, member_span);
        }
        // `Type::method(receiver, ..)`: the method called on its first
        // argument, as Rust's path call is.
        if definition.receiver().is_some()
            && matches!(self.peek(), Token::LeftParen)
            && let Some(tokens) = self.path_method_call(&member, member_span, None)
        {
            self.pending_turbofish = None;
            self.splice_tokens(tokens);
            return self.parse_primary();
        }
        if definition.receiver().is_some() {
            return Err(Error::InvalidMethodCall {
                owner: definition.owner_name().into(),
                method: definition.name().into(),
                reason: "receiver methods must be called on a value".into(),
                call: span(owner_span.start(), member_span.end()),
            });
        }
        // `Type::NAME`: an associated const is its value.
        let (mut arguments, end) = if definition.constant() {
            (Vec::new(), text_end(member_span.start(), &member))
        } else if !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            let expression = span(owner_span.start(), member_span.end());
            if definition.generics().is_empty()
                && method_copied(&definition, &self.models).is_empty()
            {
                return self.associated_value(&definition, expression);
            }
            return self.symbolic_associated(&definition, false, expression);
        } else {
            self.parse_borrowing_arguments("associated function arguments")?
        };
        let substitution =
            self.method_substitution(&definition, None, span(owner_span.start(), end))?;
        self.require_arguments(
            &definition,
            &mut arguments,
            span(owner_span.start(), end),
            &substitution,
        )?;
        self.lend_arguments(&definition, &mut arguments, &substitution, None);
        // A generic associated function compiled per instantiation is
        // its instance for the types this call gives it (D629).
        #[cfg(feature = "rust-import")]
        let definition =
            self.bind_method_instance(definition, &substitution, span(owner_span.start(), end))?;
        let witnesses = self.witness_arguments(
            definition.generics(),
            &substitution,
            span(owner_span.start(), end),
        );
        let copied = method_copied(&definition, &self.models);
        let places = self.mutable_arguments(&arguments, copied.iter().copied())?;
        self.move_copied_in(&mut arguments, &copied);
        let source = origin::Fragment::call(
            method_name(definition.id()),
            arguments
                .iter()
                .map(ParsedExpr::rendered_source)
                .chain(witnesses),
        );
        let source = self.copied_call(source, &places)?;
        let summary = self.method_summary(&definition);
        let kept = instantiate(&Ty::from_resolved(definition.output()), &substitution);
        let kept = self.inference.resolve(&kept);
        self.kept_by_call(span(owner_span.start(), end), &kept, None, &summary);
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: instantiate(&Ty::from_resolved(definition.output()), &substitution),
            span: span(owner_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// The variant a crate re-exports at `path`, as `iced::Center` names
    /// `Alignment::Center` through `pub use Alignment::Center;`, built
    /// through its enum. `None` when `path` is no such re-export.
    #[cfg(feature = "rust-import")]
    fn reexported_variant(
        &mut self,
        path: &str,
        owner_span: Span,
        member_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let host = &self.models.host;
        let canonical = host.canonical_str(path);
        if canonical.as_str() == path {
            return Ok(None);
        }
        let Some(owner) = host.enum_variant(path) else {
            return Ok(None);
        };
        let Some(crate::host::Entry::Type(definition)) = host.entry(&owner) else {
            return Ok(None);
        };
        let definition = definition.clone();
        let dependency = host.type_owner(&owner).cloned();
        let variant = canonical.rsplit("::").next().unwrap_or_default().into();
        self.parse_imported_variant(
            owner,
            definition,
            dependency,
            variant,
            owner_span,
            member_span,
        )
        .map(Some)
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn parse_imported_variant(
        &mut self,
        path: ImmutableString,
        definition: crate::host::type_def::Definition,
        owner: Option<crate::host::facade::Dependency>,
        member: ImmutableString,
        owner_span: Span,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let crate::host::type_def::Shape::Enum(variants) = definition.shape() else {
            unreachable!("imported variant parsing is selected only for enum shape")
        };
        let Some(variant) = variants.iter().find(|variant| variant.name() == member) else {
            let error = Error::UnknownMember {
                owner: path,
                member,
                reference: member_span,
            };
            return Err(self.native_miss(definition.identity().imported().is_some(), error));
        };
        if let Some(unsupported) = variant.unsupported() {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: format!("variant `{member}`'s {}", unsupported_field(unsupported)).into(),
                construction: member_span,
            });
        }
        if !variant.constructible() {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: format!("private Rust fields make variant `{member}` non-constructible")
                    .into(),
                construction: member_span,
            });
        }
        // A generic enum's fields and context infer its parameters, and a
        // default fills each one they leave unknown, as iced's `Key<C =
        // SmolStr>` has.
        let instance = definition
            .parameters()
            .iter()
            .map(|_| self.inference.fresh())
            .collect::<Box<[_]>>();
        let fields = variant.fields().to_vec();
        let rawr_import::surface::identity::Selector::EnumVariant { form, .. } =
            variant.imported_identity()
        else {
            unreachable!("an enum variant's selector names an enum variant")
        };
        let selected_owner = self.models.host.type_owner(&path).cloned();
        let (mut arguments, end) = match form {
            rawr_import::surface::identity::Form::Unit => (Vec::new(), member_span.end()),
            rawr_import::surface::identity::Form::Tuple { .. } => {
                let mut arguments = Vec::new();
                let end = if matches!(self.peek(), Token::Unit) {
                    self.next().1
                } else {
                    self.expect(Token::LeftParen, "'(' after imported tuple variant")?;
                    while !matches!(self.peek(), Token::RightParen) {
                        if self.element_left_out()? {
                            continue;
                        }
                        arguments.push(self.parse_expr()?);
                        if matches!(self.peek(), Token::Comma) {
                            self.next();
                        } else {
                            break;
                        }
                    }
                    self.expect(Token::RightParen, "')' after imported tuple variant")?
                };
                (arguments, end)
            }
            rawr_import::surface::identity::Form::Named { .. } => {
                self.expect(Token::LeftBrace, "'{' after imported named variant")?;
                let mut values = BTreeMap::<ImmutableString, (ParsedExpr, Span)>::new();
                while !matches!(self.peek(), Token::RightBrace) {
                    let (field, field_span) =
                        self.expect_name("imported variant field initializer")?;
                    let value = if matches!(self.peek(), Token::Colon) {
                        self.next();
                        self.parse_expr()?
                    } else {
                        self.parse_name_primary(field.clone(), field_span)?
                    };
                    if values.insert(field.clone(), (value, field_span)).is_some() {
                        return Err(Error::InvalidConstruction {
                            owner: path,
                            reason: format!("field `{field}` is initialized more than once").into(),
                            construction: field_span,
                        });
                    }
                    if matches!(self.peek(), Token::Comma) {
                        self.next();
                    } else if !matches!(self.peek(), Token::RightBrace) {
                        return Err(self.unsupported("imported variant fields without ','"));
                    }
                }
                let end = self.expect(Token::RightBrace, "'}' after imported named variant")?;
                let mut arguments = Vec::with_capacity(fields.len());
                for field in &fields {
                    let Some((value, _)) = values.remove(field.name()) else {
                        return Err(Error::InvalidConstruction {
                            owner: path,
                            reason: format!("missing field `{}`", field.name()).into(),
                            construction: span(owner_span.start(), end),
                        });
                    };
                    arguments.push(value);
                }
                if let Some((field, (_, field_span))) = values.into_iter().next() {
                    return Err(Error::UnknownMember {
                        owner: path,
                        member: field,
                        reference: field_span,
                    });
                }
                (arguments, end)
            }
        };
        if arguments.len() != fields.len() {
            return Err(Error::InvalidConstruction {
                owner: path,
                reason: format!(
                    "variant `{member}` expects {} value(s), found {}",
                    fields.len(),
                    arguments.len()
                )
                .into(),
                construction: span(owner_span.start(), end),
            });
        }
        let mut parameters = definition
            .parameters()
            .iter()
            .cloned()
            .zip(instance.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        for (argument, field) in arguments.iter_mut().zip(&fields) {
            let expected = self.instantiate_host_type(
                field.ty(),
                &mut parameters,
                &path,
                argument.span,
                selected_owner.as_ref(),
                Some(&definition),
            )?;
            self.coerce_expr(&expected, argument)?;
            self.consume_imported_value(argument, &path, argument.span)?;
        }
        for (parameter, default) in instance.iter().zip(definition.defaults()) {
            if let Some(default) = default {
                let default = self.instantiate_host_type(
                    default,
                    &mut parameters,
                    &path,
                    member_span,
                    selected_owner.as_ref(),
                    Some(&definition),
                )?;
                self.structural_defaults
                    .push((parameter.clone(), default, member_span));
            }
        }
        let construction = span(owner_span.start(), end);
        let output = Ty::Host {
            proof: HostTyProof::new(definition.clone(), Some(definition.clone()), owner),
            arguments: instance,
        };
        let imported = ImportedOperation {
            kind: crate::host::structural::Kind::Variant,
            operands: arguments
                .iter()
                .map(|argument| (argument.ty.clone(), argument.span, false))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            output: output.clone(),
            receiver: None,
        };
        let helper = self.push_imported_op(
            &path,
            variant.imported_identity(),
            Some(imported),
            construction,
        )?;
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::call(
                &helper,
                arguments.iter().map(ParsedExpr::rendered_source),
            ),
            ty: output,
            span: construction,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }
}

impl<'a> Parser<'a> {
    pub(super) fn parse_builtin_path(
        &mut self,
        builtin: Ty,
        name: ImmutableString,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.next();
        let (member, member_span) = self.expect_member_name()?;
        // `char::to_ascii_uppercase`, `String::len(&s)`: a method of the
        // type as std declares it. A call of a number's inherent method is
        // its own (`parse_number_method`).
        if (!matches!(builtin, Ty::Int(_) | Ty::Float(_))
            || !matches!(self.peek(), Token::LeftParen | Token::Unit)
            || std_method::inherent(&builtin, &member).is_none())
            && let Some(value) =
                self.std_method_path(builtin.clone(), &name, &member, name_span, member_span)
        {
            return value;
        }
        // `f64::consts::PI`.
        if member.as_str() == "consts" && matches!(self.peek(), Token::DoubleColon) {
            self.next();
            let (constant, constant_span) = self.expect_member_name()?;
            let path = format!("{name}::consts::{constant}");
            let Some((value, single)) = crate::resolve::float_constant(&path) else {
                return Err(Error::UnknownMember {
                    owner: format!("{name}::consts").into(),
                    member: constant,
                    reference: constant_span,
                });
            };
            let at = span(
                name_span.start(),
                text_end(constant_span.start(), &constant),
            );
            return Ok(self.float_constant(value, single, at));
        }
        // `u8::MAX`, `i64::MIN`, `f64::MAX`: the kind's bounds (`u64::MAX`
        // and `usize::MAX` are all 64 bits set).
        if !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            let at = span(name_span.start(), member_span.end());
            if let Ty::Int(kind) = builtin {
                let bound = match (kind, member.as_str()) {
                    (Int::I128, "MAX") => Some(Wide::Signed(i128::MAX)),
                    (Int::I128, "MIN") => Some(Wide::Signed(i128::MIN)),
                    (Int::U128, "MAX") => Some(Wide::Unsigned(u128::MAX)),
                    _ => None,
                };
                if let Some(bound) = bound {
                    return self.wide_constant(bound, builtin, at);
                }
                if member.as_str() == "BITS" {
                    return Ok(self.literal(kind.bits().to_string(), Ty::Int(Int::U32), at));
                }
            }
            let text = match (&builtin, member.as_str()) {
                (Ty::Int(kind), "MAX") => kind.store(kind.full_range().1).to_string(),
                (Ty::Int(kind), "MIN") => kind.range().0.to_string(),
                (Ty::Float(Float::F32), "MAX") => format!("{:?}", f64::from(f32::MAX)),
                (Ty::Float(Float::F32), "MIN") => format!("{:?}", f64::from(f32::MIN)),
                (Ty::Float(Float::F32), "MIN_POSITIVE") => {
                    format!("{:?}", f64::from(f32::MIN_POSITIVE))
                }
                (Ty::Float(Float::F32), "EPSILON") => format!("{:?}", f64::from(f32::EPSILON)),
                (Ty::Float(_), "MAX") => format!("{:?}", f64::MAX),
                (Ty::Float(_), "MIN") => format!("{:?}", f64::MIN),
                (Ty::Float(_), "MIN_POSITIVE") => format!("{:?}", f64::MIN_POSITIVE),
                (Ty::Float(_), "EPSILON") => format!("{:?}", f64::EPSILON),
                // IEEE division by zero, as rustc folds these.
                (Ty::Float(_), "INFINITY") => "(1.0 / 0.0)".into(),
                (Ty::Float(_), "NEG_INFINITY") => "(-1.0 / 0.0)".into(),
                (Ty::Float(_), "NAN") => "(0.0 / 0.0)".into(),
                // A function as a value is `std_method_path`'s; any other
                // name std doesn't declare for the type.
                _ => {
                    return Err(Error::UnknownMember {
                        owner: name,
                        member,
                        reference: member_span,
                    })
                }
            };
            let text = if text.starts_with('-') {
                format!("({text})")
            } else {
                text
            };
            return Ok(self.literal(text, builtin, span(name_span.start(), member_span.end())));
        }
        let (mut arguments, end) = self.parse_call_arguments("associated function arguments")?;
        let expression = span(name_span.start(), end);
        let owner = owner_id(&builtin);
        // `char::from_digit(n, radix)` and `char::from_u32(n)`: an
        // `Option<char>`.
        if builtin == Ty::Char
            && let ("from_digit", [_, _]) | ("from_u32", [_]) =
                (member.as_str(), arguments.as_slice())
        {
            let mut operands = Vec::new();
            for argument in &mut arguments {
                self.coerce_expr(&Ty::Int(Int::U32), argument)?;
                operands.push(argument.rendered_source());
            }
            if operands.len() == 1 {
                operands.push("-1".into());
            }
            let source = self.prelude_op(PreludeOp::CharFrom, operands);
            let ty = Ty::Option(Box::new(Ty::Char));
            return Ok(self.operator_expr(source, ty, expression));
        }
        // `u8::try_from(x)` of an integer: Rust's `TryFrom` among integers,
        // `Err` when the value does not fit.
        if let (Ty::Int(_), "try_from", [argument]) =
            (&builtin, member.as_str(), arguments.as_slice())
        {
            let source = self.integer_operand(argument)?;
            let value = self.int_op(
                "try_from",
                argument.rendered_source(),
                "0".into(),
                &builtin,
                &source,
            );
            let ty = Ty::Result(
                Box::new(builtin),
                Box::new(Ty::ParseError(ParseError::TryFromInt)),
            );
            return Ok(self.operator_expr(value, ty, expression));
        }
        // `i32::from_str(s)`: std's `FromStr` for the type, which is
        // `s.parse::<i32>()`.
        if member.as_str() == "from_str"
            && let Some(kind) = ParseError::of(&builtin)
            && let [text] = arguments.as_mut_slice()
        {
            self.coerce_expr(&Ty::Str, text)?;
            let helper = self.push_op(Op::Kinded {
                op: PreludeOp::Parse,
                kinds: [builtin.clone(), builtin.clone()],
            });
            let source = origin::Fragment::call(&helper, [text.rendered_source()]);
            let ty = Ty::Result(Box::new(builtin), Box::new(Ty::ParseError(kind)));
            return Ok(self.operator_expr(source, ty, expression));
        }
        if builtin == Ty::String && member.as_str() == "new" && arguments.is_empty() {
            return Ok(self.literal("\"\"", Ty::String, expression));
        }
        // `f64::sqrt(x)`, `f32::rem_euclid(x, y)`: the number's method on
        // the first argument, as Rust's path call is.
        if matches!(builtin, Ty::Int(_) | Ty::Float(_))
            && !matches!(
                member.as_str(),
                "from" | "try_from" | "default" | "from_str_radix" | "from_str"
            )
            && !arguments.is_empty()
        {
            let mut receiver = arguments.remove(0);
            self.coerce_expr(&builtin, &mut receiver)?;
            self.pending_arguments = Some((arguments, end));
            return self.parse_number_method(receiver, builtin, member, member_span);
        }
        // `String::default()`, `u32::default()`, `bool::default()`.
        if member.as_str() == "default" && arguments.is_empty() {
            let source = self.default_source(&builtin, expression)?;
            return Ok(self.operator_expr(source, builtin, expression));
        }
        if builtin == Ty::String
            && member.as_str() == "with_capacity"
            && let [capacity] = arguments.as_mut_slice()
        {
            self.coerce_expr(&Ty::Int(Int::Usize), capacity)?;
            return Ok(self.literal("\"\"", Ty::String, expression));
        }
        // `String::from_utf8(bytes)`: a `Result` of the text or the error.
        if builtin == Ty::String
            && member.as_str() == "from_utf8"
            && let [bytes] = arguments.as_mut_slice()
        {
            let expected = Ty::Array {
                element: Box::new(Ty::Int(Int::U8)),
                kind: sequence::Kind::Vec,
            };
            self.coerce_expr(&expected, bytes)?;
            let code = STR_PIECES
                .iter()
                .position(|name| *name == "from_utf8")
                .expect("a listed piece")
                .to_string();
            let source = self.prelude_op(
                PreludeOp::StrPieces,
                [
                    bytes.rendered_source(),
                    code.into(),
                    "()".into(),
                    "()".into(),
                ],
            );
            let ty = Ty::Result(
                Box::new(Ty::String),
                Box::new(Ty::ParseError(ParseError::Utf8)),
            );
            return Ok(self.operator_expr(source, ty, expression));
        }
        if member.as_str() == "from"
            && let [argument] = arguments.as_mut_slice()
        {
            let source = self.inference.resolve(&argument.ty);
            // `String`'s `From<&str>`, `From<&String>` and `From<String>`.
            if builtin == Ty::String
                && match &source {
                    Ty::Str | Ty::String => true,
                    Ty::Reference { target, .. } => self.inference.resolve(target) == Ty::String,
                    _ => false,
                }
            {
                let source = argument.rendered_source();
                return Ok(self.operator_expr(source, Ty::String, expression));
            }
            // The impl for this Self type: `f32::from` beside `f64::from`,
            // which share one impl owner.
            if let Some((definition, from, _)) = match owner {
                Some(id) => self.select_conversion(id, Some(&builtin), &source, expression)?,
                None => None,
            } {
                self.coerce_expr(&from, argument)?;
                return Ok(self.operator_expr(
                    origin::Fragment::call(
                        method_name(definition.id()),
                        [argument.rendered_source()],
                    ),
                    builtin,
                    expression,
                ));
            }
            if self.unifies(&builtin, &source) {
                self.unify_type(&builtin, &source);
                let value = argument.rendered_source().surrounded("(", ")");
                return Ok(self.operator_expr(value, builtin, expression));
            }
            // Rust's lossless `From` impls among numbers: a wider integer,
            // an `f64` from an `f32` or an integer of at most 32 bits.
            let lossless = match (&builtin, &source) {
                // Into an `i128` or a `u128` from a narrower kind of fixed
                // width; a `u64`'s bits are read unsigned.
                (Ty::Int(to), Ty::Int(from)) if to.wide() || from.wide() => {
                    let fits = !from.wide()
                        && !matches!(from, Int::Isize | Int::Usize)
                        && (*to == Int::I128 || !from.signed());
                    fits.then_some(*from == Int::U64)
                }
                (Ty::Int(to), Ty::Int(from)) => {
                    let ((low, high), (to_low, to_high)) = (from.full_range(), to.full_range());
                    (to_low <= low && high <= to_high).then_some(false)
                }
                (Ty::Float(Float::F64), Ty::Float(Float::F32)) => Some(false),
                (Ty::Float(Float::F64), Ty::Int(from)) => {
                    (from.checked() && !matches!(from, Int::U64 | Int::Usize)).then_some(true)
                }
                _ => None,
            };
            if let Some(cast) = lossless {
                let mut value = argument.rendered_source();
                if cast {
                    let code = match builtin {
                        Ty::Int(_) => kind_code(builtin.clone()) + UNSIGNED_SOURCE,
                        _ => 11,
                    };
                    value = self.prelude_op(PreludeOp::Cast, [value, code.to_string().into()]);
                }
                return Ok(self.operator_expr(value, builtin, expression));
            }
            // `f32::from(degrees)`: the source type's `into` that gives the
            // target, as a crate's `impl From<Degrees> for f32` makes it,
            // since rustc's `Into` comes from `From`. A generic body's
            // facade calls carry witnesses, so it calls the type's own
            // `into`.
            if let Ty::Host { .. } = &source {
                let (forms, owner) = self.conversion_forms(&source).unwrap_or_default();
                let gives = forms.iter().any(|item| {
                    let checkpoint = self.inference.checkpoint();
                    let fits = self
                        .instantiate_host_signature(item, Some(&source), Some(&[]), Span::NONE)
                        .is_ok_and(|signature| self.unify_type(&signature.output, &builtin));
                    self.inference.rollback(checkpoint);
                    fits
                });
                if gives && self.host_witnesses.is_empty() {
                    let value = self.parse_target_into(
                        argument.clone(),
                        &source,
                        forms,
                        owner,
                        expression.end(),
                    );
                    self.unify_type(&value.ty, &builtin);
                    return Ok(self.operator_expr(value.rendered_source(), builtin, expression));
                }
                if gives {
                    self.pending_arguments = Some((Vec::new(), expression.end()));
                    let value =
                        self.parse_host_method(argument.clone(), "into".into(), expression)?;
                    return Ok(self.operator_expr(value.rendered_source(), builtin, expression));
                }
                #[cfg(feature = "rust-import")]
                if forms.is_empty() {
                    self.demand_into(&source);
                }
            }
            if let Some(op) = self.std_conversion(&builtin, &source) {
                let value = match op {
                    Some(op) => self.prelude_op(op, [argument.rendered_source()]),
                    None => argument.rendered_source(),
                };
                return Ok(self.operator_expr(value, builtin, expression));
            }
            if let Some(error) = self.conversion_not_yet(&builtin, &source, expression) {
                return Err(error);
            }
            return Err(Error::MissingCapability {
                owner: name,
                capability: format!("From<{}>", source.name(&self.models)).into(),
                operation: expression,
                derive: false,
                required_by: None,
            });
        }
        // A crate trait's function without `self` for this type.
        let found = owner
            .and_then(|id| self.models.trait_methods.get(&(id, member.clone())))
            .and_then(|definitions| definitions.first())
            .and_then(|definition| definition.trait_name().cloned())
            .and_then(|trait_name| {
                let (_, declared) = self.models.traits.get(&trait_name)?;
                let function = declared
                    .functions
                    .iter()
                    .find(|function| function.name.text == member && function.receiver.is_none())?
                    .clone();
                Some((trait_name, function))
            });
        let (Some((trait_name, function)), Some(owner)) = (found, owner) else {
            return Err(Error::UnknownMember {
                owner: name,
                member,
                reference: member_span,
            });
        };
        self.call_static_trait_function(
            builtin,
            origin::Fragment::plain(owner_key(owner)),
            &trait_name,
            &function,
            arguments,
            expression,
        )
    }
}
