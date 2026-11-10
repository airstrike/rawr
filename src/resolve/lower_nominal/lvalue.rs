//! Assignment places: what the left side of `=` names, the steps that
//! reach it, and how the assigned value is written back.

use super::op::{Op, PreludeOp};
use super::ty::Ty;
use super::{origin, BindingKind, ParsedExpr, Parser};
use crate::resolve::Error;
use crate::tokenizer::Token;
use crate::{type_def, ImmutableString, Span};

#[derive(Clone)]
pub(super) struct Field {
    pub(super) binding: ImmutableString,
    pub(super) definition: type_def::Definition,
    pub(super) slot: usize,
    /// The field's name, as written.
    pub(super) member: ImmutableString,
    pub(super) ty: Ty,
}

impl Field {
    /// The field itself, `binding[slot]`, which the runtime reads, assigns
    /// and calls methods on where it is.
    pub(super) fn place(&self) -> origin::Fragment {
        field_place(origin::Fragment::plain(self.binding.to_string()), self.slot)
    }
}

#[derive(Clone)]
pub(super) enum Lvalue {
    Binding {
        binding: ImmutableString,
        ty: Ty,
    },
    Field(Field),
    /// An element of a local array: `binding[index]`.
    Place {
        binding: ImmutableString,
        ty: Ty,
        place: origin::Fragment,
    },
    /// `place[low..high]` of a local's array (`base`): changed through a
    /// copy stored back over the range (`slice_copy`).
    Slice(Box<Slice>),
    /// A place through at least one field below a local or its element:
    /// `s.cells[i]`, `v[0].cells[j]`, `s.a.b`, changed through a copy of
    /// each field on the way, stored back (`nested_parts`).
    Nested(Box<Nested>),
}

#[derive(Clone)]
pub(super) struct Nested {
    /// The root binding, whose mutability the place needs.
    pub(super) binding: ImmutableString,
    /// The place the runtime assigns directly: the binding or an element
    /// of it (`v[0]`).
    pub(super) base: origin::Fragment,
    /// The steps after `base`, the first a field.
    pub(super) steps: Vec<PlaceStep>,
    pub(super) ty: Ty,
}

#[derive(Clone)]
pub(super) enum PlaceStep {
    /// A struct's field, by its slot.
    Field {
        slot: usize,
    },
    Index(origin::Fragment),
    /// A facade `&mut` accessor: `read` reads the part of the value bound
    /// to `place`, and `store` is the value with `stored` in its place.
    Accessor {
        place: String,
        stored: String,
        read: origin::Fragment,
        store: origin::Fragment,
    },
    /// A `RefCell` borrow's value: read from the borrow, stored back into
    /// the cell, and the borrow released after the statement when it was
    /// made in it. A `native` one is an imported method's borrow guard
    /// (`island::place::Place`), which `op=` updates through one call.
    Cell {
        release: bool,
        native: bool,
    },
    /// The place a script function's `&mut` output names below the place
    /// before it (`*c.slot(1) += 5`, `s.name_mut().push('!')`): `path`
    /// is the runtime path `places::path_invoke` reads and stores through.
    /// With `call`, the place is reached by calling the function on the
    /// place before it, `before` + place + `after`, which gives the path
    /// into the local `path` names; without it, `path` was given earlier
    /// (`let r = c.slot(0);`, or an `Option<&mut T>` output's `Some`).
    Path {
        path: origin::Fragment,
        call: Option<(origin::Fragment, origin::Fragment)>,
    },
    /// Statements the place's read runs first: the `Option` whose `Some`
    /// place `unwrap()` keeps, evaluated so its place is located.
    Setup(origin::Fragment),
    /// A tuple's member, `t.0`: its index, which two borrows of a place's
    /// members keep apart, as two of its fields are (E0499).
    Member(usize),
}

/// Whether `op=` on `nested` opens it ([`Parser::open_compound`]): it
/// passes through a `RefCell` borrow or a borrow guard.
pub(super) fn opens_compound(nested: &Nested) -> bool {
    // A place a call returns (`*c.slot(1) += 5`) calls it once too.
    nested.steps.iter().any(|step| {
        matches!(
            step,
            PlaceStep::Cell { .. } | PlaceStep::Path { .. } | PlaceStep::Setup(_)
        )
    })
}

/// A nested place `op=` opened: the statements before the leaf's change,
/// the leaf's place, and the statements after it.
pub(super) struct OpenedPlace {
    pub(super) opening: origin::Fragment,
    pub(super) place: origin::Fragment,
    pub(super) store: origin::Fragment,
}

impl OpenedPlace {
    /// The statement storing `replacement` in the leaf.
    pub(super) fn store(self, replacement: origin::Fragment) -> origin::Fragment {
        let mut rendered = origin::Fragment::plain("{ ");
        rendered.push(self.opening);
        rendered.push(self.place);
        rendered.push_text(" = ");
        rendered.push(replacement);
        rendered.push_text("; ");
        rendered.push(self.store);
        rendered.push_text("}");
        rendered
    }
}

#[derive(Clone)]
pub(super) struct Slice {
    pub(super) binding: ImmutableString,
    pub(super) ty: Ty,
    pub(super) base: origin::Fragment,
    pub(super) low: origin::Fragment,
    pub(super) high: origin::Fragment,
    pub(super) inclusive: bool,
}

impl Lvalue {
    pub(super) fn binding(&self) -> &ImmutableString {
        match self {
            Self::Binding { binding, .. } | Self::Place { binding, .. } => binding,
            Self::Slice(slice) => &slice.binding,
            Self::Field(field) => &field.binding,
            Self::Nested(nested) => &nested.binding,
        }
    }

    pub(super) fn ty(&self) -> &Ty {
        match self {
            Self::Slice(slice) => &slice.ty,
            Self::Nested(nested) => &nested.ty,
            Self::Binding { ty, .. } | Self::Place { ty, .. } => ty,
            Self::Field(field) => &field.ty,
        }
    }
}

pub(super) enum Writeback {
    Direct,
    Owned,
    /// A place the runtime changes in place: `v[0]`, a field `s[1]`, a
    /// nested place's leaf.
    Place(origin::Fragment),
}

impl<'a> Parser<'a> {
    /// The statement storing `value` into `lvalue`.
    pub(super) fn store_lvalue(
        &mut self,
        lvalue: &Lvalue,
        value: origin::Fragment,
    ) -> Result<origin::Fragment, Error> {
        Ok(match lvalue {
            Lvalue::Binding { binding, .. } => {
                let mut rendered = origin::Fragment::plain(format!("{binding} = "));
                rendered.push(value);
                rendered.push_text(";");
                rendered
            }
            Lvalue::Place { place, .. } => {
                let mut rendered = place.clone();
                rendered.push_text(" = ");
                rendered.push(value);
                rendered.push_text(";");
                rendered
            }
            Lvalue::Field(field) => {
                let mut rendered = field.place();
                rendered.push_text(" = ");
                rendered.push(value);
                rendered.push_text(";");
                rendered
            }
            Lvalue::Slice(_) => return Err(self.unsupported("changing a slice range in place")),
            Lvalue::Nested(nested) => self.store_nested(nested, value),
        })
    }

    /// `place = value` of a nested place: each field on the way copied
    /// into a local, the leaf set, and the fields stored back.
    pub(super) fn store_nested(
        &mut self,
        nested: &Nested,
        replacement: origin::Fragment,
    ) -> origin::Fragment {
        let (read, place, store) = self.nested_parts_with(nested, true, None);
        let mut rendered = origin::Fragment::plain("{ ");
        rendered.push(read);
        rendered.push(place);
        rendered.push_text(" = ");
        rendered.push(replacement);
        rendered.push_text("; ");
        rendered.push(store);
        rendered.push_text("}");
        rendered
    }

    /// The left operand of `place op= value` through a `RefCell` borrow
    /// or a borrow guard, and the statement's parts. The place's own read
    /// gives the value the operator changes, so `*cell.borrow_mut() += 1`
    /// borrows once. Through a borrow guard (D554.7(f)) the right operand
    /// is evaluated first, as rustc evaluates a primitive `op=`'s, and the
    /// guard's method is called once to read, change and store the value.
    pub(super) fn open_compound(
        &mut self,
        nested: &Nested,
        at: Span,
        replacement: &mut ParsedExpr,
    ) -> (ParsedExpr, OpenedPlace) {
        let native = nested
            .steps
            .iter()
            .any(|step| matches!(step, PlaceStep::Cell { native: true, .. }));
        let mut evaluated = origin::Fragment::default();
        let rhs = native.then(|| {
            let rhs = self.temporary("rhs");
            evaluated.push_text(format!("let {rhs} = "));
            evaluated.push(replacement.rendered_source());
            evaluated.push_text("; ");
            let mut operand = self.operator_expr(
                rhs.as_str().into(),
                replacement.ty.clone(),
                replacement.span,
            );
            operand.outcome = replacement.outcome;
            operand.evaluation = replacement.evaluation;
            *replacement = operand;
            rhs
        });
        let (read, place, store) = self.nested_parts_with(nested, false, rhs.as_deref());
        let target = self.operator_expr(place.clone(), nested.ty.clone(), at);
        let mut opening = evaluated;
        opening.push(read);
        (
            target,
            OpenedPlace {
                opening,
                place,
                store,
            },
        )
    }

    /// A nested place as one the runtime assigns and calls methods on: the
    /// statements copying each field on the way into a local, the leaf's
    /// place, and the statements storing the fields back, innermost first.
    pub(super) fn nested_parts(
        &mut self,
        nested: &Nested,
    ) -> (origin::Fragment, origin::Fragment, origin::Fragment) {
        self.nested_parts_with(nested, false, None)
    }

    /// [`Self::nested_parts`] of a place the statement `overwrite`s: a
    /// cell's or a borrow guard's value at the leaf is stored without being
    /// read first, so `*state.write() = v` calls the guard's method once.
    /// A place an `op=` `updates` with its evaluated right operand, so named,
    /// changes a borrow guard's value inside one call of its method
    /// ([`Self::cell_update_step`]).
    pub(super) fn nested_parts_with(
        &mut self,
        nested: &Nested,
        overwrite: bool,
        updates: Option<&str>,
    ) -> (origin::Fragment, origin::Fragment, origin::Fragment) {
        let mut place = nested.base.clone();
        let mut read = origin::Fragment::default();
        let mut stores = Vec::new();
        let leaf = nested.steps.len().saturating_sub(1);
        for (index, step) in nested.steps.iter().enumerate() {
            match step {
                PlaceStep::Index(index) => {
                    place.push_text("[");
                    place.push(index.clone());
                    place.push_text("]");
                }
                // The field itself, below the place before it (D568).
                PlaceStep::Field { slot, .. } => place = field_place(place, *slot),
                PlaceStep::Cell { release, native } => {
                    let reads = !(overwrite && index == leaf);
                    let (cell_read, cell_store, local) = match updates {
                        Some(rhs) if *native => self.cell_update_step(&place, rhs),
                        _ => self.cell_step(&place, *release, reads),
                    };
                    read.push(cell_read);
                    stores.push(cell_store);
                    place = origin::Fragment::plain(local);
                }
                PlaceStep::Path { path, call } => {
                    if let Some((before, after)) = call {
                        read.push_text(format!("let {} = ", path.as_str()));
                        read.push(before.clone());
                        read.push(place.clone());
                        read.push(after.clone());
                        read.push_text("; ");
                    }
                    let local = self.temporary("part");
                    if overwrite && index == leaf {
                        read.push_text(format!("let {local} = (); "));
                    } else {
                        let getter = self.push_op(Op::Prelude(PreludeOp::PathGet));
                        read.push_text(format!("let {local} = "));
                        read.push(place.clone());
                        read.push_text(format!(".{getter}("));
                        read.push(path.clone());
                        read.push_text("); ");
                    }
                    let setter = self.push_op(Op::Prelude(PreludeOp::PathSet));
                    let mut store = place.clone();
                    store.push_text(format!(".{setter}("));
                    store.push(path.clone());
                    store.push_text(format!(", {local}); "));
                    stores.push(store);
                    place = origin::Fragment::plain(local);
                }
                PlaceStep::Setup(setup) => {
                    read.push(setup.clone());
                    read.push_text(" ");
                }
                PlaceStep::Member(member) => place.push_text(format!("[{member}]")),
                PlaceStep::Accessor {
                    place: bound,
                    stored,
                    read: reader,
                    store: writer,
                } => {
                    let local = self.temporary("part");
                    read.push_text(format!("let {local} = {{ let {bound} = "));
                    read.push(place.clone());
                    read.push_text("; ");
                    read.push(reader.clone());
                    read.push_text(" }; ");
                    let mut store = place.clone();
                    store.push_text(format!(" = {{ let {bound} = "));
                    store.push(place.clone());
                    store.push_text(format!("; let {stored} = {local}; "));
                    store.push(writer.clone());
                    store.push_text(" }; ");
                    stores.push(store);
                    place = origin::Fragment::plain(local);
                }
            }
        }
        let mut store = origin::Fragment::default();
        for part in stores.into_iter().rev() {
            store.push(part);
        }
        (read, place, store)
    }

    /// A nested place changed by the statement being read (a method on it,
    /// `push`): its fields copied out before the statement and stored back
    /// after it (`field_entries`); the leaf's place.
    pub(super) fn nested_place(&mut self, nested: &Nested) -> origin::Fragment {
        let (read, place, store) = self.nested_parts(nested);
        self.field_entries.push((read, store));
        place
    }

    /// `place op= value`'s new value: through the left operand's
    /// `AddAssign` (and kin) on a crate type, else the binary operator's.
    pub(super) fn compound_assignment(
        &mut self,
        operator: Token,
        target: ParsedExpr,
        mut replacement: ParsedExpr,
    ) -> Result<ParsedExpr, Error> {
        let symbol = match operator {
            Token::Plus => "+=",
            Token::Minus => "-=",
            Token::Multiply => "*=",
            Token::Divide => "/=",
            Token::Modulo => "%=",
            Token::Ampersand => "&=",
            Token::Pipe => "|=",
            Token::XOr => "^=",
            Token::LeftShift => "<<=",
            Token::RightShift => ">>=",
            _ => "",
        };
        if let Some(value) = self.operator_impl(symbol, None, &target, Some(&mut replacement))? {
            return Ok(value);
        }
        self.apply_binary(operator, None, target, replacement)
    }

    /// `place = replacement;` for a mutable local, element or field.
    pub(super) fn assign_place(
        &mut self,
        lvalue: &Lvalue,
        replacement: origin::Fragment,
        mutation: Span,
    ) -> Result<origin::Fragment, Error> {
        let binding_name = lvalue.binding().clone();
        let Some(binding) = self.mutation_binding(&binding_name) else {
            return Err(self.unsupported("assignment through an unknown binding"));
        };
        if !binding.mutable {
            return Err(match &binding.kind {
                BindingKind::Receiver { owner } => Error::ImmutableReceiver {
                    owner: owner.clone(),
                    mutation,
                    receiver: binding.declaration,
                },
                BindingKind::Local | BindingKind::Invocation => {
                    self.immutable_assignment(lvalue, binding.declaration, mutation)
                }
            });
        }
        Ok(match lvalue {
            Lvalue::Binding { .. } => {
                let mut rendered = origin::Fragment::plain(format!("{binding_name} = "));
                rendered.push(replacement);
                rendered.push_text(";");
                rendered
            }
            Lvalue::Place { place, .. } => {
                let mut rendered = place.clone();
                rendered.push_text(" = ");
                rendered.push(replacement);
                rendered.push_text(";");
                rendered
            }
            Lvalue::Field(field) => {
                let mut rendered = field.place();
                rendered.push_text(" = ");
                rendered.push(replacement);
                rendered.push_text(";");
                rendered
            }
            Lvalue::Slice(_) => return Err(self.unsupported("assigning to a slice range")),
            Lvalue::Nested(nested) => self.store_nested(nested, replacement),
        })
    }
}

/// `lvalue` (a field, a local's element or a nested place) followed by
/// `step`: a nested place of type `ty`.
pub(super) fn nested_step(lvalue: &Lvalue, step: PlaceStep, ty: Ty) -> Lvalue {
    let mut nested = match lvalue {
        Lvalue::Nested(nested) => (**nested).clone(),
        Lvalue::Field(field) => Nested {
            binding: field.binding.clone(),
            base: origin::Fragment::plain(field.binding.to_string()),
            steps: vec![PlaceStep::Field { slot: field.slot }],
            ty: field.ty.clone(),
        },
        Lvalue::Place { binding, place, .. } => Nested {
            binding: binding.clone(),
            base: place.clone(),
            steps: Vec::new(),
            ty: ty.clone(),
        },
        Lvalue::Binding { .. } | Lvalue::Slice(_) => {
            unreachable!("only fields, elements and nested places extend")
        }
    };
    nested.steps.push(step);
    nested.ty = ty;
    Lvalue::Nested(Box::new(nested))
}

/// The field at `slot` of the struct `base` is, as the runtime reaches it in
/// place: `base[slot]`. A local or a chain of parts below one stays a chain,
/// so `s.a.b` is the place `s[0][1]`; any other value is indexed as a
/// temporary, `(base)[slot]`.
pub(super) fn field_place(base: origin::Fragment, slot: usize) -> origin::Fragment {
    let mut place = if is_place_chain(base.as_str()) {
        base
    } else {
        base.surrounded("(", ")")
    };
    place.push_text(format!("[{slot}]"));
    place
}

/// The place `value` reads, as the runtime reaches it where it is: a local
/// by its name, an element (`v[i]`), a field (`s[1]`), or a chain of fields,
/// members and elements below a local. `None` for any other value, or for
/// one read through a prelude.
pub(super) fn in_place(value: &ParsedExpr) -> Option<origin::Fragment> {
    if !value.prelude.is_empty() {
        return None;
    }
    match value.lvalue.as_ref()? {
        Lvalue::Binding { binding, .. } => {
            (value.source.as_str() == binding.as_str()).then(|| value.source.clone())
        }
        Lvalue::Place { place, .. } => Some(place.clone()),
        Lvalue::Field(field) => Some(field.place()),
        Lvalue::Nested(nested) => {
            let mut place = nested.base.clone();
            for step in &nested.steps {
                match step {
                    PlaceStep::Field { slot } => place = field_place(place, *slot),
                    PlaceStep::Index(index) => {
                        place.push_text("[");
                        place.push(index.clone());
                        place.push_text("]");
                    }
                    PlaceStep::Member(member) => place.push_text(format!("[{member}]")),
                    _ => return None,
                }
            }
            Some(place)
        }
        Lvalue::Slice(_) => None,
    }
}

/// The local and the place of `value` when it is a chain of fields and
/// members below a local (`s.a.0` is `s[0][0]`), each slot a literal.
pub(super) fn fields_below_local(
    value: &ParsedExpr,
) -> Option<(ImmutableString, origin::Fragment)> {
    match value.lvalue.as_ref()? {
        Lvalue::Field(field) => Some((field.binding.clone(), field.place())),
        Lvalue::Nested(nested)
            if nested
                .steps
                .iter()
                .all(|step| matches!(step, PlaceStep::Field { .. } | PlaceStep::Member(_))) =>
        {
            let base = nested.base.as_str();
            if !is_place_chain(base) {
                return None;
            }
            let root = base.trim_start_matches('(');
            let root = &root[..root
                .find(|character: char| character != '_' && !character.is_alphanumeric())
                .unwrap_or(root.len())];
            Some((root.into(), in_place(value)?))
        }
        _ => None,
    }
}

/// Whether `source` is a local's name followed by indexes only:
/// `s`, `this`, `s[0][i + 1]`, `(s)[0]`.
pub(super) fn is_place_chain(source: &str) -> bool {
    // `(s)` reads as `s`.
    let (name, rest) = match source.strip_prefix('(') {
        Some(inner) => match inner.split_once(')') {
            Some(parts) => parts,
            None => return false,
        },
        None => source.split_at(
            source
                .find(|character: char| character != '_' && !character.is_alphanumeric())
                .unwrap_or(source.len()),
        ),
    };
    if name.is_empty()
        || name.starts_with(|character: char| character.is_ascii_digit())
        || variable_name(name).is_none()
    {
        return false;
    }
    let mut depth = 0usize;
    for character in rest.chars() {
        match character {
            '[' => depth += 1,
            ']' => match depth.checked_sub(1) {
                Some(outer) => depth = outer,
                None => return false,
            },
            '"' | '\'' => return false,
            _ if depth == 0 => return false,
            _ => {}
        }
    }
    depth == 0
}

pub(super) fn variable_name(source: &str) -> Option<ImmutableString> {
    source
        .chars()
        .all(|character| character == '_' || character.is_alphanumeric())
        .then(|| source.into())
}

/// The place an assignment's text `text` assigns: `g.c[0]` of
/// `g.c[0] += 2`.
pub(super) fn assigned_place(text: &str) -> &str {
    let bytes = text.as_bytes();
    let assignment = (0..bytes.len()).find(|&index| {
        bytes[index] == b'='
            && bytes.get(index + 1) != Some(&b'=')
            && !matches!(
                index.checked_sub(1).map(|before| bytes[before]),
                Some(b'=' | b'!' | b'<' | b'>')
            )
    });
    let Some(assignment) = assignment else {
        return text.trim();
    };
    text[..assignment]
        .trim_end()
        .trim_end_matches(['+', '-', '*', '/', '%', '^', '&', '|', '<', '>'])
        .trim_end()
}
