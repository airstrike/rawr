//! Core-owned atomic installation of generation, binding, epoch, and host state.

use std::fmt;
use std::sync::Arc;

use crate::reload;

use super::{installed, Binding};

type Admission<T> = Result<(Box<[reload::compatibility::Evidence]>, Retired<T>), Rejected<T>>;

/// One exact installed generation and its complete authority-free host state.
///
/// Candidate admission accepts no downstream commit callback.
///
/// ```compile_fail
/// # use rawr::reload::installation::current::{Prepared, Slot};
/// fn bypass<T>(slot: &mut Slot<T>, prepared: Prepared<T>) {
///     let _ = slot.admit(prepared, |_state, _installed| ());
/// }
/// ```
#[doc(hidden)]
pub struct Slot<T> {
    identity: Stamp,
    installed: Installed<T>,
}

/// One complete candidate state sealed for direct slot replacement.
///
/// ```compile_fail
/// # use rawr::reload::installation::current::Prepared;
/// fn extract<T>(prepared: Prepared<T>) -> T {
///     prepared.state
/// }
/// ```
#[doc(hidden)]
pub struct Prepared<T> {
    identity: Stamp,
    epoch: Stamp,
    retained: Arc<reload::Generation>,
    generation: Arc<reload::Generation>,
    binding: Binding,
    evidence: Box<[reload::compatibility::Evidence]>,
    state: T,
}

/// One retired installed bundle retained until every outer gate has exited.
#[doc(hidden)]
pub struct Retired<T>(Installed<T>);

/// One rejected candidate bundle retained until every outer gate has exited.
#[doc(hidden)]
pub struct Rejected<T>(Prepared<T>);

/// One mutation borrow that rotated the slot epoch before exposing state.
#[doc(hidden)]
pub struct Mutation<'a, T>(&'a mut Installed<T>);

pub(super) struct Proof {
    identity: Stamp,
    epoch: Stamp,
}

struct Installed<T> {
    generation: Arc<reload::Generation>,
    binding: Binding,
    epoch: Stamp,
    state: T,
}

#[derive(Clone, Debug)]
struct Stamp(Arc<()>);

impl<T> Slot<T> {
    pub(super) fn new(generation: Arc<reload::Generation>, binding: Binding, state: T) -> Self {
        Self {
            identity: Stamp::new(),
            installed: Installed {
                generation,
                binding,
                epoch: Stamp::new(),
                state,
            },
        }
    }

    /// Return the exact immutable current-generation metadata proof.
    #[must_use]
    pub fn generation(&self) -> Arc<reload::Generation> {
        Arc::clone(&self.installed.generation)
    }

    /// Borrow the complete authority-free current host state.
    #[must_use]
    pub const fn state(&self) -> &T {
        &self.installed.state
    }

    /// Borrow authority minted only from this already-installed slot.
    #[must_use]
    pub fn context(&self) -> installed::Context<'_> {
        installed::Context::new(&self.installed.binding)
    }

    /// Borrow state mutably after revoking every earlier preparation.
    pub fn mutation(&mut self) -> Mutation<'_, T> {
        self.installed.epoch.rotate();
        Mutation(&mut self.installed)
    }

    /// Admit one complete candidate bundle by direct atomic slot replacement.
    pub(in crate::reload) fn admit(&mut self, prepared: Prepared<T>) -> Admission<T> {
        if !prepared.identity.matches(&self.identity)
            || !prepared.epoch.matches(&self.installed.epoch)
            || !Arc::ptr_eq(&prepared.retained, &self.installed.generation)
        {
            return Err(Rejected(prepared));
        }
        let Prepared {
            generation,
            binding,
            evidence,
            state,
            ..
        } = prepared;
        let retired = std::mem::replace(
            &mut self.installed,
            Installed {
                generation,
                binding,
                epoch: Stamp::new(),
                state,
            },
        );
        Ok((evidence, Retired(retired)))
    }

    pub(in crate::reload) fn reject(prepared: Prepared<T>) -> Rejected<T> {
        Rejected(prepared)
    }

    pub(super) fn retains(&self, retained: &Arc<reload::Generation>) -> bool {
        Arc::ptr_eq(retained, &self.installed.generation)
    }

    pub(super) fn proof(&self) -> Proof {
        Proof {
            identity: self.identity.clone(),
            epoch: self.installed.epoch.clone(),
        }
    }

    pub(super) fn seal(
        &self,
        proof: Proof,
        retained: Arc<reload::Generation>,
        generation: Arc<reload::Generation>,
        binding: Binding,
        evidence: Box<[reload::compatibility::Evidence]>,
        state: T,
    ) -> Prepared<T> {
        Prepared {
            identity: proof.identity,
            epoch: proof.epoch,
            retained,
            generation,
            binding,
            evidence,
            state,
        }
    }
}

impl<T> Mutation<'_, T> {
    /// Split this rotating guard into mutable state and installed authority.
    #[must_use]
    pub fn parts(&mut self) -> (&mut T, installed::Context<'_>) {
        (&mut self.0.state, installed::Context::new(&self.0.binding))
    }
}

impl<T> std::ops::Deref for Mutation<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0.state
    }
}

impl<T> std::ops::DerefMut for Mutation<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.state
    }
}

impl<T> Retired<T> {
    /// Release retired authority and return old host state after every gate exits.
    #[must_use]
    pub fn release(self) -> T {
        let Installed {
            generation,
            binding,
            state,
            ..
        } = self.0;
        drop(generation);
        drop(binding);
        state
    }
}

impl<T> Rejected<T> {
    /// Release rejected authority and return candidate state after every gate exits.
    #[must_use]
    pub fn release(self) -> T {
        let Prepared { binding, state, .. } = self.0;
        drop(binding);
        state
    }
}

impl Stamp {
    fn new() -> Self {
        Self(Arc::new(()))
    }

    fn rotate(&mut self) {
        self.0 = Arc::new(());
    }

    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> fmt::Debug for Slot<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Slot(<private>)")
    }
}

impl<T> fmt::Debug for Prepared<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Prepared(<private>)")
    }
}

impl<T> fmt::Debug for Retired<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Retired(<private>)")
    }
}

impl<T> fmt::Debug for Rejected<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Rejected(<private>)")
    }
}

impl<T> fmt::Debug for Mutation<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Mutation(<private>)")
    }
}
