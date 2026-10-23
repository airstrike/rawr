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
    pub(crate) const fn new(
        parameters: Box<[super::signature::Parameter]>,
        variants: Box<[Variant]>,
        declaration_only: bool,
        copy: bool,
        default: bool,
    ) -> Self {
        Self {
            parameters,
            variants,
            declaration_only,
            copy,
            default,
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
}
