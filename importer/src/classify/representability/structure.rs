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
