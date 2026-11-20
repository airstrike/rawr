//! Mutability: rustc's errors for a change through an immutable binding,
//! and the receivers a `&mut self` or by-value method needs.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn require_mutable_host_receiver(
        &self,
        receiver: &ParsedExpr,
        operation: Span,
        item: &crate::host::item::Definition,
    ) -> Result<(), Error> {
        self.mutable_use(receiver, operation);
        // A `&mut T` reaches its value mutably without being `mut` itself,
        // as a pattern's binding through `&mut T` does (#786).
        if matches!(
            self.inference.resolve(&receiver.ty),
            Ty::Reference { mutable: true, .. }
        ) {
            return Ok(());
        }
        // An imported value is changed where it lives, through its handle,
        // so a place below a mutable binding (`self.panes.split(..)`) needs
        // only that binding to be mutable, as Rust's `&mut self` does.
        let root = match &receiver.lvalue {
            Some(Lvalue::Field(field)) => Some(field.binding.clone()),
            Some(Lvalue::Nested(nested)) => Some(nested.binding.clone()),
            Some(Lvalue::Place { binding, .. }) => Some(binding.clone()),
            _ => None,
        };
        if let Some(root) = root {
            return match self.mutation_binding(root.as_str()) {
                Some(binding) if binding.mutable => Ok(()),
                Some(binding) => Err(match &binding.kind {
                    BindingKind::Receiver { .. } => self.receiver_borrow(
                        &binding.ty,
                        self.written_place(receiver.span),
                        receiver.span,
                    ),
                    BindingKind::Local | BindingKind::Invocation => {
                        let place = self
                            .written(receiver.span)
                            .unwrap_or_else(|| root.to_string());
                        Error::borrow_immutable_place(&place, &root, receiver.span)
                    }
                }),
                None => Err(Error::InvalidHostOperation {
                    item: item.path().into(),
                    reason: format!("cannot borrow `{root}` as mutable (E0596)").into(),
                    operation,
                }),
            };
        }
        let Some(name) = variable_name(receiver.source.as_str()) else {
            // A temporary, as `rand::rng()` of `rand::rng().random_range(..)`
            // or a struct literal, is borrowed mutably where it is made and
            // lives to the end of its statement, as rustc's auto-borrow of
            // a value expression does.
            if !self.flow().is_place(receiver.span) {
                return Ok(());
            }
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: "calling a `&mut self` method on a value that isn't a `let mut` binding \
                         is not supported in Rawr yet"
                    .into(),
                operation,
            });
        };
        let reason = match self.bindings.get(name.as_str()) {
            Some(binding) if binding.mutable && binding.available => return Ok(()),
            Some(binding)
                if binding.available
                    && matches!(
                        self.inference.resolve(&binding.ty),
                        Ty::Reference { mutable: true, .. }
                    ) =>
            {
                return Ok(());
            }
            Some(binding) if !binding.available => {
                format!("borrow of moved value: `{name}` (E0382)")
            }
            _ => format!(
                "cannot borrow `{name}` as mutable, as it is not declared as mutable (E0596)"
            ),
        };
        Err(Error::InvalidHostOperation {
            item: item.path().into(),
            reason: reason.into(),
            operation,
        })
    }

    pub(super) fn consume_host_receiver(
        &mut self,
        receiver: &ParsedExpr,
        operation: Span,
        item: &crate::host::item::Definition,
    ) -> Result<(), Error> {
        if self.type_provides(&receiver.ty, capability::Kind::Copy) {
            return Ok(());
        }
        let Some(name) = variable_name(receiver.source.as_str()) else {
            return Ok(());
        };
        let Some(binding) = self.bindings.get_mut(name.as_str()) else {
            return Ok(());
        };
        if !binding.available {
            return Err(Error::InvalidHostOperation {
                item: item.path().into(),
                reason: format!("use of moved value: `{name}` (E0382)").into(),
                operation,
            });
        }
        binding.available = false;
        let declaration = binding.declaration;
        self.moved_at.insert(declaration, receiver.span);
        Ok(())
    }

    /// The rest of the source line from `at`, as the program writes it.
    pub(super) fn written_from(&self, at: Position) -> Option<String> {
        let key = self.models.sources.key(self.models.context.index());
        let source = self.models.sources.resolve(key).ok()?;
        let line = source.text().split('\n').nth(at.line()?.checked_sub(1)?)?;
        let from = at.position()?.checked_sub(1)?;
        Some(line.chars().skip(from).collect())
    }

    /// The source text at `at`, on one line, as the program writes it.
    pub(super) fn written(&self, at: Span) -> Option<String> {
        let key = self.models.sources.key(self.models.context.index());
        let source = self.models.sources.resolve(key).ok()?;
        let (start, end) = (at.start(), at.end());
        if start.line()? != end.line()? {
            return None;
        }
        let line = source
            .text()
            .split('\n')
            .nth(start.line()?.checked_sub(1)?)?;
        let chars = line.chars().collect::<Vec<_>>();
        let from = start.position()?.checked_sub(1)?;
        let to = end.position()?.checked_sub(1)?;
        (from < to && to <= chars.len()).then(|| chars[from..to].iter().collect())
    }

    /// rustc's E0594 for an assignment at `at`, the whole assignment, to a
    /// place below a `self` of type `receiver` that can't change it: one
    /// behind `&self`, or a `self` not declared `mut`.
    pub(super) fn receiver_assignment(&self, receiver: &Ty, at: Span) -> Error {
        // The assignment's span ends where its last token starts.
        let at = self.expression_span(at);
        let place = self.written(at).map_or_else(
            || "self".to_owned(),
            |text| assigned_place(&text).to_owned(),
        );
        if !matches!(
            self.inference.resolve(receiver),
            Ty::Reference { mutable: false, .. }
        ) {
            return Error::assign_immutable(&place, "self", at);
        }
        Error::StdPath {
            code: "E0594",
            message: format!("cannot assign to `{place}`, which is behind a `&` reference").into(),
            label: Some(
                "`self` is a `&` reference, so the data it refers to cannot be written".into(),
            ),
            note: None,
            reference: at,
        }
    }

    /// rustc's error for an assignment to `place`, which has no lvalue,
    /// after its `operator` at `at`. A value that is no place is E0070
    /// (E0067 for a compound assignment), found while the assignment's
    /// types are checked. A place with a pointer above it that can't change
    /// it (`*r` of a `&`, a field of an `Arc`'s value) is E0594: rustc's
    /// borrow check comes after the value's type check, so the value is
    /// parsed first, and the error marks the whole assignment. `None` for a
    /// place rawr gives no lvalue of its own yet.
    pub(super) fn unplaced_assignment(
        &mut self,
        place: Span,
        operator: &Token,
        at: Position,
    ) -> Result<Option<Error>, Error> {
        if self.is_value_expression(place) {
            return Ok(Some(Error::InvalidAssignee {
                compound: *operator != Token::Equals,
                operator: span(at, text_end(at, operator.literal_syntax())),
                assignee: self.expression_span(place),
            }));
        }
        let Some(Unplaced {
            assigned: Some(assigned),
            ..
        }) = self.unmovable.get(&place).cloned()
        else {
            return Ok(None);
        };
        let replacement = self.parse_expr()?;
        let at = span(place.start(), replacement.span.end());
        Ok(Some(match assigned {
            Unassignable::Counted(counter) => Error::BorrowConflict {
                code: "E0594",
                message: counter.unassignable(),
                access: at,
                borrow: Span::NONE,
            },
            Unassignable::Shared(holder) => self.shared_assignment(holder.as_deref(), at),
        }))
    }

    /// Whether the expression spanning `at` is a value, never a place: a
    /// literal, an operator's result or a call's, or one of them in
    /// parentheses. A call's value is known by where it ends, which `*f()`,
    /// the place the call's reference refers to, shares.
    pub(super) fn is_value_expression(&self, at: Span) -> bool {
        self.values.contains(&at)
            || (self.flow().is_call(at) && !at.text(self.text).starts_with('*'))
    }

    /// rustc's E0594 for an assignment at `at`, the whole assignment, to a
    /// place behind a `&` reference, which `holder`, a local or `self`,
    /// holds when the place is directly below it.
    pub(super) fn shared_assignment(&self, holder: Option<&str>, at: Span) -> Error {
        // The assignment's span ends where its last token starts.
        let at = self.expression_span(at);
        let place = self.written(at).map_or_else(
            || "*_".to_owned(),
            |text| described_place(assigned_place(&text)),
        );
        let label = holder.map_or_else(
            || "cannot assign".to_owned(),
            |holder| format!("`{holder}` is a `&` reference, so it cannot be written to"),
        );
        Error::StdPath {
            code: "E0594",
            message: format!("cannot assign to `{place}`, which is behind a `&` reference").into(),
            label: Some(label.into()),
            note: None,
            reference: at,
        }
    }

    /// rustc's error for an assignment at `at` to `place`, a place below
    /// `binding`, a `&` local: E0596 for the container of an `IndexMut`
    /// element, which the assignment borrows mutably, and E0594 otherwise.
    pub(super) fn shared_local_assignment(&self, binding: &str, place: Span, at: Span) -> Error {
        let written = self
            .written(at)
            .map(|text| assigned_place(&text).to_owned());
        let indexed = written
            .as_deref()
            .filter(|_| self.index_mut_elements.contains(&place))
            .and_then(|text| text.rfind('[').map(|open| text[..open].trim_end()));
        let Some(indexed) = indexed else {
            return self.shared_assignment(Some(binding), at);
        };
        let borrowed = described_place(indexed);
        let borrowed = if borrowed == binding {
            format!("*{binding}")
        } else {
            borrowed
        };
        Error::StdPath {
            code: "E0596",
            message: format!(
                "cannot borrow `{borrowed}` as mutable, as it is behind a `&` reference"
            )
            .into(),
            label: Some(
                format!("`{binding}` is a `&` reference, so it cannot be borrowed as mutable")
                    .into(),
            ),
            note: None,
            reference: Span::new(at.start(), text_end(at.start(), indexed)),
        }
    }

    /// The place below `self` written at `at`, as `self.items`.
    pub(super) fn written_place(&self, at: Span) -> Option<String> {
        self.written(at)
            .filter(|text| text.starts_with("self.") || text.starts_with("self["))
    }

    /// rustc's E0596 for a `&mut` borrow of `place`, a place below `self`
    /// written from where `at` starts, or `self` itself, through a `self`
    /// of type `receiver` that can't change it: one behind `&self`, or a
    /// `self` not declared `mut`.
    pub(super) fn receiver_borrow(&self, receiver: &Ty, place: Option<String>, at: Span) -> Error {
        let shared = matches!(
            self.inference.resolve(receiver),
            Ty::Reference { mutable: false, .. }
        );
        let at = place.as_deref().map_or(at, |place| {
            Span::new(at.start(), text_end(at.start(), place))
        });
        let place = place.unwrap_or_else(|| if shared { "*self" } else { "self" }.to_owned());
        if !shared {
            return Error::borrow_immutable_place(&place, "self", at);
        }
        let label = if place == "*self" {
            "`self` is a `&` reference, so it cannot be borrowed as mutable"
        } else {
            "`self` is a `&` reference, so the data it refers to cannot be borrowed as mutable"
        };
        Error::StdPath {
            code: "E0596",
            message: format!("cannot borrow `{place}` as mutable, as it is behind a `&` reference")
                .into(),
            label: Some(label.into()),
            note: None,
            reference: at,
        }
    }

    /// rustc's error for an assignment to `lvalue`, at `at`, whose root
    /// local `binding` (declared at `declaration`) isn't `mut`: E0384 for the
    /// local itself, E0596 for an element of a `Vec`, which `IndexMut`
    /// borrows, and E0594 for another part.
    pub(super) fn immutable_assignment(
        &self,
        lvalue: &Lvalue,
        declaration: Span,
        at: Span,
    ) -> Error {
        let binding = lvalue.binding();
        let place = self
            .written(at)
            .map(|text| assigned_place(&text).to_string())
            .unwrap_or_else(|| binding.to_string());
        let array = self.bindings.get(binding.as_str()).is_some_and(|local| {
            matches!(
                self.inference.resolve(&local.ty),
                Ty::Array {
                    kind: sequence::Kind::Array(_),
                    ..
                }
            )
        });
        match lvalue {
            Lvalue::Binding { .. } => Error::ImmutableBinding {
                binding: binding.clone(),
                mutation: at,
                declaration,
                declared: declaration,
                matched: self.matched_bindings.contains(&declaration.start()),
            },
            Lvalue::Field(field) => {
                Error::assign_immutable(&format!("{binding}.{}", field.member), binding, at)
            }
            // An element of anything but an array is `IndexMut`'s, which
            // borrows the indexed place mutably.
            _ if !array && place.ends_with(']') => {
                let indexed = place
                    .rfind('[')
                    .map_or(place.as_str(), |open| place[..open].trim_end());
                Error::borrow_immutable_place(
                    indexed,
                    binding,
                    Span::new(at.start(), text_end(at.start(), indexed)),
                )
            }
            Lvalue::Place { .. } | Lvalue::Slice(..) => {
                Error::assign_immutable(&format!("{binding}[_]"), binding, at)
            }
            Lvalue::Nested(..) => Error::assign_immutable(&place, binding, at),
        }
    }

    /// The binding a change through `name` changes. A closure's `self` is
    /// the method's own: the statement making the closure writes it back
    /// (`lend_self`).
    pub(super) fn mutation_binding(&self, name: &str) -> Option<&Binding> {
        if name == SELF_CAPTURE {
            self.self_changed.set(true);
            return self
                .bindings
                .get("self")
                .filter(|binding| matches!(binding.kind, BindingKind::Receiver { .. }));
        }
        self.bindings.get(name)
    }

    pub(super) fn require_mutable_receiver(
        &self,
        value: &ParsedExpr,
        definition: &method::Definition,
        operation: Span,
    ) -> Result<Writeback, Error> {
        self.mutable_use(value, operation);
        let (name, writeback) = if let Some(Lvalue::Field(field)) = &value.lvalue {
            (field.binding.clone(), Writeback::Place(field.place()))
        } else if let Some(Lvalue::Binding { binding, .. }) = &value.lvalue {
            (binding.clone(), Writeback::Direct)
        } else {
            let Some(name) = variable_name(value.source.as_str()) else {
                return Err(Error::InvalidMethodCall {
                    owner: definition.owner_name().into(),
                    method: definition.name().into(),
                    reason:
                        "calling a `&mut self` method on a value that isn't a `let mut` binding \
                             is not supported in Rawr yet"
                            .into(),
                    call: operation,
                });
            };
            (name, Writeback::Direct)
        };
        let Some(binding) = self.mutation_binding(name.as_str()) else {
            return Err(Error::InvalidMethodCall {
                owner: definition.owner_name().into(),
                method: definition.name().into(),
                reason: format!("cannot borrow `{name}` as mutable (E0596)").into(),
                call: operation,
            });
        };
        if binding.mutable {
            return Ok(writeback);
        }
        Err(match &binding.kind {
            BindingKind::Receiver { .. } => {
                let place = match &value.lvalue {
                    Some(Lvalue::Field(field)) => Some(format!("self.{}", field.member)),
                    _ => self.written_place(value.span),
                };
                self.receiver_borrow(&binding.ty, place, value.span)
            }
            // The method's `&mut self` borrows the local itself: rustc's
            // E0596, as for std's methods.
            BindingKind::Local | BindingKind::Invocation
                if matches!(writeback, Writeback::Direct) =>
            {
                Error::BorrowConflict {
                    code: "E0596",
                    message: format!(
                        "cannot borrow `{name}` as mutable, as it is not declared as mutable"
                    )
                    .into(),
                    access: value.span,
                    borrow: Span::NONE,
                }
            }
            BindingKind::Local | BindingKind::Invocation => {
                let place = match &value.lvalue {
                    Some(Lvalue::Field(field)) => format!("{}.{}", field.binding, field.member),
                    _ => self.written(value.span).unwrap_or_else(|| name.to_string()),
                };
                Error::borrow_immutable_place(&place, &name, value.span)
            }
        })
    }

    pub(super) fn require_invocation_receiver(
        &self,
        value: &ParsedExpr,
        operation: Span,
    ) -> Result<(), Error> {
        // A value a callback is lent, as the `&mut Frame` of a canvas's
        // `draw`, is used only through the parameter that receives it.
        let lent_only = |ty: &Ty| Error::InvalidHostOperation {
            item: ty.name(&self.models),
            reason: "it is lent only to the callback that receives it; calling its methods \
                     except through that `&mut` parameter is not supported in Rawr yet"
                .into(),
            operation,
        };
        let Ty::Reference {
            mutable: true,
            target,
        } = self.inference.resolve(&value.ty)
        else {
            return Err(lent_only(&value.ty));
        };
        if target.invocation().is_none() {
            return Err(lent_only(&target));
        }
        let lent = variable_name(value.source.as_str())
            .and_then(|name| self.bindings.get(name.as_str()))
            .is_some_and(|binding| {
                matches!(binding.kind, BindingKind::Invocation)
                    && binding.mutable
                    && binding.ty == value.ty
            });
        if lent {
            Ok(())
        } else {
            Err(lent_only(&target))
        }
    }

    pub(super) fn reject_invocation(
        &self,
        ty: &Ty,
        operation: Span,
        action: &str,
    ) -> Result<(), Error> {
        let ty = self.inference.resolve(ty);
        let Some(definition) = ty.invocation() else {
            return Ok(());
        };
        Err(Error::InvalidHostOperation {
            item: definition.path().into(),
            reason: format!(
                "a `{}` {action} is not supported in Rawr yet; it is lent only to the callback \
                 that receives it",
                definition.path()
            )
            .into(),
            operation,
        })
    }
}
