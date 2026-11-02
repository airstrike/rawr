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
    /// A closure parameter whose output is a future or a sipper, as
    /// `sipper(f: F)` with `F: FnOnce(Sender<P>) -> Fut, Fut: Future` takes:
    /// a script's `async` block has no Rust type to give it (#545).
    FutureClosure,
    /// A `macro_rules!` macro: Rawr expands its source, so it has no native
    /// operation (the facade plan carries the source).
    MacroRules,
    /// A closure parameter whose input is an associated type of a type
    /// parameter, as iced's `Task::sip` takes `impl FnMut(S::Item) -> T`:
    /// the type the call fixes `S` to decides it (#568).
    ProjectedClosureInput,
}

impl Kind {
    /// Why an item of this kind can't be used yet, naming the Rust
    /// construct as a diagnostic says it: "`item` is not supported yet:
    /// it ...".
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Module => "it is a module",
            Self::Union => "it is a union",
            Self::Static => "it is a `static`",
            Self::Trait => "it is a trait",
            Self::AssociatedType => "it is an associated type",
            Self::TypeConstant => "it is a type-level constant",
            Self::SysrootDefinition => "it is a standard library definition",
            Self::UnsafeFunction => "it is an `unsafe` function",
            Self::AsyncFunction => "it is an `async` function",
            Self::VariadicFunction => "it is a variadic function",
            Self::ForeignAbi => "it uses a foreign ABI",
            Self::HigherRankedBinder => "its signature has a higher-ranked lifetime (`for<'a>`)",
            Self::EscapingBorrow => {
                "its return type borrows at a lifetime that none of its arguments has"
            }
            Self::CallableProtocol => "it takes a closure whose type isn't supported",
            Self::ExplicitReceiver => "its receiver has an explicit type (`self: Type`)",
            Self::UnsafeImplementation => "it is an `unsafe impl`",
            Self::NegativeImplementation => "it is a negative impl",
            Self::ConstImplementation => "it is a `const` impl",
            Self::DefaultImplementation => "it is a `default` impl",
            Self::GenericImplementation => "it is a generic impl",
            Self::ReferenceType => "its signature borrows a type that can't be lent yet",
            Self::RawPointer => "its signature has a raw pointer",
            Self::FunctionPointer => "its signature has a function pointer",
            Self::TraitObject => "its signature has a trait object",
            Self::OpaqueType => "its signature has an opaque `impl Trait` type",
            Self::Projection => "its signature has an associated type projection",
            Self::TupleType => "its signature has a tuple type where one isn't supported yet",
            Self::SliceType => "its signature has a slice type where one isn't supported yet",
            Self::ArrayType => "its signature has an array type",
            Self::NeverType => "its signature has the never type",
            Self::UnsupportedPrimitive => "its signature has a primitive type that isn't supported",
            Self::UnsupportedGenericArgument => {
                "its signature has a generic argument that isn't supported"
            }
            Self::LifetimeOrConstGeneric => "its signature borrows at an early-bound lifetime",
            Self::Predicate => "it has a `where` clause that isn't supported",
            Self::PrivateType => "its signature names a private type",
            Self::UnnameableType => "its signature names a type that has no public path",
            Self::UnresolvedDefinition => "its signature names an item that has no public path",
            Self::MissingConstantValue => "it is a constant without a value",
            Self::UnevaluatedConstant => "it is a constant whose value isn't known at compile time",
            Self::UnsupportedConstant => "it is a constant of a type that isn't supported",
            Self::Extraction => "it is a kind of Rust item that isn't supported",
            Self::ScriptFutureType => "its signature names the type of the future it takes",
            Self::FutureClosure => "it takes a closure that returns a future",
            Self::MacroRules => "it is a `macro_rules!` macro",
            Self::ProjectedClosureInput => {
                "it takes a closure whose input is an associated type of a type parameter"
            }
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

    /// Why the item can't be used yet, as [`Kind::describe`] says it, with
    /// what the compiler reported for an item it couldn't describe.
    #[must_use]
    pub const fn describe(&self) -> &'static str {
        use crate::api::ledger::Reason as Ledger;
        match &self.extraction {
            Some(Ledger::UnnameablePrivate(_)) => Kind::PrivateType.describe(),
            Some(Ledger::UnresolvedDefinition(_)) => Kind::UnresolvedDefinition.describe(),
            Some(Ledger::ExternalAuthorityUnavailable { .. }) => {
                "it comes from a crate that `cargo metadata` doesn't list"
            }
            Some(Ledger::UnsupportedPublicPath { .. }) => {
                "its public path names a kind of Rust item that isn't supported"
            }
            _ => self.kind.describe(),
        }
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
            Kind::FutureClosure,
            Kind::MacroRules,
            Kind::ProjectedClosureInput,
        ];
        assert_eq!(kinds.len(), 46);
        let encoded = kinds
            .iter()
            .map(|kind| serde_json::to_string(kind).expect("rejection kind encodes"))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(encoded.len(), kinds.len());
    }
}
