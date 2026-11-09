//! Method chains and indexing: the steps after an operand (`.field`,
//! `.method(..)`, `[index]`, `?`), and the array and sort methods.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_postfix(&mut self) -> Result<ParsedExpr, Error> {
        let start = self.flow().value_start();
        let value = self.parse_primary()?;
        let value = self.parse_postfix_of(value)?;
        // A value made from borrows that may still hold them, other than
        // the borrow or the holder itself: what a call's signature says
        // it keeps, which these facts do not follow yet. Only the
        // differential harness reads the note.
        if cfg!(test) && self.may_hold_borrow(&value.ty) {
            self.flow().escapes_unless_direct(value.span, start);
        }
        Ok(value)
    }

    /// The postfix operations after the primary `value`.
    pub(super) fn parse_postfix_of(&mut self, value: ParsedExpr) -> Result<ParsedExpr, Error> {
        self.parse_postfix_steps(value)
            .map_err(|error| self.inherent_not_yet(error))
    }

    /// A method std declares on a native type that rawr doesn't run yet
    /// resolves, and fails at its use (D712.1): `v.dedup_by_key(f)` is
    /// `Vec::dedup_by_key`'s not-yet error, never an unknown member.
    pub(super) fn inherent_not_yet(&self, error: Error) -> Error {
        if let Error::UnknownMember {
            member, reference, ..
        } = &error
            && let Some(ty) = self.std_probed.get(&reference.start())
            && let Some(found) = std_method::inherent(ty, member)
        {
            return Error::NotYet {
                path: std_method::path(&found).into(),
                reference: *reference,
            };
        }
        // An unstable one, as `Option::map_or_default`: rustc's E0658.
        if let Error::UnknownMember {
            member, reference, ..
        } = &error
            && let Some(ty) = self.std_probed.get(&reference.start())
            && let Some(unstable) = std_method::steps(ty).into_iter().find_map(|step| {
                crate::std_table::item(&format!("{}::{member}", step.owner))
                    .filter(|item| item.kind == "assoc-fn")
                    .and_then(|item| crate::resolve::std_path::unstable(item.stability, *reference))
            })
        {
            return unstable;
        }
        error
    }

    pub(super) fn parse_postfix_steps(
        &mut self,
        mut value: ParsedExpr,
    ) -> Result<ParsedExpr, Error> {
        // `b.clone()` on a `Box<T>` clones the box: the type to give back.
        let mut rebox: Option<Ty> = None;
        // The method call the last step made, whose value is `value` now.
        let mut called: Option<(borrows::CallReceiver, ImmutableString)> = None;
        // Whether `value` is a method call's value, which is no place of a
        // local's even where the lowering keeps it as one (`v.last_mut()`).
        loop {
            if let Some(boxed) = rebox.take() {
                value.ty = boxed;
            }
            let mut from_call = false;
            if let Some((receiver, member)) = called.take() {
                self.finish_method_call(&receiver, &member, &value);
                from_call = true;
            }
            // The second field of `t.0.1`, which reads as one number.
            let pending = self.pending_member.take();
            if pending.is_none() && matches!(self.peek(), Token::LeftBracket) {
                let base = (value.span, value.ty.clone());
                value = self.parse_index(value)?;
                self.flow().index_end(value.span.start());
                self.part_of(
                    (base.0, &base.1),
                    value.span,
                    &value.ty,
                    borrows::PartOf::Element,
                );
                continue;
            }
            // `make()(x)`, `(self.f)(x)`: a call of a function value.
            if pending.is_none() && matches!(self.peek(), Token::LeftParen | Token::Unit) {
                let mut ty = self.inference.resolve(&value.ty);
                let mut through = 0;
                loop {
                    ty = match ty {
                        Ty::Reference { target, .. } | Ty::Boxed(target) => {
                            self.inference.resolve(&target)
                        }
                        // An `Rc<dyn Fn(A) -> B>` is called through its
                        // value.
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
                if matches!(ty, Ty::Fn { .. } | Ty::FnItem { .. } | Ty::FnChoice { .. }) {
                    self.reject_invocation(&ty, value.span, "called as a function")?;
                    let start = value.span;
                    for _ in 0..through {
                        self.counted_value(&mut value);
                    }
                    let callee = value.rendered_source().surrounded("(", ")");
                    value = self.call_function_value(callee, ty, "<function>".into(), start)?;
                    continue;
                }
            }
            // `future.await`.
            if pending.is_none()
                && matches!(self.peek(), Token::Period)
                && matches!(self.peek_second(), Token::Reserved(ref keyword) if keyword.as_str() == "await")
            {
                self.next();
                let at = self.next().1;
                value = self.parse_await(value, at)?;
                continue;
            }
            // `value?`, also as the `?` of `?.` and `?[`.
            match self.peek() {
                _ if pending.is_some() => {}
                Token::Reserved(keyword) if keyword.as_str() == "?" => {
                    let end = self.next().1;
                    value = self.parse_try(value, end)?;
                    continue;
                }
                Token::Period => {}
                _ => break,
            }
            let (member, member_span) = match pending {
                Some(member) => member,
                None => {
                    self.next();
                    self.expect_member_name()?
                }
            };
            // A member of a call a recovered miss stands for (`recover_miss`)
            // is that miss's too: `!` again. A member of the call of a type
            // Rawr doesn't know (`Column::with_children(..).spacing(10)`) is
            // asked of that type; any other names nothing more to ask.
            #[cfg(feature = "rust-import")]
            if self.recover_misses
                && !self.recovered.is_empty()
                && matches!(self.inference.resolve(&value.ty), Ty::Never)
            {
                let start = value.span.start();
                let owner = self
                    .recovered_types
                    .iter()
                    .find(|(at, _)| *at == start)
                    .map(|(_, owner)| owner.clone());
                let known = owner.is_some();
                let absorbed = Error::UnknownMember {
                    owner: owner.unwrap_or_else(|| "!".into()),
                    member,
                    reference: member_span,
                };
                value = self.recover_miss(start, |_| Err(absorbed))?;
                if !known {
                    self.recovered.pop();
                }
                continue;
            }
            // A member of a local whose type nothing has given yet, as
            // `let mut it = todo!(); it.next()`: rustc needs the type here
            // (E0282).
            if matches!(value.lvalue, Some(Lvalue::Binding { .. }))
                && let Some(error) = self.unsettled(&value.ty, value.span, None)
            {
                return Err(error);
            }
            // `visitor.by_name::<String>(v)`: a program method's turbofish,
            // which its generic parameters take, as `f::<T>(..)`'s do.
            if matches!(self.peek(), Token::DoubleColon)
                && matches!(self.peek_second(), Token::LessThan)
                && (self.program_method(&value.ty, &member) || self.host_method(&value.ty, &member))
            {
                self.next();
                self.next();
                self.pending_turbofish = Some(self.parse_turbofish_types()?);
            }
            // `Trait::method::<T>(receiver)` rewritten to a method call
            // (`path_method_call`) keeps its turbofish.
            if let Some(turbofish) = self.spliced_turbofish.remove(&member_span.start()) {
                self.pending_turbofish = Some(turbofish);
            }
            // A method call borrows its place; a field is a place below it.
            if matches!(self.peek(), Token::LeftParen | Token::Unit) {
                self.call_on_place(&value);
                called = Some((self.call_receiver(&value, from_call), member.clone()));
            } else {
                self.project_place(
                    &value,
                    crate::resolve::borrowck::Projection::Field(member.clone()),
                );
            }
            // An `Rc`'s, `Arc`'s or `Weak`'s own method; any other member of
            // an `Rc` or `Arc` is its value's, through std's `Deref`.
            if let Some(parsed) = self.counted_member(&mut value, &member, member_span)? {
                value = parsed;
                continue;
            }
            // `Box<T>` auto-derefs for method calls and fields, as in Rust.
            if let Some((unboxed, boxed)) = self.unbox(&value.ty) {
                // `boxed.as_ref()` is a reference to the boxed value, as
                // `self.on_change.as_ref()` lends a boxed closure.
                if matches!(member.as_str(), "as_ref" | "as_mut")
                    && matches!(self.peek(), Token::Unit)
                {
                    let end = self.next().1;
                    let target = match unboxed {
                        Ty::Reference { target, .. } => *target,
                        unboxed => unboxed,
                    };
                    value.ty = Ty::Reference {
                        mutable: member.as_str() == "as_mut",
                        target: Box::new(target),
                    };
                    value.span = span(value.span.start(), end);
                    value.lvalue = None;
                    continue;
                }
                if member.as_str() == "clone" {
                    rebox = Some(boxed);
                }
                value.ty = unboxed;
            }
            // A `Cow`'s own method; any other member is its `Deref`
            // target's.
            if let Some(parsed) = self.cow_member(&mut value, &member, member_span)? {
                value = parsed;
                continue;
            }
            while let Ty::Cow(_) = self.inference.resolve(&value.ty) {
                let owner = self.inference.resolve(&value.ty);
                let Some((target, call)) = self.deref_target(&owner, false) else {
                    break;
                };
                self.apply_derefs(&mut value, vec![call], target)?;
            }
            if let Ty::Reference { target, .. } = self.inference.resolve(&value.ty)
                && let Ty::Cow(_) = self.inference.resolve(&target)
                && let Some((target, call)) = self.deref_target(&target, false)
            {
                self.apply_derefs(&mut value, vec![call], target)?;
            }
            // `(*s).len()`: `str`'s methods take it by reference, which the
            // probe's autoref makes a `&str`.
            if self.inference.resolve(&value.ty) == Ty::UnsizedStr {
                value.ty = Ty::Str;
            }
            // A member a program's type lacks, through its `impl Deref`
            // (D711 step 3): `wrapper.len()` is `wrapper.deref().len()`.
            value = self.probe_derefs(value, &member, member_span)?;
            #[cfg(feature = "rust-import")]
            let (has_host, imported_definition) = {
                let resolved = self.inference.resolve(&value.ty);
                let method = matches!(self.peek(), Token::LeftParen | Token::Unit);
                if let Some(error) =
                    self.ambiguous_imported_member(&resolved, &member, member_span, method)
                {
                    return Err(error);
                }
                let host = resolved.host();
                (
                    host.is_some(),
                    resolved
                        .host_alias()
                        .filter(|definition| definition.identity().imported().is_some() && !method)
                        .cloned()
                        .or_else(|| {
                            (!method)
                                .then(|| self.imported_field_owner(&resolved, &member))
                                .flatten()
                        }),
                )
            };
            #[cfg(not(feature = "rust-import"))]
            let has_host = self.inference.resolve(&value.ty).host().is_some();
            // `value.into()` on a generic `T: Into<U>` is its bound's.
            let bounded_into = {
                let resolved = self.inference.resolve(&value.ty);
                bounded_value(&resolved).is_some_and(|bounded| {
                    self.bound_traits(&bounded)
                        .iter()
                        .any(|name| name == "Into")
                })
            };
            // `a.clone_from(&b)`: `Clone`'s provided method, which std's
            // body runs as `*self = source.clone()` (#759).
            if member == "clone_from"
                && matches!(self.peek(), Token::LeftParen)
                && self.provided_clone_from(&value)
            {
                value = self.parse_clone_from(value, member_span)?;
                continue;
            }
            // `x.try_into()` into a program type: its `TryFrom` impl, chosen
            // once the target is known, as `into` is (#757).
            if member == "try_into"
                && matches!(self.peek(), Token::Unit)
                && self.program_try_from(&self.inference.resolve(&value.ty))
            {
                let end = self.next().1;
                value = self.parse_try_into(value, end);
                continue;
            }
            if member == "into" && matches!(self.peek(), Token::Unit) && !bounded_into {
                let end = self.next().1;
                let source = self.inference.resolve(&value.ty);
                // A facade type's own `into` another facade type
                // (`gradient::Linear` into the container style it is the
                // background of). One into a number is `f32::from`'s.
                if let Ty::Host { proof, .. } = &source
                    && self
                        .models
                        .host_items(&format!("{}::into", proof.definition.path()))
                        .is_some_and(|items| {
                            items.iter().any(|item| {
                                item.kind() == crate::host::item::Kind::Method
                                    && item.signature().parameters().is_empty()
                                    && matches!(
                                        item.signature().output().shape(),
                                        crate::host::type_ref::Shape::Registered { .. }
                                    )
                            })
                        })
                {
                    // Chosen by the type the use site needs, as Rust infers
                    // `Into`'s target: `gradient.into()` is a container
                    // style, or a `Background` through `Background::from`.
                    // A generic body's facade calls carry witnesses, so it
                    // calls the type's own `into`.
                    if self.host_witnesses.is_empty() {
                        value = self.parse_host_into(value, &source, end)?;
                    } else {
                        self.pending_arguments = Some((Vec::new(), end));
                        value = self.parse_host_method(value, "into".into(), member_span)?;
                    }
                    continue;
                }
                // An imported crate type's or std type's `.into()`: the
                // target the use site needs, through that type's `From` of
                // it, as rustc infers `Some(color.into())` a `Background`
                // where the field is an `Option<Background>`, or an
                // `io::Error` a crate error through its `#[from]`.
                #[cfg(feature = "rust-import")]
                if let Ty::Host { .. } = &source
                    && self.host_witnesses.is_empty()
                    && self.projected_host_types(&source).is_empty()
                {
                    value = self.parse_host_into(value, &source, end)?;
                    continue;
                }
                // A value no `From` of the program takes, as a number: the
                // target the use site needs, through that crate type's
                // `From` of it, as rustc infers `size: 14.0.into()` a
                // `Pixels` through `From<f32>` and the literal an `f32`.
                #[cfg(feature = "rust-import")]
                if self.host_witnesses.is_empty()
                    && !matches!(
                        source,
                        Ty::Infer(_) | Ty::Param(_) | Ty::Projection(..) | Ty::Host { .. }
                    )
                    && !source.is_text()
                    && self.widget_erasure(&source).is_none()
                    && !self.converts_from(&source)
                {
                    value = self.parse_target_into(value, &source, Vec::new(), None, end);
                    continue;
                }
                // A number's `.into()`: the target the use site needs,
                // through std's `From` of it (`impl From<u8> for u16`), as
                // rustc infers it, and E0282 or E0283 when no use does.
                if matches!(source, Ty::Int(_) | Ty::Float(_))
                    && self.host_witnesses.is_empty()
                    && !self.converts_from(&source)
                {
                    value = self.parse_target_into(value, &source, Vec::new(), None, end);
                    continue;
                }
                value = if self.projected_host_types(&source).is_empty()
                    && self.converts_from(&source)
                {
                    self.parse_conversion_into(value, end)
                } else {
                    self.parse_into(value, end)?
                };
                continue;
            }
            // `Message::Changed.with(id)`: a method of a crate's trait in
            // scope on a function or closure, which the trait's blanket
            // impl over `Fn(..)` gives it, called as the trait's method
            // over its `Self` (`surface::member`), as rustc selects it.
            #[cfg(feature = "rust-import")]
            if matches!(self.peek(), Token::LeftParen | Token::Unit)
                && matches!(
                    self.inference.resolve(&value.ty),
                    Ty::Fn { .. } | Ty::FnItem { .. } | Ty::FnChoice { .. } | Ty::Callable { .. }
                )
                && let Some(path) = self.scoped_trait_member(&member)
            {
                let items = self
                    .models
                    .host_items(&path)
                    .expect("a scoped trait member has items")
                    .to_vec();
                let owner = self.models.item_owner(&path).cloned();
                let start = value.span.start();
                let (arguments, end) = self.parse_call_arguments("trait method arguments")?;
                // A `fn` pointer, as a tuple variant's constructor is as a
                // value, is `Fn` through the closure that calls it.
                if let Ty::Fn { inputs, .. } = self.inference.resolve(&value.ty) {
                    value = self.fn_pointer_closure(value, &inputs)?;
                }
                let mut inputs = Vec::with_capacity(arguments.len() + 1);
                self.move_local(&mut value)?;
                inputs.push(value);
                for mut argument in arguments {
                    self.move_local(&mut argument)?;
                    inputs.push(argument);
                }
                value = self.call_host_function(&items, owner, inputs, span(start, end))?;
                continue;
            }
            // `rows.cartesian_product(columns)`: a method of a crate's trait
            // in scope that every iterator has, as itertools' `Itertools`,
            // on a program's iterator or a crate's own, called as the
            // trait's method over its `Self` (`surface::member`). std's own
            // methods come first, as rustc finds an inherent method or
            // `Iterator`'s before a trait's.
            #[cfg(feature = "rust-import")]
            if matches!(self.peek(), Token::LeftParen | Token::Unit)
                && self.iterator_value(&value.ty)
                && std_method::iterator(&member, |_| true).is_none()
                && std_method::inherent(&self.inference.resolve(&value.ty), &member).is_none()
                && let Some(path) = self.scoped_trait_member(&member)
            {
                let items = self
                    .models
                    .host_items(&path)
                    .expect("a scoped trait member has items")
                    .to_vec();
                let owner = self.models.item_owner(&path).cloned();
                let start = value.span.start();
                let (arguments, end) = self.parse_call_arguments("trait method arguments")?;
                let mut inputs = Vec::with_capacity(arguments.len() + 1);
                self.move_local(&mut value)?;
                inputs.push(value);
                for mut argument in arguments {
                    self.move_local(&mut argument)?;
                    inputs.push(argument);
                }
                value = self.call_host_function(&items, owner, inputs, span(start, end))?;
                continue;
            }
            // `value.fmt(f)` in a `Display` or `Debug` `fmt` body writes the
            // value into its formatter, through the impl's own trait.
            if member == "fmt"
                && matches!(self.peek(), Token::LeftParen)
                && let Some(formatter) = self.formatter.clone()
                && matches!(self.peek_second(), Token::Identifier(name) if *name == *formatter)
            {
                value = self.parse_value_fmt(value, formatter)?;
                continue;
            }
            // `f.debug_struct("Life").field("cells", &n).finish()` and its
            // kin, in a `fmt` body.
            if matches!(
                member.as_str(),
                "debug_struct" | "debug_tuple" | "debug_list"
            ) && matches!(self.peek(), Token::LeftParen | Token::Unit)
                && self
                    .formatter
                    .as_ref()
                    .is_some_and(|formatter| value.source.as_str() == formatter.as_str())
            {
                value = self.parse_debug_builder(value, &member)?;
                continue;
            }
            // `f.write_str(text)` into a `fmt` body's formatter.
            if member == "write_str"
                && matches!(self.peek(), Token::LeftParen)
                && self
                    .formatter
                    .as_ref()
                    .is_some_and(|formatter| value.source.as_str() == formatter.as_str())
            {
                let (mut arguments, end) = self.parse_call_arguments("`write_str` arguments")?;
                let [text] = arguments.as_mut_slice() else {
                    return Err(Error::argument_count(
                        Callee::Method,
                        crate::resolve::Qualified("Formatter", &member).to_string(),
                        1,
                        arguments.len(),
                        span(value.span.start(), end),
                    ));
                };
                self.coerce_expr(&Ty::Str, text)?;
                let mut source = origin::Fragment::plain("{ ");
                source.push(value.rendered_source());
                source.push_text(" += (");
                source.push(text.rendered_source());
                source.push_text("); ");
                source.push(self.prelude_op(PreludeOp::Ok, ["()".into()]));
                source.push_text(" }");
                value = self.operator_expr(
                    source,
                    Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Unit)),
                    span(value.span.start(), end),
                );
                continue;
            }
            // A method call auto-derefs through every reference: `x.abs()`
            // of a `&&i64` is `i64::abs`. One reference is left, which
            // the builtin methods below look through.
            let mut builtin = self.inference.resolve(&value.ty);
            while let Ty::Reference { target, .. } = &builtin
                && let inner @ Ty::Reference { .. } = self.inference.resolve(target)
            {
                builtin = inner;
            }
            self.std_probed
                .insert(member_span.start(), without_references(&builtin).clone());
            // std's `&mut self` method on a local not declared `mut`
            // (rustc's E0596), as its declaration takes its receiver.
            if matches!(self.peek(), Token::LeftParen | Token::Unit)
                && !matches!(builtin, Ty::Reference { .. } | Ty::Str)
                && let Some(Lvalue::Binding { binding: name, .. }) = &value.lvalue
                && let Some(binding) = self.mutation_binding(name)
                && !binding.mutable
                && matches!(binding.kind, BindingKind::Local | BindingKind::Invocation)
                && std_method::inherent(&builtin, &member)
                    .is_some_and(|found| found.mode == receiver::Mode::Mutable)
                && !owner_id(&builtin).is_some_and(|owner| {
                    self.models
                        .trait_methods
                        .contains_key(&(owner, member.clone()))
                })
            {
                return Err(Error::BorrowConflict {
                    code: "E0596",
                    message: format!(
                        "cannot borrow `{name}` as mutable, as it is not declared as mutable"
                    )
                    .into(),
                    access: value.span,
                    borrow: Span::NONE,
                });
            }
            // std's method of a `String`, `Vec` or map that takes `self`
            // moves it out of its place (`s.into_bytes()`).
            if matches!(self.peek(), Token::LeftParen | Token::Unit)
                && matches!(
                    builtin,
                    Ty::String
                        | Ty::Array {
                            kind: sequence::Kind::Vec | sequence::Kind::Deque,
                            ..
                        }
                        | Ty::Keyed(..)
                )
                && std_method::inherent(&builtin, &member).is_some_and(|found| {
                    found.mode == receiver::Mode::Owned && found.owner != "slice"
                })
                && !owner_id(&builtin).is_some_and(|owner| {
                    self.models
                        .trait_methods
                        .contains_key(&(owner, member.clone()))
                })
            {
                self.move_local(&mut value.clone())?;
            }
            // `Trait::method(value, ..)` of a crate's trait on an `Option`,
            // `Result` or `Vec` the crate implements it for, as `column!`'s
            // `Widget::_boxed(maybe)` of an `Option<T>` that iced's `impl<T>
            // Widget for Option<T>` boxes: the trait's method over its
            // `Self` (`surface::member`), which rustc selects the impl for
            // by the crate's impl headers, never the std type's own method
            // of that name. Text keeps the trait's impl for `&str`
            // (`surface::text`).
            #[cfg(feature = "rust-import")]
            if matches!(&builtin, Ty::Option(_) | Ty::Result(..) | Ty::Array { .. })
                && self.spliced_traits.contains_key(&member_span.start())
            {
                match self.spliced_trait_call(value, &member, member_span)? {
                    Ok(called) => {
                        value = called;
                        continue;
                    }
                    Err(back) => value = back,
                }
            }
            // `Trait::f(0usize, ..)` of a crate's trait, where no type the
            // call can be on has `f`, as `de::Error::invalid_length(0usize,
            // ..)` of a type parameter whose `invalid_length` Rawr can't
            // take: the trait's member is unknown, at the call.
            #[cfg(feature = "rust-import")]
            if matches!(
                without_references(&builtin),
                Ty::Int(_) | Ty::Float(_) | Ty::Char | Ty::Bool
            ) && let Some(trait_path) = self.spliced_traits.get(&member_span.start()).cloned()
                && self
                    .models
                    .host_items(&format!("{trait_path}::{member}"))
                    .is_none()
            {
                return Err(Error::UnknownMember {
                    owner: trait_path,
                    member,
                    reference: member_span,
                });
            }
            // `a.min(b)`, `a.max(b)`, `a.clamp(lo, hi)` through `Ord`, unless
            // the type has its own method of that name.
            if matches!(member.as_str(), "min" | "max" | "clamp")
                && matches!(self.peek(), Token::LeftParen)
            {
                let mut receiver = builtin.clone();
                while let Ty::Reference { target, .. } | Ty::Boxed(target) = receiver {
                    receiver = self.inference.resolve(&target);
                }
                let own = owner_id(&receiver).is_some_and(|id| {
                    self.models.method(id, &member).is_some()
                        || self
                            .models
                            .trait_methods
                            .contains_key(&(id, member.clone()))
                });
                if !own && (matches!(receiver, Ty::Float(_)) || self.ord_type(&receiver).is_some())
                {
                    value = self.parse_ord_method(value, member, member_span)?;
                    continue;
                }
            }
            // std's `dyn Error`: its text and its `source`.
            if self.std_error(&builtin, true)
                && matches!(self.peek(), Token::LeftParen | Token::Unit)
            {
                value = self.parse_error_member(value, member, member_span)?;
                continue;
            }
            // A trait object has only its trait's methods, dispatched by
            // the value's runtime type.
            let object = match &builtin {
                object @ Ty::Dyn(..) => Some(object.clone()),
                Ty::Reference { target, .. } => match &**target {
                    object @ Ty::Dyn(..) => Some(object.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(object) = object {
                let Ty::Dyn(trait_name, _) = &object else {
                    unreachable!("matched a trait object");
                };
                // A native trait's methods are the crate's, which rawr
                // doesn't call through a trait object yet.
                if type_ref::is_native_trait(trait_name) {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!(
                            "calling `{member}` of the native trait object `{}` is not supported yet",
                            object.name(&self.models)
                        )
                        .into(),
                        source: member_span,
                    });
                }
                // The trait's own methods, then its supertraits'.
                let found = std::iter::once(trait_name.clone())
                    .chain(supertraits(&self.models, trait_name))
                    .find_map(|trait_name| {
                        let (_, declared) = self.models.traits.get(&trait_name)?;
                        declared
                            .functions
                            .iter()
                            .find(|function| function.name.text == member)
                            .map(|function| (trait_name.clone(), function.clone()))
                    });
                // std's blanket `ToString` of a `Display` object.
                if found.is_none()
                    && member.as_str() == "to_string"
                    && matches!(self.peek(), Token::Unit)
                    && let Some(text) = self.display_text(&object, value.rendered_source())
                {
                    let (_, end) = self.parse_borrowing_arguments("`to_string` arguments")?;
                    value = self.operator_expr(text, Ty::String, span(value.span.start(), end));
                    continue;
                }
                let (Some((trait_name, function)), Token::LeftParen | Token::Unit) =
                    (found, self.peek())
                else {
                    return Err(Error::UnknownMember {
                        owner: object.name(&self.models),
                        member,
                        reference: member_span,
                    });
                };
                let (arguments, end) =
                    self.parse_borrowing_arguments("trait object method arguments")?;
                value = self.call_trait_method(
                    value,
                    object.clone(),
                    &trait_name,
                    &function,
                    arguments,
                    member_span,
                    end,
                )?;
                continue;
            }
            // A generic value has only its bounds' methods: `clone` and
            // the functions of its user trait bounds.
            // An associated type's value likewise has its declared bounds'.
            if let Some(bounded) = bounded_value(&builtin) {
                let name = bounded.name(&self.models);
                let (arguments, end) =
                    self.parse_borrowing_arguments("generic value method arguments")?;
                if let Some((trait_name, function)) =
                    self.bound_trait_function(&bounded, &member, member_span)?
                {
                    value = self.call_trait_method(
                        value,
                        bounded.clone(),
                        &trait_name,
                        &function,
                        arguments,
                        member_span,
                        end,
                    )?;
                    continue;
                }
                if member.as_str() == "to_string"
                    && arguments.is_empty()
                    && let Some(text) = self.display_text(&bounded, value.rendered_source())
                {
                    value = self.operator_expr(text, Ty::String, span(value.span.start(), end));
                    continue;
                }
                if member.as_str() != "clone" || !arguments.is_empty() {
                    return Err(Error::UnknownMember {
                        owner: name,
                        member,
                        reference: member_span,
                    });
                }
                let param = bounded;
                if !self.type_provides(&param, capability::Kind::Clone) {
                    return Err(Error::MissingCapability {
                        owner: name,
                        capability: "Clone".into(),
                        operation: member_span,
                        derive: false,
                        required_by: None,
                    });
                }
                // A hand-written `Clone` runs for the value's type.
                let source = if self.models.std_traits.contains("Clone") {
                    Self::std_dispatch("Clone", "clone", value.rendered_source(), &[])
                } else {
                    self.prelude_op(PreludeOp::ValueClone, [value.rendered_source()])
                };
                value = self.operator_expr(source, param, span(value.span.start(), end));
                continue;
            }
            // A blanket impl's function (`impl<T: Bound> Trait for T`) on a
            // value no impl or inherent function of that name is for, as
            // `vec.into_content()` of inboard's `IntoContent`.
            if matches!(self.peek(), Token::LeftParen | Token::Unit)
                && let Some(definition) = self.blanket_receiver(&builtin, &member)
            {
                value = self.call_blanket_method(value, &definition, &member, member_span)?;
                continue;
            }
            // A crate trait's function on a builtin value (`5.area()`);
            // the builtin's own method of that name comes first, as
            // Rust's inherent methods do.
            if matches!(self.peek(), Token::LeftParen | Token::Unit) {
                let receiver = match &builtin {
                    Ty::Reference { target, .. } => (**target).clone(),
                    ty => ty.clone(),
                };
                // A `Vec<T>` or `[T; N]` without the method reaches its
                // `[T]`'s impl, through `Deref` or unsizing, as a slice.
                let receiver = match self.slice_impl_step(&receiver, &member) {
                    Some((slice, mutable)) => {
                        value.ty = Ty::Reference {
                            mutable,
                            target: Box::new(slice.clone()),
                        };
                        slice
                    }
                    None => receiver,
                };
                // Rust probes the receiver by value before it autorefs it:
                // on a value (not a reference), a trait function taking
                // `self` comes before an inherent one taking `&self`, and
                // one taking `&self` before an inherent `&mut self`.
                let by_value_first = |this: &Self, owner: type_def::Id| {
                    !matches!(builtin, Ty::Reference { .. })
                        && this
                            .models
                            .trait_methods
                            .get(&(owner, member.clone()))
                            .and_then(|definitions| {
                                definitions
                                    .iter()
                                    .filter_map(|definition| definition.receiver())
                                    .map(receiver_rank)
                                    .min()
                            })
                            .is_some_and(|rank| {
                                std_method::inherent(&receiver, &member)
                                    .is_some_and(|found| rank < found.rank)
                            })
                };
                if let Some(owner) = owner_id(&receiver)
                    && (owner.builtin_code().is_some() || matches!(receiver, Ty::Host { .. }))
                    && (std_method::inherent(&receiver, &member).is_none()
                        || by_value_first(self, owner))
                    && self
                        .models
                        .trait_methods
                        .contains_key(&(owner, member.clone()))
                {
                    let (args, end) = self.parse_call_arguments("method arguments")?;
                    let owner_name = receiver.name(&self.models);
                    value = match self.call_owner_method(
                        value,
                        owner,
                        owner_name.clone(),
                        &member,
                        member_span,
                        args,
                        end,
                    )? {
                        Ok(called) => called,
                        Err(_) => {
                            return Err(Error::UnknownMember {
                                owner: owner_name,
                                member,
                                reference: member_span,
                            });
                        }
                    };
                    continue;
                }
            }
            if self.iterator_receiver(&builtin, &member) {
                value = self.parse_iterator_method(value, member, member_span)?;
                continue;
            }
            let array = match &builtin {
                Ty::Array { element, .. } => Some((**element).clone()),
                Ty::Reference { target, .. } => match &**target {
                    Ty::Array { element, .. } => Some((**element).clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(element) = array {
                value = self.parse_array_method(value, element, member, member_span)?;
                continue;
            }
            if let Some(keyed) = self.keyed_type(&builtin) {
                value = self.parse_keyed_method(value, keyed, member, member_span)?;
                continue;
            }
            let option = match &builtin {
                Ty::Option(element) => Some((**element).clone()),
                Ty::Reference { target, .. } => match &**target {
                    Ty::Option(element) => Some((**element).clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(element) = option {
                value = self.parse_option_method(value, element, member, member_span)?;
                continue;
            }
            let result = match &builtin {
                Ty::Result(ok, err) => Some(((**ok).clone(), (**err).clone())),
                Ty::Reference { target, .. } => match &**target {
                    Ty::Result(ok, err) => Some(((**ok).clone(), (**err).clone())),
                    _ => None,
                },
                _ => None,
            };
            if let Some((ok, err)) = result {
                value = self.parse_result_method(value, ok, err, member, member_span)?;
                continue;
            }
            let reverse = match &builtin {
                Ty::Reverse(inner) => Some((**inner).clone()),
                Ty::Reference { target, .. } => match &**target {
                    Ty::Reverse(inner) => Some((**inner).clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(inner) = reverse
                && !matches!(
                    member.as_str(),
                    "cmp" | "partial_cmp" | "min" | "max" | "clamp"
                )
            {
                value = self.parse_reverse_member(value, inner, member, member_span)?;
                continue;
            }
            let cell = match &builtin {
                Ty::Cell(kind, inner) => Some((*kind, (**inner).clone())),
                Ty::Reference { target, .. } => match &**target {
                    Ty::Cell(kind, inner) => Some((*kind, (**inner).clone())),
                    _ => None,
                },
                _ => None,
            };
            if let Some((CellKind::Cell, inner)) = cell {
                value = self.parse_plain_cell_member(value, inner, member, member_span)?;
                continue;
            }
            if let Some((CellKind::Mutex, inner)) = cell {
                value = self.parse_mutex_member(value, inner, member, member_span)?;
                continue;
            }
            if let Some((_, inner)) = cell {
                value = self.parse_cell_member(value, inner, member, member_span)?;
                continue;
            }
            let thread = match &builtin {
                Ty::Thread(kind, inner) => Some((*kind, (**inner).clone())),
                Ty::Reference { target, .. } => match &**target {
                    Ty::Thread(kind, inner) => Some((*kind, (**inner).clone())),
                    _ => None,
                },
                _ => None,
            };
            if let Some((kind, inner)) = thread {
                value = self.parse_thread_member(value, kind, inner, member, member_span)?;
                continue;
            }
            let tuple = match &builtin {
                Ty::Tuple(members) => Some(members.clone()),
                Ty::Reference { target, .. } => match &**target {
                    Ty::Tuple(members) => Some(members.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(members) = tuple {
                let Some(index) = member
                    .parse::<usize>()
                    .ok()
                    .filter(|index| *index < members.len())
                else {
                    return Err(Error::UnknownMember {
                        owner: builtin.name(&self.models),
                        member,
                        reference: member_span,
                    });
                };
                let mut source = value.rendered_source();
                source.push_text(format!("[{index}]"));
                let end = text_end(member_span.start(), &member);
                // `t.0` of a local's place is a place, and so is `s.pair.0`.
                let lvalue = match &value.lvalue {
                    Some(Lvalue::Binding { binding, .. } | Lvalue::Place { binding, .. })
                        if value.prelude.is_empty() =>
                    {
                        Some(Lvalue::Place {
                            binding: binding.clone(),
                            ty: members[index].clone(),
                            place: source.clone(),
                        })
                    }
                    Some(lvalue @ (Lvalue::Field(_) | Lvalue::Nested(_))) => Some(nested_step(
                        lvalue,
                        PlaceStep::Member(index),
                        members[index].clone(),
                    )),
                    _ => None,
                };
                self.part_of(
                    (value.span, &value.ty),
                    span(value.span.start(), end),
                    &members[index],
                    borrows::PartOf::Member(index),
                );
                // A member of what can't be moved out of or assigned to, as
                // an `Arc`'s value, can't be either.
                if let Some(unmovable) = self.unmovable.get(&value.span).cloned() {
                    self.unmovable
                        .insert(span(value.span.start(), end), unmovable);
                }
                value = self.operator_expr(
                    source,
                    members[index].clone(),
                    span(value.span.start(), end),
                );
                value.lvalue = lvalue;
                continue;
            }
            let number = match &builtin {
                Ty::Reference { target, .. } => (**target).clone(),
                builtin => builtin.clone(),
            };
            // `a.eq(&b)` and `a.ne(&b)` on builtin values: `==` and `!=`.
            if matches!(member.as_str(), "eq" | "ne")
                && matches!(self.peek(), Token::LeftParen)
                && matches!(
                    without_references(&number),
                    Ty::Int(_)
                        | Ty::Float(_)
                        | Ty::Bool
                        | Ty::Char
                        | Ty::String
                        | Ty::Str
                        | Ty::Ordering
                )
            {
                let (mut arguments, end) = self.parse_call_arguments("comparison arguments")?;
                let call = span(value.span.start(), end);
                let [other] = arguments.as_mut_slice() else {
                    return Err(Error::argument_count(
                        Callee::Method,
                        crate::resolve::Qualified(&number.name(&self.models), &member).to_string(),
                        1,
                        arguments.len(),
                        call,
                    ));
                };
                let mut other = other.clone();
                while let Ty::Reference { target, .. } = self.inference.resolve(&other.ty) {
                    other.ty = *target;
                }
                let mut subject = value;
                subject.ty = without_references(&number).clone();
                let operator = if member.as_str() == "eq" {
                    Token::EqualsTo
                } else {
                    Token::NotEqualsTo
                };
                value = self.apply_binary(operator, None, subject, other)?;
                value.span = call;
                continue;
            }
            // `a.cmp(&b)` and `a.partial_cmp(&b)` on builtin values (Rust's
            // `Ord` and `PartialOrd` impls), and `Ordering`'s methods.
            if matches!(member.as_str(), "cmp" | "partial_cmp")
                && self.std_ordered(&number, member.as_str() == "partial_cmp")
            {
                value = self.parse_compare_method(value, number, member)?;
                continue;
            }
            if number == Ty::Ordering {
                value = self.parse_ordering_method(value, member, member_span)?;
                continue;
            }
            if let Ty::Time(time) = number {
                value = self.parse_time_method(value, time, member, member_span)?;
                continue;
            }
            // `e.kind()` of a `ParseIntError`: a borrow of its kind, which
            // Rawr's std holds as std's own `IntErrorKind`.
            if number == Ty::ParseError(ParseError::Int)
                && member.as_str() == "kind"
                && matches!(self.peek(), Token::Unit)
                && let Some(kind) = self.std_held("num::IntErrorKind")
            {
                let end = self.next().1;
                let source = self.prelude_op(PreludeOp::IntErrorKind, [value.rendered_source()]);
                let ty = Ty::Reference {
                    mutable: false,
                    target: Box::new(kind),
                };
                value = self.operator_expr(source, ty, span(value.span.start(), end));
                continue;
            }
            // `e.to_string()` and `e.clone()` of a `ParseIntError` and kin:
            // its message, which is its value.
            if let Ty::ParseError(error) = number
                && matches!(member.as_str(), "to_string" | "clone")
                && matches!(self.peek(), Token::Unit)
            {
                let end = self.next().1;
                let (ty, source) = if member.as_str() == "clone" {
                    (number, value.rendered_source())
                } else {
                    (
                        Ty::String,
                        self.parse_error_display(error, value.rendered_source()),
                    )
                };
                value = self.operator_expr(source, ty, span(value.span.start(), end));
                continue;
            }
            // `x.clone()` of a `Copy` scalar is the value.
            if member.as_str() == "clone"
                && matches!(
                    number,
                    Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Unit
                )
                && matches!(self.peek(), Token::Unit)
            {
                let end = self.next().1;
                let source = value.rendered_source();
                value = self.operator_expr(source, number, span(value.span.start(), end));
                continue;
            }
            if matches!(number, Ty::Int(_) | Ty::Float(_)) && member != "to_string" {
                value = self.parse_number_method(value, number, member, member_span)?;
                continue;
            }
            // `c.to_uppercase()` and `c.to_lowercase()`: std's iterators of
            // the chars its case maps to, which Rawr's std holds as std's
            // own.
            if number == Ty::Char
                && matches!(member.as_str(), "to_uppercase" | "to_lowercase")
                && matches!(self.peek(), Token::Unit)
                && let Some(ty) = self.std_held(if member.as_str() == "to_uppercase" {
                    "char::ToUppercase"
                } else {
                    "char::ToLowercase"
                })
            {
                let end = self.next().1;
                let upper = (member.as_str() == "to_uppercase").to_string();
                let source = self.prelude_op(
                    PreludeOp::CaseMapping,
                    [value.rendered_source(), upper.into()],
                );
                value = self.operator_expr(source, ty, span(value.span.start(), end));
                continue;
            }
            if number == Ty::Char
                && let Some(code) = CHAR_METHODS.iter().position(|(name, _)| *name == member)
            {
                value = self.parse_char_method(value, code, member_span)?;
                continue;
            }
            if matches!(number, Ty::InclusiveRange(_) | Ty::FloatRange) {
                value = self.parse_range_method(value, number, member, member_span)?;
                continue;
            }
            if number == Ty::Bool
                && matches!(member.as_str(), "then_some" | "then")
                && matches!(self.peek(), Token::LeftParen)
            {
                value = self.parse_bool_method(value, member)?;
                continue;
            }
            // `Trait::method(text, ..)` of a crate's trait, as `column!`'s
            // `Widget::_boxed("label")`: the method the trait's impl for
            // `&str` gives (`surface::text`).
            #[cfg(feature = "rust-import")]
            if (builtin.is_text()
                || matches!(&builtin, Ty::Reference { target, .. } if target.is_text()))
                && let Some(trait_path) = self.spliced_traits.get(&member_span.start()).cloned()
            {
                value = self.parse_text_member(value, &trait_path, member, member_span)?;
                continue;
            }
            if builtin.is_text()
                || matches!(&builtin, Ty::Reference { target, .. } if target.is_text())
                || member == "to_string"
                    && matches!(
                        without_references(&number),
                        Ty::Int(_) | Ty::Float(_) | Ty::Char | Ty::Bool
                    )
            {
                value = self.parse_string_method(value, member, member_span)?;
                continue;
            }
            #[cfg(feature = "rust-import")]
            if let Some(definition) = imported_definition {
                value = self.parse_imported_field(value, definition, member, member_span)?;
                continue;
            }
            if has_host {
                // `Trait::method(value, ..)` where the crate gives the
                // value's type no such member of its own, as `column!`'s
                // `Widget::_boxed(widget.map(f))` of a `Map`, whose impl
                // declares parameters its type does not have: the trait's
                // method over its `Self`, which rustc selects the impl for.
                #[cfg(feature = "rust-import")]
                if !self.host_has_member(&value.ty, &member) {
                    match self.spliced_trait_call(value, &member, member_span)? {
                        Ok(called) => {
                            value = called;
                            continue;
                        }
                        Err(back) => value = back,
                    }
                }
                value = self.parse_host_method(value, member, member_span)?;
                continue;
            }
            let Some(definition_id) = self.inference.resolve(&value.ty).script() else {
                // `f.curry(1)` on a closure: rustc's E0599, or E0609 for a field.
                let written = crate::tokenizer::raw_names(&member).into_owned();
                return Err(Error::UnknownMember {
                    owner: self.inference.resolve(&value.ty).name(&self.models),
                    reference: Span::new(
                        member_span.start(),
                        text_end(member_span.start(), &written),
                    ),
                    member: written.into(),
                });
            };
            // `Trait::method(value, ..)` of a crate's trait on a script type
            // that implements it, as `column!`'s `Widget::_boxed(circle)`:
            // the trait's method over its `Self` (`surface::member`), which
            // the script's own impl of the trait answers.
            #[cfg(feature = "rust-import")]
            if self.models.method(definition_id, &member).is_none()
                && !self
                    .models
                    .trait_methods
                    .contains_key(&(definition_id, member.clone()))
            {
                match self.spliced_trait_call(value, &member, member_span)? {
                    Ok(called) => {
                        value = called;
                        continue;
                    }
                    Err(back) => value = back,
                }
            }
            // A scripted widget's `map` and `boxed` (`erasure_with`).
            let resolved = self.inference.resolve(&value.ty);
            if matches!(self.peek(), Token::LeftParen | Token::Unit)
                && self.models.method(definition_id, &member).is_none()
                && !self
                    .models
                    .trait_methods
                    .contains_key(&(definition_id, member.clone()))
                && let Some(erased) = self.erasure_with(&resolved, &member)
            {
                self.coerce_expr(&erased, &mut value)?;
                value = self.parse_host_method(value, member, member_span)?;
                continue;
            }
            let model = self.models.definition(definition_id).clone();

            if matches!(self.peek(), Token::LeftParen | Token::Unit) {
                let (args, end) = self.parse_borrowing_arguments("method arguments")?;

                // `value.to_string()` through a hand-written `Display`.
                if member.as_str() == "to_string"
                    && args.is_empty()
                    && self.models.method(definition_id, &member).is_none()
                    && let Some(text) = self.display_text(
                        &Ty::Script(definition_id, Box::default()),
                        value.rendered_source(),
                    )
                {
                    value = self.operator_expr(text, Ty::String, span(value.span.start(), end));
                    continue;
                }
                let (returned, rest) = match self.call_owner_method(
                    value,
                    definition_id,
                    model.name().into(),
                    &member,
                    member_span,
                    args,
                    end,
                )? {
                    Ok(called) => {
                        value = called;
                        continue;
                    }
                    Err(rest) => rest,
                };
                value = returned;
                let args = rest;

                let capability = match member.as_str() {
                    "clone" => capability::Kind::Clone,
                    "copy" => capability::Kind::Copy,
                    "debug" => capability::Kind::Debug,
                    _ => {
                        return Err(self
                            .private_member(definition_id, &member, member_span)
                            .unwrap_or_else(|| Error::UnknownMember {
                                owner: model.name().into(),
                                member,
                                reference: member_span,
                            }));
                    }
                };
                if !args.is_empty() {
                    return Err(Error::argument_count(
                        Callee::Method,
                        crate::resolve::Qualified(model.name(), &member).to_string(),
                        0,
                        args.len(),
                        span(member_span.start(), end),
                    ));
                }
                self.require_capability(&model, capability, member_span)?;
                if capability == capability::Kind::Clone {
                    self.refuse_cell_clone(&value.ty, member_span)?;
                }
                let (op, ty) = if capability == capability::Kind::Debug {
                    (
                        Op::Debug {
                            definition: model.definition().clone(),
                        },
                        Ty::String,
                    )
                } else {
                    (
                        Op::Clone {
                            definition: model.definition().clone(),
                        },
                        // The clone has the receiver's type, arguments included.
                        match self.inference.resolve(&value.ty) {
                            Ty::Reference { target, .. } => *target,
                            ty => ty,
                        },
                    )
                };
                let helper = self.push_op(op);
                let expression_span = span(value.span.start(), end);
                value = ParsedExpr {
                    prelude: value.prelude,
                    source: origin::Fragment::call(&helper, [value.source]),
                    ty,
                    span: expression_span,
                    lvalue: None,
                    outcome: crate::host::usage::Outcome::Conservative,
                    evaluation: crate::host::usage::Evaluation::Required,
                }
                .retain_origin();
                continue;
            }

            let Nominal::Struct { definition, item } = model else {
                return Err(Error::UnknownMember {
                    owner: self.models.definition(definition_id).name().into(),
                    member,
                    reference: member_span,
                });
            };
            let Some((slot, field)) = item
                .fields
                .iter()
                .enumerate()
                .find(|(_, field)| field.name.text == member)
            else {
                return Err(Error::UnknownMember {
                    owner: definition.name().into(),
                    member,
                    reference: member_span,
                });
            };
            // A generic struct's field has the type its arguments give it.
            let arguments = match self.inference.resolve(&value.ty) {
                Ty::Script(_, arguments) => arguments,
                Ty::Reference { target, .. } => match *target {
                    Ty::Script(_, arguments) => arguments,
                    _ => Box::default(),
                },
                _ => Box::default(),
            };
            let substitution = self.nominal_substitution(definition_id, &arguments);
            let ty = instantiate(&Ty::from_resolved(&field.ty), &substitution);
            // A field of a value made here is read from its temporary, and
            // of a place read through a reference stays that place.
            let through_reference =
                matches!(self.inference.resolve(&value.ty), Ty::Reference { .. })
                    || self.borrowed.contains(&value.span);
            if let Some(temporary) = self.hold_temporary(&mut value)? {
                value.lvalue = Some(Lvalue::Binding {
                    binding: temporary,
                    ty: value.ty.clone(),
                });
            }
            let lvalue = match &value.lvalue {
                // `v[0].cells`, `s.a.b`: a field below an element or a field.
                Some(lvalue @ (Lvalue::Place { .. } | Lvalue::Field(_) | Lvalue::Nested(_))) => {
                    Some(nested_step(lvalue, PlaceStep::Field { slot }, ty.clone()))
                }
                _ => variable_name(value.source.as_str()).map(|binding| {
                    Lvalue::Field(lvalue::Field {
                        binding,
                        definition: definition.clone(),
                        slot,
                        member: member.clone(),
                        ty: ty.clone(),
                    })
                }),
            };
            let lvalue = match (lvalue, &value.lvalue) {
                (None, Some(Lvalue::Binding { binding, .. })) if self.drops() => {
                    Some(Lvalue::Field(lvalue::Field {
                        binding: binding.clone(),
                        definition: definition.clone(),
                        slot,
                        member: member.clone(),
                        ty: ty.clone(),
                    }))
                }
                (lvalue, _) => lvalue,
            };
            let owner = definition.id();
            let expression_span = span(value.span.start(), member_span.end());
            // A field of what can't be moved out of, as an `Rc`'s value,
            // can't be either (rustc's E0507).
            if let Some(unmovable) = self.unmovable.get(&value.span).cloned() {
                self.unmovable.insert(expression_span, unmovable);
            }
            self.part_of(
                (value.span, &value.ty),
                expression_span,
                &ty,
                borrows::PartOf::Field(owner, slot),
            );
            if through_reference && lvalue.is_none() && self.drops() {
                self.borrowed.insert(expression_span);
            }
            // The field is read where it is, `s[slot]`: a chain of fields
            // copies none of the values on the way (D568).
            value = ParsedExpr {
                prelude: value.prelude,
                source: field_place(value.source, slot),
                ty,
                span: expression_span,
                lvalue,
                outcome: crate::host::usage::Outcome::Conservative,
                evaluation: crate::host::usage::Evaluation::Required,
            }
            .retain_origin();
        }

        if let Some(boxed) = rebox {
            value.ty = boxed;
        }
        if let Some((receiver, member)) = called {
            self.finish_method_call(&receiver, &member, &value);
        }
        Ok(value)
    }

    /// rustc's E0624 for `member` of the script type `owner` where it is
    /// an inherent function private to a module this source is outside of
    /// and nothing else of that name is found.
    pub(super) fn private_member(
        &self,
        owner: type_def::Id,
        member: &str,
        reference: Span,
    ) -> Option<Error> {
        let definition = self.models.private_method(owner, member)?;
        Some(Error::PrivateItem {
            item: if definition.receiver().is_some() {
                crate::resolve::Private::Method
            } else {
                crate::resolve::Private::AssociatedFunction
            },
            name: definition.name().into(),
            reference,
            declaration: (definition.source() == self.models.context)
                .then_some(definition.name_span()),
        })
    }

    /// The blanket impl's function `member` a call on `receiver` reaches:
    /// one whose where-clauses hold for it, where neither its own type's
    /// impls nor its inherent or iterator methods have `member`.
    pub(super) fn blanket_receiver(
        &mut self,
        receiver: &Ty,
        member: &ImmutableString,
    ) -> Option<method::Definition> {
        let owner = type_def::Id::builtin(crate::resolve::BLANKET_OWNER);
        if !self
            .models
            .trait_methods
            .contains_key(&(owner, member.clone()))
        {
            return None;
        }
        let bare = without_references(receiver).clone();
        let own = owner_id(&bare).is_some_and(|id| {
            self.models.method(id, member).is_some()
                || self
                    .models
                    .trait_methods
                    .contains_key(&(id, member.clone()))
        });
        let iterator =
            matches!(bare, Ty::Iter(..)) && std_method::iterator(member, |_| true).is_some();
        if own || iterator || std_method::inherent(&bare, member).is_some() {
            return None;
        }
        self.blanket_method(receiver, member)
    }

    /// `value.member(args)` through the blanket impl's function
    /// `definition`. An impl over an iterator (`resolve::
    /// erase_iterator_owner`) takes the iterator over `value`.
    pub(super) fn call_blanket_method(
        &mut self,
        mut value: ParsedExpr,
        definition: &method::Definition,
        member: &ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        if matches!(impl_self_ty(definition, &BTreeMap::new()), Ty::Iter(..))
            && let Some(iterator) = self.iterator_of(&value)
        {
            value = iterator;
        }
        let (args, end) = self.parse_call_arguments("method arguments")?;
        let owner = type_def::Id::builtin(crate::resolve::BLANKET_OWNER);
        let owner_name = value.ty.name(&self.models);
        match self.call_owner_method(
            value,
            owner,
            owner_name.clone(),
            member,
            member_span,
            args,
            end,
        )? {
            Ok(called) => Ok(called),
            Err(_) => Err(Error::UnknownMember {
                owner: owner_name,
                member: member.clone(),
                reference: member_span,
            }),
        }
    }

    /// `value.member(args)` through `owner`'s inherent or trait function
    /// `member`; the value and arguments back when it has none.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub(super) fn call_owner_method(
        &mut self,
        mut value: ParsedExpr,
        owner: type_def::Id,
        owner_name: ImmutableString,
        member: &ImmutableString,
        member_span: Span,
        mut args: Vec<ParsedExpr>,
        end: Position,
    ) -> Result<Result<ParsedExpr, (ParsedExpr, Vec<ParsedExpr>)>, Error> {
        let member = member.clone();
        let self_typed = self.self_typed_function(owner, &member, &value.ty);
        // Two impls of one generic trait: the call's trait arguments
        // pick one, by inference.
        if self_typed.is_none()
            && self.receiver_method(owner, &member).is_none()
            && let Some((trait_name, function)) = self.generic_trait_method(owner, &member)
        {
            let receiver = match self.inference.resolve(&value.ty) {
                Ty::Reference { target, .. } => *target,
                ty => ty,
            };
            return self
                .call_trait_method(
                    value,
                    receiver,
                    &trait_name,
                    &function,
                    args,
                    member_span,
                    end,
                )
                .map(Ok);
        }
        if let Some(definition) =
            self_typed.or_else(|| self.probed_method(owner, &member, &value.ty))
        {
            let summary = self.method_summary(&definition);
            self.note_call(end, summary);
            let Some(mode) = definition.receiver() else {
                return Err(Error::InvalidMethodCall {
                    owner: owner_name.clone(),
                    method: member,
                    reason: "associated functions must be called with `Type::function(...)`".into(),
                    call: span(value.span.start(), end),
                });
            };
            let substitution = self.method_substitution(
                &definition,
                Some(&value.ty),
                span(member_span.start(), end),
            )?;
            self.require_arguments(
                &definition,
                &mut args,
                span(member_span.start(), end),
                &substitution,
            )?;
            let receiver_root = in_place(&value).map(|place| {
                let place = place.as_str();
                place[..place.find('[').unwrap_or(place.len())].to_string()
            });
            self.lend_arguments(
                &definition,
                &mut args,
                &substitution,
                receiver_root.as_deref(),
            );
            // A generic method compiled per instantiation is its instance
            // for the types this call gives it (D629).
            #[cfg(feature = "rust-import")]
            let definition = self.bind_method_instance(
                definition,
                &substitution,
                span(member_span.start(), end),
            )?;
            // rustc's E0040: `drop` runs where the value drops, not by name.
            if definition.trait_name().is_some_and(|name| name == "Drop") {
                return Err(Error::Syntax {
                    message: "explicit use of destructor method (E0040)".into(),
                    label: "explicit destructor calls not allowed".into(),
                    help: Some("consider using `drop` function: `drop(value)`".into()),
                    at: member_span,
                });
            }
            // `c.slot(1)` of `fn slot(&mut self, i: usize) -> &mut i32`:
            // the place the output borrows.
            let output = instantiate(&Ty::from_resolved(definition.output()), &substitution);
            // A `&mut self` method borrows its receiver for the call.
            if matches!(mode, receiver::Mode::Mutable) {
                self.mutable_use(&value, member_span);
            }
            if matches!(mode, receiver::Mode::Mutable)
                && definition.name() != "index_mut"
                && let Some(borrowed) = self.place_output(&output)
            {
                let call = span(value.span.start(), end);
                let receiver = self.borrowed_receiver(&value, call)?;
                let witnesses = self.witness_arguments(definition.generics(), &substitution, call);
                let arguments = origin::Fragment::join(
                    args.iter()
                        .map(ParsedExpr::rendered_source)
                        .chain(witnesses),
                    ", ",
                );
                let method = method_name(definition.id());
                let places =
                    self.mutable_arguments(&args, method_copied(&definition, &self.models))?;
                let mut source =
                    origin::Fragment::plain(format!("{}.{method}(", places::RECEIVER_PLACE));
                source.push(arguments);
                source.push_text(")");
                let source = self.copied_call(source, &places)?;
                return Ok(Ok(self.borrowed_call(&receiver, source, borrowed, call)));
            }
            // A nested receiver of a method that also copies `&mut`
            // arguments back is read and stored around the call, so the
            // copies' stores (`p.take(&mut s.t.0)`) come after its own.
            let mut inline = None;
            let writeback = match mode {
                receiver::Mode::Shared => {
                    self.hold_temporary(&mut value)?;
                    None
                }
                // An element (`v[0]`) or a place below a field
                // (`g.rows[i].cells[j]`) is changed where it is.
                receiver::Mode::Mutable
                    if let Some(Lvalue::Place { binding, place, .. }) = &value.lvalue =>
                {
                    self.require_mutable_binding(binding, member_span)?;
                    Some(Writeback::Place(place.clone()))
                }
                receiver::Mode::Mutable if let Some(Lvalue::Nested(nested)) = &value.lvalue => {
                    let nested = (**nested).clone();
                    self.require_mutable_binding(&nested.binding, member_span)?;
                    if method_copied(&definition, &self.models).is_empty() {
                        Some(Writeback::Place(self.nested_place(&nested)))
                    } else {
                        let (read, place, store) = self.nested_parts(&nested);
                        inline = Some((read, store));
                        Some(Writeback::Place(place))
                    }
                }
                receiver::Mode::Mutable => {
                    Some(self.require_mutable_receiver(&value, &definition, member_span)?)
                }
                // The method takes the value, as a move or copy does.
                receiver::Mode::Owned => {
                    self.move_local(&mut value)?;
                    Some(Writeback::Owned)
                }
            };
            let witnesses = self.witness_arguments(
                definition.generics(),
                &substitution,
                span(member_span.start(), end),
            );
            let copied = method_copied(&definition, &self.models);
            let places = self.mutable_arguments(&args, copied.iter().copied())?;
            self.move_copied_in(&mut args, &copied);
            let arguments = origin::Fragment::join(
                args.iter()
                    .map(ParsedExpr::rendered_source)
                    .chain(witnesses),
                ", ",
            );
            let method = method_name(definition.id());
            let source = match writeback {
                Some(Writeback::Owned) => {
                    let receiver = self.temporary("receiver");
                    let mut source = origin::Fragment::plain(format!("({{\nlet {receiver} = "));
                    source.push(value.rendered_source());
                    source.push_text(format!(";\n{receiver}.{method}("));
                    source.push(arguments);
                    source.push_text(")\n})");
                    source
                }
                // A literal builtin receiver (`5.area()`) is parenthesised.
                None if owner.builtin_code().is_some() => {
                    let mut source = value.rendered_source().surrounded("(", ")");
                    source.push_text(format!(".{method}("));
                    source.push(arguments);
                    source.push_text(")");
                    source
                }
                None | Some(Writeback::Direct) => {
                    let mut source = value.rendered_source();
                    source.push_text(format!(".{method}("));
                    source.push(arguments);
                    source.push_text(")");
                    source
                }
                Some(Writeback::Place(mut place)) => {
                    place.push_text(format!(".{method}("));
                    place.push(arguments);
                    place.push_text(")");
                    match inline {
                        Some((read, store)) => {
                            let result = self.temporary("result");
                            let mut source = origin::Fragment::plain("({ ");
                            source.push(read);
                            source.push_text(format!("let {result} = "));
                            source.push(place);
                            source.push_text("; ");
                            source.push(store);
                            source.push_text(format!("{result} }})"));
                            source
                        }
                        None => place,
                    }
                }
            };
            let source = self.copied_call(source, &places)?;
            let expression_span = span(value.span.start(), end);
            return Ok(Ok(ParsedExpr {
                prelude: origin::Fragment::default(),
                source,
                ty: instantiate(&Ty::from_resolved(definition.output()), &substitution),
                span: expression_span,
                lvalue: None,
                outcome: crate::host::usage::Outcome::Conservative,
                evaluation: crate::host::usage::Evaluation::Required,
            }
            .retain_origin()));
        }
        Ok(Err((value, args)))
    }

    /// The pointer `name` names in a path: `Box`, unless the program names
    /// something `Box`, and `Rc` or `Arc` as imported from std.
    pub(super) fn pointer_named(&self, name: &str) -> Option<Pointer> {
        if self.models.get(name).is_some() {
            return None;
        }
        match self.models.import(name) {
            Some(binding) => crate::resolve::std_pointer(&binding.qualified).map(Pointer::named),
            None => (name == "Box").then_some(Pointer::Box),
        }
    }

    /// The type a value of type `ty` auto-derefs to through each `Box`
    /// (keeping a reference to it as a reference), with the outermost box
    /// type; `None` when `ty` is not a box or a reference to one.
    pub(super) fn unbox(&mut self, ty: &Ty) -> Option<(Ty, Ty)> {
        let ty = self.inference.resolve(ty);
        let (mutable, boxed) = match &ty {
            Ty::Boxed(..) => (None, ty.clone()),
            Ty::Reference { mutable, target } if matches!(**target, Ty::Boxed(..)) => {
                (Some(*mutable), (**target).clone())
            }
            _ => return None,
        };
        // Each pointer's `Deref` target, from std's impls.
        let mut inner = boxed.clone();
        while let Ty::Boxed(..) = inner
            && let Some((target, std_impl::DerefCall::Value)) = self.deref_target(&inner, false)
        {
            inner = self.inference.resolve(&target);
        }
        let unboxed = match mutable {
            Some(mutable) => Ty::Reference {
                mutable,
                target: Box::new(inner),
            },
            None => inner,
        };
        Some((unboxed, boxed))
    }

    /// `value[index]` on an array, `Vec` or slice.
    pub(super) fn parse_index(&mut self, mut value: ParsedExpr) -> Result<ParsedExpr, Error> {
        // `rc[i]` indexes the value, through std's `Deref`.
        let mut pointer = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } = pointer {
            pointer = self.inference.resolve(&target);
        }
        if let Ty::Counted(_, element) = pointer {
            self.counted_value(&mut value);
            value.ty = Ty::Reference {
                mutable: false,
                target: element,
            };
        }
        self.indexed(&value);

        if let Some(place) = self.indexed_place(&value)? {
            return Ok(place);
        }
        self.next();
        let outer = std::mem::replace(&mut self.no_struct, false);
        let index = self.parse_range_or_expr();
        self.no_struct = outer;
        let index = index?;
        let end = self.expect(Token::RightBracket, "']' after an index")?;
        let index = match index {
            Ok(index) => index,
            Err((low, high, inclusive)) => {
                return self.parse_slice(value, low, high, inclusive, end);
            }
        };
        self.index_into(value, index, end)
    }

    /// An expression, or the bounds of a range (`a..b`, `a..=b`, `a..`,
    /// `..b`, `..=b`, `..`) as `Err`: what an index and `drain` take.
    #[allow(clippy::type_complexity)]
    pub(super) fn parse_range_or_expr(
        &mut self,
    ) -> Result<Result<ParsedExpr, (Option<ParsedExpr>, Option<ParsedExpr>, bool)>, Error> {
        let open = match self.peek() {
            Token::ExclusiveRange => Some(false),
            Token::InclusiveRange => Some(true),
            _ => None,
        };
        match open {
            Some(inclusive) => {
                self.next();
                let high = if matches!(self.peek(), Token::RightBracket | Token::RightParen) {
                    Ok(None)
                } else {
                    self.parse_expr().map(Some)
                };
                high.map(|high| Err((None, high, inclusive)))
            }
            None => {
                self.last_range = None;
                self.parse_expr().map(|index| match self.last_range.take() {
                    Some((low, high, inclusive, range)) if range == index.span => {
                        Err((Some(low), high, inclusive))
                    }
                    _ => Ok(index),
                })
            }
        }
    }

    pub(super) fn index_into(
        &mut self,
        value: ParsedExpr,
        mut index: ParsedExpr,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        if let Some(indexed) = self.index_impl(&value, &mut index, end)? {
            return Ok(indexed);
        }
        // `value[index]` of a crate type: its `impl Index<Idx>`'s `index`,
        // which the importer brings as any member of the type (#709).
        #[cfg(feature = "rust-import")]
        {
            let mut owner = self.inference.resolve(&value.ty);
            while let Ty::Reference { target, .. } = owner {
                owner = self.inference.resolve(&target);
            }
            if let Ty::Host { proof, .. } = &owner
                && proof.definition.identity().imported().is_some()
            {
                let expression = span(value.span.start(), end);
                self.pending_arguments = Some((vec![index], end));
                let mut indexed = self.parse_host_method(value, "index".into(), expression)?;
                if let Ty::Reference { target, .. } = self.inference.resolve(&indexed.ty) {
                    indexed.ty = *target;
                }
                indexed.span = expression;
                return Ok(indexed);
            }
        }
        // `cow[i]` of a `Cow<[T]>` or a reference to one: through its
        // `Deref`, as rustc's index auto-derefs.
        let mut value = value;
        while let Ty::Cow(_) = self.inference.resolve(&value.ty) {
            let owner = self.inference.resolve(&value.ty);
            let Some((target, call)) = self.deref_target(&owner, false) else {
                break;
            };
            self.apply_derefs(&mut value, vec![call], target)?;
        }
        if let Ty::Reference { target, .. } = self.inference.resolve(&value.ty)
            && let Ty::Cow(_) = self.inference.resolve(&target)
            && let Some((target, call)) = self.deref_target(&target, false)
        {
            self.apply_derefs(&mut value, vec![call], target)?;
        }
        if let Some(keyed) = self.keyed_type(&value.ty) {
            return self.keyed_index(value, keyed, index, end);
        }
        self.require_type(&Ty::Int(Int::Usize), &index.ty, index.span)?;
        let (element, kind) = match self.inference.resolve(&value.ty) {
            Ty::Array { element, kind } => (*element, kind),
            Ty::Reference { target, .. } => match *target {
                Ty::Array { element, kind } => (*element, kind),
                ty => {
                    return Err(Error::TypeMismatch {
                        expected: "an array, Vec or slice".into(),
                        actual: ty.name(&self.models),
                        expression: value.span,
                    })
                }
            },
            ty => {
                return Err(Error::TypeMismatch {
                    expected: "an array, Vec or slice".into(),
                    actual: ty.name(&self.models),
                    expression: value.span,
                })
            }
        };
        // Rust panics on a negative or out-of-range index. The index is a
        // block, so the evaluator places that panic at the block, which
        // stands for the whole expression, whose `[` a `Vec`'s panic names,
        // or for an array's or a slice's start alone, where rustc places
        // its panic: their index is built in, a `Vec`'s is `Index::index`.
        let mut checked = origin::Fragment::plain("{ let i = ");
        checked.push(index.rendered_source());
        checked.push_text("; if i < 0 { __rawr_throw \"index out of bounds\" } i }");
        checked.cover(match kind {
            sequence::Kind::Array(_) | sequence::Kind::Slice => point_span(value.span.start()),
            sequence::Kind::Vec | sequence::Kind::Deque => span(value.span.start(), end),
        });
        let length = kind.length();
        let mut source = value.rendered_source().surrounded("(", ")");
        source.push_text("[");
        source.push(checked.clone());
        source.push_text("]");
        let lvalue = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) => {
                let mut place = origin::Fragment::plain(binding.to_string());
                place.push_text("[");
                place.push(checked.clone());
                place.push_text("]");
                Some(Lvalue::Place {
                    binding: binding.clone(),
                    ty: element.clone(),
                    place,
                })
            }
            // An element of an element, or of a `&mut` parameter.
            Some(Lvalue::Place { binding, place, .. }) => {
                let mut place = place.clone();
                place.push_text("[");
                place.push(checked.clone());
                place.push_text("]");
                Some(Lvalue::Place {
                    binding: binding.clone(),
                    ty: element.clone(),
                    place,
                })
            }
            // `s.cells[i]`, `s.grid[r][c]`, `v[0].cells[j]`: below a field.
            Some(lvalue @ (Lvalue::Field(_) | Lvalue::Nested(_))) => Some(nested_step(
                lvalue,
                PlaceStep::Index(checked),
                element.clone(),
            )),
            _ => None,
        };
        // An element of a value that is no local's: rustc's E0508 for an
        // array's, E0507 for a `Vec`'s or a slice's, moved out by value.
        if lvalue.is_none() {
            let owner = self.inference.resolve(&value.ty);
            let message = match (&owner, length) {
                (Ty::Array { .. }, Some(_)) => format!(
                    "cannot move out of type `{}`, a non-copy array",
                    owner.name(&self.models)
                ),
                _ => format!("cannot move out of index of `{}`", owner.name(&self.models)),
            };
            let code = if length.is_some() { "E0508" } else { "E0507" };
            self.unmovable.insert(
                span(value.span.start(), end),
                Unplaced {
                    moved: (code, message),
                    assigned: None,
                },
            );
        }
        let mut indexed = self.operator_expr(source, element, span(value.span.start(), end));
        indexed.lvalue = lvalue;
        Ok(indexed)
    }

    /// `v[low..high]` of an array, `Vec` or slice, or a string by bytes:
    /// a copy of the range (`[T]`, `str`), which `&` borrows.
    pub(super) fn parse_slice(
        &mut self,
        value: ParsedExpr,
        low: Option<ParsedExpr>,
        high: Option<ParsedExpr>,
        inclusive: bool,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        let mut peeled = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } = peeled {
            peeled = self.inference.resolve(&target);
        }
        let ty = match peeled {
            // std has no `Index<Range<usize>>` for a `VecDeque`.
            Ty::Array {
                kind: sequence::Kind::Deque,
                ..
            } => {
                return Err(Error::MissingCapability {
                    owner: peeled.name(&self.models),
                    capability: "Index<Range<usize>>".into(),
                    operation: span(value.span.start(), end),
                    derive: false,
                    required_by: None,
                })
            }
            Ty::Array { element, .. } => Ty::Array {
                element,
                kind: sequence::Kind::Slice,
            },
            Ty::String | Ty::Str => Ty::Str,
            ty => {
                return Err(Error::TypeMismatch {
                    expected: "an array, Vec, slice or string".into(),
                    actual: ty.name(&self.models),
                    expression: value.span,
                })
            }
        };
        let mut bound =
            |bound: Option<ParsedExpr>, open: &str| -> Result<origin::Fragment, Error> {
                match bound {
                    Some(bound) => {
                        self.require_type(&Ty::Int(Int::Usize), &bound.ty, bound.span)?;
                        Ok(bound.rendered_source())
                    }
                    None => Ok(origin::Fragment::from(open)),
                }
            };
        let (low, high) = (bound(low, "0")?, bound(high, "-1")?);
        // `v[a..b]` of a local's array is a place a `&mut` borrow changes.
        let lvalue = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) if ty != Ty::Str => {
                Some((binding.clone(), origin::Fragment::plain(binding.as_str())))
            }
            Some(Lvalue::Place { binding, place, .. }) if ty != Ty::Str => {
                Some((binding.clone(), place.clone()))
            }
            _ => None,
        }
        .map(|(binding, base)| {
            Lvalue::Slice(Box::new(lvalue::Slice {
                binding,
                ty: ty.clone(),
                base,
                low: low.clone(),
                high: high.clone(),
                inclusive,
            }))
        });
        let source = self.prelude_op(
            PreludeOp::Slice,
            [
                value.rendered_source(),
                low,
                high,
                inclusive.to_string().into(),
            ],
        );
        let mut sliced = self.operator_expr(source, ty, span(value.span.start(), end));
        sliced.lvalue = lvalue;
        Ok(sliced)
    }

    /// The `[T]` whose program impl a trait method `member` of a `Vec<T>`
    /// or `[T; N]` `receiver` runs when the receiver's type has none of its
    /// own, and whether it takes `&mut self`: Rust's probe reaches the
    /// slice through `Deref` or unsizing. `[T]` and `Vec<T>` are each
    /// their own impl owner.
    pub(super) fn slice_impl_step(
        &self,
        receiver: &Ty,
        member: &ImmutableString,
    ) -> Option<(Ty, bool)> {
        let Ty::Array {
            element,
            kind: sequence::Kind::Vec | sequence::Kind::Array(_),
        } = receiver
        else {
            return None;
        };
        let has = |ty: &Ty| {
            owner_id(ty).and_then(|owner| self.models.trait_methods.get(&(owner, member.clone())))
        };
        if has(receiver).is_some() {
            return None;
        }
        let slice = Ty::sequence(sequence::Kind::Slice, (**element).clone());
        let mutable = has(&slice)?
            .iter()
            .any(|definition| definition.receiver() == Some(receiver::Mode::Mutable));
        Some((slice, mutable))
    }

    /// The range operand of `drain(r)` or `range(r)`, between `open` and
    /// `end`: a value of `usize`s, which holds no borrow, for the call's
    /// summary to read.
    pub(super) fn range_operand(&mut self, open: Position, end: Position) {
        let range = crate::resolve::borrowck::record::Argument {
            span: span(open, end),
            holds: false,
            returns: false,
        };
        self.flow().call_operands(&[range], end);
    }

    /// The `Vec`, array and slice methods Rust code reaches for.
    pub(super) fn parse_array_method(
        &mut self,
        value: ParsedExpr,
        element: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let receiver_ty = self.inference.resolve(&value.ty);
        // A method of another sequence type, as `push` of a `&[T]`.
        if std_method::foreign_sequence_method(&receiver_ty, &member) {
            return Err(Error::UnknownMember {
                owner: receiver_ty.name(&self.models),
                member,
                reference: member_span,
            });
        }
        let kind = match without_references(&receiver_ty) {
            Ty::Array { kind, .. } => *kind,
            _ => sequence::Kind::Vec,
        };
        // A `VecDeque` is an array at runtime, whose back is its end: its
        // methods `DEQUE_METHODS` names run as the `Vec`'s, and the others
        // std has are not run yet.
        // The member as written, whose declaration types its closures.
        let written = member.clone();
        let member: ImmutableString = if kind == sequence::Kind::Deque {
            let Some((_, runs)) = DEQUE_METHODS
                .iter()
                .find(|(name, _)| *name == member.as_str())
            else {
                return Err(Error::UnknownMember {
                    owner: receiver_ty.name(&self.models),
                    member,
                    reference: member_span,
                });
            };
            (*runs).into()
        } else {
            member
        };
        // `v.as_mut_slice()` is the place `v`, whose parts a pattern reaches.
        let whole = (member.as_str() == "as_mut_slice")
            .then(|| value.lvalue.clone())
            .flatten();
        // A sort or a mutable walk of `v[a..b]` changes a copy of the
        // range, stored back over it.
        if let Some(Lvalue::Slice(_)) = &value.lvalue
            && (member.starts_with("sort") || member.as_str() == "iter_mut")
        {
            let copy: ImmutableString = self.temporary("slice").into();
            let local = Binding {
                ty: value.ty.clone(),
                mutable: true,
                available: true,
                declaration: value.span,
                kind: BindingKind::Local,
            };
            let outer = self.bindings.insert(copy.clone(), local);
            let mut local = self.operator_expr(copy.as_str().into(), value.ty.clone(), value.span);
            local.lvalue = Some(Lvalue::Binding {
                binding: copy.clone(),
                ty: value.ty.clone(),
            });
            let changed = self.parse_array_method(local, element, member, member_span);
            match outer {
                Some(outer) => self.bindings.insert(copy.clone(), outer),
                None => self.bindings.remove(&copy),
            };
            let changed = changed?;
            let result = self.temporary("result");
            let mut source = origin::Fragment::plain(format!("{{ let {copy} = "));
            source.push(value.rendered_source());
            source.push_text(format!("; let {result} = "));
            source.push(changed.rendered_source());
            source.push_text("; ");
            source.push(self.store_into(&value, &copy)?);
            source.push_text(format!(" {result} }}"));
            let expression = span(value.span.start(), changed.span.end());
            return Ok(self.operator_expr(source, changed.ty, expression));
        }
        if member.as_str() == "iter_mut" {
            return self.parse_iter_mut(&value, element, member_span);
        }
        // The methods that drop some elements, or move them through an
        // iterator, other than `truncate` and `clear`.
        if matches!(
            member.as_str(),
            "drain"
                | "retain"
                | "retain_mut"
                | "dedup"
                | "dedup_by"
                | "dedup_by_key"
                | "resize"
                | "resize_with"
                | "fill"
                | "fill_with"
                | "extend"
                | "extend_from_slice"
                | "append"
                | "into_iter"
                | "map"
        ) && self.needs_drop(&element, member_span)?
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`{member}` on a `Vec` or array of values whose type implements `Drop`: \
                     dropping them where Rust does is not supported in Rawr yet"
                )
                .into(),
                source: member_span,
            });
        }
        // `v.drain(range)`: the removed elements, as an iterator.
        if member.as_str() == "drain" {
            let open = self.expect(Token::LeftParen, "'(' after `drain`")?;
            let bounds = self.parse_range_or_expr()?;
            let end = self.expect(Token::RightParen, "')' after `drain`'s range")?;
            self.range_operand(open, end);
            let Err((low, high, inclusive)) = bounds else {
                return Err(self.unsupported("`drain` of something other than a range"));
            };
            let mut operands = vec![origin::Fragment::from("4")];
            for (bound, open) in [(low, "0"), (high, "-1")] {
                operands.push(match bound {
                    Some(bound) => {
                        self.require_type(&Ty::Int(Int::Usize), &bound.ty, bound.span)?;
                        bound.rendered_source()
                    }
                    None => open.into(),
                });
            }
            operands.push(inclusive.to_string().into());
            let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
            let source = self.mutation_call(&value, &helper, operands)?;
            let ty = Ty::iter(element);
            return Ok(self.operator_expr(source, ty, span(value.span.start(), end)));
        }
        // A deque's `range(r)`: an iterator over references to the
        // elements of `r`, which is the array's own range.
        if kind == sequence::Kind::Deque && member.as_str() == "range" {
            let open = self.expect(Token::LeftParen, "'(' after `range`")?;
            let bounds = self.parse_range_or_expr()?;
            let end = self.expect(Token::RightParen, "')' after `range`'s range")?;
            self.range_operand(open, end);
            let Err((low, high, inclusive)) = bounds else {
                return Err(self.unsupported("`range` of something other than a range"));
            };
            let mut operands = vec![value.rendered_source().surrounded("(", ")")];
            for (bound, open) in [(low, "0"), (high, "-1")] {
                operands.push(match bound {
                    Some(bound) => {
                        self.require_type(&Ty::Int(Int::Usize), &bound.ty, bound.span)?;
                        bound.rendered_source()
                    }
                    None => open.into(),
                });
            }
            operands.push(inclusive.to_string().into());
            let slice = self.prelude_op(PreludeOp::Slice, operands);
            let slice = self.operator_expr(
                slice,
                Ty::sequence(sequence::Kind::Slice, element.clone()),
                span(value.span.start(), end),
            );
            let item = Ty::Reference {
                mutable: false,
                target: Box::new(element),
            };
            return Ok(self.array_iterator(&slice, item, end));
        }
        // `[T; N]::map(f)`: `[U; N]`, each element passed by value.
        if member.as_str() == "map"
            && let Ty::Array {
                kind: sequence::Kind::Array(length),
                ..
            } = self.inference.resolve(&value.ty)
        {
            let (mut arguments, end) =
                self.parse_std_arguments(&value.ty, &member, "array method arguments")?;
            let [callable] = arguments.as_mut_slice() else {
                return Err(Error::argument_count(
                    Callee::Method,
                    crate::resolve::Qualified(
                        &self.inference.resolve(&value.ty).name(&self.models),
                        &member,
                    )
                    .to_string(),
                    1,
                    arguments.len(),
                    span(member_span.start(), end),
                ));
            };
            let output = self.inference.fresh();
            let (item, items) = (self.temporary("map"), self.temporary("map"));
            let call =
                self.callable_call(callable, &[element], &output, vec![item.as_str().into()])?;
            let mut source =
                origin::Fragment::plain(format!("{{ let {items} = []; for {item} in "));
            source.push(value.rendered_source().surrounded("(", ")"));
            source.push_text(format!(" {{ {items}.push("));
            source.push(call);
            source.push_text(format!("); }} {items} }}"));
            let ty = Ty::Array {
                element: Box::new(output),
                kind: sequence::Kind::Array(length),
            };
            return Ok(self.operator_expr(
                source.surrounded("(", ")"),
                ty,
                span(value.span.start(), end),
            ));
        }
        // `v.get(low..high)`: std's `SliceIndex` of a range, an
        // `Option<&[T]>`. `VecDeque::get` takes an index alone.
        if member.as_str() == "get"
            && kind != sequence::Kind::Deque
            && matches!(self.peek(), Token::LeftParen)
        {
            self.next();
            let bounds = self.parse_range_or_expr()?;
            let end = self.expect(Token::RightParen, "')' after call arguments")?;
            let operands: Vec<_> = match &bounds {
                Ok(index) => vec![self.argument_fact(index)],
                Err((low, high, _)) => low
                    .iter()
                    .chain(high)
                    .map(|bound| self.argument_fact(bound))
                    .collect(),
            };
            self.flow().call_operands(&operands, end);
            match bounds {
                Ok(index) => self.pending_arguments = Some((vec![index], end)),
                Err((low, high, inclusive)) => {
                    let mut operands = vec![value.rendered_source().surrounded("(", ")")];
                    for (bound, open) in [(low, "0"), (high, "-1")] {
                        operands.push(match bound {
                            Some(bound) => {
                                self.require_type(&Ty::Int(Int::Usize), &bound.ty, bound.span)?;
                                bound.rendered_source()
                            }
                            None => open.into(),
                        });
                    }
                    operands.push(inclusive.to_string().into());
                    let source = self.prelude_op(PreludeOp::SliceGetRange, operands);
                    let slice = Ty::Reference {
                        mutable: false,
                        target: Box::new(Ty::sequence(sequence::Kind::Slice, element)),
                    };
                    let ty = Ty::Option(Box::new(slice));
                    return Ok(self.operator_expr(source, ty, span(value.span.start(), end)));
                }
            }
        }
        let reference = Ty::Reference {
            mutable: false,
            target: Box::new(element.clone()),
        };
        let (mut arguments, end) = if member.as_str() == "append" {
            self.parse_borrowing_arguments("array method arguments")?
        } else {
            self.parse_std_arguments(&receiver_ty, &written, "array method arguments")?
        };
        if let Some(sorted) = self.parse_sort_method(&value, &element, &member, &mut arguments)? {
            return Ok(self.operator_expr(sorted, Ty::Unit, span(value.span.start(), end)));
        }
        let receiver = value.rendered_source().surrounded("(", ")");
        let expression = span(value.span.start(), end);
        // `v.get_mut(i)`: `Some` of the element's place, or `None`.
        if member.as_str() == "get_mut"
            && let [index] = arguments.as_mut_slice()
        {
            self.coerce_expr(&Ty::Int(Int::Usize), index)?;
            let slot = self.temporary("index");
            let mut locate = origin::Fragment::plain(format!("{slot} = "));
            locate.push(index.rendered_source());
            locate.push_text(format!("; if {slot} >= "));
            locate.push(receiver.clone());
            locate.push_text(format!(".len() {{ {slot} = -1; }} "));
            return self.optional_element(&value, &element, &slot, locate, expression);
        }
        // `v.first_mut()`, `v.last_mut()`: as `get_mut` of the end's index.
        if matches!(member.as_str(), "first_mut" | "last_mut") && arguments.is_empty() {
            let slot = self.temporary("index");
            let mut locate = origin::Fragment::plain(format!("{slot} = "));
            locate.push(receiver.clone());
            locate.push_text(if member.as_str() == "last_mut" {
                ".len() - 1; "
            } else {
                ".len(); "
            });
            if member.as_str() == "first_mut" {
                locate.push_text(format!(
                    "if {slot} > 0 {{ {slot} = 0; }} else {{ {slot} = -1; }} "
                ));
            }
            return self.optional_element(&value, &element, &slot, locate, expression);
        }
        let (source, ty) = match (member.as_str(), arguments.as_mut_slice()) {
            ("len", []) => (receiver.surrounded("", ".len()"), Ty::Int(Int::Usize)),
            ("is_empty", []) => (receiver.surrounded("(", ".len() == 0)"), Ty::Bool),
            // `v.capacity()` of a local or a temporary: the runtime `Vec`'s.
            // A field, an element or a borrowed `Vec` is read through a
            // copy, whose capacity is its length. Rust grows a `Vec` of
            // one-byte items from 8, not 4, and gives a `Vec` of zero-size
            // items `usize::MAX`.
            ("capacity", []) => {
                if matches!(self.inference.resolve(&value.ty), Ty::Reference { .. })
                    || !matches!(value.lvalue, None | Some(Lvalue::Binding { .. }))
                    || matches!(
                        self.inference.resolve(&element),
                        Ty::Int(Int::U8 | Int::I8) | Ty::Bool | Ty::Unit
                    )
                {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct:
                            "`capacity()` of a `Vec` of one-byte or zero-size items, or of one \
                                    other than a local or a temporary"
                                .into(),
                        source: member_span,
                    });
                }
                let helper = self.push_op(Op::Prelude(PreludeOp::VecCapacity));
                (
                    receiver.surrounded("", &format!(".{helper}()")),
                    Ty::Int(Int::Usize),
                )
            }
            // `v.iter()` borrows the elements, through any reference to
            // `v`; `v.into_iter()` takes them, or borrows them through `&v`.
            ("iter", []) => {
                let item = Ty::Reference {
                    mutable: false,
                    target: Box::new(element),
                };
                return Ok(self.array_iterator(&value, item, end));
            }
            ("into_iter", []) => {
                let item = match self.inference.resolve(&value.ty) {
                    Ty::Reference { mutable, .. } => Ty::Reference {
                        mutable,
                        target: Box::new(element),
                    },
                    _ => {
                        // `v.into_iter()` takes `v`, which the iterator's
                        // chain still reads where it runs.
                        self.move_local(&mut value.clone())?;
                        element
                    }
                };
                return Ok(self.array_iterator(&value, item, end));
            }
            ("clone", []) => {
                self.refuse_cell_clone(&value.ty, member_span)?;
                let cloned = self.prelude_op(PreludeOp::ValueClone, [receiver]);
                match self.inference.resolve(&value.ty) {
                    Ty::Reference { target, .. } => (cloned, *target),
                    ty => (cloned, ty),
                }
            }
            ("to_vec", []) => (
                self.prelude_op(PreludeOp::ValueClone, [receiver]),
                Ty::Array {
                    element: Box::new(element),
                    kind: sequence::Kind::Vec,
                },
            ),
            // `[String]::join(sep)`, `concat()`: Rust's `Join` for strings
            // (`&str`, `String`) and, for `concat`, chars.
            ("join", [_]) | ("concat", []) => {
                let item = match self.inference.resolve(&element) {
                    Ty::Reference { target, .. } => self.inference.resolve(&target),
                    ty => ty,
                };
                let text = item.is_text() || member.as_str() == "concat" && item == Ty::Char;
                if !text {
                    return Err(Error::UnknownMember {
                        owner: self.inference.resolve(&value.ty).name(&self.models),
                        member,
                        reference: member_span,
                    });
                }
                let separator = match arguments.first_mut() {
                    Some(separator) => {
                        self.coerce_expr(&Ty::Str, separator)?;
                        separator.rendered_source()
                    }
                    None => "\"\"".into(),
                };
                (
                    self.prelude_op(PreludeOp::Join, [receiver, separator]),
                    Ty::String,
                )
            }
            // A slice of the whole, which references are at runtime.
            ("as_slice" | "as_mut_slice", []) => (
                receiver,
                Ty::Reference {
                    mutable: member.as_str() == "as_mut_slice",
                    target: Box::new(Ty::sequence(sequence::Kind::Slice, element)),
                },
            ),
            // `windows(n)`, `chunks(n)` and the other `SLICE_CHUNKS`: an
            // iterator over `&[T]` slices.
            (name, [size]) if SLICE_CHUNKS.contains(&name) => {
                self.coerce_expr(&Ty::Int(Int::Usize), size)?;
                let code = SLICE_CHUNKS.iter().position(|chunks| *chunks == name);
                let code = code.unwrap_or_default().to_string();
                let slices = self.prelude_op(
                    PreludeOp::SliceChunks,
                    [receiver, size.rendered_source(), code.into()],
                );
                let slice = Ty::sequence(sequence::Kind::Slice, element);
                let item = Ty::Reference {
                    mutable: false,
                    target: Box::new(slice.clone()),
                };
                let slices = self.operator_expr(
                    slices,
                    Ty::Array {
                        element: Box::new(item.clone()),
                        kind: sequence::Kind::Vec,
                    },
                    expression,
                );
                return Ok(self.array_iterator(&slices, item, end));
            }
            // `v.split_off(at)`: the elements from `at` on, as a new `Vec`
            // (or `VecDeque`).
            ("split_off", [at]) => {
                self.coerce_expr(&Ty::Int(Int::Usize), at)?;
                let code = vec_method_code("split_off").to_string();
                let operands = vec![code.into(), at.rendered_source(), "()".into(), "()".into()];
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let source = self.mutation_call(&value, &helper, operands)?;
                (source, Ty::sequence(kind, element))
            }
            // `first`, `last` and `get(i)`: an `Option<&T>`.
            ("first" | "last", []) | ("get", [_]) => {
                let index = match arguments.first_mut() {
                    Some(index) => {
                        self.coerce_expr(&Ty::Int(Int::Usize), index)?;
                        index.rendered_source()
                    }
                    None if member.as_str() == "first" => "0".into(),
                    None => "-1".into(),
                };
                let source = self.prelude_op(PreludeOp::SliceGet, [receiver, index]);
                let item = Ty::Reference {
                    mutable: false,
                    target: Box::new(element),
                };
                (source, Ty::Option(Box::new(item)))
            }
            // A `Vec` changed in place: by Rawr's own op with Rust's
            // panics, or the runtime's array methods where they agree.
            // `fill(x)` and `copy_from_slice(&src)` of a `Vec`, a slice or
            // a range `v[a..b]`.
            ("fill" | "copy_from_slice", [argument]) => {
                let expected = if member.as_str() == "fill" {
                    element
                } else {
                    Ty::Reference {
                        mutable: false,
                        target: Box::new(Ty::sequence(sequence::Kind::Slice, element)),
                    }
                };
                self.coerce_expr(&expected, argument)?;
                let code = vec_method_code(member.as_str()).to_string();
                let operands = vec![
                    code.into(),
                    "()".into(),
                    argument.rendered_source(),
                    "()".into(),
                ];
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let source = self.mutation_call(&value, &helper, operands)?;
                (source, Ty::Unit)
            }
            ("insert", [_, _])
            | ("remove" | "swap_remove", [_])
            | ("swap", [_, _])
            | ("push_front", [_])
            | ("dedup" | "pop" | "pop_front", []) => {
                // A deque's `push_front(x)` is `insert(0, x)`.
                let front = member.as_str() == "push_front";
                let name = if front { "insert" } else { member.as_str() };
                let code = vec_method_code(name);
                let mut operands = vec![origin::Fragment::from(code.to_string())];
                if front {
                    operands.push("0".into());
                }
                for (index, argument) in arguments.iter_mut().enumerate() {
                    let inserted = name == "insert" && (front || index == 1);
                    let expected = if inserted {
                        element.clone()
                    } else {
                        Ty::Int(Int::Usize)
                    };
                    self.coerce_expr(&expected, argument)?;
                    if inserted && self.needs_drop(&element, argument.span)? {
                        self.move_local(argument)?;
                    }
                    operands.push(argument.rendered_source());
                }
                operands.resize(4, "()".into());
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let source = self.mutation_call(&value, &helper, operands)?;
                let ty = match name {
                    "remove" | "swap_remove" => element,
                    "pop" | "pop_front" => Ty::Option(Box::new(element)),
                    _ => Ty::Unit,
                };
                (source, ty)
            }
            // A deque's `remove(i)`, `swap_remove_back(i)` and
            // `swap_remove_front(i)`, `None` past its end, and its
            // `rotate_left(n)` and `rotate_right(n)`.
            (
                "deque_remove" | "swap_remove_back" | "swap_remove_front" | "rotate_left"
                | "rotate_right",
                [index],
            ) if kind == sequence::Kind::Deque => {
                self.coerce_expr(&Ty::Int(Int::Usize), index)?;
                let code = vec_method_code(member.as_str()).to_string();
                let operands = vec![
                    code.into(),
                    index.rendered_source(),
                    "()".into(),
                    "()".into(),
                ];
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let source = self.mutation_call(&value, &helper, operands)?;
                let ty = if member.starts_with("rotate") {
                    Ty::Unit
                } else {
                    Ty::Option(Box::new(element))
                };
                (source, ty)
            }
            ("truncate", [_]) | ("reverse" | "clear", []) | ("retain", [_]) => {
                let mut operands = Vec::new();
                if let [argument] = arguments.as_mut_slice() {
                    let expected = if member.as_str() == "retain" {
                        Ty::Fn {
                            inputs: Box::new([reference.clone()]),
                            output: Box::new(Ty::Bool),
                        }
                    } else {
                        Ty::Int(Int::Usize)
                    };
                    self.coerce_expr(&expected, argument)?;
                    operands.push(argument.rendered_source());
                }
                // `truncate` and `clear` drop the elements past the length,
                // in order.
                if matches!(member.as_str(), "truncate" | "clear")
                    && self.needs_drop(&element, member_span)?
                {
                    let (length, kept, at) = (
                        self.temporary("length"),
                        self.temporary("kept"),
                        self.temporary("at"),
                    );
                    let mut source = origin::Fragment::plain(format!("{{ let {length} = "));
                    source.push(operands.pop().unwrap_or_else(|| "0".into()));
                    source.push_text(format!("; let {kept} = "));
                    source.push(receiver.clone());
                    source.push_text("; ");
                    let operands = if member.as_str() == "truncate" {
                        vec![origin::Fragment::plain(length.clone())]
                    } else {
                        Vec::new()
                    };
                    source.push(self.mutation_call(&value, member.as_str(), operands)?);
                    let drop =
                        drops::glue_call(&format!("{kept}[{at}]"), &self.drop_shape(&element));
                    source.push_text(format!(
                        "; let {at} = {length}; while {at} < {kept}.len() {{ {drop} \
                         {at} += 1; }} }}"
                    ));
                    return Ok(self.operator_expr(source, Ty::Unit, expression));
                }
                let source = if member.as_str() == "retain" {
                    let direct = self.prelude_op(PreludeOp::NativePlace, [value.rendered_source()]);
                    let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                    let mut native_arguments = vec![vec_method_code("retain").to_string().into()];
                    native_arguments.extend(operands.clone());
                    native_arguments.resize(4, "()".into());
                    let native = self.mutation_call(&value, &helper, native_arguments)?;
                    let script = self.mutation_call(&value, "retain", operands)?;
                    let mut source = direct.surrounded("if ", " { ");
                    source.push(native);
                    source.push_text("; } else { ");
                    source.push(script);
                    source.push_text("; }");
                    source
                } else {
                    self.mutation_call(&value, member.as_str(), operands)?
                };
                (source.surrounded("{ ", "; }"), Ty::Unit)
            }
            // `resize(n, value)` and `resize_with(n, f)`: the new items made
            // first, then one change of the `Vec` (`splice`), which drops the
            // items past `n` or adds the new ones.
            ("resize" | "resize_with", [length, argument]) => {
                self.coerce_expr(&Ty::Int(Int::Usize), length)?;
                let with = member.as_str() == "resize_with";
                let expected = if with {
                    Ty::Fn {
                        inputs: Box::default(),
                        output: Box::new(element.clone()),
                    }
                } else {
                    element.clone()
                };
                self.coerce_expr(&expected, argument)?;
                let direct = self.prelude_op(PreludeOp::NativePlace, [value.rendered_source()]);
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let native = self.mutation_call(
                    &value,
                    &helper,
                    vec![
                        vec_method_code(member.as_str()).to_string().into(),
                        length.rendered_source(),
                        argument.rendered_source(),
                        "()".into(),
                    ],
                )?;
                let (n, len, extra, made) = (
                    self.temporary("length"),
                    self.temporary("len"),
                    self.temporary("extra"),
                    self.temporary("made"),
                );
                let item = if with {
                    format!("{made}.__rawr_call()")
                } else {
                    made.clone()
                };
                let mut source = origin::Fragment::plain(format!("{{ let {n} = "));
                source.push(length.rendered_source());
                source.push_text(format!("; let {len} = "));
                source.push(receiver.clone());
                source.push_text(format!(".len(); let {extra} = []; let {made} = "));
                source.push(argument.rendered_source());
                source.push_text(format!(
                    "; while {len} + {extra}.len() < {n} {{ {extra}.push({item}); }} "
                ));
                let operands = vec![
                    origin::Fragment::plain(format!("if {n} < {len} {{ {n} }} else {{ {len} }}")),
                    origin::Fragment::plain(len.clone()),
                    origin::Fragment::plain(extra.clone()),
                ];
                source.push(self.mutation_call(&value, "splice", operands)?);
                source.push_text("; }");
                let mut dispatched = direct.surrounded("if ", " { ");
                dispatched.push(native);
                dispatched.push_text("; } else { ");
                dispatched.push(source);
                dispatched.push_text(" }");
                (dispatched, Ty::Unit)
            }
            // `extend` from an iterable of `T`, or of `&T` for a `Copy` `T`.
            ("extend" | "extend_from_slice", [argument]) => {
                let borrowed = match self.inference.resolve(&argument.ty) {
                    Ty::Reference { mutable, target } => match *target {
                        Ty::Array { element, .. } => Some((
                            argument.rendered_source(),
                            Ty::Reference {
                                mutable,
                                target: element,
                            },
                        )),
                        _ => None,
                    },
                    _ => None,
                };
                let Some((items, item)) = borrowed.or_else(|| self.collected_items(argument))
                else {
                    return Err(Error::TypeMismatch {
                        expected: "an array or an iterator to `extend` with".into(),
                        actual: argument.ty.name(&self.models),
                        expression: argument.span,
                    });
                };
                let mode = if member.as_str() == "extend_from_slice" {
                    "extend_clone"
                } else if matches!(self.inference.resolve(&item), Ty::Reference { .. }) {
                    "extend_copy"
                } else {
                    "extend_move"
                };
                let item = match self.inference.resolve(&item) {
                    Ty::Reference { target, .. } if self.unifies(&element, &target) => *target,
                    item => item,
                };
                self.require_type(&element, &item, argument.span)?;
                let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
                let source = self.mutation_call(
                    &value,
                    &helper,
                    vec![
                        vec_method_code(mode).to_string().into(),
                        items,
                        "()".into(),
                        "()".into(),
                    ],
                )?;
                (source.surrounded("{ ", "; }"), Ty::Unit)
            }
            // A deque's `as_slices()`: the whole, which is contiguous at
            // runtime and a slice of it as `as_slice` is, then an empty
            // slice.
            ("as_slices", []) if kind == sequence::Kind::Deque => {
                let source = receiver.surrounded("[", ", []]");
                let half = Ty::Reference {
                    mutable: false,
                    target: Box::new(Ty::sequence(sequence::Kind::Slice, element)),
                };
                (source, Ty::Tuple(Box::new([half.clone(), half])))
            }
            // `split_at(mid)`: the two halves, `(&[T], &[T])`.
            ("split_at", [mid]) => {
                self.coerce_expr(&Ty::Int(Int::Usize), mid)?;
                let source = self.prelude_op(PreludeOp::SplitAt, [receiver, mid.rendered_source()]);
                let half = Ty::Reference {
                    mutable: false,
                    target: Box::new(Ty::sequence(sequence::Kind::Slice, element)),
                };
                (source, Ty::Tuple(Box::new([half.clone(), half])))
            }
            ("contains", [argument]) => {
                let expected = Ty::Reference {
                    mutable: false,
                    target: Box::new(element.clone()),
                };
                self.coerce_expr(&expected, argument)?;
                (
                    self.prelude_op(
                        PreludeOp::SequenceContains,
                        [receiver, argument.rendered_source()],
                    ),
                    Ty::Bool,
                )
            }
            // The slice search family: a `Result<usize, usize>`, or
            // `partition_point`'s `usize`.
            ("binary_search" | "binary_search_by" | "partition_point", [_])
            | ("binary_search_by_key", [_, _]) => {
                let found =
                    self.binary_search(receiver, &element, &member, &mut arguments, member_span)?;
                let ty = if member.as_str() == "partition_point" {
                    Ty::Int(Int::Usize)
                } else {
                    Ty::Result(Box::new(Ty::Int(Int::Usize)), Box::new(Ty::Int(Int::Usize)))
                };
                (found, ty)
            }
            // `v.iter().min()`, `max`, `min_by_key`, `max_by_key`, `min_by`
            // and `max_by`: Rust's first minimum and last maximum.
            ("min" | "max", []) | ("min_by_key" | "max_by_key" | "min_by" | "max_by", [_])
                if matches!(self.inference.resolve(&value.ty), Ty::Reference { .. }) =>
            {
                let input = Ty::Reference {
                    mutable: false,
                    target: Box::new(reference.clone()),
                };
                let (source, item) = self.extremum(
                    receiver,
                    &element,
                    &member,
                    arguments.first_mut().map(|by| (by, input)),
                    member_span,
                )?;
                (source, Ty::Option(Box::new(item)))
            }
            // `v.append(&mut w)`: `w`'s elements moved to the end of `v`.
            ("append", [other]) => {
                let expected = Ty::Reference {
                    mutable: true,
                    target: Box::new(Ty::sequence(kind, element)),
                };
                self.coerce_expr(&expected, other)?;
                let places = self.mutable_arguments(std::slice::from_ref(other), [0])?;
                let mut source =
                    self.mutation_call(&value, "append", vec![other.rendered_source()])?;
                let direct = self.prelude_op(PreludeOp::NativePlace, [other.rendered_source()]);
                source = source.surrounded("{ ", "; if !(");
                source.push(direct);
                source.push_text(") { ");
                source.push(self.store_into(&places[0], "[]")?);
                source.push_text(" } }");
                (source, Ty::Unit)
            }
            // `v.push(x)` on a local, an element (`v[i].push(x)`,
            // `map.entry(k).or_default().push(x)`) or a field.
            ("push", [argument]) => {
                self.coerce_expr(&element, argument)?;
                // The element moves in, out of its local.
                if self.needs_drop(&element, argument.span)? {
                    self.move_local(argument)?;
                }
                let operand = argument.rendered_source();
                let source = self.mutation_call(&value, "push", vec![operand])?;
                (source.surrounded("{ ", "; }"), Ty::Unit)
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: self.inference.resolve(&value.ty).name(&self.models),
                    member,
                    reference: member_span,
                })
            }
        };
        let mut parsed = self.operator_expr(source, ty, expression);
        if whole.is_some() {
            parsed.lvalue = whole;
        }
        Ok(parsed)
    }

    /// `v.sort()`, `sort_by(f)` and `sort_by_key(f)` (and their
    /// `sort_unstable` spellings) on a mutable local `Vec`: a stable sort
    /// by `Ord::cmp`, `f`'s `Ordering`, or `Ord::cmp` of `f`'s keys,
    /// computed once per element. `None` for other methods.
    pub(super) fn parse_sort_method(
        &mut self,
        value: &ParsedExpr,
        element: &Ty,
        member: &str,
        arguments: &mut [ParsedExpr],
    ) -> Result<Option<origin::Fragment>, Error> {
        let stable = !member.starts_with("sort_unstable");
        let member = match member {
            "sort_unstable" => "sort",
            "sort_unstable_by" => "sort_by",
            "sort_unstable_by_key" => "sort_by_key",
            member => member,
        };
        let by = match (member, &mut *arguments) {
            ("sort", []) => None,
            ("sort_by" | "sort_by_key", [f]) => Some(f),
            _ => return Ok(None),
        };
        // A local is sorted in place; a field, an element or a nested place
        // through a copy stored back.
        let (binding, store) = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) => {
                let binding = binding.clone();
                self.require_mutable_binding(&binding, value.span)?;
                (binding.to_string(), None)
            }
            Some(_) => {
                let copy = self.temporary("sorted");
                let store = self.store_into(value, &copy)?;
                (copy, Some(store))
            }
            None => return Err(self.unsupported("sorting a value that is not a place")),
        };
        let reference = Ty::Reference {
            mutable: false,
            target: Box::new(element.clone()),
        };
        let missing = |this: &Self, ty: &Ty| Error::MissingCapability {
            owner: ty.name(&this.models),
            capability: "Ord".into(),
            operation: value.span,
            derive: matches!(ty, Ty::Script(..)),
            required_by: None,
        };
        let (a, b) = (self.temporary("left"), self.temporary("right"));
        let pair = (
            origin::Fragment::plain(a.clone()),
            origin::Fragment::plain(b.clone()),
        );
        let mut source = origin::Fragment::plain("{ ");
        let comparator;
        match (member, by) {
            ("sort_by", Some(f)) => {
                let expected = Ty::Fn {
                    inputs: Box::new([reference.clone(), reference]),
                    output: Box::new(Ty::Ordering),
                };
                self.coerce_expr(&expected, f)?;
                comparator = f.rendered_source();
                source.push_text(format!("{binding}.sort("));
                source.push(f.rendered_source());
                source.push_text(");");
            }
            ("sort_by_key", Some(f)) => {
                let key = self.inference.fresh();
                let expected = Ty::Fn {
                    inputs: Box::new([reference]),
                    output: Box::new(key.clone()),
                };
                self.coerce_expr(&expected, f)?;
                let (function, keyed, item) = (
                    self.temporary("key"),
                    self.temporary("keyed"),
                    self.temporary("item"),
                );
                let left_key = self.temporary("left_key");
                let right_key = self.temporary("right_key");
                let native_compare = self
                    .ord_compare(&key, left_key.as_str().into(), right_key.as_str().into())
                    .ok_or_else(|| missing(self, &key))?;
                let mut native = origin::Fragment::plain(format!("{{ let {function} = "));
                native.push(f.rendered_source());
                native.push_text(format!("; |{a}, {b}| {{ let {left_key} = {function}.__rawr_call({a}); let {right_key} = {function}.__rawr_call({b}); "));
                native.push(native_compare);
                native.push_text(" } }");
                comparator = native;
                let compare = self
                    .ord_compare(
                        &key,
                        origin::Fragment::plain(format!("{a}[0]")),
                        origin::Fragment::plain(format!("{b}[0]")),
                    )
                    .ok_or_else(|| missing(self, &key))?;
                source.push_text(format!("let {function} = "));
                source.push(f.rendered_source());
                source.push_text(format!(
                    "; let {keyed} = []; for {item} in {binding} {{ \
                     {keyed}.push([{function}.__rawr_call({item}), {item}]); }} {keyed}.sort(|{a}, {b}| "
                ));
                source.push(compare);
                source.push_text(format!("); {binding} = {keyed}.map(|{item}| {item}[1]);"));
            }
            _ => {
                let compare = self
                    .ord_compare(element, pair.0, pair.1)
                    .ok_or_else(|| missing(self, element))?;
                comparator = compare.clone().surrounded(&format!("|{a}, {b}| "), "");
                source.push_text(format!("{binding}.sort(|{a}, {b}| "));
                source.push(compare);
                source.push_text(");");
            }
        }
        if let Some(store) = store {
            let mut copied = origin::Fragment::plain(format!("{{ let {binding} = "));
            copied.push(value.rendered_source());
            copied.push_text("; ");
            copied.push(source);
            copied.push_text(" } ");
            copied.push(store);
            source = copied;
        }
        source.push_text(" }");
        let helper = self.push_op(Op::Prelude(PreludeOp::VecMethod));
        let native = self.mutation_call(
            value,
            &helper,
            vec![
                vec_method_code(match (member == "sort", stable) {
                    (true, true) => "sort",
                    (true, false) => "sort_unstable",
                    (false, true) => "sort_by",
                    (false, false) => "sort_unstable_by",
                })
                .to_string()
                .into(),
                comparator,
                "()".into(),
                "()".into(),
            ],
        )?;
        let mut routed = origin::Fragment::plain("if ");
        routed.push(self.prelude_op(PreludeOp::NativePlace, [value.rendered_source()]));
        routed.push_text(" { ");
        routed.push(native);
        routed.push_text("; } else ");
        routed.push(source);
        Ok(Some(routed))
    }

    /// `binary_search(&x)`, `binary_search_by(f)`,
    /// `binary_search_by_key(&b, f)` and `partition_point(p)` of the slice
    /// `values`: std's `binary_search_by` loop, which probes the same
    /// elements in the same order, so an `Ok` among equal elements and an
    /// `Err`'s insertion point are std's. `partition_point` is its
    /// `unwrap_or_else(|i| i)` of `p`'s `Less` or `Greater`.
    fn binary_search(
        &mut self,
        values: origin::Fragment,
        element: &Ty,
        member: &str,
        arguments: &mut [ParsedExpr],
        member_span: Span,
    ) -> Result<origin::Fragment, Error> {
        let reference = |target: &Ty| Ty::Reference {
            mutable: false,
            target: Box::new(target.clone()),
        };
        let names = [
            self.temporary("values"),
            self.temporary("by"),
            self.temporary("sought"),
            self.temporary("size"),
            self.temporary("base"),
            self.temporary("order"),
        ];
        let [all, function, sought, size, base, order] = &names;
        let mut source = origin::Fragment::plain(format!("{{ let {all} = "));
        source.push(values);
        source.push_text("; ");
        // What the search compares, by the probed element's `key` and the
        // sought value: `None` where the closure's own value is the order.
        let key = match (member, arguments) {
            ("binary_search", [x]) => {
                self.coerce_expr(&reference(element), x)?;
                source.push_text(format!("let {sought} = "));
                source.push(x.rendered_source());
                source.push_text("; ");
                Some(element.clone())
            }
            ("binary_search_by_key", [b, f]) => {
                let key = self.inference.fresh();
                let expected = Ty::Fn {
                    inputs: Box::new([reference(element)]),
                    output: Box::new(key.clone()),
                };
                self.coerce_expr(&expected, f)?;
                self.coerce_expr(&reference(&key), b)?;
                source.push_text(format!("let {sought} = "));
                source.push(b.rendered_source());
                source.push_text(format!("; let {function} = "));
                source.push(f.rendered_source());
                source.push_text("; ");
                Some(key)
            }
            (_, [f]) => {
                let output = if member == "partition_point" {
                    Ty::Bool
                } else {
                    Ty::Ordering
                };
                let expected = Ty::Fn {
                    inputs: Box::new([reference(element)]),
                    output: Box::new(output),
                };
                self.coerce_expr(&expected, f)?;
                source.push_text(format!("let {function} = "));
                source.push(f.rendered_source());
                source.push_text("; ");
                None
            }
            _ => return Err(self.unsupported("a slice search's arguments")),
        };
        // The order of the element at `at` against what is sought, as
        // std's closure for `member` gives it.
        let probe = |this: &mut Self, at: &str| -> Result<origin::Fragment, Error> {
            let item = format!("{all}[{at}]");
            let Some(key) = &key else {
                let call = format!("{function}.__rawr_call({item})");
                return Ok(origin::Fragment::plain(if member == "partition_point" {
                    format!("if {call} {{ -1 }} else {{ 1 }}")
                } else {
                    call
                }));
            };
            let probed = if member == "binary_search" {
                item
            } else {
                format!("{function}.__rawr_call({item})")
            };
            this.ord_compare(
                key,
                origin::Fragment::plain(probed),
                origin::Fragment::plain(sought.clone()),
            )
            .ok_or_else(|| Error::MissingCapability {
                owner: key.name(&this.models),
                capability: "Ord".into(),
                operation: member_span,
                derive: matches!(key, Ty::Script(..)),
                required_by: None,
            })
        };
        let (half, mid) = (self.temporary("half"), self.temporary("mid"));
        let inside = probe(self, &mid)?;
        let last = probe(self, base)?;
        // std narrows to one element without stopping at an equal one,
        // keeping `base` only where the probe compares `Greater`.
        source.push_text(format!(
            "let {size} = {all}.len(); let {base} = 0; \
             while {size} > 1 {{ let {half} = {size} / 2; let {mid} = {base} + {half}; if "
        ));
        source.push(inside);
        source.push_text(format!(
            " <= 0 {{ {base} = {mid}; }} {size} -= {half}; }} let {order} = if {size} == 0 {{ 1 }} else {{ "
        ));
        source.push(last);
        source.push_text(format!(
            " }}; let {base} = if {order} < 0 {{ {base} + 1 }} else {{ {base} }}; "
        ));
        if member == "partition_point" {
            source.push_text(format!("{base} }}"));
        } else {
            let ok = self.prelude_op(PreludeOp::Ok, [origin::Fragment::plain(base.clone())]);
            let err = self.prelude_op(PreludeOp::Err, [origin::Fragment::plain(base.clone())]);
            source.push_text(format!("if {order} == 0 {{ "));
            source.push(ok);
            source.push_text(" } else { ");
            source.push(err);
            source.push_text(" } }");
        }
        Ok(source.surrounded("(", ")"))
    }

    /// The element `iter().min()` and its kin pick from `values`, an
    /// `Option` of a reference to it: the first least, or the last
    /// greatest, by `Ord::cmp`, by a key's, or by a comparison function.
    pub(super) fn extremum(
        &mut self,
        values: origin::Fragment,
        element: &Ty,
        member: &str,
        by: Option<(&mut ParsedExpr, Ty)>,
        member_span: Span,
    ) -> Result<(origin::Fragment, Ty), Error> {
        let item = Ty::Reference {
            mutable: false,
            target: Box::new(element.clone()),
        };
        let names = [
            self.temporary("values"),
            self.temporary("by"),
            self.temporary("best"),
            self.temporary("best_key"),
            self.temporary("first"),
            self.temporary("item"),
            self.temporary("key"),
        ];
        let [all, function, best, best_key, first, current, key] = &names;
        let mut source = origin::Fragment::plain(format!("{{ let {all} = "));
        source.push(values);
        source.push_text(format!(
            "; let {best} = (); let {best_key} = (); let {first} = true; "
        ));
        let keyed = member.ends_with("_key");
        // `by`'s closure takes a reference to the iterator's item, `input`:
        // `&&T` over `v.iter()`.
        let compare = if let Some((by, input)) = by {
            let output = if keyed {
                self.inference.fresh()
            } else {
                Ty::Ordering
            };
            let inputs: Box<[Ty]> = if keyed {
                Box::new([input])
            } else {
                Box::new([input.clone(), input])
            };
            let expected = Ty::Fn {
                inputs,
                output: Box::new(output.clone()),
            };
            self.coerce_expr(&expected, by)?;
            source.push_text(format!("let {function} = "));
            source.push(by.rendered_source());
            source.push_text("; ");
            if keyed {
                self.ord_compare(
                    &output,
                    origin::Fragment::plain(best_key.clone()),
                    origin::Fragment::plain(key.clone()),
                )
                .ok_or_else(|| Error::MissingCapability {
                    owner: output.name(&self.models),
                    capability: "Ord".into(),
                    operation: member_span,
                    derive: matches!(output, Ty::Script(..)),
                    required_by: None,
                })?
            } else {
                origin::Fragment::plain(format!("{function}.__rawr_call({best}, {current})"))
            }
        } else {
            self.ord_compare(
                element,
                origin::Fragment::plain(best_key.clone()),
                origin::Fragment::plain(key.clone()),
            )
            .ok_or_else(|| Error::MissingCapability {
                owner: element.name(&self.models),
                capability: "Ord".into(),
                operation: member_span,
                derive: matches!(element, Ty::Script(..)),
                required_by: None,
            })?
        };
        let key_value = if keyed {
            format!("{function}.__rawr_call({current})")
        } else {
            current.clone()
        };
        // Rust keeps the best unless it compares greater (`min`) or keeps
        // it only while it compares greater (`max`).
        let operator = if member.starts_with("min") { ">" } else { "<=" };
        source.push_text(format!(
            "for {current} in {all} {{ let {key} = {key_value}; if {first} || "
        ));
        source.push(compare);
        source.push_text(format!(
            " {operator} 0 {{ {best} = {current}; {best_key} = {key}; }} {first} = false; }} if {first} {{ "
        ));
        let none = self.prelude_op(PreludeOp::None, []);
        source.push(none);
        source.push_text(" } else { ");
        let some = self.prelude_op(PreludeOp::Some, [origin::Fragment::plain(best.clone())]);
        source.push(some);
        source.push_text(" } }");
        Ok((source.surrounded("(", ")"), item))
    }
}

impl<'a> Parser<'a> {
    /// Whether, from inside a literal's opening bracket, a method call,
    /// field, index or `?` follows its closing one.
    pub(super) fn postfix_after_literal(&mut self) -> bool {
        let mut ahead = self.input.clone();
        let mut depth = 0_usize;
        while let Some((token, _)) = ahead.next() {
            match token {
                Token::LeftBracket | Token::LeftParen | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightBracket | Token::RightParen | Token::RightBrace if depth > 0 => {
                    depth -= 1;
                }
                Token::RightBracket | Token::RightParen => {
                    return matches!(ahead.next(), Some((Token::Period | Token::LeftBracket, _)));
                }
                Token::EOF => return false,
                _ => {}
            }
        }
        false
    }

    /// `value?` in a body returning `Option`: the `Some` value, or an early
    /// `return None`. In a body returning `Result<U, E>`, `value` is a
    /// `Result<T, E>`: the `Ok` value, or an early `return Err(error)`, the
    /// error converted by `impl From<E>` when `E` is not the body's error.
    pub(super) fn parse_try(
        &mut self,
        value: ParsedExpr,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        let expression = span(value.span.start(), end);
        let (element, early, is, unwrap) = match self.inference.resolve(&value.ty) {
            Ty::Option(element) => (
                *element,
                Ty::Option(Box::new(self.inference.fresh())),
                PreludeOp::IsSome,
                PreludeOp::Unwrap,
            ),
            Ty::Result(ok, err) => (
                *ok,
                Ty::Result(Box::new(self.inference.fresh()), err),
                PreludeOp::IsOk,
                PreludeOp::UnwrapOk,
            ),
            actual => {
                return Err(Error::TypeMismatch {
                    expected: "Option<_> or Result<_, _>".into(),
                    actual: actual.name(&self.models),
                    expression: value.span,
                });
            }
        };
        let Some(output) = self.return_ty.clone() else {
            return Err(self.unsupported("`?` outside a function or closure"));
        };
        // `?` in a body returning `Result<_, Box<dyn Error>>` boxes an
        // error, a `&str` or a `String` as `dyn Error`, as its `From` impls do.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && !matches!(*error, Ty::Infer(_))
            && self.boxed_std_error(&returned)
            && !self.boxed_std_error(&error)
        {
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(" } else { return ");
            let unwrapped_error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let Some(converted) = self.error_object(&error, unwrapped_error, true, value.span)?
            else {
                return Err(Error::MissingCapability {
                    owner: error.name(&self.models),
                    capability: "std::error::Error".into(),
                    operation: value.span,
                    derive: matches!(error.as_ref(), Ty::Script(..)),
                    required_by: None,
                });
            };
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        // `s.parse()?` there, whose error is known once the parse's target
        // is: boxed by the target's kind.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && matches!(*error, Ty::Infer(_))
            && self.boxed_std_error(&returned)
            && let Some(target) = self
                .pending_parses
                .iter()
                .find(|(_, pending, ..)| self.inference.resolve(pending) == *error)
                .map(|(target, ..)| target.clone())
        {
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(" } else { return ");
            let unwrapped_error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let helper = self.push_op(Op::Kinded {
                op: PreludeOp::ErrorParse,
                kinds: [target.clone(), target],
            });
            let converted = origin::Fragment::call(&helper, [unwrapped_error]);
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        // `?` converts the error through `From`, as in Rust: `Err(e)?` in a
        // body returning `Result<_, F>` returns `Err(F::from(e))`.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && !matches!(*error, Ty::Infer(_))
            && let Some(target) = owner_id(&returned)
            && !self.unifies(&returned, &error)
            && let Some((conversion, ..)) =
                self.select_conversion(target, Some(&returned), &error, value.span)?
        {
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let mut value = value;
            self.move_out_value(&mut value)?;
            let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(format!(" }} else {{ {drops}return "));
            let error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let converted = origin::Fragment::call(method_name(conversion.id()), [error]);
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        // `?` into a host error converts through its `From`, as a crate's
        // `#[from] std::io::Error` writes it: `Err(e)?` returns
        // `Err(F::from(e))`.
        if let (Ty::Result(_, error), Ty::Result(_, returned)) = (
            self.inference.resolve(&early),
            self.inference.resolve(&output),
        ) && !matches!(*error, Ty::Infer(_))
            && matches!(*returned, Ty::Host { .. })
            && !self.unifies(&returned, &error)
        {
            // The error's own `into`s too: a crate's `impl From<Error> for
            // io::Error` is a member of its error, not of std's (#1451).
            let (forms, owner) = self.conversion_forms(&error).unwrap_or_default();
            let host = HostInto {
                forms,
                owner,
                receiver: value.span,
            };
            let Some(helper) = self.host_into_helper(&host, &error, &returned, value.span)? else {
                // An imported error whose `from` the program never named,
                // or an imported error's `into`: its member, which the
                // importer brings once it is demanded.
                #[cfg(feature = "rust-import")]
                if let Some((owner, member)) = self
                    .imported_public_path(&returned, "from")
                    .map(|owner| (owner, "from"))
                    .or_else(|| {
                        self.imported_public_path(&error, "into")
                            .map(|owner| (owner, "into"))
                    })
                {
                    return Err(Error::UnknownMember {
                        owner,
                        member: member.into(),
                        reference: value.span,
                    });
                }
                return Err(self.try_operator(&output, &early, expression));
            };
            let ok = self.inference.fresh();
            self.unify_type(&output, &Ty::Result(Box::new(ok), returned));
            let mut value = value;
            self.move_out_value(&mut value)?;
            let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
            let operand = self.temporary("try");
            let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
            source.push(value.rendered_source());
            source.push_text("; if ");
            let test = self.prelude_op(is, [operand.as_str().into()]);
            source.push(test);
            source.push_text(" { ");
            let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
            source.push(unwrapped);
            source.push_text(format!(" }} else {{ {drops}return "));
            let error = self.prelude_op(PreludeOp::UnwrapErr, [operand.as_str().into()]);
            let converted = origin::Fragment::call(&helper, [error]);
            let returned = self.prelude_op(PreludeOp::Err, [converted]);
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
            source.push_text("; } }");
            return Ok(self.operator_expr(source.surrounded("(", ")"), element, expression));
        }
        if !self.unify_type(&output, &early) {
            return Err(self.try_operator(&output, &early, expression));
        }
        // The early return is the operand itself: `None`, or `Err(error)`.
        let mut value = value;
        self.move_out_value(&mut value)?;
        let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
        let operand = self.temporary("try");
        let mut source = origin::Fragment::plain(format!("{{ let {operand} = "));
        source.push(value.rendered_source());
        source.push_text("; if ");
        let test = self.prelude_op(is, [operand.as_str().into()]);
        source.push(test);
        source.push_text(" { ");
        let unwrapped = self.prelude_op(unwrap, [operand.as_str().into()]);
        source.push(unwrapped);
        let returned = origin::Fragment::plain(operand.as_str());
        source.push_text(format!(" }} else {{ {drops}return "));
        source.push(self.copied_return(returned.clone()).unwrap_or(returned));
        source.push_text("; } }");
        let source = source.surrounded("(", ")");
        // `v.get_mut(i)?` is the element's place, as `unwrap()` keeps it.
        let place = match &value.lvalue {
            Some(lvalue)
                if !matches!(lvalue, Lvalue::Slice(_))
                    && matches!(
                        self.inference.resolve(&element),
                        Ty::Reference { mutable: true, .. }
                    ) =>
            {
                let mut nested = places::as_nested(lvalue);
                let mut setup = source.clone();
                setup.push_text(";");
                nested.steps.insert(0, PlaceStep::Setup(setup));
                Some(Lvalue::Nested(Box::new(nested)))
            }
            _ => None,
        };
        let mut value = self.operator_expr(source, element, expression);
        value.lvalue = place;
        Ok(value)
    }

    /// rustc's E0277 for `expression`, a `?` whose operand returns `early`
    /// where the function's output is `output`, which can't hold it.
    pub(super) fn try_operator(&self, output: &Ty, early: &Ty, expression: Span) -> Error {
        let output = self.inference.resolve(output);
        let early = self.inference.resolve(early);
        let name = output.name(&self.models);
        let (message, label) = match (&output, &early) {
            (Ty::Result(_, returned), Ty::Result(_, error)) => (
                format!(
                    "`?` couldn't convert the error to `{}`",
                    returned.name(&self.models)
                ),
                format!(
                    "the trait `From<{}>` is not implemented for `{}`",
                    error.name(&self.models),
                    returned.name(&self.models)
                ),
            ),
            (Ty::Result(..), _) => (
                "the `?` operator can only be used on `Result`s, not `Option`s, in a function \
                 that returns `Result`"
                    .to_string(),
                format!("use `.ok_or(...)?` to provide an error compatible with `{name}`"),
            ),
            (Ty::Option(_), _) => (
                "the `?` operator can only be used on `Option`s, not `Result`s, in a function \
                 that returns `Option`"
                    .to_string(),
                format!(
                    "use `.ok()?` if you want to discard the `{}` error information",
                    early.name(&self.models)
                ),
            ),
            _ => (
                "the `?` operator can only be used in a function that returns `Result` or \
                 `Option` (or another type that implements `FromResidual`)"
                    .to_string(),
                format!("cannot use the `?` operator in a function that returns `{name}`"),
            ),
        };
        // rustc points at the `?` itself.
        let end = expression.end();
        let at = match (end.line(), end.position()) {
            (Some(line), Some(column)) if column > 1 => Span::new(
                Position::new(
                    u16::try_from(line).unwrap_or(u16::MAX),
                    u16::try_from(column - 1).unwrap_or(u16::MAX),
                ),
                end,
            ),
            _ => expression,
        };
        Error::Unimplemented {
            message: message.into(),
            label: label.into(),
            help: None,
            at,
        }
    }
}
