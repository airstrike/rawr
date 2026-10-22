//! Closed representability results carrying complete plan or rejection proofs.

use serde::{Deserialize, Serialize};

pub mod callable;
pub mod constant;
pub mod convert;
pub mod enumeration;
pub mod exclude;
pub mod function;
pub mod method;
pub mod origin;
pub mod ownership;
pub mod predicate;
pub mod reject;
pub mod signature;
pub mod structure;
pub mod value;

/// The sole closed result for each selected Rust path.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Representability {
    Value(value::Plan),
    Struct(structure::Plan),
    Enum(enumeration::Plan),
    Function(function::Plan),
    Method(method::Plan),
    Constant(constant::Plan),
    FunctionItem(callable::Item),
    Invocation(callable::Invocation),
    Excluded(exclude::Reason),
    Rejected(reject::Reason),
}

impl Representability {
    pub(crate) fn contains_selector(
        &self,
        declared: &crate::surface::identity::Selector,
        selector: &crate::surface::identity::Selector,
    ) -> bool {
        use crate::surface::identity::Selector;
        match self {
            Self::Struct(plan) => {
                matches!(selector, Selector::Declaration)
                    || plan.constructor() == Some(selector)
                    || plan
                        .fields()
                        .iter()
                        .any(|field| field.selector() == selector)
            }
            Self::Enum(plan) => {
                matches!(selector, Selector::Declaration)
                    || plan
                        .variants()
                        .iter()
                        .any(|variant| variant.selector() == selector)
            }
            Self::Value(_)
            | Self::Function(_)
            | Self::Method(_)
            | Self::Constant(_)
            | Self::FunctionItem(_)
            | Self::Invocation(_) => declared == selector,
            Self::Excluded(_) | Self::Rejected(_) => false,
        }
    }

    /// Return whether Phase 2 may retain a symbolic use in sealed demand.
    #[must_use]
    pub const fn is_demandable(&self) -> bool {
        match self {
            Self::Value(_) => false,
            Self::Struct(plan) => !plan.declaration_only(),
            Self::Enum(plan) => !plan.declaration_only(),
            Self::Function(_) | Self::Method(_) | Self::Constant(_) | Self::Invocation(_) => true,
            Self::FunctionItem(_) | Self::Excluded(_) | Self::Rejected(_) => false,
        }
    }
}
