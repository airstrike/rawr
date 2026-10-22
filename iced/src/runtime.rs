//! Native message envelopes carrying exact application provenance.

use std::sync::Arc;

use rawr::{application, item, program, Dynamic};

use crate::{effect, subscription, task};

/// One Rawr message issued by a particular application instance and generation.
///
/// Ordinary host code cannot construct or retag an envelope from copied
/// identities. Native widget descriptors receive the current stamping
/// authority through [`crate::widget::Context::message`].
///
/// ```compile_fail
/// use rawr::{application, program, Dynamic};
/// use rawr_iced::runtime::Message;
///
/// fn forge(
///     instance: application::Instance,
///     generation: program::GenerationId,
///     payload: Dynamic,
/// ) -> Message {
///     Message::new(instance, generation, payload)
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Message {
    instance: application::Instance,
    generation: program::GenerationId,
    payload: Dynamic,
    mapping_error: Option<Arc<program::Error>>,
    task_error: Option<Arc<task::Error>>,
    subscription_error: Option<Arc<subscription::Error>>,
    subscription: bool,
    task_completion: Option<effect::Completion>,
}

impl Message {
    #[must_use = "the exact message envelope must be delivered"]
    pub(crate) fn new(
        instance: application::Instance,
        generation: program::GenerationId,
        payload: Dynamic,
    ) -> Self {
        Self {
            instance,
            generation,
            payload,
            mapping_error: None,
            task_error: None,
            subscription_error: None,
            subscription: false,
            task_completion: None,
        }
    }

    pub(crate) fn task_error(
        instance: application::Instance,
        generation: program::GenerationId,
        error: task::Error,
    ) -> Self {
        Self {
            instance,
            generation,
            payload: Dynamic::UNIT,
            mapping_error: None,
            task_error: Some(Arc::new(error)),
            subscription_error: None,
            subscription: false,
            task_completion: None,
        }
    }

    pub(crate) fn subscription(
        instance: application::Instance,
        generation: program::GenerationId,
        payload: Dynamic,
    ) -> Self {
        Self {
            instance,
            generation,
            payload,
            mapping_error: None,
            task_error: None,
            subscription_error: None,
            subscription: true,
            task_completion: None,
        }
    }

    pub(crate) fn subscription_error(
        instance: application::Instance,
        generation: program::GenerationId,
        error: subscription::Error,
    ) -> Self {
        Self {
            instance,
            generation,
            payload: Dynamic::UNIT,
            mapping_error: None,
            task_error: None,
            subscription_error: Some(Arc::new(error)),
            subscription: true,
            task_completion: None,
        }
    }

    pub(crate) fn task_completed(completion: effect::Completion) -> Self {
        let owner = completion.owner();
        Self {
            instance: owner.instance(),
            generation: owner.generation(),
            payload: Dynamic::UNIT,
            mapping_error: None,
            task_error: None,
            subscription_error: None,
            subscription: false,
            task_completion: Some(completion),
        }
    }

    /// Return the issuing application instance.
    #[must_use]
    pub const fn instance(&self) -> application::Instance {
        self.instance
    }

    /// Return the issuing executable generation.
    #[must_use]
    pub const fn generation(&self) -> program::GenerationId {
        self.generation
    }

    /// Return the opaque Rawr payload.
    #[must_use = "the opaque payload belongs to this exact message envelope"]
    pub const fn payload(&self) -> &Dynamic {
        &self.payload
    }

    pub(crate) fn into_payload(self) -> Dynamic {
        self.payload
    }

    pub(crate) fn mapping_error(&self) -> Option<&Arc<program::Error>> {
        self.mapping_error.as_ref()
    }

    pub(crate) fn task_failure(&self) -> Option<&Arc<task::Error>> {
        self.task_error.as_ref()
    }

    pub(crate) fn subscription_failure(&self) -> Option<&Arc<subscription::Error>> {
        self.subscription_error.as_ref()
    }

    pub(crate) const fn is_subscription(&self) -> bool {
        self.subscription
    }

    pub(crate) const fn task_completion(&self) -> Option<&effect::Completion> {
        self.task_completion.as_ref()
    }

    pub(crate) fn map_application(
        mut self,
        authority: &rawr::reload::installation::installed::Application,
        mapper: &item::Value,
    ) -> Self {
        if self.mapping_error.is_some()
            || self.task_error.is_some()
            || self.subscription_error.is_some()
            || self.task_completion.is_some()
        {
            return self;
        }
        match authority.map_message(mapper, self.payload.take()) {
            Ok(payload) => self.payload = payload,
            Err(error) => self.mapping_error = Some(Arc::new(error)),
        }
        self
    }

    pub(crate) fn map_application_intermediate(
        mut self,
        authority: &rawr::reload::installation::installed::Application,
        mapper: &item::Value,
    ) -> Self {
        if self.mapping_error.is_some()
            || self.task_error.is_some()
            || self.subscription_error.is_some()
            || self.task_completion.is_some()
        {
            return self;
        }
        match authority.invoke(mapper, &mut [self.payload.take()]) {
            Ok(payload) => self.payload = payload,
            Err(error) => self.mapping_error = Some(Arc::new(error)),
        }
        self
    }

    pub(crate) fn map_preview(
        mut self,
        catalog: &crate::preview::catalog::Authority,
        key: &rawr::preview::Key,
        mapper: &item::Value,
    ) -> Self {
        if self.mapping_error.is_some()
            || self.task_error.is_some()
            || self.subscription_error.is_some()
            || self.task_completion.is_some()
        {
            return self;
        }
        let previous = self.payload.clone();
        let mut arguments = [self.payload.take()];
        match catalog.map_message(key, mapper, arguments[0].take()) {
            Ok(payload) => self.payload = payload,
            Err(rawr::preview::RuntimeError::Invocation(error)) => {
                self.mapping_error = Some(Arc::new(error));
            }
            Err(error) => {
                self.payload = previous;
                debug_assert!(
                    false,
                    "compiler-proven preview mapping reached an invalid catalog boundary: {error}"
                );
            }
        }
        self
    }

    pub(crate) fn map_preview_intermediate(
        mut self,
        catalog: &crate::preview::catalog::Authority,
        key: &rawr::preview::Key,
        mapper: &item::Value,
    ) -> Self {
        if self.mapping_error.is_some()
            || self.task_error.is_some()
            || self.subscription_error.is_some()
            || self.task_completion.is_some()
        {
            return self;
        }
        let previous = self.payload.clone();
        match catalog.map_task_value(key, mapper, self.payload.take()) {
            Ok(payload) => self.payload = payload,
            Err(rawr::preview::RuntimeError::Invocation(error)) => {
                self.mapping_error = Some(Arc::new(error));
            }
            Err(error) => {
                self.payload = previous;
                debug_assert!(
                    false,
                    "compiler-proven intermediate preview mapping reached an invalid catalog boundary: {error}"
                );
            }
        }
        self
    }
}
