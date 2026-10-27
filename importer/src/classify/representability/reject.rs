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
    /// A signature names a future parameter `F` other than as the input
    /// that takes the future or through `F::Output`, as tokio's
    /// `LocalKey::scope` returns a `TaskLocalFuture<T, F>`. A script
    /// future fills `F` with a type Rawr has no name for (D527).
    ScriptFutureType,
    /// A `macro_rules!` macro: Rawr expands its source, so it has no native
    /// operation (the facade plan carries the source).
    MacroRules,
}

impl Kind {
    /// Why an item of this kind has no binding, as a diagnostic says it.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Module => "it is a module",
            Self::Union => "it is a union",
            Self::Static => "it is a static rawr can't read",
            Self::Trait => "it is a trait",
            Self::AssociatedType => "it is an associated type",
            Self::TypeConstant => "it is a type-level constant",
            Self::SysrootDefinition => "it is a standard library definition",
            Self::UnsafeFunction => "it is an unsafe function",
            Self::AsyncFunction => "it is an async function",
            Self::VariadicFunction => "it is a variadic function",
            Self::ForeignAbi => "it uses a foreign ABI",
            Self::HigherRankedBinder => "its signature has a higher-ranked lifetime",
            Self::EscapingBorrow => "its output keeps a borrow rawr can't hold",
            Self::NonStaticCallable => "it takes a callable that isn't 'static",
            Self::CallableProtocol => "it takes a callable rawr can't make",
            Self::ExplicitReceiver => "its receiver has an explicit type",
            Self::UnsafeImplementation => "it is an unsafe impl",
            Self::NegativeImplementation => "it is a negative impl",
            Self::ConstImplementation => "it is a const impl",
            Self::DefaultImplementation => "it is a default impl",
            Self::GenericImplementation => "it is a generic impl",
            Self::ReferenceType => "its signature has a reference type rawr can't lend",
            Self::RawPointer => "its signature has a raw pointer",
            Self::FunctionPointer => "its signature has a function pointer",
            Self::TraitObject => "its signature has a trait object",
            Self::OpaqueType => "its signature has an opaque `impl Trait` type",
            Self::Projection => "its signature has an associated type projection",
            Self::TupleType => "its signature has a tuple type rawr can't hold there",
            Self::SliceType => "its signature has a slice type rawr can't hold there",
            Self::ArrayType => "its signature has an array type",
            Self::NeverType => "its signature has the never type",
            Self::UnsupportedPrimitive => "its signature has an unsupported primitive",
            Self::UnsupportedGenericArgument => "its signature has an unsupported generic argument",
            Self::LifetimeOrConstGeneric => "its signature borrows at a lifetime rawr can't hold",
            Self::Predicate => "it has a where clause rawr can't check",
            Self::PrivateType => "its signature names a private type",
            Self::UnnameableType => "its signature names a type generated code can't name",
            Self::UnresolvedDefinition => {
                "its signature names a definition the importer didn't see"
            }
            Self::MissingConstantValue => "it is a constant without a value",
            Self::UnevaluatedConstant => "it is a constant rustc didn't evaluate",
            Self::UnsupportedConstant => "it is a constant of an unsupported type",
            Self::Extraction => "the importer couldn't extract it",
            Self::ScriptFutureType => "its signature names a future parameter's own type",
            Self::MacroRules => "it is a `macro_rules!` macro, which rawr expands from its source",
        }
    }
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
            Kind::ScriptFutureType,
            Kind::MacroRules,
        ];
        assert_eq!(kinds.len(), 44);
        let encoded = kinds
            .iter()
            .map(|kind| serde_json::to_string(kind).expect("rejection kind encodes"))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(encoded.len(), kinds.len());
    }
}
