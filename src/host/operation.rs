//! Exact compiler-selected host operation proofs.

use crate::{host, item, program, Dynamic, ImmutableString};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;
use std::future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "sync")]
type Future = Pin<Box<dyn future::Future<Output = Result<Dynamic, Error>> + Send + 'static>>;
#[cfg(not(feature = "sync"))]
type Future = Pin<Box<dyn future::Future<Output = Result<Dynamic, Error>> + 'static>>;

/// An opaque proof of one exact instantiated symbolic host operation.
#[derive(Clone)]
pub struct Proof {
    owner: host::facade::Dependency,
    definition: host::item::Definition,
    fingerprint: host::item::Fingerprint,
    path: ImmutableString,
    inputs: Box<[host::runtime::type_ref::Type]>,
    output: host::runtime::type_ref::Type,
}

/// The exact boxed arguments validated for one symbolic operation call.
#[derive(Clone)]
pub struct Arguments {
    values: Box<[Dynamic]>,
}

/// A compiler-sealed symbolic operation call awaiting host issuance.
#[derive(Clone)]
pub struct Call {
    proof: Proof,
    arguments: Arguments,
}

/// One exact bound-owner-issued operation awaiting its validated payload.
#[must_use = "an issued operation must be awaited by the host runtime"]
pub struct Pending {
    future: Future,
}

/// An exact bound owner could not issue or complete a symbolic operation.
#[derive(Debug)]
pub enum Error {
    /// The compiler-selected operation has no matching exact runtime binding.
    Binding(host::runtime::Error),
    /// The durable arguments did not match the operation's exact arity.
    Arity {
        /// Canonical operation path.
        operation: ImmutableString,
        /// Exact declared argument count.
        expected: usize,
        /// Supplied argument count.
        actual: usize,
    },
    /// One durable argument failed its exact operation input proof.
    Input {
        /// Canonical operation path.
        operation: ImmutableString,
        /// Zero-based declared parameter index.
        index: usize,
        /// Exact runtime value failure.
        source: host::runtime::ValueError,
    },
    /// The normalized native result failed its exact operation output proof.
    Output {
        /// Canonical operation path.
        operation: ImmutableString,
        /// Exact runtime value failure.
        source: host::runtime::ValueError,
    },
    /// The symbolic mapper could not rebind or execute under the issuing authority.
    Mapper(program::Error),
    /// The issuing application or preview Message lacks an exact runtime proof.
    MessageContract {
        /// Human-readable Message contract.
        message: ImmutableString,
    },
}

pub(crate) struct Request {
    proof: Proof,
    arguments: Arguments,
    mapper: item::Value,
}

impl Proof {
    pub(crate) fn new(
        owner: host::facade::Dependency,
        item: &host::item::Definition,
        inputs: Box<[host::runtime::type_ref::Type]>,
        output: host::runtime::type_ref::Type,
    ) -> Self {
        Self {
            owner,
            definition: item.clone(),
            fingerprint: item.fingerprint(),
            path: item.path().into(),
            inputs,
            output,
        }
    }

    /// Return the canonical qualified operation path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn owner(&self) -> &host::facade::Dependency {
        &self.owner
    }

    pub(crate) const fn fingerprint(&self) -> &host::item::Fingerprint {
        &self.fingerprint
    }

    pub(crate) fn types_for(
        &self,
        owner: &host::facade::Dependency,
        definition: &host::item::Definition,
    ) -> Option<(
        &[host::runtime::type_ref::Type],
        &host::runtime::type_ref::Type,
    )> {
        (owner == &self.owner
            && definition == &self.definition
            && definition.fingerprint() == self.fingerprint)
            .then_some((&self.inputs, &self.output))
    }

    pub(crate) fn dependencies(&self) -> BTreeSet<host::facade::Dependency> {
        debug_assert_eq!(self.definition.fingerprint(), self.fingerprint);
        let mut dependencies = BTreeSet::from([self.owner.clone()]);
        for input in &self.inputs {
            input.collect_dependencies(&mut dependencies);
        }
        self.output.collect_dependencies(&mut dependencies);
        dependencies
    }

    pub(crate) const fn inputs(&self) -> &[host::runtime::type_ref::Type] {
        &self.inputs
    }

    pub(crate) const fn output(&self) -> &host::runtime::type_ref::Type {
        &self.output
    }
}

impl Arguments {
    pub(crate) fn seal(values: Box<[Dynamic]>) -> Self {
        Self { values }
    }

    /// Return the number of validated positional arguments.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.values.len()
    }

    /// Return whether this operation call has no arguments.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Borrow one validated positional argument.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Dynamic> {
        self.values.get(index)
    }

    /// Consume the carrier into its validated positional values.
    #[must_use]
    pub fn into_values(self) -> Box<[Dynamic]> {
        self.values
    }
}

impl Call {
    pub(crate) fn seal(proof: Proof, values: Box<[Dynamic]>) -> Self {
        Self {
            proof,
            arguments: Arguments::seal(values),
        }
    }

    pub(crate) fn into_parts(self) -> (Proof, Arguments) {
        (self.proof, self.arguments)
    }
}

impl Pending {
    #[cfg(feature = "sync")]
    pub(crate) fn new(
        future: impl future::Future<Output = Result<Dynamic, Error>> + Send + 'static,
    ) -> Self {
        Self {
            future: Box::pin(future),
        }
    }

    #[cfg(not(feature = "sync"))]
    pub(crate) fn new(
        future: impl future::Future<Output = Result<Dynamic, Error>> + 'static,
    ) -> Self {
        Self {
            future: Box::pin(future),
        }
    }
}

impl future::Future for Pending {
    type Output = Result<Dynamic, Error>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.future.as_mut().poll(context)
    }
}

impl Request {
    pub(crate) fn new(call: Call, mapper: &item::Value) -> Self {
        let (proof, arguments) = call.into_parts();
        Self {
            proof,
            arguments,
            mapper: mapper.clone(),
        }
    }

    pub(crate) fn into_parts(self) -> (Proof, Arguments, item::Value) {
        (self.proof, self.arguments, self.mapper)
    }
}

impl fmt::Debug for Proof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Proof(<private>)")
    }
}

impl fmt::Debug for Arguments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Arguments(<private>)")
    }
}

impl fmt::Debug for Call {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Call(<private>)")
    }
}

impl fmt::Debug for Pending {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Pending(<private>)")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(error) => error.fmt(formatter),
            Self::Arity {
                operation,
                expected,
                actual,
            } => write!(
                formatter,
                "operation `{operation}` received {actual} argument(s), expected {expected}"
            ),
            Self::Input {
                operation,
                index,
                source,
            } => {
                write!(
                    formatter,
                    "operation `{operation}` received invalid argument {index}: {source}"
                )
            }
            Self::Output { operation, source } => write!(
                formatter,
                "operation `{operation}` completed with invalid output: {source}"
            ),
            Self::Mapper(error) => error.fmt(formatter),
            Self::MessageContract { message } => write!(
                formatter,
                "operation issuer Message contract `{message}` has no exact runtime representation"
            ),
        }
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Input { source, .. } | Self::Output { source, .. } => Some(source),
            Self::Mapper(error) => Some(error),
            Self::Arity { .. } | Self::MessageContract { .. } => None,
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for Error {
    fn source(&self) -> Option<&(dyn core_error::Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Input { source, .. } | Self::Output { source, .. } => Some(source),
            Self::Mapper(error) => Some(error),
            Self::Arity { .. } | Self::MessageContract { .. } => None,
        }
    }
}

impl PartialEq for Proof {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.fingerprint == other.fingerprint
            && self.inputs == other.inputs
            && self.output == other.output
    }
}

impl Eq for Proof {}

impl Hash for Proof {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.owner.hash(state);
        self.fingerprint.hash(state);
        self.inputs.hash(state);
        self.output.hash(state);
    }
}

impl PartialOrd for Proof {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Proof {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.owner, &self.fingerprint, &self.inputs, &self.output).cmp(&(
            &other.owner,
            &other.fingerprint,
            &other.inputs,
            &other.output,
        ))
    }
}

impl crate::preview::digest::Write for Proof {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2d00);
        writer.tag(0x2d01);
        self.owner.write(writer)?;
        writer.tag(0x2d02);
        self.fingerprint.write(writer)?;
        writer.tag(0x2d03);
        writer.count(self.inputs.len());
        for input in &self.inputs {
            input.write(writer)?;
        }
        writer.tag(0x2d04);
        self.output.write(writer)?;
        Ok(())
    }
}
