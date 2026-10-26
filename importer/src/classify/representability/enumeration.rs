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
    /// The crate implements `Iterator` for the type: a `for` loop walks it
    /// by its `next` (D558 slice 2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    iterator: bool,
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
            iterator: false,
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

    pub(crate) const fn with_borrowing(mut self, lifetimes: u32, iterator: bool) -> Self {
        self.lifetimes = lifetimes;
        self.iterator = iterator;
        self
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

    /// Drop the variant fields `keep` refuses. A variant that lost one cannot
    /// be built, as with a private field.
    pub(crate) fn retain_fields(&mut self, mut keep: impl FnMut(&super::structure::Field) -> bool) {
        for variant in &mut self.variants {
            let before = variant.fields.len();
            let mut fields = std::mem::take(&mut variant.fields).into_vec();
            fields.retain(|field| keep(field));
            if fields.len() != before {
                variant.constructible = false;
            }
            variant.fields = fields.into_boxed_slice();
        }
    }
}
