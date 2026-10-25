//! Origin-checked application launch proof.

use crate::{application, host, program};
use std::fmt;

/// A validated application contract bound to its exact compiler Engine.
#[derive(Clone)]
pub struct Launch {
    contract: super::descriptor::Contract,
    fingerprint: application::Fingerprint,
    authority: program::Authority,
}

impl fmt::Debug for Launch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Launch")
            .field("fingerprint", &self.fingerprint)
            .field("authority", &self.authority)
            .finish_non_exhaustive()
    }
}

impl Launch {
    pub(crate) fn new(
        contract: super::descriptor::Contract,
        fingerprint: application::Fingerprint,
        authority: program::Authority,
    ) -> Self {
        Self {
            contract,
            fingerprint,
            authority,
        }
    }

    /// Bind this origin-checked launch to exact native facade implementations.
    pub fn bind(
        self,
        runtime: host::runtime::Registry,
    ) -> Result<application::BoundLaunch, host::runtime::Error> {
        let mut projections = self.authority.projection_requirements().to_vec();
        if let Some(projection) = self.view().boundary_projection() {
            projections.push(projection.clone());
        }
        projections.sort();
        projections.dedup();
        let mut boundary_types = vec![self.view().runtime_output().clone()];
        boundary_types.extend(self.state().exact_runtime_type());
        boundary_types.extend(self.message().exact_runtime_type());
        boundary_types.extend(self.boot().task().and_then(application::Task::runtime_type));
        boundary_types.extend(self.update().task().runtime_type());
        boundary_types.extend(
            self.title()
                .map(application::Title::runtime_output)
                .cloned(),
        );
        boundary_types.extend(
            self.theme()
                .map(application::Theme::runtime_output)
                .cloned(),
        );
        boundary_types.extend(
            self.subscription()
                .map(application::Subscription::runtime_output)
                .cloned(),
        );
        runtime.validate(
            self.dependencies(),
            self.authority.runtime_requirements(),
            &projections,
            self.authority.widget_requirements(),
            &boundary_types,
        )?;
        Ok(application::BoundLaunch::new(
            self,
            crate::Shared::new(runtime),
        ))
    }

    /// Return the deterministic exact application proof.
    #[must_use]
    pub const fn fingerprint(&self) -> &application::Fingerprint {
        &self.fingerprint
    }

    /// Return every exact host-facade dependency required before execution.
    #[must_use]
    pub fn dependencies(&self) -> &[host::facade::Dependency] {
        &self.contract.proof.dependencies
    }

    /// Return the exact stable application state type.
    #[must_use]
    pub fn state(&self) -> &application::Type {
        &self.contract.proof.state
    }

    /// Return the exact stable application message type.
    #[must_use]
    pub fn message(&self) -> &application::Type {
        &self.contract.proof.message
    }

    /// Return the normalized boot contract.
    #[must_use]
    pub fn boot(&self) -> &application::Boot {
        &self.contract.proof.boot
    }

    /// Return `main`'s statements before the entry, when present.
    #[must_use]
    pub fn prelude(&self) -> Option<&application::Prelude> {
        self.contract.proof.prelude.as_ref()
    }

    /// Return the normalized update callback.
    #[must_use]
    pub fn update(&self) -> &application::Update {
        &self.contract.proof.update
    }

    /// Return the normalized view callback.
    #[must_use]
    pub fn view(&self) -> &application::View {
        &self.contract.proof.view
    }

    /// Return the optional normalized state-derived title callback.
    #[must_use]
    pub fn title(&self) -> Option<&application::Title> {
        self.contract.proof.title.as_ref()
    }

    /// Return the optional normalized state-derived theme callback.
    #[must_use]
    pub fn theme(&self) -> Option<&application::Theme> {
        self.contract.proof.theme.as_ref()
    }

    /// Return the optional normalized state-derived subscription callback.
    #[must_use]
    pub fn subscription(&self) -> Option<&application::Subscription> {
        self.contract.proof.subscription.as_ref()
    }

    /// Return whether the native application requests centered initial placement.
    #[must_use]
    pub fn centered(&self) -> bool {
        self.contract.proof.centered
    }

    /// Return every compiler-validated local preview in stable key order.
    #[must_use]
    pub fn previews(&self) -> &[crate::preview::Definition] {
        self.authority.previews()
    }

    /// Return the exact origin-checked program authority.
    #[must_use]
    pub const fn authority(&self) -> &program::Authority {
        &self.authority
    }
}

impl application::Boot {
    fn task(&self) -> Option<&application::Task> {
        match self {
            Self::Default => None,
            Self::Explicit { task, .. } => Some(task),
        }
    }
}
