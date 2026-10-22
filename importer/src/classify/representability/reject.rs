//! Stable positioned rejection reasons.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Kind {
    Module,
    Union,
    Static,
    Trait,
    AssociatedType,
    TypeConstant,
    SysrootDefinition,
    UnsafeFunction,
    AsyncFunction,
    VariadicFunction,
    ForeignAbi,
    HigherRankedBinder,
    EscapingBorrow,
    NonStaticCallable,
    CallableProtocol,
    ExplicitReceiver,
    UnsafeImplementation,
    NegativeImplementation,
    ConstImplementation,
    DefaultImplementation,
    GenericImplementation,
    ReferenceType,
    RawPointer,
    FunctionPointer,
    TraitObject,
    OpaqueType,
    Projection,
    TupleType,
    SliceType,
    ArrayType,
    NeverType,
    UnsupportedPrimitive,
    UnsupportedGenericArgument,
    LifetimeOrConstGeneric,
    Predicate,
    PrivateType,
    UnnameableType,
    UnresolvedDefinition,
    MissingConstantValue,
    UnevaluatedConstant,
    UnsupportedConstant,
    Extraction,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Reason {
    kind: Kind,
    span: crate::api::model::Span,
    extraction: Option<crate::api::ledger::Reason>,
}

impl Reason {
    pub(crate) const fn classification(kind: Kind, span: crate::api::model::Span) -> Self {
        Self {
            kind,
            span,
            extraction: None,
        }
    }

    pub(crate) fn extraction(
        reason: &crate::api::ledger::Reason,
        span: crate::api::model::Span,
    ) -> Self {
        Self {
            kind: Kind::Extraction,
            span,
            extraction: Some(reason.clone()),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    #[must_use]
    pub const fn span(&self) -> &crate::api::model::Span {
        &self.span
    }

    #[must_use]
    pub const fn extraction_reason(&self) -> Option<&crate::api::ledger::Reason> {
        self.extraction.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn closed_rejection_families_have_distinct_stable_encodings() {
        let kinds = [
            Kind::Module,
            Kind::Union,
            Kind::Static,
            Kind::Trait,
            Kind::AssociatedType,
            Kind::TypeConstant,
            Kind::SysrootDefinition,
            Kind::UnsafeFunction,
            Kind::AsyncFunction,
            Kind::VariadicFunction,
            Kind::ForeignAbi,
            Kind::HigherRankedBinder,
            Kind::EscapingBorrow,
            Kind::NonStaticCallable,
            Kind::CallableProtocol,
            Kind::ExplicitReceiver,
            Kind::UnsafeImplementation,
            Kind::NegativeImplementation,
            Kind::ConstImplementation,
            Kind::DefaultImplementation,
            Kind::GenericImplementation,
            Kind::ReferenceType,
            Kind::RawPointer,
            Kind::FunctionPointer,
            Kind::TraitObject,
            Kind::OpaqueType,
            Kind::Projection,
            Kind::TupleType,
            Kind::SliceType,
            Kind::ArrayType,
            Kind::NeverType,
            Kind::UnsupportedPrimitive,
            Kind::UnsupportedGenericArgument,
            Kind::LifetimeOrConstGeneric,
            Kind::Predicate,
            Kind::PrivateType,
            Kind::UnnameableType,
            Kind::UnresolvedDefinition,
            Kind::MissingConstantValue,
            Kind::UnevaluatedConstant,
            Kind::UnsupportedConstant,
            Kind::Extraction,
        ];
        assert_eq!(kinds.len(), 42);
        let encoded = kinds
            .iter()
            .map(|kind| serde_json::to_string(kind).expect("rejection kind encodes"))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(encoded.len(), kinds.len());
    }
}
