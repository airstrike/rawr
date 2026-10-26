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
    /// The crate implements `Iterator` for the type: a `for` loop walks it
    /// by its `next` (D558 slice 2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    iterator: bool,
    /// An iced type whose lifetime is iced's view lifetime, as pane_grid's
    /// `PaneGrid<'a, ..>` and `Content<'a, ..>`: it has a lifetime and a
    /// type parameter bounded by iced's `Renderer`. In an iced program its
    /// value is a recipe replayed at each lowering (D558 tier 2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    view: bool,
    /// A view type iced implements `Widget` for, which a view shows as an
    /// element.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    widget: bool,
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
            iterator: false,
            view: false,
            widget: false,
        }
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

    pub(crate) const fn with_borrowing(mut self, lifetimes: u32, iterator: bool) -> Self {
        self.lifetimes = lifetimes;
        self.iterator = iterator;
        self
    }

    pub(crate) const fn with_view(mut self, view: bool, widget: bool) -> Self {
        self.view = view;
        self.widget = view && widget;
        self
    }

    /// Whether the type is an iced view type ([`Plan`]'s `view`).
    #[must_use]
    pub const fn view(&self) -> bool {
        self.view
    }

    /// Whether the type is a view type iced implements `Widget` for.
    #[must_use]
    pub const fn widget(&self) -> bool {
        self.widget
    }

    /// How many lifetime parameters the type declares.
    #[must_use]
    pub const fn lifetimes(&self) -> u32 {
        self.lifetimes
    }

    /// Whether the crate implements `Iterator` for the type.
    #[must_use]
    pub const fn iterator(&self) -> bool {
        self.iterator
    }

    /// Drop the public fields `keep` refuses. A struct that lost one has no
    /// constructor, as with a private field.
    pub(crate) fn retain_fields(&mut self, mut keep: impl FnMut(&Field) -> bool) {
        let before = self.fields.len();
        let mut fields = std::mem::take(&mut self.fields).into_vec();
        fields.retain(|field| keep(field));
        if fields.len() != before {
            self.constructible = false;
            self.constructor = None;
        }
        self.fields = fields.into_boxed_slice();
    }
}
