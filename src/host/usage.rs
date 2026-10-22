//! Compile-time policy for values discarded by expression statements.

/// Whether a host value must be consumed instead of silently discarded.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Policy {
    /// Discarding the value has no framework significance.
    #[default]
    Ordinary,
    /// A discarded value is diagnosed unless its exact producer permits it.
    Required,
}

/// Whether one exact host item permits its produced value to be discarded.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Outcome {
    /// The output follows its type's normal usage policy.
    #[default]
    Conservative,
    /// The exact producer's output may be discarded.
    Discardable,
}

/// Whether a discarded exact producer invocation may be omitted.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Evaluation {
    /// The producer still executes when its output is discarded.
    #[default]
    Required,
    /// The facade promises the complete producer expression is inert.
    Elidable,
}
