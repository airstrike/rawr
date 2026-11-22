//! Structured generated-adapter invocation failures.

/// A runner-reported typed thunk rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Rejection {
    #[error("adapter argument is missing")]
    MissingArgument,
    #[error("adapter argument or result type does not match")]
    TypeMismatch,
    #[error("native value handle is stale")]
    StaleHandle,
    #[error("runner native arena is full")]
    ArenaFull,
    #[error("runner adapter allocation failed")]
    Allocation,
    #[error("adapter invocation payload is malformed")]
    Malformed,
    #[error("adapter argument cardinality does not match")]
    Cardinality,
    #[error("adapter identity or signature is not in the compiled table")]
    MissingAdapter,
    #[error("adapter invocation belongs to a stale generation")]
    Stale,
    #[error("a crate's value was reached on a thread that holds none")]
    NoArena,
    #[error("adapter output has no witness of its type")]
    Unwitnessed,
    #[error("a crate value's method ran without its handle")]
    Unhandled,
    #[error("a lent arena was missing inside its own lend")]
    Unlent,
}

/// Local invocation encoding or validation failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("adapter invocation allocation failed")]
    Allocation,
    #[error("adapter invocation exceeded the runner frame limit")]
    Limit,
    #[error("adapter invocation payload is malformed")]
    Malformed,
    #[error("adapter expects {expected} arguments, received {actual}")]
    Cardinality { expected: usize, actual: usize },
}

impl Rejection {
    pub(crate) const fn decode(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::MissingArgument,
            2 => Self::TypeMismatch,
            3 => Self::StaleHandle,
            4 => Self::ArenaFull,
            5 => Self::Allocation,
            6 => Self::Malformed,
            7 => Self::Cardinality,
            8 => Self::MissingAdapter,
            9 => Self::Stale,
            17 => Self::NoArena,
            18 => Self::Unwitnessed,
            19 => Self::Unhandled,
            20 => Self::Unlent,
            _ => return None,
        })
    }
}
