//! Types written in a body: annotations, `fn` and `dyn` types, aliases,
//! turbofish arguments and `PhantomData`, resolved to the lowering's types.

use super::*;

impl<'a> Parser<'a> {
    /// `PhantomData<T>`, which holds nothing: the unit type. Its arguments
    /// are read and left out.
    pub(super) fn parse_phantom_type(&mut self) -> Result<Ty, Error> {
        if matches!(self.peek(), Token::LessThan) {
            self.next();
            loop {
                self.parse_local_type()?;
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
            self.expect_close_angle("'>' after `PhantomData`'s argument")?;
        }
        Ok(Ty::Unit)
    }

    /// How many `::name` segments follow `name` to spell `PhantomData`:
    /// `PhantomData`, `marker::PhantomData` or `std::marker::PhantomData`.
    pub(super) fn phantom_path(&mut self, name: &str) -> Option<usize> {
        let names = self.lookahead_path(name);
        (1..=names.len())
            .find(|count| {
                self.std_scoped(&names[..*count].join("::"))
                    .is_some_and(|path| crate::resolve::std_phantom(&path))
            })
            .map(|count| (count - 1) * 2)
    }

    /// `name` and the names after it joined by `::`, as written ahead.
    pub(super) fn lookahead_path(&mut self, name: &str) -> Vec<String> {
        let mut names = vec![name.to_owned()];
        if self.split_angle.is_some() || self.bindings.contains_key(name) {
            return names;
        }
        let mut ahead = self.input.clone();
        loop {
            if !matches!(ahead.next(), Some((Token::DoubleColon, _))) {
                break;
            }
            let Some((Token::Identifier(next), _)) = ahead.next() else {
                break;
            };
            names.push(next.to_string());
        }
        names
    }

    /// One of std's ranges (`Range<T>`, `RangeInclusive<T>`, `RangeFrom<T>`,
    /// `RangeTo<T>`, `RangeToInclusive<T>`, `RangeFull`), after its name.
    pub(super) fn parse_range_type(&mut self, range: &str) -> Result<Ty, Error> {
        if range == "RangeFull" {
            return Ok(Ty::OpenRange(range::Open::Full, Box::new(Ty::Unit)));
        }
        self.expect(Token::LessThan, "'<' after a range type")?;
        let element = self.parse_local_type()?;
        self.expect_close_angle("'>' after a range type's element")?;
        if let Some(open) = range::Open::named(range) {
            return Ok(Ty::OpenRange(open, Box::new(element)));
        }
        if range != "RangeInclusive" {
            return Ok(Ty::Range(Box::new(element)));
        }
        Ok(match element {
            Ty::Float(_) => Ty::FloatRange,
            Ty::Int(kind) => Ty::InclusiveRange(kind),
            Ty::Infer(_) => Ty::InclusiveRange(Int::Var(self.inference.fresh_id())),
            element => Ty::InclusiveOf(Box::new(element)),
        })
    }

    /// A closure's or `fn` type's output after its `->`: a type, or the
    /// never type `!`, which stable Rust names only there.
    pub(super) fn parse_local_output_type(&mut self) -> Result<Ty, Error> {
        if matches!(self.peek(), Token::Bang) {
            self.next();
            return Ok(Ty::Never);
        }
        self.parse_local_type()
    }

    /// A local annotation: `_`, `()`, `&T`, or a type name with generic
    /// arguments (`Element<'_, Message>`, `Element<_>`); lifetimes are erased.
    pub(super) fn parse_local_type(&mut self) -> Result<Ty, Error> {
        // `t!()`: the type the program's own macro expands to.
        if self.splice_macro_ahead(super::expand::Place::Type)? {
            return self.parse_local_type();
        }
        // `::image::ImageError`: a leading `::` names an outside crate,
        // which no `use` or generic parameter shadows.
        if matches!(self.peek(), Token::DoubleColon) {
            let start = self.next().1;
            let head = match self.peek() {
                Token::Identifier(name) => ImmutableString::from(name.as_str()),
                _ => return Err(self.unsupported("a crate name after a leading `::`")),
            };
            if matches!(head.as_str(), "crate" | "Self")
                || self.generics.contains_key(head.as_str())
                || !matches!(self.peek_second(), Token::DoubleColon)
            {
                let reference = span(start, text_end(self.position(), &head));
                return Err(crate::resolve::not_a_crate(&head, reference));
            }
            let outer = self.models.global_head.replace(head);
            let ty = self.parse_local_type();
            self.models.global_head = outer;
            return ty;
        }
        match self.peek() {
            Token::Underscore => {
                self.next();
                return Ok(self.inference.fresh());
            }
            Token::Unit => {
                self.next();
                return Ok(Ty::Unit);
            }
            // `!` outside a function's output is unstable in Rust.
            Token::Bang => {
                let at = self.position();
                return Err(crate::resolve::std_path::experimental_never(point_span(at)));
            }
            Token::LeftParen => {
                self.next();
                let mut members = Vec::new();
                let mut trailing = false;
                while !matches!(self.peek(), Token::RightParen) {
                    members.push(self.parse_local_type()?);
                    trailing = matches!(self.peek(), Token::Comma);
                    if trailing {
                        self.next();
                    } else if !matches!(self.peek(), Token::RightParen) {
                        return Err(self.unsupported("tuple type members without ','"));
                    }
                }
                self.next();
                return Ok(if members.len() == 1 && !trailing {
                    members.pop().expect("one parenthesized type")
                } else {
                    Ty::Tuple(members.into_boxed_slice())
                });
            }
            Token::LeftBracket => {
                self.next();
                let element = Box::new(self.parse_local_type()?);
                let length = if matches!(self.peek(), Token::SemiColon) {
                    self.next();
                    Some(self.parse_array_length("array length")?)
                } else {
                    None
                };
                self.expect(Token::RightBracket, "']' after an array type")?;
                let kind = length.map_or(sequence::Kind::Slice, sequence::Kind::Array);
                return Ok(Ty::Array { element, kind });
            }
            Token::Fn => {
                self.next();
                return self.parse_local_fn_type(true);
            }
            // `for<'a> fn(&'a T)`: a function pointer for every `'a`,
            // whose lifetimes are elided ones.
            Token::For => {
                let outer = self.parse_local_binder()?;
                if !matches!(self.peek(), Token::Fn) {
                    self.bound_lifetimes.truncate(outer);
                    return Err(self.unsupported("a higher-ranked type other than `fn(..)`"));
                }
                self.next();
                let ty = self.parse_local_fn_type(true);
                self.bound_lifetimes.truncate(outer);
                return ty;
            }
            Token::Ampersand => {
                self.next();
                self.skip_lifetime();
                let mutable = matches!(self.peek(), Token::Mut);
                if mutable {
                    self.next();
                }
                if !mutable && self.peek_identifier() == Some("str") {
                    self.next();
                    return Ok(Ty::Str);
                }
                let target = Box::new(self.parse_local_type()?);
                return Ok(Ty::Reference { mutable, target });
            }
            // `&&T`, which the lexer reads as one `&&`: `& &T`.
            Token::And => {
                let at = self.next().1;
                self.splice_tokens(vec![(Token::Ampersand, at)]);
                let target = Box::new(self.parse_local_type()?);
                return Ok(Ty::Reference {
                    mutable: false,
                    target,
                });
            }
            _ => {}
        }
        // `<T as Trait>::Name`: the associated type `T::Name`.
        if matches!(self.peek(), Token::LessThan) {
            self.next();
            let base = self.parse_local_type()?;
            let written = if matches!(self.peek(), Token::As) {
                self.next();
                Some(self.expect_name("trait in a qualified type path")?.0)
            } else {
                None
            };
            self.expect(Token::GreaterThan, "'>' in a qualified type path")?;
            self.expect(Token::DoubleColon, "'::' after a qualified type path")?;
            let (member, member_span) = self.expect_qualified_name()?;
            let trait_name = self.projection_trait(&base, written, &member, member_span)?;
            return Ok(self
                .inference
                .resolve(&Ty::Projection(Box::new(base), trait_name, member)));
        }
        let (mut name, mut name_span) = match self.peek() {
            Token::Reserved(keyword) if keyword.as_str() == "super" => {
                let position = self.next().1;
                ("super".into(), point_span(position))
            }
            Token::SelfValue if matches!(self.peek_second(), Token::DoubleColon) => {
                let position = self.next().1;
                ("self".into(), point_span(position))
            }
            // `let a := 1;`, `let a:;`: no type starts there.
            token @ (Token::Equals | Token::SemiColon) => {
                let at = self.position();
                let written = token.literal_syntax();
                return Err(Error::Syntax {
                    message: format!("expected type, found `{written}`").into(),
                    label: "expected type".into(),
                    help: None,
                    at: Span::new(at, text_end(at, written)),
                });
            }
            _ => self.expect_name("binding type")?,
        };
        // A type parameter native code fills, and its associated types, as
        // `S` and `S::Ok`: the opaque types the signature names (#593).
        #[cfg(feature = "rust-import")]
        if let Some(path) = self.opaque.get(&name).cloned() {
            return self.opaque_parameter_type(&name, &path, name_span);
        }
        // A bare `str`, as `Cow<'_, str>` writes it: rustc's unsized `str`.
        if name.as_str() == "str"
            && !matches!(self.peek(), Token::DoubleColon)
            && self.models.get(&name).is_none()
        {
            return Ok(Ty::UnsizedStr);
        }
        // `dyn Fn(A) -> B` and `dyn Trait`.
        if name.as_str() == "dyn" && !self.models.get(&name).is_some() {
            // `dyn for<'a> Fn(&'a T)`.
            if matches!(self.peek(), Token::For) {
                let outer = self.parse_local_binder()?;
                self.splice_tokens(vec![(
                    Token::Identifier(Box::new("dyn".into())),
                    name_span.start(),
                )]);
                let ty = self.parse_local_type();
                self.bound_lifetimes.truncate(outer);
                return ty;
            }
            if let Token::Identifier(trait_name) | Token::Reserved(trait_name) = self.peek()
                && matches!(trait_name.as_str(), "Fn" | "FnMut" | "FnOnce")
            {
                self.next();
                return self.parse_local_fn_type(false);
            }
            // `dyn std::error::Error`: traits are crate-wide by name, as in
            // item headers. A native trait is its full path, which a
            // leading `::` (`dyn ::tracing::Value`) roots at its crate.
            if matches!(self.peek(), Token::DoubleColon) {
                self.next();
            }
            let (first, first_span) = self.expect_name("trait object type")?;
            let (mut trait_name, mut trait_span) = (first.clone(), first_span);
            let mut written = first.to_string();
            while matches!(self.peek(), Token::DoubleColon) {
                self.next();
                (trait_name, trait_span) = self.expect_name("trait object type")?;
                written.push_str("::");
                written.push_str(&trait_name);
            }
            let path_span = Span::new(
                first_span.start(),
                text_end(trait_span.start(), &trait_name),
            );
            let known = self.native_trait(&first, &written, path_span);
            // A crate's path the facade doesn't have yet, as `dyn
            // $crate::Value` of a crate's macro names its own trait: asked
            // for as any missing path is, and known at the next check.
            #[cfg(feature = "rust-import")]
            if known.is_none()
                && let Some(miss) = self.trait_path_miss(&first, &written, path_span)
            {
                return Err(miss);
            }
            let trait_name = known.unwrap_or(trait_name);
            let ty = self.dyn_type(&trait_name, trait_span)?;
            // `dyn Shape + Send + 'a`: Rust admits only auto traits and
            // lifetimes after the first trait (E0225).
            while matches!(self.peek(), Token::Plus | Token::UnaryPlus) {
                self.next();
                if self.skip_lifetime() {
                    continue;
                }
                let (mut extra, extra_span) = self.expect_name("auto trait in a trait object")?;
                while matches!(self.peek(), Token::DoubleColon) {
                    self.next();
                    extra = self.expect_name("auto trait in a trait object")?.0;
                }
                if !matches!(
                    extra.as_str(),
                    "Send" | "Sync" | "Unpin" | "UnwindSafe" | "RefUnwindSafe"
                ) {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a second non-auto trait in a trait object is not supported; \
                                    only auto traits (`Send`, `Sync`, `Unpin`) can be added, as \
                                    in Rust (E0225)"
                            .into(),
                        source: extra_span,
                    });
                }
            }
            return Ok(ty);
        }
        // `Self` in a method body names the impl's type.
        if name.as_str() == "Self"
            && !matches!(self.peek(), Token::DoubleColon)
            && let Some(self_ty) = &self.self_ty
        {
            return Ok(self_ty.clone());
        }
        // `Text` after `use std::string::String as Text;` is `String`
        // (#810).
        if !self.generics.contains_key(name.as_str())
            && let Some(spelling) = self.prelude_alias(&name)
        {
            self.splice_tokens(vec![(
                Token::Identifier(Box::new(spelling.into())),
                name_span.start(),
            )]);
            return self.parse_local_type();
        }
        // A type alias: its target, resolved once with the item headers.
        if !matches!(self.peek(), Token::DoubleColon)
            && !self.generics.contains_key(name.as_str())
            && let Some(alias) = self.models.alias(&name).cloned()
        {
            return self.parse_alias_type(name, name_span, &alias);
        }
        let mut qualified = None;
        // `T::Output`, `Self::Output`, `Square::Output`: an associated type.
        if matches!(self.peek(), Token::DoubleColon)
            && let Some(base) = self.projection_base(&name)
        {
            self.next();
            let (member, member_span) = self.expect_qualified_name()?;
            if !matches!(self.peek(), Token::DoubleColon | Token::LessThan) {
                let trait_name = self.projection_trait(&base, None, &member, member_span)?;
                return Ok(self.inference.resolve(&Ty::Projection(
                    Box::new(base),
                    trait_name,
                    member,
                )));
            }
            return Err(self.unsupported("a path below an associated type"));
        }
        if matches!(self.peek(), Token::DoubleColon) {
            let first = name.clone();
            let mut last = name_span;
            while matches!(self.peek(), Token::DoubleColon) {
                self.next();
                let (segment, segment_span) = self.expect_qualified_name()?;
                name = format!("{name}::{segment}").into();
                name_span = Span::new(name_span.start(), text_end(segment_span.start(), &segment));
                last = Span::new(segment_span.start(), name_span.end());
            }
            // A std type through this crate's imports, as
            // `super::HashMap<K, V>` after the parent's `use
            // std::collections::HashMap;`, is std's path.
            if let Some(tokens) = self.std_spelled(name_span) {
                self.splice_tokens(tokens);
                return self.parse_local_type();
            }
            // A crate's re-export of a std type is that type, as serde's
            // `__private::Formatter` is `core::fmt::Formatter` (#593).
            if let Some(std) = self.std_alias(&first, &name) {
                let at = name_span.start();
                let mut tokens = Vec::new();
                for (index, segment) in std.split("::").enumerate() {
                    if index > 0 {
                        tokens.push((Token::DoubleColon, at));
                    }
                    tokens.push((Token::Identifier(Box::new(segment.into())), at));
                }
                self.splice_tokens(tokens);
                return self.parse_local_type();
            }
            // `m::q::S<i32>`: a script type through its modules, with the
            // generic arguments its path gives.
            if let Some(model) = self.models.qualified_owner(name_span).cloned() {
                if !matches!(self.peek(), Token::LessThan) {
                    return Ok(model.ty());
                }
                let arguments = self.parse_local_arguments()?;
                return Self::applied_script_type(&model, last, arguments);
            }
            if let Some(alias) = self.models.qualified_alias(name_span).cloned() {
                return self.parse_alias_type(name, name_span, &alias);
            }
            // `std::rc::Weak<T>`, or `sync::Weak<T>` after `use std::sync;`.
            if let Some(weak) = self
                .std_scoped(&name)
                .and_then(|path| counted::std_weak(&path))
                && matches!(self.peek(), Token::LessThan)
            {
                self.next();
                let element = Box::new(self.parse_local_type()?);
                self.expect_close_angle("'>' after `Weak<T`")?;
                return Ok(Ty::Weak(Counter::weak_named(weak), element));
            }
            // The std path the path names in this scope, which the std
            // types below are read from: through the `use` that binds its
            // first segment, or from std's root.
            let written = name.clone();
            let mut std_path = self.std_scoped(&written);
            // A path into std is std's, as rustc resolves it (D711); one
            // to a type Rawr represents reads as that type's own name.
            if self.models.get(&first).is_none() && self.models.import(&first).is_none() {
                let std = crate::resolve::std_path::check(
                    &name,
                    crate::std_table::Namespace::Type,
                    "type",
                    name_span,
                )?;
                if let Some(spelling) = std.and_then(crate::resolve::std_path::spelling) {
                    if spelling == "Cow" {
                        return self.parse_cow_type();
                    }
                    // A prelude type, as `core::option::Option<T>` names
                    // `Option`, is the prelude's name, which nothing here
                    // shadows.
                    if !spelling.contains("::")
                        && self.models.get(spelling).is_none()
                        && self.models.import(spelling).is_none()
                        && crate::std_table::prelude(crate::resolve::std_path::EDITION)
                            .any(|path| path.path.rsplit("::").next() == Some(spelling))
                    {
                        self.splice_tokens(vec![(
                            Token::Identifier(Box::new(spelling.into())),
                            name_span.start(),
                        )]);
                        return self.parse_local_type();
                    }
                    // A std path written through another module, as
                    // `std::collections::hash_map::HashMap`, reads as the
                    // path the helpers know.
                    if let Some(canonical) = crate::resolve::std_canonical(spelling) {
                        std_path = Some(canonical.into());
                    }
                    name = spelling.into();
                }
            }
            let path = std_path.as_deref().unwrap_or(&written);
            if let Some(collection) = crate::resolve::std_collection(path) {
                return self.parse_keyed_type(collection, name_span);
            }
            if crate::resolve::std_reverse(path) {
                return self.parse_reverse_type();
            }
            if self.pin_named(path) && matches!(self.peek(), Token::LessThan) {
                return self.parse_pin_type();
            }
            if crate::resolve::std_phantom(path) {
                return self.parse_phantom_type();
            }
            if let Some(kind) = self.cell_named(path) {
                return self.parse_cell_type(kind);
            }
            if let Some(kind) = self.thread_named(path) {
                return self.parse_thread_type(kind);
            }
            if let Some(range) = crate::resolve::std_range(path) {
                return self.parse_range_type(range);
            }
            if let Some(pointer) = self.pointer_named(path)
                && matches!(self.peek(), Token::LessThan)
            {
                self.next();
                let element = Box::new(self.parse_local_type()?);
                self.unstable_argument(pointer.name(), 1, name_span)?;
                self.expect_close_angle(&format!("'>' after `{}<T`", pointer.name()))?;
                return Ok(pointer.ty(*element));
            }
            if let Some(kind) = self.growable_named(path)
                && matches!(self.peek(), Token::LessThan)
            {
                return self.parse_growable_type(kind, &name, name_span);
            }
            let std_time = std_path.as_deref().and_then(crate::resolve::std_time);
            let std_error = std_path
                .as_deref()
                .and_then(crate::resolve::std_parse_error);
            // A host type named through an imported module or in full; a
            // path into a `.rs` module through the app crate.
            let rust = self
                .models
                .qualified_rust(name_span)
                .map(|spelled| spelled.join("::"));
            let path: ImmutableString = match self.models.import(&first) {
                _ if rust.is_some() => rust.unwrap_or_default().into(),
                Some(binding)
                    if matches!(
                        binding.target,
                        item::Import::Module | item::Import::ModuleItem { .. }
                    ) =>
                {
                    format!("{}{}", binding.qualified, &name[first.len()..]).into()
                }
                _ => name.clone(),
            };
            // std's `Duration` and `Instant`, in full or through a crate's
            // re-export of them, as `time::Duration` after `use iced::time`.
            let path = crate::resolve::std_reexport(&path, &self.models.host);
            if let Some(time) = crate::resolve::std_time(&path).or(std_time) {
                return Ok(Ty::Time(match time {
                    "Duration" => Time::Duration,
                    _ => Time::Instant,
                }));
            }
            if crate::resolve::std_arguments(&path)
                || std_path
                    .as_deref()
                    .is_some_and(crate::resolve::std_arguments)
            {
                return self.parse_arguments_type();
            }
            // `std::num::ParseIntError` and kin, the errors of `str::parse`.
            if let Some(error) = crate::resolve::std_parse_error(&path).or(std_error) {
                return Ok(Ty::ParseError(
                    ParseError::named(error).expect("a std parse error"),
                ));
            }
            // A crate's alias of a builtin, as iced's `markdown::Uri` is
            // `String`.
            if let Some(crate::host::Entry::Builtin(builtin)) = self.models.host.entry(&path)
                && let Some(ty) = builtin_path_type(builtin)
            {
                return Ok(ty);
            }
            // A crate's type alias, as `iced::Result`.
            if let Some(crate::host::Entry::TypeAlias(alias)) = self.models.host.entry(&path)
                && let Some(alias) = type_ref::Alias::host(alias, &self.models.host, name_span)
            {
                return self.parse_alias_type(name, name_span, &alias);
            }
            let Some(definition) = self.models.host.source_type(&path).cloned() else {
                // A std type Rawr doesn't represent yet.
                if let Some(error) = crate::resolve::std_path::not_yet(
                    &path,
                    &path,
                    crate::std_table::Namespace::Type,
                    name_span,
                ) {
                    return Err(error);
                }
                // rustc stops at a first segment nothing in scope names.
                if self.unresolved_root(&first) {
                    let mut start = name_span.start();
                    let mut segments = name
                        .split("::")
                        .map(|segment| {
                            let at = Span::new(start, text_end(start, segment));
                            start = text_end(at.end(), "::");
                            (ImmutableString::from(segment), at)
                        })
                        .collect::<Vec<_>>();
                    // A value's path goes on past the type, as
                    // `time::Duration::from_secs` does.
                    let mut ahead = self.input.clone();
                    if let (Some((Token::DoubleColon, _)), Some((Token::Identifier(next), at))) =
                        (ahead.next(), ahead.next())
                    {
                        segments.push((next.as_str().into(), point_span(at)));
                    }
                    return Err(self.unresolved_root_error(&segments));
                }
                // The path through the module a `use` binds, as rustc
                // resolves it, which a miss demands from the crate; the
                // report writes the module as the source does.
                return Err(Error::UnknownType {
                    name: path,
                    reference: name_span,
                    path: false,
                    imports: Box::default(),
                });
            };
            qualified = Some((definition, self.models.host.type_owner(&path).cloned()));
        }
        if name.as_str() == "Option"
            && matches!(self.peek(), Token::LessThan)
            && qualified.is_none()
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none()
        {
            self.next();
            let element = Box::new(self.parse_local_type()?);
            self.expect_close_angle("'>' after `Option<T`")?;
            return Ok(Ty::Option(element));
        }
        if name.as_str() == "Result"
            && matches!(self.peek(), Token::LessThan)
            && qualified.is_none()
            && self.models.get(&name).is_none()
            && self.models.import(&name).is_none()
        {
            self.next();
            let ok = Box::new(self.parse_local_type()?);
            self.expect(Token::Comma, "',' after `Result<T`")?;
            let err = Box::new(self.parse_local_type()?);
            self.expect_close_angle("'>' after `Result<T, E`")?;
            return Ok(Ty::Result(ok, err));
        }
        if matches!(self.peek(), Token::LessThan)
            && qualified.is_none()
            && let Some(pointer) = self.pointer_named(&name)
        {
            self.next();
            let element = Box::new(self.parse_local_type()?);
            self.unstable_argument(pointer.name(), 1, name_span)?;
            self.expect_close_angle(&format!("'>' after `{}<T`", pointer.name()))?;
            return Ok(pointer.ty(*element));
        }
        if matches!(self.peek(), Token::LessThan)
            && qualified.is_none()
            && let Some(counter) = self.weak_named(&name)
        {
            self.next();
            let element = Box::new(self.parse_local_type()?);
            self.expect_close_angle("'>' after `Weak<T`")?;
            return Ok(Ty::Weak(counter, element));
        }

        // A bare name of a std type, through the `use` that binds it.
        let scoped = qualified
            .is_none()
            .then(|| self.std_scoped(&name))
            .flatten();
        if let Some(collection) = crate::resolve::hasher_alias(&name)
            .filter(|_| qualified.is_none() && self.models.get(&name).is_none())
            .or_else(|| scoped.as_deref().and_then(crate::resolve::std_collection))
        {
            return self.parse_keyed_type(collection, name_span);
        }
        if scoped.as_deref().is_some_and(crate::resolve::std_reverse)
            && matches!(self.peek(), Token::LessThan)
        {
            return self.parse_reverse_type();
        }
        if self.cow_named(&name) && matches!(self.peek(), Token::LessThan) && qualified.is_none() {
            return self.parse_cow_type();
        }
        if qualified.is_none() && matches!(self.peek(), Token::LessThan) && self.pin_named(&name) {
            return self.parse_pin_type();
        }
        if scoped.as_deref().is_some_and(crate::resolve::std_phantom) {
            return self.parse_phantom_type();
        }
        if matches!(self.peek(), Token::LessThan)
            && qualified.is_none()
            && let Some(kind) = self.cell_named(&name)
        {
            return self.parse_cell_type(kind);
        }
        if matches!(self.peek(), Token::LessThan)
            && qualified.is_none()
            && let Some(kind) = self.thread_named(&name)
        {
            return self.parse_thread_type(kind);
        }
        if let Some(range) = scoped.as_deref().and_then(crate::resolve::std_range)
            && (matches!(self.peek(), Token::LessThan) || range == "RangeFull")
        {
            return self.parse_range_type(range);
        }
        if let Some(kind) = self.growable_named(&name)
            && matches!(self.peek(), Token::LessThan)
            && qualified.is_none()
        {
            return self.parse_growable_type(kind, &name, name_span);
        }
        // The name whole, which an error marks.
        let named = Span::new(name_span.start(), text_end(name_span.start(), &name));
        let generic = matches!(self.peek(), Token::LessThan);
        let mut arguments = if generic {
            self.parse_local_arguments()?
        } else {
            Vec::new()
        };
        let host = qualified.clone().or_else(|| {
            self.models
                .import(&name)
                .and_then(|binding| match &binding.target {
                    item::Import::Type { definition, owner } => {
                        Some((definition.clone(), owner.clone()))
                    }
                    _ => None,
                })
        });
        match host {
            Some((definition, owner)) if generic || !definition.parameters().is_empty() => {
                self.default_host_arguments(&definition, owner.as_ref(), &mut arguments);
                if arguments.len() != definition.parameters().len() {
                    return Err(Error::TypeMismatch {
                        expected: format!(
                            "{name} with {} type argument(s)",
                            definition.parameters().len()
                        )
                        .into(),
                        actual: format!("{} type argument(s)", arguments.len()).into(),
                        expression: name_span,
                    });
                }
                // A generic imported type in an annotation (`Rgb<Srgb, f32>`)
                // needs no adapter of its own: its arguments come with the
                // values the program builds.
                let alias = definition.identity().imported().map(|_| definition.clone());
                Ok(Ty::Host {
                    proof: HostTyProof::new(definition, alias, owner),
                    arguments: arguments.into_boxed_slice(),
                })
            }
            None if generic
                && let Some(model) = self.models.get(&name).cloned()
                && !model.generics().is_empty() =>
            {
                Self::applied_script_type(&model, name_span, arguments)
            }
            // A name nothing in scope binds is rustc's error for it, its
            // arguments or not.
            _ if generic => Err(self
                .resolve_type(&name, named)
                .err()
                .unwrap_or_else(|| self.unsupported("generic arguments on a non-generic type"))),
            Some((definition, owner)) if qualified.is_some() => {
                // The use is of the type's own path, as a path through a
                // module import (`alignment::Alignment` after `use
                // iced::alignment;`) names it.
                #[cfg(feature = "rust-import")]
                if let Some(selector) = definition.imported_declaration() {
                    self.push_imported_op(&definition.path(), &selector, None, name_span)?;
                }
                let alias = definition.identity().imported().map(|_| definition.clone());
                Ok(Ty::Host {
                    proof: HostTyProof::new(definition, alias, owner),
                    arguments: Box::default(),
                })
            }
            // A registered facade type named through `use`.
            Some((definition, owner)) if definition.identity().imported().is_none() => {
                Ok(Ty::Host {
                    proof: HostTyProof::new(definition, None, owner),
                    arguments: Box::default(),
                })
            }
            _ => self.resolve_type(&name, named),
        }
    }

    /// The generic arguments `<A, B>` after a type's name, its lifetimes
    /// left out.
    /// `fmt::Arguments`, after its name, with its lifetime if written:
    /// the text `format_args!` formats.
    fn parse_arguments_type(&mut self) -> Result<Ty, Error> {
        if matches!(self.peek(), Token::LessThan) && !self.parse_local_arguments()?.is_empty() {
            return Err(self.unsupported("a type argument of `fmt::Arguments`"));
        }
        Ok(Ty::Arguments)
    }

    fn parse_local_arguments(&mut self) -> Result<Vec<Ty>, Error> {
        self.expect(Token::LessThan, "'<' before generic arguments")?;
        let mut arguments = Vec::new();
        while !matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
            if !self.skip_lifetime() {
                arguments.push(self.parse_local_argument()?);
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                return Err(self.unsupported("generic arguments"));
            }
        }
        self.expect_close_angle("'>' after generic arguments")?;
        Ok(arguments)
    }

    /// The script type `model` with `arguments` for its generic parameters,
    /// which must be as many: else rustc's E0107 at `name`, the type's
    /// name where the path ends.
    fn applied_script_type(model: &Nominal, name: Span, arguments: Vec<Ty>) -> Result<Ty, Error> {
        let expected = model.generics().len();
        if arguments.len() != expected {
            let plural = |count: usize| match count {
                1 => "1 generic argument".to_string(),
                count => format!("{count} generic arguments"),
            };
            let kind = match model {
                Nominal::Struct { .. } => "struct",
                Nominal::Enum { .. } => "enum",
            };
            let supplied = if arguments.len() == 1 { "was" } else { "were" };
            return Err(Error::StdPath {
                code: "E0107",
                message: format!(
                    "{kind} takes {} but {} {supplied} supplied",
                    plural(expected),
                    plural(arguments.len())
                )
                .into(),
                label: Some(format!("expected {}", plural(expected)).into()),
                note: None,
                reference: name,
            });
        }
        Ok(Ty::Script(
            model.definition().id(),
            arguments.into_boxed_slice(),
        ))
    }

    /// `(A, ..) -> B` after `fn` or `dyn Fn` in a local type.
    /// `(A, ..) -> B` after `fn`, or after an `Fn` trait of `dyn`, as
    /// `pointer` says: a `fn` pointer's output may be `!`, and an `Fn`
    /// trait's, its `Output` type, may not.
    pub(super) fn parse_local_fn_type(&mut self, pointer: bool) -> Result<Ty, Error> {
        let mut inputs = Vec::new();
        if matches!(self.peek(), Token::Unit) {
            self.next();
        } else {
            self.expect(Token::LeftParen, "'(' after `fn` in a type")?;
            while !matches!(self.peek(), Token::RightParen) {
                inputs.push(self.parse_local_type()?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else if !matches!(self.peek(), Token::RightParen) {
                    return Err(self.unsupported("`fn` type parameters without ','"));
                }
            }
            self.next();
        }
        let output = match self.peek() {
            Token::ThinArrow if pointer => {
                self.next();
                self.parse_local_output_type()?
            }
            Token::ThinArrow => {
                self.next();
                self.parse_local_type()?
            }
            _ => Ty::Unit,
        };
        Ok(Ty::Fn {
            inputs: inputs.into_boxed_slice(),
            output: Box::new(output),
        })
    }

    /// The type `name` names before `::Output` in a local type, when it is a
    /// generic parameter, `Self` in a method, or a script type with
    /// associated types.
    /// The one opaque type of a type parameter native code fills, or of one
    /// of its associated types, that has a function `member` with no
    /// receiver, for a call through the crate trait at `trait_path`
    /// (`opaque_parameter_type`).
    #[cfg(feature = "rust-import")]
    pub(super) fn opaque_function_owner(&self, trait_path: &str, member: &str) -> Option<String> {
        if !matches!(
            self.models.host.entry(trait_path),
            Some(crate::host::Entry::Module | crate::host::Entry::ModuleItem(_))
        ) {
            return None;
        }
        let mut found = None;
        for path in self.opaque.values() {
            let (module, own) = path.rsplit_once("::")?;
            for child in self.models.host.children(module) {
                if !child.starts_with(own) {
                    continue;
                }
                let candidate = format!("{module}::{child}");
                let Some(items) = self.models.host_items(&format!("{candidate}::{member}")) else {
                    continue;
                };
                if !items.iter().any(|item| item.receiver().is_none()) {
                    continue;
                }
                if found.replace(candidate).is_some() {
                    return None;
                }
            }
        }
        found
    }

    /// The call of the function `member` of the opaque type at the host path
    /// `owner`, as its path names it.
    #[cfg(feature = "rust-import")]
    pub(super) fn opaque_function_call(
        &mut self,
        owner: &str,
        member: &ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let full = format!("{owner}::{member}");
        let Some(items) = self.models.host_items(&full).map(<[_]>::to_vec) else {
            return Err(Error::UnknownMember {
                owner: owner.into(),
                member: member.clone(),
                reference: member_span,
            });
        };
        let item_owner = self.models.item_owner(&full).cloned();
        let named = span(member_span.start(), text_end(member_span.start(), member));
        self.parse_host_function_or_item(items.into_boxed_slice(), item_owner, named)
    }

    /// The opaque type of the type parameter `name` native code fills, at
    /// the host path `path`, or of the associated type a `::` after it
    /// names (`resolve::opaque_generics`).
    #[cfg(feature = "rust-import")]
    pub(super) fn opaque_parameter_type(
        &mut self,
        name: &ImmutableString,
        path: &str,
        name_span: Span,
    ) -> Result<Ty, Error> {
        let (path, span) = if matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::Identifier(_))
        {
            self.next();
            let (member, member_span) = self.expect_qualified_name()?;
            let span = Span::new(name_span.start(), text_end(member_span.start(), &member));
            let projected = rawr_import::surface::opaque::parameter_projection(path, &member)
                .filter(|projected| self.models.host.source_type(projected).is_some())
                .ok_or_else(|| Error::UnknownMember {
                    owner: name.clone(),
                    member: member.clone(),
                    reference: span,
                })?;
            (projected, span)
        } else {
            (path.to_owned(), name_span)
        };
        let definition = self
            .models
            .host
            .source_type(&path)
            .cloned()
            .ok_or_else(|| Error::UnknownType {
                name: name.clone(),
                reference: span,
                path: false,
                imports: Box::default(),
            })?;
        Ok(Ty::from_resolved(&type_ref::Type::Path(type_ref::Path {
            name: path.as_str().into(),
            target: type_ref::Target::Host {
                definition,
                owner: self.models.host.type_owner(&path).cloned(),
            },
            arguments: Box::default(),
            span,
        })))
    }

    pub(super) fn projection_base(&self, name: &str) -> Option<Ty> {
        if self.generics.contains_key(name) {
            return Some(Ty::param(name.into()));
        }
        if name == "Self" {
            return self.self_ty.clone();
        }
        let model = self.models.get(name)?;
        let id = model.definition().id();
        let associated = &self.models.associated;
        (associated.impls.keys().any(|(owner, ..)| *owner == id)).then(|| model.ty())
    }

    /// The trait whose associated type `member` of `base` a local type
    /// names: the one written (`<T as Shape>::Output`), else the one trait
    /// that gives it, as Rust resolves it; two are ambiguous.
    pub(super) fn projection_trait(
        &self,
        base: &Ty,
        written: Option<ImmutableString>,
        member: &ImmutableString,
        span: Span,
    ) -> Result<ImmutableString, Error> {
        let declares = |trait_name: &ImmutableString| {
            self.models
                .traits
                .get(trait_name)
                .is_some_and(|(_, declared)| {
                    declared.associated.iter().any(|name| name.text == *member)
                })
        };
        let candidates: Vec<ImmutableString> = match self.inference.resolve(base) {
            ty @ Ty::Param(..) => self
                .bound_traits(&ty)
                .into_iter()
                .filter(|trait_name| declares(trait_name))
                .collect(),
            Ty::Script(id, _) => {
                // In an impl of a trait giving it, `Self::Output` is that
                // impl's, as in Rust.
                if let Some(trait_name) = &self.impl_trait
                    && declares(trait_name)
                    && written.is_none()
                {
                    return Ok(trait_name.clone());
                }
                self.models
                    .associated
                    .impls
                    .keys()
                    .filter(|(owner, _, name)| *owner == id && name == member)
                    .map(|(_, trait_name, _)| trait_name.clone())
                    .collect()
            }
            // `<&T as For>::Fore` of an impl for `&T` forwarding to `T`'s.
            Ty::Reference { mutable, target } => self
                .projection_trait(&target, written.clone(), member, span)
                .ok()
                .filter(|trait_name| {
                    self.models
                        .associated
                        .reference_forwards
                        .contains(&(trait_name.clone(), mutable))
                })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        };
        let owner = base.name(&self.models);
        match (written, &candidates[..]) {
            (Some(written), _) if candidates.contains(&written) => Ok(written),
            (None, [found]) => Ok(found.clone()),
            (None, [first, second, ..]) => Err(Error::InvalidMethodSignature {
                owner: owner.clone(),
                method: member.clone(),
                reason: format!(
                    "is an ambiguous associated type: both `{first}` and `{second}` declare \
                     `{member}`; write `<{owner} as {first}>::{member}`"
                )
                .into(),
                signature: span,
            }),
            (written, _) => Err(Error::UnknownMember {
                owner: match written {
                    Some(written) => format!("<{owner} as {written}>").into(),
                    None => owner,
                },
                member: member.clone(),
                reference: span,
            }),
        }
    }

    /// The full path of the native trait a trait object names as `written`
    /// (from `first`, at `span`), when it is one: `dyn crate::page::Page`,
    /// `dyn page::Page` after `use crate::page;` or `dyn Page` after `use
    /// crate::page::Page;`, as the item headers write it
    /// (`type_ref::native_trait`).
    #[cfg_attr(
        not(feature = "rust-import"),
        allow(clippy::unused_self, unused_variables)
    )]
    pub(super) fn native_trait(
        &self,
        first: &str,
        written: &str,
        span: Span,
    ) -> Option<ImmutableString> {
        #[cfg(feature = "rust-import")]
        {
            let path = match (self.models.qualified_rust(span), self.models.import(first)) {
                (Some(spelled), _) => spelled.join("::"),
                (None, Some(binding))
                    if matches!(
                        binding.target,
                        item::Import::Module | item::Import::ModuleItem { .. }
                    ) =>
                {
                    format!("{}{}", binding.qualified, &written[first.len()..])
                }
                (None, _) => written.to_owned(),
            };
            if !type_ref::is_native_trait(&path) {
                return None;
            }
            self.trait_known_as(&path)
        }
        #[cfg(not(feature = "rust-import"))]
        None
    }

    /// The one path a crate trait at `path` is known by: one trait reached
    /// through several paths is one trait (D872), as tracing's
    /// `field::Value` is tracing-core's, and `page::Page` is the crate's
    /// `Page` after its root's `use page::Page;`. `None` when no crate
    /// declares a trait at `path`.
    #[cfg(feature = "rust-import")]
    fn trait_known_as(&self, path: &str) -> Option<ImmutableString> {
        let parsed = crate::host::path::Path::parse(path).ok()?;
        let header = self.models.host.trait_header(&parsed)?;
        Some(
            self.models
                .host
                .trait_name(&header.identity)
                .unwrap_or_else(|| path.into()),
        )
    }

    /// The miss of `written`, the path of a trait object's trait from the
    /// crate root `first`, when the facade has nothing there yet, as a
    /// crate's macro names its own trait through `$crate`: asked for as an
    /// expression's missing path is. `None` for a path that names
    /// something, or no crate.
    #[cfg(feature = "rust-import")]
    fn trait_path_miss(&self, first: &str, written: &str, reference: Span) -> Option<Error> {
        if self.models.import(first).is_some() || self.models.host.entry(first).is_none() {
            return None;
        }
        let parsed = crate::host::path::Path::parse(written).ok()?;
        if self.models.host.trait_header(&parsed).is_some()
            || self.models.host.source_type(written).is_some()
        {
            return None;
        }
        let (module, item) = written.rsplit_once("::")?;
        Some(Error::NotInModule {
            module: module.into(),
            item: item.into(),
            reference,
        })
    }

    /// The path from the crate's root of `concrete`, a type of the program's
    /// own with no parameters, when the program implements the native trait
    /// `trait_name` for it: what rawr's copy of the crate names the type's
    /// stand-in by (`package::island`, #1264). With it, the trait's path as
    /// the impl writes it, which the stand-in's impl compares with its own:
    /// the trait object names the trait by the path it is known by, as
    /// `Page` after the root's `use page::Page;` (`Self::native_trait`).
    pub(super) fn native_object_owner(
        &self,
        concrete: &Ty,
        trait_name: &str,
    ) -> Option<(String, ImmutableString)> {
        let Ty::Script(definition, arguments) = self.inference.resolve(concrete) else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let names = |path: &str| {
            path == trait_name || {
                #[cfg(feature = "rust-import")]
                {
                    self.trait_known_as(path)
                        .is_some_and(|known| known.as_str() == trait_name)
                }
                #[cfg(not(feature = "rust-import"))]
                false
            }
        };
        let implemented = self.models.script_impls.iter().find(|candidate| {
            candidate.generics.is_empty()
                && candidate.arguments.is_empty()
                && matches!(&candidate.owner, type_ref::Type::Path(type_ref::Path {
                    target: type_ref::Target::Script { definition: owner, .. },
                    ..
                }) if *owner == definition)
                && names(candidate.path.as_str())
        })?;
        let key = self.models.definition(definition).definition().key();
        let owner = key
            .modules()
            .iter()
            .map(ImmutableString::as_str)
            .chain([key.name()])
            .collect::<Vec<_>>()
            .join("::");
        Some((owner, implemented.path.clone()))
    }

    /// `dyn Trait` of a crate trait that can be made into an object.
    /// `dyn Shape<Output = i64>` gives each associated type (E0191). A
    /// native trait's object (`type_ref::is_native_trait`) is the crate's,
    /// which checked the trait.
    pub(super) fn dyn_type(&mut self, trait_name: &str, span: Span) -> Result<Ty, Error> {
        let mut bindings = Vec::new();
        let mut positional = Vec::new();
        if matches!(self.peek(), Token::LessThan) {
            self.next();
            // `dyn Convert<i64, ..>`: a generic trait's arguments first.
            while !(matches!(self.peek(), Token::Identifier(_))
                && matches!(self.peek_second(), Token::Equals))
            {
                positional.push(self.parse_local_type()?);
                if !matches!(self.peek(), Token::Comma) {
                    break;
                }
                self.next();
            }
            while matches!(self.peek(), Token::Identifier(_))
                && matches!(self.peek_second(), Token::Equals)
            {
                let (name, _) = self.expect_name("associated type binding of a trait object")?;
                self.expect(Token::Equals, "'=' in an associated type binding")?;
                let ty = self.parse_local_type()?;
                bindings.push((name, ty));
                if !matches!(self.peek(), Token::Comma) {
                    break;
                }
                self.next();
            }
            self.expect_close_angle("'>' after associated type bindings")?;
        }
        // `dyn Iterator<Item = T>` is Rawr's iterator of `T`, as `impl
        // Iterator<Item = T>` is.
        if matches!(
            trait_name,
            "Iterator" | "DoubleEndedIterator" | "ExactSizeIterator"
        ) && !self.models.traits.contains_key(trait_name)
        {
            return match (&positional[..], &mut bindings[..]) {
                ([], [(name, item)]) if name.as_str() == "Item" => {
                    Ok(Ty::iter(std::mem::replace(item, Ty::Unknown)))
                }
                _ => Err(Error::UnsupportedNominalSyntax {
                    construct: format!("`dyn {trait_name}` without its `<Item = T>`").into(),
                    source: span,
                }),
            };
        }
        // A native trait's object: the crate checked the trait.
        if !self.models.traits.contains_key(trait_name) && type_ref::is_native_trait(trait_name) {
            bindings.extend(
                positional
                    .into_iter()
                    .enumerate()
                    .map(|(index, ty)| (ImmutableString::from(index.to_string()), ty)),
            );
            bindings.sort_by(|(a, _), (b, _)| a.cmp(b));
            return Ok(Ty::Dyn(trait_name.into(), bindings.into_boxed_slice()));
        }
        let Some((_, declared)) = self.models.traits.get(trait_name) else {
            return Err(Error::UnknownType {
                name: trait_name.into(),
                reference: span,
                path: false,
                imports: Box::default(),
            });
        };
        let names = bindings
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        if let Some(error) =
            crate::resolve::dyn_incompatibility(declared, &names, span, &self.models.traits)
        {
            return Err(error);
        }
        if positional.len() != declared.generics.len() {
            return Err(Error::InvalidTypeArguments {
                ty: trait_name.into(),
                expected: declared.generics.len(),
                actual: positional.len(),
                reference: span,
            });
        }
        bindings.extend(
            positional
                .into_iter()
                .enumerate()
                .map(|(index, ty)| (ImmutableString::from(index.to_string()), ty)),
        );
        bindings.sort_by(|(a, _), (b, _)| a.cmp(b));
        Ok(Ty::Dyn(trait_name.into(), bindings.into_boxed_slice()))
    }

    /// `for<'a, 'b>` before a body's type: its lifetimes join
    /// `bound_lifetimes`, and the length to truncate it back to is returned.
    pub(super) fn parse_local_binder(&mut self) -> Result<usize, Error> {
        let outer = self.bound_lifetimes.len();
        self.expect(Token::For, "`for` of a higher-ranked type")?;
        self.expect(Token::LessThan, "'<' after `for` in a higher-ranked type")?;
        while !matches!(self.peek(), Token::GreaterThan) {
            self.expect(Token::Apostrophe, "a lifetime in a higher-ranked type")?;
            match self.next().0 {
                Token::Identifier(name) => self.bound_lifetimes.push(name.as_str().into()),
                _ => {
                    self.bound_lifetimes.truncate(outer);
                    return Err(self.unsupported("a lifetime name after '\''"));
                }
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::GreaterThan) {
                self.bound_lifetimes.truncate(outer);
                return Err(self.unsupported("bound lifetimes without ','"));
            }
        }
        self.next();
        Ok(outer)
    }

    pub(super) fn skip_lifetime(&mut self) -> bool {
        if !matches!(self.peek(), Token::Apostrophe) {
            return false;
        }
        let at = self.next().1;
        // `let r: &'static u32 = &c;`, `f::<&'a u32>(&c)`: a lifetime a
        // body's type names, which these facts do not read yet. One a
        // `for<'a>` binds is an elided one.
        let elided = match self.next().0 {
            Token::Underscore => true,
            Token::Identifier(name) => self
                .bound_lifetimes
                .iter()
                .any(|bound| bound.as_str() == name.as_str()),
            _ => false,
        };
        if !elided {
            self.flow()
                .unmodeled("a lifetime written in a body's type", point_span(at));
        }
        true
    }

    pub(super) fn resolve_type(&mut self, name: &str, span: Span) -> Result<Ty, Error> {
        if self.generics.contains_key(name) {
            return Ok(Ty::param(name.into()));
        }
        // `T` of `trait Convert<T>` in its impl's bodies.
        if let Some(ty) = self.trait_parameters.get(name) {
            return Ok(ty.clone());
        }
        Ok(match name {
            "bool" => Ty::Bool,
            "char" => Ty::Char,
            "String" => Ty::String,
            name if let Some(ty) = numeric_type(name) => ty,
            // `use iced::widget::markdown::Uri;`: a crate's alias of a
            // builtin.
            name if let Some(binding) = self.models.import(name)
                && matches!(binding.target, item::Import::Alias)
                && let Some(crate::host::Entry::Builtin(builtin)) =
                    self.models.host.entry(&binding.qualified)
                && let Some(ty) = builtin_path_type(builtin) =>
            {
                ty
            }
            // `use std::time::Duration;`: std's `Duration` and `Instant`.
            name if let Some(time) = self
                .std_scoped(name)
                .as_deref()
                .and_then(crate::resolve::std_time) =>
            {
                Ty::Time(match time {
                    "Duration" => Time::Duration,
                    _ => Time::Instant,
                })
            }
            _ => {
                #[cfg(feature = "rust-import")]
                if let Some(binding) = self.models.import(name).cloned()
                    && let item::Import::Type { definition, owner } = binding.target
                    && definition.parameters().is_empty()
                    && let Some(selector) = definition.imported_declaration()
                {
                    // A mirror is its marked type, as a closure's `|theme:
                    // &Theme|` names it through the crate root's re-export
                    // (#1484).
                    if let Some(marked) = ty::mirrored(&definition) {
                        return Ok(Ty::Script(marked, Box::default()));
                    }
                    self.push_imported_op(&binding.qualified, &selector, None, span)?;
                    return Ok(Ty::Host {
                        proof: HostTyProof::new(definition.clone(), Some(definition), owner),
                        arguments: Box::default(),
                    });
                }
                let scoped = self.std_scoped(name);
                if scoped.as_deref().is_some_and(crate::resolve::std_ordering) {
                    return Ok(Ty::Ordering);
                }
                if scoped.as_deref().is_some_and(crate::resolve::std_arguments) {
                    return self.parse_arguments_type();
                }
                if let Some(error) = scoped.as_deref().and_then(crate::resolve::std_parse_error) {
                    return Ok(Ty::ParseError(
                        ParseError::named(error).expect("a std parse error"),
                    ));
                }
                match self.models.get(name).map(Nominal::ty) {
                    Some(ty) => ty,
                    None => {
                        return Err(self.std_type_not_yet(name, span).unwrap_or_else(|| {
                            Error::UnknownType {
                                name: name.into(),
                                reference: span,
                                path: false,
                                imports: self.imports(name, Wanted::Type),
                            }
                        }));
                    }
                }
            }
        })
    }

    /// rustc's E0631: a closure or function passed where a callable of as
    /// many parameters is expected, whose parameters are not the callable's,
    /// as `|s: State|` where `Fn(&State)` is.
    pub(super) fn argument_signature(
        &mut self,
        expected: &Ty,
        actual: &Ty,
        at: Span,
    ) -> Option<Error> {
        let (Ty::Callable { inputs: want, .. } | Ty::Fn { inputs: want, .. }) = expected else {
            return None;
        };
        let (closure, have) = match actual {
            Ty::FnItem { key, inputs, .. } => (
                key.name().contains("{closure#") && !self.path_values.contains(&**key),
                inputs,
            ),
            // A closure or function the host is given, as its callable.
            Ty::Callable {
                inputs,
                item: Some(item),
                ..
            } => (item.key().name().contains("{closure#"), inputs),
            Ty::Fn { inputs, .. } => (true, inputs),
            _ => return None,
        };
        if want.len() != have.len() {
            return None;
        }
        let checkpoint = self.inference.checkpoint();
        let fits = want
            .iter()
            .zip(have.iter())
            .all(|(want, have)| self.unify_type(want, have));
        self.inference.rollback(checkpoint);
        if fits {
            return None;
        }
        let signature = |inputs: &[Ty], models: &_| {
            let inputs = inputs
                .iter()
                .map(|input| self.inference.resolve(input).name(models).to_string())
                .collect::<Vec<_>>();
            format!("fn({}) -> _", inputs.join(", "))
        };
        Some(Error::ArgumentSignature {
            closure,
            expected: signature(want, &self.models).into(),
            found: signature(have, &self.models).into(),
            at,
        })
    }

    pub(super) fn require_type(
        &mut self,
        expected: &Ty,
        actual: &Ty,
        span: Span,
    ) -> Result<(), Error> {
        if self.unify_type(expected, actual) {
            return Ok(());
        }
        let (mut expected, mut actual) = (
            self.inference.resolve(expected),
            self.inference.resolve(actual),
        );
        // One function's opaque output at two type arguments: rustc's label
        // names the arguments that differ (`expected `Sq`, found `Ci``).
        while let (Ty::Param(_, Parameter::Opaque(want)), Ty::Param(_, Parameter::Opaque(have))) =
            (&expected, &actual)
            && want.opaque == have.opaque
            && let Some((wanted, had)) = want
                .arguments
                .iter()
                .zip(have.arguments.iter())
                .find(|(wanted, had)| wanted != had)
        {
            (expected, actual) = (wanted.clone(), had.clone());
        }
        if let Some(error) = self.argument_signature(&expected, &actual, span) {
            return Err(error);
        }
        if matches!(actual, Ty::Unit) && self.else_less.contains(&span.start()) {
            return Err(Error::IfWithoutElse {
                expected: expected.name(&self.models),
                expression: span,
            });
        }
        let (expected_name, found) = super::ty::mismatched_names(&expected, &actual, &self.models);
        // A function's opaque output, which rustc's label describes.
        let opaque = match (expected.is_opaque(), actual.is_opaque()) {
            (true, true) => Some(crate::resolve::Opaqueness::Both),
            (true, false) => Some(crate::resolve::Opaqueness::Expected),
            (false, true) => Some(crate::resolve::Opaqueness::Found),
            (false, false) => None,
        };
        if let Some(opaque) = opaque {
            return Err(Error::OpaqueMismatch {
                expected: expected_name,
                actual: found,
                opaque,
                expression: self.expression_span(span),
            });
        }
        let mut found = found.to_string();
        // Arrays of two lengths: rustc's label for them.
        if let (
            Ty::Array {
                kind: sequence::Kind::Array(want),
                ..
            },
            Ty::Array {
                kind: sequence::Kind::Array(have),
                ..
            },
        ) = (&expected, &actual)
            && want != have
        {
            found += &format!(
                " (expected an array with a size of {want}, found one with a size of {have}; E0308)"
            );
        }
        Err(Error::TypeMismatch {
            expected: expected_name,
            actual: found.into(),
            expression: self.expression_span(span),
        })
    }

    /// A type alias's target, with the generic arguments written after its
    /// name, and each parameter not written its default, as
    /// `Element<'a, Message>` leaves `Theme` to its own.
    pub(super) fn parse_alias_type(
        &mut self,
        name: ImmutableString,
        name_span: Span,
        alias: &type_ref::Alias,
    ) -> Result<Ty, Error> {
        let mut arguments = Vec::new();
        if matches!(self.peek(), Token::LessThan) {
            self.next();
            while !matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                if !self.skip_lifetime() {
                    arguments.push(self.parse_local_argument()?);
                }
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else if !matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                    return Err(self.unsupported("generic arguments"));
                }
            }
            self.expect_close_angle("'>' after generic arguments")?;
        }
        let written = arguments.len();
        while arguments.len() < alias.params.len() {
            let Some(Some(default)) = alias.defaults.get(arguments.len()) else {
                break;
            };
            let substitution = alias
                .params
                .iter()
                .cloned()
                .zip(arguments.iter().cloned())
                .collect();
            arguments.push(instantiate(&Ty::from_resolved(default), &substitution));
        }
        if arguments.len() != alias.params.len() {
            return Err(Error::InvalidTypeArguments {
                ty: name,
                expected: alias.params.len(),
                actual: written,
                reference: name_span,
            });
        }
        let Some(target) = &alias.target else {
            return Err(Error::UnknownType {
                name,
                reference: name_span,
                path: false,
                imports: Box::default(),
            });
        };
        let substitution = alias.params.iter().cloned().zip(arguments).collect();
        Ok(instantiate(&Ty::from_resolved(target), &substitution))
    }

    /// A generic argument: a type, or a const argument's value as
    /// `Ty::Const` (`4`, `-1`, `'a'`, `{ 0 + 1 + 1 }`, a `const`'s name),
    /// which rustc evaluates while it compiles. `N` and `{ N }` of a const
    /// parameter are the parameter, as a type parameter is.
    pub(super) fn parse_local_argument(&mut self) -> Result<Ty, Error> {
        let start = self.position();
        let mut tokens = Vec::new();
        match self.peek() {
            Token::LeftBrace => {
                self.next();
                let mut depth = 1usize;
                loop {
                    let (token, _) = self.next();
                    match token {
                        Token::LeftBrace => depth += 1,
                        Token::RightBrace => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Token::EOF => return Err(self.unsupported("an unclosed const argument")),
                        _ => {}
                    }
                    tokens.push(token);
                }
            }
            Token::IntegerConstant(_) | Token::CharConstant(_) | Token::True | Token::False => {
                tokens.push(self.next().0);
            }
            Token::Minus | Token::UnaryMinus => {
                self.next();
                tokens.push(Token::UnaryMinus);
                tokens.push(self.next().0);
            }
            Token::Identifier(name) if self.names_constant(name.as_str()) => {
                tokens.push(self.next().0);
            }
            _ => return self.parse_local_type(),
        }
        let pieces = crate::item::constant::pieces(&tokens);
        if let Some([crate::item::constant::Piece::Name(name)]) = pieces.as_deref()
            && self.constants.contains_key(name)
        {
            return Ok(Ty::param(name.clone()));
        }
        let value = pieces.and_then(|pieces| {
            crate::item::constant::evaluate(&pieces, &mut |name| self.constant_value(name, 0))
        });
        let Some(value) = value else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a const generic argument other than a constant expression of \
                            literals and `const` items"
                    .into(),
                source: span(start, self.position()),
            });
        };
        Ok(Ty::Const(value.text().into()))
    }

    /// Whether `name`, read where a generic argument starts, is a `const`
    /// of the program, which no type or local of the name shadows.
    fn names_constant(&self, name: &str) -> bool {
        !self.generics.contains_key(name)
            && !self.bindings.contains_key(name)
            && self.models.get(name).is_none()
            && self.models.import(name).is_none()
            && self
                .models
                .function(name)
                .is_some_and(|function| function.definition.body.constant)
    }

    /// The types of a turbofish after its `::<`, through its `>`.
    pub(super) fn parse_turbofish_types(&mut self) -> Result<Vec<Ty>, Error> {
        let mut types = vec![self.parse_local_argument()?];
        while matches!(self.peek(), Token::Comma) {
            self.next();
            if matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                break;
            }
            types.push(self.parse_local_argument()?);
        }
        self.expect_close_angle("'>' after turbofish type arguments")?;
        Ok(types)
    }
}
