//! Closed callable lifetime and ownership proofs for demanded adapters.

use serde::{Deserialize, Serialize};

/// One supported native callable class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Class {
    /// An input-only borrow that cannot escape the native invocation.
    Borrowed(borrowed::Proof),
    /// An owned immutable callable scoped to an epoch or shorter region.
    Shared(shared::Proof),
    /// An owned mutable callable retired with its epoch.
    Mutable(mutable::Proof),
    /// An owned callable with exactly one successful invocation.
    Once(once::Proof),
}

/// Callable invocation failure.
#[derive(Clone, Copy, Debug, Eq, thiserror::Error, PartialEq)]
pub enum Error {
    /// The one-shot callable was already consumed.
    #[error("callable has already been consumed")]
    Consumed,
}

/// Input-only borrowed callable evidence.
pub mod borrowed {
    use serde::{Deserialize, Serialize};

    /// Proof that every borrow is input-only and nonescaping.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
    pub struct Proof {
        late_bound: bool,
    }

    impl Proof {
        pub(crate) const fn input_only(late_bound: bool) -> Self {
            Self { late_bound }
        }

        /// Whether this callable universally quantifies an input lifetime.
        #[must_use]
        pub const fn is_late_bound(self) -> bool {
            self.late_bound
        }
    }
}

/// Owned immutable callable evidence.
pub mod shared {
    use serde::{Deserialize, Serialize};

    /// The native lifetime of an owned immutable callable.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
    pub enum Scope {
        /// The callable retires with the runner epoch.
        Epoch,
        /// The callable retires at the end of one profile-owned native region.
        Region,
    }

    /// Proof that a callable is owned, immutable, and `'static`.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
    pub struct Proof {
        scope: Scope,
    }

    impl Proof {
        pub(crate) const fn static_owned(scope: Scope) -> Self {
            Self { scope }
        }

        /// Return the native scope that owns the callable.
        #[must_use]
        pub const fn scope(self) -> Scope {
            self.scope
        }
    }
}

/// Owned mutable callable evidence.
pub mod mutable {
    use serde::{Deserialize, Serialize};

    /// Proof that a mutable callable is owned, `'static`, and epoch-scoped.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
    pub struct Proof {
        _private: (),
    }

    impl Proof {
        pub(crate) const fn static_owned() -> Self {
            Self { _private: () }
        }
    }
}

/// Consumed callable evidence and runner-local state.
pub mod once {
    use serde::{Deserialize, Serialize};

    /// Proof that a callable is owned and may be invoked once.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
    pub struct Proof {
        _private: (),
    }

    impl Proof {
        pub(crate) const fn static_owned() -> Self {
            Self { _private: () }
        }
    }

    /// Runner-local state that deterministically rejects a second call.
    pub struct Ready<F>(Option<F>);

    impl<F> Ready<F> {
        /// Seal one native callable into its unconsumed state.
        #[must_use]
        pub const fn new(function: F) -> Self {
            Self(Some(function))
        }

        /// Consume and invoke the function exactly once.
        pub fn call<A, O>(&mut self, argument: A) -> Result<O, super::Error>
        where
            F: FnOnce(A) -> O,
        {
            let function = self.0.take().ok_or(super::Error::Consumed)?;
            Ok(function(argument))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{once, Error};

    #[test]
    fn once_state_rejects_a_second_call_without_panicking() {
        let mut ready = once::Ready::new(|value: i64| value + 1);
        assert_eq!(ready.call(1), Ok(2));
        assert_eq!(ready.call(1), Err(Error::Consumed));
    }
}
