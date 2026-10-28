//! Stable semantic identities for classified imported operations.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::api::{identity as api, ledger, model};
use crate::classify::representability::{convert, ownership};

const DEFINITION_DOMAIN: &[u8] = b"rawr.identity.definition/v1";
const EXPORT_DOMAIN: &[u8] = b"rawr.identity.export/v1";
const IMPLEMENTATION_DOMAIN: &[u8] = b"rawr.identity.implementation/v1";
const OPERATION_DOMAIN: &[u8] = b"rawr.identity.operation/v3";
const SIPPER_DOMAIN: &[u8] = b"rawr.identity.sipper/v1";
const VALUE_DOMAIN: &[u8] = b"rawr.identity.value/v1";
const ELEMENT_DOMAIN: &[u8] = b"rawr.identity.element/v1";
const THEME_DOMAIN: &[u8] = b"rawr.identity.theme/v1";
const RENDERER_DOMAIN: &[u8] = b"rawr.identity.renderer/v1";
const FACADE_DOMAIN: &[u8] = b"rawr.identity.facade-value/v1";
const FUTURE_DOMAIN: &[u8] = b"rawr.identity.future/v1";
const DURATION_DOMAIN: &[u8] = b"rawr.identity.duration/v1";
const RANGE_INCLUSIVE_DOMAIN: &[u8] = b"rawr.identity.range-inclusive/v1";
const TEXT_MEMBER_DOMAIN: &[u8] = b"rawr.identity.text-member/v1";

/// Canonical generic extension slot; Phase 2 has no constructor.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Hook {}

/// Stable semantic identity of one Rust definition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Definition([u8; 32]);

/// Stable semantic identity of one public export alias.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Export([u8; 32]);

/// Stable semantic identity of one implementation edge.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Implementation([u8; 32]);

/// One named field in exact constructor grammar order.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Field {
    index: u32,
    name: Box<str>,
}

/// Closed constructor grammar retained from rustc.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Form {
    Unit,
    Tuple { arity: u32 },
    Named { fields: Box<[Field]> },
}

/// Closed semantic selector for every symbolic imported operation.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Selector {
    Declaration,
    StructConstructor(Form),
    StructField(Field),
    EnumVariant {
        index: u32,
        name: Box<str>,
        form: Form,
    },
    Function {
        item: Definition,
    },
    Constant {
        item: Definition,
    },
    Method {
        item: Definition,
        receiver: ownership::Receiver,
    },
    AssociatedFunction {
        item: Definition,
    },
    AssociatedConstant {
        item: Definition,
    },
    Unavailable,
}

impl Selector {
    /// Return the stable canonical selector digest used by host proof fingerprints.
    #[doc(hidden)]
    #[must_use]
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut framed = Framed::new(b"rawr.identity.selector/v1");
        framed.selector(self);
        framed.finish()
    }

    /// Return checked dynamic ownership retained by a selector clone.
    #[doc(hidden)]
    pub fn retained_bytes(&self) -> Option<usize> {
        selector_bytes(self)
    }

    /// Fallibly duplicate this selector after its retained size is charged.
    #[doc(hidden)]
    pub fn try_clone(&self) -> Result<Self, crate::surface::Error> {
        fn field(field: &Field) -> Result<Field, crate::surface::Error> {
            let mut name = String::new();
            name.try_reserve_exact(field.name.len())
                .map_err(|_| crate::surface::Error::Allocation)?;
            name.push_str(&field.name);
            Ok(Field {
                index: field.index,
                name: name.into_boxed_str(),
            })
        }

        fn form(form: &Form) -> Result<Form, crate::surface::Error> {
            Ok(match form {
                Form::Unit => Form::Unit,
                Form::Tuple { arity } => Form::Tuple { arity: *arity },
                Form::Named { fields } => {
                    let mut cloned = Vec::new();
                    cloned
                        .try_reserve_exact(fields.len())
                        .map_err(|_| crate::surface::Error::Allocation)?;
                    for source in fields {
                        cloned.push(field(source)?);
                    }
                    Form::Named {
                        fields: cloned.into_boxed_slice(),
                    }
                }
            })
        }

        Ok(match self {
            Self::Declaration => Self::Declaration,
            Self::StructConstructor(source) => Self::StructConstructor(form(source)?),
            Self::StructField(source) => Self::StructField(field(source)?),
            Self::EnumVariant {
                index,
                name,
                form: source,
            } => {
                let mut cloned = String::new();
                cloned
                    .try_reserve_exact(name.len())
                    .map_err(|_| crate::surface::Error::Allocation)?;
                cloned.push_str(name);
                Self::EnumVariant {
                    index: *index,
                    name: cloned.into_boxed_str(),
                    form: form(source)?,
                }
            }
            Self::Function { item } => Self::Function { item: *item },
            Self::Constant { item } => Self::Constant { item: *item },
            Self::Method { item, receiver } => Self::Method {
                item: *item,
                receiver: *receiver,
            },
            Self::AssociatedFunction { item } => Self::AssociatedFunction { item: *item },
            Self::AssociatedConstant { item } => Self::AssociatedConstant { item: *item },
            Self::Unavailable => Self::Unavailable,
        })
    }
}

impl std::hash::Hash for Selector {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.fingerprint(), state);
    }
}

/// One operation identity independent of graph order and diagnostic position.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Identity {
    definition: Option<Definition>,
    export: Export,
    implementation: Option<Implementation>,
    selector: Selector,
    types: Box<[convert::Type]>,
    constants: Box<[convert::Constant]>,
    binders: Box<[super::instance::Binder]>,
    callable: Option<super::callable::Class>,
    functions: Box<[super::instance::FunctionItem]>,
    mappers: Box<[super::instance::EventMapper]>,
    predicates: Box<[Implementation]>,
    /// The script functions that fill callable parameters (D594).
    #[serde(default)]
    slots: Box<[super::instance::Slot]>,
    /// The script's impls that cross with the instance's script types
    /// (D594).
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    impls: Box<[super::instance::ScriptImpl]>,
    hook: Option<Hook>,
    semantic: [u8; 32],
}

/// A digest of `impls`, as an instance's identity frames them.
fn impls_digest(impls: &[super::instance::ScriptImpl]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"rawr.instance.impls/v1");
    digest.update(serde_json::to_vec(impls).unwrap_or_default());
    digest.finalize().into()
}

impl std::hash::Hash for Identity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.semantic, state);
    }
}

impl Definition {
    pub(crate) fn of(definition: &api::Definition) -> Self {
        let mut framed = Framed::new(DEFINITION_DOMAIN);
        framed.definition(definition);
        Self(framed.finish())
    }

    /// The method `method` of the trait impl `implementation` for `&str`,
    /// called on Rawr's string (`surface::text`). A trait's provided method
    /// is one definition every impl shares, so the impl tells this one from
    /// the same method of another impl at the same type arguments.
    pub(crate) fn text_member(implementation: &api::Definition, method: &api::Definition) -> Self {
        let mut framed = Framed::new(TEXT_MEMBER_DOMAIN);
        framed.definition(implementation);
        framed.definition(method);
        Self(framed.finish())
    }

    /// The type a Rust `impl Sipper<Output, Progress>` crosses as. Sipper is
    /// a trait, so no Rust definition names it; iced's facade declares it as
    /// `task::Sipper<Output, Progress>`.
    #[must_use]
    pub fn sipper() -> Self {
        Self(Framed::new(SIPPER_DOMAIN).finish())
    }

    /// The type a Rust future of `T` crosses as: iced's `Task<T>`. Any
    /// crate may return one, so no crate's own types declare it.
    #[must_use]
    pub fn future() -> Self {
        Self(Framed::new(FUTURE_DOMAIN).finish())
    }

    /// Rust's `std::time::Duration`, which crosses by value as iced's
    /// `time::Duration`. It lives in `core`, which no crate's facade declares.
    #[must_use]
    pub fn duration() -> Self {
        Self(Framed::new(DURATION_DOMAIN).finish())
    }

    /// Rust's `std::ops::RangeInclusive<T>` of a float or of a type
    /// parameter, at its one argument `T`: Rawr's `RangeInclusive<f64>` or
    /// `RangeInclusive<i64>` as `T` is a float or an integer, as a crate's
    /// `slider(range: RangeInclusive<T>, value: T, ..)` takes either. It
    /// lives in `core`, which no crate's facade declares.
    #[must_use]
    pub fn range_inclusive() -> Self {
        Self(Framed::new(RANGE_INCLUSIVE_DOMAIN).finish())
    }

    /// A script value where a generic item is instantiated at a script type
    /// (D513.2): the runner builds the item at `rawr::Value`, and each value
    /// that comes back is checked against the script type it stands for.
    #[must_use]
    pub fn value() -> Self {
        Self(Framed::new(VALUE_DOMAIN).finish())
    }

    /// An iced element a view value takes where an imported view type has
    /// a widget parameter, as pane_grid's `Content<'a, Message, T, W>`
    /// does: the facade widget the script passes, lowered (D558 tier 2).
    #[must_use]
    pub fn element() -> Self {
        Self(Framed::new(ELEMENT_DOMAIN).finish())
    }

    /// iced's `Theme`, which the iced facade declares, as an imported view
    /// type's `Theme` argument.
    #[must_use]
    pub fn theme() -> Self {
        Self(Framed::new(THEME_DOMAIN).finish())
    }

    /// iced's `Renderer`, which the iced facade declares, as an imported
    /// view type's `Renderer` argument.
    #[must_use]
    pub fn renderer() -> Self {
        Self(Framed::new(RENDERER_DOMAIN).finish())
    }

    /// A value of the facade type at `path`, as `iced::Length`, which a
    /// script value holds as the Rust value itself: an imported view type's
    /// argument the facade declares (D558 tier 2).
    #[must_use]
    pub fn facade(path: &str) -> Self {
        let mut framed = Framed::new(FACADE_DOMAIN);
        framed.strings([path].into_iter());
        Self(framed.finish())
    }

    /// Whether these bytes name a type the iced facade declares for a Rust
    /// value no imported crate declares: a future, a sipper or a duration.
    #[must_use]
    pub fn is_projected(bytes: &[u8; 32]) -> bool {
        *bytes == Self::future().0
            || *bytes == Self::sipper().0
            || *bytes == Self::duration().0
            || *bytes == Self::range_inclusive().0
            || *bytes == Self::value().0
            || *bytes == Self::element().0
            || *bytes == Self::theme().0
            || *bytes == Self::renderer().0
    }

    /// Reconstitute one semantic definition digest carried by an authenticated
    /// installed imported facade.
    #[doc(hidden)]
    #[must_use]
    pub const fn authenticated(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the semantic digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Export {
    pub(crate) fn imported(export: &ledger::Export) -> Self {
        let mut framed = Framed::new(EXPORT_DOMAIN);
        framed.definition(export.target());
        framed.export_key(export.key());
        framed.count(export.aliases().len());
        for alias in export.aliases() {
            framed.strings(alias.from());
            framed.strings(alias.to());
        }
        framed.count(export.reexports().len());
        for reexport in export.reexports() {
            framed.byte(reexport_kind(reexport.kind()));
            match reexport.item() {
                Some(item) => {
                    framed.byte(1);
                    framed.unit(item.unit());
                    framed.bytes(item.def_path_hash().as_bytes());
                }
                None => framed.byte(0),
            }
        }
        Self(framed.finish())
    }

    pub(crate) fn unresolved(export: &api::Export) -> Self {
        let mut framed = Framed::new(EXPORT_DOMAIN);
        framed.byte(0);
        framed.export_key(export);
        Self(framed.finish())
    }

    /// Borrow the semantic digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Implementation {
    /// The implementation of the trait `trait_id` that the opaque `impl
    /// Trait` type `opaque` a function gives has, which no Rust item writes.
    pub(crate) fn opaque(
        opaque: &crate::api::identity::Definition,
        trait_id: &crate::api::identity::Definition,
    ) -> Self {
        let mut framed = Framed::new(b"rawr.identity.opaque-implementation/v1");
        framed.definition(opaque);
        framed.definition(trait_id);
        Self(framed.finish())
    }

    pub(crate) fn of(
        implementation: &model::Implementation,
    ) -> Result<Self, crate::classify::Error> {
        let mut framed = Framed::new(IMPLEMENTATION_DOMAIN);
        framed.definition(implementation.id());
        framed.bytes(implementation.key().as_bytes());
        framed.trait_ref(implementation.trait_ref());
        framed.ty(implementation.self_type());
        Ok(Self(framed.finish()))
    }

    /// Borrow the semantic digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub(crate) fn converted_type_fingerprint(ty: &convert::Type) -> [u8; 32] {
    let mut framed = Framed::new(b"rawr.identity.converted-type/v1");
    framed.converted_type(ty);
    framed.finish()
}

impl Field {
    pub(crate) fn new(index: u32, name: Box<str>) -> Self {
        Self { index, name }
    }

    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Form {
    pub(crate) fn from_fields(
        form: model::FieldForm,
        fields: impl ExactSizeIterator<Item = Field>,
    ) -> Result<Self, crate::classify::Error> {
        let count = fields.len();
        let arity = u32::try_from(count).map_err(|_| crate::classify::Error::IndexOverflow)?;
        Ok(match form {
            model::FieldForm::Unit => Self::Unit,
            model::FieldForm::Tuple => Self::Tuple { arity },
            model::FieldForm::Record => {
                let mut retained = Vec::new();
                retained
                    .try_reserve_exact(count)
                    .map_err(|_| crate::classify::Error::Allocation)?;
                for field in fields {
                    retained.push(field);
                }
                Self::Named {
                    fields: retained.into_boxed_slice(),
                }
            }
        })
    }
}

impl Identity {
    pub(crate) fn checked_retained_bytes(&self) -> Option<usize> {
        self.checked_retained_bytes_with(&self.selector)
    }

    /// Independently recurse retained semantic identity ownership for driver audits.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub fn driver_retained_bytes(&self) -> Option<usize> {
        self.checked_retained_bytes()
    }

    pub(crate) fn checked_retained_bytes_with(&self, selector: &Selector) -> Option<usize> {
        let types = self
            .types
            .iter()
            .try_fold(0_usize, |bytes, ty| bytes.checked_add(type_bytes(ty)?))?;
        let constants = self.constants.iter().try_fold(0_usize, |bytes, value| {
            bytes
                .checked_add(value.ty().len())?
                .checked_add(value.value().len())
        })?;
        std::mem::size_of::<Self>()
            .checked_add(selector_bytes(selector)?)?
            .checked_add(types)?
            .checked_add(constants)?
            .checked_add(
                self.binders
                    .len()
                    .checked_mul(std::mem::size_of::<super::instance::Binder>())?,
            )
            .and_then(|bytes| {
                self.functions.iter().try_fold(bytes, |bytes, function| {
                    bytes
                        .checked_add(std::mem::size_of::<super::instance::FunctionItem>())?
                        .checked_add(function.identity().checked_retained_bytes()?)
                })
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    self.mappers
                        .len()
                        .checked_mul(std::mem::size_of::<super::instance::EventMapper>())?,
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    self.predicates
                        .len()
                        .checked_mul(std::mem::size_of::<Implementation>())?,
                )
            })
    }

    pub(crate) fn checked_retained_bytes_with_instance(
        &self,
        selector: &Selector,
        instance: &super::instance::Arguments,
    ) -> Option<usize> {
        let types = instance
            .types()
            .iter()
            .try_fold(0_usize, |bytes, ty| bytes.checked_add(type_bytes(ty)?))?;
        let constants = instance
            .constants()
            .iter()
            .try_fold(0_usize, |bytes, value| {
                bytes
                    .checked_add(value.ty().len())?
                    .checked_add(value.value().len())
            })?;
        std::mem::size_of::<Self>()
            .checked_add(selector_bytes(selector)?)?
            .checked_add(types)?
            .checked_add(constants)?
            .checked_add(
                instance
                    .binders()
                    .len()
                    .checked_mul(std::mem::size_of::<super::instance::Binder>())?,
            )
            .and_then(|bytes| {
                instance
                    .function_items()
                    .iter()
                    .try_fold(bytes, |bytes, function| {
                        bytes
                            .checked_add(std::mem::size_of::<super::instance::FunctionItem>())?
                            .checked_add(function.identity().checked_retained_bytes()?)
                    })
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    instance
                        .event_mappers()
                        .len()
                        .checked_mul(std::mem::size_of::<super::instance::EventMapper>())?,
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    instance
                        .predicate_proofs()
                        .len()
                        .checked_mul(std::mem::size_of::<Implementation>())?,
                )
            })
    }

    pub(crate) fn same_binding(&self, other: &Self) -> bool {
        self.definition == other.definition
            && self.export == other.export
            && self.implementation == other.implementation
    }

    pub(crate) fn imported(
        export: &ledger::Export,
        implementation: Option<&model::Implementation>,
        selector: Selector,
    ) -> Result<Self, crate::classify::Error> {
        let definition = Definition::of(export.target());
        let export = Export::imported(export);
        let implementation = implementation.map(Implementation::of).transpose()?;
        Ok(Self::seal(
            Some(definition),
            export,
            implementation,
            selector,
        ))
    }

    pub(crate) fn unavailable(export: &api::Export) -> Result<Self, crate::classify::Error> {
        Ok(Self::seal(
            None,
            Export::unresolved(export),
            None,
            Selector::Unavailable,
        ))
    }

    pub(crate) fn selected(&self, selector: Selector) -> Self {
        Self::seal(self.definition, self.export, self.implementation, selector)
    }

    pub(crate) fn instantiated(
        &self,
        selector: Selector,
        instance: super::instance::Arguments,
    ) -> Self {
        let super::instance::Parts {
            types,
            constants,
            binders,
            callable,
            functions,
            mappers,
            predicates,
            slots,
            impls,
        } = instance.into_parts();
        let mut sealed = Self::seal_instance(
            self.definition,
            self.export,
            self.implementation,
            selector,
            types,
            constants,
            binders,
            callable,
            functions,
            mappers,
            predicates,
            slots,
        );
        // An instance without impls frames as it did before them.
        if !impls.is_empty() {
            let mut framed = Framed::new(OPERATION_DOMAIN);
            framed.bytes(&sealed.semantic);
            framed.byte(3);
            framed.bytes(&impls_digest(&impls));
            sealed.semantic = framed.finish();
            sealed.impls = impls;
        }
        sealed
    }

    fn seal(
        definition: Option<Definition>,
        export: Export,
        implementation: Option<Implementation>,
        selector: Selector,
    ) -> Self {
        let types = Box::default();
        let constants = Box::default();
        let binders = Box::default();
        let callable = None;
        let functions = Box::default();
        let mappers = Box::default();
        let predicates = Box::default();
        Self::seal_instance(
            definition,
            export,
            implementation,
            selector,
            types,
            constants,
            binders,
            callable,
            functions,
            mappers,
            predicates,
            Box::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn seal_instance(
        definition: Option<Definition>,
        export: Export,
        implementation: Option<Implementation>,
        selector: Selector,
        types: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        binders: Box<[super::instance::Binder]>,
        callable: Option<super::callable::Class>,
        functions: Box<[super::instance::FunctionItem]>,
        mappers: Box<[super::instance::EventMapper]>,
        predicates: Box<[Implementation]>,
        slots: Box<[super::instance::Slot]>,
    ) -> Self {
        let hook = None;
        let mut framed = Framed::new(OPERATION_DOMAIN);
        match definition {
            Some(definition) => {
                framed.byte(1);
                framed.bytes(definition.as_bytes());
            }
            None => framed.byte(0),
        }
        framed.bytes(export.as_bytes());
        match implementation {
            Some(implementation) => {
                framed.byte(1);
                framed.bytes(implementation.as_bytes());
            }
            None => framed.byte(0),
        }
        framed.selector(&selector);
        framed.count(types.len());
        for ty in &types {
            framed.converted_type(ty);
        }
        framed.count(constants.len());
        for constant in &constants {
            framed.converted_constant(constant);
        }
        framed.count(binders.len());
        for binder in &binders {
            framed.surface_binder(*binder);
        }
        match callable {
            Some(class) => {
                framed.byte(1);
                framed.callable(class);
            }
            None => framed.byte(0),
        }
        framed.count(functions.len());
        for function in &functions {
            framed.0.update(function.position().to_le_bytes());
            framed.bytes(function.identity().as_bytes());
            let (start, end) = function.byte_range();
            framed.0.update(start.to_le_bytes());
            framed.0.update(end.to_le_bytes());
        }
        framed.count(mappers.len());
        for mapper in &mappers {
            framed.0.update(mapper.position().to_le_bytes());
            framed.bytes(mapper.seal());
            let (start, end) = mapper.byte_range();
            framed.0.update(start.to_le_bytes());
            framed.0.update(end.to_le_bytes());
            match mapper.projection() {
                Some(projection) => {
                    framed.byte(1);
                    framed.bytes(projection.selector());
                }
                None => framed.byte(0),
            }
        }
        framed.count(predicates.len());
        for predicate in &predicates {
            framed.bytes(predicate.as_bytes());
        }
        // An instance without slots frames as it did before them.
        if !slots.is_empty() {
            framed.byte(2);
            framed.count(slots.len());
            for slot in &slots {
                framed.0.update(slot.position().to_le_bytes());
                framed.bytes(slot.key());
            }
        }
        framed.byte(0);
        Self {
            definition,
            export,
            implementation,
            selector,
            types,
            constants,
            binders,
            callable,
            functions,
            mappers,
            predicates,
            slots,
            impls: Box::default(),
            hook,
            semantic: framed.finish(),
        }
    }

    /// The script's impls that cross with the instance's script types
    /// (D594).
    #[must_use]
    pub fn impls(&self) -> &[super::instance::ScriptImpl] {
        &self.impls
    }

    /// The script functions that fill callable parameters (D594).
    #[must_use]
    pub fn slots(&self) -> &[super::instance::Slot] {
        &self.slots
    }

    /// Return the canonical definition shared by all of its export aliases.
    #[must_use]
    pub const fn definition(&self) -> Option<Definition> {
        self.definition
    }

    /// Return the distinct public-export identity.
    #[must_use]
    pub const fn export(&self) -> Export {
        self.export
    }

    /// Return the exact implementation identity for an associated operation.
    #[must_use]
    pub const fn implementation(&self) -> Option<Implementation> {
        self.implementation
    }

    /// Return the closed semantic operation selector.
    #[must_use]
    pub const fn selector(&self) -> &Selector {
        &self.selector
    }

    /// Borrow the stable operation digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.semantic
    }

    /// Return demanded concrete type substitutions; empty in Phase 2.
    #[must_use]
    pub const fn types(&self) -> &[convert::Type] {
        &self.types
    }

    /// Return demanded concrete const substitutions; empty in Phase 2.
    #[must_use]
    pub const fn constants(&self) -> &[convert::Constant] {
        &self.constants
    }

    /// Return normalized late-bound callable inputs.
    #[must_use]
    pub const fn binders(&self) -> &[super::instance::Binder] {
        &self.binders
    }

    /// Return the callable class; absent for Phase 2 declarations.
    #[must_use]
    pub const fn callable(&self) -> Option<super::callable::Class> {
        self.callable
    }

    #[must_use]
    pub const fn function_items(&self) -> &[super::instance::FunctionItem] {
        &self.functions
    }

    #[must_use]
    pub const fn event_mappers(&self) -> &[super::instance::EventMapper] {
        &self.mappers
    }

    #[must_use]
    pub const fn predicate_proofs(&self) -> &[Implementation] {
        &self.predicates
    }

    /// Return the generic profile hook; always absent in Phase 2.
    #[must_use]
    pub const fn hook(&self) -> Option<&Hook> {
        self.hook.as_ref()
    }
}

fn selector_bytes(selector: &Selector) -> Option<usize> {
    match selector {
        Selector::StructConstructor(form) => form_bytes(form),
        Selector::StructField(field) => Some(field.name.len()),
        Selector::EnumVariant { name, form, .. } => name.len().checked_add(form_bytes(form)?),
        Selector::Declaration
        | Selector::Function { .. }
        | Selector::Constant { .. }
        | Selector::Method { .. }
        | Selector::AssociatedFunction { .. }
        | Selector::AssociatedConstant { .. }
        | Selector::Unavailable => Some(0),
    }
}

fn form_bytes(form: &Form) -> Option<usize> {
    match form {
        Form::Named { fields } => fields.iter().try_fold(0_usize, |bytes, field| {
            bytes
                .checked_add(std::mem::size_of::<Field>())?
                .checked_add(field.name.len())
        }),
        Form::Unit | Form::Tuple { .. } => Some(0),
    }
}

fn type_bytes(ty: &convert::Type) -> Option<usize> {
    match ty {
        convert::Type::Nominal { arguments, .. } => {
            arguments.iter().try_fold(0_usize, |bytes, argument| {
                bytes
                    .checked_add(std::mem::size_of::<convert::Type>())?
                    .checked_add(type_bytes(argument)?)
            })
        }
        convert::Type::FunctionItem { input, output } => type_bytes(input)?
            .checked_add(type_bytes(output)?)
            .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<convert::Type>())),
        convert::Type::ProfileMessage { payload, .. } => type_bytes(payload)?
            .checked_add(std::mem::size_of::<convert::Type>())
            .and_then(|bytes| bytes.checked_add(16)),
        convert::Type::Option(element)
        | convert::Type::Borrowed { ty: element, .. }
        | convert::Type::Range(element)
        | convert::Type::Sequence { element, .. } => {
            type_bytes(element)?.checked_add(std::mem::size_of::<convert::Type>())
        }
        convert::Type::Result(ok, err) => type_bytes(ok)?
            .checked_add(type_bytes(err)?)?
            .checked_add(std::mem::size_of::<convert::Type>().checked_mul(2)?),
        convert::Type::Tuple(members) => members.iter().try_fold(0_usize, |bytes, member| {
            bytes
                .checked_add(std::mem::size_of::<convert::Type>())?
                .checked_add(type_bytes(member)?)
        }),
        convert::Type::Script { path, arguments } => {
            arguments.iter().try_fold(path.len(), |bytes, argument| {
                bytes
                    .checked_add(std::mem::size_of::<convert::Type>())?
                    .checked_add(type_bytes(argument)?)
            })
        }
        convert::Type::Inferred => Some(0),
        convert::Type::Unit | convert::Type::Builtin(_) | convert::Type::Parameter(_) => Some(0),
    }
}

struct Framed(Sha256);

impl Framed {
    fn new(domain: &[u8]) -> Self {
        let mut digest = Sha256::new();
        digest.update(domain);
        Self(digest)
    }

    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }

    fn byte(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn count(&mut self, value: usize) {
        self.0.update((value as u64).to_le_bytes());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.update((value.len() as u64).to_le_bytes());
        self.0.update(value);
    }

    fn strings<'a>(&mut self, values: impl ExactSizeIterator<Item = &'a str>) {
        self.count(values.len());
        for value in values {
            self.bytes(value.as_bytes());
        }
    }

    fn definition(&mut self, definition: &api::Definition) {
        self.unit(definition.unit());
        self.bytes(definition.def_path_hash().as_bytes());
        self.byte(namespace(definition.namespace()));
    }

    fn unit(&mut self, unit: &api::Unit) {
        match unit.authority() {
            api::Authority::Cargo(package) => {
                self.byte(0);
                self.bytes(package.cargo_id().as_bytes());
                self.bytes(package.name().as_bytes());
                self.bytes(package.version().as_bytes());
                self.bytes(package.source().as_bytes());
            }
            api::Authority::Sysroot(sysroot) => {
                self.byte(1);
                self.bytes(sysroot.compiler_commit().as_bytes());
                self.bytes(sysroot.selected_target().as_bytes());
                self.bytes(sysroot.crate_name().as_bytes());
                self.bytes(sysroot.stable_crate_id().as_bytes());
            }
        }
        self.bytes(unit.target().as_bytes());
        self.bytes(unit.target_kind().as_bytes());
        self.bytes(unit.crate_name().as_bytes());
        self.byte(match unit.compilation() {
            api::Compilation::Host => 0,
            api::Compilation::Target => 1,
        });
        self.strings(unit.features());
        self.bytes(unit.stable_crate_id().as_bytes());
    }

    fn export_key(&mut self, export: &api::Export) {
        self.bytes(export.root_alias().as_bytes());
        self.strings(export.path());
        self.byte(namespace(export.namespace()));
    }

    fn field(&mut self, field: &Field) {
        self.0.update(field.index.to_le_bytes());
        self.bytes(field.name.as_bytes());
    }

    fn form(&mut self, form: &Form) {
        match form {
            Form::Unit => self.byte(0),
            Form::Tuple { arity } => {
                self.byte(1);
                self.0.update(arity.to_le_bytes());
            }
            Form::Named { fields } => {
                self.byte(2);
                self.count(fields.len());
                for field in fields {
                    self.field(field);
                }
            }
        }
    }

    fn selector(&mut self, selector: &Selector) {
        match selector {
            Selector::Declaration => self.byte(0),
            Selector::StructConstructor(form) => {
                self.byte(1);
                self.form(form);
            }
            Selector::StructField(field) => {
                self.byte(2);
                self.field(field);
            }
            Selector::EnumVariant { index, name, form } => {
                self.byte(3);
                self.0.update(index.to_le_bytes());
                self.bytes(name.as_bytes());
                self.form(form);
            }
            Selector::Function { item } => {
                self.byte(4);
                self.bytes(item.as_bytes());
            }
            Selector::Constant { item } => {
                self.byte(5);
                self.bytes(item.as_bytes());
            }
            Selector::Method { item, receiver } => {
                self.byte(6);
                self.bytes(item.as_bytes());
                self.byte(match receiver {
                    ownership::Receiver::Owned => 0,
                    ownership::Receiver::Shared => 1,
                    ownership::Receiver::Mutable => 2,
                });
            }
            Selector::AssociatedFunction { item } => {
                self.byte(7);
                self.bytes(item.as_bytes());
            }
            Selector::AssociatedConstant { item } => {
                self.byte(8);
                self.bytes(item.as_bytes());
            }
            Selector::Unavailable => self.byte(9),
        }
    }

    fn trait_ref(&mut self, trait_ref: &model::Trait) {
        match trait_ref {
            model::Trait::Inherent => self.byte(0),
            model::Trait::Trait { id, arguments } => {
                self.byte(1);
                self.definition(id);
                self.arguments(arguments);
            }
        }
    }

    fn arguments(&mut self, arguments: &[model::Argument]) {
        self.count(arguments.len());
        for argument in arguments {
            self.argument(argument);
        }
    }

    fn argument(&mut self, argument: &model::Argument) {
        match argument {
            model::Argument::Lifetime(lifetime) => {
                self.byte(0);
                self.lifetime(lifetime);
            }
            model::Argument::Type(ty) => {
                self.byte(1);
                self.ty(ty);
            }
            model::Argument::Const(value) => {
                self.byte(2);
                self.constant(value);
            }
        }
    }

    fn ty(&mut self, ty: &model::Type) {
        match ty {
            model::Type::Primitive(name) => {
                self.byte(0);
                self.bytes(name.as_bytes());
            }
            model::Type::Definition { id, arguments } => {
                self.byte(1);
                self.definition(id);
                self.arguments(arguments);
            }
            model::Type::Parameter { owner, index } => {
                self.byte(2);
                self.definition(owner);
                self.0.update(index.to_le_bytes());
            }
            model::Type::SelfType(owner) => {
                self.byte(3);
                self.definition(owner);
            }
            model::Type::Tuple(members) => {
                self.byte(4);
                self.count(members.len());
                for member in members {
                    self.ty(member);
                }
            }
            model::Type::Slice(element) => {
                self.byte(5);
                self.ty(element);
            }
            model::Type::Array { element, length } => {
                self.byte(6);
                self.ty(element);
                self.constant(length);
            }
            model::Type::Reference {
                lifetime,
                mutability,
                ty,
            } => {
                self.byte(7);
                self.lifetime(lifetime);
                self.mutability(*mutability);
                self.ty(ty);
            }
            model::Type::RawPointer { mutability, ty } => {
                self.byte(8);
                self.mutability(*mutability);
                self.ty(ty);
            }
            model::Type::FunctionPointer(signature) => {
                self.byte(9);
                self.signature(signature);
            }
            model::Type::TraitObject { bounds, lifetime } => {
                self.byte(10);
                self.count(bounds.len());
                for bound in bounds {
                    self.object_bound(bound);
                }
                self.lifetime(lifetime);
            }
            model::Type::ImplTrait {
                id,
                bounds,
                captures,
            } => {
                self.byte(11);
                self.definition(id);
                self.count(bounds.len());
                for predicate in bounds {
                    self.predicate(predicate);
                }
                self.arguments(captures);
            }
            model::Type::Projection {
                trait_id,
                associated,
                arguments,
            } => {
                self.byte(12);
                self.definition(trait_id);
                self.definition(associated);
                self.arguments(arguments);
            }
            model::Type::Never => self.byte(13),
        }
    }

    fn signature(&mut self, signature: &model::Signature) {
        self.byte(match signature.safety() {
            model::Safety::Safe => 0,
            model::Safety::Unsafe => 1,
        });
        self.byte(u8::from(signature.const_function()));
        self.byte(u8::from(signature.async_function()));
        self.bytes(signature.abi().as_bytes());
        self.byte(u8::from(signature.variadic()));
        self.count(signature.binders().len());
        for binder in signature.binders() {
            self.binder(binder);
        }
        self.receiver(signature.receiver());
        self.count(signature.inputs().len());
        for input in signature.inputs() {
            self.ty(input);
        }
        self.ty(signature.output());
    }

    fn receiver(&mut self, receiver: &model::Receiver) {
        match receiver {
            model::Receiver::None => self.byte(0),
            model::Receiver::Value { ty } => {
                self.byte(1);
                self.ty(ty);
            }
            model::Receiver::Reference {
                lifetime,
                mutability,
                ty,
            } => {
                self.byte(2);
                self.lifetime(lifetime);
                self.mutability(*mutability);
                self.ty(ty);
            }
            model::Receiver::Explicit { ty } => {
                self.byte(3);
                self.ty(ty);
            }
        }
    }

    fn lifetime(&mut self, lifetime: &model::Lifetime) {
        match lifetime {
            model::Lifetime::Static => self.byte(0),
            model::Lifetime::Early { owner, index } => {
                self.byte(1);
                self.definition(owner);
                self.0.update(index.to_le_bytes());
            }
            model::Lifetime::Late { binder, index } => {
                self.byte(2);
                self.binder(binder);
                self.0.update(index.to_le_bytes());
            }
            model::Lifetime::Erased => self.byte(3),
        }
    }

    fn binder(&mut self, binder: &model::Binder) {
        self.definition(binder.owner());
        self.0.update(binder.depth().to_le_bytes());
        self.0.update(binder.index().to_le_bytes());
        self.bytes(binder.name().as_bytes());
    }

    fn constant(&mut self, value: &model::Const) {
        match value {
            model::Const::Literal { ty, value } => {
                self.byte(0);
                self.bytes(ty.as_bytes());
                self.bytes(value.as_bytes());
            }
            model::Const::Parameter { owner, index } => {
                self.byte(1);
                self.definition(owner);
                self.0.update(index.to_le_bytes());
            }
            model::Const::Unevaluated {
                definition,
                arguments,
            } => {
                self.byte(2);
                self.definition(definition);
                self.arguments(arguments);
            }
        }
    }

    fn predicate(&mut self, predicate: &model::Predicate) {
        match predicate {
            model::Predicate::Bound { ty, bound } => {
                self.byte(0);
                self.ty(ty);
                self.bound(bound);
            }
            model::Predicate::ProjectionEquality { projection, ty } => {
                self.byte(1);
                self.ty(projection);
                self.ty(ty);
            }
            model::Predicate::LifetimeOutlives { longer, shorter } => {
                self.byte(2);
                self.lifetime(longer);
                self.lifetime(shorter);
            }
            model::Predicate::TypeOutlives { ty, lifetime } => {
                self.byte(3);
                self.ty(ty);
                self.lifetime(lifetime);
            }
            model::Predicate::ConstEvaluatable(value) => {
                self.byte(4);
                self.constant(value);
            }
        }
    }

    fn bound(&mut self, bound: &model::Bound) {
        match bound {
            model::Bound::Trait {
                trait_id,
                arguments,
                polarity,
                binders,
            } => {
                self.byte(0);
                self.definition(trait_id);
                self.arguments(arguments);
                self.byte(match polarity {
                    model::Polarity::Positive => 0,
                    model::Polarity::Negative => 1,
                });
                self.count(binders.len());
                for binder in binders {
                    self.binder(binder);
                }
            }
            model::Bound::MaybeSized => self.byte(1),
        }
    }

    fn object_bound(&mut self, bound: &model::ObjectBound) {
        match bound {
            model::ObjectBound::Trait(bound) => {
                self.byte(0);
                self.bound(bound);
            }
            model::ObjectBound::ProjectionEquality {
                associated,
                arguments,
                ty,
            } => {
                self.byte(1);
                self.definition(associated);
                self.arguments(arguments);
                self.ty(ty);
            }
        }
    }

    fn mutability(&mut self, mutability: model::Mutability) {
        self.byte(match mutability {
            model::Mutability::Shared => 0,
            model::Mutability::Mutable => 1,
        });
    }

    fn converted_type(&mut self, ty: &convert::Type) {
        match ty {
            convert::Type::Inferred => self.byte(14),
            convert::Type::Unit => self.byte(0),
            convert::Type::Builtin(builtin) => {
                self.byte(1);
                self.byte(match builtin {
                    convert::Builtin::Bool => 0,
                    convert::Builtin::Char => 1,
                    convert::Builtin::F64 => 2,
                    convert::Builtin::I64 => 3,
                    convert::Builtin::String => 4,
                    convert::Builtin::Str => 17,
                    convert::Builtin::StaticStr => 19,
                    convert::Builtin::Text => 18,
                    convert::Builtin::InclusiveRange => 5,
                    convert::Builtin::I8 => 6,
                    convert::Builtin::I16 => 7,
                    convert::Builtin::I32 => 8,
                    convert::Builtin::Isize => 9,
                    convert::Builtin::U8 => 10,
                    convert::Builtin::U16 => 11,
                    convert::Builtin::U32 => 12,
                    convert::Builtin::U64 => 13,
                    convert::Builtin::Usize => 14,
                    convert::Builtin::F32 => 15,
                });
            }
            convert::Type::Nominal {
                definition,
                arguments,
            } => {
                self.byte(2);
                self.bytes(definition.as_bytes());
                self.count(arguments.len());
                for argument in arguments {
                    self.converted_type(argument);
                }
            }
            convert::Type::FunctionItem { input, output } => {
                self.byte(4);
                self.converted_type(input);
                self.converted_type(output);
            }
            convert::Type::ProfileMessage { mapper, payload } => {
                self.byte(5);
                self.bytes(mapper);
                self.converted_type(payload);
            }
            convert::Type::Option(element) => {
                self.byte(6);
                self.converted_type(element);
            }
            convert::Type::Borrowed { mutable, ty } => {
                self.byte(if *mutable { 12 } else { 11 });
                self.converted_type(ty);
            }
            convert::Type::Range(element) => {
                self.byte(10);
                self.converted_type(element);
            }
            convert::Type::Result(ok, err) => {
                self.byte(7);
                self.converted_type(ok);
                self.converted_type(err);
            }
            convert::Type::Sequence { element, owned } => {
                self.byte(8);
                self.byte(u8::from(*owned));
                self.converted_type(element);
            }
            convert::Type::Tuple(members) => {
                self.byte(9);
                self.count(members.len());
                for member in members {
                    self.converted_type(member);
                }
            }
            convert::Type::Parameter(index) => {
                self.byte(3);
                self.0.update(index.to_le_bytes());
            }
            convert::Type::Script { path, arguments } => {
                self.byte(13);
                self.bytes(path.as_bytes());
                self.count(arguments.len());
                for argument in arguments {
                    self.converted_type(argument);
                }
            }
        }
    }

    fn converted_constant(&mut self, value: &convert::Constant) {
        self.bytes(value.ty().as_bytes());
        self.bytes(value.value().as_bytes());
    }

    fn surface_binder(&mut self, binder: super::instance::Binder) {
        self.0.update(binder.depth().to_le_bytes());
        self.0.update(binder.index().to_le_bytes());
    }

    fn callable(&mut self, class: super::callable::Class) {
        match class {
            super::callable::Class::Borrowed(proof) => {
                self.byte(0);
                self.byte(u8::from(proof.is_late_bound()));
            }
            super::callable::Class::Shared(proof) => {
                self.byte(1);
                self.byte(match proof.scope() {
                    super::callable::shared::Scope::Epoch => 0,
                    super::callable::shared::Scope::Region => 1,
                });
            }
            super::callable::Class::Mutable(_) => self.byte(2),
            super::callable::Class::Once(_) => self.byte(3),
        }
    }
}

const fn namespace(namespace: api::Namespace) -> u8 {
    match namespace {
        api::Namespace::Type => 0,
        api::Namespace::Value => 1,
        api::Namespace::Macro => 2,
    }
}

const fn reexport_kind(kind: ledger::ReexportKind) -> u8 {
    match kind {
        ledger::ReexportKind::Single => 0,
        ledger::ReexportKind::Glob => 1,
        ledger::ReexportKind::ExternCrate => 2,
        ledger::ReexportKind::MacroUse => 3,
        ledger::ReexportKind::MacroExport => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::{Definition, Export, Framed, Selector};
    use crate::api::{identity as api, ledger, model};
    use crate::classify::Policy;

    #[test]
    fn domains_and_length_frames_are_unambiguous() {
        let mut left = Framed::new(b"domain");
        left.bytes(b"ab");
        left.bytes(b"c");
        let mut right = Framed::new(b"domain");
        right.bytes(b"a");
        right.bytes(b"bc");
        assert_ne!(left.finish(), right.finish());

        let mut definition = Framed::new(super::DEFINITION_DOMAIN);
        definition.bytes(b"same");
        let mut export = Framed::new(super::EXPORT_DOMAIN);
        export.bytes(b"same");
        let mut implementation = Framed::new(super::IMPLEMENTATION_DOMAIN);
        implementation.bytes(b"same");
        let mut operation = Framed::new(super::OPERATION_DOMAIN);
        operation.bytes(b"same");
        let domains = [
            definition.finish(),
            export.finish(),
            implementation.finish(),
            operation.finish(),
        ];
        assert_eq!(
            domains
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4
        );
    }

    #[test]
    fn definition_identity_covers_unit_authority_and_namespace() {
        let base = definition(
            "fixture",
            "0.1.0",
            "path+file:///fixture",
            "feature",
            api::Namespace::Type,
        );
        for changed in [
            definition(
                "other",
                "0.1.0",
                "path+file:///fixture",
                "feature",
                api::Namespace::Type,
            ),
            definition(
                "fixture",
                "0.2.0",
                "path+file:///fixture",
                "feature",
                api::Namespace::Type,
            ),
            definition(
                "fixture",
                "0.1.0",
                "registry+https://example.invalid",
                "feature",
                api::Namespace::Type,
            ),
            definition(
                "fixture",
                "0.1.0",
                "path+file:///fixture",
                "other",
                api::Namespace::Type,
            ),
            definition(
                "fixture",
                "0.1.0",
                "path+file:///fixture",
                "feature",
                api::Namespace::Value,
            ),
        ] {
            assert_ne!(Definition::of(&base), Definition::of(&changed));
        }
    }

    #[test]
    fn export_position_is_not_semantic_identity() {
        let graph = crate::test_fixture::graph();
        let ledger::Path::Imported(original) = graph.exports().next().expect("fixture export")
        else {
            panic!("fixture starts with imported export");
        };
        let relocated = ledger::Export::new(
            original.key().clone(),
            original.target().clone(),
            original.aliases().cloned().collect(),
            original.reexports().cloned().collect(),
            model::Span::new("elsewhere.rs".into(), 80, 90, 8, 0, 8, 10),
        );
        assert_eq!(Export::imported(original), Export::imported(&relocated));
    }

    #[test]
    fn closed_selectors_produce_distinct_operation_identities() {
        let contract = crate::test_fixture::graph()
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
            .expect("fixture classifies");
        let base = contract
            .outcomes()
            .next()
            .expect("fixture outcome")
            .identity();
        let item = base
            .definition()
            .expect("imported identity has a definition");
        let selectors = [
            Selector::Declaration,
            Selector::StructConstructor(super::Form::Unit),
            Selector::StructField(super::Field::new(0, "field".into())),
            Selector::EnumVariant {
                index: 0,
                name: "Variant".into(),
                form: super::Form::Tuple { arity: 1 },
            },
            Selector::Function { item },
            Selector::Constant { item },
            Selector::Method {
                item,
                receiver: crate::classify::representability::ownership::Receiver::Owned,
            },
            Selector::Method {
                item,
                receiver: crate::classify::representability::ownership::Receiver::Shared,
            },
            Selector::Method {
                item,
                receiver: crate::classify::representability::ownership::Receiver::Mutable,
            },
            Selector::AssociatedFunction { item },
            Selector::AssociatedConstant { item },
            Selector::Unavailable,
        ];
        let identities = selectors
            .into_iter()
            .map(|selector| base.selected(selector))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(identities.len(), 12);
    }

    fn definition(
        name: &str,
        version: &str,
        source: &str,
        feature: &str,
        namespace: api::Namespace,
    ) -> api::Definition {
        api::Definition::new(
            api::Unit::new(
                api::Authority::Cargo(api::Package::new(
                    format!("{source}#{name}@{version}").into_boxed_str(),
                    name.into(),
                    version.into(),
                    source.into(),
                )),
                "target".into(),
                "lib".into(),
                name.into(),
                api::Compilation::Target,
                Box::from([Box::<str>::from(feature)]),
                "0000000000000001".into(),
            ),
            "01".into(),
            namespace,
        )
    }
}
