//! Assignment places: what the left side of `=` names, the steps that
//! reach it, and how the assigned value is written back.

use super::ty::Ty;
use super::{field_place, origin};
use crate::{type_def, ImmutableString};

#[derive(Clone)]
pub(super) struct FieldLvalue {
    pub(super) binding: ImmutableString,
    pub(super) definition: type_def::Definition,
    pub(super) slot: usize,
    /// The field's name, as written.
    pub(super) member: ImmutableString,
    pub(super) ty: Ty,
}

impl FieldLvalue {
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
    Field(FieldLvalue),
    /// An element of a local array: `binding[index]`.
    Place {
        binding: ImmutableString,
        ty: Ty,
        place: origin::Fragment,
    },
    /// `place[low..high]` of a local's array (`base`): changed through a
    /// copy stored back over the range (`slice_copy`).
    Slice(Box<SliceLvalue>),
    /// A place through at least one field below a local or its element:
    /// `s.cells[i]`, `v[0].cells[j]`, `s.a.b`, changed through a copy of
    /// each field on the way, stored back (`nested_parts`).
    Nested(Box<NestedLvalue>),
}

#[derive(Clone)]
pub(super) struct NestedLvalue {
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
pub(super) fn opens_compound(nested: &NestedLvalue) -> bool {
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
pub(super) struct SliceLvalue {
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
