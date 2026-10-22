//! Capability-attenuated native preview authority retained by one host.

use std::sync::Arc;

use rawr::{host, item, preview, program, widget, Dynamic};

/// One native-bound catalog retained by a static or reloadable preview host.
#[derive(Clone)]
pub(crate) struct Authority(rawr::reload::installation::installed::Catalog);

pub(crate) trait Evaluation {
    fn has_application(&self) -> bool;
    fn previews(&self) -> &[preview::Definition];
    fn generation(&self) -> program::GenerationId;
    fn seed(&self, key: &preview::Key) -> Result<Option<Dynamic>, preview::RuntimeError>;
    fn subject(
        &self,
        key: &preview::Key,
        fixture: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError>;
    fn theme(
        &self,
        key: &preview::Key,
        index: usize,
        state: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError>;
    fn source(
        &self,
        location: preview::Location,
    ) -> Result<rawr::diagnostic::Source, preview::RuntimeError>;
}

impl Authority {
    pub(crate) fn static_(catalog: Arc<preview::BoundCatalog>) -> Self {
        Self(rawr::reload::installation::installed::Catalog::from_bound(
            catalog,
        ))
    }

    pub(crate) const fn reload(catalog: rawr::reload::installation::installed::Catalog) -> Self {
        Self(catalog)
    }

    pub(crate) fn application(&self) -> Option<rawr::reload::installation::installed::Application> {
        self.0.application()
    }

    pub(crate) fn previews(&self) -> &[preview::Definition] {
        self.0.previews()
    }

    pub(crate) fn generation(&self) -> program::GenerationId {
        self.0.borrow().generation()
    }

    pub(crate) fn package(&self) -> &str {
        self.0.package()
    }

    pub(crate) fn seed(
        &self,
        key: &preview::Key,
    ) -> Result<Option<Dynamic>, preview::RuntimeError> {
        self.0.borrow().seed(key)
    }

    pub(crate) fn subject(
        &self,
        key: &preview::Key,
        fixture: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.borrow().subject(key, fixture)
    }

    pub(crate) fn update(
        &self,
        key: &preview::Key,
        state: &Dynamic,
        message: Dynamic,
    ) -> Result<preview::Update, preview::RuntimeError> {
        self.0.update(key, state, message)
    }

    pub(crate) fn theme(
        &self,
        key: &preview::Key,
        index: usize,
        state: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.borrow().theme(key, index, state)
    }

    pub(crate) fn map_message(
        &self,
        key: &preview::Key,
        mapper: &item::Value,
        payload: Dynamic,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.map_message(key, mapper, payload)
    }

    pub(crate) fn profile_event_mapper(&self, seal: &[u8; 16]) -> Option<item::Value> {
        self.0.profile_event_mapper(seal)
    }

    pub(crate) fn issue_operation(
        &self,
        key: &preview::Key,
        call: host::operation::Call,
        mapper: &item::Value,
    ) -> Result<host::operation::Pending, host::operation::Error> {
        self.0.issue_operation(key, call, mapper)
    }

    pub(crate) fn map_task_value(
        &self,
        key: &preview::Key,
        mapper: &item::Value,
        payload: Dynamic,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.map_task_value(key, mapper, payload)
    }

    pub(crate) fn invoke_function(
        &self,
        key: &preview::Key,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.0.invoke_function(key, function, arguments)
    }

    pub(crate) fn validate_witness_argument(
        &self,
        witness: &host::runtime::Witness,
        index: usize,
        value: &Dynamic,
    ) -> Result<(), host::runtime::ValueError> {
        self.0.validate_witness_argument(witness, index, value)
    }

    pub(crate) fn validate_message(
        &self,
        key: &preview::Key,
        value: &Dynamic,
    ) -> Result<(), preview::RuntimeError> {
        self.0.validate_message(key, value)
    }

    pub(crate) fn source(
        &self,
        location: preview::Location,
    ) -> Result<rawr::diagnostic::Source, preview::RuntimeError> {
        self.0.borrow().source(location)
    }

    pub(crate) fn promote_widget(
        &self,
        key: &preview::Key,
        promotion: &widget::Promotion,
        data: Dynamic,
    ) -> Result<widget::Descriptor, widget::Error> {
        self.0.promote_widget(key, promotion, data)
    }

    #[cfg(test)]
    pub(crate) fn widget_promotions(&self) -> &[widget::Promotion] {
        self.0.widget_promotions()
    }

    pub(crate) fn bind_widget(
        &self,
        key: &preview::Key,
        descriptor: &widget::Descriptor,
    ) -> Result<widget::Binding, widget::Error> {
        self.0.bind_widget(key, descriptor)
    }
}

impl Evaluation for Authority {
    fn has_application(&self) -> bool {
        self.application().is_some()
    }

    fn previews(&self) -> &[preview::Definition] {
        self.previews()
    }

    fn generation(&self) -> program::GenerationId {
        self.generation()
    }

    fn seed(&self, key: &preview::Key) -> Result<Option<Dynamic>, preview::RuntimeError> {
        self.seed(key)
    }

    fn subject(
        &self,
        key: &preview::Key,
        fixture: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.subject(key, fixture)
    }

    fn theme(
        &self,
        key: &preview::Key,
        index: usize,
        state: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.theme(key, index, state)
    }

    fn source(
        &self,
        location: preview::Location,
    ) -> Result<rawr::diagnostic::Source, preview::RuntimeError> {
        self.source(location)
    }
}

impl Evaluation for rawr::reload::installation::Catalog<'_> {
    fn has_application(&self) -> bool {
        self.application().is_some()
    }

    fn previews(&self) -> &[preview::Definition] {
        self.previews()
    }

    fn generation(&self) -> program::GenerationId {
        self.generation()
    }

    fn seed(&self, key: &preview::Key) -> Result<Option<Dynamic>, preview::RuntimeError> {
        self.seed(key)
    }

    fn subject(
        &self,
        key: &preview::Key,
        fixture: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.subject(key, fixture)
    }

    fn theme(
        &self,
        key: &preview::Key,
        index: usize,
        state: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.theme(key, index, state)
    }

    fn source(
        &self,
        location: preview::Location,
    ) -> Result<rawr::diagnostic::Source, preview::RuntimeError> {
        self.source(location)
    }
}
