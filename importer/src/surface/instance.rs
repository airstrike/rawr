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
}

pub(super) struct Parts {
    pub(super) types: Box<[convert::Type]>,
    pub(super) constants: Box<[convert::Constant]>,
    pub(super) binders: Box<[Binder]>,
    pub(super) callable: Option<super::callable::Class>,
    pub(super) functions: Box<[FunctionItem]>,
    pub(super) mappers: Box<[EventMapper]>,
    pub(super) predicates: Box<[super::identity::Implementation]>,
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
        })
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
        convert::Type::Nominal { arguments, .. } => {
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
        convert::Type::Option(element) => {
            concrete_type_bytes(element, actual, limit).and_then(|bytes| {
                bytes
                    .checked_add(std::mem::size_of::<convert::Type>())
                    .ok_or(super::Error::Limit {
                        resource: "surface instance bytes",
                        limit: usize::MAX,
                        actual: usize::MAX,
                    })
            })
        }
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
        convert::Type::Unit | convert::Type::Builtin(_) => Ok(std::mem::size_of::<convert::Type>()),
    }
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
