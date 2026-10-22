//! Owned attenuated authority issued only at final host admission.

use std::sync::Arc;

use crate::{application, host, item, preview, widget, Dynamic};

use super::Binding;

/// One final-admission capability over the still-sealed candidate binding.
#[doc(hidden)]
pub struct Context<'a> {
    binding: &'a Binding,
}

/// Owned attenuated catalog authority for one admitted host.
#[derive(Clone)]
#[doc(hidden)]
pub struct Catalog(Arc<preview::BoundCatalog>);

/// Owned attenuated application authority for one admitted host.
#[derive(Clone)]
#[doc(hidden)]
pub struct Application(application::BoundLaunch);

impl<'a> Context<'a> {
    pub(super) const fn new(binding: &'a Binding) -> Self {
        Self { binding }
    }

    /// Attach the admitted candidate catalog to its complete host payload.
    #[must_use]
    pub fn catalog(&self) -> Catalog {
        Catalog(Arc::clone(&self.binding.0))
    }

    /// Attach the admitted candidate application to its complete host payload.
    #[must_use]
    pub fn application(&self) -> Option<Application> {
        self.binding.0.application().cloned().map(Application)
    }

    /// Attach one inert prepared operation to the admitted runtime task.
    #[must_use = "the prepared operation must be retained or executed"]
    pub fn operation(
        &self,
        prepared: super::operation::Prepared,
    ) -> crate::host::operation::Pending {
        prepared.into_inner()
    }
}

impl Catalog {
    /// Rebind one generated profile event-mapper seal to this admitted catalog.
    #[doc(hidden)]
    #[must_use]
    pub fn profile_event_mapper(&self, seal: &[u8; 16]) -> Option<item::Value> {
        self.0.profile_event_mapper(seal)
    }

    /// Attenuate one already-bound static catalog.
    #[doc(hidden)]
    #[must_use]
    pub fn from_bound(catalog: Arc<preview::BoundCatalog>) -> Self {
        Self(catalog)
    }

    /// Borrow this admitted catalog without cloning its authority.
    #[must_use]
    pub fn borrow(&self) -> super::Catalog<'_> {
        super::Catalog(&self.0)
    }

    /// Clone the admitted application authority for live host ownership.
    #[must_use]
    pub fn application(&self) -> Option<Application> {
        self.0.application().cloned().map(Application)
    }

    /// Return compiler-validated previews in stable key order.
    #[must_use]
    pub fn previews(&self) -> &[preview::Definition] {
        self.0.previews()
    }

    /// Return the compiler-selected root package name.
    #[must_use]
    pub fn package(&self) -> &str {
        self.0.package()
    }

    /// Return every compiler-selected nominal-widget promotion site.
    #[must_use]
    pub fn widget_promotions(&self) -> &[crate::widget::Promotion] {
        self.0.widget_promotions()
    }

    /// Invoke one preview update under admitted catalog authority.
    pub fn update(
        &self,
        key: &preview::Key,
        state: &Dynamic,
        message: Dynamic,
    ) -> Result<preview::Update, preview::RuntimeError> {
        self.0.update(key, state, message)
    }

    /// Map one scripted widget payload to the selected preview Message.
    pub fn map_message(
        &self,
        key: &preview::Key,
        mapper: &item::Value,
        payload: Dynamic,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.map_message(key, mapper, payload)
    }

    /// Issue one compiler-proven host operation under admitted authority.
    pub fn issue_operation(
        &self,
        key: &preview::Key,
        call: host::operation::Call,
        mapper: &item::Value,
    ) -> Result<host::operation::Pending, host::operation::Error> {
        self.0.issue_operation(key, call, mapper)
    }

    /// Map one task payload to the selected preview Message.
    pub fn map_task_value(
        &self,
        key: &preview::Key,
        mapper: &item::Value,
        payload: Dynamic,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.map_task_value(key, mapper, payload)
    }

    /// Invoke one compiler-proven function with its arguments in order.
    pub fn invoke_function(
        &self,
        key: &preview::Key,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.invoke_function(key, function, arguments)
    }

    /// Validate one task value against an exact runtime witness argument.
    pub fn validate_witness_argument(
        &self,
        witness: &host::runtime::Witness,
        index: usize,
        value: &Dynamic,
    ) -> Result<(), host::runtime::ValueError> {
        self.0.validate_witness_argument(witness, index, value)
    }

    /// Validate one exact selected preview Message value.
    pub fn validate_message(
        &self,
        key: &preview::Key,
        value: &Dynamic,
    ) -> Result<(), preview::RuntimeError> {
        self.0.validate_message(key, value)
    }

    /// Promote owned nominal data through one exact preview widget site.
    pub fn promote_widget(
        &self,
        key: &preview::Key,
        promotion: &widget::Promotion,
        data: Dynamic,
    ) -> Result<widget::Descriptor, widget::Error> {
        self.0.promote_widget(key, promotion, data)
    }

    /// Bind one durable scripted widget to admitted preview authority.
    pub fn bind_widget(
        &self,
        key: &preview::Key,
        descriptor: &widget::Descriptor,
    ) -> Result<widget::Binding, widget::Error> {
        self.0.bind_widget(key, descriptor)
    }
}

impl Application {
    /// Rebind one generated profile event-mapper seal to this admitted application.
    #[doc(hidden)]
    #[must_use]
    pub fn profile_event_mapper(&self, seal: &[u8; 16]) -> Option<item::Value> {
        self.0.profile_event_mapper(seal)
    }

    /// Attenuate one already-bound static application launch.
    #[doc(hidden)]
    #[must_use]
    pub fn from_bound(launch: application::BoundLaunch) -> Self {
        Self(launch)
    }

    /// Borrow this admitted application without cloning its authority.
    #[must_use]
    pub fn borrow(&self) -> super::Application<'_> {
        super::Application(&self.0)
    }

    /// Return the normalized boot contract.
    #[must_use]
    pub fn boot(&self) -> &application::Boot {
        self.0.boot()
    }

    /// Return the normalized update callback.
    #[must_use]
    pub fn update(&self) -> &application::Update {
        self.0.update()
    }

    /// Return the normalized view callback.
    #[must_use]
    pub fn view(&self) -> &application::View {
        self.0.view()
    }

    /// Return the optional state-derived title callback.
    #[must_use]
    pub fn title(&self) -> Option<&application::Title> {
        self.0.title()
    }

    /// Return the optional state-derived theme callback.
    #[must_use]
    pub fn theme(&self) -> Option<&application::Theme> {
        self.0.theme()
    }

    /// Return the optional state-derived subscription callback.
    #[must_use]
    pub fn subscription(&self) -> Option<&application::Subscription> {
        self.0.subscription()
    }

    /// Return whether the application requests centered initial placement.
    #[must_use]
    pub fn centered(&self) -> bool {
        self.0.centered()
    }

    /// Return the immutable executable generation identifier.
    #[must_use]
    pub fn generation(&self) -> crate::program::GenerationId {
        self.0.generation()
    }

    /// Resolve exact retained declaration evidence for one callback.
    #[must_use]
    pub fn callback_evidence(
        &self,
        callback: &application::Callback,
    ) -> Option<application::callback::Evidence<'_>> {
        self.0.callback_evidence(callback)
    }

    /// Materialize the default application State.
    pub fn boot_default(&self) -> Result<crate::Dynamic, application::RuntimeError> {
        self.0.boot_default()
    }

    /// Invoke and validate the explicit application boot callback.
    pub fn invoke_boot(&self) -> Result<application::Startup, application::RuntimeError> {
        self.0.invoke_boot()
    }

    /// The crate's free function `name`, as a symbolic item.
    #[must_use]
    pub fn function(&self, name: &str) -> Option<crate::item::Value> {
        self.0.function(name)
    }

    /// Invoke one exact application symbolic item.
    pub fn invoke(
        &self,
        retained: &crate::item::Value,
        arguments: &mut [crate::Dynamic],
    ) -> Result<crate::Dynamic, crate::program::Error> {
        self.0.invoke(retained, arguments)
    }

    /// Invoke and normalize the application view callback.
    pub fn invoke_view(
        &self,
        arguments: &mut [crate::Dynamic],
    ) -> Result<crate::Dynamic, crate::program::Error> {
        self.0.invoke_view(arguments)
    }

    /// Invoke and normalize the optional application title callback.
    pub fn invoke_title(
        &self,
        arguments: &mut [crate::Dynamic],
    ) -> Result<Option<crate::Dynamic>, crate::program::Error> {
        self.0.invoke_title(arguments)
    }

    /// Invoke and normalize the optional application theme callback.
    pub fn invoke_theme(
        &self,
        arguments: &mut [crate::Dynamic],
    ) -> Result<Option<crate::Dynamic>, crate::program::Error> {
        self.0.invoke_theme(arguments)
    }

    /// Invoke and normalize the optional application subscription callback.
    pub fn invoke_subscription(
        &self,
        arguments: &mut [crate::Dynamic],
    ) -> Result<Option<crate::Dynamic>, crate::program::Error> {
        self.0.invoke_subscription(arguments)
    }

    /// Invoke and normalize one application update callback.
    pub fn invoke_update(
        &self,
        arguments: &mut [crate::Dynamic],
    ) -> Result<Option<crate::Dynamic>, crate::program::Error> {
        self.0.invoke_update(arguments)
    }

    /// Validate one exact application State value.
    pub fn validate_state(&self, value: &crate::Dynamic) -> Result<(), application::RuntimeError> {
        self.0.validate_state(value)
    }

    /// The State `value` as bytes a new runner process can seed from.
    pub fn export_state(&self, value: &crate::Dynamic) -> Option<Vec<u8>> {
        self.0.export_state(value)
    }

    /// The State exported `bytes` carry, rebuilt in this program, when every
    /// script type in it is unchanged.
    pub fn import_state(&self, bytes: &[u8]) -> Option<crate::Dynamic> {
        self.0.import_state(bytes)
    }

    /// Validate one exact application Message value.
    pub fn validate_message(
        &self,
        value: &crate::Dynamic,
    ) -> Result<(), application::RuntimeError> {
        self.0.validate_message(value)
    }

    /// Issue one compiler-proven host operation.
    pub fn issue_operation(
        &self,
        call: crate::host::operation::Call,
        mapper: &crate::item::Value,
    ) -> Result<crate::host::operation::Pending, crate::host::operation::Error> {
        self.0.issue_operation(call, mapper)
    }

    /// Validate one task value against an exact runtime witness argument.
    pub fn validate_witness_argument(
        &self,
        witness: &crate::host::runtime::Witness,
        index: usize,
        value: &crate::Dynamic,
    ) -> Result<(), crate::host::runtime::ValueError> {
        self.0.validate_witness_argument(witness, index, value)
    }

    /// Bind one durable symbolic subscription plan.
    pub fn bind_stream_plan(
        &self,
        plan: crate::host::stream::Plan,
    ) -> Result<Box<[crate::host::stream::Recipe]>, crate::host::stream::Error> {
        self.0.bind_stream_plan(plan)
    }

    /// Map and validate one application Message payload.
    pub fn map_message(
        &self,
        mapper: &crate::item::Value,
        payload: crate::Dynamic,
    ) -> Result<crate::Dynamic, crate::program::Error> {
        self.0.map_message(mapper, payload)
    }

    /// Return every compiler-selected widget promotion site.
    pub fn widget_promotions(&self) -> &[crate::widget::Promotion] {
        self.0.widget_promotions()
    }

    /// Promote owned nominal data through one exact application widget site.
    pub fn promote_widget(
        &self,
        promotion: &crate::widget::Promotion,
        data: crate::Dynamic,
    ) -> Result<crate::widget::Descriptor, crate::widget::Error> {
        self.0.promote_widget(promotion, data)
    }

    /// Bind one durable scripted widget to this application authority.
    pub fn bind_widget(
        &self,
        descriptor: &crate::widget::Descriptor,
    ) -> Result<crate::widget::Binding, crate::widget::Error> {
        self.0.bind_widget(descriptor)
    }
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Context(<private>)")
    }
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Catalog(<private>)")
    }
}

impl std::fmt::Debug for Application {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Application(<private>)")
    }
}
