//! Application contracts promoted with exact native facade authority.

#[cfg(not(feature = "no_function"))]
use crate::item;
use crate::{application, host, program, Dynamic, Shared};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// A launch whose complete native facade requirements have been proven.
#[derive(Clone)]
pub struct BoundLaunch {
    launch: application::Launch,
    #[cfg_attr(feature = "no_function", allow(dead_code))]
    runtime: Shared<host::runtime::Registry>,
}

/// Runtime application state validation or default construction failed.
#[derive(Debug)]
pub enum RuntimeError {
    /// Explicit boot invocation was requested for a Default-backed application.
    DefaultBoot,
    /// Default construction was requested for an explicit-boot application.
    ExplicitBoot,
    /// The executable-owned runtime model rejected State construction.
    StateConstruction(program::construction::Error),
    /// Exact launch-bound validation rejected an application State value.
    State(host::runtime::ValueError),
    /// The executable-owned runtime model rejected a delivered Message.
    Message(host::runtime::ValueError),
    /// The explicit boot callback failed under the bound authority.
    Invocation(program::Error),
    /// The explicit boot callback did not return its sealed current envelope.
    Startup(application::startup::Error),
    /// The normalized Task contract had no exact runtime representation.
    TaskRepresentation {
        /// Human-readable Task contract.
        task: String,
    },
    /// The explicit boot Task failed exact runtime validation.
    Task(program::Error),
}

impl BoundLaunch {
    /// Resolve one non-authoritative generated event-mapper seal against this
    /// exact application generation.
    #[doc(hidden)]
    pub fn profile_event_mapper(&self, seal: &[u8; 16]) -> Option<item::Value> {
        self.launch.authority().items().find_map(|item| {
            (crate::preview::digest::event_mapper(item).ok().as_ref() == Some(seal))
                .then(|| item.clone())
        })
    }

    pub(crate) fn new(
        launch: application::Launch,
        runtime: Shared<host::runtime::Registry>,
    ) -> Self {
        Self { launch, runtime }
    }

    /// Return the deterministic exact application proof.
    #[must_use]
    pub const fn fingerprint(&self) -> &application::Fingerprint {
        self.launch.fingerprint()
    }

    /// Return every exact host-facade dependency proven for this launch.
    #[must_use]
    pub fn dependencies(&self) -> &[host::facade::Dependency] {
        self.launch.dependencies()
    }

    /// Return the exact stable application state type.
    #[must_use]
    pub fn state(&self) -> &application::Type {
        self.launch.state()
    }

    /// Return the exact stable application message type.
    #[must_use]
    pub fn message(&self) -> &application::Type {
        self.launch.message()
    }

    /// Return the normalized boot contract.
    #[must_use]
    pub fn boot(&self) -> &application::Boot {
        self.launch.boot()
    }

    /// Return the normalized update callback.
    #[must_use]
    pub fn update(&self) -> &application::Update {
        self.launch.update()
    }

    /// Return the normalized view callback.
    #[must_use]
    pub fn view(&self) -> &application::View {
        self.launch.view()
    }

    /// Return the optional normalized state-derived title callback.
    #[must_use]
    pub fn title(&self) -> Option<&application::Title> {
        self.launch.title()
    }

    /// Return the optional normalized state-derived theme callback.
    #[must_use]
    pub fn theme(&self) -> Option<&application::Theme> {
        self.launch.theme()
    }

    /// Return the optional normalized state-derived subscription callback.
    #[must_use]
    pub fn subscription(&self) -> Option<&application::Subscription> {
        self.launch.subscription()
    }

    /// Return whether the native application requests centered initial placement.
    #[must_use]
    pub fn centered(&self) -> bool {
        self.launch.centered()
    }

    /// Return every compiler-validated local preview in stable key order.
    #[must_use]
    pub fn previews(&self) -> &[crate::preview::Definition] {
        self.launch.previews()
    }

    /// Return every compiler-selected nominal-widget promotion site.
    pub fn widget_promotions(&self) -> &[crate::widget::Promotion] {
        self.launch.authority().widget_promotions()
    }

    /// Resolve one compiler-proven application callback to exact retained declaration evidence.
    ///
    /// A callback from another executable has no source in this launch.
    #[must_use]
    pub fn callback_evidence(
        &self,
        callback: &application::Callback,
    ) -> Option<application::callback::Evidence<'_>> {
        self.launch
            .authority()
            .item_evidence(callback.item())
            .map(application::callback::Evidence::new)
    }

    /// The crate's free function `name`, as a symbolic item.
    #[must_use]
    pub fn function(&self, name: &str) -> Option<item::Value> {
        self.launch
            .authority()
            .items()
            .find(|item| {
                item.kind() == item::Kind::Function
                    && item.key().owner().is_none()
                    && item.key().name() == name
            })
            .cloned()
    }

    /// Return this launch's immutable executable generation.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.launch.authority().generation()
    }

    /// Invoke a symbolic item with this launch's exact native facade authority.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke(
        &self,
        retained: &item::Value,
        args: &mut [Dynamic],
    ) -> Result<Dynamic, program::Error> {
        if let application::Boot::Explicit { callback, .. } = self.boot()
            && retained.key() == callback.item().key()
        {
            return Err(program::Error::ApplicationBoundary {
                key: retained.key().clone(),
                boundary: "application boot",
            });
        }
        self.launch
            .authority()
            .invoke_bound(retained, args, self.runtime.clone())
    }

    /// Invoke the exact view callback and normalize its authenticated output.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke_view(&self, args: &mut [Dynamic]) -> Result<Dynamic, program::Error> {
        let callback = self.view().callback().item();
        let value = self
            .launch
            .authority()
            .invoke_bound(callback, args, self.runtime.clone())?;
        match self.view().boundary_projection() {
            Some(projection) => self.launch.authority().project_bound(
                callback,
                projection,
                value,
                self.runtime.clone(),
            ),
            None => {
                self.launch.authority().validate_bound_type(
                    callback,
                    self.view().runtime_output(),
                    &value,
                    &self.runtime,
                )?;
                Ok(value)
            }
        }
    }

    /// Invoke and validate the exact state-derived title callback.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke_title(&self, args: &mut [Dynamic]) -> Result<Option<Dynamic>, program::Error> {
        let Some(title) = self.title() else {
            return Ok(None);
        };
        let callback = title.callback().item();
        let value = self
            .launch
            .authority()
            .invoke_bound(callback, args, self.runtime.clone())?;
        self.launch.authority().validate_bound_type(
            callback,
            title.runtime_output(),
            &value,
            &self.runtime,
        )?;
        Ok(Some(value))
    }

    /// Invoke and validate the exact state-derived theme callback.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke_theme(&self, args: &mut [Dynamic]) -> Result<Option<Dynamic>, program::Error> {
        let Some(theme) = self.theme() else {
            return Ok(None);
        };
        let callback = theme.callback().item();
        let value = self
            .launch
            .authority()
            .invoke_bound(callback, args, self.runtime.clone())?;
        self.launch.authority().validate_bound_type(
            callback,
            theme.runtime_output(),
            &value,
            &self.runtime,
        )?;
        Ok(Some(value))
    }

    /// Invoke and validate the exact state-derived subscription callback.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke_subscription(
        &self,
        args: &mut [Dynamic],
    ) -> Result<Option<Dynamic>, program::Error> {
        let Some(subscription) = self.subscription() else {
            return Ok(None);
        };
        let callback = subscription.callback().item();
        let value = self
            .launch
            .authority()
            .invoke_bound(callback, args, self.runtime.clone())?;
        self.launch.authority().validate_bound_type(
            callback,
            subscription.runtime_output(),
            &value,
            &self.runtime,
        )?;
        Ok(Some(value))
    }

    /// Invoke and validate the exact explicit boot callback.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke_boot(&self) -> Result<application::Startup, RuntimeError> {
        let application::Boot::Explicit { callback, task } = self.boot() else {
            return Err(RuntimeError::DefaultBoot);
        };
        let output = self
            .launch
            .authority()
            .invoke_bound(callback.item(), &mut [], self.runtime.clone())
            .map_err(RuntimeError::Invocation)?;

        match task {
            application::Task::None => {
                self.validate_state(&output)?;
                Ok(application::Startup::new(output, None))
            }
            application::Task::Facade(ty) => {
                let (state, task) = application::startup::Envelope::open(output, self.generation())
                    .map_err(RuntimeError::Startup)?;
                self.validate_state(&state)?;
                let runtime_type =
                    ty.exact_runtime_type()
                        .ok_or_else(|| RuntimeError::TaskRepresentation {
                            task: ty.display_name(),
                        })?;
                self.launch
                    .authority()
                    .validate_bound_type(callback.item(), &runtime_type, &task, &self.runtime)
                    .map_err(RuntimeError::Task)?;
                Ok(application::Startup::new(state, Some(task)))
            }
        }
    }

    /// Invoke update and normalize its unit or durable Task result.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke_update(&self, args: &mut [Dynamic]) -> Result<Option<Dynamic>, program::Error> {
        let callback = self.update().callback();
        let output = self.invoke(callback.item(), args)?;
        match self.update().task() {
            application::Task::None => {
                let runtime_type = callback
                    .output()
                    .exact_runtime_type()
                    .expect("validated unit update output has an exact runtime type");
                self.launch.authority().validate_bound_type(
                    callback.item(),
                    &runtime_type,
                    &output,
                    &self.runtime,
                )?;
                Ok(None)
            }
            application::Task::Facade(ty) => {
                let runtime_type = ty
                    .exact_runtime_type()
                    .expect("validated facade Task output has an exact runtime type");
                self.launch.authority().validate_bound_type(
                    callback.item(),
                    &runtime_type,
                    &output,
                    &self.runtime,
                )?;
                Ok(Some(output))
            }
        }
    }

    /// Materialize the exact current State for a Default-backed application.
    pub fn boot_default(&self) -> Result<Dynamic, RuntimeError> {
        if !matches!(self.boot(), application::Boot::Default) {
            return Err(RuntimeError::ExplicitBoot);
        }
        let value = self
            .launch
            .authority()
            .materialize_default(self.state())
            .map_err(RuntimeError::StateConstruction)?;
        self.validate_state(&value)?;
        Ok(value)
    }

    /// Validate one runtime value against the exact current State proof.
    pub fn validate_state(&self, value: &Dynamic) -> Result<(), RuntimeError> {
        self.launch
            .authority()
            .validate_application_value(value, self.state(), &self.runtime)
            .map_err(RuntimeError::State)
    }

    /// The State `value` as bytes a new runner process can seed from;
    /// `None` when it holds anything but plain script data.
    pub fn export_state(&self, value: &Dynamic) -> Option<Vec<u8>> {
        self.validate_state(value).ok()?;
        crate::program::model::carry::encode(value)
    }

    /// The State exported `bytes` carry, rebuilt in this program; `None` when
    /// a script type in it changed or the result is not this program's State.
    pub fn import_state(&self, bytes: &[u8]) -> Option<Dynamic> {
        let value = self.launch.authority().decode_carried(bytes)?;
        self.validate_state(&value).ok()?;
        Some(value)
    }

    /// Validate one delivered value against the exact current Message proof.
    pub fn validate_message(&self, value: &Dynamic) -> Result<(), RuntimeError> {
        self.launch
            .authority()
            .validate_application_value(value, self.message(), &self.runtime)
            .map_err(RuntimeError::Message)
    }

    /// Issue one compiler-proven host operation under this exact application authority.
    #[cfg(not(feature = "no_function"))]
    pub fn issue_operation(
        &self,
        call: host::operation::Call,
        mapper: &item::Value,
    ) -> Result<host::operation::Pending, host::operation::Error> {
        let request = host::operation::Request::new(call, mapper);
        self.launch
            .authority()
            .issue_operation(request, self.runtime.clone())
    }

    /// Validate one value against an exact argument carried by a runtime witness.
    #[cfg(not(feature = "no_function"))]
    pub fn validate_witness_argument(
        &self,
        witness: &host::runtime::Witness,
        index: usize,
        value: &Dynamic,
    ) -> Result<(), host::runtime::ValueError> {
        self.launch
            .authority()
            .validate_witness_argument(witness, index, value, &self.runtime)
    }

    /// Bind a durable symbolic subscription plan under this exact application authority.
    ///
    /// Every recipe receives stable identity and complete input/mapper proofs,
    /// but no native stream factory starts until that recipe is consumed.
    #[cfg(not(feature = "no_function"))]
    pub fn bind_stream_plan(
        &self,
        plan: host::stream::Plan,
    ) -> Result<Box<[host::stream::Recipe]>, host::stream::Error> {
        let message = self.message().exact_runtime_type().ok_or_else(|| {
            host::stream::Error::MessageContract {
                message: self.message().display_name().into(),
            }
        })?;
        plan.into_streams()
            .into_vec()
            .into_iter()
            .map(|stream| {
                self.launch
                    .authority()
                    .bind_stream(stream.into_request(message.clone()), self.runtime.clone())
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Vec::into_boxed_slice)
    }

    /// Map and validate one payload under this exact application authority.
    #[cfg(not(feature = "no_function"))]
    pub fn map_message(
        &self,
        mapper: &item::Value,
        payload: Dynamic,
    ) -> Result<Dynamic, program::Error> {
        let mut arguments = [payload];
        let value = self.invoke(mapper, &mut arguments)?;
        let runtime_type =
            self.message()
                .exact_runtime_type()
                .ok_or_else(|| program::Error::Evaluation {
                    key: mapper.key().clone(),
                    error: crate::EvalAltResult::ErrorRuntime(
                        format!(
                            "application Message contract `{}` has no exact runtime representation",
                            self.message().display_name()
                        )
                        .into(),
                        crate::Position::NONE,
                    )
                    .into(),
                })?;
        self.launch.authority().validate_bound_type(
            mapper,
            &runtime_type,
            &value,
            &self.runtime,
        )?;
        Ok(value)
    }

    /// Promote owned nominal data through one current scripted-widget site proof.
    pub fn promote_widget(
        &self,
        promotion: &crate::widget::Promotion,
        data: Dynamic,
    ) -> Result<crate::widget::Descriptor, crate::widget::Error> {
        self.launch
            .authority()
            .promote_widget(promotion, data, self.message(), &self.runtime)
    }

    /// Bind one durable scripted widget to this exact application launch.
    pub fn bind_widget(
        &self,
        descriptor: &crate::widget::Descriptor,
    ) -> Result<crate::widget::Binding, crate::widget::Error> {
        self.launch
            .authority()
            .bind_widget(descriptor, self.runtime.clone())
    }
}

impl fmt::Debug for BoundLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundLaunch")
            .field("fingerprint", self.fingerprint())
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefaultBoot => {
                formatter.write_str("explicit boot requested for a Default-backed application")
            }
            Self::ExplicitBoot => {
                formatter.write_str("default boot requested for an explicit-boot application")
            }
            Self::StateConstruction(error) => error.fmt(formatter),
            Self::State(error) | Self::Message(error) => error.fmt(formatter),
            Self::Invocation(error) | Self::Task(error) => error.fmt(formatter),
            Self::Startup(error) => error.fmt(formatter),
            Self::TaskRepresentation { task } => {
                write!(
                    formatter,
                    "application Task contract `{task}` has no exact runtime representation"
                )
            }
        }
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DefaultBoot => None,
            Self::ExplicitBoot => None,
            Self::StateConstruction(error) => Some(error),
            Self::State(error) | Self::Message(error) => Some(error),
            Self::Invocation(error) | Self::Task(error) => Some(error),
            Self::Startup(error) => Some(error),
            Self::TaskRepresentation { .. } => None,
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn core_error::Error + 'static)> {
        match self {
            Self::DefaultBoot => None,
            Self::ExplicitBoot => None,
            Self::StateConstruction(error) => Some(error),
            Self::State(error) | Self::Message(error) => Some(error),
            Self::Invocation(error) | Self::Task(error) => Some(error),
            Self::Startup(error) => Some(error),
            Self::TaskRepresentation { .. } => None,
        }
    }
}
