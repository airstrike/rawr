//! Exact durable plans and bound execution for symbolic host streams.

pub mod identity;

use crate::preview::digest::Write;
use crate::{host, item, program, Dynamic, ImmutableString};
use futures_core::Stream;
use std::collections::BTreeSet;
use std::fmt;
use std::pin::Pin;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;
use std::task::{Context, Poll};

const IDENTITY_VERSION: &[u8] = b"rawr-stream-plan-identity-v1";

#[cfg(feature = "sync")]
type Native = Pin<Box<dyn Stream<Item = Dynamic> + Send + 'static>>;
#[cfg(not(feature = "sync"))]
type Native = Pin<Box<dyn Stream<Item = Dynamic> + 'static>>;

#[cfg(feature = "sync")]
type Mapper = Box<dyn FnMut(Dynamic) -> Result<Option<Dynamic>, Error> + Send + 'static>;
#[cfg(not(feature = "sync"))]
type Mapper = Box<dyn FnMut(Dynamic) -> Result<Option<Dynamic>, Error> + 'static>;

#[cfg(feature = "sync")]
type Starter = Box<dyn FnOnce() -> Issued + Send + 'static>;
#[cfg(not(feature = "sync"))]
type Starter = Box<dyn FnOnce() -> Issued + 'static>;

/// Explicit version of a facade stream operation's canonical argument protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Version(u32);

/// An opaque proof of one exact instantiated symbolic stream operation.
#[derive(Clone)]
pub struct Proof {
    owner: host::facade::Dependency,
    definition: host::item::Definition,
    fingerprint: host::item::Fingerprint,
    path: ImmutableString,
    version: Version,
    inputs: Box<[host::runtime::type_ref::Type]>,
    output: host::runtime::type_ref::Type,
}

/// Exact boxed arguments retained by one symbolic stream call.
#[derive(Clone)]
pub struct Arguments {
    values: Box<[Dynamic]>,
}

/// A compiler-sealed symbolic stream call.
#[derive(Clone)]
pub struct Call {
    proof: Proof,
    arguments: Arguments,
}

/// A durable, cloneable subscription plan containing only symbolic intent.
#[derive(Clone)]
pub struct Plan {
    streams: Box<[Planned]>,
    witness: host::runtime::Witness,
}

#[derive(Clone)]
pub(crate) struct Planned {
    call: Call,
    mappers: Box<[item::Value]>,
    /// Whether each mapper is a `filter_map`, whose `None` drops the item.
    filters: Box<[bool]>,
}

/// Stable canonical identity of one exact issued stream plan.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identity([u8; 16]);

/// One exact bound recipe whose stable identity is known before native start.
#[must_use = "a bound stream recipe must be retained or started by the host runtime"]
pub struct Recipe {
    identity: Identity,
    starter: Starter,
}

/// One bound-owner-issued stream. It runs only native work and yields each
/// raw item as an [`Arrival`], whose mappers the host runs on the script
/// thread.
#[must_use = "an issued stream must be retained by the host runtime"]
pub struct Issued {
    identity: Identity,
    stream: Native,
    mapper: crate::Shared<std::sync::Mutex<Mapper>>,
}

/// One native stream item awaiting validation and its script mappers.
#[must_use = "a stream item must be finished on the script thread"]
pub struct Arrival {
    value: Dynamic,
    mapper: crate::Shared<std::sync::Mutex<Mapper>>,
}

impl Arrival {
    /// Validate the item and run the mapper chain. Call this on the thread
    /// that owns the script runtime. `None` means a `filter_map` dropped it.
    pub fn finish(self) -> Result<Option<Dynamic>, Error> {
        // A Rust value becomes a Rawr value here, on the script thread.
        let value = match self.value.try_cast_result::<host::runtime::Deferred>() {
            Ok(deferred) => deferred.resolve().map_err(|message| Error::Native {
                message: message.into(),
            })?,
            Err(value) => value,
        };
        let mut mapper = self
            .mapper
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (mapper)(value)
    }
}

impl fmt::Debug for Arrival {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Arrival(<private>)")
    }
}

/// An exact bound owner could not issue or consume a symbolic stream.
#[derive(Debug)]
pub enum Error {
    /// The compiler-selected stream has no matching exact runtime binding.
    Binding(host::runtime::Error),
    /// Durable arguments did not match the stream operation's exact arity.
    Arity {
        /// Canonical stream operation path.
        operation: ImmutableString,
        /// Declared argument count.
        expected: usize,
        /// Supplied argument count.
        actual: usize,
    },
    /// One durable argument failed its exact input proof.
    Input {
        /// Canonical stream operation path.
        operation: ImmutableString,
        /// Zero-based declared parameter index.
        index: usize,
        /// Exact runtime value failure.
        source: host::runtime::ValueError,
    },
    /// One argument's deterministic identity encoder failed before start.
    IdentityEncoding {
        /// Canonical stream operation path.
        operation: ImmutableString,
        /// Zero-based declared parameter index.
        index: usize,
        /// Stable encoding failure.
        source: identity::Error,
    },
    /// Canonical semantic proof material could not be represented.
    IdentityProof(crate::preview::digest::Error),
    /// One normalized stream item failed its exact output proof.
    Output {
        /// Canonical stream operation path.
        operation: ImmutableString,
        /// Exact runtime value failure.
        source: host::runtime::ValueError,
    },
    /// A symbolic mapper could not rebind or execute under the issuer.
    Mapper(program::Error),
    /// A mapped item failed the issuer's exact Message proof.
    Message {
        /// Stable final mapper identity, when a mapper exists.
        mapper: Option<item::Key>,
        /// Exact runtime value failure.
        source: host::runtime::ValueError,
    },
    /// The issuing application or preview Message lacks an exact runtime proof.
    MessageContract {
        /// Human-readable Message contract.
        message: ImmutableString,
    },
    /// A Rust stream's item could not become a Rawr value.
    Native {
        /// Why the item could not cross.
        message: ImmutableString,
    },
}

pub(crate) struct Request {
    call: Call,
    mappers: Box<[item::Value]>,
    filters: Box<[bool]>,
    message: host::runtime::type_ref::Type,
}

impl Version {
    /// Declare a canonical argument identity protocol version.
    #[must_use]
    pub const fn new(version: u32) -> Self {
        Self(version)
    }

    /// Return the exact declared protocol version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Proof {
    pub(crate) fn new(
        owner: host::facade::Dependency,
        definition: &host::item::Definition,
        inputs: Box<[host::runtime::type_ref::Type]>,
        output: host::runtime::type_ref::Type,
    ) -> Option<Self> {
        let version = definition.stream_identity()?;
        Some(Self {
            owner,
            definition: definition.clone(),
            fingerprint: definition.fingerprint(),
            path: definition.path().into(),
            version,
            inputs,
            output,
        })
    }

    /// Return the canonical qualified stream operation path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the exact identity protocol version.
    #[must_use]
    pub const fn identity_version(&self) -> Version {
        self.version
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
            && definition.fingerprint() == self.fingerprint
            && definition.stream_identity() == Some(self.version))
        .then_some((&self.inputs, &self.output))
    }

    pub(crate) fn dependencies(&self) -> BTreeSet<host::facade::Dependency> {
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

    pub(crate) fn identity(
        &self,
        contributions: &[identity::Contribution],
        mappers: &[item::Value],
        filters: &[bool],
    ) -> Result<Identity, crate::preview::digest::Error> {
        let mut writer = crate::preview::digest::Writer::with_version(IDENTITY_VERSION);
        writer.tag(0x3100);
        writer.tag(0x3101);
        self.owner.write(&mut writer)?;
        writer.tag(0x3102);
        self.fingerprint.write(&mut writer)?;
        writer.tag(0x3103);
        writer.u32(self.version.get());
        writer.tag(0x3104);
        writer.count(contributions.len());
        for (index, (input, contribution)) in self.inputs.iter().zip(contributions).enumerate() {
            writer.tag(0x3110);
            writer.count(index);
            input.write(&mut writer)?;
            writer.bytes(contribution.bytes());
        }
        writer.tag(0x3105);
        writer.count(mappers.len());
        for mapper in mappers {
            mapper.write(&mut writer)?;
        }
        // A plan without `filter_map` keeps the identity it always had.
        if filters.iter().any(|filter| *filter) {
            writer.tag(0x3106);
            for filter in filters {
                writer.u32(u32::from(*filter));
            }
        }
        Ok(Identity(writer.finish().bytes()))
    }
}

impl Arguments {
    pub(crate) fn seal(values: Box<[Dynamic]>) -> Self {
        Self { values }
    }

    /// Return the number of exact positional arguments.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.values.len()
    }

    /// Return whether this call has no arguments.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Borrow one exact positional argument.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Dynamic> {
        self.values.get(index)
    }

    pub(crate) const fn values(&self) -> &[Dynamic] {
        &self.values
    }

    /// Consume the carrier into its exact positional values.
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

impl Plan {
    /// Construct an empty subscription plan.
    pub fn none(call: &host::runtime::Call<'_>) -> Result<Self, host::runtime::WitnessError> {
        Ok(Self {
            streams: Box::default(),
            witness: call.output_witness()?,
        })
    }

    pub(crate) fn stream(call: Call, witness: host::runtime::Witness) -> Self {
        Self {
            streams: vec![Planned {
                call,
                mappers: Box::default(),
                filters: Box::default(),
            }]
            .into_boxed_slice(),
            witness,
        }
    }

    /// Flatten exact subscription plans in source order.
    pub fn batch(
        plans: impl IntoIterator<Item = Self>,
        call: &host::runtime::Call<'_>,
    ) -> Result<Self, host::runtime::WitnessError> {
        Ok(Self {
            streams: plans
                .into_iter()
                .flat_map(|plan| plan.streams.into_vec())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            witness: call.output_witness()?,
        })
    }

    /// Append one exact symbolic mapper to every stream in this plan.
    pub fn map(
        self,
        mapper: item::Value,
        call: &host::runtime::Call<'_>,
    ) -> Result<Self, host::runtime::WitnessError> {
        self.push(mapper, false, call)
    }

    /// Append one exact symbolic mapper returning an `Option` to every
    /// stream in this plan; an item it maps to `None` is dropped.
    pub fn filter_map(
        self,
        mapper: item::Value,
        call: &host::runtime::Call<'_>,
    ) -> Result<Self, host::runtime::WitnessError> {
        self.push(mapper, true, call)
    }

    fn push(
        mut self,
        mapper: item::Value,
        filter: bool,
        call: &host::runtime::Call<'_>,
    ) -> Result<Self, host::runtime::WitnessError> {
        let witness = call.transform_witness(&self.witness)?;
        for stream in &mut self.streams {
            let mut mappers = stream.mappers.to_vec();
            mappers.push(mapper.clone());
            stream.mappers = mappers.into_boxed_slice();
            let mut filters = stream.filters.to_vec();
            filters.resize(stream.mappers.len() - 1, false);
            filters.push(filter);
            stream.filters = filters.into_boxed_slice();
        }
        self.witness = witness;
        Ok(self)
    }

    /// Return whether the plan contains no streams.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.streams.is_empty()
    }

    /// Borrow exact launch-local proof of the plan's logical output type.
    #[must_use]
    pub const fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }

    pub(crate) fn into_streams(self) -> Box<[Planned]> {
        self.streams
    }
}

impl Planned {
    pub(crate) fn into_request(self, message: host::runtime::type_ref::Type) -> Request {
        let mut filters = self.filters.into_vec();
        filters.resize(self.mappers.len(), false);
        Request {
            call: self.call,
            mappers: self.mappers,
            filters: filters.into_boxed_slice(),
            message,
        }
    }
}

impl Request {
    #[cfg(test)]
    pub(crate) fn new(
        call: Call,
        mappers: Box<[item::Value]>,
        message: host::runtime::type_ref::Type,
    ) -> Self {
        let filters = vec![false; mappers.len()].into_boxed_slice();
        Self {
            call,
            mappers,
            filters,
            message,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Proof,
        Arguments,
        Box<[item::Value]>,
        Box<[bool]>,
        host::runtime::type_ref::Type,
    ) {
        let (proof, arguments) = self.call.into_parts();
        (proof, arguments, self.mappers, self.filters, self.message)
    }
}

impl Identity {
    /// Return fixed-width canonical identity bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }
}

impl Recipe {
    pub(crate) fn new(identity: Identity, starter: Starter) -> Self {
        Self { identity, starter }
    }

    /// Return canonical identity without starting the native stream factory.
    #[must_use]
    pub const fn identity(&self) -> Identity {
        self.identity
    }

    /// Start the exact factory once and retain bound validation authority.
    #[must_use = "starting a stream returns its only execution authority"]
    pub fn start(self) -> Issued {
        (self.starter)()
    }
}

impl Issued {
    pub(crate) fn new(identity: Identity, stream: Native, mapper: Mapper) -> Self {
        Self {
            identity,
            stream,
            mapper: crate::Shared::new(std::sync::Mutex::new(mapper)),
        }
    }

    /// Return the canonical identity of this exact bound stream plan.
    #[must_use]
    pub const fn identity(&self) -> Identity {
        self.identity
    }
}

impl Stream for Issued {
    type Item = Arrival;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.stream.as_mut().poll_next(context) {
            Poll::Ready(Some(value)) => Poll::Ready(Some(Arrival {
                value,
                mapper: self.mapper.clone(),
            })),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl fmt::Display for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Identity({self})")
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

impl fmt::Debug for Plan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Plan(<private>)")
    }
}

impl fmt::Debug for Issued {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Issued(<private>)")
    }
}

impl fmt::Debug for Recipe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Recipe(<private>)")
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
                "stream operation `{operation}` received {actual} argument(s), expected {expected}"
            ),
            Self::Input {
                operation,
                index,
                source,
            } => write!(
                formatter,
                "stream operation `{operation}` received invalid argument {index}: {source}"
            ),
            Self::IdentityEncoding {
                operation,
                index,
                source,
            } => write!(
                formatter,
                "stream operation `{operation}` could not encode argument {index}: {source}"
            ),
            Self::IdentityProof(error) => error.fmt(formatter),
            Self::Output { operation, source } => write!(
                formatter,
                "stream operation `{operation}` yielded invalid output: {source}"
            ),
            Self::Mapper(error) => error.fmt(formatter),
            Self::Message { mapper, source } => match mapper {
                Some(mapper) => write!(
                    formatter,
                    "stream mapper `{mapper}` returned an invalid Message: {source}"
                ),
                None => write!(formatter, "stream yielded an invalid Message: {source}"),
            },
            Self::MessageContract { message } => write!(
                formatter,
                "stream issuer Message contract `{message}` has no exact runtime representation"
            ),
            Self::Native { message } => {
                write!(
                    formatter,
                    "a Rust stream's item could not cross into Rawr: {message}"
                )
            }
        }
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Input { source, .. }
            | Self::Output { source, .. }
            | Self::Message { source, .. } => Some(source),
            Self::IdentityEncoding { source, .. } => Some(source),
            Self::IdentityProof(error) => Some(error),
            Self::Mapper(error) => Some(error),
            Self::Arity { .. } | Self::MessageContract { .. } | Self::Native { .. } => None,
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for Error {
    fn source(&self) -> Option<&(dyn core_error::Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Input { source, .. }
            | Self::Output { source, .. }
            | Self::Message { source, .. } => Some(source),
            Self::IdentityEncoding { source, .. } => Some(source),
            Self::IdentityProof(error) => Some(error),
            Self::Mapper(error) => Some(error),
            Self::Arity { .. } | Self::MessageContract { .. } | Self::Native { .. } => None,
        }
    }
}

impl PartialEq for Proof {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.fingerprint == other.fingerprint
            && self.version == other.version
            && self.inputs == other.inputs
            && self.output == other.output
    }
}

impl Eq for Proof {}

impl PartialOrd for Proof {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Proof {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            &self.owner,
            &self.fingerprint,
            self.version,
            &self.inputs,
            &self.output,
        )
            .cmp(&(
                &other.owner,
                &other.fingerprint,
                other.version,
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
        writer.tag(0x3000);
        self.owner.write(writer)?;
        self.fingerprint.write(writer)?;
        writer.u32(self.version.get());
        writer.count(self.inputs.len());
        for input in &self.inputs {
            input.write(writer)?;
        }
        self.output.write(writer)
    }
}
