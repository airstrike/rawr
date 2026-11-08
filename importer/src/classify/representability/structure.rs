//! Truthful imported struct shape.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Field {
    selector: crate::surface::identity::Selector,
    name: Box<str>,
    index: u32,
    ty: super::convert::Type,
    span: crate::api::model::Span,
}

/// The first public field of a struct or variant whose type rawr can't
/// hold, as Rust writes it: the reason a literal can't build the type,
/// which a message names at the literal (D664).
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Unsupported {
    field: Box<str>,
    ty: Box<str>,
}

impl Unsupported {
    pub(crate) const fn new(field: Box<str>, ty: Box<str>) -> Self {
        Self { field, ty }
    }

    /// The field's name, or its index in a tuple struct.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The field's type, as the crate writes it.
    #[must_use]
    pub fn ty(&self) -> &str {
        &self.ty
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    constructor: Option<crate::surface::identity::Selector>,
    parameters: Box<[super::signature::Parameter]>,
    form: crate::api::model::FieldForm,
    fields: Box<[Field]>,
    constructible: bool,
    declaration_only: bool,
    copy: bool,
    default: bool,
    debug: bool,
    clone: bool,
    partial_eq: bool,
    eq: bool,
    hash: bool,
    /// How many lifetime parameters the type declares, before its type
    /// parameters: a value of it may borrow, and lives in a scope frame's
    /// region when it does (D558 tier 1).
    #[serde(default, skip_serializing_if = "is_zero")]
    lifetimes: u32,
    /// Whether the type is covariant (or bivariant) in each of its
    /// lifetimes, as rustc's variance says: its value at `'static` is one
    /// at a scope frame region's shorter lifetime.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    covariant: bool,
    /// The positions, among its type parameters, of those a field's boxed
    /// callable gives, as `view: Box<dyn Fn(&D) -> W>` of iced's `Lazy<'a,
    /// W, D>` gives `W`: a script closure gives them at `'static`.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    given: Vec<u32>,
    /// The crate implements `Iterator` for the type: a `for` loop walks it
    /// by its `next` (D558 slice 2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    iterator: bool,
    /// The crate implements `PartialOrd` for the type.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    partial_ord: bool,
    /// The crate implements `Ord` for the type: a value of it keys a
    /// B-tree map or set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    ord: bool,
    /// The crate implements `std::error::Error` for the type: a value of
    /// it boxes as a `Box<dyn Error>` (#1451).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    error: bool,
    /// The first public field rawr can't hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unsupported: Option<Unsupported>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl Field {
    pub(crate) const fn new(
        selector: crate::surface::identity::Selector,
        name: Box<str>,
        index: u32,
        ty: super::convert::Type,
        span: crate::api::model::Span,
    ) -> Self {
        Self {
            selector,
            name,
            index,
            ty,
            span,
        }
    }

    #[must_use]
    pub const fn selector(&self) -> &crate::surface::identity::Selector {
        &self.selector
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    #[must_use]
    pub const fn ty(&self) -> &super::convert::Type {
        &self.ty
    }

    #[must_use]
    pub const fn span(&self) -> &crate::api::model::Span {
        &self.span
    }
}

impl Plan {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        constructor: Option<crate::surface::identity::Selector>,
        parameters: Box<[super::signature::Parameter]>,
        form: crate::api::model::FieldForm,
        fields: Box<[Field]>,
        constructible: bool,
        declaration_only: bool,
        copy: bool,
        default: bool,
        debug: bool,
        clone: bool,
        partial_eq: bool,
        eq: bool,
        hash: bool,
    ) -> Self {
        Self {
            constructor,
            parameters,
            form,
            fields,
            constructible,
            declaration_only,
            copy,
            default,
            debug,
            clone,
            partial_eq,
            eq,
            hash,
            lifetimes: 0,
            covariant: false,
            given: Vec::new(),
            iterator: false,
            partial_ord: false,
            ord: false,
            error: false,
            unsupported: None,
        }
    }

    /// The plan, with `unsupported` as the first public field rawr can't
    /// hold.
    pub(crate) fn with_unsupported(mut self, unsupported: Option<Unsupported>) -> Self {
        self.unsupported = unsupported;
        self
    }

    /// The first public field rawr can't hold.
    #[must_use]
    pub const fn unsupported(&self) -> Option<&Unsupported> {
        self.unsupported.as_ref()
    }

    #[must_use]
    pub const fn constructor(&self) -> Option<&crate::surface::identity::Selector> {
        self.constructor.as_ref()
    }

    #[must_use]
    pub const fn parameters(&self) -> &[super::signature::Parameter] {
        &self.parameters
    }

    #[must_use]
    pub const fn fields(&self) -> &[Field] {
        &self.fields
    }

    #[must_use]
    pub const fn form(&self) -> crate::api::model::FieldForm {
        self.form
    }

    #[must_use]
    pub const fn constructible(&self) -> bool {
        self.constructible
    }

    #[must_use]
    pub const fn declaration_only(&self) -> bool {
        self.declaration_only
    }

    #[must_use]
    pub const fn copy(&self) -> bool {
        self.copy
    }

    /// Whether the crate implements `Default` for the type.
    #[must_use]
    pub const fn default(&self) -> bool {
        self.default
    }

    /// Whether the crate implements `Debug` for the type.
    #[must_use]
    pub const fn debug(&self) -> bool {
        self.debug
    }

    /// Whether the crate implements `Clone` for the type.
    #[must_use]
    pub const fn cloneable(&self) -> bool {
        self.clone
    }

    /// Whether the crate implements `PartialEq` for the type.
    #[must_use]
    pub const fn partial_eq(&self) -> bool {
        self.partial_eq
    }

    /// Whether the crate implements `Eq` for the type.
    #[must_use]
    pub const fn eq(&self) -> bool {
        self.eq
    }

    /// Whether the crate implements `Hash` for the type.
    #[must_use]
    pub const fn hash(&self) -> bool {
        self.hash
    }

    /// The plan, with the crate's `PartialOrd` and `Ord` for the type.
    pub(crate) const fn with_order(mut self, partial_ord: bool, ord: bool) -> Self {
        self.partial_ord = partial_ord;
        self.ord = ord;
        self
    }

    pub(crate) const fn with_error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    /// Whether the crate implements `std::error::Error` for the type.
    #[must_use]
    pub const fn error(&self) -> bool {
        self.error
    }

    /// Whether the crate implements `PartialOrd` for the type.
    #[must_use]
    pub const fn partial_ord(&self) -> bool {
        self.partial_ord
    }

    /// Whether the crate implements `Ord` for the type.
    #[must_use]
    pub const fn ord(&self) -> bool {
        self.ord
    }

    pub(crate) const fn with_borrowing(
        mut self,
        lifetimes: u32,
        covariant: bool,
        iterator: bool,
    ) -> Self {
        self.lifetimes = lifetimes;
        self.covariant = covariant;
        self.iterator = iterator;
        self
    }

    /// How many lifetime parameters the type declares.
    #[must_use]
    pub const fn lifetimes(&self) -> u32 {
        self.lifetimes
    }

    /// Whether the type is covariant in each of its lifetimes.
    #[must_use]
    pub const fn covariant(&self) -> bool {
        self.covariant
    }

    /// The plan, with the positions of the type parameters a field's boxed
    /// callable gives.
    pub(crate) fn with_given(mut self, given: Vec<u32>) -> Self {
        self.given = given;
        self
    }

    /// The positions, among its type parameters, of those a field's boxed
    /// callable gives.
    #[must_use]
    pub fn given(&self) -> &[u32] {
        &self.given
    }

    /// Whether the crate implements `Iterator` for the type.
    #[must_use]
    pub const fn iterator(&self) -> bool {
        self.iterator
    }

    /// Drop the public fields `keep` refuses. A struct that lost one has no
    /// constructor, as with a private field, and `written` names the first
    /// one it lost.
    pub(crate) fn retain_fields(
        &mut self,
        mut keep: impl FnMut(&Field) -> bool,
        mut written: impl FnMut(&Field) -> Unsupported,
    ) {
        let before = self.fields.len();
        let mut fields = std::mem::take(&mut self.fields).into_vec();
        let mut unsupported = self.unsupported.take();
        fields.retain(|field| {
            let kept = keep(field);
            if !kept && unsupported.is_none() {
                unsupported = Some(written(field));
            }
            kept
        });
        self.unsupported = unsupported;
        if fields.len() != before {
            self.constructible = false;
            self.constructor = None;
        }
        self.fields = fields.into_boxed_slice();
    }
}
