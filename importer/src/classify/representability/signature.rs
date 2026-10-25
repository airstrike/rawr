//! Admitted declared generic and callable signature vocabulary.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Parameter {
    index: u32,
    name: Box<str>,
    kind: Kind,
    /// A type's defaulted parameter (`Hsl<S = Srgb>`): what a type written
    /// without the argument takes.
    default: Option<super::convert::Type>,
}

/// One admitted generic substitution kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Kind {
    Type,
    Constant,
    Callable,
    /// `F: Future`: a script future fills it as `rawr::future::Script`, and
    /// the instance writes it `_` (D527). Its `F::Output` is the synthetic
    /// type parameter [`future_output`] names.
    Future,
}

/// The bit that marks a synthetic type parameter: the `Output` of the
/// future parameter in its low bits (D527).
pub const FUTURE_OUTPUT: u32 = 0x8000_0000;

/// The synthetic parameter that stands for `F::Output` of the future
/// parameter at `index`.
#[must_use]
pub const fn future_output(index: u32) -> u32 {
    FUTURE_OUTPUT | index
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Signature {
    parameters: Box<[Parameter]>,
    inputs: Box<[super::convert::Type]>,
    output: super::convert::Type,
}

impl Parameter {
    pub(crate) const fn new(index: u32, name: Box<str>, kind: Kind) -> Self {
        Self {
            index,
            name,
            kind,
            default: None,
        }
    }

    pub(crate) fn with_default(mut self, default: Option<super::convert::Type>) -> Self {
        self.default = default;
        self
    }

    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// The type a type written without this argument takes.
    #[must_use]
    pub const fn default(&self) -> Option<&super::convert::Type> {
        self.default.as_ref()
    }

    pub(crate) fn mark_callable(&mut self) {
        self.kind = Kind::Callable;
    }

    pub(crate) fn mark_future(&mut self) {
        self.kind = Kind::Future;
    }

    /// Whether this is the synthetic `F::Output` of a future parameter.
    #[must_use]
    pub const fn is_future_output(&self) -> bool {
        self.index & FUTURE_OUTPUT != 0
    }
}

impl Signature {
    pub(crate) const fn new(
        parameters: Box<[Parameter]>,
        inputs: Box<[super::convert::Type]>,
        output: super::convert::Type,
    ) -> Self {
        Self {
            parameters,
            inputs,
            output,
        }
    }

    #[must_use]
    pub const fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }

    #[must_use]
    pub const fn inputs(&self) -> &[super::convert::Type] {
        &self.inputs
    }

    #[must_use]
    pub const fn output(&self) -> &super::convert::Type {
        &self.output
    }
}
