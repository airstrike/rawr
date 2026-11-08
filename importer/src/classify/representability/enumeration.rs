//! Truthful imported enum shape.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Variant {
    selector: crate::surface::identity::Selector,
    name: Box<str>,
    form: crate::api::model::FieldForm,
    fields: Box<[super::structure::Field]>,
    constructible: bool,
    span: crate::api::model::Span,
    /// The first public field rawr can't hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unsupported: Option<super::structure::Unsupported>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    parameters: Box<[super::signature::Parameter]>,
    variants: Box<[Variant]>,
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
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl Variant {
    pub(crate) const fn new(
        selector: crate::surface::identity::Selector,
        name: Box<str>,
        form: crate::api::model::FieldForm,
        fields: Box<[super::structure::Field]>,
        constructible: bool,
        span: crate::api::model::Span,
    ) -> Self {
        Self {
            selector,
            name,
            form,
            fields,
            constructible,
            span,
            unsupported: None,
        }
    }

    /// The variant, with `unsupported` as the first public field rawr
    /// can't hold.
    pub(crate) fn with_unsupported(
        mut self,
        unsupported: Option<super::structure::Unsupported>,
    ) -> Self {
        self.unsupported = unsupported;
        self
    }

    /// The first public field rawr can't hold.
    #[must_use]
    pub const fn unsupported(&self) -> Option<&super::structure::Unsupported> {
        self.unsupported.as_ref()
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
    pub const fn fields(&self) -> &[super::structure::Field] {
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
    pub const fn span(&self) -> &crate::api::model::Span {
        &self.span
    }
}

impl Plan {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        parameters: Box<[super::signature::Parameter]>,
        variants: Box<[Variant]>,
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
            parameters,
            variants,
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
        }
    }

    #[must_use]
    pub const fn parameters(&self) -> &[super::signature::Parameter] {
        &self.parameters
    }

    #[must_use]
    pub const fn variants(&self) -> &[Variant] {
        &self.variants
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

    /// Drop the variant fields `keep` refuses. A variant that lost one cannot
    /// be built, as with a private field, and `written` names the first one
    /// it lost.
    pub(crate) fn retain_fields(
        &mut self,
        mut keep: impl FnMut(&super::structure::Field) -> bool,
        mut written: impl FnMut(&super::structure::Field) -> super::structure::Unsupported,
    ) {
        for variant in &mut self.variants {
            let before = variant.fields.len();
            let mut fields = std::mem::take(&mut variant.fields).into_vec();
            let mut unsupported = variant.unsupported.take();
            fields.retain(|field| {
                let kept = keep(field);
                if !kept && unsupported.is_none() {
                    unsupported = Some(written(field));
                }
                kept
            });
            variant.unsupported = unsupported;
            if fields.len() != before {
                variant.constructible = false;
            }
            variant.fields = fields.into_boxed_slice();
        }
    }
}
