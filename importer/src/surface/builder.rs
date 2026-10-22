//! Durable symbolic replay for consuming native builders.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::identity;

const DOMAIN: &[u8] = b"rawr.surface.builder/v2";

/// One owned Rawr argument position consumed exactly once during replay.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Value(u32);

/// One constructor or consuming method in an ordered builder replay.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Step {
    /// Create the first native builder value inside this invocation.
    Construct(super::Entry, Box<[Value]>),
    /// Consume the previous native builder and produce the next value.
    Consume {
        entry: super::Entry,
        arguments: Box<[Value]>,
    },
}

/// A checked builder chain containing symbolic values only.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Plan {
    result: identity::Identity,
    steps: Box<[Step]>,
    semantic: [u8; 32],
}

impl Value {
    /// Bind one Rawr invocation argument to its exact zero-based position.
    #[must_use]
    pub const fn argument(position: u32) -> Self {
        Self(position)
    }

    #[must_use]
    pub const fn position(self) -> u32 {
        self.0
    }
}

impl Step {
    /// Transform one selected constructor use into the first replay step.
    #[must_use]
    pub const fn construct(
        identity: identity::Identity,
        origin: super::Origin,
        arguments: Box<[Value]>,
    ) -> Self {
        Self::Construct(super::Entry { identity, origin }, arguments)
    }

    /// Transform one selected owned-method use into a consuming replay step.
    #[must_use]
    pub const fn consume(
        identity: identity::Identity,
        origin: super::Origin,
        arguments: Box<[Value]>,
    ) -> Self {
        Self::Consume {
            entry: super::Entry { identity, origin },
            arguments,
        }
    }

    #[must_use]
    pub const fn entry(&self) -> &super::Entry {
        match self {
            Self::Construct(entry, _) | Self::Consume { entry, .. } => entry,
        }
    }

    #[must_use]
    pub const fn arguments(&self) -> &[Value] {
        match self {
            Self::Construct(_, arguments) | Self::Consume { arguments, .. } => arguments,
        }
    }

    #[must_use]
    pub const fn is_construct(&self) -> bool {
        matches!(self, Self::Construct(_, _))
    }
}

impl Plan {
    /// Validate one nonempty construct/consume chain and its exact-once ordered
    /// argument positions before any native value exists.
    pub fn new(
        result: identity::Identity,
        steps: Box<[Step]>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, super::Error> {
        let Some((first, remaining)) = steps.split_first() else {
            return Err(super::Error::BuilderShape);
        };
        if !first.is_construct() || remaining.is_empty() || remaining.iter().any(Step::is_construct)
        {
            return Err(super::Error::BuilderShape);
        }
        if !matches!(
            first.entry().identity().selector(),
            identity::Selector::StructConstructor(_)
                | identity::Selector::Function { .. }
                | identity::Selector::AssociatedFunction { .. }
        ) || remaining.iter().any(|step| {
            !matches!(
                step.entry().identity().selector(),
                identity::Selector::Method {
                    receiver: crate::classify::representability::ownership::Receiver::Owned,
                    ..
                }
            )
        }) {
            return Err(super::Error::BuilderShape);
        }
        if steps.last().map(Step::entry).map(super::Entry::identity) != Some(&result) {
            return Err(super::Error::BuilderShape);
        }
        // Replay item and byte sizes are accounting targets recorded on
        // a surface account for this plan; checked arithmetic still refuses.
        let mut account = super::account::Account::surface();
        let count = steps.iter().try_fold(steps.len(), |items, step| {
            items
                .checked_add(step.arguments().len())
                .ok_or(super::Error::Limit {
                    resource: "builder replay items",
                    limit: limits.items,
                    actual: usize::MAX,
                })
        })?;
        account.exceeded("builder replay items", limits.items, count);
        let argument_count = steps
            .iter()
            .map(Step::arguments)
            .map(<[Value]>::len)
            .sum::<usize>();
        let mut observed = Vec::new();
        observed
            .try_reserve_exact(argument_count)
            .map_err(|_| super::Error::Allocation)?;
        observed.resize(argument_count, false);
        for value in steps.iter().flat_map(Step::arguments) {
            let index = value.position() as usize;
            let Some(seen) = observed.get_mut(index) else {
                return Err(super::Error::BuilderArgument);
            };
            if *seen {
                return Err(super::Error::BuilderArgument);
            }
            *seen = true;
        }
        if observed.iter().any(|seen| !seen) {
            return Err(super::Error::BuilderArgument);
        }
        let retained = retained_bytes(&result, &steps).ok_or(super::Error::Limit {
            resource: "builder replay retained bytes",
            limit: limits.json_bytes,
            actual: usize::MAX,
        })?;
        account.exceeded("builder replay retained bytes", limits.json_bytes, retained);
        let semantic = semantic(&result, &steps);
        Ok(Self {
            result,
            steps,
            semantic,
        })
    }

    #[must_use]
    pub const fn result(&self) -> &identity::Identity {
        &self.result
    }

    #[must_use]
    pub const fn steps(&self) -> &[Step] {
        &self.steps
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.semantic
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        retained_bytes(&self.result, &self.steps)
    }
}

fn semantic(result: &identity::Identity, steps: &[Step]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(DOMAIN);
    digest.update(result.as_bytes());
    digest.update((steps.len() as u64).to_le_bytes());
    for step in steps {
        digest.update([u8::from(!step.is_construct())]);
        digest.update(step.entry().identity().as_bytes());
        digest.update((step.arguments().len() as u64).to_le_bytes());
        for argument in step.arguments() {
            digest.update(argument.position().to_le_bytes());
        }
    }
    digest.finalize().into()
}

fn retained_bytes(result: &identity::Identity, steps: &[Step]) -> Option<usize> {
    let mut bytes = result.checked_retained_bytes()?;
    bytes = bytes.checked_add(steps.len().checked_mul(std::mem::size_of::<Step>())?)?;
    for step in steps {
        bytes = bytes
            .checked_add(step.entry().identity().checked_retained_bytes()?)?
            .checked_add(step.entry().origin().source().len())?
            .checked_add(
                step.arguments()
                    .len()
                    .checked_mul(std::mem::size_of::<Value>())?,
            )?;
    }
    Some(bytes)
}
