//! Bounded versioned external-metadata probe protocol.

use serde::Deserialize;

use crate::cargo;
use crate::{Error, Result};

pub(crate) mod attempt;

#[cfg(test)]
std::thread_local! {
    static ACCOUNTED_DECODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) const SCHEMA: u32 = 27;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    pub(crate) schema: u32,
    pub(crate) fragments: Vec<Fragment>,
}

/// Allocation-free logical populations in one fully owned compiler document.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Population {
    pub(crate) fragments: usize,
    pub(crate) definitions: usize,
    pub(crate) implementations: usize,
    pub(crate) scalar_predicate_proofs: usize,
    pub(crate) exports: usize,
    pub(crate) anonymous: usize,
    pub(crate) types: usize,
    pub(crate) predicates: usize,
    pub(crate) generics: usize,
    pub(crate) instantiations: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fragment {
    pub(crate) handshake: Handshake,
    pub(crate) reachable: Vec<Export>,
    pub(crate) definitions: Vec<Definition>,
    pub(crate) implementations: Vec<Implementation>,
    pub(crate) scalar_predicate_proofs: Vec<ScalarPredicateProof>,
    /// The selected crate's name index, published for demanded units.
    #[serde(default)]
    pub(crate) index: Vec<crate::api::ledger::Name>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScalarPredicateProof {
    pub(crate) owner: Id,
    pub(crate) parameter: u32,
    pub(crate) trait_id: Id,
    pub(crate) scalar: Scalar,
    pub(crate) outcome: ScalarPredicateOutcome,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scalar {
    Unit,
    Bool,
    Char,
    F64,
    I64,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ScalarPredicateOutcome {
    Absent,
    Positive { implementation: Id },
    Unresolved,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Handshake {
    pub(crate) schema: u32,
    pub(crate) package: Package,
    pub(crate) root: Root,
    pub(crate) target: String,
    pub(crate) target_kind: String,
    pub(crate) target_kinds: Vec<String>,
    pub(crate) crate_types: Vec<String>,
    pub(crate) crate_name: String,
    pub(crate) edition: String,
    pub(crate) profile: String,
    pub(crate) compilation: Compilation,
    pub(crate) host: String,
    pub(crate) selected_target: String,
    pub(crate) features: Vec<String>,
    pub(crate) probe_arguments_digest: String,
    pub(crate) compiler_commit: String,
    pub(crate) invocation_nonce: String,
    pub(crate) unit_correlation: String,
    pub(crate) unit_key: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) artifact: Artifact,
    pub(crate) sysroot: Vec<Sysroot>,
    pub(crate) loaded_crates: Vec<ExternalCrate>,
    pub(crate) external_crates: Vec<ExternalCrate>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub(crate) path: String,
    pub(crate) length: u64,
    pub(crate) digest: String,
    pub(crate) profile: String,
    pub(crate) fresh: bool,
    pub(crate) crate_hash: String,
    pub(crate) stable_crate_id: String,
    pub(crate) probe_alias: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Package {
    pub(crate) cargo_id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sysroot {
    pub(crate) compiler_commit: String,
    pub(crate) selected_target: String,
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalCrate {
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
    pub(crate) artifacts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Root {
    Selected { aliases: Vec<String> },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Compilation {
    Target,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Id {
    pub(crate) authority: Authority,
    pub(crate) def_path_hash: String,
    pub(crate) namespace: Namespace,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Authority {
    Compiler {
        crate_name: String,
        stable_crate_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Namespace {
    Type,
    Value,
    Macro,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Export {
    pub(crate) binding: Binding,
    pub(crate) root_alias: String,
    pub(crate) path: Vec<String>,
    pub(crate) namespace: Namespace,
    pub(crate) target: ExportTarget,
    pub(crate) aliases: Vec<Alias>,
    pub(crate) reexports: Vec<Reexport>,
    pub(crate) span: Span,
    /// A re-exported enum variant's name, whose enum `target` is.
    #[serde(default)]
    pub(crate) variant: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Binding {
    Nameable,
    AnonymousImport,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ExportTarget {
    Definition { id: Id },
    Unsupported { compiler_kind: String },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reexport {
    pub(crate) kind: ReexportKind,
    pub(crate) evidence: ReexportEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ReexportEvidence {
    Identified { item: ReexportId, span: Span },
    IdentitylessMacro,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReexportKind {
    Single,
    Glob,
    ExternCrate,
    MacroUse,
    MacroExport,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReexportId {
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
    pub(crate) def_path_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Alias {
    pub(crate) from: Vec<String>,
    pub(crate) to: Vec<String>,
    pub(crate) span: Span,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Definition {
    pub(crate) id: Id,
    pub(crate) display_path: Vec<String>,
    pub(crate) owner: Owner,
    pub(crate) visibility: Visibility,
    pub(crate) shape: Shape,
    pub(crate) span: Span,
    pub(crate) origin: Origin,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Owner {
    Free,
    Definition { id: Id },
    Implementation { id: Id },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Visibility {
    Public,
    Restricted { scope: Id },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[allow(
    clippy::large_enum_variant,
    reason = "the versioned wire shape keeps each compiler record inline for exact decoding"
)]
pub(crate) enum Shape {
    UnnameablePrivate,
    Module,
    Struct {
        field_form: FieldForm,
        fields: Vec<Field>,
        fields_complete: bool,
        non_exhaustive: bool,
        generics: Generics,
    },
    Union {
        field_form: FieldForm,
        fields: Vec<Field>,
        fields_complete: bool,
        non_exhaustive: bool,
        generics: Generics,
    },
    Enum {
        variants: Vec<Variant>,
        generics: Generics,
    },
    Const {
        ty: Type,
        value: Default<Const>,
    },
    TypeConst {
        ty: Type,
        rhs: Default<Const>,
    },
    Static {
        ty: Type,
        mutable: bool,
        thread_local: bool,
    },
    TypeAlias {
        ty: Type,
        generics: Generics,
    },
    Function {
        signature: Signature,
        generics: Generics,
    },
    Trait {
        unsafe_trait: bool,
        auto_trait: bool,
        supertraits: Vec<Predicate>,
        associated: Vec<Id>,
        generics: Generics,
    },
    AssociatedType {
        bounds: Vec<Predicate>,
        default: Default<Type>,
        generics: Generics,
    },
    Sysroot {
        compiler_kind: String,
    },
    MacroRules {
        source: String,
        krate: String,
    },
    Unknown {
        compiler_kind: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Default<T> {
    Absent,
    Present { value: T },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) index: u32,
    pub(crate) visibility: Visibility,
    pub(crate) ty: Type,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FieldForm {
    Record,
    Tuple,
    Unit,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Variant {
    pub(crate) name: String,
    pub(crate) field_form: FieldForm,
    pub(crate) fields: Vec<Field>,
    pub(crate) fields_complete: bool,
    pub(crate) non_exhaustive: bool,
    pub(crate) discriminant: Default<Const>,
    pub(crate) span: Span,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Signature {
    pub(crate) safety: Safety,
    pub(crate) const_function: bool,
    pub(crate) async_function: bool,
    pub(crate) abi: String,
    pub(crate) variadic: bool,
    pub(crate) binders: Vec<Binder>,
    pub(crate) receiver: Receiver,
    pub(crate) inputs: Vec<Type>,
    pub(crate) output: Type,
    #[serde(default)]
    pub(crate) guard: Option<Guard>,
}

/// A borrowing method's guard output: its `Deref::Target`, and whether it
/// is `DerefMut`.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Guard {
    pub(crate) target: Type,
    pub(crate) mutable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Safety {
    Safe,
    Unsafe,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Receiver {
    None,
    Value {
        ty: Type,
    },
    Reference {
        lifetime: Lifetime,
        mutability: Mutability,
        ty: Type,
    },
    Explicit {
        ty: Type,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mutability {
    Shared,
    Mutable,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Generics {
    pub(crate) parameters: Vec<Parameter>,
    pub(crate) predicates: Vec<Predicate>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parameter {
    pub(crate) owner: Id,
    pub(crate) index: u32,
    pub(crate) name: String,
    pub(crate) parameter_kind: ParameterKind,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ParameterKind {
    Lifetime {
        bounds: Vec<Bound>,
    },
    Type {
        bounds: Vec<Bound>,
        default: Default<Type>,
    },
    Const {
        ty: Type,
        default: Default<Const>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Predicate {
    Bound { ty: Type, bound: Bound },
    ProjectionEquality { projection: Type, ty: Type },
    LifetimeOutlives { longer: Lifetime, shorter: Lifetime },
    TypeOutlives { ty: Type, lifetime: Lifetime },
    ConstEvaluatable { value: Const },
    Unknown { compiler_kind: String },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Bound {
    Trait {
        trait_id: Id,
        arguments: Vec<Argument>,
        polarity: Polarity,
        binders: Vec<Binder>,
    },
    MaybeSized,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Polarity {
    Positive,
    Negative,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binder {
    pub(crate) owner: Id,
    pub(crate) depth: u32,
    pub(crate) index: u32,
    pub(crate) name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Lifetime {
    Static,
    Early { owner: Id, index: u32 },
    Late { binder: Binder, index: u32 },
    Erased,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[allow(
    clippy::enum_variant_names,
    reason = "SelfType is a stable versioned wire spelling distinct from other type variants"
)]
pub(crate) enum Type {
    Primitive {
        name: String,
    },
    Definition {
        id: Id,
        arguments: Vec<Argument>,
    },
    Parameter {
        owner: Id,
        index: u32,
    },
    SelfType {
        owner: Id,
    },
    Tuple {
        elements: Vec<Type>,
    },
    Slice {
        element: Box<Type>,
    },
    Array {
        element: Box<Type>,
        length: Const,
    },
    Reference {
        lifetime: Lifetime,
        mutability: Mutability,
        ty: Box<Type>,
    },
    RawPointer {
        mutability: Mutability,
        ty: Box<Type>,
    },
    FunctionPointer {
        signature: Box<Signature>,
    },
    TraitObject {
        bounds: Vec<ObjectBound>,
        lifetime: Lifetime,
    },
    ImplTrait {
        id: Id,
        bounds: Vec<Predicate>,
        captures: Vec<Argument>,
    },
    Projection {
        trait_id: Id,
        associated: Id,
        arguments: Vec<Argument>,
    },
    Never,
    Unknown {
        compiler_kind: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ObjectBound {
    Trait {
        bound: Bound,
    },
    ProjectionEquality {
        associated: Id,
        arguments: Vec<Argument>,
        ty: Type,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Argument {
    Lifetime { lifetime: Lifetime },
    Type { ty: Type },
    Const { value: Const },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Const {
    Literal {
        ty: String,
        value: String,
    },
    Parameter {
        owner: Id,
        index: u32,
    },
    Unevaluated {
        definition: Id,
        arguments: Vec<Argument>,
    },
    Unknown {
        compiler_kind: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Implementation {
    pub(crate) key: String,
    pub(crate) id: Id,
    pub(crate) relevance: Vec<Id>,
    pub(crate) trait_ref: Trait,
    pub(crate) self_type: Type,
    pub(crate) safety: Safety,
    pub(crate) polarity: Polarity,
    pub(crate) const_impl: bool,
    pub(crate) default_impl: bool,
    pub(crate) generics: Generics,
    pub(crate) associated: Vec<Id>,
    pub(crate) span: Span,
    pub(crate) origin: Origin,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Trait {
    Inherent,
    Trait { id: Id, arguments: Vec<Argument> },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub(crate) struct Span {
    pub(crate) file: String,
    pub(crate) byte_start: u32,
    pub(crate) byte_end: u32,
    pub(crate) line_start: u32,
    pub(crate) column_start: u32,
    pub(crate) line_end: u32,
    pub(crate) column_end: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Origin {
    Source,
    Expansion {
        call_site: Span,
        definition_site: Span,
    },
    Synthesized {
        mechanism: String,
    },
}

impl Document {
    pub(crate) fn population(&self) -> Population {
        let mut population = Population {
            fragments: self.fragments.len(),
            ..Population::default()
        };
        for fragment in &self.fragments {
            population.definitions = population
                .definitions
                .saturating_add(fragment.definitions.len());
            population.implementations = population
                .implementations
                .saturating_add(fragment.implementations.len());
            population.exports = population.exports.saturating_add(fragment.reachable.len());
            population.anonymous = population.anonymous.saturating_add(
                fragment
                    .reachable
                    .iter()
                    .filter(|export| matches!(export.binding, Binding::AnonymousImport))
                    .count(),
            );
            for definition in &fragment.definitions {
                population.shape(&definition.shape);
            }
            for implementation in &fragment.implementations {
                population.type_node(&implementation.self_type);
                population.generics(&implementation.generics);
                if let Trait::Trait { arguments, .. } = &implementation.trait_ref {
                    population.arguments(arguments);
                }
            }
        }
        population
    }
}

impl Population {
    fn shape(&mut self, shape: &Shape) {
        match shape {
            Shape::UnnameablePrivate
            | Shape::Module
            | Shape::Sysroot { .. }
            | Shape::MacroRules { .. }
            | Shape::Unknown { .. } => {}
            Shape::Struct {
                fields, generics, ..
            }
            | Shape::Union {
                fields, generics, ..
            } => {
                self.generics(generics);
                for field in fields {
                    self.type_node(&field.ty);
                }
            }
            Shape::Enum { variants, generics } => {
                self.generics(generics);
                for variant in variants {
                    for field in &variant.fields {
                        self.type_node(&field.ty);
                    }
                    if let Default::Present { value } = &variant.discriminant {
                        self.constant(value);
                    }
                }
            }
            Shape::Const { ty, value } | Shape::TypeConst { ty, rhs: value } => {
                self.type_node(ty);
                if let Default::Present { value } = value {
                    self.constant(value);
                }
            }
            Shape::Static { ty, .. } => self.type_node(ty),
            Shape::TypeAlias { ty, generics } => {
                self.type_node(ty);
                self.generics(generics);
            }
            Shape::Function {
                signature,
                generics,
            } => {
                self.signature(signature);
                self.generics(generics);
            }
            Shape::Trait {
                supertraits,
                generics,
                ..
            } => {
                self.predicates(supertraits);
                self.generics(generics);
            }
            Shape::AssociatedType {
                bounds,
                default,
                generics,
            } => {
                self.predicates(bounds);
                if let Default::Present { value } = default {
                    self.type_node(value);
                }
                self.generics(generics);
            }
        }
    }

    fn signature(&mut self, signature: &Signature) {
        match &signature.receiver {
            Receiver::None => {}
            Receiver::Value { ty } | Receiver::Reference { ty, .. } | Receiver::Explicit { ty } => {
                self.type_node(ty)
            }
        }
        for input in &signature.inputs {
            self.type_node(input);
        }
        self.type_node(&signature.output);
    }

    fn generics(&mut self, generics: &Generics) {
        self.generics = self.generics.saturating_add(1);
        for parameter in &generics.parameters {
            match &parameter.parameter_kind {
                ParameterKind::Lifetime { bounds } => {
                    for bound in bounds {
                        self.bound(bound);
                    }
                }
                ParameterKind::Type { bounds, default } => {
                    for bound in bounds {
                        self.bound(bound);
                    }
                    if let Default::Present { value } = default {
                        self.type_node(value);
                    }
                }
                ParameterKind::Const { ty, default } => {
                    self.type_node(ty);
                    if let Default::Present { value } = default {
                        self.constant(value);
                    }
                }
            }
        }
        self.predicates(&generics.predicates);
    }

    fn predicates(&mut self, predicates: &[Predicate]) {
        for predicate in predicates {
            self.predicate(predicate);
        }
    }

    fn predicate(&mut self, predicate: &Predicate) {
        self.predicates = self.predicates.saturating_add(1);
        match predicate {
            Predicate::Bound { ty, bound } => {
                self.type_node(ty);
                self.bound(bound);
            }
            Predicate::ProjectionEquality { projection, ty } => {
                self.type_node(projection);
                self.type_node(ty);
            }
            Predicate::LifetimeOutlives { .. } | Predicate::Unknown { .. } => {}
            Predicate::TypeOutlives { ty, .. } => self.type_node(ty),
            Predicate::ConstEvaluatable { value } => self.constant(value),
        }
    }

    fn bound(&mut self, bound: &Bound) {
        if let Bound::Trait { arguments, .. } = bound {
            if !arguments.is_empty() {
                self.instantiations = self.instantiations.saturating_add(1);
            }
            self.arguments(arguments);
        }
    }

    fn arguments(&mut self, arguments: &[Argument]) {
        for argument in arguments {
            match argument {
                Argument::Lifetime { .. } => {}
                Argument::Type { ty } => self.type_node(ty),
                Argument::Const { value } => self.constant(value),
            }
        }
    }

    fn type_node(&mut self, ty: &Type) {
        self.types = self.types.saturating_add(1);
        match ty {
            Type::Primitive { .. }
            | Type::Parameter { .. }
            | Type::SelfType { .. }
            | Type::Never
            | Type::Unknown { .. } => {}
            Type::Definition { arguments, .. } => {
                if !arguments.is_empty() {
                    self.instantiations = self.instantiations.saturating_add(1);
                }
                self.arguments(arguments);
            }
            Type::Tuple { elements } => {
                for element in elements {
                    self.type_node(element);
                }
            }
            Type::Slice { element }
            | Type::RawPointer { ty: element, .. }
            | Type::Reference { ty: element, .. } => self.type_node(element),
            Type::Array { element, length } => {
                self.type_node(element);
                self.constant(length);
            }
            Type::FunctionPointer { signature } => self.signature(signature),
            Type::TraitObject { bounds, .. } => {
                for bound in bounds {
                    match bound {
                        ObjectBound::Trait { bound } => self.bound(bound),
                        ObjectBound::ProjectionEquality { arguments, ty, .. } => {
                            if !arguments.is_empty() {
                                self.instantiations = self.instantiations.saturating_add(1);
                            }
                            self.arguments(arguments);
                            self.type_node(ty);
                        }
                    }
                }
            }
            Type::ImplTrait {
                bounds, captures, ..
            } => {
                if !captures.is_empty() {
                    self.instantiations = self.instantiations.saturating_add(1);
                }
                self.predicates(bounds);
                self.arguments(captures);
            }
            Type::Projection { arguments, .. } => {
                if !arguments.is_empty() {
                    self.instantiations = self.instantiations.saturating_add(1);
                }
                self.arguments(arguments);
            }
        }
    }

    fn constant(&mut self, value: &Const) {
        if let Const::Unevaluated { arguments, .. } = value {
            if !arguments.is_empty() {
                self.instantiations = self.instantiations.saturating_add(1);
            }
            self.arguments(arguments);
        }
    }
}

#[cfg(test)]
pub(crate) fn decode(bytes: &[u8], limits: &cargo::Limits) -> Result<Document> {
    decode_recorded(bytes, limits).map(|(document, _)| document)
}

/// Decode under a fresh canonical account and return that account, so a test
/// can assert the exceedance record the scan left behind.
#[cfg(test)]
pub(crate) fn decode_recorded(
    bytes: &[u8],
    limits: &cargo::Limits,
) -> Result<(Document, super::Allocation)> {
    let mut allocation = super::Allocation::new(limits);
    let document = decode_accounted(bytes, limits, &mut allocation)?;
    Ok((document, allocation))
}

pub(crate) fn decode_accounted(
    bytes: &[u8],
    limits: &cargo::Limits,
    allocation: &mut super::Allocation,
) -> Result<Document> {
    // The sidecar document size is an accounting target recorded on the
    // canonical account.
    allocation.exceeded("sidecar bytes", limits.json_bytes, bytes.len());
    // The scan's crossings are folded into the account on every exit path, so
    // a document that crosses a target and then fails to scan keeps them.
    let mut exceeded = crate::exceedance::Record::new();
    let scanned = scan(bytes, limits, false, &mut exceeded);
    allocation.absorb_exceedances(&exceeded);
    let usage = scanned?;
    allocation.items(usage.items)?;
    allocation.bytes(usage.bytes)?;
    allocation.captured_document(usage.items);
    let document: Document = serde_json::from_slice(bytes).map_err(|error| Error::Extraction {
        code: "sidecar-schema",
        reason: error.to_string().into_boxed_str(),
    })?;
    if document.schema != SCHEMA {
        return Err(Error::Extraction {
            code: "unknown-sidecar-schema",
            reason: format!("expected {SCHEMA}, found {}", document.schema).into_boxed_str(),
        });
    }
    #[cfg(test)]
    ACCOUNTED_DECODES.with(|count| count.set(count.get().saturating_add(1)));
    Ok(document)
}

/// Decode only the handshakes of sidecar bytes already authenticated against
/// the sealed bundle that fully decoded them; `Capture::into_parts` decodes the
/// complete document if a consumer needs it.
pub(crate) fn decode_handshakes(bytes: &[u8]) -> Result<Document> {
    use serde::de::IgnoredAny;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Shell {
        schema: u32,
        fragments: Vec<Part>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code, reason = "the payload fields are skipped, not read")]
    struct Part {
        handshake: Handshake,
        reachable: IgnoredAny,
        definitions: IgnoredAny,
        implementations: IgnoredAny,
        scalar_predicate_proofs: IgnoredAny,
        #[serde(default)]
        index: IgnoredAny,
    }
    let shell: Shell = serde_json::from_slice(bytes).map_err(|error| Error::Extraction {
        code: "sidecar-schema",
        reason: error.to_string().into_boxed_str(),
    })?;
    if shell.schema != SCHEMA {
        return Err(Error::Extraction {
            code: "unknown-sidecar-schema",
            reason: format!("expected {SCHEMA}, found {}", shell.schema).into_boxed_str(),
        });
    }
    let fragments = shell
        .fragments
        .into_iter()
        .map(|part| Fragment {
            handshake: part.handshake,
            reachable: Vec::new(),
            definitions: Vec::new(),
            implementations: Vec::new(),
            scalar_predicate_proofs: Vec::new(),
            index: Vec::new(),
        })
        .collect();
    Ok(Document {
        schema: shell.schema,
        fragments,
    })
}

#[cfg(test)]
pub(crate) fn reset_accounted_decodes() {
    ACCOUNTED_DECODES.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn accounted_decodes() -> usize {
    ACCOUNTED_DECODES.with(std::cell::Cell::get)
}

#[derive(Clone, Copy)]
enum ArrayClass {
    Other,
    Packages,
    Definitions,
    Edges,
}

#[derive(Clone, Copy)]
enum Frame {
    Object {
        pending: ArrayClass,
        width: usize,
    },
    Array {
        class: ArrayClass,
        elements: usize,
        width: usize,
    },
}

pub(crate) struct Preflight {
    pub(crate) items: usize,
    pub(crate) bytes: usize,
    pub(crate) admission_bytes: usize,
}

/// Reuse the fixed-stack scanner for the closed selected-API allocation sites.
#[cfg(test)]
pub(crate) fn selected_preflight(bytes: &[u8], limits: &cargo::Limits) -> Result<Preflight> {
    preflight(bytes, limits, true)
}

/// The selected-API scan for a canonical transaction: the targets it crosses
/// reach `allocation` on every exit, including a scan that then fails.
#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "stored API replay is driver-only")
)]
pub(crate) fn selected_preflight_accounted(
    bytes: &[u8],
    limits: &cargo::Limits,
    allocation: &super::Allocation,
) -> Result<Preflight> {
    let mut exceeded = crate::exceedance::Record::new();
    let scanned = scan(bytes, limits, true, &mut exceeded);
    allocation.absorb_exceedances(&exceeded);
    let scanned = scanned.map_err(|error| allocation.failure(error))?;
    Ok(Preflight {
        items: scanned.items,
        bytes: scanned.bytes,
        admission_bytes: scanned.admission_bytes,
    })
}

#[cfg(test)]
fn preflight(bytes: &[u8], limits: &cargo::Limits, selected: bool) -> Result<Preflight> {
    let mut exceeded = crate::exceedance::Record::new();
    let scanned = scan(bytes, limits, selected, &mut exceeded)?;
    Ok(Preflight {
        items: scanned.items,
        bytes: scanned.bytes,
        admission_bytes: scanned.admission_bytes,
    })
}

struct Scanned {
    items: usize,
    bytes: usize,
    admission_bytes: usize,
}

/// The fixed-stack scan. Accounting crossings are written to `exceeded` as
/// they happen, so the caller keeps them even when the scan then fails.
fn scan(
    bytes: &[u8],
    limits: &cargo::Limits,
    selected: bool,
    exceeded: &mut crate::exceedance::Record,
) -> Result<Scanned> {
    let mut stack = [Frame::Object {
        pending: ArrayClass::Other,
        width: 0,
    }; 256];
    let mut shape = crate::api::decode::Shape::default();
    let mut depth = 0_usize;
    let mut containers = 0_usize;
    let mut packages = 0_usize;
    let mut definitions = 0_usize;
    let mut edges = 0_usize;
    let mut items = 0_usize;
    let mut strings = 0_usize;
    let mut string_bytes = 0_usize;
    let mut index = 0_usize;
    let mut value_pending = true;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => {
                count_element(
                    &mut stack[..depth],
                    &mut items,
                    &mut packages,
                    &mut definitions,
                    &mut edges,
                    limits,
                    exceeded,
                )?;
                containers = containers.saturating_add(1);
                target(
                    exceeded,
                    "sidecar containers",
                    limits.containers,
                    containers,
                );
                limited("sidecar depth", limits.depth, depth.saturating_add(1))?;
                if depth == stack.len() {
                    return Err(schema("sidecar fixed-depth preflight exceeded"));
                }
                stack[depth] = Frame::Object {
                    pending: ArrayClass::Other,
                    width: crate::api::decode::Shape::widest(),
                };
                depth += 1;
                if selected {
                    shape.depth(depth);
                }
                value_pending = false;
                index += 1;
            }
            b'[' => {
                count_element(
                    &mut stack[..depth],
                    &mut items,
                    &mut packages,
                    &mut definitions,
                    &mut edges,
                    limits,
                    exceeded,
                )?;
                containers = containers.saturating_add(1);
                target(
                    exceeded,
                    "sidecar containers",
                    limits.containers,
                    containers,
                );
                let (class, width) = match stack[..depth].last_mut() {
                    Some(Frame::Object { pending, width }) => {
                        let class = *pending;
                        *pending = ArrayClass::Other;
                        (class, *width)
                    }
                    Some(Frame::Array { .. }) | None => {
                        (ArrayClass::Other, crate::api::decode::Shape::widest())
                    }
                };
                limited("sidecar depth", limits.depth, depth.saturating_add(1))?;
                if depth == stack.len() {
                    return Err(schema("sidecar fixed-depth preflight exceeded"));
                }
                stack[depth] = Frame::Array {
                    class,
                    elements: 0,
                    width,
                };
                depth += 1;
                if selected {
                    shape.depth(depth);
                }
                value_pending = true;
                index += 1;
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Err(schema("unbalanced closing delimiter"));
                }
                depth -= 1;
                if selected
                    && let Frame::Array {
                        width, elements, ..
                    } = stack[depth]
                {
                    shape.array(width, elements);
                }
                value_pending = false;
                index += 1;
            }
            b'"' => {
                let (end, decoded) = string_end(bytes, index)?;
                target(exceeded, "sidecar string", limits.string_bytes, decoded);
                strings = strings.saturating_add(1);
                if selected {
                    shape.string(&bytes[index + 1..end - 1]);
                }
                string_bytes = string_bytes.saturating_add(end.saturating_sub(index + 2));
                target(
                    exceeded,
                    "sidecar aggregate string bytes",
                    limits.json_bytes,
                    string_bytes,
                );
                if let Some(Frame::Object { pending, width }) = stack[..depth].last_mut() {
                    let mut cursor = end;
                    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                        cursor += 1;
                    }
                    if cursor < bytes.len() && bytes[cursor] == b':' {
                        let key = &bytes[index + 1..end - 1];
                        if key.contains(&b'\\') {
                            return Err(schema("escaped JSON object keys are not canonical"));
                        }
                        *pending = array_class(key);
                        if selected {
                            *width = crate::api::decode::Shape::array_width(key, depth);
                            shape.key(key);
                        }
                    } else if value_pending {
                        count_element(
                            &mut stack[..depth],
                            &mut items,
                            &mut packages,
                            &mut definitions,
                            &mut edges,
                            limits,
                            exceeded,
                        )?;
                    }
                } else if value_pending {
                    count_element(
                        &mut stack[..depth],
                        &mut items,
                        &mut packages,
                        &mut definitions,
                        &mut edges,
                        limits,
                        exceeded,
                    )?;
                }
                value_pending = false;
                index = end;
            }
            b',' => {
                value_pending = matches!(stack[..depth].last(), Some(Frame::Array { .. }));
                index += 1;
            }
            b':' => {
                value_pending = true;
                index += 1;
            }
            byte if byte.is_ascii_whitespace() => index += 1,
            _byte => {
                if selected && value_pending {
                    // Stored model numerics are unsigned integers. Reject
                    // noncanonical numeric forms before serde's optional float
                    // scratch path; booleans and null retain their exact forms.
                    let end = bytes[index..]
                        .iter()
                        .position(|byte| {
                            byte.is_ascii_whitespace() || matches!(*byte, b',' | b'}' | b']')
                        })
                        .map_or(bytes.len(), |length| index + length);
                    let value = &bytes[index..end];
                    let unsigned = !value.is_empty()
                        && value.len() <= 20
                        && value.iter().all(u8::is_ascii_digit)
                        && (value.len() == 1 || value[0] != b'0');
                    if !unsigned && !matches!(value, b"true" | b"false" | b"null") {
                        return Err(schema("noncanonical stored API scalar"));
                    }
                }
                if value_pending {
                    count_element(
                        &mut stack[..depth],
                        &mut items,
                        &mut packages,
                        &mut definitions,
                        &mut edges,
                        limits,
                        exceeded,
                    )?;
                    value_pending = false;
                }
                index += 1;
            }
        }
    }
    if depth != 0 {
        return Err(schema("unclosed JSON container"));
    }
    let (bytes, admission_bytes) = if selected {
        shape.bytes(string_bytes)
    } else {
        (string_bytes, string_bytes)
    };
    Ok(Scanned {
        items: items.saturating_add(containers).saturating_add(strings),
        bytes,
        admission_bytes,
    })
}

/// A scanned count crossing an accounting target is recorded with its
/// truthful running total; the scan continues and never refuses on it.
fn target(
    exceeded: &mut crate::exceedance::Record,
    resource: &'static str,
    target: usize,
    actual: usize,
) {
    if actual > target {
        exceeded.exceeded(resource, target as u64, actual as u64, None);
    }
}

fn count_element(
    stack: &mut [Frame],
    items: &mut usize,
    packages: &mut usize,
    definitions: &mut usize,
    edges: &mut usize,
    limits: &cargo::Limits,
    exceeded: &mut crate::exceedance::Record,
) -> Result<()> {
    let Some(Frame::Array {
        class, elements, ..
    }) = stack.last_mut()
    else {
        return Ok(());
    };
    *elements = elements.saturating_add(1);
    *items = items.saturating_add(1);
    target(exceeded, "sidecar items", limits.items, *items);
    match class {
        ArrayClass::Other => {}
        ArrayClass::Packages => {
            *packages = packages.saturating_add(1);
            target(exceeded, "sidecar packages", limits.packages, *packages);
        }
        ArrayClass::Definitions => {
            *definitions = definitions.saturating_add(1);
            target(exceeded, "sidecar definitions", limits.units, *definitions);
        }
        ArrayClass::Edges => {
            *edges = edges.saturating_add(1);
            target(exceeded, "sidecar edges", limits.edges, *edges);
        }
    }
    Ok(())
}

fn array_class(key: &[u8]) -> ArrayClass {
    const DEFINITIONS: &[&[u8]] = &[b"definitions"];
    const EDGES: &[&[u8]] = &[
        b"reachable",
        b"implementations",
        b"aliases",
        b"reexports",
        b"relevance",
        b"associated",
        b"arguments",
        b"captures",
        b"inputs",
        b"bounds",
        b"supertraits",
        b"predicates",
        b"fields",
        b"variants",
        b"parameters",
        b"binders",
        b"artifacts",
        b"path",
        b"from",
        b"to",
        b"display_path",
        b"elements",
        b"features",
        b"dependencies",
        b"target_kinds",
        b"crate_types",
    ];
    if matches!(
        key,
        b"fragments" | b"sysroot" | b"loaded_crates" | b"external_crates"
    ) {
        ArrayClass::Packages
    } else if DEFINITIONS.contains(&key) {
        ArrayClass::Definitions
    } else if EDGES.contains(&key) {
        ArrayClass::Edges
    } else {
        ArrayClass::Other
    }
}

/// Locate the end of one JSON string already held in memory. The decoded
/// length is measured whole and compared with the string target by the caller.
fn string_end(bytes: &[u8], start: usize) -> Result<(usize, usize)> {
    let mut index = start + 1;
    let mut decoded = 0_usize;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => return Ok((index + 1, decoded)),
            b'\\' => {
                index = index.saturating_add(2);
                if index > bytes.len() {
                    return Err(schema("truncated JSON escape"));
                }
                decoded = decoded.saturating_add(1);
            }
            byte if byte < 0x20 => return Err(schema("control byte in JSON string")),
            _byte => {
                index += 1;
                decoded = decoded.saturating_add(1);
            }
        }
    }
    Err(schema("unterminated JSON string"))
}

/// The fixed-stack depth bound guards the scan stack and keeps its terminal
/// `Error::Limit`.
fn limited(resource: &'static str, limit: usize, actual: usize) -> Result<()> {
    if actual <= limit {
        Ok(())
    } else {
        Err(Error::Limit {
            resource,
            limit,
            actual,
        })
    }
}

fn schema(reason: &str) -> Error {
    Error::Extraction {
        code: "sidecar-schema",
        reason: reason.into(),
    }
}

#[cfg(test)]
mod physical_ceilings {
    use super::decode_accounted;
    use crate::cargo::Limits;
    use crate::extract::Allocation;
    use crate::Error;

    /// A document that crosses a configurable target and then fails to scan
    /// (truncation, depth) still leaves the crossing on the original account.
    #[test]
    fn scan_failure_keeps_the_crossing_already_recorded() {
        let mut limits = Limits::standard();
        limits.containers = 1;
        limits.string_bytes = 1;
        for (bytes, axis) in [
            (&br#"[[],["ab","\"#[..], "sidecar containers"),
            (&br#"["abc","\"#[..], "sidecar string"),
            (&br#"[[],[]"#[..], "sidecar containers"),
        ] {
            let mut allocation = Allocation::new(&limits);
            let error = decode_accounted(bytes, &limits, &mut allocation)
                .expect_err("malformed input fails after the crossing");
            assert!(
                matches!(
                    error,
                    Error::Extraction {
                        code: "sidecar-schema",
                        ..
                    }
                ),
                "{error:?}"
            );
            assert!(
                allocation.exceedances().axis(axis).is_some(),
                "`{axis}` survives the failed scan"
            );
            assert_eq!(allocation.usage(), (0, 0));
        }

        limits = Limits::standard();
        limits.containers = 1;
        limits.depth = 2;
        let mut allocation = Allocation::new(&limits);
        let error = decode_accounted(b"[[[]]]", &limits, &mut allocation)
            .expect_err("the depth bound refuses after the crossing");
        assert!(
            matches!(
                error,
                Error::Limit {
                    resource: "sidecar depth",
                    ..
                }
            ),
            "{error:?}"
        );
        assert!(allocation
            .exceedances()
            .axis("sidecar containers")
            .is_some());
    }
}
