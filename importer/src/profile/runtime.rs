//! Framework-neutral staged profile runtime contract.

/// Read-only evidence available during side-effect-free preparation.
pub struct Prepare<'a> {
    key: crate::supervisor::GenerationKey,
    snapshot: crate::surface::Snapshot,
    profile: &'a super::Proof,
}

/// Framework profile lifecycle confined to one runner epoch.
pub trait Runtime: Send + 'static {
    type State;
    type Prepared;
    type Activated;

    fn prepare(&mut self, context: Prepare<'_>) -> Result<Self::Prepared, super::Error>;

    fn activate(
        &mut self,
        prepared: Self::Prepared,
        route: super::activation::Route,
    ) -> Result<Self::Activated, super::Error>;

    fn admit(
        &mut self,
        activated: Self::Activated,
        route: super::activation::Route,
    ) -> Result<Self::State, super::Error>;

    fn retire(&mut self, state: Self::State);
}

impl Prepare<'_> {
    /// Bind read-only preparation context to already-sealed generation evidence.
    #[doc(hidden)]
    pub const fn from_generation(
        key: crate::supervisor::GenerationKey,
        snapshot: crate::surface::Snapshot,
        profile: &super::Proof,
    ) -> Prepare<'_> {
        Prepare {
            key,
            snapshot,
            profile,
        }
    }

    /// Return the complete generation identity.
    #[must_use]
    pub const fn key(&self) -> crate::supervisor::GenerationKey {
        self.key
    }

    /// Return the exact Rawr snapshot identity.
    #[must_use]
    pub const fn snapshot(&self) -> crate::surface::Snapshot {
        self.snapshot
    }

    /// Borrow the exact generated profile proof.
    #[must_use]
    pub const fn profile(&self) -> &super::Proof {
        self.profile
    }
}
