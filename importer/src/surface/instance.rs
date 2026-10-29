//! Canonical concrete substitutions selected by checked Rawr use.

use serde::{Deserialize, Serialize};

use crate::classify::representability::{self, convert};

/// One normalized higher-ranked input binder retained by a callable instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Binder {
    depth: u32,
    index: u32,
}

/// A bounded, fully concrete set of native surface arguments.
pub struct Arguments {
    types: Box<[convert::Type]>,
    constants: Box<[convert::Constant]>,
    binders: Box<[Binder]>,
    callable: Option<super::callable::Class>,
    functions: Box<[FunctionItem]>,
    mappers: Box<[EventMapper]>,
    predicates: Box<[super::identity::Implementation]>,
    slots: Box<[Slot]>,
    impls: Box<[ScriptImpl]>,
}

/// A script function that fills a callable parameter of the item as a
/// slot (D594): a function item the runner generates, which calls the
/// script function in the generation current where native code calls it.
/// Its key names the script function, so two functions are two slots, of
/// two Rust types, as two Rust functions are.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Slot {
    position: u32,
    key: [u8; 32],
}

impl Slot {
    /// The slot for the script function named `name` at the call's input
    /// `position`.
    #[must_use]
    pub fn new(position: u32, name: &str) -> Self {
        use sha2::{Digest as _, Sha256};
        let mut digest = Sha256::new();
        digest.update(b"rawr.instance.slot/v1");
        digest.update(name.as_bytes());
        Self {
            position,
            key: digest.finalize().into(),
        }
    }

    /// The input the slot fills.
    #[must_use]
    pub const fn position(&self) -> u32 {
        self.position
    }

    /// The digest of the script function's name.
    #[must_use]
    pub const fn key(&self) -> &[u8; 32] {
        &self.key
    }
}

pub(super) struct Parts {
    pub(super) types: Box<[convert::Type]>,
    pub(super) constants: Box<[convert::Constant]>,
    pub(super) binders: Box<[Binder]>,
    pub(super) callable: Option<super::callable::Class>,
    pub(super) functions: Box<[FunctionItem]>,
    pub(super) mappers: Box<[EventMapper]>,
    pub(super) predicates: Box<[super::identity::Implementation]>,
    pub(super) slots: Box<[Slot]>,
    pub(super) impls: Box<[ScriptImpl]>,
}

/// A script's impl of a trait that crosses into native code with its owner
/// (D594): `impl canvas::Program<Message> for Arc`, of a dependency's trait
/// for a script type, or `impl From<Color> for iced::Color`, of a std trait
/// between a script type and a crate's. The runner writes it for the
/// proxies, and each function calls the script's own in the generation
/// current where native code calls it.
///
/// Types are as an instance's are, with each of the impl's own type
/// parameters a [`convert::Type::Parameter`] at its index in `generics`.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ScriptImpl {
    owner: convert::Type,
    trait_path: Box<str>,
    arguments: Box<[convert::Type]>,
    generics: Box<[Box<str>]>,
    associated: Box<[(Box<str>, convert::Type)]>,
    functions: Box<[ScriptImplFunction]>,
    /// The trait's lifetime arguments, which the header writes `'_`.
    #[serde(default)]
    lifetimes: u32,
}

/// One function of a [`ScriptImpl`].
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ScriptImplFunction {
    name: Box<str>,
    member: Box<str>,
    receiver: ImplReceiver,
    inputs: Box<[convert::Type]>,
    output: convert::Type,
}

/// How a [`ScriptImplFunction`] takes `self`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ImplReceiver {
    /// An associated function, as `From::from`.
    None,
    /// `&self`.
    Shared,
    /// `&mut self`.
    Mutable,
    /// `self`.
    Owned,
}

impl ScriptImpl {
    /// The impl of the trait at `trait_path` (the Rust path the runner
    /// writes, `iced::widget::canvas::Program` or `From`), with the type
    /// arguments the header writes, for `owner`.
    #[must_use]
    pub const fn new(
        owner: convert::Type,
        trait_path: Box<str>,
        arguments: Box<[convert::Type]>,
        generics: Box<[Box<str>]>,
        associated: Box<[(Box<str>, convert::Type)]>,
        functions: Box<[ScriptImplFunction]>,
    ) -> Self {
        Self {
            owner,
            trait_path,
            arguments,
            generics,
            associated,
            functions,
            lifetimes: 0,
        }
    }

    /// This impl of a trait with `lifetimes` lifetime arguments before its
    /// types, as iced's `Component<'a, Message>`.
    #[must_use]
    pub const fn with_lifetimes(mut self, lifetimes: u32) -> Self {
        self.lifetimes = lifetimes;
        self
    }

    /// The trait's lifetime arguments.
    #[must_use]
    pub const fn lifetimes(&self) -> u32 {
        self.lifetimes
    }

    /// The type the impl is for.
    #[must_use]
    pub const fn owner(&self) -> &convert::Type {
        &self.owner
    }

    /// The trait's Rust path.
    #[must_use]
    pub fn trait_path(&self) -> &str {
        &self.trait_path
    }

    /// The trait's type arguments as the header writes them.
    #[must_use]
    pub fn arguments(&self) -> &[convert::Type] {
        &self.arguments
    }

    /// The impl's own type parameters, by name.
    #[must_use]
    pub fn generics(&self) -> &[Box<str>] {
        &self.generics
    }

    /// The associated types the impl gives, by name.
    #[must_use]
    pub fn associated(&self) -> &[(Box<str>, convert::Type)] {
        &self.associated
    }

    /// The impl's functions.
    #[must_use]
    pub fn functions(&self) -> &[ScriptImplFunction] {
        &self.functions
    }
}

impl ScriptImplFunction {
    /// The function `name`, which the script's program holds as its
    /// owner's `member` (`Program<Message>::draw`).
    #[must_use]
    pub const fn new(
        name: Box<str>,
        member: Box<str>,
        receiver: ImplReceiver,
        inputs: Box<[convert::Type]>,
        output: convert::Type,
    ) -> Self {
        Self {
            name,
            member,
            receiver,
            inputs,
            output,
        }
    }

    /// The function's Rust name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The member the script's program holds it as.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// How it takes `self`.
    #[must_use]
    pub const fn receiver(&self) -> ImplReceiver {
        self.receiver
    }

    /// Its inputs after `self`.
    #[must_use]
    pub fn inputs(&self) -> &[convert::Type] {
        &self.inputs
    }

    /// Its output.
    #[must_use]
    pub const fn output(&self) -> &convert::Type {
        &self.output
    }
}

/// One exact public imported function item consumed at a direct formal.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct FunctionItem {
    position: u32,
    identity: Box<super::identity::Identity>,
    byte_start: u32,
    byte_end: u32,
}

/// One exact script mapper accepted only by the authenticated profile event lane.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct EventMapper {
    position: u32,
    seal: [u8; 16],
    byte_start: u32,
    byte_end: u32,
    projection: Option<EventProjection>,
}

/// One private compiler/profile proof that a single mapped invocation may be
/// converted by the closed profile replay. It grants no facade projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct EventProjection {
    profile: [u8; 32],
    profile_version: u32,
    profile_rules: [u8; 32],
    profile_source: [u8; 32],
    profile_projection: [u8; 32],
    source: super::identity::Definition,
    target: super::identity::Definition,
    widget: super::identity::Implementation,
    message: [u8; 32],
    invocation: [u8; 32],
    selector: [u8; 32],
}

impl Arguments {
    /// Transform checked inference results into a finite surface instance.
    pub fn new(
        types: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        binders: Box<[Binder]>,
        callable: Option<super::callable::Class>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        // Argument count, token and byte sizes are accounting targets
        // recorded on a surface account for this instance; only checked
        // arithmetic and the physical type depth refuse.
        let mut account = super::account::Account::surface();
        let items = types
            .len()
            .checked_add(constants.len())
            .and_then(|items| items.checked_add(binders.len()))
            .ok_or(super::Error::Limit {
                resource: "surface instance arguments",
                limit: limits.items,
                actual: usize::MAX,
            })?;
        account.exceeded("surface instance arguments", limits.items, items);
        let mut bytes = 0_usize;
        for ty in &types {
            bytes = bytes
                .checked_add(concrete_type_bytes(ty, 0, limits.depth)?)
                .ok_or(super::Error::Limit {
                    resource: "surface instance bytes",
                    limit: limits.json_bytes,
                    actual: usize::MAX,
                })?;
        }
        for value in &constants {
            account.exceeded(
                "surface constant token bytes",
                limits.string_bytes,
                value.ty().len().max(value.value().len()),
            );
            bytes = bytes
                .checked_add(value.ty().len())
                .and_then(|bytes| bytes.checked_add(value.value().len()))
                .ok_or(super::Error::Limit {
                    resource: "surface instance bytes",
                    limit: limits.json_bytes,
                    actual: usize::MAX,
                })?;
        }
        account.exceeded("surface instance bytes", limits.json_bytes, bytes);
        Ok(Self {
            types,
            constants,
            binders,
            callable,
            functions: Box::default(),
            mappers: Box::default(),
            predicates: Box::default(),
            slots: Box::default(),
            impls: Box::default(),
        })
    }

    /// Attach the script's impls that cross into native code with the
    /// script types among the instance's arguments (D594).
    #[doc(hidden)]
    #[must_use]
    pub fn with_impls(mut self, impls: Box<[ScriptImpl]>) -> Self {
        self.impls = impls;
        self
    }

    /// Attach the script functions that fill the item's callable
    /// parameters as slots (D594).
    #[doc(hidden)]
    pub fn with_slots(mut self, slots: Box<[Slot]>) -> Result<Self, super::Error> {
        if !slots
            .windows(2)
            .all(|pair| pair[0].position < pair[1].position)
        {
            return Err(super::Error::InstanceMismatch);
        }
        self.slots = slots;
        Ok(self)
    }

    /// Attach the exact compile-time function items selected by checked Rawr source.
    #[doc(hidden)]
    pub fn with_function_items(
        mut self,
        functions: Box<[FunctionItem]>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        if !functions
            .windows(2)
            .all(|pair| pair[0].position < pair[1].position)
        {
            return Err(super::Error::InstanceMismatch);
        }
        // The item count is an accounting target, recorded not refused.
        super::account::Account::surface().exceeded(
            "surface instance function items",
            limits.items,
            functions.len(),
        );
        self.functions = functions;
        Ok(self)
    }

    /// Attach exact non-authoritative script event-mapper seals selected by
    /// checked Rawr source. They remain disjoint from imported function items.
    #[doc(hidden)]
    pub fn with_event_mappers(
        mut self,
        mappers: Box<[EventMapper]>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        // The item count is an accounting target, recorded not refused.
        super::account::Account::surface().exceeded(
            "surface instance event mappers",
            limits.items,
            mappers.len(),
        );
        if !mappers
            .windows(2)
            .all(|pair| pair[0].position < pair[1].position)
            || self.functions.iter().any(|function| {
                mappers
                    .iter()
                    .any(|mapper| mapper.position == function.position)
            })
        {
            return Err(super::Error::InstanceMismatch);
        }
        self.mappers = mappers;
        Ok(self)
    }

    /// Seal an input-only callable instance, including normalized late-bound
    /// input binders that cannot escape through output or storage.
    #[doc(hidden)]
    pub fn borrowed(
        types: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        binders: Box<[Binder]>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        let proof = super::callable::borrowed::Proof::input_only(!binders.is_empty());
        Self::new(
            types,
            constants,
            binders,
            Some(super::callable::Class::Borrowed(proof)),
            limits,
        )
    }

    /// Seal an owned immutable `'static` callable for one native scope.
    #[doc(hidden)]
    pub fn shared(
        types: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        binders: Box<[Binder]>,
        scope: super::callable::shared::Scope,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        let proof = super::callable::shared::Proof::static_owned(scope);
        Self::new(
            types,
            constants,
            binders,
            Some(super::callable::Class::Shared(proof)),
            limits,
        )
    }

    /// Seal an owned mutable `'static` callable retired with its epoch.
    #[doc(hidden)]
    pub fn mutable(
        types: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        binders: Box<[Binder]>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        Self::new(
            types,
            constants,
            binders,
            Some(super::callable::Class::Mutable(
                super::callable::mutable::Proof::static_owned(),
            )),
            limits,
        )
    }

    /// Seal an owned one-shot callable with explicit consumed state.
    #[doc(hidden)]
    pub fn once(
        types: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        binders: Box<[Binder]>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        Self::new(
            types,
            constants,
            binders,
            Some(super::callable::Class::Once(
                super::callable::once::Proof::static_owned(),
            )),
            limits,
        )
    }

    pub(crate) fn matches(
        &self,
        representability: &representability::Representability,
    ) -> Result<(), super::Error> {
        let parameters = match representability {
            representability::Representability::FunctionItem(item) => item.signature().parameters(),
            representability::Representability::Invocation(item) => item.signature().parameters(),
            // A generic struct made at an instance other than its defaulted
            // one, as iced's `Size<Length>`.
            representability::Representability::Struct(plan)
                if !self.types.is_empty()
                    && self.constants.is_empty()
                    && self.binders.is_empty()
                    && self.callable.is_none()
                    && self.slots.is_empty() =>
            {
                plan.parameters()
            }
            _ if self.types.is_empty()
                && self.constants.is_empty()
                && self.binders.is_empty()
                && self.callable.is_none() =>
            {
                return Ok(())
            }
            _ => return Err(super::Error::InstanceMismatch),
        };
        let type_parameters = parameters
            .iter()
            .filter(|parameter| parameter.kind() == representability::signature::Kind::Type)
            .count();
        let constant_parameters = parameters
            .iter()
            .filter(|parameter| parameter.kind() == representability::signature::Kind::Constant)
            .count();
        // A call whose callables are slots needs each parameter to cross.
        if !self.slots.is_empty() {
            let crossings = match representability {
                representability::Representability::FunctionItem(item) => item.crossings(),
                representability::Representability::Invocation(item) => item.crossings(),
                _ => &[],
            };
            return if type_parameters == self.types.len()
                && constant_parameters == self.constants.len()
                && self.callable.is_none()
                && !crossings.is_empty()
            {
                Ok(())
            } else {
                Err(super::Error::InstanceMismatch)
            };
        }
        if type_parameters == self.types.len()
            && constant_parameters == self.constants.len()
            && parameters
                .first()
                .map(|_| match representability {
                    representability::Representability::FunctionItem(item) => {
                        item.class() == self.callable && item.binders() == self.binders.as_ref()
                    }
                    representability::Representability::Invocation(item) => {
                        item.class() == self.callable && item.binders() == self.binders.as_ref()
                    }
                    _ => true,
                })
                .unwrap_or(self.callable.is_none() && self.binders.is_empty())
        {
            Ok(())
        } else {
            Err(super::Error::InstanceMismatch)
        }
    }

    pub(crate) const fn types(&self) -> &[convert::Type] {
        &self.types
    }

    pub(crate) const fn constants(&self) -> &[convert::Constant] {
        &self.constants
    }

    pub(crate) const fn binders(&self) -> &[Binder] {
        &self.binders
    }

    /// Inspect the closed callable ownership class selected by checked use.
    #[must_use]
    pub const fn callable_class(&self) -> Option<super::callable::Class> {
        self.callable
    }

    pub(super) fn into_parts(self) -> Parts {
        Parts {
            types: self.types,
            constants: self.constants,
            binders: self.binders,
            callable: self.callable,
            functions: self.functions,
            mappers: self.mappers,
            predicates: self.predicates,
            slots: self.slots,
            impls: self.impls,
        }
    }

    pub(crate) const fn function_items(&self) -> &[FunctionItem] {
        &self.functions
    }

    pub(crate) const fn event_mappers(&self) -> &[EventMapper] {
        &self.mappers
    }

    pub(super) fn event_mappers_mut(&mut self) -> &mut [EventMapper] {
        &mut self.mappers
    }

    pub(crate) const fn predicate_proofs(&self) -> &[super::identity::Implementation] {
        &self.predicates
    }

    pub(crate) fn seal_predicates(&mut self, predicates: Box<[super::identity::Implementation]>) {
        self.predicates = predicates;
    }
}

impl FunctionItem {
    /// Transform one exact classified selector and positioned source item into
    /// a compile-time-only operand proof.
    #[doc(hidden)]
    pub fn selected(
        position: u32,
        base: &super::identity::Identity,
        selector: super::identity::Selector,
        byte_start: u32,
        byte_end: u32,
    ) -> Result<Self, super::Error> {
        if byte_start >= byte_end {
            return Err(super::Error::InstanceMismatch);
        }
        Ok(Self {
            position,
            identity: Box::new(base.selected(selector)),
            byte_start,
            byte_end,
        })
    }

    /// [`Self::selected`] of a generic function at the types `instance`
    /// gives its parameters, as iced's `window::close::<Message>` passed
    /// as `and_then(window::close)`.
    #[doc(hidden)]
    pub fn instantiated(
        position: u32,
        base: &super::identity::Identity,
        selector: super::identity::Selector,
        instance: Arguments,
        byte_start: u32,
        byte_end: u32,
    ) -> Result<Self, super::Error> {
        if byte_start >= byte_end {
            return Err(super::Error::InstanceMismatch);
        }
        Ok(Self {
            position,
            identity: Box::new(base.instantiated(selector, instance)),
            byte_start,
            byte_end,
        })
    }

    #[must_use]
    pub const fn position(&self) -> u32 {
        self.position
    }

    #[must_use]
    pub const fn identity(&self) -> &super::identity::Identity {
        &self.identity
    }

    #[must_use]
    pub const fn byte_range(&self) -> (u32, u32) {
        (self.byte_start, self.byte_end)
    }
}

impl EventMapper {
    /// Transform one checked symbolic mapper position and bounded source range
    /// into a non-authoritative instance proof.
    #[doc(hidden)]
    pub fn selected(
        position: u32,
        seal: [u8; 16],
        byte_start: u32,
        byte_end: u32,
    ) -> Result<Self, super::Error> {
        if seal == [0; 16] || byte_start >= byte_end {
            return Err(super::Error::InstanceMismatch);
        }
        Ok(Self {
            position,
            seal,
            byte_start,
            byte_end,
            projection: None,
        })
    }

    #[must_use]
    pub const fn position(&self) -> u32 {
        self.position
    }

    #[must_use]
    pub const fn seal(&self) -> &[u8; 16] {
        &self.seal
    }

    #[must_use]
    pub const fn byte_range(&self) -> (u32, u32) {
        (self.byte_start, self.byte_end)
    }

    pub(crate) const fn projection(&self) -> Option<&EventProjection> {
        self.projection.as_ref()
    }

    pub(crate) fn seal_projection(
        &mut self,
        projection: EventProjection,
    ) -> Result<(), super::Error> {
        if self.projection.replace(projection).is_some() {
            return Err(super::Error::InstanceMismatch);
        }
        Ok(())
    }
}

impl EventProjection {
    #[allow(
        clippy::too_many_arguments,
        reason = "the proof bundle seals every joined authority"
    )]
    pub(super) const fn sealed(
        profile: [u8; 32],
        profile_version: u32,
        profile_rules: [u8; 32],
        profile_source: [u8; 32],
        profile_projection: [u8; 32],
        source: super::identity::Definition,
        target: super::identity::Definition,
        widget: super::identity::Implementation,
        message: [u8; 32],
        invocation: [u8; 32],
        selector: [u8; 32],
    ) -> Self {
        Self {
            profile,
            profile_version,
            profile_rules,
            profile_source,
            profile_projection,
            source,
            target,
            widget,
            message,
            invocation,
            selector,
        }
    }

    pub(crate) const fn profile(&self) -> &[u8; 32] {
        &self.profile
    }

    pub(crate) const fn profile_version(&self) -> u32 {
        self.profile_version
    }

    pub(crate) const fn profile_rules(&self) -> &[u8; 32] {
        &self.profile_rules
    }

    pub(crate) const fn profile_source(&self) -> &[u8; 32] {
        &self.profile_source
    }

    pub(crate) const fn profile_projection(&self) -> &[u8; 32] {
        &self.profile_projection
    }

    pub(crate) const fn source(&self) -> super::identity::Definition {
        self.source
    }

    pub(crate) const fn target(&self) -> super::identity::Definition {
        self.target
    }

    pub(crate) const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl Binder {
    /// Retain a rustc-normalized de Bruijn binder identity.
    #[doc(hidden)]
    #[must_use]
    pub const fn normalized(depth: u32, index: u32) -> Self {
        Self { depth, index }
    }

    /// Return the normalized de Bruijn depth.
    #[must_use]
    pub const fn depth(self) -> u32 {
        self.depth
    }

    /// Return the binder-local index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }
}

fn concrete_type_bytes(
    ty: &convert::Type,
    depth: usize,
    limit: usize,
) -> Result<usize, super::Error> {
    let actual = depth.checked_add(1).ok_or(super::Error::Limit {
        resource: "surface type depth",
        limit,
        actual: usize::MAX,
    })?;
    if actual > limit {
        return Err(super::Error::Limit {
            resource: "surface type depth",
            limit,
            actual,
        });
    }
    match ty {
        convert::Type::Parameter(_) => Err(super::Error::UnconstrainedGeneric),
        convert::Type::Nominal { arguments, .. } | convert::Type::Script { arguments, .. } => {
            arguments
                .iter()
                .try_fold(std::mem::size_of::<convert::Type>(), |bytes, argument| {
                    bytes
                        .checked_add(concrete_type_bytes(argument, actual, limit)?)
                        .ok_or(super::Error::Limit {
                            resource: "surface instance bytes",
                            limit: usize::MAX,
                            actual: usize::MAX,
                        })
                })
        }
        convert::Type::FunctionItem { .. } => Err(super::Error::InstanceMismatch),
        convert::Type::Tuple(members) => {
            members
                .iter()
                .try_fold(std::mem::size_of::<convert::Type>(), |bytes, member| {
                    bytes
                        .checked_add(concrete_type_bytes(member, actual, limit)?)
                        .ok_or(super::Error::Limit {
                            resource: "surface instance bytes",
                            limit: usize::MAX,
                            actual: usize::MAX,
                        })
                })
        }
        convert::Type::Option(element)
        | convert::Type::Borrowed { ty: element, .. }
        | convert::Type::Range(element)
        | convert::Type::Sequence { element, .. } => concrete_type_bytes(element, actual, limit)
            .and_then(|bytes| {
                bytes
                    .checked_add(std::mem::size_of::<convert::Type>())
                    .ok_or(super::Error::Limit {
                        resource: "surface instance bytes",
                        limit: usize::MAX,
                        actual: usize::MAX,
                    })
            }),
        convert::Type::Result(ok, err) => {
            let ok = concrete_type_bytes(ok, actual, limit)?;
            let err = concrete_type_bytes(err, actual, limit)?;
            ok.checked_add(err)
                .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<convert::Type>()))
                .ok_or(super::Error::Limit {
                    resource: "surface instance bytes",
                    limit: usize::MAX,
                    actual: usize::MAX,
                })
        }
        convert::Type::ProfileMessage { payload, .. } => {
            concrete_type_bytes(payload, actual, limit).and_then(|bytes| {
                bytes.checked_add(16).ok_or(super::Error::Limit {
                    resource: "surface instance bytes",
                    limit: usize::MAX,
                    actual: usize::MAX,
                })
            })
        }
        convert::Type::Inferred => Ok(std::mem::size_of::<convert::Type>()),
        convert::Type::Unit | convert::Type::Builtin(_) => Ok(std::mem::size_of::<convert::Type>()),
    }
}

/// The key of one instance of a generic imported item: a digest of its
/// type arguments in the item's declared order. Rawr keys a call by it,
/// and the generated runner binds the instance's adapter by it.
#[must_use]
pub fn key(types: &[convert::Type]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"rawr.instance.key/v1");
    digest.update(serde_json::to_vec(types).unwrap_or_default());
    digest.finalize().into()
}

/// [`key`] of an instance whose callables are the script functions
/// `slots` (D594): each function is an instance of its own, as each Rust
/// function is a type of its own.
#[must_use]
pub fn key_with_slots(types: &[convert::Type], slots: &[Slot]) -> [u8; 32] {
    if slots.is_empty() {
        return key(types);
    }
    use sha2::{Digest as _, Sha256};
    let mut digest = Sha256::new();
    digest.update(key(types));
    for slot in slots {
        digest.update(slot.position.to_le_bytes());
        digest.update(slot.key);
    }
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use crate::classify::representability::convert;

    /// The argument item and byte targets are accounting; an instance
    /// past them is still sealed with its exact arguments.
    #[test]
    fn instance_arguments_past_their_targets_are_sealed() {
        let types: Box<[convert::Type]> = Box::new([convert::Type::Unit, convert::Type::Unit]);
        let standard = crate::cargo::Limits::standard();
        let exact = super::Arguments::new(
            types.clone(),
            Box::default(),
            Box::default(),
            None,
            &standard,
        )
        .expect("arguments under the standard targets seal");
        assert_eq!(exact.types.len(), 2);
        let bounded = crate::cargo::Limits::fixture(0, 1, 0);
        let past = super::Arguments::new(types, Box::default(), Box::default(), None, &bounded)
            .expect("arguments past the item and byte targets still seal");
        assert_eq!(past.types.len(), 2);
        let functions = past
            .with_function_items(Box::default(), &bounded)
            .expect("function items attach past the item target");
        assert!(functions.functions.is_empty());
    }
}
