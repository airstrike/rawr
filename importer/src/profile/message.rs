//! Generation-authenticated bounded profile messages.

/// Monotonic per-route message sequence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Sequence(u64);

/// A bounded profile control message codec.
pub trait Message: Sized {
    fn encode(&self, output: &mut crate::protocol::Writer<'_>) -> Result<(), Error>;
    fn decode(input: &mut crate::protocol::Reader<'_>) -> Result<Self, Error>;
}

/// One message bound to an exact generation, receipt, and sequence.
pub struct Envelope<M> {
    route: super::activation::Route,
    sequence: Sequence,
    message: M,
}

impl Sequence {
    pub(crate) const fn first() -> Self {
        Self(1)
    }

    pub(crate) fn next(self) -> Result<Self, Error> {
        self.0.checked_add(1).map(Self).ok_or(Error::Exhausted)
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

impl<M> Envelope<M> {
    pub(crate) const fn new(
        route: super::activation::Route,
        sequence: Sequence,
        message: M,
    ) -> Self {
        Self {
            route,
            sequence,
            message,
        }
    }

    /// Return the exact generation and capability receipt.
    #[must_use]
    pub const fn route(&self) -> super::activation::Route {
        self.route
    }

    /// Return the exact route-local sequence.
    #[must_use]
    pub const fn sequence(&self) -> Sequence {
        self.sequence
    }

    /// Borrow the decoded semantic message.
    #[must_use]
    pub const fn message(&self) -> &M {
        &self.message
    }

    /// Consume the envelope into its semantic message.
    pub fn into_message(self) -> M {
        self.message
    }
}

impl<M: Message> Envelope<M> {
    pub(crate) fn encode(self) -> Result<Box<[u8]>, Error> {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(72).map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(self.route.key().as_bytes());
        bytes.extend_from_slice(self.route.receipt().as_bytes());
        bytes.extend_from_slice(&self.sequence.get().to_le_bytes());
        let mut output = crate::protocol::Writer::new(&mut bytes);
        self.message.encode(&mut output)?;
        Ok(bytes.into_boxed_slice())
    }

    pub(crate) fn decode(
        route: super::activation::Route,
        sequence: Sequence,
        bytes: &[u8],
    ) -> Result<Self, Error> {
        if bytes.len() < 72
            || &bytes[..32] != route.key().as_bytes()
            || &bytes[32..64] != route.receipt().as_bytes()
            || bytes[64..72] != sequence.get().to_le_bytes()
        {
            return Err(Error::Stale);
        }
        let mut input = crate::protocol::Reader::new(&bytes[72..]);
        let message = M::decode(&mut input)?;
        if !input.is_empty() {
            return Err(Error::Trailing);
        }
        Ok(Self::new(route, sequence, message))
    }
}

/// Profile message encoding or authentication failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("profile message allocation failed")]
    Allocation,
    #[error("profile message exceeded the runner frame limit")]
    Limit,
    #[error("profile message was truncated or malformed")]
    Malformed,
    #[error("profile message contains trailing bytes")]
    Trailing,
    #[error("profile message belongs to a stale generation, receipt, or sequence")]
    Stale,
    #[error("profile message sequence is exhausted")]
    Exhausted,
}
