//! Receiver modes for resolved inherent methods.

/// The access a method receives to its nominal owner value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Mode {
    /// A `self` receiver consumes its owner value.
    Owned,
    /// An `&self` receiver may observe but not mutate its owner.
    Shared,
    /// An `&mut self` receiver may observe and mutate its owner.
    Mutable,
}
