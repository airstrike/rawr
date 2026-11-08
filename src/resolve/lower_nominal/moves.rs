//! Rust's moves: a by-value use of a local or of a part of one, what a
//! `move` closure takes, and rustc's E0382 and E0507.

use super::*;

impl<'a> Parser<'a> {
    /// Rust's move out of a local. A by-value use of a local whose type is
    /// not `Copy` leaves the local moved, so a later use is rustc's E0382.
    /// The use reads `__rawr_move(local)`, which leaves `()` in the slot, so
    /// the value drops with its new owner. A closure's capture moves only in
    /// a `move` closure or async block, which owns it.
    pub(super) fn move_local(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        // A crate struct's field read in place: its by-value use moves the
        // field out of the struct, which the move check follows.
        if value.lvalue.is_none()
            && self.shared_field_reads.contains(&value.span)
            && self.moves_by_value(&value.ty)
        {
            self.flow().moved(value.span.start());
            return Ok(());
        }
        if let Some(Unplaced {
            moved: (code, message),
            ..
        }) = self.unmovable.get(&value.span)
            && self.moves_by_value(&value.ty)
        {
            return Err(Error::BorrowConflict {
                code,
                message: message.as_str().into(),
                access: value.span,
                borrow: value.span,
            });
        }
        if let Some(Lvalue::Field(field)) = &value.lvalue {
            let field = field.clone();
            return self.move_field(value, &field);
        }
        if let Some(Lvalue::Place { .. } | Lvalue::Nested(..)) = &value.lvalue
            && self.moves_by_value(&value.ty)
        {
            return self.move_part(value);
        }
        // An owned `self` moves out of the method's receiver, with drop
        // glue in the crate: its drop is its new owner's.
        if value.lvalue.is_none()
            && value.prelude.is_empty()
            && value.source.as_str() == "self"
            && self.drops()
            && self.closures.is_empty()
            && self.bindings.get("self").is_some_and(|binding| {
                matches!(binding.kind, BindingKind::Receiver { .. })
                    && !matches!(self.inference.resolve(&binding.ty), Ty::Reference { .. })
            })
            && self.needs_drop(&value.ty, value.span)?
        {
            value.source =
                origin::Fragment::call(crate::engine::KEYWORD_MOVE, [value.source.clone()]);
            return Ok(());
        }
        let Some(Lvalue::Binding { binding: name, .. }) = &value.lvalue else {
            return Ok(());
        };
        // A local read through a conversion (`__take(v)` of a `Vec` whose
        // element type the call settled) moves too, and reads as it did.
        let plain = value.prelude.is_empty() && value.source.as_str() == name.as_str();
        let name = name.clone();
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Ok(());
        };
        if !matches!(binding.kind, BindingKind::Local) {
            return Ok(());
        }
        let declaration = binding.declaration;
        if !self.moves_by_value(&value.ty) {
            self.note_unsettled_copy(value);
            // A parameter of a function type may be `impl Fn` or `F: Fn`,
            // which is not `Copy`: Rawr's one function type doesn't say.
            if matches!(self.inference.resolve(&value.ty), Ty::Fn { .. })
                && self.parameter_declarations.contains(&declaration)
            {
                self.flow().unmodeled(
                    "a by-value use of a parameter of a function type, which may not be `Copy`",
                    value.span,
                );
            }
            // A copy of a `Copy` closure has its own copies of what it
            // changes, as Rust copies the closure's captured values.
            if let Ty::FnItem { key, .. } = self.inference.resolve(&value.ty)
                && let Some(state) = self.copied_state.get(&*key)
            {
                let mut operands = vec![value.source.clone()];
                operands.extend(
                    state
                        .iter()
                        .map(|name| origin::Fragment::plain(format!("\"{name}\""))),
                );
                value.source =
                    origin::Fragment::call(crate::engine::KEYWORD_COPY_CLOSURE, operands);
            }
            return Ok(());
        }
        // A closure that moves out a capture is `FnOnce`. One that is not
        // `move` takes that capture by value, as Rust's closure capture
        // does.
        if let Some(scope) = self.closures.last_mut()
            && scope.outer.get(name.as_str()) == Some(&declaration)
        {
            scope.consumed.insert(name.clone());
        }
        // The use starting here is the local's own: the binding a scope
        // frame's call is read through (`scoped_call`) starts where the
        // call's receiver does, and moving it moves no receiver.
        let start = value.span.start();
        if self
            .flow()
            .mentioned_local(start)
            .is_some_and(|(mentioned, _)| mentioned == name)
        {
            self.flow().moved(start);
        }
        if let Some(binding) = self.bindings.get_mut(name.as_str()) {
            binding.available = false;
        }
        self.moved_at.insert(declaration, value.span);
        if plain {
            value.source =
                origin::Fragment::call(crate::engine::KEYWORD_MOVE, [value.source.clone()]);
        }
        Ok(())
    }

    /// A by-value use of a part of a local below a member or a field, or of
    /// an element: `t.1` and `s.inner.b` move that part out, and the local
    /// is partly moved, which the move check follows (T155). An element, or
    /// a part behind a reference, can't be moved out of (rustc's E0508 and
    /// E0507), nor a part of a value whose type implements `Drop` (E0509).
    pub(super) fn move_part(&mut self, value: &ParsedExpr) -> Result<(), Error> {
        let root = match &value.lvalue {
            Some(Lvalue::Place { binding, .. }) => binding.clone(),
            // A place a call gives (`v.last_mut()`, a `RefCell` borrow) is
            // a value, which moves nothing out of the local.
            Some(Lvalue::Nested(nested))
                if nested.steps.iter().all(|step| {
                    matches!(
                        step,
                        PlaceStep::Field { .. } | PlaceStep::Index(_) | PlaceStep::Member(_)
                    )
                }) =>
            {
                nested.binding.clone()
            }
            _ => return Ok(()),
        };
        if self.flow().is_call(value.span) {
            return Ok(());
        }
        let place = self.flow().place_at(value.span.start());
        let root_ty = self
            .bindings
            .get(root.as_str())
            .map(|binding| binding.ty.clone());
        let (Some(place), Some(root_ty)) = (place, root_ty) else {
            return Err(self.move_out_error(value, "of index of `{owner}`"));
        };
        if let Ty::Counted(..) = self.inference.resolve(&root_ty) {
            return Err(self.move_out_error(value, "of a value behind `{owner}`"));
        }
        let mut name = root.to_string();
        for (index, step) in place.projection.iter().enumerate() {
            match step {
                borrowck::Projection::Field(field) => {
                    let owner = self.part_ty(&root_ty, &place.projection[..index]);
                    if let Some(Ty::Script(owner, _)) = owner {
                        self.reject_drop_move(owner, true, value.span)?;
                    }
                    name = format!("{name}.{field}");
                }
                borrowck::Projection::Deref => {
                    let mutable = matches!(
                        self.part_ty(&root_ty, &place.projection[..index]),
                        Some(Ty::Reference { mutable: true, .. })
                    );
                    let behind = if mutable { "mutable" } else { "shared" };
                    let rest = place.projection[index + 1..]
                        .iter()
                        .map(|step| match step {
                            borrowck::Projection::Field(field) => format!(".{field}"),
                            _ => String::new(),
                        })
                        .collect::<String>();
                    return Err(Error::BorrowConflict {
                        code: "E0507",
                        message: format!(
                            "cannot move out of `{name}{rest}` which is behind a {behind} reference"
                        )
                        .into(),
                        access: value.span,
                        borrow: value.span,
                    });
                }
                _ => {
                    let ty = self
                        .part_ty(&root_ty, &place.projection[..index])
                        .map_or_else(|| value.ty.name(&self.models), |ty| ty.name(&self.models));
                    return Err(Error::BorrowConflict {
                        code: "E0508",
                        message: format!("cannot move out of type `{ty}`, a non-copy array").into(),
                        access: value.span,
                        borrow: value.span,
                    });
                }
            }
        }
        // `v[0]` of a `Vec`: its `Index` borrows all of it.
        if place.projection.is_empty() {
            return Err(Error::BorrowConflict {
                code: "E0507",
                message: format!(
                    "cannot move out of index of `{}`",
                    root_ty.name(&self.models)
                )
                .into(),
                access: value.span,
                borrow: value.span,
            });
        }
        if self.needs_drop(&value.ty, value.span)? {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a move out of a member or of a field below a field, whose type has \
                            drop glue: not supported in Rawr yet"
                    .into(),
                source: value.span,
            });
        }
        self.flow().moved(value.span.start());
        Ok(())
    }

    /// rustc's E0507 for a by-value use of `value`, a place Rust cannot
    /// move out of.
    pub(super) fn move_out_error(&self, value: &ParsedExpr, what: &str) -> Error {
        let owner = match &value.lvalue {
            Some(Lvalue::Place { binding, .. }) | Some(Lvalue::Binding { binding, .. }) => self
                .bindings
                .get(binding.as_str())
                .map(|binding| binding.ty.name(&self.models))
                .unwrap_or_else(|| value.ty.name(&self.models)),
            Some(Lvalue::Nested(nested)) => nested.binding.to_string().into(),
            _ => value.ty.name(&self.models),
        };
        Error::BorrowConflict {
            code: "E0507",
            message: format!("cannot move out {}", what.replace("{owner}", &owner)).into(),
            access: value.span,
            borrow: value.span,
        }
    }

    /// `local.field` by value, where the field's type is not `Copy`: the
    /// local is partially moved. A later use of the field, or of the
    /// whole local, is rustc's E0382. The other fields stay usable, and
    /// assigning the field makes it usable again.
    pub(super) fn move_field(
        &mut self,
        value: &mut ParsedExpr,
        field: &lvalue::Field,
    ) -> Result<(), Error> {
        let Some(binding) = self.bindings.get(field.binding.as_str()) else {
            return Ok(());
        };
        let declaration = binding.declaration;
        // `self.v` through `&self`, or `rc.v` through an `Rc`: rustc's
        // E0507.
        let behind = match self.inference.resolve(&binding.ty) {
            Ty::Reference { mutable: true, .. } => Some("behind a mutable reference"),
            Ty::Reference { .. } => Some("behind a shared reference"),
            Ty::Counted(Counter::Rc, _) => Some("behind an `Rc`"),
            Ty::Counted(Counter::Arc, _) => Some("behind an `Arc`"),
            _ => None,
        };
        if let Some(behind) = behind
            && value.prelude.is_empty()
            && self.moves_by_value(&value.ty)
        {
            return Err(self.move_out_error(
                value,
                &format!("of `{}.{}` which is {behind}", field.binding, field.member),
            ));
        }
        // An owned `self` is a local here, with drop glue in the crate.
        let owned_self = self.drops()
            && matches!(binding.kind, BindingKind::Receiver { .. })
            && self.closures.is_empty();
        let captured = self
            .closures
            .last()
            .is_some_and(|scope| scope.outer.get(field.binding.as_str()) == Some(&declaration));
        if captured && self.needs_drop(&value.ty, value.span)? {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a closure that moves a field whose type implements `Drop` out of a \
                            captured local: not supported in Rawr yet"
                    .into(),
                source: value.span,
            });
        }
        if !value.prelude.is_empty() || !self.moves_by_value(&value.ty) {
            self.note_unsettled_copy(value);
            return Ok(());
        }
        // The field moves out, and the move check follows what is left: in a
        // closure, its capture is by value (T152). A type with `impl Drop`
        // keeps its fields, as its `drop` must see them (E0509).
        self.reject_drop_move(field.definition.id(), true, value.span)?;
        self.flow().moved(value.span.start());
        if !(matches!(binding.kind, BindingKind::Local) || owned_self)
            || !matches!(self.inference.resolve(&binding.ty), Ty::Script(..))
            || captured
        {
            return Ok(());
        }
        // A field with drop glue moves out: `()` is left in the local, whose
        // own drop then passes over it.
        if self.needs_drop(&value.ty, value.span)? {
            let moved = self.temporary("moved");
            let mut source = origin::Fragment::plain(format!("{{ let {moved} = "));
            source.push(std::mem::take(&mut value.source));
            source.push_text(format!(
                "; {}({}, {}); {moved} }}",
                crate::engine::KEYWORD_CLEAR,
                field.binding,
                field.slot
            ));
            value.source = source;
        }
        Ok(())
    }

    /// Whether a `move` closure or async block written at `at` takes the
    /// local `name` out of its scope, which then has it moved.
    pub(super) fn move_capture(&mut self, name: &ImmutableString, at: Span) -> bool {
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return false;
        };
        let declaration = binding.declaration;
        if !matches!(binding.kind, BindingKind::Local)
            || self.aliases.contains_key(name.as_str())
            || self.entry_aliases.contains_key(name.as_str())
            || !binding.available
            || !self.moves_by_value(&binding.ty.clone())
        {
            return false;
        }
        // Inside a closure, a capture moved on is taken by that closure
        // too.
        if let Some(scope) = self.closures.last_mut()
            && scope.outer.get(name.as_str()) == Some(&declaration)
        {
            scope.consumed.insert(name.clone());
        }
        if let Some(binding) = self.bindings.get_mut(name.as_str()) {
            binding.available = false;
        }
        self.moved_at.insert(declaration, at);
        true
    }

    /// A `move` closure written at `at` takes the parts `parts` of the local
    /// `name` (each a path below it, and whether it is copied), as rustc's
    /// capture analysis has it from edition 2021 (T145). A field that is
    /// not `Copy` moves out, so the local is partly moved; the others are
    /// copied. A type with `impl Drop` can't be moved out of, so its local
    /// moves whole.
    pub(super) fn move_capture_parts(
        &mut self,
        name: &ImmutableString,
        parts: &[&(Span, Vec<borrowck::Projection>, bool)],
        at: Span,
    ) -> Result<bool, Error> {
        let Some(binding) = self.bindings.get(name.as_str()) else {
            return Ok(false);
        };
        if !matches!(binding.kind, BindingKind::Local)
            || self.aliases.contains_key(name.as_str())
            || self.entry_aliases.contains_key(name.as_str())
            || !binding.available
        {
            return Ok(false);
        }
        let ty = binding.ty.clone();
        let moved: Vec<&Vec<borrowck::Projection>> = parts
            .iter()
            .filter(|(_, _, copy)| !copy)
            .map(|(_, path, _)| path)
            .collect();
        if moved.is_empty() {
            return Ok(false);
        }
        if let Ty::Script(id, _) = self.inference.resolve(&ty)
            && self.models.has_drop(id)
        {
            return Ok(self.move_capture(name, at));
        }
        // The move check follows each part the closure moves (T156).
        for path in moved {
            // A part with drop glue would drop with the local and not with
            // the closure, which Rawr's closures, values copied freely,
            // cannot do.
            if let Some(part) = self.part_ty(&ty, path)
                && self.needs_drop(&part, at)?
            {
                let path = path
                    .iter()
                    .map(|step| match step {
                        borrowck::Projection::Field(field) => format!(".{field}"),
                        _ => String::new(),
                    })
                    .collect::<String>();
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "a closure that takes `{name}{path}`, whose type implements `Drop`, by \
                         move: dropping it with the closure is not supported in Rawr yet"
                    )
                    .into(),
                    source: at,
                });
            }
            // rustc captures a field of a type with `impl Drop` whole.
            if (1..path.len()).any(|length| {
                matches!(self.part_ty(&ty, &path[..length]),
                    Some(Ty::Script(id, _)) if self.models.has_drop(id))
            }) {
                self.flow().unmodeled(
                    "a `move` closure that moves a part of a field with `Drop`",
                    at,
                );
            }
        }
        Ok(false)
    }

    /// After branches, a local is available when each branch that goes on
    /// left it available. A branch that diverges (`return`, `break`, a
    /// panic) does not go on, as rustc's move check has it.
    pub(super) fn merge_branches(
        &mut self,
        branches: &[(&BTreeMap<ImmutableString, Binding>, &Ty)],
    ) {
        let going = branches
            .iter()
            .filter(|(_, ty)| !matches!(self.inference.resolve(ty), Ty::Never))
            .map(|(bindings, _)| *bindings)
            .collect::<Vec<_>>();
        if going.is_empty() {
            return;
        }
        for (name, binding) in &mut self.bindings {
            binding.available = going
                .iter()
                .all(|arm| arm.get(name).is_some_and(|binding| binding.available));
        }
    }

    /// Whether a value of `ty` moves when used by value: a known type that
    /// is not `Copy`. References and trait objects are used in place.
    pub(super) fn moves_by_value(&self, ty: &Ty) -> bool {
        let resolved = self.inference.resolve(ty);
        if let Ty::FnItem { key, .. } = &resolved {
            return self.owning_closures.contains(&**key);
        }
        // With `impl Drop` in the crate, a value of a generic type moves as
        // in Rust, unless its bounds make it `Copy`: its drop is its last
        // owner's.
        if matches!(resolved, Ty::Param(_) | Ty::Projection(..)) {
            return !self.type_provides(&resolved, capability::Kind::Copy);
        }
        !matches!(
            resolved,
            Ty::Infer(_)
                | Ty::Unknown
                | Ty::Never
                | Ty::Reference { .. }
                | Ty::Dyn(..)
                | Ty::Iter(..)
                | Ty::Callable { .. }
                | Ty::Param(_)
                | Ty::Projection(..)
        ) && !self.type_provides(&resolved, capability::Kind::Copy)
            && !self.copy_unsettled(&resolved)
    }

    /// Whether `ty` being `Copy` waits on inference: none of its parts
    /// makes it not `Copy`, and one still unsettled may (`(None, 1)` before
    /// the `Option`'s element is known).
    pub(super) fn copy_unsettled(&self, ty: &Ty) -> bool {
        let parts = match self.inference.resolve(ty) {
            Ty::Infer(_) | Ty::Unknown => return true,
            Ty::Tuple(members) => members.into_vec(),
            Ty::Array {
                element,
                kind: sequence::Kind::Array(_),
            }
            | Ty::Option(element)
            | Ty::Reverse(element) => vec![*element],
            Ty::Result(ok, err) => vec![*ok, *err],
            Ty::Script(id, arguments)
                if self
                    .models
                    .definition(id)
                    .capabilities()
                    .contains(capability::Kind::Copy) =>
            {
                arguments.into_vec()
            }
            _ => return false,
        };
        let unsettled = |part: &Ty| self.copy_unsettled(part);
        parts.iter().any(unsettled)
            && parts
                .iter()
                .all(|part| unsettled(part) || self.type_provides(part, capability::Kind::Copy))
    }

    /// A by-value use of `value`, whose type may not be `Copy` once it is
    /// settled: the move check leaves it to rustc.
    pub(super) fn note_unsettled_copy(&self, value: &ParsedExpr) {
        if self.copy_unsettled(&value.ty) {
            self.flow().unmodeled(
                "a by-value use of a value whose type is not settled there",
                value.span,
            );
        }
    }

    /// Whether `ty` holds no native value: a number, `bool`, `char`,
    /// `String` or the unit, alone or in an `Option`, a `Result`, a `Vec`,
    /// an array or a tuple, as the generated bridge's `plain_value` reads a
    /// field's type. Rawr holds such a value as its own.
    #[cfg(feature = "rust-import")]
    pub(super) fn plain_value(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Unit | Ty::Bool | Ty::Char | Ty::Int(_) | Ty::Float(_) | Ty::String => true,
            Ty::Option(inner) | Ty::Array { element: inner, .. } => self.plain_value(&inner),
            Ty::Result(ok, err) => self.plain_value(&ok) && self.plain_value(&err),
            Ty::Tuple(members) => members.iter().all(|member| self.plain_value(member)),
            // One of std's types Rawr's std holds as its own, as `PathBuf`.
            Ty::Host { proof, .. } => {
                proof
                    .definition
                    .path()
                    .strip_prefix(crate::std_facade::ROOT)
                    .is_some_and(|rest| rest.starts_with("::"))
                    && proof
                        .definition
                        .capabilities()
                        .contains(capability::Kind::Clone)
            }
            _ => false,
        }
    }

    /// rustc's E0382 for the local `name`, whose `binding` was moved, at
    /// its use at `reference`, which `borrowed` it or used it by value.
    pub(super) fn moved_value(
        &self,
        name: &str,
        binding: &Binding,
        reference: Span,
        borrowed: bool,
    ) -> Error {
        if let Some(assigned) = self.deferred.get(&binding.declaration)
            && !self.moved_at.contains_key(&binding.declaration)
        {
            return Error::Uninitialized {
                name: name.into(),
                possibly: assigned.is_some(),
                reference: self.name_span(reference, name),
                declaration: self.name_span(binding.declaration, name),
            };
        }
        let moved = self
            .moved_at
            .get(&binding.declaration)
            .copied()
            .unwrap_or(binding.declaration);
        Error::MovedValue {
            name: name.into(),
            moved: self.name_span(moved, name),
            reference: self.name_span(reference, name),
            borrowed,
            cause: Some(self.move_cause(name, binding)),
            previous_iteration: false,
        }
    }

    /// Why using the local `name` after its move is rustc's E0382: its
    /// type, which isn't `Copy`, labelled at its declaration.
    pub(super) fn move_cause(&self, name: &str, binding: &Binding) -> MoveCause {
        MoveCause {
            declaration: self.name_span(binding.declaration, name),
            ty: self.inference.resolve(&binding.ty).name(&self.models),
        }
    }
}
